// @generated automatically by Diesel CLI.

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
    users (id) {
        id -> Int8,
        is_admin -> Bool,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    messages,
    users,
);
