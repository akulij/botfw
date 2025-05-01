use crate::query_call_consume;
use crate::CallDB;
use bson::oid::ObjectId;
use chrono::DateTime;
use chrono::FixedOffset;
use chrono::Local;
use serde::{Deserialize, Serialize};

use super::DbResult;
use bson::doc;

#[derive(Serialize, Deserialize, Default)]
pub struct CallbackInfo<C>
where
    C: Serialize,
{
    pub _id: bson::oid::ObjectId,
    pub created_at: DateTime<FixedOffset>,
    pub literal: Option<String>,
    #[serde(flatten)]
    pub callback: C,
}

impl<C> CallbackInfo<C>
where
    C: Serialize + for<'a> Deserialize<'a> + Send + Sync,
{
    pub fn new(callback: C) -> Self {
        Self {
            _id: Default::default(),
            created_at: Local::now().into(),
            literal: None,
            callback,
        }
    }

    pub fn get_id(&self) -> String {
        self._id.to_hex()
    }

    query_call_consume!(store, self, db, Self, {
        let db = db.get_database().await;
        let ci = db.collection::<Self>("callback_info");

        ci.insert_one(&self).await?;

        Ok(self)
    });

    pub async fn get<D: CallDB>(db: &mut D, id: &str) -> DbResult<Option<Self>> {
        let db = db.get_database().await;
        let ci = db.collection::<Self>("callback_info");

        let id = match ObjectId::parse_str(id) {
            Ok(id) => id,
            Err(_) => return Ok(None),
        };

        Ok(ci
            .find_one(doc! {
                "_id": id
            })
            .await?)
    }

    pub async fn get_callback<D: CallDB>(db: &mut D, id: &str) -> DbResult<Option<C>> {
        Self::get(db, id).await.map(|co| co.map(|c| c.callback))
    }
}
