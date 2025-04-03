CREATE TABLE messages (
    id SERIAL PRIMARY KEY,
    chat_id BIGINT NOT NULL,
    message_id BIGINT NOT NULL,
    token VARCHAR(255) NOT NULL,
    UNIQUE (chat_id, message_id)
);
