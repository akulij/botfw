pub mod models;
pub mod schema;
use crate::Config;

use self::models::*;

use diesel::prelude::*;
//use diesel::query_dsl::methods::FilterDsl;
//use diesel::{prelude::*, r2d2::{ConnectionManager, Pool}};
use diesel_async::AsyncPgConnection;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::pooled_connection::bb8::Pool;
use diesel_async::RunQueryDsl;

#[derive(Clone)]
pub struct DB {
    //pool: Pool<ConnectionManager<AsyncPgConnection>>
    pool: diesel_async::pooled_connection::bb8::Pool<AsyncPgConnection>
}

impl DB {
    pub async fn new<S: Into<String>>(db_url: S) -> Self {
        let config = AsyncDieselConnectionManager::<diesel_async::AsyncPgConnection>::new(db_url);
        let pool = Pool::builder().build(config).await.unwrap();
        //let mg = diesel::r2d2::ConnectionManager::new(db_url);
        //let pool = diesel::r2d2::Pool::builder()
        //    .max_size(15)
        //    .build(mg)
        //    .unwrap();
        DB { pool }
    }

    pub async fn get_users(&mut self) -> Vec<User> {
        use self::schema::users::dsl::*;
        let mut conn = self.pool.get().await.unwrap();
        //let mut conn = AsyncPgConnection::establish(&std::env::var("DATABASE_URL").unwrap()).await.unwrap();
        users.filter(id.gt(0)).load::<User>(&mut conn).await.unwrap()
    }

    pub async fn set_admin(&mut self, userid: i64, isadmin: bool) {
        use self::schema::users::dsl::*;
        let connection = &mut self.pool.get().await.unwrap();
        //diesel::update(users).filter(id.eq(userid)).set(is_admin.eq(true)).execute(connection);
        //diesel::update(users).filter(id.eq(userid)).set(is_admin.eq(true)).load(connection).await.unwrap();
        diesel::update(users)
            .filter(id.eq(userid))
            .set(is_admin.eq(isadmin))
            .execute(connection).await.unwrap();
    }

    pub async fn get_or_init_user(&mut self, userid: i64) -> User {
        use self::schema::users::dsl::*;
        let connection = &mut self.pool.get().await.unwrap();

        let user = users.filter(id.eq(userid)).first::<User>(connection).await.optional().unwrap();

        match user {
            Some(existing_user) => existing_user,
            None => {
                diesel::insert_into(users).values((id.eq(userid as i64), is_admin.eq(false))).get_result(connection).await.unwrap()
            }
        }
    }
}
