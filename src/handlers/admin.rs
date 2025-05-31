use std::str::FromStr;

use itertools::Itertools;
use log::{info, warn};
use std::time::Duration;
use teloxide::dispatching::dialogue::serializer::Json;
use teloxide::net::Download;
use teloxide::prelude::*;
use teloxide::sugar::request::RequestReplyExt;
use teloxide::types::{MediaKind, MessageId, MessageKind, ParseMode};
use teloxide::utils::render::RenderMessageTextHelper;
use teloxide::{dptree, types::Update};

use futures::StreamExt;

use crate::admin::{admin_command_handler, AdminCommands};
use crate::bot_handler::BotHandler;
use crate::db::bots::BotInstance;
use crate::db::message_forward::MessageForward;
use crate::db::{CallDB, DB};
use crate::mongodb_storage::MongodbStorage;
use crate::{BotDialogue, BotError, BotResult, CallbackStore, State};

pub fn admin_handler() -> BotHandler {
    dptree::entry()
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
        .branch(command_handler())
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
                        .filter_map(|msg: Message| {
                            let text = msg.caption().unwrap_or("");
                            let mut parts = text.split_whitespace();
                            let cmd = parts.next().unwrap_or("");
                            let arg = parts.next().unwrap_or("");

                            match cmd.to_lowercase().as_str() == "/newscript" {
                                true => Some(arg.to_string()),
                                false => None,
                            }
                        })
                        .endpoint(newscript_handler),
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
}
async fn newscript_handler(bot: Bot, mut db: DB, msg: Message, name: String) -> BotResult<()> {
    let script = match msg.kind {
        MessageKind::Common(message) => {
            match message.media_kind {
                MediaKind::Document(media_document) => {
                    let doc = media_document.document;
                    let file = bot.get_file(doc.file.id).await?;
                    let mut stream = bot.download_file_stream(&file.path);
                    let mut buf: Vec<u8> = Vec::new();
                    while let Some(bytes) = stream.next().await {
                        let mut bytes = bytes.unwrap().to_vec();
                        buf.append(&mut bytes);
                    }
                    let script = match String::from_utf8(buf) {
                        Ok(s) => s,
                        Err(err) => {
                            warn!("Failed to parse buf to string, err: {err}");
                            bot.send_message(msg.chat.id, format!("Failed to Convert file to script: file is not UTF-8, err: {err}")).await?;
                            return Ok(());
                        }
                    };
                    script
                }
                _ => todo!(),
            }
        }
        _ => todo!(),
    };

    match BotInstance::get_by_name(&mut db, &name).await? {
        Some(bi) => bi,
        None => {
            bot.send_message(
                msg.chat.id,
                format!("Failed to set script, possibly bots name is incorrent"),
            )
            .await?;
            return Ok(());
        }
    };
    BotInstance::update_script(&mut db, &name, &script).await?;

    bot.send_message(msg.chat.id, "New script is set!").await?;
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

fn command_handler() -> BotHandler {
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
        .filter_command::<AdminCommands>()
        .enter_dialogue::<Message, MongodbStorage<Json>, State>()
        .endpoint(admin_command_handler)
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
