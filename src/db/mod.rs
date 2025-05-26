pub mod application;
pub mod bots;
pub mod callback_info;
pub mod message_forward;
pub mod raw_calls;

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use enum_stringify::EnumStringify;
use futures::stream::TryStreamExt;

use mongodb::options::IndexOptions;
use mongodb::{bson::doc, options::ClientOptions, Client};
use mongodb::{Collection, Database, IndexModel};
use serde::{Deserialize, Serialize};

#[derive(EnumStringify)]
#[enum_stringify(case = "flat")]
pub enum ReservationStatus {
    Booked,
    Paid,
}

pub trait GetReservationStatus {
    fn get_status(&self) -> Option<ReservationStatus>;
}

//impl GetReservationStatus for models::Reservation {
//    fn get_status(&self) -> Option<ReservationStatus> {
//        ReservationStatus::try_from(self.status.clone()).ok()
//    }
//}
#[derive(Serialize, Deserialize, Default)]
pub struct User {
    pub _id: bson::oid::ObjectId,
    pub id: i64,
    pub is_admin: bool,
    pub first_name: String,
    pub last_name: Option<String>,
    pub username: Option<String>,
    pub language_code: Option<String>,
    pub metas: Vec<String>,
}

#[macro_export]
macro_rules! query_call {
    ($func_name:ident, $self:ident, $db:ident, $return_type:ty, $body:block) => {
        pub async fn $func_name<D: CallDB>(&$self, $db: &mut D)
            -> DbResult<$return_type> $body
    };
}

#[macro_export]
macro_rules! query_call_consume {
    ($func_name:ident, $self:ident, $db:ident, $return_type:ty, $body:block) => {
        pub async fn $func_name<D: CallDB>($self, $db: &mut D)
            -> DbResult<$return_type> $body
    };
}

impl User {
    query_call!(update_user, self, db, (), {
        let db_collection = db.get_database().await.collection::<Self>("users");

        db_collection
            .update_one(
                doc! { "_id": self._id },
                doc! {
                    "$set": {
                        "first_name": &self.first_name,
                        "last_name": &self.last_name,
                        "username": &self.username,
                        "language_code": &self.language_code,
                        "is_admin": &self.is_admin,
                    }
                },
            )
            .await?;

        Ok(())
    });

    pub async fn insert_meta<D: CallDB>(&self, db: &mut D, meta: &str) -> DbResult<()> {
        let db_collection = db.get_database().await.collection::<Self>("users");

        db_collection
            .update_one(
                doc! { "_id": self._id },
                doc! {
                    "$push": {
                        "metas": meta,
                    }
                },
            )
            .await?;

        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub struct Message {
    pub _id: bson::oid::ObjectId,
    pub chat_id: i64,
    pub message_id: i64,
    pub token: String,
    pub variant: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Literal {
    pub _id: bson::oid::ObjectId,
    pub token: String,
    pub value: String,
}

#[derive(Serialize, Deserialize)]
pub struct LiteralAlternative {
    pub _id: bson::oid::ObjectId,
    pub token: String,
    pub variant: String,
    pub value: String,
}

#[derive(Serialize, Deserialize)]
pub struct Event {
    pub _id: bson::oid::ObjectId,
    pub time: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
pub struct Media {
    pub _id: bson::oid::ObjectId,
    pub token: String,
    pub media_type: String,
    pub file_id: String,
    pub media_group_id: Option<String>,
}

#[derive(Clone)]
pub struct DB {
    client: Client,
    name: String,
}

impl DB {
    pub async fn new<S: Into<String>>(db_url: S, name: String) -> DbResult<Self> {
        let options = ClientOptions::parse(db_url.into()).await?;
        let client = Client::with_options(options)?;

        Ok(DB { client, name })
    }

    pub async fn migrate(&mut self) -> DbResult<()> {
        /// some migrations doesn't realy need type of collection
        type AnyCollection = Event;
        let events = self.get_database().await.collection::<Event>("events");
        events
            .create_index(
                IndexModel::builder()
                    .keys(doc! {"time": 1})
                    .options(IndexOptions::builder().unique(true).build())
                    .build(),
            )
            .await?;

        // clear callbacks after a day because otherwise database will contain so much data
        // for just button clicks
        let callback_info = self
            .get_database()
            .await
            .collection::<AnyCollection>("callback_info");
        callback_info
            .create_index(
                IndexModel::builder()
                    .keys(doc! {"created_at": 1})
                    .options(
                        IndexOptions::builder()
                            .expire_after(Duration::from_secs(60 * 60 * 24 /* 1 day */))
                            .build(),
                    )
                    .build(),
            )
            .await?;
        Ok(())
    }

    pub async fn init<S: Into<String>>(db_url: S, name: String) -> DbResult<Self> {
        let mut db = Self::new(db_url, name).await?;
        db.migrate().await?;

        Ok(db)
    }
}

pub trait DbCollection {
    const COLLECTION: &str;
}

pub trait GetCollection {
    async fn get_collection<C: DbCollection + Send + Sync>(&mut self) -> Collection<C>;
}

#[async_trait]
impl CallDB for DB {
    async fn get_database(&mut self) -> Database {
        self.client.database(&self.name)
    }
}

impl<T: CallDB> GetCollection for T {
    async fn get_collection<C: DbCollection + Send + Sync>(&mut self) -> Collection<C> {
        self.get_database()
            .await
            .collection(<C as DbCollection>::COLLECTION)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum DbError {
    #[error("error while processing mongodb query: {0}")]
    MongodbError(#[from] mongodb::error::Error),
}
pub type DbResult<T> = Result<T, DbError>;

#[async_trait]
pub trait CallDB {
    //type C;
    async fn get_database(&mut self) -> Database;
    //async fn get_pool(&mut self) -> PooledConnection<'_, AsyncDieselConnectionManager<C>>;
    async fn get_users(&mut self) -> DbResult<Vec<User>> {
        let db = self.get_database().await;
        let users = db.collection::<User>("users");

        Ok(users.find(doc! {}).await?.try_collect().await?)
    }

    async fn set_admin(&mut self, userid: i64, isadmin: bool) -> DbResult<()> {
        let db = self.get_database().await;
        let users = db.collection::<User>("users");
        users
            .update_one(
                doc! {
                    "id": userid
                },
                doc! {
                    "$set": { "is_admin": isadmin }
                },
            )
            .await?;

        Ok(())
    }

    async fn get_or_init_user(&mut self, userid: i64, firstname: &str) -> DbResult<User> {
        let db = self.get_database().await;
        let users = db.collection::<User>("users");

        users
            .update_one(
                doc! { "id": userid },
                doc! {
                    "$set": doc! { "first_name": firstname},
                    "$setOnInsert": doc! { "is_admin": false, "metas": [] },
                },
            )
            .upsert(true)
            .await?;

        Ok(users
            .find_one(doc! { "id": userid })
            .await?
            .expect("no such user created"))
    }

    async fn get_message(&mut self, chatid: i64, messageid: i32) -> DbResult<Option<Message>> {
        let db = self.get_database().await;
        let messages = db.collection::<Message>("messages");

        let msg = messages
            .find_one(doc! { "chat_id": chatid, "message_id": messageid as i64 })
            .await?;

        Ok(msg)
    }

    async fn get_message_literal(
        &mut self,
        chatid: i64,
        messageid: i32,
    ) -> DbResult<Option<String>> {
        let msg = self.get_message(chatid, messageid).await?;
        Ok(msg.map(|m| m.token))
    }

    async fn set_message_literal(
        &mut self,
        chatid: i64,
        messageid: i32,
        literal: &str,
    ) -> DbResult<()> {
        let db = self.get_database().await;
        let messages = db.collection::<Message>("messages");

        messages
            .update_one(
                doc! {
                    "chat_id": chatid,
                    "message_id": messageid as i64
                },
                doc! {
                    "$set": { "token": literal }
                },
            )
            .upsert(true)
            .await?;

        Ok(())
    }

    async fn set_message_literal_variant(
        &mut self,
        chatid: i64,
        messageid: i32,
        literal: &str,
        variant: &str,
    ) -> DbResult<()> {
        let db = self.get_database().await;
        let messages = db.collection::<Message>("messages");

        messages
            .update_one(
                doc! {
                    "chat_id": chatid,
                    "message_id": messageid as i64
                },
                doc! {
                    "$set": { "token": literal, "variant": variant }
                },
            )
            .upsert(true)
            .await?;

        Ok(())
    }

    async fn get_literal(&mut self, literal: &str) -> DbResult<Option<Literal>> {
        let db = self.get_database().await;
        let messages = db.collection::<Literal>("literals");

        let literal = messages.find_one(doc! { "token": literal }).await?;

        Ok(literal)
    }

    async fn get_literal_value(&mut self, literal: &str) -> DbResult<Option<String>> {
        let literal = self.get_literal(literal).await?;

        Ok(literal.map(|l| l.value))
    }

    async fn set_literal(&mut self, literal: &str, valuestr: &str) -> DbResult<()> {
        let db = self.get_database().await;
        let literals = db.collection::<Literal>("literals");

        literals
            .update_one(
                doc! { "token": literal },
                doc! { "$set": { "value": valuestr } },
            )
            .upsert(true)
            .await?;

        Ok(())
    }

    async fn get_literal_alternative(
        &mut self,
        literal: &str,
        variant: &str,
    ) -> DbResult<Option<LiteralAlternative>> {
        let db = self.get_database().await;
        let messages = db.collection::<LiteralAlternative>("literal_alternatives");

        let literal = messages
            .find_one(doc! { "token": literal, "variant": variant })
            .await?;

        Ok(literal)
    }

    async fn get_literal_alternative_value(
        &mut self,
        literal: &str,
        variant: &str,
    ) -> DbResult<Option<String>> {
        let literal = self.get_literal_alternative(literal, variant).await?;

        Ok(literal.map(|l| l.value))
    }

    async fn set_literal_alternative(
        &mut self,
        literal: &str,
        variant: &str,
        valuestr: &str,
    ) -> DbResult<()> {
        let db = self.get_database().await;
        let literals = db.collection::<LiteralAlternative>("literal_alternatives");

        literals
            .update_one(
                doc! { "token": literal, "variant": variant },
                doc! { "$set": { "value": valuestr } },
            )
            .upsert(true)
            .await?;

        Ok(())
    }

    async fn get_all_events(&mut self) -> DbResult<Vec<Event>> {
        let db = self.get_database().await;
        let events = db.collection::<Event>("events");

        Ok(events.find(doc! {}).await?.try_collect().await?)
    }

    async fn create_event(&mut self, event_datetime: chrono::DateTime<Utc>) -> DbResult<Event> {
        let db = self.get_database().await;
        let events = db.collection::<Event>("events");

        let new_event = Event {
            _id: bson::oid::ObjectId::new(),
            time: event_datetime,
        };

        events.insert_one(&new_event).await?;

        Ok(new_event)
    }

    async fn delete_event(&mut self, event_datetime: chrono::DateTime<Utc>) -> DbResult<()> {
        let db = self.get_database().await;
        let events = db.collection::<Event>("events");

        events.delete_one(doc! { "time": event_datetime }).await?;

        Ok(())
    }

    async fn delete_all_events(&mut self) -> DbResult<usize> {
        let db = self.get_database().await;
        let events = db.collection::<Event>("events");

        let delete_result = events.delete_many(doc! {}).await?;

        Ok(delete_result.deleted_count as usize)
    }

    async fn get_media(&mut self, literal: &str) -> DbResult<Vec<Media>> {
        let db = self.get_database().await;
        let media = db.collection::<Media>("media");

        let media_items = media
            .find(doc! { "token": literal })
            .await?
            .try_collect()
            .await?;

        Ok(media_items)
    }

    async fn is_media_group_exists(&mut self, media_group: &str) -> DbResult<bool> {
        let db = self.get_database().await;
        let media = db.collection::<Media>("media");

        let is_exists = media
            .count_documents(doc! { "media_group_id": media_group })
            .await?
            > 0;

        Ok(is_exists)
    }

    async fn drop_media(&mut self, literal: &str) -> DbResult<usize> {
        let db = self.get_database().await;
        let media = db.collection::<Media>("media");

        let deleted_count = media
            .delete_many(doc! { "token": literal })
            .await?
            .deleted_count;

        Ok(deleted_count as usize)
    }

    async fn drop_media_except(&mut self, literal: &str, except_group: &str) -> DbResult<usize> {
        let db = self.get_database().await;
        let media = db.collection::<Media>("media");

        let deleted_count = media
            .delete_many(doc! {
                "token": literal,
                "media_group_id": { "$ne": except_group }
            })
            .await?
            .deleted_count;

        Ok(deleted_count as usize)
    }

    async fn add_media(
        &mut self,
        literal: &str,
        mediatype: &str,
        fileid: &str,
        media_group: Option<&str>,
    ) -> DbResult<Media> {
        let db = self.get_database().await;
        let media = db.collection::<Media>("media");

        let new_media = Media {
            _id: bson::oid::ObjectId::new(),
            token: literal.to_string(),
            media_type: mediatype.to_string(),
            file_id: fileid.to_string(),
            media_group_id: media_group.map(|g| g.to_string()),
        };

        media.insert_one(&new_media).await?;

        Ok(new_media)
    }
}

#[cfg(test)]
mod tests;
