use super::super::{callback_info::CallbackInfo, CallDB, DB};
use super::setup_db;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
#[serde(tag = "type")]
#[serde(rename = "snake_case")]
pub enum Callback {
    #[default]
    MoreInfo,
    NextPage,
}

type CI = CallbackInfo<Callback>;

#[tokio::test]
async fn test_store() {
    let mut db = setup_db().await;

    let ci = CI::new(Default::default());

    ci.store(&mut db).await.unwrap();

    let ci = CI::get(&mut db, &ci.get_id()).await.unwrap();

    assert!(ci.is_some());
}
