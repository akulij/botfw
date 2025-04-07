// @generated automatically by Diesel CLI.

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

diesel::allow_tables_to_appear_in_same_query!(
    literals,
    messages,
    teloxide_dialogues,
    users,
);
