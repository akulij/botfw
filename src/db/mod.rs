pub mod models;
pub mod schema;

use std::os::unix::process::CommandExt;

use self::models::*;

use chrono::Utc;
use diesel::prelude::*;
use diesel::query_builder::NoFromClause;
use diesel::query_builder::SelectStatement;
use diesel_async::pooled_connection::bb8::Pool;
use bb8::PooledConnection;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::AsyncConnection;
use diesel_async::AsyncPgConnection;
use diesel_async::RunQueryDsl;
use enum_stringify::EnumStringify;
use async_trait::async_trait;

#[derive(EnumStringify)]
#[enum_stringify(case = "flat")]
pub enum ReservationStatus {
    Booked,
    Paid,
}

pub trait GetReservationStatus {
    fn get_status(&self) -> Option<ReservationStatus>;
}

impl GetReservationStatus for models::Reservation {
    fn get_status(&self) -> Option<ReservationStatus> {
        ReservationStatus::try_from(self.status.clone()).ok()
    }
}

#[derive(Clone)]
pub struct DB {
    pool: diesel_async::pooled_connection::bb8::Pool<AsyncPgConnection>,
}

impl DB {
    pub async fn new<S: Into<String>>(db_url: S) -> Self {
        let config = AsyncDieselConnectionManager::<diesel_async::AsyncPgConnection>::new(db_url);
        let pool = Pool::builder().build(config).await.unwrap();
        DB { pool }
    }
}

#[async_trait]
impl CallDB for DB {
    async fn get_pool(&mut self) -> PooledConnection<'_, AsyncDieselConnectionManager<AsyncPgConnection>> {
        self.pool.get().await.unwrap()
    }
}

#[async_trait]
pub trait CallDB {
    //type C;
    async fn get_pool(&mut self) -> PooledConnection<'_, AsyncDieselConnectionManager<AsyncPgConnection>>;
    //async fn get_pool(&mut self) -> PooledConnection<'_, AsyncDieselConnectionManager<C>>;
    async fn get_users(&mut self) -> Vec<User> {
        use self::schema::users::dsl::*;
        let mut conn = self.get_pool().await;
        users
            .filter(id.gt(0))
            .load::<User>(&mut conn)
            .await
            .unwrap()
    }

    async fn set_admin(&mut self, userid: i64, isadmin: bool) {
        use self::schema::users::dsl::*;
        let mut conn = self.get_pool().await;
        diesel::update(users)
            .filter(id.eq(userid))
            .set(is_admin.eq(isadmin))
            .execute(&mut conn)
            .await
            .unwrap();
    }

    async fn get_or_init_user(&mut self, userid: i64, firstname: &str) -> User {
        use self::schema::users::dsl::*;
        let conn = &mut self.get_pool().await;

        let user = users
            .filter(id.eq(userid))
            .first::<User>(conn)
            .await
            .optional()
            .unwrap();

        match user {
            Some(existing_user) => existing_user,
            None => diesel::insert_into(users)
                .values((
                    id.eq(userid as i64),
                    is_admin.eq(false),
                    first_name.eq(firstname),
                ))
                .get_result(conn)
                .await
                .unwrap(),
        }
    }

    async fn get_message(
        &mut self,
        chatid: i64,
        messageid: i32,
    ) -> Result<Option<Message>, Box<dyn std::error::Error>> {
        use self::schema::messages::dsl::*;
        let conn = &mut self.get_pool().await;

        let msg = messages
            .filter(chat_id.eq(chatid))
            .filter(message_id.eq(messageid as i64))
            .first::<Message>(conn)
            .await
            .optional()?;

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
        use self::schema::messages::dsl::*;
        let msg = self.get_message(chatid, messageid).await?;
        let conn = &mut self.get_pool().await;

        match msg {
            Some(msg) => {
                diesel::update(messages)
                    .filter(id.eq(msg.id))
                    .set(token.eq(literal))
                    .execute(conn)
                    .await?;
            }
            None => {
                diesel::insert_into(messages)
                    .values((
                        chat_id.eq(chatid),
                        message_id.eq(messageid as i64),
                        token.eq(literal),
                    ))
                    .execute(conn)
                    .await?;
            }
        };

        Ok(())
    }

    async fn get_literal(
        &mut self,
        literal: &str,
    ) -> Result<Option<Literal>, Box<dyn std::error::Error>> {
        use self::schema::literals::dsl::*;
        let conn = &mut self.get_pool().await;

        let literal = literals
            .filter(token.eq(literal))
            .first::<Literal>(conn)
            .await
            .optional()?;

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
        use self::schema::literals::dsl::*;
        let conn = &mut self.get_pool().await;

        diesel::insert_into(literals)
            .values((token.eq(literal), value.eq(valuestr)))
            .on_conflict(token)
            .do_update()
            .set(value.eq(valuestr))
            .execute(conn)
            .await?;

        Ok(())
    }

    async fn get_all_events(&mut self) -> Vec<Event> {
        use self::schema::events::dsl::*;
        let mut conn = self.get_pool().await;
        events
            .filter(id.gt(0))
            .load::<Event>(&mut conn)
            .await
            .unwrap()
    }

    async fn create_event(
        &mut self,
        event_datetime: chrono::DateTime<Utc>,
    ) -> Result<Event, Box<dyn std::error::Error>> {
        use self::schema::events::dsl::*;
        let conn = &mut self.get_pool().await;

        let new_event = diesel::insert_into(events)
            .values((time.eq(event_datetime),))
            .get_result::<Event>(conn)
            .await?;

        Ok(new_event)
    }

    async fn get_media(
        &mut self,
        literal: &str,
    ) -> Result<Vec<Media>, Box<dyn std::error::Error>> {
        use self::schema::media::dsl::*;
        let conn = &mut self.get_pool().await;

        let media_items = media.filter(token.eq(literal)).load::<Media>(conn).await?;

        Ok(media_items)
    }

    async fn is_media_group_exists(
        &mut self,
        media_group: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        use self::schema::media::dsl::*;
        let conn = &mut self.get_pool().await;

        let is_exists = media
            .filter(media_group_id.eq(media_group))
            .count()
            .get_result::<i64>(conn)
            .await?
            > 0;

        Ok(is_exists)
    }

    async fn drop_media(&mut self, literal: &str) -> Result<usize, Box<dyn std::error::Error>> {
        use self::schema::media::dsl::*;
        let conn = &mut self.get_pool().await;

        let deleted_count = diesel::delete(media.filter(token.eq(literal)))
            .execute(conn)
            .await?;

        Ok(deleted_count)
    }

    async fn drop_media_except(
        &mut self,
        literal: &str,
        except_group: &str,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        use self::schema::media::dsl::*;
        let conn = &mut self.get_pool().await;

        let deleted_count = diesel::delete(
            media.filter(
                token
                    .eq(literal)
                    .and(media_group_id.ne(except_group).or(media_group_id.is_null())),
            ),
        )
        .execute(conn)
        .await?;

        Ok(deleted_count)
    }

    async fn add_media(
        &mut self,
        literal: &str,
        mediatype: &str,
        fileid: &str,
        media_group: Option<&str>,
    ) -> Result<Media, Box<dyn std::error::Error>> {
        use self::schema::media::dsl::*;
        let conn = &mut self.get_pool().await;

        let new_media = diesel::insert_into(media)
            .values((
                token.eq(literal),
                media_type.eq(mediatype),
                file_id.eq(fileid),
                media_group_id.eq(media_group),
            ))
            .get_result::<Media>(conn)
            .await?;

        Ok(new_media)
    }
}

#[cfg(test)]
mod tests;
