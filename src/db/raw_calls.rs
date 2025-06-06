use async_trait::async_trait;
use mongodb::Database;

use super::CallDB;
use serde_json::Value;

#[derive(thiserror::Error, Debug)]
pub enum RawCallError {
    #[error("error while processing mongodb query: {0}")]
    MongodbError(#[from] mongodb::error::Error),
    #[error("error while buildint bson's query document: {0}")]
    DocumentError(#[from] mongodb::bson::extjson::de::Error),
    #[error("error when expected map: {0}")]
    NotAMapError(String),
}
pub type RawCallResult<T> = Result<T, RawCallError>;

#[async_trait]
pub trait RawCall {
    async fn get_database(&mut self) -> Database;
    async fn find_one(&mut self, collection: &str, query: Value) -> RawCallResult<Option<Value>> {
        let db = self.get_database().await;
        let value = db.collection::<Value>(collection);

        let map = match query {
            Value::Object(map) => map,
            _ => return Err(RawCallError::NotAMapError("query is not a map".to_string())),
        };

        let doc = map.try_into()?;
        let ret = value.find_one(doc).await?;
        Ok(ret)
    }
}

#[async_trait]
impl<T: CallDB + Send> RawCall for T {
    async fn get_database(&mut self) -> Database {
        CallDB::get_database(self).await
    }
}
