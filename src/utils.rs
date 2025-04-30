use serde::{Deserialize, Serialize};
use teloxide::types::InlineKeyboardButton;

use crate::{
    db::{callback_info::CallbackInfo, CallDB},
    BotResult,
};

macro_rules! single_button_markup {
    ($button:expr) => {
        InlineKeyboardMarkup {
            inline_keyboard: vec![vec![$button]],
        }
    };
}

pub async fn create_callback_button<C, D>(
    literal: &str,
    ci: CallbackInfo<C>,
    db: &mut D,
) -> BotResult<InlineKeyboardButton>
where
    C: Serialize + for<'a> Deserialize<'a> + Send + Sync,
    D: CallDB + Send,
{
    let text = db
        .get_literal_value(literal)
        .await?
        .unwrap_or("Please, set content of this message".into());
    let ci = ci.store(db).await?;

    Ok(InlineKeyboardButton::new(
        text,
        teloxide::types::InlineKeyboardButtonKind::CallbackData(ci.get_id()),
    ))
}
