pub mod admin;
pub mod db;
pub mod mongodb_storage;
pub mod utils;

use db::callback_info::CallbackInfo;
use log::{info, warn};
use std::time::Duration;

use crate::admin::{admin_command_handler, AdminCommands};
use crate::admin::{secret_command_handler, SecretCommands};
use crate::db::{CallDB, DB};
use crate::mongodb_storage::MongodbStorage;

use chrono::{DateTime, Utc};
use chrono_tz::Asia;
use db::DbError;
use envconfig::Envconfig;
use serde::{Deserialize, Serialize};
use teloxide::dispatching::dialogue::serializer::Json;
use teloxide::dispatching::dialogue::{GetChatId, Serializer};
use teloxide::types::{
    InlineKeyboardButton, InlineKeyboardMarkup, InputFile, InputMedia, MediaKind, MessageKind,
    ParseMode, ReplyMarkup,
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
}

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
enum UserCommands {
    /// The first message of user
    Start,
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
        lang: String,
        is_caption_set: bool,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename = "snake_case")]
pub enum Callback {
    MoreInfo,
    ProjectPage { id: u32 },
}

type CallbackStore = CallbackInfo<Callback>;

pub struct BotController {
    pub bot: Bot,
    pub db: DB,
}

impl BotController {
    pub async fn new(config: &Config) -> Result<Self, Box<dyn std::error::Error>> {
        let bot = Bot::new(&config.bot_token);
        let db = DB::init(&config.db_url).await?;

        Ok(Self { bot, db })
    }
}

#[derive(thiserror::Error, Debug)]
pub enum BotError {
    DBError(#[from] DbError),
    TeloxideError(#[from] teloxide::RequestError),
    // TODO: not a really good to hardcode types, better to extend it later
    StorageError(#[from] mongodb_storage::MongodbStorageError<<Json as Serializer<State>>::Error>),
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
    let config = Config::init_from_env()?;

    let mut bc = BotController::new(&config).await?;
    let state_mgr = MongodbStorage::open(config.db_url.clone().as_ref(), "gongbot", Json).await?;

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

    let handler = dptree::entry()
        .inspect(|u: Update| {
            info!("{u:#?}"); // Print the update to the console with inspect
        })
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
                    dptree::case![State::Edit {
                        literal,
                        lang,
                        is_caption_set
                    }]
                    .endpoint(edit_msg_handler),
                ),
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
            answer_message(
                &bot,
                q.chat_id().map(|i| i.0).unwrap_or(q.from.id.0 as i64),
                &mut db,
                "more_info",
                None as Option<InlineKeyboardMarkup>,
            )
            .await?
        }
        _ => {
            unimplemented!()
        }
    };

    Ok(())
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
                    lang,
                    is_caption_set: false,
                })
                .await?;
            bot.send_message(
                msg.chat.id,
                "Ok, now you have to send message text (formatting supported)",
            )
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
    (literal, lang, is_caption_set): (String, String, bool),
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
        UserCommands::Start => {
            let mut db2 = db.clone();
            answer_message(
                &bot,
                msg.chat.id.0,
                &mut db,
                "start",
                Some(make_start_buttons(&mut db2).await?),
            )
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

async fn answer_message<RM: Into<ReplyMarkup>>(
    bot: &Bot,
    chat_id: i64,
    db: &mut DB,
    literal: &str,
    keyboard: Option<RM>,
) -> BotResult<()> {
    let text = db
        .get_literal_value(literal)
        .await?
        .unwrap_or("Please, set content of this message".into());
    let media = db.get_media(literal).await?;
    let (chat_id, msg_id) = match media.len() {
        // just a text
        0 => {
            let msg = bot.send_message(ChatId(chat_id), text);
            let msg = match keyboard {
                Some(kbd) => msg.reply_markup(kbd),
                None => msg,
            };
            let msg = msg.parse_mode(teloxide::types::ParseMode::Html);
            info!("ENTS: {:?}", msg.entities);
            let msg = msg.await?;

            (msg.chat.id.0, msg.id.0)
        }
        // single media
        1 => {
            let media = &media[0]; // safe, cause we just checked len
            match media.media_type.as_str() {
                "photo" => {
                    let msg = bot.send_photo(
                        ChatId(chat_id),
                        InputFile::file_id(media.file_id.to_string()),
                    );
                    let msg = match text.as_str() {
                        "" => msg,
                        text => msg.caption(text),
                    };
                    let msg = match keyboard {
                        Some(kbd) => msg.reply_markup(kbd),
                        None => msg,
                    };

                    let msg = msg.parse_mode(teloxide::types::ParseMode::Html);
                    let msg = msg.await?;

                    (msg.chat.id.0, msg.id.0)
                }
                "video" => {
                    let msg = bot.send_video(
                        ChatId(chat_id),
                        InputFile::file_id(media.file_id.to_string()),
                    );
                    let msg = match text.as_str() {
                        "" => msg,
                        text => msg.caption(text),
                    };
                    let msg = match keyboard {
                        Some(kbd) => msg.reply_markup(kbd),
                        None => msg,
                    };

                    let msg = msg.parse_mode(teloxide::types::ParseMode::Html);
                    let msg = msg.await?;

                    (msg.chat.id.0, msg.id.0)
                }
                _ => {
                    todo!()
                }
            }
        }
        // >= 2, should use media group
        _ => {
            let media: Vec<InputMedia> = media
                .into_iter()
                .enumerate()
                .map(|(i, m)| {
                    let ifile = InputFile::file_id(m.file_id);
                    let caption = if i == 0 {
                        match text.as_str() {
                            "" => None,
                            text => Some(text.to_string()),
                        }
                    } else {
                        None
                    };
                    match m.media_type.as_str() {
                        "photo" => InputMedia::Photo(teloxide::types::InputMediaPhoto {
                            media: ifile,
                            caption,
                            parse_mode: Some(ParseMode::Html),
                            caption_entities: None,
                            has_spoiler: false,
                            show_caption_above_media: false,
                        }),
                        "video" => InputMedia::Video(teloxide::types::InputMediaVideo {
                            media: ifile,
                            thumbnail: None,
                            caption,
                            parse_mode: Some(ParseMode::Html),
                            caption_entities: None,
                            show_caption_above_media: false,
                            width: None,
                            height: None,
                            duration: None,
                            supports_streaming: None,
                            has_spoiler: false,
                        }),
                        _ => {
                            todo!()
                        }
                    }
                })
                .collect();
            let msg = bot.send_media_group(ChatId(chat_id), media);

            let msg = msg.await?;

            (msg[0].chat.id.0, msg[0].id.0)
        }
    };
    db.set_message_literal(chat_id, msg_id, literal).await?;
    Ok(())
}

async fn make_start_buttons(db: &mut DB) -> BotResult<InlineKeyboardMarkup> {
    let mut buttons: Vec<Vec<InlineKeyboardButton>> = db
        .get_all_events()
        .await?
        .iter()
        .map(|e| {
            vec![InlineKeyboardButton::callback(
                e.time.with_timezone(&Asia::Dubai).to_string(),
                format!("event:{}", e._id),
            )]
        })
        .collect();
    buttons.push(vec![InlineKeyboardButton::callback(
        "More info",
        CallbackStore::new(Callback::MoreInfo)
            .store(db)
            .await?
            .get_id(),
    )]);

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
