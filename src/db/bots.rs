use bson::doc;
use chrono::{DateTime, FixedOffset, Local};
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};

use super::DbCollection;
use super::DbResult;
use crate::db::GetCollection;
use crate::query_call_consume;
use crate::CallDB;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct BotInstance {
    pub _id: bson::oid::ObjectId,
    pub name: String,
    pub token: String,
    pub script: String,
    pub restart_flag: bool,
    pub created_at: DateTime<FixedOffset>,
}

impl DbCollection for BotInstance {
    const COLLECTION: &str = "bots";
}

impl BotInstance {
    pub fn new(name: String, token: String, script: String) -> Self {
        Self {
            _id: Default::default(),
            name,
            token,
            script,
            restart_flag: false,
            created_at: Local::now().into(),
        }
    }

    query_call_consume!(store, self, db, Self, {
        let bi = db.get_collection::<Self>().await;

        bi.insert_one(&self).await?;

        Ok(self)
    });

    pub async fn get_all<D: GetCollection>(db: &mut D) -> DbResult<Vec<Self>> {
        let bi = db.get_collection::<Self>().await;

        Ok(bi.find(doc! {}).await?.try_collect().await?)
    }

    pub async fn get_by_name<D: GetCollection>(db: &mut D, name: &str) -> DbResult<Option<Self>> {
        let bi = db.get_collection::<Self>().await;

        Ok(bi.find_one(doc! {"name": name}).await?)
    }

    pub async fn restart_one<D: GetCollection>(
        db: &mut D,
        name: &str,
        restart: bool,
    ) -> DbResult<()> {
        let bi = db.get_collection::<Self>().await;

        bi.update_one(
            doc! {"name": name},
            doc! { "$set": { "restart_flag": restart } },
        )
        .await?;
        Ok(())
    }

    pub async fn restart_all<D: GetCollection>(db: &mut D, restart: bool) -> DbResult<()> {
        let bi = db.get_collection::<Self>().await;

        bi.update_many(doc! {}, doc! { "$set": { "restart_flag": restart } })
            .await?;
        Ok(())
    }

    pub async fn update_script<D: GetCollection>(
        db: &mut D,
        name: &str,
        script: &str,
    ) -> DbResult<()> {
        let bi = db.get_collection::<Self>().await;

        bi.update_one(
            doc! {"name": name},
            doc! { "$set": {
                    "script": script,
                    "restart_flag": true,
                }
            },
        )
        .await?;
        Ok(())
    }
}
