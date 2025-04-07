CREATE TYPE reservation_status AS ENUM ('booked', 'paid');

CREATE TABLE reservations (
    id SERIAL PRIMARY KEY,
    user_id INTEGER REFERENCES users(id),
    entered_name VARCHAR(255),
    booked_time TIMESTAMP NOT NULL,
    event_id INTEGER REFERENCES events(id),
    status reservation_status NOT NULL
);
