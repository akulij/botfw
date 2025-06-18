pub mod application;
pub mod db;
pub mod message_info;
use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use crate::config::Provider;
use crate::db::raw_calls::RawCallError;
use crate::db::{CallDB, DbError, User, DB};
use crate::message_answerer::MessageAnswererError;
use crate::runtimes::v8::V8Runtime;
use crate::utils::parcelable::{ParcelType, Parcelable, ParcelableError, ParcelableResult};
use crate::{notify_admin, runtimes, BotError};
use chrono::{DateTime, Days, NaiveTime, ParseError, TimeDelta, Timelike, Utc};
use db::attach_db_obj;
use futures::future::join_all;
use itertools::Itertools;
use serde::Deserialize;
use serde::Serialize;

#[derive(thiserror::Error, Debug)]
pub enum ScriptError {
    #[error("error context: {0:?}")]
    ContextError(#[from] ContextError),
    #[error("error running: {0:?}")]
    ExecutionError(#[from] ExecutionError),
    #[error("error from anyhow: {0:?}")]
    SerdeError(#[from] quickjs_rusty::serde::Error),
    #[error("error value: {0:?}")]
    ValueError(#[from] ValueError),
    #[error("error bot function execution: {0:?}")]
    BotFunctionError(String),
    #[error("error from DB: {0:?}")]
    DBError(#[from] DbError),
    #[error("error resolving data: {0:?}")]
    ResolveError(#[from] ResolveError),
    #[error("error while calling db from runtime: {0:?}")]
    RawCallError(#[from] RawCallError),
    #[error("error while locking mutex: {0:?}")]
    MutexError(String),
    #[error("can't send message to user to user: {0:?}")]
    MAError(#[from] MessageAnswererError),
    #[error("other script error: {0:?}")]
    Other(String),
}

impl From<BotError> for ScriptError {
    fn from(value: BotError) -> Self {
        match value {
            crate::BotError::DBError(db_error) => ScriptError::DBError(db_error),
            error => ScriptError::Other(format!("BotError: {error}")),
        }
    }
}

impl<T> From<PoisonError<T>> for ScriptError {
    fn from(value: PoisonError<T>) -> Self {
        Self::MutexError(format!("Can't lock Mutex in script, err: {}", value))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ResolveError {
    #[error("wrong literal: {0:?}")]
    IncorrectLiteral(String),
}

pub type ScriptResult<T> = Result<T, ScriptError>;

// TODO: remove this function since it is suitable only for early development
#[allow(clippy::print_stdout)]
fn print(s: String) {
    println!("{s}");
}

pub struct Runner {
    // context: Mutex<Context>,
    runtime: V8Runtime,
}

impl<P: Provider> Runner {
    pub fn init() -> ScriptResult<Self> {
        let runtime = runtimes::v8::V8Runtime::new();

        Ok(Runner { runtime })
    }

    pub fn init_with_db(db: &mut DB) -> ScriptResult<Self> {
        todo!()
        // let mut runner = Self::init()?;
        // runner.call_attacher(|c, o| attach_db_obj(c, o, db))??;
        //
        // Ok(runner)
    }

    pub fn call_attacher<F, R>(&mut self, f: F) -> ScriptResult<R>
    where
        F: FnOnce(&Self, &mut P::Value) -> R,
    {
        todo!()
        // let context = self.context.lock().expect("Can't lock context");
        // let mut global = context.global()?;
        //
        // let res = f(&context, &mut global);
        // Ok(res)
    }

    pub fn run_script(&self, content: &str) -> ScriptResult<JsValue> {
        let ctx = match self.context.lock() {
            Ok(ctx) => ctx,
            Err(err) => {
                return Err(ScriptError::MutexError(format!(
                    "can't lock js Context mutex, err: {err}"
                )))
            }
        };

        let val = ctx.eval(content, false)?;

        Ok(val)
    }

    pub fn init_config(&self, content: &str) -> ScriptResult<RunnerConfig> {
        let val = self.run_script(content)?;

        // let rc: RunnerConfig = from_js(unsafe { self.context.context_raw() }, &val)?;
        let rc: RunnerConfig = DeserializerJS::deserialize_js(&val)?;

        Ok(rc)
    }
}

#[cfg(test)]
// allowing this since it is better for debugging tests)
#[allow(clippy::unwrap_used)]
#[allow(clippy::print_stdout)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_run_script_valid() {
        let runner = Runner::init().unwrap();
        let val = runner.run_script(r#"print"#).unwrap();
        println!("Val: {:?}", val);
        let val = runner.run_script(r#"print('Hello from JS!');"#).unwrap();
        println!("Val: {:?}", val);
        assert!(val.is_null());
        let val = runner.run_script(r#"const a = 1+2; a"#).unwrap();
        println!("Val: {:?}", val);
        assert_eq!(val.to_int(), Ok(3));
        let val = runner.run_script(r#"a + 39"#).unwrap();
        println!("Val: {:?}", val);
        assert_eq!(val.to_int(), Ok(42));
    }

    #[test]
    fn test_run_script_file_main() {
        let runner = Runner::init().unwrap();
        let val = runner.run_script(include_str!("../mainbot.js")).unwrap();
        println!("config: {:?}", val);
        let d: RunnerConfig = DeserializerJS::deserialize_js(&val).unwrap();
        println!("desr rc: {:?}", d);
    }

    #[test]
    fn test_func_deserialization_main() {
        let runner = Runner::init().unwrap();
        let _ = runner
            .run_script("function cancel_buttons() {return 'cancelation'}")
            .unwrap();

        let f = BotFunction::by_name("cancel_buttons".to_string());
        let res = f.call_context(&runner).unwrap();

        println!("RES: {res:?}");
        let sres: String = res.js_into().unwrap();
        println!("Deserialized RES: {:?}", sres);
        assert_eq!(sres, "cancelation");
    }

    #[test]
    fn test_run_script_invalid() {
        let runner = Runner::init().unwrap();
        let result = runner.run_script(r#"invalid_script();"#);

        assert!(result.is_err());
        let errstr =
            if let Err(ScriptError::ExecutionError(ExecutionError::Exception(errstr))) = result {
                errstr.to_string().unwrap()
            } else {
                panic!("test returned wrong error!, {result:?}");
            };
        if errstr != "ReferenceError: invalid_script is not defined" {
            panic!("test returned an error, but the wrong one, {errstr}")
        }
    }

    #[test]
    fn test_notification_struct() {
        let botn = json!({
            "time": "18:00",
            "filter": {"random": 2},
            "message": {"text": "some"},
        });
        let n: BotNotification = serde_json::from_value(botn).unwrap();
        println!("BotNotification: {n:#?}");
        assert!(matches!(n.time, NotificationTime::Specific(..)));
        let time = if let NotificationTime::Specific(st) = n.time {
            st
        } else {
            unreachable!()
        };
        assert_eq!(
            time,
            SpecificTime {
                hour: 18,
                minutes: 00
            }
        );
    }

    #[test]
    fn test_notification_time() {
        let botn = json!({
            "time": "18:00",
            "filter": {"random": 2},
            "message": {"text": "some"},
        });
        let n: BotNotification = serde_json::from_value(botn).unwrap();
        println!("BotNotification: {n:#?}");
        let start_time = chrono::offset::Utc::now();
        // let start_time = chrono::offset::Utc::now() + TimeDelta::try_hours(5).unwrap();
        let start_time = start_time.with_hour(13).unwrap().with_minute(23).unwrap();
        let left = n.left_time(start_time, start_time);
        let secs = left.as_secs();
        let minutes = secs / 60;
        let hours = minutes / 60;
        let minutes = minutes % 60;
        println!("Left: {hours}:{minutes}");

        let when_should = chrono::offset::Utc::now()
            .with_hour(18)
            .unwrap()
            .with_minute(00)
            .unwrap();

        let should_left = (when_should - start_time).to_std().unwrap();
        let should_left = Duration::from_secs(should_left.as_secs());

        assert_eq!(left, should_left)
    }

    #[test]
    fn test_notification_time_nextday() {
        let botn = json!({
            "time": "11:00",
            "filter": {"random": 2},
            "message": {"text": "some"},
        });
        let n: BotNotification = serde_json::from_value(botn).unwrap();
        println!("BotNotification: {n:#?}");
        let start_time = chrono::offset::Utc::now();
        // let start_time = chrono::offset::Utc::now() + TimeDelta::try_hours(5).unwrap();
        let start_time = start_time.with_hour(13).unwrap().with_minute(23).unwrap();
        let left = n.left_time(start_time, start_time);
        let secs = left.as_secs();
        let minutes = secs / 60;
        let hours = minutes / 60;
        let minutes = minutes % 60;
        println!("Left: {hours}:{minutes}");

        let when_should = chrono::offset::Utc::now()
            .with_hour(11)
            .unwrap()
            .with_minute(00)
            .unwrap();

        let should_left = (when_should + TimeDelta::days(1) - start_time)
            .to_std()
            .unwrap();
        let should_left = Duration::from_secs(should_left.as_secs());

        assert_eq!(left, should_left)
    }
}
