use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct MessageInfo {
    variant: Option<String>,
}

pub struct MessageInfoBuilder {
    inner: MessageInfo,
}

impl MessageInfoBuilder {
    pub fn new() -> Self {
        Self {
            inner: Default::default(),
        }
    }

    pub fn set_variant(mut self, variant: Option<String>) -> Self {
        self.inner.variant = variant;
        self
    }

    pub fn build(self) -> MessageInfo {
        self.inner
    }
}
