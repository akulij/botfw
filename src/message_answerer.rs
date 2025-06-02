use log::{info, warn};
use teloxide::prelude::*;
use teloxide::types::{
    InputFile, InputMedia, InputMediaPhoto, InputMediaVideo, MessageId, ParseMode,
};
use teloxide::{
    types::{ChatId, InlineKeyboardMarkup},
    Bot,
};

use crate::db::Media;
use crate::{
    db::{CallDB, DB},
    notify_admin, BotResult,
};

macro_rules! send_media {
    ($self:ident, $method:ident, $chat_id:expr, $file_id: expr, $text: expr, $keyboard: expr) => {{
        let msg = $self
            .bot
            .$method(ChatId($chat_id), InputFile::file_id($file_id.to_string()));
        let msg = match $text.as_str() {
            "" => msg,
            text => msg.caption(text),
        };
        let msg = match $keyboard {
            Some(kbd) => msg.reply_markup(kbd),
            None => msg,
        };
        let msg = msg.parse_mode(teloxide::types::ParseMode::Html);

        let msg = msg.await?;
        Ok((msg.chat.id.0, msg.id.0))
    }};
}

pub struct MessageAnswerer<'a> {
    bot: &'a Bot,
    chat_id: i64,
    db: &'a mut DB,
}

impl<'a> MessageAnswerer<'a> {
    pub fn new(bot: &'a Bot, db: &'a mut DB, chat_id: i64) -> Self {
        Self { bot, chat_id, db }
    }

    async fn get_text(
        &mut self,
        literal: &str,
        variant: Option<&str>,
        is_replace: bool,
    ) -> BotResult<String> {
        let variant_text = match variant {
            Some(variant) => {
                let value = self
                    .db
                    .get_literal_alternative_value(literal, variant)
                    .await?;
                if value.is_none() && !is_replace {
                    notify_admin(&format!("variant {variant} for literal {literal} is not found! falling back to just literal")).await;
                }
                value
            }
            None => None,
        };
        let text = match variant_text {
            Some(text) => text,
            None => self
                .db
                .get_literal_value(literal)
                .await?
                .unwrap_or("Please, set content of this message".into()),
        };

        Ok(text)
    }

    pub async fn answer(
        mut self,
        literal: &str,
        variant: Option<&str>,
        keyboard: Option<InlineKeyboardMarkup>,
    ) -> BotResult<(i64, i32)> {
        let text = self.get_text(literal, variant, false).await?;
        self.answer_inner(text, literal, variant, keyboard).await
    }

    pub async fn answer_text(
        self,
        text: String,
        keyboard: Option<InlineKeyboardMarkup>,
    ) -> BotResult<(i64, i32)> {
        self.send_message(text, keyboard).await
    }

    async fn answer_inner(
        mut self,
        text: String,
        literal: &str,
        variant: Option<&str>,
        keyboard: Option<InlineKeyboardMarkup>,
    ) -> BotResult<(i64, i32)> {
        let media = self.db.get_media(literal).await?;
        let (chat_id, msg_id) = match media.len() {
            // just a text
            0 => self.send_message(text, keyboard).await?,
            // single media
            1 => self.send_media(&media[0], text, keyboard).await?,
            // >= 2, should use media group
            _ => self.send_media_group(media, text).await?,
        };
        self.store_message_info(msg_id, literal, variant).await?;
        Ok((chat_id, msg_id))
    }

    pub async fn replace_message(
        mut self,
        message_id: i32,
        literal: &str,
        keyboard: Option<InlineKeyboardMarkup>,
    ) -> BotResult<()> {
        let variant = self
            .db
            .get_message(self.chat_id, message_id)
            .await?
            .and_then(|m| m.variant);
        let text = self.get_text(literal, variant.as_deref(), true).await?;
        let media = self.db.get_media(literal).await?;
        let (chat_id, msg_id) = match media.len() {
            // just a text
            0 => {
                let msg =
                    self.bot
                        .edit_message_text(ChatId(self.chat_id), MessageId(message_id), &text);
                let msg = match keyboard {
                    Some(ref kbd) => msg.reply_markup(kbd.clone()),
                    None => msg,
                };
                let msg = msg.parse_mode(teloxide::types::ParseMode::Html);
                info!("ENTS: {:?}", msg.entities);
                let msg = match msg.await {
                    Ok(msg) => msg,
                    Err(teloxide::RequestError::Api(teloxide::ApiError::Unknown(errtext)))
                        if errtext.as_str()
                            == "Bad Request: there is no text in the message to edit" =>
                    {
                        // fallback to sending message
                        warn!("Fallback into sending message instead of editing because it contains media");
                        self.answer_inner(text, literal, variant.as_deref(), keyboard)
                            .await?;
                        return Ok(());
                    }
                    Err(err) => return Err(err.into()),
                };

                (msg.chat.id.0, msg.id.0)
            }
            // single media
            1 => {
                let media = &media[0]; // safe, cause we just checked len
                let input_file = InputFile::file_id(media.file_id.to_string());
                let media = match media.media_type.as_str() {
                    "photo" => InputMedia::Photo(teloxide::types::InputMediaPhoto::new(input_file)),
                    "video" => InputMedia::Video(teloxide::types::InputMediaVideo::new(input_file)),
                    _ => todo!(),
                };
                self.bot
                    .edit_message_media(ChatId(self.chat_id), MessageId(message_id), media)
                    .await?;

                let msg = self
                    .bot
                    .edit_message_caption(ChatId(self.chat_id), MessageId(message_id));
                let msg = match text.as_str() {
                    "" => msg,
                    text => msg.caption(text),
                };
                let msg = match keyboard {
                    Some(kbd) => msg.reply_markup(kbd),
                    None => msg,
                };

                let msg = msg.parse_mode(teloxide::types::ParseMode::Html);
                let msg = msg.await?;

                (msg.chat.id.0, msg.id.0)
            }
            // >= 2, should use media group
            _ => {
                todo!();
            }
        };

        self.store_message_info(msg_id, literal, variant.as_deref())
            .await?;

        Ok(())
    }

    async fn store_message_info(
        &mut self,
        message_id: i32,
        literal: &str,
        variant: Option<&str>,
    ) -> BotResult<()> {
        match variant {
            Some(variant) => {
                self.db
                    .set_message_literal_variant(self.chat_id, message_id, literal, variant)
                    .await?
            }
            None => {
                self.db
                    .set_message_literal(self.chat_id, message_id, literal)
                    .await?
            }
        };

        Ok(())
    }

    async fn send_message(
        &self,
        text: String,
        keyboard: Option<InlineKeyboardMarkup>,
    ) -> BotResult<(i64, i32)> {
        let msg = self.bot.send_message(ChatId(self.chat_id), text);
        let msg = match keyboard {
            Some(kbd) => msg.reply_markup(kbd),
            None => msg,
        };
        let msg = msg.parse_mode(teloxide::types::ParseMode::Html);
        info!("ENTS: {:?}", msg.entities);
        let msg = msg.await?;

        Ok((msg.chat.id.0, msg.id.0))
    }

    async fn send_media(
        &self,
        media: &Media,
        text: String,
        keyboard: Option<InlineKeyboardMarkup>,
    ) -> BotResult<(i64, i32)> {
        match media.media_type.as_str() {
            "photo" => {
                send_media!(
                    self,
                    send_photo,
                    self.chat_id,
                    media.file_id,
                    text,
                    keyboard
                )
            }
            "video" => {
                send_media!(
                    self,
                    send_video,
                    self.chat_id,
                    media.file_id,
                    text,
                    keyboard
                )
            }
            _ => {
                todo!()
            }
        }
    }

    async fn send_media_group(&self, media: Vec<Media>, text: String) -> BotResult<(i64, i32)> {
        let media: Vec<InputMedia> = media
            .into_iter()
            .enumerate()
            .map(|(i, m)| {
                let ifile = InputFile::file_id(m.file_id);
                let caption = if i == 0 {
                    match text.as_str() {
                        "" => None,
                        text => Some(text.to_string()),
                    }
                } else {
                    None
                };
                match m.media_type.as_str() {
                    "photo" => InputMedia::Photo(InputMediaPhoto {
                        caption,
                        parse_mode: Some(ParseMode::Html),
                        ..InputMediaPhoto::new(ifile)
                    }),
                    "video" => InputMedia::Video(InputMediaVideo {
                        caption,
                        parse_mode: Some(ParseMode::Html),
                        ..InputMediaVideo::new(ifile)
                    }),
                    _ => {
                        todo!()
                    }
                }
            })
            .collect();
        let msg = self.bot.send_media_group(ChatId(self.chat_id), media);

        let msg = msg.await?;

        Ok((msg[0].chat.id.0, msg[0].id.0))
    }
}
