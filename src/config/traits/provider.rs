use std::fmt::Debug;

use serde::{Deserialize, Serialize};
use std::error::Error;

use crate::config::RunnerConfig;

pub trait Provider: Clone {
    type Function: ProviderCall<Provider = Self>
        + Serialize
        + for<'a> Deserialize<'a>
        + Debug
        + Send
        + Sync
        + Clone;
    type Value: ProviderDeserialize<Provider = Self>
        + ProviderSerialize<Provider = Self>
        + Serialize
        + for<'a> Deserialize<'a>
        + Debug
        + Send
        + Sync
        + Clone;
    type Error: Error;

    type InitData;
    fn init_config(&self, d: Self::InitData) -> Result<RunnerConfig<Self>, Self::Error>;
}

pub trait ProviderCall {
    type Provider: Provider<Function = Self>;
    fn call(
        &self,
        args: &[&<Self::Provider as Provider>::Value],
    ) -> Result<Option<<Self::Provider as Provider>::Value>, <Self::Provider as Provider>::Error>;
}

pub trait ProviderDeserialize {
    type Provider: Provider<Value = Self>;
    // fn de_into<T: for Deserialize>(&self) -> Result<T, <Self::Provider as Provider>::Error>;
    fn de_into<T>(&self) -> Result<T, <Self::Provider as Provider>::Error>;
}

pub trait ProviderSerialize {
    type Provider: Provider<Value = Self>;
    // fn de_into<T: for Deserialize>(&self) -> Result<T, <Self::Provider as Provider>::Error>;
    fn se_from<T: Serialize>(from: &T) -> Result<Self, <Self::Provider as Provider>::Error>
    where
        Self: Sized;
}
