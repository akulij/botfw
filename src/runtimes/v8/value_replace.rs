use std::collections::HashMap;

use deno_core::JsRuntime;
use v8::{Local, Value};

/// move out unsupported by serde types to map, leaving key instead of value to get this value
pub(crate) fn replace(
    value: &mut Local<'_, Value>,
    runtime: &mut JsRuntime,
    outmap: &mut HashMap<String, Local<'_, Value>>,
) {
    todo!()
}
