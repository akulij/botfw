pub mod traits;

pub mod dialog;
pub mod function;
pub mod notification;
pub mod result;
pub mod time;

use std::time::Duration;

use chrono::DateTime;
use chrono::TimeDelta;
use chrono::Utc;
use dialog::message::BotMessage;
use dialog::BotDialog;
use itertools::Itertools;
use notification::batch::NotificationBatch;
use notification::BotNotification;
use serde::Deserialize;
use serde::Serialize;
pub use traits::Provider;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RunnerConfig<P: Provider> {
    config: BotConfig,
    pub dialog: BotDialog<P>,
    #[serde(default)]
    notifications: Vec<BotNotification<P>>,
    #[serde(skip)]
    created_at: ConfigCreatedAt,
}

impl<P: Provider> RunnerConfig<P> {
    /// command without starting `/`
    pub fn get_command_message(&self, command: &str) -> Option<BotMessage<P>> {
        let bm = self.dialog.commands.get(command).cloned();

        bm.map(|bm| bm.fill_literal(command.to_string()).update_defaults())
    }

    pub fn get_command_message_varianted(
        &self,
        command: &str,
        variant: &str,
    ) -> Option<BotMessage<P>> {
        if !self.dialog.commands.contains_key(command) {
            return None;
        }
        // fallback to regular if not found
        let bm = match self.dialog.variants.get(command).cloned() {
            Some(bm) => bm,
            None => return self.get_command_message(command),
        };
        // get variant of message
        let bm = match bm.get(variant).cloned() {
            Some(bm) => bm,
            None => return self.get_command_message(command),
        };

        Some(bm.fill_literal(command.to_string()).update_defaults())
    }

    pub fn get_callback_message(&self, callback: &str) -> Option<BotMessage<P>> {
        let bm = self.dialog.buttons.get(callback).cloned();

        bm.map(|bm| bm.fill_literal(callback.to_string()))
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.timezoned_time(self.created_at.at)
    }

    pub fn timezoned_time(&self, dt: DateTime<Utc>) -> DateTime<Utc> {
        dt + TimeDelta::try_hours(self.config.timezone.into())
            .unwrap_or_else(|| TimeDelta::try_hours(0).expect("Timezone UTC+0 does not exists"))
    }

    /// if None is returned, then garanteed that later calls will also return None,
    /// so, if you'll get None, no notifications will be provided later
    pub fn get_nearest_notifications(&self) -> Option<NotificationBatch<P>> {
        let start_time = self.created_at();
        let now = self.timezoned_time(chrono::offset::Utc::now());

        let ordered = self
            .notifications
            .iter()
            .filter(|f| f.left_time(start_time, now) > Duration::from_secs(1))
            .sorted_by_key(|f| f.left_time(start_time, now))
            .collect::<Vec<_>>();

        let left = match ordered.first() {
            Some(notification) => notification.left_time(start_time, now),
            // No notifications provided
            None => return None,
        };
        // get all that should be sent at the same time
        let notifications = ordered
            .into_iter()
            .filter(|n| n.left_time(start_time, now) == left)
            .cloned()
            .collect::<Vec<_>>();

        Some(NotificationBatch::new(left, notifications))
    }
}

#[derive(Debug, Clone)]
struct ConfigCreatedAt {
    at: DateTime<Utc>,
}

impl Default for ConfigCreatedAt {
    fn default() -> Self {
        Self {
            at: chrono::offset::Utc::now(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BotConfig {
    version: f64,
    /// relative to UTC, for e.g.,
    /// timezone = 3 will be UTC+3,
    /// timezone =-2 will be UTC-2,
    #[serde(default)]
    timezone: i8,
}
