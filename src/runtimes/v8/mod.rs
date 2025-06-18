mod value_replace;
use std::{
    collections::HashMap,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::{
        mpsc::{Receiver, Sender},
        Arc, Mutex, RwLock,
    },
    thread::JoinHandle,
};

use crate::config::{
    traits::{ProviderCall, ProviderDeserialize, ProviderSerialize},
    Provider, RunnerConfig,
};
use deno_core::{ascii_str, error::CoreError, FastString, JsRuntime, RuntimeOptions};
use serde::{Deserialize, Serialize};
use serde_v8::{from_v8, Value as SerdeValue};
use v8::{Context, ContextScope, Function, HandleScope, Local, OwnedIsolate};

enum EventType {
    GetScriptConfig(String),
    ExecuteFunction(V8Function, Vec<V8Value>),
}

pub struct Event {
    event: EventType,
    runtime: Arc<V8Runtime>,
}

enum RuntimeReturn {
    Value(V8Value),
    OptionalValue(Option<V8Value>),
    Config(RunnerConfig<V8Runtime>),
}

impl RuntimeReturn {
    fn as_value(self) -> Option<V8Value> {
        if let Self::Value(v) = self {
            Some(v)
        } else {
            None
        }
    }

    fn as_optional_value(self) -> Option<Option<V8Value>> {
        if let Self::OptionalValue(v) = self {
            Some(v)
        } else {
            None
        }
    }

    fn as_config(self) -> Option<RunnerConfig<V8Runtime>> {
        if let Self::Config(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct V8Runtime {
    #[serde(skip, default = "default_runtime")]
    runtime: Arc<Mutex<JoinHandle<()>>>,
    #[serde(skip, default = "default_sender")]
    tx: Sender<Event>,
    #[serde(skip, default = "default_receiver")]
    rx: Arc<Mutex<Receiver<RuntimeReturn>>>,
}

fn default_runtime() -> Arc<Mutex<JoinHandle<()>>> {
    todo!()
}

fn default_sender() -> Sender<Event> {
    todo!()
}

fn default_receiver() -> Arc<Mutex<Receiver<RuntimeReturn>>> {
    todo!()
}

impl Default for V8Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl V8Runtime {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<Event>();
        let (rtx, rrx) = std::sync::mpsc::channel::<RuntimeReturn>();
        let thread = std::thread::spawn(move || {
            let options = RuntimeOptions::default();
            let mut runtime = JsRuntime::new(options);
            let handlers: HashMap<&str, v8::Local<'_, v8::Value>> = HashMap::new();
            loop {
                let event = match rx.recv() {
                    Ok(event) => event,
                    Err(err) => break,
                };
                match event {
                    Event::GetScriptConfig(script) => {
                        let code = FastString::from(script);

                        let result = runtime.execute_script("", code).unwrap();
                        let mut scope = runtime.handle_scope();
                        let result = Local::new(&mut scope, result);
                        value_replace::replace(&mut result, &mut runtime, &mut handlers);
                        let config: RunnerConfig<Self> = from_v8(&mut scope, result).unwrap();

                        // rtx.send(RuntimeReturn::Value(unsafe { V8Value::new(SerdeValue::from(result)) }))
                        rtx.send(RuntimeReturn::Config(config)).unwrap();
                    }
                    Event::ExecuteFunction(f, args) => {
                        let value = unsafe { f.get_inner() }.get_value();
                        let value = handlers[value];
                        let mut scope = runtime.handle_scope();
                        let context = Local::new(&mut scope, runtime.main_context());
                        let global = context.global(&mut scope).into();
                        let f: Local<'_, Function> = value.try_into().unwrap();
                        let args: Vec<Local<'_, v8::Value>> = args
                            .into_iter()
                            .map(|a| unsafe {
                                let r = a.get_inner().get_value();
                                r
                            })
                            .collect();
                        let result = f.call(&mut scope, global, args.as_slice());
                        let result = result.map(|r| SerdeValue::from(r));

                        rtx.send(RuntimeReturn::OptionalValue(
                            result.map(|result| unsafe { V8Value::new(SerdeValue::from(result)) }),
                        ))
                        .unwrap();
                    }
                };
            }
        });

        Self {
            runtime: Arc::new(Mutex::new(thread)),
            tx,
            rx: Arc::new(Mutex::new(rrx)),
        }
    }

    pub(crate) fn call_event(&self, event: Event) -> RuntimeReturn {
        // locking before send to avoid runtime output shuffle
        // because reciever depends on sender
        // and runtime single-threaded anyway
        let rx = self.rx.lock().unwrap();
        self.tx.send(event).unwrap();
        rx.recv().unwrap()
    }
}

#[derive(thiserror::Error, Debug)]
pub enum V8Error {
    #[error("v8 data error: {0:?}")]
    DataError(#[from] v8::DataError),
    #[error("Failed to create v8 string: {0:?}")]
    StringCreation(String),
    #[error("Deno core error: {0:?}")]
    DenoCore(#[from] CoreError),
    #[error("error context: {0:?}")]
    Other(String),
}

pub struct V8Init {
    code: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct V8Value {
    value: ValueHandler,
    runtime: Arc<V8Runtime>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ValueHandler {
    value: String,
}

impl ValueHandler {
    fn get_value(&self) -> &str {
        &self.value
    }
}

impl V8Value {
    unsafe fn get_inner(&self) -> &ValueHandler {
        &self.value
    }
    unsafe fn new(runtime: Arc<V8Runtime>, value: ValueHandler) -> Self {
        Self { runtime, value }
    }
}

impl ProviderDeserialize for V8Value {
    type Provider = V8Runtime;

    fn de_into<T>(&self) -> Result<T, <Self::Provider as Provider>::Error> {
        todo!()
    }
}

impl ProviderSerialize for V8Value {
    type Provider = V8Runtime;

    fn se_from<T: Serialize>(from: &T) -> Result<Self, <Self::Provider as Provider>::Error>
    where
        Self: Sized,
    {
        todo!()
    }
}

impl std::fmt::Debug for V8Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("V8Value")
            .field("value", &"_".to_string())
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct V8Function {
    value: ValueHandler,
    runtime: Arc<Mutex<V8Runtime>>,
}

impl V8Function {
    unsafe fn get_inner(&self) -> &ValueHandler {
        &self.value
    }
}

impl ProviderCall for V8Function {
    type Provider = V8Runtime;

    fn call(
        &self,
        args: &[&<Self::Provider as Provider>::Value],
    ) -> Result<Option<<Self::Provider as Provider>::Value>, <Self::Provider as Provider>::Error>
    {
        let result: RuntimeReturn =
            self.runtime
                .lock()
                .unwrap()
                .call_event(Event::ExecuteFunction(
                    self.clone(),
                    args.into_iter().map(|v| (*v).clone()).collect(),
                ));
        Ok(result.as_optional_value().unwrap())
    }
}

impl std::fmt::Debug for V8Function {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("V8Value")
            .field("value", &"_".to_string())
            .finish()
    }
}

impl Provider for V8Runtime {
    type Function = V8Function;

    type Value = V8Value;

    type Error = V8Error;

    type InitData = V8Init;

    fn init_config(&self, d: Self::InitData) -> Result<RunnerConfig<Self>, Self::Error> {
        let result = self.call_event(Event::GetScriptConfig(d.code));
        let value = result.as_config().unwrap();
        Ok(value)
    }
}
