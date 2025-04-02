pub mod db;
pub mod admin;

use crate::db::DB;
use crate::admin::{AdminCommands, admin_command_handler};

use teloxide::{dispatching::dialogue::GetChatId, payloads::SendMessageSetters, prelude::*, types::InputFile, utils::{command::BotCommands, render::RenderMessageTextHelper}};
use envconfig::Envconfig;

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

// These are should not appear in /help
#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
enum SecretCommands {
    /// Activate admin mode
    Secret {
        pass: String
    },
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>{
    dotenvy::dotenv()?;
    let config = Config::init_from_env()?;

    let bot = Bot::new(&config.bot_token);
    let db = DB::new(&config.db_url).await;

    let handler = dptree::entry()
        .inspect(|u: Update| {
            eprintln!("{u:#?}"); // Print the update to the console with inspect
        })
        .branch(
            Update::filter_message()
            .branch(
                dptree::entry().filter_command::<UserCommands>().endpoint(user_command_handler)
            )
            .branch(
                dptree::entry().filter_command::<SecretCommands>()
                    .map(move || config.admin_password.clone())
                    .endpoint(secret_command_handler)
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
        UserCommands::Start => {
            bot.send_photo(msg.chat.id, InputFile::file_id("AgACAgIAAxkBAANRZ-2EJWUdkgwG4tfJfNwut4bssVkAAunyMRvTJ2FLn4FTtVdyfOoBAAMCAANzAAM2BA")).await?;
            Ok(())
        },
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

async fn secret_command_handler(
    mut db: DB,
    //config: Config,
    bot: Bot,
    msg: Message,
    cmd: SecretCommands,
    admin_password: String
) -> Result<(), teloxide::RequestError> {
    println!("Admin Pass: {}", admin_password);
    let user = db.get_or_init_user(msg.from.clone().unwrap().id.0 as i64).await;
    println!("MSG: {}", msg.html_text().unwrap());
    match cmd {
        SecretCommands::Secret { pass } => {
            if user.is_admin == true {
                bot.send_message(msg.from.unwrap().id, "You are an admin already").await?;
            } else if pass == admin_password {
                db.set_admin(user.id, true).await;
                bot.send_message(msg.from.unwrap().id, "You are admin now!").await?;
            }
            Ok(())
        },
    }
}

async fn echo(
    bot: Bot,
    msg: Message,
) -> Result<(), teloxide::RequestError> {
    if let Some(photo) = msg.photo() {
        println!("File ID: {}", photo[0].file.id);
    }
    bot.send_message(msg.chat.id, msg.html_text().unwrap_or("UNWRAP".into())).parse_mode(teloxide::types::ParseMode::Html).await?;
    Ok(())
}
