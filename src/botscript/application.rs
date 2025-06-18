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
    db: DB,
    bot: Bot,
) -> Result<(), ScriptError> {
    // To guarantee that closure is valid if thread panics
    let db: std::sync::Mutex<DB> = std::sync::Mutex::new(db);
    let bot: std::sync::Mutex<Bot> = std::sync::Mutex::new(bot);

    let user_application =
        c.create_callback(move |q: OwnedJsObject| -> Result<_, ScriptError> {
            let mut db = { db.lock().map_err(ScriptError::from)?.clone() };
            let bot = { bot.lock().map_err(ScriptError::from)?.clone() };
            let user: teloxide::types::User = match from_js(q.context(), &q) {
                Ok(q) => q,
                Err(_) => todo!(),
            };

            let application =
                futures::executor::block_on(Application::new(user.clone()).store_db(&mut db))?;

            let msg = tokio::task::block_in_place(|| {
                Handle::current()
                    .block_on(async { send_application_to_chat(&bot, &mut db, &application).await })
            });
            let msg = msg.map_err(ScriptError::from)?;

            let (chat_id, msg_id) = tokio::task::block_in_place(|| {
                Handle::current().block_on(async {
                    MessageAnswerer::new(&bot, &mut db, user.id.0 as i64)
                        .answer("left_application_msg", None, None)
                        .await
                })
            })?;
            futures::executor::block_on(
                MessageForward::new(msg.chat.id.0, msg.id.0, chat_id, msg_id, false)
                    .store_db(&mut db),
            )?;

            let ret = true;
            Ok(ret)
        })?;

    o.set_property("user_application", user_application.into_value())?;
    Ok(())
}
