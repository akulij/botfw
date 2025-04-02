pub mod models;
pub mod schema;
use crate::Config;

use self::models::*;

use diesel::{prelude::*, r2d2::{ConnectionManager, Pool}};

pub fn establish_connection(cfg: Config) -> PgConnection {
    PgConnection::establish(&cfg.db_url)
        .unwrap_or_else(|_| panic!("Error connecting to {}", cfg.db_url))
}

#[derive(Clone)]
pub struct DB {
    pool: Pool<ConnectionManager<PgConnection>>
}

impl DB {
    pub fn new<S: Into<String>>(db_url: S) -> Self{
        let mg = diesel::r2d2::ConnectionManager::new(db_url);
        let pool = diesel::r2d2::Pool::builder()
            .max_size(15)
            .build(mg)
            .unwrap();
        DB { pool }
    }
    pub fn make_admin(&mut self, userid: i64) {
        use self::schema::users::dsl::*;
        let connection = &mut self.pool.get().unwrap();
        diesel::update(users).filter(id.eq(userid)).set(is_admin.eq(true)).execute(connection);
    }

    pub fn get_or_init_user(&mut self, userid: i64) -> User {
        use self::schema::users::dsl::*;
        let connection = &mut self.pool.get().unwrap();

        let user = users.filter(id.eq(userid)).first::<User>(connection).optional().unwrap();

        match user {
            Some(existing_user) => existing_user,
            None => {
                diesel::insert_into(users).values((id.eq(userid as i64), is_admin.eq(false))).get_result(connection).unwrap()
            }
        }
    }
}
