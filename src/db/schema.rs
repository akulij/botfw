// @generated automatically by Diesel CLI.

diesel::table! {
    events (id) {
        id -> Int4,
        time -> Timestamptz,
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
    media (id) {
        id -> Int4,
        token -> Varchar,
        media_type -> Varchar,
        file_id -> Varchar,
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
    reservations (id) {
        id -> Int4,
        user_id -> Int8,
        #[max_length = 255]
        entered_name -> Varchar,
        booked_time -> Timestamp,
        event_id -> Int4,
        status -> Varchar,
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
    media,
    messages,
    reservations,
    teloxide_dialogues,
    users,
);
