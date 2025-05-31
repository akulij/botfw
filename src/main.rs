pub mod admin;
pub mod bot_handler;
pub mod bot_manager;
pub mod botscript;
pub mod commands;
pub mod db;
pub mod handlers;
pub mod message_answerer;
pub mod mongodb_storage;
pub mod utils;

use bot_manager::start_bot;
use botscript::{BotMessage, Runner, RunnerConfig, ScriptError, ScriptResult};
use commands::BotCommand;
use db::application::Application;
use db::bots::BotInstance;
use db::callback_info::CallbackInfo;
use db::message_forward::MessageForward;
use handlers::admin::admin_handler;
use itertools::Itertools;
use log::{error, info, warn};
use message_answerer::MessageAnswerer;
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use teloxide::sugar::request::RequestReplyExt;
use utils::create_callback_button;

use crate::admin::{admin_command_handler, AdminCommands};
use crate::admin::{secret_command_handler, SecretCommands};
use crate::db::{CallDB, DB};
use crate::mongodb_storage::MongodbStorage;

use chrono::{DateTime, Utc};
use db::DbError;
use envconfig::Envconfig;
use serde::{Deserialize, Serialize};
use teloxide::dispatching::dialogue::serializer::Json;
use teloxide::dispatching::dialogue::{GetChatId, Serializer};
use teloxide::types::{
    InlineKeyboardButton, InlineKeyboardMarkup, InputFile, InputMedia, MediaKind, MessageId,
    MessageKind, ParseMode, ReplyMarkup,
};
use teloxide::{
    payloads::SendMessageSetters,
    prelude::*,
    utils::{command::BotCommands, render::RenderMessageTextHelper},
};

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

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
enum UserCommands {
    /// The first message of user
    Start(String),
    /// Shows this message.
    Help,
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
    pub rc: Arc<RwLock<RunnerConfig>>,
    pub runner: Runner,
}

unsafe impl Send for BotController {}

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

        let runner = Runner::init_with_db(&mut db)?;
        let rc = runner.init_config(script)?;
        let rc = Arc::new(RwLock::new(rc));

        Ok(Self {
            bot,
            db,
            rc,
            runner,
        })
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

    let mut bc = BotController::new(&config).await?;
    let state_mgr = MongodbStorage::open(config.db_url.clone().as_ref(), "gongbot", Json).await?;

    for bi in BotInstance::get_all(&mut bc.db).await? {
        let info = start_bot(bi, &mut bc.db, vec![admin_handler()]).await?;
        println!("Started bot: {}", info.name);
    }

    // TODO: delete this in production
    // allow because values are hardcoded and if they will be unparsable
    // we should panic anyway
    #[allow(clippy::unwrap_used)]
    let events: Vec<DateTime<Utc>> = ["2025-04-09T18:00:00+04:00", "2025-04-11T16:00:00+04:00"]
        .iter()
        .map(|d| DateTime::parse_from_rfc3339(d).unwrap().into())
        .collect();

    for event in events {
        match bc.db.create_event(event).await {
            Ok(e) => info!("Created event {}", e._id),
            Err(err) => info!("Failed to create event, error: {}", err),
        }
    }
    //
    let rc: std::sync::Arc<RwLock<_>> = bc.rc;

    let handler = dptree::entry()
        .inspect(|u: Update| {
            info!("{u:#?}"); // Print the update to the console with inspect
        })
        .branch(
            Update::filter_message()
                .filter_map(|m: Message| m.text().and_then(|t| BotCommand::from_str(t).ok()))
                .filter_map(move |bc: BotCommand| {
                    let rc = std::sync::Arc::clone(&rc);
                    let command = bc.command();

                    let rc = rc.read().expect("RwLock lock on commands map failed");

                    rc.get_command_message(command)
                })
                .endpoint(botscript_command_handler),
        )
        // .branch(
        //     Update::filter_message()
        //         .enter_dialogue::<Message, MongodbStorage<Json>, State>()
        //         .branch(dptree::case![State::MessageForwardReply].endpoint(user_reply_to_support)),
        // )
        .branch(Update::filter_message().endpoint(echo));

    Dispatcher::builder(bc.bot, handler)
        .dependencies(dptree::deps![bc.db, state_mgr])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}

async fn botscript_command_handler(
    bot: Bot,
    mut db: DB,
    bm: BotMessage,
    msg: Message,
) -> BotResult<()> {
    info!("Eval BM: {:?}", bm);
    let buttons = bm
        .resolve_buttons(&mut db)
        .await?
        .map(|buttons| InlineKeyboardMarkup {
            inline_keyboard: buttons
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|b| match b {
                            botscript::ButtonLayout::Callback {
                                name,
                                literal: _,
                                callback,
                            } => InlineKeyboardButton::callback(name, callback),
                        })
                        .collect()
                })
                .collect(),
        });
    let literal = bm.literal().map_or("", |s| s.as_str());

    let ma = MessageAnswerer::new(&bot, &mut db, msg.chat.id.0);
    ma.answer(literal, None, buttons).await?;

    Ok(())
}

async fn callback_handler(bot: Bot, mut db: DB, q: CallbackQuery) -> BotResult<()> {
    bot.answer_callback_query(&q.id).await?;

    let data = match q.data {
        Some(ref data) => data,
        None => {
            // not really our case to handle
            return Ok(());
        }
    };

    let callback = match CallbackStore::get_callback(&mut db, data).await? {
        Some(callback) => callback,
        None => {
            warn!("Not found callback for data: {data}");
            // doing this silently beacuse end user shouldn't know about backend internal data
            return Ok(());
        }
    };

    match callback {
        Callback::MoreInfo => {
            let keyboard = Some(single_button_markup!(
                create_callback_button("go_home", Callback::GoHome, &mut db).await?
            ));

            let chat_id = q.chat_id().map(|i| i.0).unwrap_or(q.from.id.0 as i64);
            let message_id = q.message.map_or_else(
                || {
                    Err(BotError::MsgTooOld(
                        "Failed to get message id, probably message too old".to_string(),
                    ))
                },
                |m| Ok(m.id().0),
            )?;
            MessageAnswerer::new(&bot, &mut db, chat_id)
                .replace_message(message_id, "more_info_msg", keyboard)
                .await?
        }
        Callback::ProjectPage { id } => {
            let nextproject = match db
                .get_literal_value(&format!("project_{}_msg", id + 1))
                .await?
                .unwrap_or("emptyproject".into())
                .as_str()
            {
                "end" | "empty" | "none" => None,
                _ => Some(
                    create_callback_button(
                        "next_project",
                        Callback::ProjectPage { id: id + 1 },
                        &mut db,
                    )
                    .await?,
                ),
            };
            let prevproject = match id.wrapping_sub(1) {
                0 => None,
                _ => Some(
                    create_callback_button(
                        "prev_project",
                        Callback::ProjectPage {
                            id: id.wrapping_sub(1),
                        },
                        &mut db,
                    )
                    .await?,
                ),
            };
            let keyboard = buttons_markup!(
                [prevproject, nextproject].into_iter().flatten(),
                [create_callback_button("go_home", Callback::GoHome, &mut db).await?]
            );

            let chat_id = q.chat_id().map(|i| i.0).unwrap_or(q.from.id.0 as i64);
            let message_id = q.message.map_or_else(
                || {
                    Err(BotError::MsgTooOld(
                        "Failed to get message id, probably message too old".to_string(),
                    ))
                },
                |m| Ok(m.id().0),
            )?;
            MessageAnswerer::new(&bot, &mut db, chat_id)
                .replace_message(message_id, &format!("project_{}_msg", id), Some(keyboard))
                .await?
        }
        Callback::GoHome => {
            let keyboard = make_start_buttons(&mut db).await?;

            let chat_id = q.chat_id().map(|i| i.0).unwrap_or(q.from.id.0 as i64);
            let message_id = q.message.map_or_else(
                || {
                    Err(BotError::MsgTooOld(
                        "Failed to get message id, probably message too old".to_string(),
                    ))
                },
                |m| Ok(m.id().0),
            )?;
            MessageAnswerer::new(&bot, &mut db, chat_id)
                .replace_message(message_id, "start", Some(keyboard))
                .await?
        }
        Callback::LeaveApplication => {
            let application = Application::new(q.from.clone()).store(&mut db).await?;
            let msg = send_application_to_chat(&bot, &mut db, &application).await?;

            let (chat_id, msg_id) = MessageAnswerer::new(&bot, &mut db, q.from.id.0 as i64)
                .answer("left_application_msg", None, None)
                .await?;
            MessageForward::new(msg.chat.id.0, msg.id.0, chat_id, msg_id, false)
                .store(&mut db)
                .await?;
        }
        Callback::AskQuestion => {
            MessageAnswerer::new(&bot, &mut db, q.from.id.0 as i64)
                .answer("ask_question_msg", None, None)
                .await?;
        }
    };

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
            return Err(BotError::AdminMisconfiguration(format!(
                "admin forget to set support_chat_id"
            )));
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
            return Err(BotError::AdminMisconfiguration(format!(
                "admin forget to set application_format"
            )));
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

async fn user_command_handler(
    mut db: DB,
    bot: Bot,
    msg: Message,
    cmd: UserCommands,
) -> BotResult<()> {
    let tguser = match msg.from.clone() {
        Some(user) => user,
        None => return Ok(()), // do nothing, cause its not usecase of function
    };
    let user = db
        .get_or_init_user(tguser.id.0 as i64, &tguser.first_name)
        .await?;
    let user = update_user_tg(user, &tguser);
    user.update_user(&mut db).await?;
    info!(
        "MSG: {}",
        msg.html_text().unwrap_or("|EMPTY_MESSAGE|".into())
    );
    match cmd {
        UserCommands::Start(meta) => {
            if !meta.is_empty() {
                user.insert_meta(&mut db, &meta).await?;
            }
            let variant = match meta.as_str() {
                "" => None,
                variant => Some(variant),
            };
            let mut db2 = db.clone();
            MessageAnswerer::new(&bot, &mut db, msg.chat.id.0)
                .answer("start", variant, Some(make_start_buttons(&mut db2).await?))
                .await?;
            Ok(())
        }
        UserCommands::Help => {
            bot.send_message(msg.chat.id, UserCommands::descriptions().to_string())
                .await?;
            Ok(())
        }
    }
}

async fn make_start_buttons(db: &mut DB) -> BotResult<InlineKeyboardMarkup> {
    let mut buttons: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    buttons.push(vec![
        create_callback_button("show_projects", Callback::ProjectPage { id: 1 }, db).await?,
    ]);
    buttons.push(vec![
        create_callback_button("more_info", Callback::MoreInfo, db).await?,
    ]);
    buttons.push(vec![
        create_callback_button("leave_application", Callback::LeaveApplication, db).await?,
    ]);
    buttons.push(vec![
        create_callback_button("ask_question", Callback::AskQuestion, db).await?,
    ]);

    Ok(InlineKeyboardMarkup::new(buttons))
}

async fn echo(bot: Bot, msg: Message) -> BotResult<()> {
    if let Some(photo) = msg.photo() {
        info!("File ID: {}", photo[0].file.id);
    }
    bot.send_message(msg.chat.id, msg.html_text().unwrap_or("UNWRAP".into()))
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
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
