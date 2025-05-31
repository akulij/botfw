pub mod parcelable;

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
                    $buttons.into_iter().collect::<Vec<_>>(),
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

    #[test]
    fn test_buttons_markup() {
        let button1 = InlineKeyboardButton::new(
            "Button 1",
            teloxide::types::InlineKeyboardButtonKind::CallbackData("callback1".into()),
        );
        let button2 = InlineKeyboardButton::new(
            "Button 2",
            teloxide::types::InlineKeyboardButtonKind::CallbackData("callback2".into()),
        );

        let markup = buttons_markup!([button1.clone(), button2.clone()], [button1.clone()]);

        assert_eq!(markup.inline_keyboard.len(), 2);
        assert_eq!(markup.inline_keyboard[0][0].text, "Button 1");
        assert_eq!(markup.inline_keyboard[0][1].text, "Button 2");
        assert_eq!(markup.inline_keyboard[1][0].text, "Button 1");
    }
}
