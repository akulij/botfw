use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::Duration,
};

use log::{error, info};
use teloxide::{dispatching::dialogue::serializer::Json, dptree, prelude::Dispatcher, Bot};

use crate::{
    bot_handler::{script_handler, BotHandler},
    db::{bots::BotInstance, DbError, DB},
    message_answerer::MessageAnswerer,
    mongodb_storage::MongodbStorage,
    BotController, BotResult, BotRuntime,
};

pub struct BotRunner {
    controller: BotController,
    info: BotInfo,
    notificator: NotificatorThread,
    thread: Option<JoinHandle<BotResult<()>>>,
}

pub enum NotificatorThread {
    Running(Option<JoinHandle<BotResult<()>>>),
    Done,
}

#[derive(Clone)]
pub struct BotInfo {
    pub name: String,
}

pub static DEFAULT_SCRIPT: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/default_script.js"));

pub struct BotManager<BIG, HM, BIS, HI, FBIS, FHI>
where
    BIG: FnMut() -> FBIS,
    FBIS: Future<Output = BIS>,
    BIS: Iterator<Item = BotInstance>,
    HM: FnMut(BotInstance) -> FHI,
    FHI: Future<Output = HI>,
    HI: Iterator<Item = BotHandler>,
{
    bot_pool: HashMap<String, BotRunner>,
    bi_getter: BIG,
    h_mapper: HM,
}

impl<BIG, HM, BIS, HI, FBIS, FHI> BotManager<BIG, HM, BIS, HI, FBIS, FHI>
where
    BIG: FnMut() -> FBIS,
    FBIS: Future<Output = BIS>,
    BIS: Iterator<Item = BotInstance>,
    HM: FnMut(BotInstance) -> FHI,
    FHI: Future<Output = HI>,
    HI: Iterator<Item = BotHandler>,
{
    /// bi_getter - fnmut that returns iterator over BotInstance
    /// h_map     - fnmut that returns iterator over handlers by BotInstance
    pub fn with(bi_getter: BIG, h_mapper: HM) -> Self {
        Self {
            bot_pool: Default::default(),
            bi_getter,
            h_mapper,
        }
    }

    pub async fn dispatch(mut self, db: &mut DB) -> BotResult<()> {
        loop {
            'biter: for bi in (self.bi_getter)().await {
                // removing handler to force restart
                // TODO: wait till all updates are processed in bot
                // Temporarly disabling code, because it's free of js runtime
                // spreads panic
                if bi.restart_flag {
                    info!(
                        "Trying to restart bot `{}`, new script: {}",
                        bi.name, bi.script
                    );
                    let _runner = self.bot_pool.remove(&bi.name);
                };
                // start, if not started
                let mut bot_runner = match self.bot_pool.remove(&bi.name) {
                    Some(br) => br,
                    None => {
                        let handlers = (self.h_mapper)(bi.clone()).await;
                        info!("NEW INSTANCE: Starting new instance! bot name: {}", bi.name);
                        self.start_bot(bi, db, handlers.collect()).await?;
                        continue 'biter;
                    }
                };

                // checking if thread is not finished, otherwise clearing handler
                bot_runner.thread = match bot_runner.thread {
                    Some(thread) => {
                        if thread.is_finished() {
                            let err = thread.join();
                            error!("Thread bot `{}` finished with error: {:?}", bi.name, err);
                            None
                        } else {
                            Some(thread)
                        }
                    }
                    None => None,
                };

                // checking if thread is running, otherwise start thread
                bot_runner.thread = match bot_runner.thread {
                    Some(thread) => Some(thread),
                    None => {
                        let handlers = (self.h_mapper)(bi.clone()).await;
                        let handler = script_handler_gen(
                            bot_runner.controller.runtime.clone(),
                            handlers.collect(),
                        )
                        .await;
                        Some(
                            spawn_bot_thread(
                                bot_runner.controller.bot.clone(),
                                bot_runner.controller.db.clone(),
                                handler,
                            )
                            .await?,
                        )
                    }
                };
                self.bot_pool.insert(bi.name.clone(), bot_runner);
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    pub async fn start_bot(
        &mut self,
        bi: BotInstance,
        db: &mut DB,
        plug_handlers: Vec<BotHandler>,
    ) -> BotResult<BotInfo> {
        let db = db.clone().with_name(bi.name.clone());
        let controller = BotController::with_db(db.clone(), &bi.token, &bi.script).await?;

        let handler = script_handler_gen(controller.runtime.clone(), plug_handlers).await;

        let thread =
            spawn_bot_thread(controller.bot.clone(), controller.db.clone(), handler).await?;
        let notificator = spawn_notificator_thread(controller.clone()).await?;
        let notificator = NotificatorThread::Running(Some(notificator));

        let info = BotInfo {
            name: bi.name.clone(),
        };
        let runner = BotRunner {
            controller,
            info: info.clone(),
            notificator,
            thread: Some(thread),
        };

        self.bot_pool.insert(bi.name.clone(), runner);

        Ok(info)
    }
}

async fn script_handler_gen(
    r: Arc<Mutex<BotRuntime>>,
    plug_handlers: Vec<BotHandler>,
) -> BotHandler {
    let handler = script_handler(r.clone());
    // each handler will be added to dptree::entry()
    let handler = plug_handlers
        .into_iter()
        // as well as the script handler at the end
        .chain(std::iter::once(handler))
        .fold(dptree::entry(), |h, plug| h.branch(plug));
    handler
}

pub async fn spawn_bot_thread(
    bot: Bot,
    mut db: DB,
    handler: BotHandler,
) -> BotResult<JoinHandle<BotResult<()>>> {
    let state_mgr = MongodbStorage::from_db(&mut db, Json)
        .await
        .map_err(DbError::from)?;
    let thread = std::thread::spawn(move || -> BotResult<()> {
        let state_mgr = state_mgr;

        // let rt = tokio::runtime::Builder::new_current_thread()
        //     .enable_all()
        //     .build()?;
        let rt = tokio::runtime::Runtime::new()?;

        rt.block_on(
            Dispatcher::builder(bot, handler)
                .dependencies(dptree::deps![db, state_mgr])
                .build()
                .dispatch(),
        );

        Ok(())
    });

    Ok(thread)
}

pub async fn spawn_notificator_thread(
    mut c: BotController,
) -> BotResult<JoinHandle<BotResult<()>>> {
    let thread = std::thread::spawn(move || -> BotResult<()> {
        let rt = tokio::runtime::Runtime::new()?;

        rt.block_on(async {
            loop {
                let notifications = {
                    let r = c.runtime.lock().expect("Poisoned Runtime lock");
                    r.rc.get_nearest_notifications()
                };

                match notifications {
                    Some(n) => {
                        // waiting time to send notification
                        tokio::time::sleep(n.wait_for()).await;
                        'n: for n in n.notifications().iter() {
                            for user in n.get_users(&c.db).await?.into_iter() {
                                let text = match n.resolve_message(&c.db, &user).await? {
                                    Some(text) => text,
                                    None => continue 'n,
                                };

                                let ma = MessageAnswerer::new(&c.bot, &mut c.db, user.id);
                                ma.answer_text(text.clone(), None).await?;
                            }
                        }
                    }
                    None => break Ok(()),
                }
            }
        })
    });

    Ok(thread)
}
