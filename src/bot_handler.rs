use log::info;
use std::{
    str::FromStr,
    sync::{Arc, RwLock},
};
use teloxide::{
    dispatching::UpdateFilterExt,
    dptree::{self, Handler},
    prelude::DependencyMap,
    types::{InlineKeyboardButton, InlineKeyboardMarkup, Message, Update},
    Bot,
};

use crate::{
    botscript::{self, BotMessage, RunnerConfig},
    commands::BotCommand,
    db::{CallDB, DB},
    message_answerer::MessageAnswerer,
    update_user_tg, BotResult,
};

pub type BotHandler =
    Handler<'static, DependencyMap, BotResult<()>, teloxide::dispatching::DpHandlerDescription>;

pub fn script_handler(rc: Arc<RwLock<RunnerConfig>>) -> BotHandler {
    dptree::entry().branch(
        Update::filter_message()
            // check if message is command
            .filter_map(|m: Message| m.text().and_then(|t| BotCommand::from_str(t).ok()))
            // check if command is presented in config
            .filter_map(move |bc: BotCommand| {
                let rc = std::sync::Arc::clone(&rc);
                let command = bc.command();

                let rc = rc.read().expect("RwLock lock on commands map failed");

                rc.get_command_message(command)
            })
            .endpoint(botscript_command_handler),
    )
}

async fn botscript_command_handler(
    bot: Bot,
    mut db: DB,
    bm: BotMessage,
    msg: Message,
) -> BotResult<()> {
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
