-- This file should undo anything in `up.sql`
ALTER TABLE players DROP COLUMN buffs;

DROP TABLE buffs;
