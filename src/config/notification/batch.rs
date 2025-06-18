use std::time::Duration;

use crate::config::Provider;

use super::BotNotification;

#[derive(Debug, Clone)]
pub struct NotificationBatch<P: Provider> {
    wait_for: Duration,
    notifications: Vec<BotNotification<P>>,
}

impl<P: Provider> NotificationBatch<P> {
    pub fn new(wait_for: Duration, notifications: Vec<BotNotification<P>>) -> Self {
        Self {
            wait_for,
            notifications,
        }
    }

    pub fn wait_for(&self) -> Duration {
        self.wait_for
    }

    pub fn notifications(&self) -> &[BotNotification<P>] {
        &self.notifications
    }
}
