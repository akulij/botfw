use chrono::{DateTime, FixedOffset, Local};
use serde::{Deserialize, Serialize};

use super::DbResult;
use super::DB;
use crate::query_call_consume;
use crate::CallDB;

#[derive(Serialize, Deserialize, Default)]
pub struct Application<C>
where
    C: Serialize,
{
    pub _id: bson::oid::ObjectId,
    pub created_at: DateTime<FixedOffset>,
    #[serde(flatten)]
    pub from: C,
}

impl<C> Application<C>
where
    C: Serialize + for<'a> Deserialize<'a> + Send + Sync,
{
    pub fn new(from: C) -> Self {
        Self {
            _id: Default::default(),
            created_at: Local::now().into(),
            from,
        }
    }

    query_call_consume!(store, self, db, Self, {
        let db = db.get_database().await;
        let ci = db.collection::<Self>("applications");

        ci.insert_one(&self).await?;

        Ok(self)
    });

    pub async fn store_db(self, db: &mut DB) -> DbResult<Self> {
        let db = db.get_database().await;
        let ci = db.collection::<Self>("applications");

        ci.insert_one(&self).await?;

        Ok(self)
    }
}
