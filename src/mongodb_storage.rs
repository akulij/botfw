use std::{
    fmt::{Debug, Display},
    sync::Arc,
};

use futures::future::BoxFuture;
use mongodb::bson::doc;
use mongodb::Database;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use teloxide::dispatching::dialogue::{Serializer, Storage};

use crate::db::{CallDB, DB};

pub struct MongodbStorage<S> {
    database: Database,
    serializer: S,
}

impl<S> MongodbStorage<S> {
    pub async fn open(
        database_url: &str,
        database_name: &str,
        serializer: S,
    ) -> Result<Arc<Self>, mongodb::error::Error> {
        let client = mongodb::Client::with_uri_str(database_url).await?;
        let database = client.database(database_name);

        Ok(Arc::new(Self {
            database,
            serializer,
        }))
    }

    pub async fn from_db(db: &mut DB, serializer: S) -> Result<Arc<Self>, mongodb::error::Error> {
        Ok(Arc::new(Self {
            database: CallDB::get_database(db).await,
            serializer,
        }))
    }
}

#[derive(Serialize, Deserialize)]
pub struct Dialogue {
    chat_id: i64,
    dialogue: Vec<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum MongodbStorageError<SE>
where
    SE: Debug + Display,
{
    MongodbError(#[from] mongodb::error::Error),
    SerdeError(SE),
}

pub type MongodbStorageResult<T, SE> = Result<T, MongodbStorageError<SE>>;

impl<SE> std::fmt::Display for MongodbStorageError<SE>
where
    SE: Debug + Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl<S, D> Storage<D> for MongodbStorage<S>
where
    S: Send + Sync + Serializer<D> + 'static,
    D: Send + Serialize + DeserializeOwned + 'static,

    <S as Serializer<D>>::Error: Debug + Display,
{
    type Error = MongodbStorageError<<S as Serializer<D>>::Error>;

    fn remove_dialogue(
        self: std::sync::Arc<Self>,
        chat_id: teloxide::prelude::ChatId,
    ) -> BoxFuture<'static, Result<(), Self::Error>>
    where
        D: Send + 'static,
    {
        Box::pin(async move {
            let d = self.database.collection::<Dialogue>("dialogues");
            d.delete_one(doc! { "chat_id": chat_id.0 })
                .await
                .map(|_| ())?;
            Ok(())
        })
    }

    fn update_dialogue(
        self: std::sync::Arc<Self>,
        chat_id: teloxide::prelude::ChatId,
        dialogue: D,
    ) -> BoxFuture<'static, Result<(), Self::Error>>
    where
        D: Send + 'static,
    {
        Box::pin(async move {
            let d = self.database.collection::<Dialogue>("dialogues");
            d.update_one(
                doc! {
                    "chat_id": chat_id.0
                },
                doc! {
                        "$set": doc! {
                            "dialogue": self.serializer.serialize(&dialogue)
                                .map_err(MongodbStorageError::SerdeError)?
                                .into_iter().map(|v| v as u32).collect::<Vec<u32>>()
                        }
                },
            )
            .upsert(true)
            .await?;
            Ok(())
        })
    }

    fn get_dialogue(
        self: std::sync::Arc<Self>,
        chat_id: teloxide::prelude::ChatId,
    ) -> BoxFuture<'static, Result<Option<D>, Self::Error>> {
        Box::pin(async move {
            let d = self.database.collection::<Dialogue>("dialogues");
            let d = d.find_one(doc! { "chat_id": chat_id.0 }).await?;
            let d = match d {
                Some(d) => d,
                None => return Ok(None),
            };
            let d = self
                .serializer
                .deserialize(
                    d.dialogue
                        .into_iter()
                        .map(|i| i as u8)
                        .collect::<Vec<_>>()
                        .as_slice(),
                )
                .map_err(MongodbStorageError::SerdeError)?;

            Ok(Some(d))
        })
    }
}
