use std::sync::RwLock;

use quickjs_rusty::context::Context;

use quickjs_rusty::serde::{from_js, to_js};
use quickjs_rusty::{utils::create_empty_object, OwnedJsObject, OwnedJsValue as JsValue};

use crate::db::raw_calls::RawCall;
use crate::db::DB;

use super::ScriptError;

pub fn attach_db_obj(c: &Context, o: &mut OwnedJsObject, db: &DB) -> Result<(), ScriptError> {
    let dbobj = JsValue::new(o.context(), create_empty_object(o.context())?)
        .try_into_object()
        .expect("the created object was not an object :/");

    let db: std::sync::Arc<RwLock<DB>> = std::sync::Arc::new(RwLock::new(db.clone()));

    let find_one = c.create_callback(
        move |collection: String, q: OwnedJsObject| -> Result<_, ScriptError> {
            // let db = db.clone();
            let query: serde_json::Value = match from_js(q.context(), &q) {
                Ok(q) => q,
                Err(_) => todo!(),
            };

            let value = futures::executor::block_on(
                db.write()
                    .expect("failed to gain write acces to db (probably RwLock is poisoned)")
                    .find_one(&collection, query),
            )?;

            let ret = match value {
                Some(v) => Some(to_js(q.context(), &v)?),
                None => None,
            };
            Ok(ret)
        },
    )?;
    let find_one = JsValue::from((unsafe { c.context_raw() }, find_one));

    dbobj.set_property("find_one", find_one)?;

    o.set_property("db", dbobj.into_value())?;

    Ok(())
}
