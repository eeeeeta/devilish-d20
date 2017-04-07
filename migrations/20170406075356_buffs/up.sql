-- Your SQL goes here
ALTER TABLE players ADD COLUMN buffs VARCHAR[] NOT NULL DEFAULT '{}';

CREATE TABLE buffs (
       id SERIAL PRIMARY KEY,
       name VARCHAR NOT NULL,
       descrip VARCHAR NOT NULL,
       code VARCHAR NOT NULL
);
