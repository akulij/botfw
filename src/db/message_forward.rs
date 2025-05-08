use bson::doc;
use serde::{Deserialize, Serialize};

use super::DbResult;
use crate::query_call_consume;
use crate::CallDB;

#[derive(Serialize, Deserialize)]
pub struct MessageForward {
    pub _id: bson::oid::ObjectId,
    pub chat_id: i64,
    pub message_id: i64,
    pub source_chat_id: i64,
    pub source_message_id: i64,
    pub reply: bool,
}

impl MessageForward {
    pub fn new(
        chat_id: i64,
        message_id: i64,
        source_chat_id: i64,
        source_message_id: i64,
        reply: bool,
    ) -> Self {
        Self {
            _id: Default::default(),
            chat_id,
            message_id,
            source_chat_id,
            source_message_id,
            reply,
        }
    }

    query_call_consume!(store, self, db, Self, {
        let db = db.get_database().await;
        let ci = db.collection::<Self>("message_forward");

        ci.insert_one(&self).await?;

        Ok(self)
    });

    pub async fn get<D: CallDB>(
        db: &mut D,
        chat_id: i64,
        message_id: i64,
    ) -> DbResult<Option<Self>> {
        let db = db.get_database().await;
        let ci = db.collection::<Self>("message_forward");

        let mf = ci
            .find_one(doc! {
                "chat_id": chat_id,
                "message_id": message_id,
            })
            .await?;

        Ok(mf)
    }
}
