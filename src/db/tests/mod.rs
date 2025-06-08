#![allow(clippy::unwrap_used)]

mod callback_info_tests;
use dotenvy;

use super::CallDB;
use super::DB;

async fn setup_db() -> DB {
    dotenvy::dotenv().unwrap();
    let db_url = std::env::var("DATABASE_URL").unwrap();

    DB::new(db_url, "tests".to_string()).await.unwrap()
}

#[tokio::test]
async fn test_get_media() {
    let mut db = setup_db().await;

    let _result = db.drop_media("test_get_media_literal").await.unwrap();

    let media_items = db.get_media("test_get_media_literal").await.unwrap();
    assert_eq!(media_items.len(), 0);

    let _result = db
        .add_media("test_get_media_literal", "photo", "file_id_1", None)
        .await
        .unwrap();

    let media_items = db.get_media("test_get_media_literal").await.unwrap();
    assert_eq!(media_items.len(), 1);

    let _result = db
        .add_media("test_get_media_literal", "video", "file_id_2", None)
        .await
        .unwrap();

    let media_items = db.get_media("test_get_media_literal").await.unwrap();
    assert_eq!(media_items.len(), 2);

    // Clean up after test
    let _result = db.drop_media("test_get_media_literal").await.unwrap();
}

#[tokio::test]
async fn test_add_media() {
    let mut db = setup_db().await;

    let literal = "test_literal";
    let media_type = "photo";
    let file_id = "LjaldhAOh";

    let _result = db.drop_media(literal).await.unwrap();

    let _result = db
        .add_media(literal, media_type, file_id, None)
        .await
        .unwrap();

    // Verify that the media was added is correct
    let media_items = db.get_media(literal).await.unwrap();
    assert_eq!(media_items.len(), 1);
    assert_eq!(media_items[0].token, literal);
    assert_eq!(media_items[0].media_type, media_type);
    assert_eq!(media_items[0].file_id, file_id);

    // Clean up after test
    let _result = db.drop_media(literal).await.unwrap();
}

#[tokio::test]
async fn test_drop_media() {
    let mut db = setup_db().await;

    let _result = db.drop_media("test_drop_media_literal").await.unwrap();

    let _result = db
        .add_media("test_drop_media_literal", "photo", "file_id_1", None)
        .await
        .unwrap();

    // Verify that the media was added
    let media_items = db.get_media("test_drop_media_literal").await.unwrap();
    assert_eq!(media_items.len(), 1);

    let _result = db.drop_media("test_drop_media_literal").await.unwrap();

    // Verify that the media has been dropped
    let media_items = db.get_media("test_drop_media_literal").await.unwrap();
    assert_eq!(media_items.len(), 0);

    // Clean up after test
    let _result = db.drop_media("test_drop_media_literal").await.unwrap();
}

#[tokio::test]
async fn test_is_media_group_exists() {
    let mut db = setup_db().await;

    let media_group = "test_media_group";
    let literal = "test_media_group_literal";

    let _ = db.drop_media(literal).await.unwrap();

    let exists = db.is_media_group_exists(media_group).await.unwrap();
    assert!(!exists);

    let _ = db
        .add_media(literal, "photo", "file_id_1", Some(media_group))
        .await
        .unwrap();

    let exists = db.is_media_group_exists(media_group).await.unwrap();
    assert!(exists);

    let _ = db.drop_media(literal).await.unwrap();

    let exists = db.is_media_group_exists(media_group).await.unwrap();
    assert!(!exists);
}

#[tokio::test]
async fn test_drop_media_except() {
    let mut db = setup_db().await;

    let media_group = "test_media_group_except";
    let literal = "test_media_group_except_literal";
    let _ = db.drop_media(literal).await.unwrap();

    let _ = db
        .add_media(literal, "photo", "file_id_2", None)
        .await
        .unwrap();
    let _ = db
        .add_media(literal, "photo", "file_id_3", None)
        .await
        .unwrap();

    let media_items = db.get_media(literal).await.unwrap();
    assert_eq!(media_items.len(), 2);

    let _deleted_count = db.drop_media_except(literal, media_group).await.unwrap();
    let media_items = db.get_media(literal).await.unwrap();
    assert_eq!(media_items.len(), 0);

    let _ = db
        .add_media(literal, "photo", "file_id_1", Some(media_group))
        .await
        .unwrap();
    let _ = db
        .add_media(literal, "photo", "file_id_2", None)
        .await
        .unwrap();
    let _ = db
        .add_media(literal, "photo", "file_id_3", None)
        .await
        .unwrap();

    let _deleted_count = db.drop_media_except(literal, media_group).await.unwrap();
    let media_items = db.get_media(literal).await.unwrap();
    assert_eq!(media_items.len(), 1);
    let _ = db.drop_media(literal).await.unwrap();

    let _ = db
        .add_media(literal, "photo", "file_id_1", Some(media_group))
        .await
        .unwrap();
    let _ = db
        .add_media(literal, "photo", "file_id_2", Some(media_group))
        .await
        .unwrap();

    let _deleted_count = db.drop_media_except(literal, media_group).await.unwrap();
    let media_items = db.get_media(literal).await.unwrap();
    assert_eq!(media_items.len(), 2);

    let _ = db.drop_media(literal).await.unwrap();
}

#[tokio::test]
async fn test_get_random_users() {
    let mut db = setup_db().await;

    let _ = db.get_or_init_user(1, "Nick").await;

    let users = db.get_random_users(1).await.unwrap();
    assert_eq!(users.len(), 1);
}
