use std::collections::HashMap;

use crate::utils::parcelable::{ParcelType, Parcelable, ParcelableError, ParcelableResult};
use itertools::Itertools;
use quickjs_rusty::serde::from_js;
use quickjs_rusty::utils::create_empty_object;
use quickjs_rusty::utils::create_string;
use quickjs_rusty::Context;
use quickjs_rusty::ContextError;
use quickjs_rusty::ExecutionError;
use quickjs_rusty::JsFunction;
use quickjs_rusty::OwnedJsValue as JsValue;
use quickjs_rusty::ValueError;
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

#[derive(Serialize, Deserialize, Debug)]
pub struct BotConfig {
    version: f64,
}

pub trait ResolveValue {
    type Value;

    fn resolve(self, runner: &Runner) -> ScriptResult<Self::Value>;
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

    fn resolve(self, runner: &Runner) -> ScriptResult<Self::Value> {
        match self {
            KeyboardDefinition::Rows(rows) => rows.into_iter().map(|r| r.resolve(runner)).collect(),
            KeyboardDefinition::Function(f) => {
                <Self as ResolveValue>::resolve(f.call_context(runner)?.js_into()?, runner)
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

    fn resolve(self, runner: &Runner) -> ScriptResult<Self::Value> {
        match self {
            RowDefinition::Buttons(buttons) => {
                buttons.into_iter().map(|b| b.resolve(runner)).collect()
            }
            RowDefinition::Function(f) => {
                <Self as ResolveValue>::resolve(f.call_context(runner)?.js_into()?, runner)
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

    fn resolve(self, runner: &Runner) -> ScriptResult<Self::Value> {
        match self {
            ButtonDefinition::Button(button) => Ok(button),
            ButtonDefinition::ButtonLiteral(l) => Ok(ButtonRaw::from_literal(l)),
            ButtonDefinition::Function(f) => {
                <Self as ResolveValue>::resolve(f.call_context(runner)?.js_into()?, runner)
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
    buttons: Option<KeyboardDefinition>,
    state: Option<String>,

    handler: Option<BotFunction>,
}

impl BotMessage {
    pub fn fill_literal(&self, l: String) -> Self {
        BotMessage {
            literal: self.clone().literal.or(Some(l)),
            ..self.clone()
        }
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

#[derive(Serialize, Deserialize, Debug)]
pub struct BotDialog {
    pub commands: HashMap<String, BotMessage>,
    stateful_msg_handlers: HashMap<String, BotMessage>,
}

impl Parcelable<BotFunction> for BotDialog {
    fn get_field(&mut self, name: &str) -> Result<ParcelType<BotFunction>, ParcelableError> {
        match name {
            "commands" => Ok(ParcelType::Parcelable(&mut self.commands)),
            "stateful_msg_handlers" => Ok(ParcelType::Parcelable(&mut self.stateful_msg_handlers)),
            field => Err(ParcelableError::FieldError(format!(
                "tried to get field {field}, but this field does not exists or private"
            ))),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RunnerConfig {
    config: BotConfig,
    pub dialog: BotDialog,
}

impl Parcelable<BotFunction> for RunnerConfig {
    fn get_field(&mut self, name: &str) -> Result<ParcelType<BotFunction>, ParcelableError> {
        match name {
            "dialog" => Ok(ParcelType::Parcelable(&mut self.dialog)),
            field => Err(ParcelableError::FieldError(format!(
                "tried to get field {field}, but this field does not exists or private"
            ))),
        }
    }
}

pub struct Runner {
    context: Context,
}

impl Runner {
    pub fn init() -> ScriptResult<Self> {
        let context = Context::new(None)?;

        context.add_callback("print", |a: String| {
            print(a);

            None::<bool>
        })?;

        Ok(Runner { context })
    }

    pub fn run_script(&self, content: &str) -> ScriptResult<JsValue> {
        let ctx = &self.context;

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
        let val = runner.run_script("start_buttons()").unwrap();
        println!("Val: {:?}", val.to_string());
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
}
