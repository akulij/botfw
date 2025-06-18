pub mod admin;
pub mod bot_handler;
pub mod bot_manager;
pub mod botscript;
pub mod commands;
pub mod config;
pub mod db;
pub mod handlers;
pub mod message_answerer;
pub mod mongodb_storage;
pub mod runtimes;
pub mod utils;

use bot_manager::BotManager;
use botscript::application::attach_user_application;
use botscript::{Runner, ScriptError, ScriptResult};
use config::result::ConfigError;
use config::{Provider, RunnerConfig};
use db::application::Application;
use db::bots::BotInstance;
use db::callback_info::CallbackInfo;
use handlers::admin::admin_handler;
use log::{error, info};
use message_answerer::MessageAnswererError;
use std::sync::{Arc, Mutex};

use crate::db::{CallDB, DB};
use crate::mongodb_storage::MongodbStorage;

use db::DbError;
use envconfig::Envconfig;
use serde::{Deserialize, Serialize};
use teloxide::dispatching::dialogue::serializer::Json;
use teloxide::dispatching::dialogue::Serializer;
use teloxide::prelude::*;

type BotDialogue = Dialogue<State, MongodbStorage<Json>>;

#[derive(Envconfig, Clone)]
pub struct Config {
    #[envconfig(from = "BOT_TOKEN")]
    pub bot_token: String,
    #[envconfig(from = "DATABASE_URL")]
    pub db_url: String,
    #[envconfig(from = "ADMIN_PASS")]
    pub admin_password: String,
    #[envconfig(from = "ADMIN_ID")]
    pub admin_id: u64,
    #[envconfig(from = "BOT_NAME")]
    pub bot_name: String,
}

trait LogMsg {
    fn log(self) -> Self;
}

impl LogMsg for <teloxide::Bot as teloxide::prelude::Requester>::SendMessage {
    fn log(self) -> Self {
        info!("msg: {}", self.text);
        self
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub enum State {
    #[default]
    Start,
    Edit {
        literal: String,
        variant: Option<String>,
        lang: String,
        is_caption_set: bool,
    },
    EditButton,
    MessageForwardReply,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum Callback {
    MoreInfo,
    ProjectPage { id: u32 },
    GoHome,
    LeaveApplication,
    AskQuestion, // Add this line for the new callback
}

type CallbackStore = CallbackInfo<Callback>;

#[derive(Clone)]
pub struct BotController {
    pub bot: Bot,
    pub db: DB,
    pub runtime: Arc<Mutex<BotRuntime>>,
}

pub struct BotRuntime<P: Provider> {
    pub rc: RunnerConfig<P>,
    pub runner: Runner,
}
unsafe impl<P: Provider> Send for BotRuntime<P> {}

impl Drop for BotController {
    fn drop(&mut self) {
        info!("called drop for BotController");
    }
}

const MAIN_BOT_SCRIPT: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/mainbot.js"));

impl BotController {
    pub async fn new(config: &Config) -> ScriptResult<Self> {
        Self::create(
            &config.bot_token,
            &config.db_url,
            &config.bot_name,
            MAIN_BOT_SCRIPT,
        )
        .await
    }

    pub async fn create(token: &str, db_url: &str, name: &str, script: &str) -> ScriptResult<Self> {
        let db = DB::init(db_url, name.to_owned()).await?;

        Self::with_db(db, token, script).await
    }

    pub async fn with_db(mut db: DB, token: &str, script: &str) -> ScriptResult<Self> {
        let bot = Bot::new(token);

        let mut runner = Runner::init_with_db(&mut db)?;
        // runner.call_attacher(|c, o| attach_user_application(c, o, db.clone(), bot.clone()))??;
        let rc = runner.init_config(script)?;
        let runtime = Arc::new(Mutex::new(BotRuntime { rc, runner }));

        Ok(Self { bot, db, runtime })
    }
}

#[derive(thiserror::Error, Debug)]
pub enum BotError {
    DBError(#[from] DbError),
    TeloxideError(#[from] teloxide::RequestError),
    // TODO: not a really good to hardcode types, better to extend it later
    StorageError(#[from] mongodb_storage::MongodbStorageError<<Json as Serializer<State>>::Error>),
    MsgTooOld(String),
    BotLogicError(String),
    AdminMisconfiguration(String),
    ScriptError(#[from] ScriptError),
    IoError(#[from] std::io::Error),
    RwLockError(String),
    MAError(#[from] MessageAnswererError),
    ConfigError(#[from] ConfigError),
}

pub type BotResult<T> = Result<T, BotError>;

impl std::fmt::Display for BotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv()?;
    pretty_env_logger::init();
    let config = Config::init_from_env()?;

    let mut db = DB::init(&config.db_url, config.bot_name.to_owned()).await?;

    BotInstance::restart_all(&mut db, false).await?;
    // if we can't get info for main bot, we should stop anyway
    #[allow(clippy::unwrap_used)]
    let bm = BotManager::with(
        async || {
            let config = config.clone();

            let mut db = DB::init(config.db_url, config.bot_name.to_owned())
                .await
                .unwrap();
            let bi = BotInstance::new(
                config.bot_name,
                config.bot_token,
                MAIN_BOT_SCRIPT.to_string(),
            );
            let instances = BotInstance::get_all(&mut db).await.unwrap();
            BotInstance::restart_all(&mut db, false).await.unwrap();
            std::iter::once(bi).chain(instances)
        },
        async |_| vec![admin_handler()].into_iter(),
    );

    bm.dispatch(&mut db).await?;
    Ok(())
}

async fn send_application_to_chat(
    bot: &Bot,
    db: &mut DB,
    app: &Application<teloxide::types::User>,
) -> BotResult<Message> {
    let chat_id: i64 = match db.get_literal_value("support_chat_id").await? {
        Some(strcid) => match strcid.parse() {
            Ok(cid) => cid,
            Err(err) => {
                notify_admin(&format!(
                    "Support chat_id should be a number. Got: {strcid}, err: {err}.\n\
                Anyways, applied user: {:?}",
                    app.from
                ))
                .await;
                return Err(BotError::BotLogicError(format!("somewhere in bots logic support_chat_id literal not stored as a number, got: {strcid}")));
            }
        },
        None => {
            notify_admin(&format!(
                "support_chat_id is not set!!!\nAnyways, applied user: {:?}",
                app.from
            ))
            .await;
            return Err(BotError::AdminMisconfiguration(
                "admin forget to set support_chat_id".to_string(),
            ));
        }
    };
    let msg = match db.get_literal_value("application_format").await? {
        Some(msg) => msg
            .replace("{user_id}", app.from.id.0.to_string().as_str())
            .replace(
                "{username}",
                app.from
                    .username
                    .clone()
                    .unwrap_or("Username not set".to_string())
                    .as_str(),
            ),
        None => {
            notify_admin("format for support_chat_id is not set").await;
            return Err(BotError::AdminMisconfiguration(
                "admin forget to set application_format".to_string(),
            ));
        }
    };

    Ok(bot.send_message(ChatId(chat_id), msg).await?)
}

/// This is an emergent situation function, so it should not return any Result, but handle Results
/// on its own
async fn notify_admin(text: &str) {
    let config = match Config::init_from_env() {
        Ok(config) => config,
        Err(err) => {
            error!("notify_admin: Failed to get config from env, err: {err}");
            return;
        }
    };
    let bot = Bot::new(&config.bot_token);
    match bot.send_message(UserId(config.admin_id), text).await {
        Ok(_) => {}
        Err(err) => {
            error!("notify_admin: Failed to send message to admin, WHATS WRONG???, err: {err}");
        }
    }
}

fn update_user_tg(user: db::User, tguser: &teloxide::types::User) -> db::User {
    db::User {
        first_name: tguser.first_name.clone(),
        last_name: tguser.last_name.clone(),
        username: tguser.username.clone(),
        language_code: tguser.language_code.clone(),
        ..user
    }
}
