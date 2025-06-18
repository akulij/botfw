use crate::config::result::ConfigResult;

use super::Provider;

pub trait ResolveValue {
    type Value;
    type Runtime: Provider;

    fn resolve(self) -> ConfigResult<Self::Value>;
}
