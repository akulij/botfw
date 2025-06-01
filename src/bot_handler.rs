use log::{error, info};
use quickjs_rusty::serde::to_js;
use std::{
    str::FromStr,
    sync::{Arc, RwLock},
};
use teloxide::{
    dispatching::{dialogue::GetChatId, UpdateFilterExt},
    dptree::{self, Handler},
    prelude::DependencyMap,
    types::{CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, Message, Update},
    Bot,
};

use crate::{
    botscript::{self, BotMessage, RunnerConfig},
    commands::BotCommand,
    db::{CallDB, DB},
    message_answerer::MessageAnswerer,
    update_user_tg, BotError, BotResult, BotRuntime,
};

pub type BotHandler =
    Handler<'static, DependencyMap, BotResult<()>, teloxide::dispatching::DpHandlerDescription>;

pub fn script_handler(r: Arc<BotRuntime>) -> BotHandler {
    let cr = r.clone();
    dptree::entry()
        .branch(
            Update::filter_message()
                // check if message is command
                .filter_map(|m: Message| m.text().and_then(|t| BotCommand::from_str(t).ok()))
                // check if command is presented in config
                .filter_map(move |bc: BotCommand| {
                    let r = std::sync::Arc::clone(&r);
                    let command = bc.command();

                    let rc = r.rc.lock().expect("RwLock lock on commands map failed");

                    rc.get_command_message(command)
                })
                .endpoint(handle_botmessage),
        )
        .branch(
            Update::filter_callback_query()
                .filter_map(move |q: CallbackQuery| {
                    q.data.and_then(|data| {
                        let r = std::sync::Arc::clone(&cr);
                        let rc = r.rc.lock().expect("RwLock lock on commands map failed");

                        rc.get_callback_message(&data)
                    })
                })
                .endpoint(handle_callback),
        )
}

async fn handle_botmessage(bot: Bot, mut db: DB, bm: BotMessage, msg: Message) -> BotResult<()> {
    info!("Eval BM: {:?}", bm);
    let tguser = match msg.from.clone() {
        Some(user) => user,
        None => return Ok(()), // do nothing, cause its not usecase of function
    };
    let user = db
        .get_or_init_user(tguser.id.0 as i64, &tguser.first_name)
        .await?;
    let user = update_user_tg(user, &tguser);
    user.update_user(&mut db).await?;

    let is_propagate: bool = match bm.get_handler() {
        Some(handler) => 'prop: {
            let ctx = match handler.context() {
                Some(ctx) => ctx,
                // falling back to propagation
                None => break 'prop true,
            };
            let jsuser = to_js(ctx, &tguser).unwrap();
            info!(
                "Calling handler {:?} with msg literal: {:?}",
                handler,
                bm.literal()
            );
            match handler.call_args(vec![jsuser]) {
                Ok(v) => {
                    if v.is_bool() {
                        v.to_bool().unwrap_or(true)
                    } else if v.is_int() {
                        v.to_int().unwrap_or(1) != 0
                    } else {
                        // falling back to propagation
                        true
                    }
                }
                Err(err) => {
                    error!("Failed to get return of handler, err: {err}");
                    // falling back to propagation
                    true
                }
            }
        }
        None => true,
    };

    if !is_propagate {
        return Ok(());
    }

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

async fn handle_callback(bot: Bot, mut db: DB, bm: BotMessage, q: CallbackQuery) -> BotResult<()> {
    info!("Eval BM: {:?}", bm);
    let tguser = q.from.clone();
    let user = db
        .get_or_init_user(tguser.id.0 as i64, &tguser.first_name)
        .await?;
    let user = update_user_tg(user, &tguser);
    user.update_user(&mut db).await?;

    println!("Is handler set: {}", bm.get_handler().is_some());
    let is_propagate: bool = match bm.get_handler() {
        Some(handler) => 'prop: {
            let ctx = match handler.context() {
                Some(ctx) => ctx,
                // falling back to propagation
                None => break 'prop true,
            };
            let jsuser = to_js(ctx, &tguser).unwrap();
            println!(
                "Calling handler {:?} with msg literal: {:?}",
                handler,
                bm.literal()
            );
            match handler.call_args(vec![jsuser]) {
                Ok(v) => {
                    println!("Ok branch, value: {v:?}");
                    if v.is_bool() {
                        v.to_bool().unwrap_or(true)
                    } else if v.is_int() {
                        v.to_int().unwrap_or(1) != 0
                    } else {
                        // falling back to propagation
                        true
                    }
                }
                Err(err) => {
                    println!("ERR branch");
                    error!("Failed to get return of handler, err: {err}");
                    // falling back to propagation
                    true
                }
            }
        }
        None => true,
    };

    if !is_propagate {
        return Ok(());
    }

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

    let (chat_id, msg_id) = {
        let chat_id = match q.chat_id() {
            Some(chat_id) => chat_id.0,
            None => tguser.id.0 as i64,
        };

        let msg_id = q.message.map_or_else(
            || {
                Err(BotError::MsgTooOld(
                    "Failed to get message id, probably message too old".to_string(),
                ))
            },
            |m| Ok(m.id().0),
        );

        (chat_id, msg_id)
    };

    let ma = MessageAnswerer::new(&bot, &mut db, chat_id);
    match bm.is_replace() {
        true => {
            match msg_id {
                Ok(msg_id) => {
                    ma.replace_message(msg_id, literal, buttons).await?;
                }
                Err(err) => {
                    ma.answer(literal, None, buttons).await?;
                }
            };
        }
        false => {
            ma.answer(literal, None, buttons).await?;
        }
    }

    Ok(())
}
