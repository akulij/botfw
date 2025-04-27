use async_trait::async_trait;
use chrono::{DateTime, Utc};
use enum_stringify::EnumStringify;
use futures::stream::{StreamExt, TryStreamExt};

use mongodb::options::IndexOptions;
use mongodb::{
    bson::doc,
    options::{ClientOptions, ResolverConfig},
    Client,
};
use mongodb::{Database, IndexModel};
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
}

#[derive(Serialize, Deserialize)]
pub struct Message {
    pub _id: bson::oid::ObjectId,
    pub chat_id: i64,
    pub message_id: i64,
    pub token: String,
}

#[derive(Serialize, Deserialize)]
pub struct Literal {
    pub _id: bson::oid::ObjectId,
    pub token: String,
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
}

impl DB {
    pub async fn new<S: Into<String>>(db_url: S) -> Self {
        let options = ClientOptions::parse(db_url.into()).await.unwrap();
        let client = Client::with_options(options).unwrap();

        DB { client }
    }

    pub async fn migrate(&mut self) -> Result<(), mongodb::error::Error> {
        let events = self.get_database().await.collection::<Event>("events");
        events
            .create_index(
                IndexModel::builder()
                    .keys(doc! {"time": 1})
                    .options(IndexOptions::builder().unique(true).build())
                    .build(),
            )
            .await?;
        Ok(())
    }

    pub async fn init<S: Into<String>>(db_url: S) -> Result<Self, mongodb::error::Error> {
        let mut db = Self::new(db_url).await;
        db.migrate().await?;

        Ok(db)
    }
}

#[async_trait]
impl CallDB for DB {
    async fn get_database(&mut self) -> Database {
        self.client.database("gongbot")
    }
}

#[async_trait]
pub trait CallDB {
    //type C;
    async fn get_database(&mut self) -> Database;
    //async fn get_pool(&mut self) -> PooledConnection<'_, AsyncDieselConnectionManager<C>>;
    async fn get_users(&mut self) -> Vec<User> {
        let db = self.get_database().await;
        let users = db.collection::<User>("users");
        users
            .find(doc! {})
            .await
            .unwrap()
            .map(|u| u.unwrap())
            .collect()
            .await
    }

    async fn set_admin(&mut self, userid: i64, isadmin: bool) {
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
            .await
            .unwrap();
    }

    async fn get_or_init_user(&mut self, userid: i64, firstname: &str) -> User {
        let db = self.get_database().await;
        let users = db.collection::<User>("users");

        users
            .update_one(
                doc! { "id": userid },
                doc! {
                    "$set": doc! { "first_name": firstname},
                    "$setOnInsert": doc! { "is_admin": false },
                },
            )
            .upsert(true)
            .await
            .unwrap();

        users
            .find_one(doc! { "id": userid })
            .await
            .unwrap()
            .expect("no such user created")
    }

    async fn get_message(
        &mut self,
        chatid: i64,
        messageid: i32,
    ) -> Result<Option<Message>, Box<dyn std::error::Error>> {
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
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let msg = self.get_message(chatid, messageid).await?;
        Ok(msg.map(|m| m.token))
    }

    async fn set_message_literal(
        &mut self,
        chatid: i64,
        messageid: i32,
        literal: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
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

    async fn get_literal(
        &mut self,
        literal: &str,
    ) -> Result<Option<Literal>, Box<dyn std::error::Error>> {
        let db = self.get_database().await;
        let messages = db.collection::<Literal>("literals");

        let literal = messages.find_one(doc! { "token": literal }).await?;

        Ok(literal)
    }

    async fn get_literal_value(
        &mut self,
        literal: &str,
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let literal = self.get_literal(literal).await?;

        Ok(literal.map(|l| l.value))
    }

    async fn set_literal(
        &mut self,
        literal: &str,
        valuestr: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
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

    async fn get_all_events(&mut self) -> Vec<Event> {
        let db = self.get_database().await;
        let events = db.collection::<Event>("events");

        events
            .find(doc! {})
            .await
            .unwrap()
            .map(|e| e.unwrap())
            .collect()
            .await
    }

    async fn create_event(
        &mut self,
        event_datetime: chrono::DateTime<Utc>,
    ) -> Result<Event, Box<dyn std::error::Error>> {
        let db = self.get_database().await;
        let events = db.collection::<Event>("events");

        let new_event = Event {
            _id: bson::oid::ObjectId::new(),
            time: event_datetime,
        };

        events.insert_one(&new_event).await?;

        Ok(new_event)
    }

    async fn get_media(&mut self, literal: &str) -> Result<Vec<Media>, Box<dyn std::error::Error>> {
        let db = self.get_database().await;
        let media = db.collection::<Media>("media");

        let media_items = media
            .find(doc! { "token": literal })
            .await?
            .try_collect()
            .await?;

        Ok(media_items)
    }

    async fn is_media_group_exists(
        &mut self,
        media_group: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let db = self.get_database().await;
        let media = db.collection::<Media>("media");

        let is_exists = media
            .count_documents(doc! { "media_group_id": media_group })
            .await?
            > 0;

        Ok(is_exists)
    }

    async fn drop_media(&mut self, literal: &str) -> Result<usize, Box<dyn std::error::Error>> {
        let db = self.get_database().await;
        let media = db.collection::<Media>("media");

        let deleted_count = media
            .delete_many(doc! { "token": literal })
            .await?
            .deleted_count;

        Ok(deleted_count as usize)
    }

    async fn drop_media_except(
        &mut self,
        literal: &str,
        except_group: &str,
    ) -> Result<usize, Box<dyn std::error::Error>> {
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
    ) -> Result<Media, Box<dyn std::error::Error>> {
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
