use crate::query_call;
use crate::CallDB;
use bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

use super::DbResult;
use bson::doc;

#[derive(Serialize, Deserialize, Default)]
pub struct CallbackInfo<C>
where
    C: Serialize,
{
    pub _id: bson::oid::ObjectId,
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
            callback,
        }
    }

    pub fn get_id(&self) -> String {
        self._id.to_hex()
    }

    query_call!(store, self, db, (), {
        let db = db.get_database().await;
        let ci = db.collection::<Self>("callback_info");

        ci.insert_one(self).await?;

        Ok(())
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
