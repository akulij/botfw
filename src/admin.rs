use teloxide::{
    prelude::*,
    utils::{command::BotCommands, render::RenderMessageTextHelper},
};

use crate::db::{CallDB, DB};
use crate::LogMsg;

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
}

pub async fn admin_command_handler(
    mut db: DB,
    bot: Bot,
    msg: Message,
    cmd: AdminCommands,
) -> Result<(), teloxide::RequestError> {
    let tguser = msg.from.clone().unwrap();
    println!("MSG: {}", msg.html_text().unwrap());
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
            db.set_admin(tguser.id.0 as i64, false).await;
            bot.send_message(msg.chat.id, "You are not an admin anymore")
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
) -> Result<(), teloxide::RequestError> {
    println!("Admin Pass: {}", admin_password);
    let tguser = msg.from.clone().unwrap();
    let user = db
        .get_or_init_user(tguser.id.0 as i64, &tguser.first_name)
        .await;
    println!("MSG: {}", msg.html_text().unwrap());
    match cmd {
        SecretCommands::Secret { pass } => {
            if user.is_admin == true {
                bot.send_message(msg.from.unwrap().id, "You are an admin already")
                    .await?;
            } else if pass == admin_password {
                db.set_admin(user.id, true).await;
                bot.send_message(msg.from.unwrap().id, "You are an admin now!")
                    .await?;
            }
            Ok(())
        }
    }
}
