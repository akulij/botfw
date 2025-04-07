CREATE TYPE reservation_status AS ENUM ('booked', 'paid');
ALTER TABLE reservations ALTER COLUMN status TYPE reservation_status USING status::reservation_status;
