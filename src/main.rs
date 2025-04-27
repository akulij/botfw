pub mod admin;
pub mod db;
pub mod mongodb_storage;

use std::time::Duration;

use crate::admin::{admin_command_handler, AdminCommands};
use crate::admin::{secret_command_handler, SecretCommands};
use crate::db::{CallDB, DB};
use crate::mongodb_storage::MongodbStorage;

use chrono::{DateTime, Utc};
use chrono_tz::Asia;
use envconfig::Envconfig;
use serde::{Deserialize, Serialize};
use teloxide::dispatching::dialogue::serializer::Json;
use teloxide::dispatching::dialogue::GetChatId;
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
struct Config {
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
        println!("msg: {}", self.text);
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv()?;
    let config = Config::init_from_env()?;

    let bot = Bot::new(&config.bot_token);
    let mut db = DB::init(&config.db_url).await;
    let db_url2 = config.db_url.clone();
    let state_mgr = MongodbStorage::open(&db_url2, "gongbot", Json).await?;

    // TODO: delete this in production
    let events: Vec<DateTime<Utc>> = vec!["2025-04-09T18:00:00+04:00", "2025-04-11T16:00:00+04:00"]
        .iter()
        .map(|d| DateTime::parse_from_rfc3339(d).unwrap().into())
        .collect();

    for event in events {
        match db.clone().create_event(event).await {
            Ok(e) => println!("Created event {}", e._id),
            Err(err) => println!("Failed to create event, error: {}", err),
        }
    }
    //

    let handler = dptree::entry()
        .inspect(|u: Update| {
            eprintln!("{u:#?}"); // Print the update to the console with inspect
        })
        .branch(Update::filter_callback_query().endpoint(callback_handler))
        .branch(command_handler(config))
        .branch(
            Update::filter_message()
                .filter_async(async |msg: Message, mut db: DB| {
                    let tguser = msg.from.unwrap();
                    let user = db
                        .get_or_init_user(tguser.id.0 as i64, &tguser.first_name)
                        .await;
                    user.is_admin
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

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![db, state_mgr])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}

async fn callback_handler(
    bot: Bot,
    mut db: DB,
    q: CallbackQuery,
) -> Result<(), teloxide::RequestError> {
    bot.answer_callback_query(&q.id).await?;

    if let Some(ref data) = q.data {
        match data.as_str() {
            "more_info" => {
                answer_message(
                    &bot,
                    q.chat_id()
                        .clone()
                        .map(|i| i.0)
                        .unwrap_or(q.from.id.0 as i64),
                    &mut db,
                    "more_info",
                    None as Option<InlineKeyboardMarkup>,
                )
                .await?
            }
            _ => {} // do nothing, yet
        }
    }

    Ok(())
}

async fn edit_msg_cmd_handler(
    bot: Bot,
    mut db: DB,
    dialogue: BotDialogue,
    msg: Message,
) -> Result<(), teloxide::RequestError> {
    match msg.reply_to_message() {
        Some(replied) => {
            let msgid = replied.id;
            // look for message in db and set text
            let literal = match db
                .get_message_literal(msg.chat.id.0, msgid.0)
                .await
                .unwrap()
            {
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
                .await
                .unwrap();
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
) -> Result<(), teloxide::RequestError> {
    use teloxide::utils::render::Renderer;

    let chat_id = msg.chat.id;
    println!("Type: {:#?}", msg.kind);
    let msg = if let MessageKind::Common(msg) = msg.kind {
        msg
    } else {
        println!("Not a Common, somehow");
        return Ok(());
    };

    match msg.media_kind {
        MediaKind::Text(text) => {
            if is_caption_set {
                return Ok(());
            };
            let html_text = Renderer::new(&text.text, &text.entities).as_html();
            db.set_literal(&literal, &html_text).await.unwrap();
            bot.send_message(chat_id, "Updated text of message!")
                .await?;
            dialogue.exit().await.unwrap();
        }
        MediaKind::Photo(photo) => {
            let group = photo.media_group_id;
            if let Some(group) = group.clone() {
                db.drop_media_except(&literal, &group).await.unwrap();
            } else {
                db.drop_media(&literal).await.unwrap();
            }
            let file_id = photo.photo[0].file.id.clone();
            db.add_media(&literal, "photo", &file_id, group.as_deref())
                .await
                .unwrap();
            match photo.caption {
                Some(text) => {
                    let html_text = Renderer::new(&text, &photo.caption_entities).as_html();
                    db.set_literal(&literal, &html_text).await.unwrap();
                    bot.send_message(chat_id, "Updated photo caption!").await?;
                }
                None => {
                    // if it is a first message in group,
                    // or just a photo without caption (unwrap_or case),
                    // set text empty
                    if !db
                        .is_media_group_exists(group.as_deref().unwrap_or(""))
                        .await
                        .unwrap()
                    {
                        db.set_literal(&literal, "").await.unwrap();
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
                .await
                .unwrap();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                dialogue.exit().await.unwrap_or(());
            });
        }
        MediaKind::Video(video) => {
            let group = video.media_group_id;
            if let Some(group) = group.clone() {
                db.drop_media_except(&literal, &group).await.unwrap();
            } else {
                db.drop_media(&literal).await.unwrap();
            }
            let file_id = video.video.file.id;
            db.add_media(&literal, "video", &file_id, group.as_deref())
                .await
                .unwrap();
            match video.caption {
                Some(text) => {
                    let html_text = Renderer::new(&text, &video.caption_entities).as_html();
                    db.set_literal(&literal, &html_text).await.unwrap();
                    bot.send_message(chat_id, "Updated video caption!").await?;
                }
                None => {
                    // if it is a first message in group,
                    // or just a video without caption (unwrap_or case),
                    // set text empty
                    if !db
                        .is_media_group_exists(group.as_deref().unwrap_or(""))
                        .await
                        .unwrap()
                    {
                        db.set_literal(&literal, "").await.unwrap();
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
                .await
                .unwrap();
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
) -> Handler<
    'static,
    DependencyMap,
    Result<(), teloxide::RequestError>,
    teloxide::dispatching::DpHandlerDescription,
> {
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
                    let tguser = msg.from.unwrap();
                    let user = db
                        .get_or_init_user(tguser.id.0 as i64, &tguser.first_name)
                        .await;
                    user.is_admin
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
) -> Result<(), teloxide::RequestError> {
    let tguser = msg.from.clone().unwrap();
    let user = db
        .get_or_init_user(tguser.id.0 as i64, &tguser.first_name)
        .await;
    println!("MSG: {}", msg.html_text().unwrap());
    match cmd {
        UserCommands::Start => {
            let mut db2 = db.clone();
            answer_message(
                &bot,
                msg.chat.id.0,
                &mut db,
                "start",
                Some(make_start_buttons(&mut db2).await),
            )
            .await
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
) -> Result<(), teloxide::RequestError> {
    let text = db
        .get_literal_value(literal)
        .await
        .unwrap()
        .unwrap_or("Please, set content of this message".into());
    let media = db.get_media(&literal).await.unwrap();
    let (chat_id, msg_id) = match media.len() {
        // just a text
        0 => {
            let msg = bot.send_message(ChatId(chat_id), text);
            let msg = match keyboard {
                Some(kbd) => msg.reply_markup(kbd),
                None => msg,
            };
            let msg = msg.parse_mode(teloxide::types::ParseMode::Html);
            println!("ENTS: {:?}", msg.entities);
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
    db.set_message_literal(chat_id, msg_id, literal)
        .await
        .unwrap();
    Ok(())
}

async fn make_start_buttons(db: &mut DB) -> InlineKeyboardMarkup {
    let mut buttons: Vec<Vec<InlineKeyboardButton>> = db
        .get_all_events()
        .await
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
        "more_info",
    )]);

    InlineKeyboardMarkup::new(buttons)
}

async fn echo(bot: Bot, msg: Message) -> Result<(), teloxide::RequestError> {
    if let Some(photo) = msg.photo() {
        println!("File ID: {}", photo[0].file.id);
    }
    bot.send_message(msg.chat.id, msg.html_text().unwrap_or("UNWRAP".into()))
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;
    Ok(())
}
