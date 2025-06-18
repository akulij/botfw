use std::error::Error;

use crate::db::DbError;

#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("error from DB: {0:?}")]
    DBError(#[from] DbError),
    #[error("error from runtime provider: {0:?}")]
    ProviderError(String),
    // #[error(transparent)]
    // Other(#[from] anyhow::Error),
    #[error("error other: {0:?}")]
    Other(String),
}

pub type ConfigResult<T> = Result<T, ConfigError>;
//
// impl<P: Provider> From<P::Error> for ConfigError<P> {
//     fn from(value: P::Error) -> Self {
//         ConfigError::ProviderError(value)
//     }
// }

impl ConfigError {
    pub fn as_provider_err(err: impl Error) -> Self {
        Self::ProviderError(format!("ProviderError: {err}"))
    }
}
