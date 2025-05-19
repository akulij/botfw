use std::collections::HashMap;

use quickjs_rusty::serde::from_js;
use quickjs_rusty::Context;
use quickjs_rusty::ContextError;
use quickjs_rusty::ExecutionError;
use quickjs_rusty::OwnedJsValue as JsValue;
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
}

pub type ScriptResult<T> = Result<T, ScriptError>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BotFunction(String); // temporal workaround

impl BotFunction {
    pub fn call_context(&self, runner: &Runner) -> ScriptResult<JsValue> {
        let func_name = &self.0;

        runner.run_script(&format!("{func_name}()"))
    }
}

pub trait DeserializeJS {
    fn js_into<'a, T: Deserialize<'a>>(&'a self) -> ScriptResult<T>;
}

impl DeserializeJS for JsValue {
    fn js_into<'a, T: Deserialize<'a>>(&'a self) -> ScriptResult<T> {
        let rc = from_js(self.context(), &self)?;

        Ok(rc)
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

#[derive(Serialize, Deserialize, Debug)]
pub struct Button {
    name: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BotMessage {
    // buttons: Vec<Button>
    buttons: BotFunction,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BotDialog(HashMap<String, BotMessage>);

#[derive(Serialize, Deserialize, Debug)]
pub struct RunnerConfig {
    config: BotConfig,
    dialog: BotDialog,
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
        let val = runner.run_script("start_buttons()").unwrap();
        println!("Val: {:?}", val.to_string());
    }

    #[test]
    fn test_deserialization_main() {
        let runner = Runner::init().unwrap();
        let val = runner.run_script(include_str!("../mainbot.js")).unwrap();
        let s: RunnerConfig = from_js(unsafe { runner.context.context_raw() }, &val).unwrap();
        println!("deser: {:#?}", s);
        let o = val.try_into_object().unwrap();
        println!("o: {:?}", recursive_format(o));
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
