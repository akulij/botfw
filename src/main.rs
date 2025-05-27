pub mod admin;
pub mod bot_handler;
pub mod bot_manager;
pub mod botscript;
pub mod commands;
pub mod db;
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

#[derive(Envconfig)]
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
        let info = start_bot(bi, &mut bc.db).await?;
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
        .branch(
            Update::filter_callback_query()
                .filter_async(async |q: CallbackQuery, mut db: DB| {
                    let tguser = q.from.clone();
                    let user = db
                        .get_or_init_user(tguser.id.0 as i64, &tguser.first_name)
                        .await;
                    user.map(|u| u.is_admin).unwrap_or(false)
                })
                .enter_dialogue::<CallbackQuery, MongodbStorage<Json>, State>()
                .branch(dptree::case![State::EditButton].endpoint(button_edit_callback)),
        )
        .branch(Update::filter_callback_query().endpoint(callback_handler))
        .branch(command_handler(config))
        .branch(
            Update::filter_message()
                .filter_async(async |msg: Message, mut db: DB| {
                    let tguser = match msg.from.clone() {
                        Some(user) => user,
                        None => return false, // do nothing, cause its not usecase of function
                    };
                    let user = db
                        .get_or_init_user(tguser.id.0 as i64, &tguser.first_name)
                        .await;
                    user.map(|u| u.is_admin).unwrap_or(false)
                })
                .enter_dialogue::<Message, MongodbStorage<Json>, State>()
                .branch(
                    Update::filter_message()
                        .filter(|msg: Message| {
                            msg.text().unwrap_or("").to_lowercase().as_str() == "edit"
                        })
                        .endpoint(edit_msg_cmd_handler),
                )
                .branch(
                    Update::filter_message()
                        .filter(|msg: Message| msg.reply_to_message().is_some())
                        .filter(|state: State| matches!(state, State::Start))
                        .endpoint(support_reply_handler),
                )
                .branch(
                    dptree::case![State::Edit {
                        literal,
                        variant,
                        lang,
                        is_caption_set
                    }]
                    .endpoint(edit_msg_handler),
                ),
        )
        .branch(
            Update::filter_message()
                .enter_dialogue::<Message, MongodbStorage<Json>, State>()
                .branch(dptree::case![State::MessageForwardReply].endpoint(user_reply_to_support)),
        )
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

async fn user_reply_to_support(bot: Bot, mut db: DB, msg: Message) -> BotResult<()> {
    let (source_chat_id, source_message_id) = (msg.chat.id.0, msg.id.0);
    let text = match msg.html_text() {
        Some(text) => text,
        // TODO: come up with better idea than just ignoring (say something to user)
        None => return Ok(()),
    };
    let scid =
        db.get_literal_value("support_chat_id")
            .await?
            .ok_or(BotError::AdminMisconfiguration(
                "support_chat_id is not set".to_string(),
            ))?;
    let support_chat_id = match scid.parse::<i64>() {
        Ok(cid) => cid,
        Err(parseerr) => {
            return Err(BotError::BotLogicError(format!(
                "source_chat_id, got: {scid}, expected: i64, err: {parseerr}"
            )))
        }
    };
    let user = msg.from.ok_or(BotError::BotLogicError(
        "Unable to get user somehow:/".to_string(),
    ))?;
    let parts = [
        Some(user.first_name),
        user.last_name,
        user.username.map(|un| format!("(@{un})")),
    ];
    #[allow(unstable_name_collisions)]
    let userformat: String = parts
        .into_iter()
        .flatten()
        .intersperse(" ".to_string())
        .collect();
    let msgtext = format!("From: {userformat}\nMessage:\n{text}");

    // TODO: fix bug: parse mode's purpose is to display user-formated text in right way,
    // but there is a bug: user can inject html code with his first/last/user name
    // it's not harmful, only visible to support, but still need a fix
    let sentmsg = bot
        .send_message(ChatId(support_chat_id), msgtext)
        .parse_mode(ParseMode::Html)
        .await?;
    MessageForward::new(
        sentmsg.chat.id.0,
        sentmsg.id.0,
        source_chat_id,
        source_message_id,
        true,
    )
    .store(&mut db)
    .await?;

    Ok(())
}

async fn support_reply_handler(
    bot: Bot,
    mut db: DB,
    msg: Message,
    state_mgr: std::sync::Arc<MongodbStorage<Json>>,
) -> BotResult<()> {
    use teloxide::utils::render::Renderer;

    let rm = match msg.reply_to_message() {
        Some(rm) => rm,
        None => {
            return Err(BotError::BotLogicError(
                "support_reply_handler should not be called when no message is replied".to_string(),
            ));
        }
    };
    let (chat_id, message_id) = (rm.chat.id.0, rm.id.0);
    let mf = match MessageForward::get(&mut db, chat_id, message_id).await? {
        Some(mf) => mf,
        None => {
            bot.send_message(msg.chat.id, "No forwarded message found for your reply")
                .await?;

            return Ok(());
        }
    };

    let text = match msg.kind {
        MessageKind::Common(message_common) => match message_common.media_kind {
            MediaKind::Text(media_text) => {
                Renderer::new(&media_text.text, &media_text.entities).as_html()
            }
            _ => {
                bot.send_message(msg.chat.id, "Only text messages currently supported!")
                    .await?;
                return Ok(());
            }
        },
        // can't hapen because we already have check for reply
        _ => unreachable!(),
    };

    let msg = bot
        .send_message(ChatId(mf.source_chat_id), text)
        .parse_mode(ParseMode::Html);
    let msg = match mf.reply {
        false => msg,
        true => msg.reply_to(MessageId(mf.source_message_id)),
    };
    msg.await?;

    let user_dialogue = BotDialogue::new(state_mgr, ChatId(mf.source_chat_id));
    user_dialogue.update(State::MessageForwardReply).await?;

    Ok(())
}

async fn button_edit_callback(
    bot: Bot,
    mut db: DB,
    dialogue: BotDialogue,
    q: CallbackQuery,
) -> BotResult<()> {
    bot.answer_callback_query(&q.id).await?;

    let id = match q.data {
        Some(id) => id,
        None => {
            bot.send_message(q.from.id, "Not compatible callback to edit text on")
                .await?;

            return Ok(());
        }
    };

    let ci = match CallbackStore::get(&mut db, &id).await? {
        Some(ci) => ci,
        None => {
            bot.send_message(
                q.from.id,
                "Can't get button information. Maybe created not by this bot or message too old",
            )
            .await?;

            return Ok(());
        }
    };
    let literal = match ci.literal {
        Some(l) => l,
        None => {
            bot.send_message(
                q.from.id,
                "This button is not editable (probably text is generated)",
            )
            .await?;

            return Ok(());
        }
    };

    let lang = "ru".to_string();
    dialogue
        .update(State::Edit {
            literal,
            variant: None,
            lang,
            is_caption_set: false,
        })
        .await?;

    bot.send_message(q.from.id, "Send text of button").await?;

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

async fn edit_msg_cmd_handler(
    bot: Bot,
    mut db: DB,
    dialogue: BotDialogue,
    msg: Message,
) -> BotResult<()> {
    match msg.reply_to_message() {
        Some(replied) => {
            let msgid = replied.id;
            // look for message in db and set text
            let literal = match db.get_message_literal(msg.chat.id.0, msgid.0).await? {
                Some(l) => l,
                None => {
                    bot.send_message(msg.chat.id, "No such message found to edit. Look if you replying bot's message and this message is supposed to be editable").await?;
                    return Ok(());
                }
            };
            // TODO: language selector will be implemented in future 😈
            let lang = "ru".to_string();
            dialogue
                .update(State::Edit {
                    literal,
                    variant: None,
                    lang,
                    is_caption_set: false,
                })
                .await?;
            bot.send_message(
                msg.chat.id,
                "Ok, now you have to send message text (formatting supported)\n\
                 <b>Notice:</b> if this message supposed to replace message (tg shows them as edited) \
                 or be raplaced, do NOT send message with multiple media, only single photo, video etc. \
                 To get more information about why, see in /why_media_group",
            ).parse_mode(ParseMode::Html)
            .await?;
        }
        None => {
            bot.send_message(msg.chat.id, "You have to reply to message to edit it")
                .await?;
        }
    };
    Ok(())
}

async fn edit_msg_handler(
    bot: Bot,
    mut db: DB,
    dialogue: BotDialogue,
    (literal, variant, lang, is_caption_set): (String, Option<String>, String, bool),
    msg: Message,
) -> BotResult<()> {
    use teloxide::utils::render::Renderer;

    let chat_id = msg.chat.id;
    info!("Type: {:#?}", msg.kind);
    let msg = if let MessageKind::Common(msg) = msg.kind {
        msg
    } else {
        info!("Not a Common, somehow");
        return Ok(());
    };

    if let Some(variant) = variant {
        if let MediaKind::Text(text) = msg.media_kind {
            let html_text = Renderer::new(&text.text, &text.entities).as_html();

            db.set_literal_alternative(&literal, &variant, &html_text)
                .await?;
            bot.send_message(chat_id, "Updated text of variant!")
                .await?;

            dialogue.exit().await?;
            return Ok(());
        } else {
            bot.send_message(
                chat_id,
                "On variants only text alternating supported. Try to send text only",
            )
            .await?;

            return Ok(());
        }
    };

    match msg.media_kind {
        MediaKind::Text(text) => {
            db.drop_media(&literal).await?;
            if is_caption_set {
                return Ok(());
            };
            let html_text = Renderer::new(&text.text, &text.entities).as_html();
            db.set_literal(&literal, &html_text).await?;
            bot.send_message(chat_id, "Updated text of message!")
                .await?;
            dialogue.exit().await?;
        }
        MediaKind::Photo(photo) => {
            let group = photo.media_group_id;
            if let Some(group) = group.clone() {
                db.drop_media_except(&literal, &group).await?;
            } else {
                db.drop_media(&literal).await?;
            }
            let file_id = photo.photo[0].file.id.clone();
            db.add_media(&literal, "photo", &file_id, group.as_deref())
                .await?;
            match photo.caption {
                Some(text) => {
                    let html_text = Renderer::new(&text, &photo.caption_entities).as_html();
                    db.set_literal(&literal, &html_text).await?;
                    bot.send_message(chat_id, "Updated photo caption!").await?;
                }
                None => {
                    // if it is a first message in group,
                    // or just a photo without caption (unwrap_or case),
                    // set text empty
                    if !db
                        .is_media_group_exists(group.as_deref().unwrap_or(""))
                        .await?
                    {
                        db.set_literal(&literal, "").await?;
                        bot.send_message(chat_id, "Set photo without caption")
                            .await?;
                    };
                }
            }
            // Some workaround because Telegram's group system
            // is not easily and obviously handled with this
            // code architecture, but probably there is a solution.
            //
            // So, this code will just wait for all media group
            // updates to be processed
            dialogue
                .update(State::Edit {
                    literal,
                    variant: None,
                    lang,
                    is_caption_set: true,
                })
                .await?;
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                dialogue.exit().await.unwrap_or(());
            });
        }
        MediaKind::Video(video) => {
            let group = video.media_group_id;
            if let Some(group) = group.clone() {
                db.drop_media_except(&literal, &group).await?;
            } else {
                db.drop_media(&literal).await?;
            }
            let file_id = video.video.file.id;
            db.add_media(&literal, "video", &file_id, group.as_deref())
                .await?;
            match video.caption {
                Some(text) => {
                    let html_text = Renderer::new(&text, &video.caption_entities).as_html();
                    db.set_literal(&literal, &html_text).await?;
                    bot.send_message(chat_id, "Updated video caption!").await?;
                }
                None => {
                    // if it is a first message in group,
                    // or just a video without caption (unwrap_or case),
                    // set text empty
                    if !db
                        .is_media_group_exists(group.as_deref().unwrap_or(""))
                        .await?
                    {
                        db.set_literal(&literal, "").await?;
                        bot.send_message(chat_id, "Set video without caption")
                            .await?;
                    };
                }
            }
            // Some workaround because Telegram's group system
            // is not easily and obviously handled with this
            // code architecture, but probably there is a solution.
            //
            // So, this code will just wait for all media group
            // updates to be processed
            dialogue
                .update(State::Edit {
                    literal,
                    variant: None,
                    lang,
                    is_caption_set: true,
                })
                .await?;
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                dialogue.exit().await.unwrap_or(());
            });
        }
        _ => {
            bot.send_message(chat_id, "this type of message is not supported yet")
                .await?;
        }
    }

    Ok(())
}

fn command_handler(
    config: Config,
) -> Handler<'static, DependencyMap, BotResult<()>, teloxide::dispatching::DpHandlerDescription> {
    Update::filter_message()
        .branch(
            dptree::entry()
                .filter_command::<UserCommands>()
                .endpoint(user_command_handler),
        )
        .branch(
            dptree::entry()
                .filter_command::<SecretCommands>()
                .map(move || config.admin_password.clone())
                .endpoint(secret_command_handler),
        )
        .branch(
            dptree::entry()
                .filter_async(async |msg: Message, mut db: DB| {
                    let tguser = match msg.from.clone() {
                        Some(user) => user,
                        None => return false, // do nothing, cause its not usecase of function
                    };
                    let user = db
                        .get_or_init_user(tguser.id.0 as i64, &tguser.first_name)
                        .await;
                    user.map(|u| u.is_admin).unwrap_or(false)
                })
                .filter_command::<AdminCommands>()
                .enter_dialogue::<Message, MongodbStorage<Json>, State>()
                .endpoint(admin_command_handler),
        )
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
