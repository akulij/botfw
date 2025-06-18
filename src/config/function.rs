use serde::{Deserialize, Serialize};

use super::{
    result::{ConfigError, ConfigResult},
    traits::ProviderCall,
    Provider,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BotFunction<P: Provider>(P::Function);

impl<P: Provider> BotFunction<P> {
    pub fn call(&self) -> ConfigResult<Option<P::Value>> {
        self.call_args(&[])
    }

    pub fn call_args(&self, args: &[&P::Value]) -> ConfigResult<Option<P::Value>> {
        let val = ProviderCall::call(&self.0, args).map_err(ConfigError::as_provider_err)?;
        Ok(val)
    }
}
