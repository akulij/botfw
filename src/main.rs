use teloxide::{payloads::SendMessageSetters, prelude::*, utils::render::RenderMessageTextHelper};
use envconfig::Envconfig;

#[derive(Envconfig)]
struct Config {
    #[envconfig(from = "BOT_TOKEN")]
    pub bot_token: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>{
    dotenvy::dotenv()?;
    let config = Config::init_from_env()?;

    let bot = Bot::new(config.bot_token);

    teloxide::repl(bot, |bot: Bot, msg: Message| async move {
        let msgtext = msg.html_text().unwrap();
        println!("Message HTML Text: {}", msgtext);

        let mut smsg = bot.send_message(msg.chat.id, msgtext);
        smsg = smsg.parse_mode(teloxide::types::ParseMode::Html);
        smsg.await?;
        Ok(())
    })
    .await;

    Ok(())
}
