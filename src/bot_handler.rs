use futures::future::join_all;
use log::{error, info};
use quickjs_rusty::serde::to_js;
use serde_json::Value;
use std::{
    str::FromStr,
    sync::{Arc, Mutex},
};
use teloxide::{
    dispatching::{dialogue::GetChatId, UpdateFilterExt},
    dptree::{self, Handler},
    prelude::{DependencyMap, Requester},
    types::{CallbackQuery, InlineKeyboardMarkup, Message, Update},
    Bot,
};

use crate::{
    botscript::{self, message_info::MessageInfoBuilder, ScriptError},
    commands::BotCommand,
    config::{
        dialog::{button::ButtonLayout, message::BotMessage},
        traits::ProviderSerialize,
        Provider,
    },
    db::{callback_info::CallbackInfo, CallDB, DB},
    message_answerer::MessageAnswerer,
    notify_admin, update_user_tg,
    utils::callback_button,
    BotError, BotResult, BotRuntime,
};

pub type BotHandler =
    Handler<'static, DependencyMap, BotResult<()>, teloxide::dispatching::DpHandlerDescription>;

type CallbackStore = CallbackInfo<Value>;

pub fn script_handler<P: Provider + Send + Sync>(r: Arc<Mutex<BotRuntime<P>>>) -> BotHandler {
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

                    let r = r.lock().expect("RwLock lock on commands map failed");
                    let rc = &r.rc;

                    // it's not necessary, but avoiding some hashmap lookups
                    match bc.args() {
                        Some(variant) => rc.get_command_message_varianted(command, variant),
                        None => rc.get_command_message(command),
                    }
                })
                .endpoint(handle_botmessage),
        )
        .branch(
            Update::filter_callback_query()
                .filter_map_async(move |q: CallbackQuery, mut db: DB| {
                    let r = Arc::clone(&cr);
                    async move {
                        let data = match q.data {
                            Some(data) => data,
                            None => return None,
                        };

                        let ci = match CallbackStore::get(&mut db, &data).await {
                            Ok(ci) => ci,
                            Err(err) => {
                                notify_admin(&format!(
                                    "Failed to get callback from CallbackInfo, err: {err}"
                                ))
                                .await;
                                return None;
                            }
                        };
                        let ci = match ci {
                            Some(ci) => ci,
                            None => return None,
                        };

                        let data = match ci.literal {
                            Some(data) => data,
                            None => return None,
                        };

                        let r = r.lock().expect("RwLock lock on commands map failed");
                        let rc = &r.rc;
                        rc.get_callback_message(&data)
                    }
                })
                .endpoint(handle_callback),
        )
}

async fn handle_botmessage<P: Provider>(
    bot: Bot,
    mut db: DB,
    bm: BotMessage<P>,
    msg: Message,
) -> BotResult<()> {
    // info!("Eval BM: {:?}", bm);
    let tguser = match msg.from.clone() {
        Some(user) => user,
        None => return Ok(()), // do nothing, cause its not usecase of function
    };
    let user = db
        .get_or_init_user(tguser.id.0 as i64, &tguser.first_name)
        .await?;
    let user = update_user_tg(user, &tguser);
    user.update_user(&mut db).await?;

    let variant = match BotCommand::from_str(msg.text().unwrap_or("")) {
        Ok(cmd) => cmd.args().map(|m| m.to_string()),
        Err(_) => None,
    };

    if bm.meta() {
        if let Some(ref meta) = variant {
            user.insert_meta(&mut db, meta).await?;
        };
    };

    let is_propagate: bool = match bm.get_handler() {
        Some(handler) => 'prop: {
            // let ctx = match handler.context() {
            //     Some(ctx) => ctx,
            //     // falling back to propagation
            //     None => break 'prop true,
            // };
            // let jsuser = to_js(ctx, &tguser).map_err(ScriptError::from)?;
            let puser = <P::Value as ProviderSerialize>::se_from(&tguser).unwrap();
            let mi = MessageInfoBuilder::new()
                .set_variant(variant.clone())
                .build();
            // let mi = to_js(ctx, &mi).map_err(ScriptError::from)?;
            let pmi = <P::Value as ProviderSerialize>::se_from(&mi).unwrap();
            // info!(
            //     "Calling handler {:?} with msg literal: {:?}",
            //     handler,
            //     bm.literal()
            // );
            match handler.call_args(vec![puser, pmi]) {
                Ok(v) => {
                    todo!()
                    // if v.is_bool() {
                    //     v.to_bool().unwrap_or(true)
                    // } else if v.is_int() {
                    //     v.to_int().unwrap_or(1) != 0
                    // } else {
                    //     // falling back to propagation
                    //     true
                    // }
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

    let button_db = db.clone();
    let buttons = bm.resolve_buttons(&mut db).await?.map(async |buttons| {
        join_all(buttons.iter().map(async |r| {
            join_all(r.iter().map(async |b| {
                match b {
                    ButtonLayout::Callback {
                        name,
                        literal: _,
                        callback,
                    } => {
                        callback_button(
                            name,
                            callback.to_string(),
                            None::<bool>,
                            &mut button_db.clone(),
                        )
                        .await
                    }
                }
            }))
            .await
            .into_iter()
            .collect::<Result<_, _>>()
        }))
        .await
        .into_iter()
        .collect::<Result<_, _>>()
    });
    let buttons = match buttons {
        Some(b) => Some(InlineKeyboardMarkup {
            inline_keyboard: b.await?,
        }),
        None => None,
    };
    let literal = bm.literal().map_or("", |s| s.as_str());

    let ma = MessageAnswerer::new(&bot, &mut db, msg.chat.id.0);
    ma.answer(literal, variant.as_deref(), buttons).await?;

    Ok(())
}

async fn handle_callback<P: Provider>(
    bot: Bot,
    mut db: DB,
    bm: BotMessage<P>,
    q: CallbackQuery,
) -> BotResult<()> {
    bot.answer_callback_query(&q.id).await?;
    // info!("Eval BM: {:?}", bm);
    let tguser = q.from.clone();
    let user = db
        .get_or_init_user(tguser.id.0 as i64, &tguser.first_name)
        .await?;
    let user = update_user_tg(user, &tguser);
    user.update_user(&mut db).await?;

    let is_propagate: bool = match bm.get_handler() {
        Some(handler) => 'prop: {
            let puser = <P::Value as ProviderSerialize>::se_from(&tguser).unwrap();
            let mi = MessageInfoBuilder::new().build();
            let pmi = <P::Value as ProviderSerialize>::se_from(&mi).unwrap();
            match handler.call_args(vec![puser, pmi]) {
                Ok(v) => {
                    todo!()
                    // if v.is_bool() {
                    //     v.to_bool().unwrap_or(true)
                    // } else if v.is_int() {
                    //     v.to_int().unwrap_or(1) != 0
                    // } else {
                    //     // falling back to propagation
                    //     true
                    // }
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

    let button_db = db.clone();
    let buttons = bm.resolve_buttons(&mut db).await?.map(async |buttons| {
        join_all(buttons.iter().map(async |r| {
            join_all(r.iter().map(async |b| {
                match b {
                    ButtonLayout::Callback {
                        name,
                        literal: _,
                        callback,
                    } => {
                        callback_button(
                            name,
                            callback.to_string(),
                            None::<bool>,
                            &mut button_db.clone(),
                        )
                        .await
                    }
                }
            }))
            .await
            .into_iter()
            .collect::<Result<_, _>>()
        }))
        .await
        .into_iter()
        .collect::<Result<_, _>>()
    });
    let buttons = match buttons {
        Some(b) => Some(InlineKeyboardMarkup {
            inline_keyboard: b.await?,
        }),
        None => None,
    };
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
                Err(_) => {
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
