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

pub type BotThread = JoinHandle<BotResult<()>>;

pub struct BotRunner {
    controller: BotController,
    info: BotInfo,
    notificator: NotificatorThread,
    thread: Option<BotThread>,
}

#[derive(Debug)]
pub enum NotificatorThread {
    Running(Option<BotThread>),
    Done,
}

#[derive(Clone)]
pub struct BotInfo {
    pub name: String,
}

pub static DEFAULT_SCRIPT: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/default_script.js"));

pub struct BotManager<BIG, BHG, BII, BHI>
where
    BIG: AsyncFnMut() -> BII,            // BotInstance Getter
    BII: Iterator<Item = BotInstance>,   // BotInstance Iterator
    BHG: AsyncFnMut(BotInstance) -> BHI, // BotHandler  Getter
    BHI: Iterator<Item = BotHandler>,    // BotHandler  Iterator
{
    bot_pool: HashMap<String, BotRunner>,
    bi_getter: BIG,
    h_mapper: BHG,
}

impl<BIG, BHG, BII, BHI> BotManager<BIG, BHG, BII, BHI>
where
    BIG: AsyncFnMut() -> BII,            // BotInstance Getter
    BII: Iterator<Item = BotInstance>,   // BotInstance Iterator
    BHG: AsyncFnMut(BotInstance) -> BHI, // BotHandler  Getter
    BHI: Iterator<Item = BotHandler>,    // BotHandler  Iterator
{
    /// bi_getter - async fnmut that returns iterator over BotInstance
    /// h_map     - async fnmut that returns iterator over handlers by BotInstance
    pub fn with(bi_getter: BIG, h_mapper: BHG) -> Self {
        Self {
            bot_pool: Default::default(),
            bi_getter,
            h_mapper,
        }
    }

    pub async fn dispatch(mut self, db: &mut DB) -> BotResult<()> {
        loop {
            for bi in (self.bi_getter)().await {
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
                        info!("NEW INSTANCE: Starting new instance! bot name: {}", bi.name);
                        self.create_bot_runner(&bi, db).await?
                    }
                };

                bot_runner.thread = clear_finished_thread(bot_runner.thread, &bi);

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

                bot_runner.notificator = check_notificator_done(bot_runner.notificator);

                bot_runner.notificator = match bot_runner.notificator {
                    NotificatorThread::Done => NotificatorThread::Done,
                    NotificatorThread::Running(thread) => {
                        NotificatorThread::Running(match thread {
                            Some(thread) => Some(thread),
                            None => {
                                let thread =
                                    spawn_notificator_thread(bot_runner.controller.clone()).await?;
                                Some(thread)
                            }
                        })
                    }
                };

                self.bot_pool.insert(bi.name.clone(), bot_runner);
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    pub async fn create_bot_runner(
        &mut self,
        bi: &BotInstance,
        db: &mut DB,
    ) -> BotResult<BotRunner> {
        let db = db.clone().with_name(bi.name.clone());
        let controller = BotController::with_db(db.clone(), &bi.token, &bi.script).await?;

        let info = BotInfo {
            name: bi.name.clone(),
        };
        let runner = BotRunner {
            controller,
            info,
            notificator: NotificatorThread::Running(None),
            thread: None,
        };

        Ok(runner)
    }
}

/// checking if thread is not finished, otherwise clearing handler
fn clear_finished_thread(thread: Option<BotThread>, bi: &BotInstance) -> Option<BotThread> {
    thread.and_then(|thread| match thread.is_finished() {
        false => Some(thread),
        // if finished, join it (should return immidiatly), and print cause of stop
        true => {
            let err = thread.join();
            error!("Thread bot `{}` finished with error: {:?}", bi.name, err);
            None
        }
    })
}

// sets NotificatorThread to Done if running thread returned Ok(...)
fn check_notificator_done(n: NotificatorThread) -> NotificatorThread {
    match n {
        NotificatorThread::Running(Some(thread)) if thread.is_finished() => {
            match thread.join() {
                // if thread returns Ok(_), then do not run it again
                Ok(result) if result.is_ok() => NotificatorThread::Done,

                // but try to restart, if returned an error
                Ok(result) => {
                    error!("Notificator thread returned error: {result:?}");
                    NotificatorThread::Running(None)
                }
                Err(panicerr) => {
                    error!("Notificator thread paniced: {panicerr:?}");
                    NotificatorThread::Running(None)
                }
            }
        }
        other => other,
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

pub async fn spawn_bot_thread(bot: Bot, mut db: DB, handler: BotHandler) -> BotResult<BotThread> {
    let state_mgr = MongodbStorage::from_db(&mut db, Json)
        .await
        .map_err(DbError::from)?;
    let thread = std::thread::spawn(move || -> BotResult<()> {
        let state_mgr = state_mgr;

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

pub async fn spawn_notificator_thread(mut c: BotController) -> BotResult<BotThread> {
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
