pub mod db;
use crate::db::DB;

use teloxide::{dispatching::dialogue::GetChatId, payloads::SendMessageSetters, prelude::*, utils::{command::BotCommands, render::RenderMessageTextHelper}};
use envconfig::Envconfig;

#[derive(Envconfig)]
struct Config {
    #[envconfig(from = "BOT_TOKEN")]
    pub bot_token: String,
    #[envconfig(from = "DATABASE_URL")]
    pub db_url: String,
}

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
enum UserCommands {
    /// The first message of user
    Start,
    /// Shows this message.
    Help,
}

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
enum AdminCommands {
    /// Shows your ID.
    MyId,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>{
    dotenvy::dotenv()?;
    let config = Config::init_from_env()?;

    let bot = Bot::new(&config.bot_token);
    let db = DB::new(&config.db_url).await;

    let handler = dptree::entry()
        .inspect(|u: Update| {
            //eprintln!("{u:#?}"); // Print the update to the console with inspect
        })
        .branch(
            Update::filter_message()
            .branch(
                dptree::entry().filter_command::<UserCommands>().endpoint(user_command_handler)
            )
            .branch(
                dptree::entry().filter_async(async |msg: Message, mut db: DB| {
                    let user = db.get_or_init_user(msg.from.unwrap().id.0 as i64).await;
                    user.is_admin
                }).filter_command::<AdminCommands>().endpoint(admin_command_handler)
            )
        )
        .branch(
            Update::filter_message().endpoint(echo)
        )
        ;

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![db])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}

async fn user_command_handler(
    mut db: DB,
    bot: Bot,
    msg: Message,
    cmd: UserCommands,
) -> Result<(), teloxide::RequestError> {
    let user = db.get_or_init_user(msg.from.clone().unwrap().id.0 as i64);
    println!("MSG: {}", msg.html_text().unwrap());
    match cmd {
        UserCommands::Help => {
            bot.send_message(msg.chat.id, UserCommands::descriptions().to_string()).await?;
            Ok(())
        },
        _ => {
            bot.send_message(msg.chat.id, "Not yet implemented").await?;
            Ok(())
        }
    }
}

async fn admin_command_handler(
    mut db: DB,
    bot: Bot,
    msg: Message,
    cmd: AdminCommands,
) -> Result<(), teloxide::RequestError> {
    let tguser = msg.from.clone().unwrap();
    let user = db.get_or_init_user(tguser.id.0 as i64);
    println!("MSG: {}", msg.html_text().unwrap());
    match cmd {
        AdminCommands::MyId => {
            bot.send_message(msg.chat.id, format!("Your ID is: {}", tguser.id)).await?;
            Ok(())
        }
        _ => {
            bot.send_message(msg.chat.id, "Not yet implemented").await?;
            Ok(())
        }
    }
}

async fn echo(
    bot: Bot,
    msg: Message,
) -> Result<(), teloxide::RequestError> {
    bot.send_message(msg.chat.id, msg.html_text().unwrap()).parse_mode(teloxide::types::ParseMode::Html).await?;
    Ok(())
}
