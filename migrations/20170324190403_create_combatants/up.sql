-- Your SQL goes here
CREATE TABLE combatants (
       id SERIAL PRIMARY KEY,
       name VARCHAR NOT NULL,
       initiative INT NOT NULL DEFAULT 0,
       cur_hp INT NOT NULL DEFAULT 0,
       max_hp INT NOT NULL DEFAULT 0,
       armor_class INT NOT NULL DEFAULT 0,
       attack VARCHAR NOT NULL,
       attack_bonus INT NOT NULL DEFAULT 0,
       player_id INT,
       monster_id INT
);
CREATE TABLE monsters (
       id SERIAL PRIMARY KEY,
       name VARCHAR NOT NULL,
       typ VARCHAR NOT NULL,
       armor_class INT NOT NULL,
       hit_points INT NOT NULL,
       strength INT NOT NULL,
       intelligence INT NOT NULL,
       dexterity INT NOT NULL,
       constitution INT NOT NULL,
       wisdom INT NOT NULL,
       charisma INT NOT NULL,
       challenge_rating VARCHAR NOT NULL,
       room_id INT[] NOT NULL
);
CREATE TABLE players (
       id SERIAL PRIMARY KEY,
       name VARCHAR NOT NULL,
       nick VARCHAR NOT NULL,
       typ VARCHAR NOT NULL,
       armor_class INT NOT NULL,
       hit_points INT NOT NULL,
       strength INT NOT NULL,
       intelligence INT NOT NULL,
       dexterity INT NOT NULL,
       constitution INT NOT NULL,
       wisdom INT NOT NULL,
       charisma INT NOT NULL,
       initiative_bonus INT NOT NULL DEFAULT 0
);
CREATE TABLE abilities (
       id SERIAL PRIMARY KEY,
       name VARCHAR NOT NULL,
       descrip VARCHAR NOT NULL,
       damage_dice VARCHAR,
       attack_bonus INT,
       uses_left INT NOT NULL,
       uses INT NOT NULL,
       monster_id INT,
       player_id INT,
       item_id INT
);
CREATE TABLE items (
       id SERIAL PRIMARY KEY,
       name VARCHAR NOT NULL,
       descrip VARCHAR NOT NULL,
       qty INT NOT NULL,
       player_id INT,
       room_id INT
);
CREATE TABLE rooms (
       id SERIAL PRIMARY KEY,
       shortid VARCHAR NOT NULL,
       name VARCHAR NOT NULL,
       descrip VARCHAR NOT NULL
);
CREATE TABLE spells (
       id SERIAL PRIMARY KEY,
       name VARCHAR NOT NULL,
       typ VARCHAR NOT NULL,
       ritual BOOLEAN NOT NULL,
       level VARCHAR NOT NULL,
       school VARCHAR NOT NULL,
       casting_time VARCHAR NOT NULL,
       range VARCHAR NOT NULL,
       duration VARCHAR NOT NULL,
       descrip VARCHAR NOT NULL
);
