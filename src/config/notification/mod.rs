use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    config::{
        result::ConfigError,
        traits::{ProviderDeserialize, ProviderSerialize},
    },
    db::{CallDB, User, DB},
};

use super::{function::BotFunction, result::ConfigResult, time::NotificationTime, Provider};

pub mod batch;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BotNotification<P: Provider> {
    time: NotificationTime,
    #[serde(default)]
    filter: NotificationFilter<P>,
    message: NotificationMessage<P>,
}

impl<P: Provider> BotNotification<P> {
    pub fn left_time(&self, start_time: DateTime<Utc>, now: DateTime<Utc>) -> Duration {
        let next = self.time.when_next(start_time, now);

        // immidate notification if time to do it passed
        let duration = (next - now).to_std().unwrap_or(Duration::from_secs(0));

        // Rounding partitions of seconds
        Duration::from_secs(duration.as_secs())
    }

    pub async fn get_users(&self, db: &DB) -> ConfigResult<Vec<User>> {
        self.filter.get_users(db).await
    }
    pub async fn resolve_message(&self, db: &DB, user: &User) -> ConfigResult<Option<String>> {
        self.message.resolve(db, user).await
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(untagged)]
pub enum NotificationFilter<P: Provider> {
    #[default]
    #[serde(rename = "all")]
    All,
    /// Send to randomly selected N people
    Random { random: u32 },
    /// Function that returns list of user id's who should get notification
    BotFunction(BotFunction<P>),
}

impl<P: Provider> NotificationFilter<P> {
    pub async fn get_users(&self, db: &DB) -> ConfigResult<Vec<User>> {
        match self {
            NotificationFilter::All => Ok(db.get_users().await?),
            NotificationFilter::Random { random } => Ok(db.get_random_users(*random).await?),
            NotificationFilter::BotFunction(f) => {
                let uids = match f.call()? {
                    Some(t) => Ok(t),
                    None => Err(ConfigError::Other(
                        "Function didn't return value".to_string(),
                    )),
                }?;
                let uids: Vec<i64> = uids.de_into().map_err(ConfigError::as_provider_err)?;
                let users = db.get_users_by_ids(uids).await?;

                Ok(users)
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum NotificationMessage<P: Provider> {
    Literal {
        literal: String,
    },
    Text {
        text: String,
    },
    /// Function can accept user which will be notified and then return generated message
    BotFunction(BotFunction<P>),
}

impl<P: Provider> NotificationMessage<P> {
    pub async fn resolve(&self, db: &DB, user: &User) -> ConfigResult<Option<String>> {
        match self {
            NotificationMessage::Literal { literal } => Ok(db.get_literal_value(literal).await?),
            NotificationMessage::Text { text } => Ok(Some(text.to_string())),
            NotificationMessage::BotFunction(f) => {
                let puser = <P::Value as ProviderSerialize>::se_from(user)
                    .map_err(ConfigError::as_provider_err)?;
                let text = match f.call_args(&[&puser])? {
                    Some(t) => t.de_into().map_err(ConfigError::as_provider_err)?,
                    None => None,
                };
                Ok(text)
            }
        }
    }
}
