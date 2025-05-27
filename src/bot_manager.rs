use std::{collections::HashMap, sync::RwLock, thread::JoinHandle};

use lazy_static::lazy_static;
use teloxide::{
    dispatching::dialogue::serializer::Json,
    dptree,
    prelude::{Dispatcher, Requester},
    Bot,
};

use crate::{
    bot_handler::script_handler,
    db::{bots::BotInstance, DbError, DB},
    mongodb_storage::MongodbStorage,
    BotController, BotError, BotResult,
};

pub struct BotRunner {
    controller: BotController,
    info: BotInfo,
    thread: JoinHandle<BotResult<()>>,
}

unsafe impl Sync for BotRunner {}
unsafe impl Send for BotRunner {}

#[derive(Clone)]
pub struct BotInfo {
    pub name: String,
}

lazy_static! {
    static ref BOT_POOL: RwLock<HashMap<String, BotRunner>> = RwLock::new(HashMap::new());
}

static DEFAULT_SCRIPT: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/default_script.js"));

pub async fn create_bot(db: &mut DB, token: &str) -> BotResult<BotInstance> {
    let bot = Bot::new(token);
    let name = bot.get_me().await?.username().to_string();

    let bi = BotInstance::new(name.clone(), token.to_string(), DEFAULT_SCRIPT.to_string())
        .store(db)
        .await?;

    Ok(bi)
}

pub async fn start_bot(bi: BotInstance, db: &mut DB) -> BotResult<BotInfo> {
    let controller = BotController::with_db(db.clone(), &bi.token, &bi.script).await?;

    let thread = spawn_bot_thread(controller.clone(), db).await?;

    let info = BotInfo {
        name: bi.name.clone(),
    };
    let runner = BotRunner {
        controller,
        info: info.clone(),
        thread,
    };

    BOT_POOL
        .write()
        .map_or_else(
            |err| {
                Err(BotError::RwLockError(format!(
                    "Failed to lock BOT_POOL because previous thread paniced, err: {err}"
                )))
            },
            Ok,
        )?
        .insert(bi.name.clone(), runner);

    Ok(info)
}

pub async fn spawn_bot_thread(
    bc: BotController,
    db: &mut DB,
) -> BotResult<JoinHandle<BotResult<()>>> {
    let state_mgr = MongodbStorage::from_db(db, Json)
        .await
        .map_err(DbError::from)?;
    let thread = std::thread::spawn(move || -> BotResult<()> {
        let state_mgr = state_mgr;

        let handler = script_handler(bc.rc);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        rt.block_on(
            Dispatcher::builder(bc.bot, handler)
                .dependencies(dptree::deps![bc.db, state_mgr])
                .build()
                .dispatch(),
        );

        Ok(())
    });

    Ok(thread)
}
