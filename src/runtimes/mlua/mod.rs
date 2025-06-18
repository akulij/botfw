use mlua::{Error, Function, Lua, Value};

use crate::config::Provider;

#[derive(Clone)]
pub struct LuaRuntime {
    lua: Lua,
}

impl LuaRuntime {
    pub fn new() -> Self {
        let lua = Lua::new();
        Self { lua }
    }
}

pub struct LuaInit {
    config: String,
}

impl Provider for LuaRuntime {
    type Function = Function;

    type Value = Value;

    type Error = Error;

    type InitData = LuaInit;

    fn init_config(&self, d: Self::InitData) -> Result<crate::config::RunnerConfig<Self>, Self::Error> {
        todo!()
    }
}
