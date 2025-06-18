pub mod button;
pub mod keyboard;
pub mod message;

use std::collections::HashMap;

use message::BotMessage;
use serde::{Deserialize, Serialize};

use super::Provider;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BotDialog<P: Provider> {
    pub commands: HashMap<String, BotMessage<P>>,
    pub buttons: HashMap<String, BotMessage<P>>,
    stateful_msg_handlers: HashMap<String, BotMessage<P>>,
    #[serde(default)]
    pub(crate) variants: HashMap<String, HashMap<String, BotMessage<P>>>,
}
