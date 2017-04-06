-- Your SQL goes here
ALTER TABLE players ADD COLUMN buffs VARCHAR[] NOT NULL DEFAULT '{}';
