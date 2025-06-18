use futures::future::join_all;
use serde::{Deserialize, Serialize};

use crate::{
    config::{function::BotFunction, result::ConfigResult, traits::ResolveValue, Provider},
    db::DB,
};

use super::{button::ButtonLayout, keyboard::KeyboardDefinition};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BotMessage<P: Provider> {
    // buttons: Vec<Button>
    literal: Option<String>,
    #[serde(default)]
    replace: bool,
    buttons: Option<KeyboardDefinition<P>>,
    state: Option<String>,

    /// flag options to command is meta, so it will be appended to user.metas in db
    meta: Option<bool>,

    handler: Option<BotFunction<P>>,
}

impl<P: Provider> BotMessage<P> {
    pub fn fill_literal(self, l: String) -> Self {
        BotMessage {
            literal: self.clone().literal.or(Some(l)),
            ..self
        }
    }

    /// chain of modifications on BotMessage
    pub fn update_defaults(self) -> Self {
        let bm = self;
        // if message is `start`, defaulting meta to true, if not set

        match bm.meta {
            Some(_) => bm,
            None => match &bm.literal {
                Some(l) if l == "start" => Self {
                    meta: Some(true),
                    ..bm
                },
                _ => bm,
            },
        }
    }

    pub fn is_replace(&self) -> bool {
        self.replace
    }

    pub fn get_handler(&self) -> Option<&BotFunction<P>> {
        self.handler.as_ref()
    }

    pub fn meta(&self) -> bool {
        self.meta.unwrap_or(false)
    }
}

impl<P: Provider> BotMessage<P> {
    pub async fn resolve_buttons(
        &self,
        db: &mut DB,
    ) -> ConfigResult<Option<Vec<Vec<ButtonLayout>>>> {
        let raw_buttons = self.buttons.clone().map(|b| b.resolve()).transpose()?;
        match raw_buttons {
            Some(braws) => {
                let kbd: Vec<Vec<_>> = join_all(braws.into_iter().map(|rows| async {
                    join_all(rows.into_iter().map(|b| async {
                        let mut db = db.clone();
                        ButtonLayout::resolve_raw(b, &mut db).await
                    }))
                    .await
                    .into_iter()
                    .collect()
                }))
                .await
                .into_iter()
                .collect::<Result<_, _>>()?;
                Ok(Some(kbd))
            }
            None => Ok(None),
        }
    }

    pub fn literal(&self) -> Option<&String> {
        self.literal.as_ref()
    }
}
