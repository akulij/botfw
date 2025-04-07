pub mod admin;
pub mod db;

use crate::admin::{admin_command_handler, AdminCommands};
use crate::admin::{secret_command_handler, SecretCommands};
use crate::db::DB;

use chrono::{DateTime, Utc};
use chrono_tz::Asia;
use db::schema::events;
use envconfig::Envconfig;
use serde::{Deserialize, Serialize};
use teloxide::dispatching::dialogue::serializer::Json;
use teloxide::dispatching::dialogue::{InMemStorage, PostgresStorage};
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use teloxide::{
    payloads::SendMessageSetters,
    prelude::*,
    types::InputFile,
    utils::{command::BotCommands, render::RenderMessageTextHelper},
};

type BotDialogue = Dialogue<State, PostgresStorage<Json>>;

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
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv()?;
    let config = Config::init_from_env()?;

    let bot = Bot::new(&config.bot_token);
    let db = DB::new(&config.db_url).await;
    let db_url2 = config.db_url.clone();
    let state_mgr = PostgresStorage::open(&db_url2, 8, Json).await?;

    // TODO: delete this in production
    let events: Vec<DateTime<Utc>> = vec!["2025-04-09T18:00:00+04:00", "2025-04-11T16:00:00+04:00"]
        .iter()
        .map(|d| DateTime::parse_from_rfc3339(d).unwrap().into())
        .collect();

    for event in events {
        match db.clone().create_event(event).await {
            Ok(e) => println!("Created event {}", e.id),
            Err(err) => println!("Failed to create event, error: {}", err),
        }
    }
    //

    let handler = dptree::entry()
        .inspect(|u: Update| {
            eprintln!("{u:#?}"); // Print the update to the console with inspect
        })
        .branch(command_handler(config))
        .branch(
            Update::filter_message()
                .filter_async(async |msg: Message, mut db: DB| {
                    let user = db.get_or_init_user(msg.from.unwrap().id.0 as i64).await;
                    user.is_admin
                })
                .enter_dialogue::<Message, PostgresStorage<Json>, State>()
                .branch(
                    Update::filter_message()
                        .filter(|msg: Message| msg.text().unwrap_or("") == "edit")
                        .endpoint(edit_msg_cmd_handler),
                )
                .branch(dptree::case![State::Edit { literal, lang }].endpoint(edit_msg_handler)),
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
                .update(State::Edit { literal, lang })
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
    (literal, lang): (String, String),
    msg: Message,
) -> Result<(), teloxide::RequestError> {
    match msg.html_text() {
        Some(text) => {
            db.set_literal(&literal, &text).await.unwrap();
            bot.send_message(msg.chat.id, "Updated text of message!")
                .await
                .unwrap();
            dialogue.exit().await.unwrap();
        }
        None => {
            bot.send_message(msg.chat.id, "Send text!").await.unwrap();
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
                    let user = db.get_or_init_user(msg.from.unwrap().id.0 as i64).await;
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
    let user = db
        .get_or_init_user(msg.from.clone().unwrap().id.0 as i64)
        .await;
    println!("MSG: {}", msg.html_text().unwrap());
    match cmd {
        UserCommands::Start => {
            let literal = "start";
            let text = db
                .get_literal_value(literal)
                .await
                .unwrap()
                .unwrap_or("Please, set content of this message".into());
            let msg = bot
                .send_message(msg.chat.id, text)
                .reply_markup(make_start_buttons(&mut db).await)
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
            db.set_message_literal(msg.chat.id.0, msg.id.0, literal)
                .await
                .unwrap();
            Ok(())
        }
        UserCommands::Help => {
            bot.send_message(msg.chat.id, UserCommands::descriptions().to_string())
                .await?;
            Ok(())
        }
    }
}

async fn make_start_buttons(db: &mut DB) -> InlineKeyboardMarkup {
    let mut buttons: Vec<Vec<InlineKeyboardButton>> = db
        .get_all_events()
        .await
        .iter()
        .map(|e| {
            vec![InlineKeyboardButton::callback(
                e.time.with_timezone(&Asia::Dubai).to_string(),
                format!("event:{}", e.id),
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
