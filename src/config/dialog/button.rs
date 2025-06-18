use serde::{Deserialize, Serialize};

use crate::{
    config::{
        function::BotFunction,
        result::{ConfigError, ConfigResult},
        traits::{ProviderDeserialize, ResolveValue},
        Provider,
    },
    db::{CallDB, DB},
    notify_admin,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum ButtonDefinition<P: Provider> {
    Button(ButtonRaw),
    ButtonLiteral(String),
    Function(BotFunction<P>),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ButtonRaw {
    name: ButtonName,
    callback_name: String,
}

impl ButtonRaw {
    pub fn from_literal(literal: String) -> Self {
        ButtonRaw {
            name: ButtonName::Literal {
                literal: literal.clone(),
            },
            callback_name: literal,
        }
    }

    pub fn name(&self) -> &ButtonName {
        &self.name
    }

    pub fn callback_name(&self) -> &str {
        &self.callback_name
    }

    pub fn literal(&self) -> Option<String> {
        match self.name() {
            ButtonName::Value { .. } => None,
            ButtonName::Literal { literal } => Some(literal.to_string()),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum ButtonName {
    Value { name: String },
    Literal { literal: String },
}

impl ButtonName {
    pub async fn resolve_name(self, db: &mut DB) -> ConfigResult<String> {
        match self {
            ButtonName::Value { name } => Ok(name),
            ButtonName::Literal { literal } => {
                let value = db.get_literal_value(&literal).await?;

                Ok(match value {
                    Some(value) => Ok(value),
                    None => {
                        notify_admin(&format!("Literal `{literal}` is not set!!!")).await;
                        Err(ConfigError::Other(format!(
                            "not found literal `{literal}` in DB"
                        )))
                    }
                }?)
            }
        }
    }
}

pub enum ButtonLayout {
    Callback {
        name: String,
        literal: Option<String>,
        callback: String,
    },
}

impl ButtonLayout {
    pub async fn resolve_raw(braw: ButtonRaw, db: &mut DB) -> ConfigResult<Self> {
        let name = braw.name().clone().resolve_name(db).await?;
        let literal = braw.literal();
        let callback = braw.callback_name().to_string();
        Ok(Self::Callback {
            name,
            literal,
            callback,
        })
    }
}

impl<P: Provider> ResolveValue for ButtonDefinition<P> {
    type Value = ButtonRaw;
    type Runtime = P;

    fn resolve(self) -> ConfigResult<Self::Value> {
        match self {
            ButtonDefinition::Button(button) => Ok(button),
            ButtonDefinition::ButtonLiteral(l) => Ok(ButtonRaw::from_literal(l)),
            ButtonDefinition::Function(f) => <Self as ResolveValue>::resolve(match f.call()? {
                Some(t) => Ok(t.de_into().map_err(ConfigError::as_provider_err)?),
                None => Err(ConfigError::Other(
                    "Function didn't return value".to_string(),
                )),
            }?),
        }
    }
}
