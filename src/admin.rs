use build_time::{build_time_local, build_time_utc};
use git_const::git_hash;
use itertools::Itertools;
use teloxide::{
    prelude::*,
    utils::{command::BotCommands, render::RenderMessageTextHelper},
};

use crate::{
    bot_manager::DEFAULT_SCRIPT,
    db::{bots::BotInstance, CallDB, DB},
    BotResult,
};
use crate::{BotDialogue, LogMsg, State};
use log::info;

// These are should not appear in /help
#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
pub enum SecretCommands {
    /// Activate admin mode
    Secret { pass: String },
}

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
pub enum AdminCommands {
    /// Shows your ID.
    MyId,
    /// Pin replied message
    Pin,
    /// Removes your admin privileges
    Deop,
    /// Send command and then click button to edits text in it
    EditButton,
    /// Set specified literal value
    SetLiteral { literal: String },
    /// Set specified literal value
    #[command(description = "handle a username and an age.", parse_with = "split")]
    SetAlternative { literal: String, variant: String },
    /// Sets chat where this message entered as support's chats
    SetChat,
    /// Shows user count and lists some of them
    Users,
    /// Cancel current action and sets user state to default
    Cancel,
    /// Create new instance of telegram bot
    Deploy { token: String },
    /// Get commit hash of this bot
    Commit,
}

pub async fn admin_command_handler(
    mut db: DB,
    bot: Bot,
    msg: Message,
    cmd: AdminCommands,
    dialogue: BotDialogue,
) -> BotResult<()> {
    let tguser = match msg.from.clone() {
        Some(user) => user,
        None => return Ok(()), // do nothing, cause its not usecase of function
    };
    info!(
        "MSG: {}",
        msg.html_text().unwrap_or("|EMPTY_MESSAGE|".into())
    );
    match cmd {
        AdminCommands::MyId => {
            bot.send_message(msg.chat.id, format!("Your ID is: {}", tguser.id))
                .log()
                .await?;
            Ok(())
        }
        AdminCommands::Pin => {
            if let Some(msg_to_pin) = msg.reply_to_message() {
                bot.pin_chat_message(msg.chat.id, msg_to_pin.id).await?;
            } else {
                bot.send_message(
                    msg.chat.id,
                    "you need to reply to some message with this command",
                )
                .log()
                .await?;
            }
            Ok(())
        }
        AdminCommands::Deop => {
            db.set_admin(tguser.id.0 as i64, false).await?;
            bot.send_message(msg.chat.id, "You are not an admin anymore")
                .await?;
            Ok(())
        }
        AdminCommands::EditButton => {
            dialogue.update(State::EditButton).await?;
            bot.send_message(msg.chat.id, "Click button which text should be edited")
                .await?;
            Ok(())
        }
        AdminCommands::SetLiteral { literal } => {
            dialogue
                .update(State::Edit {
                    literal,
                    variant: None,
                    lang: "ru".to_string(),
                    is_caption_set: false,
                })
                .await?;
            bot.send_message(msg.chat.id, "Send message for literal")
                .await?;

            Ok(())
        }
        AdminCommands::SetAlternative { literal, variant } => {
            dialogue
                .update(State::Edit {
                    literal,
                    variant: Some(variant),
                    lang: "ru".to_string(),
                    is_caption_set: false,
                })
                .await?;
            bot.send_message(msg.chat.id, "Send message for literal alternative")
                .await?;

            Ok(())
        }
        AdminCommands::SetChat => {
            dialogue.exit().await?;
            db.set_literal("support_chat_id", &msg.chat.id.0.to_string())
                .await?;
            bot.send_message(msg.chat.id, "ChatId is set!").await?;
            Ok(())
        }
        AdminCommands::Users => {
            let users = db.get_users().await?;
            let count = users.len();
            let user_list = users
                .into_iter()
                .take(5)
                .map(|u| {
                    format!(
                        "  {}{}{}",
                        u.first_name,
                        u.last_name.map_or("".into(), |l| format!(" {l}")),
                        u.username
                            .map_or("".into(), |username| format!(" (@{username})")),
                    )
                })
                .join("\n");

            bot.send_message(
                msg.chat.id,
                format!("Users count: {count}\nList:\n{user_list}"),
            )
            .await?;

            Ok(())
        }
        AdminCommands::Cancel => {
            dialogue.exit().await?;
            bot.send_message(msg.chat.id, "canceled current action")
                .await?;
            Ok(())
        }
        AdminCommands::Deploy { token } => {
            let bot_instance = {
                let botnew = Bot::new(&token);
                let name = match botnew.get_me().await {
                    Ok(me) => me.username().to_string(),
                    Err(teloxide::RequestError::Api(teloxide::ApiError::InvalidToken)) => {
                        bot.send_message(msg.chat.id, "Error: bot token is invalid")
                            .await?;
                        return Ok(());
                    }
                    Err(err) => {
                        return Err(err.into());
                    }
                };

                let bi =
                    BotInstance::new(name.clone(), token.to_string(), DEFAULT_SCRIPT.to_string())
                        .store(&mut db)
                        .await?;

                bi
            };

            bot.send_message(
                msg.chat.id,
                format!("Deployed bot with name: {}", bot_instance.name),
            )
            .await?;
            Ok(())
        }
        AdminCommands::Commit => {
            let hash = git_hash!();
            let built_utc = build_time_utc!("%H:%M %d.%m.%Y");
            let built_local = build_time_local!("%H:%M %d.%m.%Y");

            bot.send_message(
                msg.chat.id,
                format!("Commit: {hash}\nBuilt at (UTC): <b>{built_utc}</b>\n Local: <b>{built_local}</b>"),
            )
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
            Ok(())
        }
    }
}

pub async fn secret_command_handler(
    mut db: DB,
    //config: Config,
    bot: Bot,
    msg: Message,
    cmd: SecretCommands,
    admin_password: String,
) -> BotResult<()> {
    info!("Admin Pass: {}", admin_password);
    let tguser = match msg.from.clone() {
        Some(user) => user,
        None => return Ok(()), // do nothing, cause its not usecase of function
    };
    let user = db
        .get_or_init_user(tguser.id.0 as i64, &tguser.first_name)
        .await?;
    info!(
        "MSG: {}",
        msg.html_text().unwrap_or("|EMPTY_MESSAGE|".into())
    );
    match cmd {
        SecretCommands::Secret { pass } => {
            if user.is_admin {
                bot.send_message(tguser.id, "You are an admin already")
                    .await?;
            } else if pass == admin_password {
                db.set_admin(user.id, true).await?;
                bot.send_message(tguser.id, "You are an admin now!").await?;
            }
            Ok(())
        }
    }
}
