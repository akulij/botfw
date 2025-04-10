use diesel::Connection;
use diesel_async::AsyncPgConnection;
use dotenvy;

use super::DB;

async fn setup_db() -> DB {
    dotenvy::dotenv().unwrap();
    let db_url = std::env::var("DATABASE_URL").unwrap();
    let db = DB::new(db_url).await;

    db
}

#[tokio::test]
async fn test_get_media() {
    let mut db = setup_db().await;

    let result = db.drop_media("test_get_media_literal").await;
    assert!(result.is_ok());

    let media_items = db.get_media("test_get_media_literal").await.unwrap();
    assert_eq!(media_items.len(), 0);

    let result = db
        .add_media("test_get_media_literal", "photo", "file_id_1")
        .await;
    assert!(result.is_ok());

    let media_items = db.get_media("test_get_media_literal").await.unwrap();
    assert_eq!(media_items.len(), 1);

    let result = db
        .add_media("test_get_media_literal", "video", "file_id_2")
        .await;
    assert!(result.is_ok());

    let media_items = db.get_media("test_get_media_literal").await.unwrap();
    assert_eq!(media_items.len(), 2);

    // Clean up after test
    let result = db.drop_media("test_get_media_literal").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_add_media() {
    let mut db = setup_db().await;

    let literal = "test_literal";
    let media_type = "photo";
    let file_id = "LjaldhAOh";

    let result = db.drop_media(literal).await;
    assert!(result.is_ok());

    let result = db.add_media(literal, media_type, file_id).await;
    assert!(result.is_ok());

    // Verify that the media was added is correct
    let media_items = db.get_media(literal).await.unwrap();
    assert_eq!(media_items.len(), 1);
    assert_eq!(media_items[0].token, literal);
    assert_eq!(media_items[0].media_type, media_type);
    assert_eq!(media_items[0].file_id, file_id);

    // Clean up after test
    let result = db.drop_media(literal).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_drop_media() {
    let mut db = setup_db().await;

    let result = db
        .add_media("test_drop_media_literal", "photo", "file_id_1")
        .await;
    assert!(result.is_ok());

    // Verify that the media was added
    let media_items = db.get_media("test_drop_media_literal").await.unwrap();
    assert_eq!(media_items.len(), 1);

    let result = db.drop_media("test_drop_media_literal").await;
    assert!(result.is_ok());

    // Verify that the media has been dropped
    let media_items = db.get_media("test_drop_media_literal").await.unwrap();
    assert_eq!(media_items.len(), 0);

    // Clean up after test
    let result = db.drop_media("test_drop_media_literal").await;
    assert!(result.is_ok());
}
