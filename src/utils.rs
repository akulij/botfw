use serde::{Deserialize, Serialize};
use teloxide::types::InlineKeyboardButton;

use crate::{
    db::{callback_info::CallbackInfo, CallDB},
    BotResult,
};

#[macro_export]
macro_rules! single_button_markup {
    ($button:expr) => {
        InlineKeyboardMarkup {
            inline_keyboard: vec![vec![$button]],
        }
    };
}

#[macro_export]
macro_rules! stacked_buttons_markup {
    ($( $button:expr ),+) => {
        InlineKeyboardMarkup {
            inline_keyboard: vec![
                $(
                    vec![$button],
                )*
            ],
        }
    };
}

#[macro_export]
macro_rules! buttons_markup {
    ($( $buttons:expr ),+) => {
        InlineKeyboardMarkup {
            inline_keyboard: vec![
                $(
                    //$buttons.into_iter().collect::<Vec<_>>(),
                    $buttons.to_vec(),
                )*
            ],
        }
    };
}

pub async fn create_callback_button<C, D>(
    literal: &str,
    callback: C,
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
    let ci = CallbackInfo::new_with_literal(callback, literal.to_string())
        .store(db)
        .await?;

    Ok(InlineKeyboardButton::new(
        text,
        teloxide::types::InlineKeyboardButtonKind::CallbackData(ci.get_id()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use teloxide::types::InlineKeyboardButton;
    use teloxide::types::InlineKeyboardMarkup;
}
