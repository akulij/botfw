use std::sync::RwLock;

use log::info;
use quickjs_rusty::{context::Context, serde::from_js, OwnedJsObject};
use teloxide::Bot;
use tokio::runtime::Handle;

use crate::{
    db::{application::Application, message_forward::MessageForward, DB},
    message_answerer::MessageAnswerer,
    send_application_to_chat,
};

use super::ScriptError;

pub fn attach_user_application(
    c: &Context,
    o: &mut OwnedJsObject,
    db: &DB,
    bot: &Bot,
) -> Result<(), ScriptError> {
    let db: std::sync::Arc<RwLock<DB>> = std::sync::Arc::new(RwLock::new(db.clone()));
    let dbbox = Box::new(db.clone());
    let db: &'static _ = Box::leak(dbbox);

    let bot: std::sync::Arc<RwLock<Bot>> = std::sync::Arc::new(RwLock::new(bot.clone()));
    let botbox = Box::new(bot.clone());
    let bot: &'static _ = Box::leak(botbox);

    let user_application =
        c.create_callback(move |q: OwnedJsObject| -> Result<_, ScriptError> {
            let db = db.clone();
            let user: teloxide::types::User = match from_js(q.context(), &q) {
                Ok(q) => q,
                Err(_) => todo!(),
            };

            let application = futures::executor::block_on(
                Application::new(user.clone()).store_db(&mut db.write().unwrap()),
            )?;

            let db2 = db.clone();
            let msg = tokio::task::block_in_place(move || {
                Handle::current().block_on(async move {
                    send_application_to_chat(
                        &bot.read().unwrap(),
                        &mut db2.write().unwrap(),
                        &application,
                    )
                    .await
                })
            });
            let msg = match msg {
                Ok(msg) => msg,
                Err(err) => {
                    info!("Got err: {err}");
                    return Err(ScriptError::MutexError("🤦‍♂️".to_string()));
                }
            };

            let (chat_id, msg_id) = futures::executor::block_on(
                MessageAnswerer::new(
                    &bot.read().unwrap(),
                    &mut db.write().unwrap(),
                    user.id.0 as i64,
                )
                .answer("left_application_msg", None, None),
            )
            .unwrap();
            futures::executor::block_on(
                MessageForward::new(msg.chat.id.0, msg.id.0, chat_id, msg_id, false)
                    .store_db(&mut db.write().unwrap()),
            )?;

            let ret = true;
            Ok(ret)
        })?;

    o.set_property("user_application", user_application.into_value())?;
    Ok(())
}
