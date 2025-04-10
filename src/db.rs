pub mod models;
pub mod schema;
use crate::Config;

use self::models::*;

use chrono::Utc;
use diesel::prelude::*;
use diesel_async::pooled_connection::bb8::Pool;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::AsyncPgConnection;
use diesel_async::RunQueryDsl;
use enum_stringify::EnumStringify;

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

    pub async fn get_users(&mut self) -> Vec<User> {
        use self::schema::users::dsl::*;
        let mut conn = self.pool.get().await.unwrap();
        users
            .filter(id.gt(0))
            .load::<User>(&mut conn)
            .await
            .unwrap()
    }

    pub async fn set_admin(&mut self, userid: i64, isadmin: bool) {
        use self::schema::users::dsl::*;
        let connection = &mut self.pool.get().await.unwrap();
        diesel::update(users)
            .filter(id.eq(userid))
            .set(is_admin.eq(isadmin))
            .execute(connection)
            .await
            .unwrap();
    }

    pub async fn get_or_init_user(&mut self, userid: i64, firstname: &str) -> User {
        use self::schema::users::dsl::*;
        let connection = &mut self.pool.get().await.unwrap();

        let user = users
            .filter(id.eq(userid))
            .first::<User>(connection)
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
                .get_result(connection)
                .await
                .unwrap(),
        }
    }

    pub async fn get_message(
        &mut self,
        chatid: i64,
        messageid: i32,
    ) -> Result<Option<Message>, Box<dyn std::error::Error>> {
        use self::schema::messages::dsl::*;
        let conn = &mut self.pool.get().await.unwrap();

        let msg = messages
            .filter(chat_id.eq(chatid))
            .filter(message_id.eq(messageid as i64))
            .first::<Message>(conn)
            .await
            .optional()?;

        Ok(msg)
    }

    pub async fn get_message_literal(
        &mut self,
        chatid: i64,
        messageid: i32,
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let msg = self.get_message(chatid, messageid).await?;
        Ok(msg.map(|m| m.token))
    }

    pub async fn set_message_literal(
        &mut self,
        chatid: i64,
        messageid: i32,
        literal: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use self::schema::messages::dsl::*;
        let conn = &mut self.pool.get().await?;

        let msg = self.clone().get_message(chatid, messageid).await?;

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
        let conn = &mut self.pool.get().await.unwrap();

        let literal = literals
            .filter(token.eq(literal))
            .first::<Literal>(conn)
            .await
            .optional()?;

        Ok(literal)
    }

    pub async fn get_literal_value(
        &mut self,
        literal: &str,
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let literal = self.get_literal(literal).await?;

        Ok(literal.map(|l| l.value))
    }

    pub async fn set_literal(
        &mut self,
        literal: &str,
        valuestr: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use self::schema::literals::dsl::*;
        let conn = &mut self.pool.get().await.unwrap();

        diesel::insert_into(literals)
            .values((token.eq(literal), value.eq(valuestr)))
            .on_conflict(token)
            .do_update()
            .set(value.eq(valuestr))
            .execute(conn)
            .await?;

        Ok(())
    }

    pub async fn get_all_events(&mut self) -> Vec<Event> {
        use self::schema::events::dsl::*;
        let mut conn = self.pool.get().await.unwrap();
        events
            .filter(id.gt(0))
            .load::<Event>(&mut conn)
            .await
            .unwrap()
    }

    pub async fn create_event(
        &mut self,
        event_datetime: chrono::DateTime<Utc>,
    ) -> Result<Event, Box<dyn std::error::Error>> {
        use self::schema::events::dsl::*;
        let conn = &mut self.pool.get().await.unwrap();

        let new_event = diesel::insert_into(events)
            .values((time.eq(event_datetime),))
            .get_result::<Event>(conn)
            .await?;

        Ok(new_event)
    }
}
