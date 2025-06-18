use serde::{Deserialize, Serialize};

use crate::config::{
    function::BotFunction,
    result::{ConfigError, ConfigResult},
    traits::{ProviderDeserialize, ResolveValue},
    Provider,
};

use super::button::ButtonDefinition;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum KeyboardDefinition<P: Provider> {
    Rows(Vec<RowDefinition<P>>),
    Function(BotFunction<P>),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum RowDefinition<P: Provider> {
    Buttons(Vec<ButtonDefinition<P>>),
    Function(BotFunction<P>),
}

impl<P: Provider> ResolveValue for KeyboardDefinition<P> {
    type Value = Vec<<RowDefinition<P> as ResolveValue>::Value>;
    type Runtime = P;

    fn resolve(self) -> ConfigResult<Self::Value> {
        match self {
            KeyboardDefinition::Rows(rows) => rows.into_iter().map(|r| r.resolve()).collect(),
            KeyboardDefinition::Function(f) => <Self as ResolveValue>::resolve(match f.call()? {
                Some(t) => Ok(t.de_into().map_err(ConfigError::as_provider_err)?),
                None => Err(ConfigError::Other(
                    "Function didn't return value".to_string(),
                )),
            }?),
        }
    }
}

impl<P: Provider> ResolveValue for RowDefinition<P> {
    type Value = Vec<<ButtonDefinition<P> as ResolveValue>::Value>;
    type Runtime = P;

    fn resolve(self) -> ConfigResult<Self::Value> {
        match self {
            RowDefinition::Buttons(buttons) => buttons.into_iter().map(|b| b.resolve()).collect(),
            RowDefinition::Function(f) => <Self as ResolveValue>::resolve(match f.call()? {
                Some(t) => Ok(t.de_into().map_err(ConfigError::as_provider_err)?),
                None => Err(ConfigError::Other(
                    "Function didn't return value".to_string(),
                )),
            }?),
        }
    }
}
