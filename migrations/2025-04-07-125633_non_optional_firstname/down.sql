-- This file should undo anything in `up.sql`



ALTER TABLE "users" ALTER COLUMN "first_name" DROP NOT NULL;

