use std::time::Duration;

use chrono::{DateTime, Days, NaiveTime, ParseError, TimeDelta, Timelike, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum NotificationTime {
    Delta {
        #[serde(default)]
        delta_hours: u32,
        #[serde(default)]
        delta_minutes: u32,
    },
    Specific(SpecificTime),
}

impl NotificationTime {
    pub fn when_next(&self, start_time: DateTime<Utc>, now: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            NotificationTime::Delta {
                delta_hours,
                delta_minutes,
            } => {
                let delta = TimeDelta::minutes((delta_minutes + delta_hours * 60).into());

                let secs_period = delta.num_seconds();
                if secs_period == 0 {
                    return now;
                };

                let diff = now - start_time;
                let passed = diff.num_seconds().abs() % secs_period;

                now - Duration::from_secs(passed as u64) + delta
            }
            NotificationTime::Specific(time) => {
                let estimation = now;
                let estimation = estimation.with_hour(time.hour.into()).unwrap_or(estimation);
                let estimation = estimation
                    .with_minute(time.minutes.into())
                    .unwrap_or(estimation);

                if estimation < now {
                    estimation + Days::new(1)
                } else {
                    estimation
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(try_from = "SpecificTimeFormat")]
pub struct SpecificTime {
    hour: u8,
    minutes: u8,
}

impl SpecificTime {
    pub fn new(hour: u8, minutes: u8) -> Self {
        Self { hour, minutes }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum SpecificTimeFormat {
    String(String),
    Verbose { hour: u8, minutes: u8 },
}

impl TryFrom<SpecificTimeFormat> for SpecificTime {
    type Error = ParseError;

    fn try_from(stf: SpecificTimeFormat) -> Result<Self, Self::Error> {
        match stf {
            SpecificTimeFormat::Verbose { hour, minutes } => Ok(Self::new(hour, minutes)),
            SpecificTimeFormat::String(timestring) => {
                let time: NaiveTime = timestring.parse()?;

                Ok(Self::new(time.hour() as u8, time.minute() as u8))
            }
        }
    }
}
