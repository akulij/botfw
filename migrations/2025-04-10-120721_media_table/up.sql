CREATE TABLE media (
    id SERIAL PRIMARY KEY,
    token VARCHAR NOT NULL,
    media_type VARCHAR NOT NULL,
    file_id VARCHAR NOT NULL
);
