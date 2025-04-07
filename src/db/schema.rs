// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(diesel::query_builder::QueryId, Clone, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "reservation_status"))]
    pub struct ReservationStatus;
}

diesel::table! {
    events (id) {
        id -> Int4,
        time -> Timestamp,
    }
}

diesel::table! {
    literals (id) {
        id -> Int4,
        #[max_length = 255]
        token -> Varchar,
        value -> Text,
    }
}

diesel::table! {
    messages (id) {
        id -> Int4,
        chat_id -> Int8,
        message_id -> Int8,
        #[max_length = 255]
        token -> Varchar,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::ReservationStatus;

    reservations (id) {
        id -> Int4,
        user_id -> Nullable<Int4>,
        #[max_length = 255]
        entered_name -> Nullable<Varchar>,
        booked_time -> Timestamp,
        event_id -> Nullable<Int4>,
        status -> ReservationStatus,
    }
}

diesel::table! {
    teloxide_dialogues (chat_id) {
        chat_id -> Int8,
        dialogue -> Bytea,
    }
}

diesel::table! {
    users (id) {
        id -> Int8,
        is_admin -> Bool,
        #[max_length = 255]
        first_name -> Varchar,
        #[max_length = 255]
        last_name -> Nullable<Varchar>,
        #[max_length = 255]
        username -> Nullable<Varchar>,
        #[max_length = 10]
        language_code -> Nullable<Varchar>,
    }
}

diesel::joinable!(reservations -> events (event_id));
diesel::joinable!(reservations -> users (user_id));

diesel::allow_tables_to_appear_in_same_query!(
    events,
    literals,
    messages,
    reservations,
    teloxide_dialogues,
    users,
);
