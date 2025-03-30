use teloxide::prelude::*;
use envconfig::Envconfig;

#[derive(Envconfig)]
struct Config {
    #[envconfig(from = "BOT_TOKEN")]
    pub bot_token: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>{
    dotenvy::dotenv()?;

    let bot = Bot::from_env();

    teloxide::repl(bot, |bot: Bot, msg: Message| async move {
        bot.send_dice(msg.chat.id).await?;
        Ok(())
    })
    .await;

    Ok(())
}
