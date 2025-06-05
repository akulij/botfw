pub mod application;
pub mod db;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::db::raw_calls::RawCallError;
use crate::db::{CallDB, DbError, User, DB};
use crate::utils::parcelable::{ParcelType, Parcelable, ParcelableError, ParcelableResult};
use chrono::{DateTime, Days, NaiveTime, ParseError, TimeDelta, Timelike, Utc};
use db::attach_db_obj;
use futures::future::join_all;
use futures::lock::MutexGuard;
use itertools::Itertools;
use quickjs_rusty::serde::{from_js, to_js};
use quickjs_rusty::utils::create_empty_object;
use quickjs_rusty::utils::create_string;
use quickjs_rusty::ContextError;
use quickjs_rusty::ExecutionError;
use quickjs_rusty::JsFunction;
use quickjs_rusty::OwnedJsValue as JsValue;
use quickjs_rusty::ValueError;
use quickjs_rusty::{Context, OwnedJsObject};
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
}

#[derive(thiserror::Error, Debug)]
pub enum ResolveError {
    #[error("wrong literal: {0:?}")]
    IncorrectLiteral(String),
}

pub type ScriptResult<T> = Result<T, ScriptError>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BotFunction {
    func: FunctionMarker,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum FunctionMarker {
    /// serde is not able to (de)serialize this, so ignore it and fill
    /// in runtime with injection in DeserializeJS
    #[serde(skip)]
    Function(JsFunction),
    StrTemplate(String),
}

impl FunctionMarker {
    pub fn as_str_template(&self) -> Option<&String> {
        if let Self::StrTemplate(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_function(&self) -> Option<&JsFunction> {
        if let Self::Function(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn set_js_function(&mut self, f: JsFunction) {
        *self = Self::Function(f)
    }
}

impl Parcelable<Self> for BotFunction {
    fn get_field(
        &mut self,
        _name: &str,
    ) -> crate::utils::parcelable::ParcelableResult<ParcelType<Self>> {
        todo!()
    }

    fn resolve(&mut self) -> ParcelableResult<ParcelType<Self>>
    where
        Self: Sized + 'static,
    {
        Ok(ParcelType::Function(self))
    }
}

impl BotFunction {
    pub fn by_name(name: String) -> Self {
        Self {
            func: FunctionMarker::StrTemplate(name),
        }
    }

    pub fn call_context(&self, runner: &Runner) -> ScriptResult<JsValue> {
        match &self.func {
            FunctionMarker::Function(f) => {
                let val = f.call(Default::default())?;
                Ok(val)
            }
            FunctionMarker::StrTemplate(func_name) => runner.run_script(&format!("{func_name}()")),
        }
    }

    pub fn context(&self) -> Option<*mut quickjs_rusty::JSContext> {
        match &self.func {
            FunctionMarker::Function(js_function) => Some(js_function.context()),
            FunctionMarker::StrTemplate(_) => None,
        }
    }

    pub fn call(&self) -> ScriptResult<JsValue> {
        self.call_args(Default::default())
    }

    pub fn call_args(&self, args: Vec<JsValue>) -> ScriptResult<JsValue> {
        if let FunctionMarker::Function(f) = &self.func {
            let val = f.call(args)?;
            Ok(val)
        } else {
            Err(ScriptError::BotFunctionError(
                "Js Function is not defined".to_string(),
            ))
        }
    }

    pub fn set_js_function(&mut self, f: JsFunction) {
        self.func.set_js_function(f);
    }
}

pub trait DeserializeJS {
    fn js_into<'a, T: Deserialize<'a>>(&'a self) -> ScriptResult<T>;
}

impl DeserializeJS for JsValue {
    fn js_into<'a, T: Deserialize<'a>>(&'a self) -> ScriptResult<T> {
        let rc = from_js(self.context(), self)?;

        Ok(rc)
    }
}

#[derive(Default)]
pub struct DeserializerJS {
    fn_map: HashMap<String, JsFunction>,
}

impl DeserializerJS {
    pub fn new() -> Self {
        Self {
            fn_map: HashMap::new(),
        }
    }

    pub fn deserialize_js<'a, T: Deserialize<'a> + Parcelable<BotFunction> + 'static>(
        value: &'a JsValue,
    ) -> ScriptResult<T> {
        let mut s = Self::new();

        s.inject_templates(value, "".to_string())?;

        let mut res = value.js_into()?;

        for (k, jsf) in s.fn_map {
            let item: ParcelType<'_, BotFunction> =
                match Parcelable::<BotFunction>::get_nested(&mut res, &k) {
                    Ok(item) => item,
                    Err(err) => {
                        log::error!("Failed to inject original functions to structs, error: {err}");
                        continue;
                    }
                };
            if let ParcelType::Function(f) = item {
                f.set_js_function(jsf);
            }
        }

        Ok(res)
    }

    pub fn inject_templates(
        &mut self,
        value: &JsValue,
        path: String,
    ) -> ScriptResult<Option<String>> {
        if let Ok(f) = value.clone().try_into_function() {
            self.fn_map.insert(path.clone(), f);
            return Ok(Some(path));
        } else if let Ok(o) = value.clone().try_into_object() {
            let path = if path.is_empty() { path } else { path + "." }; // trying to avoid . in the start
                                                                        // of stringified path
            let res = o
                .properties_iter()?
                .chunks(2)
                .into_iter()
                // since chunks(2) is used and properties iterator over object
                // always has even elements, unwrap will not fail
                .map(
                    #[allow(clippy::unwrap_used)]
                    |mut chunk| (chunk.next().unwrap(), chunk.next().unwrap()),
                )
                .map(|(k, p)| k.and_then(|k| p.map(|p| (k, p))))
                .filter_map(|m| m.ok())
                .try_for_each(|(k, p)| {
                    let k = match k.to_string() {
                        Ok(k) => k,
                        Err(err) => return Err(ScriptError::ValueError(err)),
                    };
                    let res = match self.inject_templates(&p, path.clone() + &k)? {
                        Some(_) => {
                            let fo = JsValue::new(
                                o.context(),
                                create_empty_object(o.context()).expect("couldn't create object"),
                            )
                            .try_into_object()
                            .expect("the object created was not an object :/");
                            fo.set_property(
                                "func",
                                JsValue::new(
                                    o.context(),
                                    create_string(o.context(), "somefunc")
                                        .expect("couldn't create string"),
                                ),
                            )
                            .expect("wasn't able to set property on object :/");
                            o.set_property(&k, fo.into_value())
                        }
                        None => Ok(()),
                    };
                    match res {
                        Ok(res) => Ok(res),
                        Err(err) => Err(ScriptError::ExecutionError(err)),
                    }
                });
            res?;
        };

        Ok(None)
    }
}

// TODO: remove this function since it is suitable only for early development
#[allow(clippy::print_stdout)]
fn print(s: String) {
    println!("{s}");
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

pub trait ResolveValue {
    type Value;

    fn resolve(self) -> ScriptResult<Self::Value>;
    fn resolve_with(self, runner: &Runner) -> ScriptResult<Self::Value>;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum KeyboardDefinition {
    Rows(Vec<RowDefinition>),
    Function(BotFunction),
}

impl Parcelable<BotFunction> for KeyboardDefinition {
    fn get_field(&mut self, _name: &str) -> ParcelableResult<ParcelType<BotFunction>> {
        todo!()
    }
    fn resolve(&mut self) -> ParcelableResult<ParcelType<BotFunction>>
    where
        Self: Sized + 'static,
    {
        match self {
            KeyboardDefinition::Rows(rows) => Ok(rows.resolve()?),
            KeyboardDefinition::Function(f) => Ok(f.resolve()?),
        }
    }
}

impl ResolveValue for KeyboardDefinition {
    type Value = Vec<<RowDefinition as ResolveValue>::Value>;

    fn resolve(self) -> ScriptResult<Self::Value> {
        match self {
            KeyboardDefinition::Rows(rows) => rows.into_iter().map(|r| r.resolve()).collect(),
            KeyboardDefinition::Function(f) => {
                <Self as ResolveValue>::resolve(f.call()?.js_into()?)
            }
        }
    }

    fn resolve_with(self, runner: &Runner) -> ScriptResult<Self::Value> {
        match self {
            KeyboardDefinition::Rows(rows) => {
                rows.into_iter().map(|r| r.resolve_with(runner)).collect()
            }
            KeyboardDefinition::Function(f) => {
                <Self as ResolveValue>::resolve_with(f.call_context(runner)?.js_into()?, runner)
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum RowDefinition {
    Buttons(Vec<ButtonDefinition>),
    Function(BotFunction),
}

impl Parcelable<BotFunction> for RowDefinition {
    fn get_field(&mut self, _name: &str) -> ParcelableResult<ParcelType<BotFunction>> {
        todo!()
    }
    fn resolve(&mut self) -> ParcelableResult<ParcelType<BotFunction>>
    where
        Self: Sized + 'static,
    {
        match self {
            Self::Buttons(buttons) => Ok(buttons.resolve()?),
            Self::Function(f) => Ok(f.resolve()?),
        }
    }
}

impl ResolveValue for RowDefinition {
    type Value = Vec<<ButtonDefinition as ResolveValue>::Value>;

    fn resolve(self) -> ScriptResult<Self::Value> {
        match self {
            RowDefinition::Buttons(buttons) => buttons.into_iter().map(|b| b.resolve()).collect(),
            RowDefinition::Function(f) => <Self as ResolveValue>::resolve(f.call()?.js_into()?),
        }
    }

    fn resolve_with(self, runner: &Runner) -> ScriptResult<Self::Value> {
        match self {
            RowDefinition::Buttons(buttons) => buttons
                .into_iter()
                .map(|b| b.resolve_with(runner))
                .collect(),
            RowDefinition::Function(f) => {
                <Self as ResolveValue>::resolve_with(f.call_context(runner)?.js_into()?, runner)
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum ButtonDefinition {
    Button(ButtonRaw),
    ButtonLiteral(String),
    Function(BotFunction),
}

impl ResolveValue for ButtonDefinition {
    type Value = ButtonRaw;

    fn resolve(self) -> ScriptResult<Self::Value> {
        match self {
            ButtonDefinition::Button(button) => Ok(button),
            ButtonDefinition::ButtonLiteral(l) => Ok(ButtonRaw::from_literal(l)),
            ButtonDefinition::Function(f) => <Self as ResolveValue>::resolve(f.call()?.js_into()?),
        }
    }

    fn resolve_with(self, runner: &Runner) -> ScriptResult<Self::Value> {
        match self {
            ButtonDefinition::Button(button) => Ok(button),
            ButtonDefinition::ButtonLiteral(l) => Ok(ButtonRaw::from_literal(l)),
            ButtonDefinition::Function(f) => {
                <Self as ResolveValue>::resolve_with(f.call_context(runner)?.js_into()?, runner)
            }
        }
    }
}

impl Parcelable<BotFunction> for ButtonDefinition {
    fn get_field(&mut self, _name: &str) -> ParcelableResult<ParcelType<BotFunction>> {
        todo!()
    }
    fn resolve(&mut self) -> ParcelableResult<ParcelType<BotFunction>>
    where
        Self: Sized + 'static,
    {
        match self {
            Self::Button(braw) => Ok(braw.resolve()?),
            Self::ButtonLiteral(s) => Ok(s.resolve()?),
            Self::Function(f) => Ok(f.resolve()?),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ButtonRaw {
    name: ButtonName,
    callback_name: String,
}

impl<F> Parcelable<F> for ButtonRaw {
    fn get_field(&mut self, _name: &str) -> ParcelableResult<ParcelType<F>> {
        todo!()
    }
}

impl ButtonRaw {
    pub fn from_literal(literal: String) -> Self {
        ButtonRaw {
            name: ButtonName::Literal {
                literal: literal.clone(),
            },
            callback_name: literal,
        }
    }

    pub fn name(&self) -> &ButtonName {
        &self.name
    }

    pub fn callback_name(&self) -> &str {
        &self.callback_name
    }

    pub fn literal(&self) -> Option<String> {
        match self.name() {
            ButtonName::Value { .. } => None,
            ButtonName::Literal { literal } => Some(literal.to_string()),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum ButtonName {
    Value { name: String },
    Literal { literal: String },
}

impl ButtonName {
    pub async fn resolve_name(self, db: &mut DB) -> ScriptResult<String> {
        match self {
            ButtonName::Value { name } => Ok(name),
            ButtonName::Literal { literal } => {
                let value = db.get_literal_value(&literal).await?;

                Ok(match value {
                    Some(value) => Ok(value),
                    None => Err(ResolveError::IncorrectLiteral(format!(
                        "not found literal `{literal}` in DB"
                    ))),
                }?)
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Button {
    name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BotMessage {
    // buttons: Vec<Button>
    literal: Option<String>,
    #[serde(default)]
    replace: bool,
    buttons: Option<KeyboardDefinition>,
    state: Option<String>,

    /// flag options to command is meta, so it will be appended to user.metas in db
    #[serde(default)]
    meta: bool,

    handler: Option<BotFunction>,
}

impl BotMessage {
    pub fn fill_literal(&self, l: String) -> Self {
        BotMessage {
            literal: self.clone().literal.or(Some(l)),
            ..self.clone()
        }
    }

    pub fn is_replace(&self) -> bool {
        self.replace
    }

    pub fn get_handler(&self) -> Option<&BotFunction> {
        self.handler.as_ref()
    }

    pub fn meta(&self) -> bool {
        self.meta
    }
}

impl BotMessage {
    pub async fn resolve_buttons(
        &self,
        db: &mut DB,
    ) -> ScriptResult<Option<Vec<Vec<ButtonLayout>>>> {
        let raw_buttons = self.buttons.clone().map(|b| b.resolve()).transpose()?;
        match raw_buttons {
            Some(braws) => {
                let kbd: Vec<Vec<_>> = join_all(braws.into_iter().map(|rows| async {
                    join_all(rows.into_iter().map(|b| async {
                        let mut db = db.clone();
                        ButtonLayout::resolve_raw(b, &mut db).await
                    }))
                    .await
                    .into_iter()
                    .collect()
                }))
                .await
                .into_iter()
                .collect::<Result<_, _>>()?;
                Ok(Some(kbd))
            }
            None => Ok(None),
        }
    }

    pub fn literal(&self) -> Option<&String> {
        self.literal.as_ref()
    }
}

pub enum ButtonLayout {
    Callback {
        name: String,
        literal: Option<String>,
        callback: String,
    },
}

impl ButtonLayout {
    pub async fn resolve_raw(braw: ButtonRaw, db: &mut DB) -> ScriptResult<Self> {
        let name = braw.name().clone().resolve_name(db).await?;
        let literal = braw.literal();
        let callback = braw.callback_name().to_string();
        Ok(Self::Callback {
            name,
            literal,
            callback,
        })
    }
}

impl Parcelable<BotFunction> for BotMessage {
    fn get_field(&mut self, name: &str) -> ParcelableResult<ParcelType<BotFunction>> {
        match name {
            "buttons" => Ok(self.buttons.resolve()?),
            "state" => Ok(self.state.resolve()?),
            "handler" => Ok(self.handler.resolve()?),
            field => Err(ParcelableError::FieldError(format!(
                "tried to get field {field}, but this field does not exists or private"
            ))),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BotDialog {
    pub commands: HashMap<String, BotMessage>,
    pub buttons: HashMap<String, BotMessage>,
    stateful_msg_handlers: HashMap<String, BotMessage>,
}

impl Parcelable<BotFunction> for BotDialog {
    fn get_field(&mut self, name: &str) -> Result<ParcelType<BotFunction>, ParcelableError> {
        match name {
            "commands" => Ok(ParcelType::Parcelable(&mut self.commands)),
            "buttons" => Ok(ParcelType::Parcelable(&mut self.buttons)),
            "stateful_msg_handlers" => Ok(ParcelType::Parcelable(&mut self.stateful_msg_handlers)),
            field => Err(ParcelableError::FieldError(format!(
                "tried to get field {field}, but this field does not exists or private"
            ))),
        }
    }
}

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
    pub fn when_next(&self, start_time: &DateTime<Utc>, now: &DateTime<Utc>) -> DateTime<Utc> {
        let now = *now;
        match self {
            NotificationTime::Delta {
                delta_hours,
                delta_minutes,
            } => {
                let delta = TimeDelta::minutes((delta_minutes + delta_hours * 60).into());

                let mut estimation = *start_time;
                // super non-optimal, but fun :)
                loop {
                    if estimation < now + Duration::from_secs(1) {
                        estimation += delta;
                    } else {
                        break estimation;
                    }
                }
            }
            NotificationTime::Specific(time) => {
                let estimation = now;
                let estimation = estimation.with_hour(time.hour.into()).unwrap_or(estimation);
                let mut estimation = estimation
                    .with_minute(time.minutes.into())
                    .unwrap_or(estimation);
                // super non-optimal, but fun :)
                loop {
                    if estimation < now {
                        estimation = estimation + Days::new(1);
                    } else {
                        break estimation;
                    }
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

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(untagged)]
pub enum NotificationFilter {
    #[default]
    #[serde(rename = "all")]
    All,
    /// Send to randomly selected N people
    Random { random: u32 },
    /// Function that returns list of user id's who should get notification
    BotFunction(BotFunction),
}

impl NotificationFilter {
    pub async fn get_users(&self, db: &DB) -> ScriptResult<Vec<User>> {
        match self {
            NotificationFilter::All => Ok(db.get_users().await?),
            NotificationFilter::Random { random } => Ok(db.get_random_users(*random).await?),
            NotificationFilter::BotFunction(f) => {
                let users = f.call()?;
                let users = from_js(f.context().unwrap(), &users)?;
                Ok(users)
            }
        }
    }
}

impl Parcelable<BotFunction> for NotificationFilter {
    fn get_field(&mut self, name: &str) -> ParcelableResult<ParcelType<BotFunction>> {
        todo!()
    }

    fn resolve(&mut self) -> ParcelableResult<ParcelType<BotFunction>>
    where
        Self: Sized + 'static,
    {
        match self {
            NotificationFilter::All => Ok(ParcelType::Other(())),
            NotificationFilter::Random { .. } => Ok(ParcelType::Other(())),
            NotificationFilter::BotFunction(f) => Ok(Parcelable::<_>::resolve(f)?),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum NotificationMessage {
    Literal {
        literal: String,
    },
    Text {
        text: String,
    },
    /// Function can accept user which will be notified and then return generated message
    BotFunction(BotFunction),
}

impl Parcelable<BotFunction> for NotificationMessage {
    fn get_field(&mut self, name: &str) -> ParcelableResult<ParcelType<BotFunction>> {
        todo!()
    }

    fn resolve(&mut self) -> ParcelableResult<ParcelType<BotFunction>>
    where
        Self: Sized + 'static,
    {
        match self {
            NotificationMessage::Literal { .. } => Ok(ParcelType::Other(())),
            NotificationMessage::Text { .. } => Ok(ParcelType::Other(())),
            NotificationMessage::BotFunction(f) => Ok(f.resolve()?),
        }
    }
}

impl NotificationMessage {
    pub async fn resolve(&self, db: &DB, user: &User) -> ScriptResult<Option<String>> {
        match self {
            NotificationMessage::Literal { literal } => Ok(db.get_literal_value(literal).await?),
            NotificationMessage::Text { text } => Ok(Some(text.to_string())),
            NotificationMessage::BotFunction(f) => {
                let jsuser = to_js(f.context().expect("Function is not js"), user).unwrap();
                let text = f.call_args(vec![jsuser])?;
                let text = from_js(f.context().unwrap(), &text)?;
                Ok(text)
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BotNotification {
    time: NotificationTime,
    #[serde(default)]
    filter: NotificationFilter,
    message: NotificationMessage,
}

impl Parcelable<BotFunction> for BotNotification {
    fn get_field(&mut self, name: &str) -> ParcelableResult<ParcelType<BotFunction>> {
        match name {
            "filter" => Ok(Parcelable::<_>::resolve(&mut self.filter)?),
            "message" => Ok(Parcelable::<BotFunction>::resolve(&mut self.message)?),
            field => Err(ParcelableError::FieldError(format!(
                "tried to get field {field}, but this field does not exists or private"
            ))),
        }
    }
}

impl BotNotification {
    pub fn left_time(&self, start_time: &DateTime<Utc>, now: &DateTime<Utc>) -> Duration {
        let next = self.time.when_next(start_time, now);

        // immidate notification if time to do it passed
        let duration = (next - now).to_std().unwrap_or(Duration::from_secs(1));

        // Rounding partitions of seconds
        Duration::from_secs(duration.as_secs())
    }

    pub async fn get_users(&self, db: &DB) -> ScriptResult<Vec<User>> {
        self.filter.get_users(db).await
    }
    pub async fn resolve_message(&self, db: &DB, user: &User) -> ScriptResult<Option<String>> {
        self.message.resolve(db, user).await
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RunnerConfig {
    config: BotConfig,
    pub dialog: BotDialog,
    #[serde(default)]
    notifications: Vec<BotNotification>,
    #[serde(skip)]
    created_at: ConfigCreatedAt,
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

#[derive(Debug, Clone)]
pub struct NotificationBlock {
    wait_for: Duration,
    notifications: Vec<BotNotification>,
}

impl NotificationBlock {
    pub fn wait_for(&self) -> Duration {
        self.wait_for
    }

    pub fn notifications(&self) -> &[BotNotification] {
        &self.notifications
    }
}

impl RunnerConfig {
    /// command without starting `/`
    pub fn get_command_message(&self, command: &str) -> Option<BotMessage> {
        let bm = self.dialog.commands.get(command).cloned();

        bm.map(|bm| bm.fill_literal(command.to_string()))
    }

    pub fn get_callback_message(&self, callback: &str) -> Option<BotMessage> {
        let bm = self.dialog.buttons.get(callback).cloned();

        bm.map(|bm| bm.fill_literal(callback.to_string()))
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at.at + TimeDelta::try_hours(self.config.timezone.into()).unwrap()
    }

    /// if None is returned, then garanteed that later calls will also return None,
    /// so, if you'll get None, no notifications will be provided later
    pub fn get_nearest_notifications(&self) -> Option<NotificationBlock> {
        let start_time = self.created_at();
        let now =
            chrono::offset::Utc::now() + TimeDelta::try_hours(self.config.timezone.into()).unwrap();

        let ordered = self
            .notifications
            .iter()
            .filter(|f| f.left_time(&start_time, &now) > Duration::from_secs(1))
            .sorted_by_key(|f| f.left_time(&start_time, &now))
            .collect::<Vec<_>>();

        let left = match ordered.first() {
            Some(notification) => notification.left_time(&start_time, &now),
            // No notifications provided
            None => return None,
        };
        // get all that should be sent at the same time
        let notifications = ordered
            .into_iter()
            .filter(|n| n.left_time(&start_time, &now) == left)
            .cloned()
            .collect::<Vec<_>>();

        Some(NotificationBlock {
            wait_for: left,
            notifications,
        })
    }
}

impl Parcelable<BotFunction> for RunnerConfig {
    fn get_field(&mut self, name: &str) -> Result<ParcelType<BotFunction>, ParcelableError> {
        match name {
            "dialog" => Ok(ParcelType::Parcelable(&mut self.dialog)),
            "notifications" => Ok(ParcelType::Parcelable(&mut self.notifications)),
            field => Err(ParcelableError::FieldError(format!(
                "tried to get field {field}, but this field does not exists or private"
            ))),
        }
    }
}

#[derive(Clone)]
pub struct Runner {
    context: Arc<Mutex<Context>>,
}

impl Runner {
    pub fn init() -> ScriptResult<Self> {
        let context = Context::new(None)?;

        context.add_callback("print", |a: String| {
            print(a);

            None::<bool>
        })?;

        Ok(Runner {
            context: Arc::new(Mutex::new(context)),
        })
    }

    pub fn init_with_db(db: &mut DB) -> ScriptResult<Self> {
        let context = Context::new(None)?;
        let mut global = context.global()?;
        attach_db_obj(&context, &mut global, db)?;

        context.add_callback("print", |a: String| {
            print(a);

            None::<bool>
        })?;

        Ok(Runner {
            context: Arc::new(Mutex::new(context)),
        })
    }

    pub fn call_attacher<F, R>(&mut self, f: F) -> ScriptResult<R>
    where
        F: FnOnce(&Context, &mut OwnedJsObject) -> R,
    {
        let context = self.context.lock().unwrap();
        let mut global = context.global()?;

        let res = f(&context, &mut global);
        Ok(res)
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
    use quickjs_rusty::{serde::from_js, OwnedJsObject};
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

    fn recursive_format(o: OwnedJsObject) -> String {
        let props: Vec<_> = o.properties_iter().unwrap().map(|x| x.unwrap()).collect();
        let sp: Vec<String> = props
            .into_iter()
            .map(|v| {
                if v.is_object() {
                    recursive_format(v.try_into_object().unwrap())
                } else {
                    format!("{:?}", v)
                }
            })
            .collect();

        format!("{:?}", sp)
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
        let left = n.left_time(&start_time, &start_time);
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
}
