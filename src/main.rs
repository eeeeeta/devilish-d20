#![feature(slice_patterns, advanced_slice_patterns, unicode)]
extern crate glitch_in_the_matrix as gm;
extern crate rouler;
#[macro_use] extern crate diesel;
#[macro_use] extern crate diesel_codegen;
extern crate serde;
#[macro_use] extern crate serde_derive;
#[macro_use] extern crate error_chain;
extern crate serde_json;
extern crate dotenv;
extern crate std_unicode;

use diesel::prelude::*;
use diesel::pg::PgConnection;
use dotenv::dotenv;
use std::env;
use rouler::Roller;
use std::panic;
use gm::MatrixClient;
use gm::types::*;

use std::fs::File;
use std::io::BufReader;
use std::io::prelude::*;
use std_unicode::str::UnicodeStr;

pub mod errors {
    error_chain! {
        links {
            Matrix(::gm::errors::MatrixError, ::gm::errors::MatrixErrorKind);
        }
        foreign_links {
            Int(::std::num::ParseIntError);
            Diesel(::diesel::result::Error);
            Io(::std::io::Error);
            Json(::serde_json::Error);
        }
    }
}
use errors::*;
pub mod schema;
pub mod models;
pub mod import;
pub mod matrix;
use import::{SrdMonster, Weapon, MonsterAbility, Datafile};
use schema::combatants::dsl as cdsl;
use schema::monsters::dsl as mdsl;
use schema::abilities::dsl as adsl;
use schema::players::dsl as pdsl;
use schema::rooms::dsl as rdsl;
use schema::items::dsl as idsl;
use models::*;

sql_function!(lower, lower_t, (a: diesel::types::VarChar) -> diesel::types::VarChar);

pub fn establish_connection() -> PgConnection {
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    PgConnection::establish(&database_url)
        .expect(&format!("Error connecting to {}", database_url))
}

pub fn score_to_mod(score: i32) -> i64 {
    match score {
        1 => -5,
        2...3 => -4,
        4...5 => -3,
        6...7 => -2,
        8...9 => -1,
        10...11 => 0,
        12...13 => 1,
        14...15 => 2,
        16...17 => 3,
        18...19 => 4,
        20...21 => 5,
        22...23 => 6,
        24...25 => 7,
        26...27 => 8,
        28...29 => 9,
        30 => 10,
        _ => 0
    }
}
struct Conn {
    client: MatrixClient,
    admin: String,
    db: PgConnection,
    last_roll: Option<String>,
    cur_combatant: Option<i32>
}

impl Conn {
    fn msg(&mut self, to: &str, msg: &str) -> Result<()> {
        let m = Message::Text { body: msg.into() };
        self.client.send(to, m)?;
        ::std::thread::sleep(::std::time::Duration::from_millis(250));
        Ok(())
    }
    fn read_helpfile(&mut self, name: &str) -> Result<String> {
        if !name.is_alphanumeric() {
            bail!("Helpfile topic names are alphanumeric.");
        }
        let path = format!("./help/{}.txt", name);
        let file = File::open(path)?;
        let mut buf_reader = BufReader::new(file);
        let mut contents = String::new();
        buf_reader.read_to_string(&mut contents)?;
        Ok(contents)
    }
    fn load_datafile(&mut self, to: &str, path: &str) -> Result<()> {
        self.msg(&to, &format!("Reading datafile from {}...", path))?;
        let df = File::open(path)?;
        let mut buf_reader = BufReader::new(df);
        let mut contents = String::new();
        buf_reader.read_to_string(&mut contents)?;
        self.msg(&to, "Parsing datafile...")?;
        let df: Datafile = serde_json::from_str(&contents)?;
        self.msg(&to, "Inserting data into database...")?;
        let Datafile { mut items, rooms, mut abilities, players, monsters, weapons } = df;
        for wpn in weapons {
            let Weapon { name, descrip, qty, player_id, damage_dice, attack_bonus } = wpn;
            abilities.push(NewAbility {
                name: format!("Attack using {}", name),
                descrip: format!("Weapon: {}", descrip),
                damage_dice: damage_dice,
                attack_bonus: attack_bonus,
                player_id: player_id,
                monster_id: None,
                uses: -1,
                uses_left: -1
            });
            items.push(NewItem { name, descrip, qty, player_id });
        }
        let n_items = diesel::insert(&items).into(schema::items::table)
            .execute(&self.db)?;
        let n_rooms = diesel::insert(&rooms).into(schema::rooms::table)
            .execute(&self.db)?;
        let n_abilities = diesel::insert(&abilities).into(schema::abilities::table)
            .execute(&self.db)?;
        let n_players = diesel::insert(&players).into(schema::players::table)
            .execute(&self.db)?;
        let n_monsters = diesel::insert(&monsters).into(schema::monsters::table)
            .execute(&self.db)?;
        self.msg(&to, &format!("Inserted {} item(s), {} room(s), {} abilities, {} player(s) and {} monster(s).", n_items, n_rooms, n_abilities, n_players, n_monsters))?;
        Ok(())
    }
    fn load_mons(&mut self, to: &str) -> Result<usize> {
        self.msg(&to, "Reading monsters from srd.json...")?;
        let mons = File::open("srd.json")?;
        let mut buf_reader = BufReader::new(mons);
        let mut contents = String::new();
        buf_reader.read_to_string(&mut contents)?;
        self.msg(&to, "Parsing monsters...")?;
        let mons: Vec<SrdMonster> = serde_json::from_str(&contents)?;
        self.msg(&to, &format!("{} monsters loaded", mons.len()))?;
        self.msg(&to, "Inserting monsters into database...")?;
        let mut inserted = 0;
        for m in mons {
            let SrdMonster { name, typ, armor_class, hit_points, strength, intelligence, dexterity,
                             constitution, wisdom, charisma, challenge_rating, special_abilities,
                             actions } = m;
            let newmons = NewMonster { name, typ, armor_class, hit_points, strength, intelligence, dexterity,
                         constitution, wisdom, charisma, challenge_rating };
            let newmons: Monster = diesel::insert(&newmons).into(schema::monsters::table)
                .get_result(&self.db)?;
            let special_abilities = special_abilities.into_iter()
                .map(|abi| {
                    let MonsterAbility { name, desc, damage_dice, attack_bonus } = abi;
                    NewAbility {
                        name: name,
                        descrip: desc,
                        damage_dice: damage_dice,
                        attack_bonus: attack_bonus,
                        uses_left: -1,
                        uses: -1,
                        monster_id: Some(newmons.id),
                        player_id: None
                    }
                })
                .collect::<Vec<_>>();
            let actions = actions.into_iter()
                .map(|abi| {
                    let MonsterAbility { name, desc, damage_dice, attack_bonus } = abi;
                    NewAbility {
                        name: name,
                        descrip: desc,
                        damage_dice: damage_dice,
                        attack_bonus: attack_bonus,
                        uses_left: -1,
                        uses: -1,
                        monster_id: Some(newmons.id),
                        player_id: None
                    }
                })
                .collect::<Vec<_>>();
            diesel::insert(&special_abilities).into(schema::abilities::table)
                .execute(&self.db)?;
            diesel::insert(&actions).into(schema::abilities::table)
                .execute(&self.db)?;
            inserted += 1;
        }
        Ok(inserted)
    }
    fn roll_dice(&mut self, spec: &str) -> Result<i64> {
        let res = panic::catch_unwind(|| {
            Roller::new(spec)
        }).map_err(|_| "Invalid dicespec".to_string())?;
        self.last_roll = Some(spec.into());
        Ok(res.total())
    }
    fn print_player(&mut self, p: &Player) -> String {
        format!("#{}: {} the {}. HP {} AC {}\nStr {} Int {} Dex {} Con {} Wis {} Cha {}",
                p.id,
                p.name,
                p.typ,
                p.hit_points,
                p.armor_class,
                p.strength,
                p.intelligence,
                p.dexterity,
                p.constitution,
                p.wisdom,
                p.charisma)
    }
    fn print_monster(&mut self, m: &Monster) -> String {
        format!("* {}, a {}. HP {} AC {}", m.name, m.typ, m.hit_points, m.armor_class)
    }
    fn wound_descriptions(c: i32, m: i32) -> &'static str {
        let perc = ((c as f64 / m as f64) * 100.0f64) as i32;
        match perc {
            x if x > 100 => "radiating health",
            100 => "completely unscathed",
            75...99 => "a trifle bashed up",
            40...74 => "mildly bleeding",
            20...39 => "heavily bleeding",
            1...19 => "on the road to Deadsville",
            _ => "slightly dead"
        }
    }
    fn print_combatant(&mut self, c: &Combatant, short: bool) -> String {
        let mut msg = String::new();
        if c.monster_id.is_some() {
            msg = format!("* {} is {} [initiative: {}]",
                          c.name, Self::wound_descriptions(c.cur_hp, c.max_hp), c.initiative);
        }
        else {
            msg = format!("* {} - HP {}/{} [initiative: {}]",
                          c.name, c.cur_hp, c.max_hp, c.initiative);
        }
        if !short {
            msg.push_str(&format!("\nAC: {}\nHP: {}/{}\nMonster id: {:?}\nPlayer id: {:?}\nAttack dice: {}\nTo hit: {}",
                                  c.armor_class,
                                  c.cur_hp,
                                  c.max_hp,
                                  c.monster_id,
                                  c.player_id,
                                  c.attack,
                                  c.attack_bonus));
        }
        msg
    }
    fn print_combatants(&mut self) -> Result<String> {
        let r = cdsl::combatants.order(cdsl::initiative.desc()).load::<Combatant>(&self.db)?;
        let mut msg = format!("{} combatants active:", r.len());
        for c in r {
            msg.push_str("\n");
            msg.push_str(&self.print_combatant(&c, true));
        }
        Ok(msg)
    }
    fn print_item(&mut self, i: &Item, short: bool) -> String {
        let qty = if i.qty == -1 { "∞".into() } else { i.qty.to_string() };
        let mut ret = format!("#{}: {}x {}", i.id, qty, i.name);
        if !short {
            for line in i.descrip.lines() {
                ret.push_str("\n");
                ret.push_str(line);
            }
        }
        ret
    }
    fn print_ability(&mut self, a: &Ability, short: bool) -> String {
        let dmg = if let Some(ref dice) = a.damage_dice {
            format!(" [dmg {}]", dice)
        } else { "".into() };
        let atkb = if let Some(ref ab) = a.attack_bonus {
            format!(" [+{} to hit]", ab)
        } else { "".into() };
        let uses = if a.uses_left == -1 { "∞".into() } else { a.uses_left.to_string() };
        let mut ret = format!("#{}: {}x {}{}{}", a.id, uses, a.name, dmg, atkb);
        if !short {
            for line in a.descrip.lines() {
                ret.push_str("\n");
                ret.push_str(line);
            }
        }
        ret
    }
    fn print_items(&mut self, items: &[Item], short: bool) -> String {
        if items.len() == 0 {
            return "No results found.".into();
        }
        let mut ret = String::new();
        for i in items.iter() {
            if ret != "" {
                ret.push_str("\n");
            }
            ret.push_str(&self.print_item(i, short));
        }
        ret
    }
    fn print_abilities(&mut self, abilitys: &[Ability], short: bool) -> String {
        if abilitys.len() == 0 {
            return "No results found.".into();
        }
        let mut ret = String::new();
        for i in abilitys.iter() {
            if ret != "" {
                ret.push_str("\n");
            }
            ret.push_str(&self.print_ability(i, short));
        }
        ret
    }
    fn print_player_items(&mut self, pid: i32) -> Result<String> {
        let results = idsl::items.filter(idsl::player_id.eq(pid))
            .order(idsl::qty.desc())
            .load::<Item>(&self.db)?;
        Ok(self.print_items(&results, true))
    }
    fn print_player_abilities(&mut self, pid: i32) -> Result<String> {
        let results = adsl::abilities.filter(adsl::player_id.eq(pid))
            .order(adsl::uses_left.desc())
            .load::<Ability>(&self.db)?;
        Ok(self.print_abilities(&results, true))
    }
    fn monster_to_combatant(&mut self, mons: &Monster) -> Result<Combatant> {
        let comb = NewCombatant {
            name: &mons.name,
            attack: "1d1",
            max_hp: mons.hit_points,
            cur_hp: mons.hit_points,
            armor_class: mons.armor_class,
            monster_id: Some(mons.id),
            player_id: None,
        };
        let res = diesel::insert(&comb).into(cdsl::combatants)
            .get_result(&self.db)?;
        Ok(res)
    }
    fn player_to_combatant(&self, p: &Player) -> Result<Combatant> {
        let comb = NewCombatant {
            name: &p.name,
            attack: "1d1",
            max_hp: p.hit_points,
            cur_hp: p.hit_points,
            armor_class: p.armor_class,
            player_id: Some(p.id),
            monster_id: None
        };
        let res = diesel::insert(&comb).into(cdsl::combatants)
            .get_result(&self.db)?;
        Ok(res)
    }
    fn authenticate_nick(&mut self, nick: &str) -> Result<Player> {
        Ok(pdsl::players.filter(pdsl::nick.eq(nick))
            .get_result(&self.db)?)
    }
    fn authenticate_nick_or_dm(&mut self, code: &str, nick: &str) -> Result<Player> {
        match code.parse::<i32>() {
            Ok(x) => {
                self.check_admin(nick)?;
                Ok(pdsl::players.filter(pdsl::id.eq(x))
                   .get_result(&self.db)?)
            },
            Err(_) => {
                Ok(self.authenticate_nick(&nick)?)
            }
        }
    }
    fn check_admin(&mut self, nick: &str) -> Result<()> {
        if nick != self.admin {
            Err("You're not the DM.".into())
        }
        else {
            Ok(())
        }
    }
    fn query_monster(&mut self, q: &str) -> Result<Monster> {
        let q = format!("%{}%", q.to_lowercase());
        let mons = mdsl::monsters.filter(lower(mdsl::name).like(q))
            .get_result::<Monster>(&self.db)?;
        Ok(mons)
    }
    fn query_ability(&mut self, id: &str) -> Result<Ability> {
        let id = id.parse::<i32>()?;
        let item = adsl::abilities.filter(adsl::id.eq(id))
            .get_result::<Ability>(&self.db)?;
        Ok(item)
    }
    fn query_item(&mut self, id: &str) -> Result<Item> {
        let id = id.parse::<i32>()?;
        let item = idsl::items.filter(idsl::id.eq(id))
            .get_result::<Item>(&self.db)?;
        Ok(item)
    }
    fn query_combatant(&mut self, id: &str) -> Result<Combatant> {
        let id = format!("%{}%", id.to_lowercase());
        let item = cdsl::combatants.filter(lower(cdsl::name).like(id))
            .get_result::<Combatant>(&self.db)?;
        Ok(item)
    }
    fn query_player(&mut self, id: &str) -> Result<Player> {
        let id = format!("%{}%", id.to_lowercase());
        let item = pdsl::players.filter(lower(pdsl::name).like(id))
            .get_result::<Player>(&self.db)?;
        Ok(item)
    }
    fn get_current_combatant(&mut self) -> Result<Combatant> {
        let id = self.cur_combatant.ok_or("An encounter is not taking place.")?;
        let comb = cdsl::combatants.filter(cdsl::id.eq(id))
            .get_result::<Combatant>(&self.db)?;
        Ok(comb)
    }
    fn describe_turn_options(&mut self) -> Result<String> {
        let cc = self.get_current_combatant()?;
        let mut ret = "Abilities for the current combatant:\n\n".to_string();
        let mut abis = vec![];
        if let Some(pid) = cc.player_id {
            abis = adsl::abilities.filter(adsl::player_id.eq(pid))
                .get_results(&self.db)?;
        }
        else if let Some(mid) = cc.monster_id {
            abis = adsl::abilities.filter(adsl::monster_id.eq(mid))
                .get_results(&self.db)?;
        };
        if abis.len() > 0 {
            ret += &self.print_abilities(&abis, true);
        }
        else {
            ret += "[There don't seem to be any.]";
        }
        Ok(ret)
    }
    fn begin_encounter(&mut self) -> Result<String> {
        let mut ret = "Encounter!\n".to_string();
        let players = pdsl::players.order(pdsl::id.desc())
            .load::<Player>(&self.db)?;
        self.cur_combatant = None;
        for p in players {
            self.player_to_combatant(&p)?;
        }
        ret += &self.roll_initiative()?;
        ret += "\n\n";
        ret += &self.print_combatants()?;
        let first = cdsl::combatants.order(cdsl::initiative.desc())
            .limit(1)
            .get_result::<Combatant>(&self.db)?;
        ret += &format!("\n\nIt's now {}'s turn.\n", first.name);
        self.cur_combatant = Some(first.id);
        ret += &self.describe_turn_options()?;
        Ok(ret)
    }
    fn end_encounter(&mut self) -> Result<String> {
        diesel::delete(cdsl::combatants)
            .execute(&self.db)?;
        self.cur_combatant = None;
        Ok("Encounter ended.".to_string())
    }
    fn advance_turn(&mut self) -> Result<String> {
        let cc = self.cur_combatant.ok_or("It's nobody's turn!".to_string())?;
        let res = cdsl::combatants.order(cdsl::initiative.desc())
            .load::<Combatant>(&self.db)?;
        let mut ret = None;
        let mut last = -1i32;
        for (i, c) in res.into_iter().enumerate() {
            if i == 0 {
                last = c.id;
                ret = Some(c);
            }
            else if last == cc {
                ret = Some(c);
                break;
            }
            else {
                last = c.id;
            }
        }
        let ret = ret.ok_or("Something terrible has happened!".to_string())?;
        self.cur_combatant = Some(ret.id);
        Ok(format!("It's now {}'s turn.\n{}", ret.name, self.describe_turn_options()?))
    }
    fn roll_initiative(&mut self) -> Result<String> {
        let res = cdsl::combatants.load::<Combatant>(&self.db)?;
        let mut ret = "Rolling initiative...\n".to_string();
        for c in res {
            let initiative = if let Some(pid) = c.player_id {
                let player = pdsl::players.filter(pdsl::id.eq(pid))
                    .get_result::<Player>(&self.db)?;
                let roll = self.roll_dice("1d20")?;
                let result = roll + score_to_mod(player.dexterity) + (player.initiative_bonus as i64);
                ret.push_str(&format!("\n{} (player): [roll {}] + [dexmod {}] + [itvmod {}] => [initiative {}]",
                                      player.name,
                                      roll,
                                      score_to_mod(player.dexterity),
                                      player.initiative_bonus,
                                      result));
                result
            }
            else if let Some(mid) = c.monster_id {
                let mons = mdsl::monsters.filter(mdsl::id.eq(mid))
                    .get_result::<Monster>(&self.db)?;
                let roll = self.roll_dice("1d20")?;
                let result = roll + score_to_mod(mons.dexterity);
                ret.push_str(&format!("\n{} (monster): [roll {}] + [dexmod {}] => [initiative {}]",
                                      mons.name,
                                      roll,
                                      score_to_mod(mons.dexterity),
                                      result));
                result
            }
            else {
                let roll = self.roll_dice("1d20")?;
                ret.push_str(&format!("\n{} (???): [roll {}] => [initiative {}]",
                                      c.name,
                                      roll,
                                      roll));
                roll
            };
            diesel::update(cdsl::combatants.filter(cdsl::id.eq(c.id)))
                .set(cdsl::initiative.eq(initiative as i32))
                .execute(&self.db)?;
        }
        Ok(ret)
    }
    fn check(&mut self, player: &Player, axiom: &str) -> Result<String> {
        let axiom = match &axiom.to_lowercase() as &_ {
            "str" | "strength" => player.strength,
            "dex" | "dexterity" => player.dexterity,
            "wis" | "wisdom" => player.wisdom,
            "int" | "intelligence" => player.intelligence,
            "cha" | "charisma" => player.charisma,
            "con" | "constitution" => player.constitution,
            _ => bail!("Unknown player attribute")
        };
        let md = score_to_mod(axiom);
        let roll = self.roll_dice("1d20")?;
        if roll == 1 {
            Ok("CRITICAL FAILURE! (rolled a natural 1)".into())
        }
        else if roll == 20 {
            Ok("GREAT SUCCESS. (rolled a natural 20)".into())
        }
        else {
            Ok(format!("[roll {}] + [modifier {}] => result {}", roll, md, roll + md))
        }
    }
    fn attack(&mut self, from: &Combatant, to: &Combatant) -> Result<String> {
        let mut ret = String::new();
        ret.push_str(&format!("{} [to-hit: {}] attacks {} [AC: {}]!\n",
                              from.name,
                              from.attack_bonus,
                              to.name,
                              to.armor_class));
        let target = to.armor_class - from.attack_bonus;
        ret.push_str(&format!("Checking AC: dice roll required = {}\n", target));
        let roll = self.roll_dice("1d20")?;
        ret.push_str(&format!("rolling 1d20: result = {}\n\n", roll));
        if (roll < target as i64 && roll != 20) || roll == 1 {
            if roll == 1 {
                ret.push_str(&format!("CRITICAL FAILURE!"));
            }
            else {
                ret.push_str(&format!("Attack failed."));
            }
            return Ok(ret);
        }
        let mut dmg = self.roll_dice(&from.attack)?;
        ret.push_str(&format!("Dealing {} damage: result = {}\n", from.attack, dmg));
        if roll == 20 {
            dmg += self.roll_dice(&from.attack)?;
            ret.push_str(&format!("Critical hit! Dealing extra damage. New damage = {}\n", dmg));
        }
        let to = diesel::update(cdsl::combatants.filter(cdsl::id.eq(to.id)))
            .set(cdsl::cur_hp.eq(to.cur_hp - dmg as i32))
            .get_result::<Combatant>(&self.db)?;
        ret.push_str(&format!("Opponent's state after attack:\n\n{}", self.print_combatant(&to, true)));
        Ok(ret)
    }
    fn on_command(&mut self, nick: &str, to: &str, args: &[&str]) -> Result<()> {
        match &args as &[_] {
            &["ping"] => self.msg(to, "Pong!")?,
            &[topic @ "help"] | &["help", topic] => {
                let st = self.read_helpfile(topic)?;
                self.msg(to, &st)?;
            },
            &["join", roomid] => {
                self.check_admin(nick)?;
                self.client.join(roomid)?;
                self.msg(roomid, &format!("{} asked me to join, so here I am.", nick))?;
                self.msg(to, "Done.")?;
            },
            &["sql", query] => {
                self.check_admin(nick)?;
                let res = diesel::expression::dsl::sql::<diesel::types::Integer>(query)
                    .execute(&self.db);
                self.msg(to, &format!("Result: {:?}", res))?;
            },
            &[x @ "chk", what] | &[x @ "check", what] | &["pchk", x, what] => {
                let player = self.authenticate_nick_or_dm(x, nick)?;
                let st = self.check(&player, what)?;
                self.msg(&to, &st)?;
            }
            &[x @ "atk", tgt] | &[x @ "attack", tgt] | &["patk", x, tgt] => {
                let player = self.authenticate_nick_or_dm(x, nick)?;
                let comb = self.get_current_combatant()?;
                let tgt = self.query_combatant(tgt)?;
                if comb.player_id.is_none() || comb.player_id.unwrap() != player.id {
                    bail!("It's not your turn.");
                }
                let st = self.attack(&comb, &tgt)?;
                self.msg(&to, &st)?;
            },
            &["catk", tgt] => {
                let comb = self.get_current_combatant()?;
                let tgt = self.query_combatant(tgt)?;
                let st = self.attack(&comb, &tgt)?;
                self.msg(&to, &st)?;
            },
            &["unassigned", x @ "items"] | &["unassigned", x @ "abis"] => {
                self.check_admin(nick)?;
                if x == "items" {
                    let res = idsl::items.filter(idsl::player_id.is_null())
                        .load::<Item>(&self.db)?;
                    let st = self.print_items(&res, true);
                    self.msg(&to, &st)?;
                }
                else {
                    let res = adsl::abilities.filter(adsl::player_id.is_null())
                        .load::<Ability>(&self.db)?;
                    let st = self.print_abilities(&res, true);
                    self.msg(&to, &st)?;
                }
            },
            &[x @ "inventory"] |&[x @ "inv"] | &["pinv", x] => {
                let player = self.authenticate_nick_or_dm(x, nick)?;
                let st = self.print_player_items(player.id)?;
                self.msg(&to, &st)?;
            },
            &[x @ "abilities"] | &[x @ "abis"] | &["pabis", x] => {
                let player = self.authenticate_nick_or_dm(x, nick)?;
                let st = self.print_player_abilities(player.id)?;
                self.msg(&to, &st)?;
            },
            &["encounter", "begin"] => {
                self.check_admin(nick)?;
                let st = self.begin_encounter()?;
                self.msg(&to, &st)?;
            },
            &["encounter", "end"] => {
                self.check_admin(nick)?;
                let st = self.end_encounter()?;
                self.msg(&to, &st)?;
            },
            &["nextturn"] => {
                self.check_admin(nick)?;
                let st = self.advance_turn()?;
                self.msg(&to, &st)?;
            },
            &["init=", id, val] => {
                self.check_admin(nick)?;
                let val = val.parse::<i32>()?;
                let comb = self.query_combatant(id)?;
                diesel::update(cdsl::combatants.filter(cdsl::id.eq(comb.id)))
                    .set(cdsl::initiative.eq(val))
                    .execute(&self.db)?;
                let st = self.print_combatant(&comb, true);
                self.msg(&to, &st)?;
            },
            &["mabis", id] => {
                let mons = self.query_monster(id)?;
                self.msg(to, &format!("Abilities for a(n) {}:", mons.name))?;
                let abis = adsl::abilities.filter(adsl::monster_id.eq(mons.id))
                    .get_results::<Ability>(&self.db)?;
                let st = self.print_abilities(&abis, false);
                self.msg(&to, &st)?;
            },
            &["mtoc", id] => {
                let mons = self.query_monster(id)?;
                let comb = self.monster_to_combatant(&mons)?;
                let st = self.print_combatant(&comb, true);
                self.msg(&to, &st)?;
            },
            &["ptoc", id] => {
                let player = self.query_player(id)?;
                let comb = self.player_to_combatant(&player)?;
                let st = self.print_combatant(&comb, true);
                self.msg(&to, &st)?;
            },
            &["idesc", id] | &["describe", "item", id] => {
                let item = self.query_item(id)?;
                let st = self.print_item(&item, false);
                self.msg(&to, &st)?;
            },
            &["adesc", id] | &["describe", "ability", id] => {
                let item = self.query_ability(id)?;
                let st = self.print_ability(&item, false);
                self.msg(&to, &st)?;
            },
            &["cdesc", id] | &["describe", "combatant", id] => {
                let comb = self.query_combatant(id)?;
                let st;
                if let Ok(_) = self.check_admin(nick) {
                    st = self.print_combatant(&comb, false);
                }
                else {
                    st = self.print_combatant(&comb, true);
                }
                self.msg(&to, &st)?;
            },
            &["set_current_combatant", id] => {
                let id = id.parse::<i32>()?;
                self.cur_combatant = Some(id);
                self.msg(&to, "Done.")?;
            },
            &[x @ "use", id] | &["puse", x, id] => {
                let id = id.parse::<i32>()?;
                let player = self.authenticate_nick_or_dm(x, nick)?;
                let abi = adsl::abilities.filter(adsl::id.eq(id))
                    .filter(adsl::player_id.eq(player.id))
                    .get_result::<Ability>(&self.db)?;
                if abi.uses_left == 0 {
                    bail!("That ability has no uses left!");
                }
                let mut ret = format!("{} uses {}!\n", player.name, abi.name);
                let st = self.print_ability(&abi, false);
                ret.push_str(&st);
                if let Some(ref dice) = abi.damage_dice {
                    let comb = diesel::update(cdsl::combatants.filter(cdsl::player_id.eq(player.id)))
                        .set(cdsl::attack.eq(dice))
                        .get_result::<Combatant>(&self.db)?;
                    ret.push_str(&format!("\n{}'s new attack: {}", comb.name, comb.attack));
                }
                if let Some(ab) = abi.attack_bonus {
                    let comb = diesel::update(cdsl::combatants.filter(cdsl::player_id.eq(player.id)))
                        .set(cdsl::attack_bonus.eq(ab))
                        .get_result::<Combatant>(&self.db)?;
                    ret.push_str(&format!("\n{}'s new to hit bonus: {}", comb.name, comb.attack_bonus));
                }
                if abi.uses_left != -1 {
                    diesel::update(adsl::abilities.filter(adsl::id.eq(abi.id)))
                        .set(adsl::uses_left.eq(abi.uses_left - 1))
                        .execute(&self.db)?;
                }
                self.msg(&to, &st)?;
            },
            &["cuse", id] => {
                self.check_admin(&nick)?;
                let id = id.parse::<i32>()?;
                let comb = self.get_current_combatant()?;
                let mons_id = comb.monster_id.ok_or("The current combatant isn't a monster.")?;
                let abi = adsl::abilities.filter(adsl::id.eq(id))
                    .filter(adsl::monster_id.eq(mons_id))
                    .get_result::<Ability>(&self.db)?;
                let mut ret = format!("{} uses {}!\n", comb.name, abi.name);
                let st = self.print_ability(&abi, false);
                ret.push_str(&st);
                if let Some(ref dice) = abi.damage_dice {
                    let comb = diesel::update(cdsl::combatants.filter(cdsl::id.eq(comb.id)))
                        .set(cdsl::attack.eq(dice))
                        .get_result::<Combatant>(&self.db)?;
                    ret.push_str(&format!("\n{}'s new attack: {}", comb.name, comb.attack));
                }
                if let Some(ab) = abi.attack_bonus {
                    let comb = diesel::update(cdsl::combatants.filter(cdsl::id.eq(comb.id)))
                        .set(cdsl::attack_bonus.eq(ab))
                        .get_result::<Combatant>(&self.db)?;
                    ret.push_str(&format!("\n{}'s new to hit bonus: {}", comb.name, comb.attack_bonus));
                }
                self.msg(&to, &st)?;
            },
            &["findmons", id] => {
                let id = format!("%{}%", id.to_lowercase());
                let mons = mdsl::monsters.filter(lower(mdsl::name).like(id))
                    .load::<Monster>(&self.db)?;
                if mons.len() == 0 {
                    bail!("No results found.");
                }
                let mut st = String::new();
                for m in mons {
                    if st != "" {
                        st.push_str("\n");
                    }
                    let x = self.print_monster(&m);
                    st.push_str(&x);
                }
                self.msg(&to, &st)?;
            },
            &["loadmons"] => {
                self.check_admin(nick)?;
                let res = self.load_mons(to)?;
                self.msg(to, &format!("{} monsters loaded.", res))?;
            },
            &["loaddata", path] => {
                self.check_admin(nick)?;
                self.load_datafile(to, path)?;
            },
            &["whoami"] => {
                if let Ok(_) = self.check_admin(nick) {
                    self.msg(to, &format!("You're the DM!"))?;
                }
                else {
                    let player = self.authenticate_nick(nick)?;
                    let st = self.print_player(&player);
                    self.msg(&to, &st)?;
                }
            },
            &["players"] => {
                let players = pdsl::players.order(pdsl::id.desc())
                    .load::<Player>(&self.db)?;
                let mut st = String::new();
                for p in players {
                    if st != "" {
                        st.push_str("\n");
                    }
                    let x = self.print_player(&p);
                    st.push_str(&x);
                };
                self.msg(&to, &st)?;
            },
            &["combatants"] => {
                let st = self.print_combatants()?;
                self.msg(&to, &st)?
            },
            &["newcombat", name, attack, max_hp, armor_class] => {
                self.check_admin(nick)?;
                let max_hp = max_hp.parse::<i32>()?;
                let armor_class = armor_class.parse::<i32>()?;
                self.roll_dice(attack)?;
                let nm = NewCombatant {
                    name: name,
                    attack: attack,
                    max_hp: max_hp,
                    cur_hp: max_hp,
                    armor_class: armor_class,
                    monster_id: None,
                    player_id: None
                };
                let x = format!("Result: {:?}", diesel::insert(&nm).into(schema::combatants::table)
                                .execute(&self.db));
                self.msg(to, &x)?;
            },
            &["atk=", id, attack] => {
                self.check_admin(nick)?;
                self.roll_dice(attack)?;
                let comb = self.query_combatant(id)?;
                let x = diesel::update(cdsl::combatants.filter(cdsl::id.eq(comb.id)))
                    .set(cdsl::attack.eq(attack))
                    .get_result::<Combatant>(&self.db)?;
                self.msg(to, &format!("{}'s new attack: {}", x.name, x.attack))?;
            },
            &[a @ "hp", id, val] | &[a @ "hp=", id, val] => {
                self.check_admin(nick)?;
                let comb = self.query_combatant(id)?;
                let mut val = val.parse::<i32>()?;
                if a == "hp" {
                    val = comb.cur_hp + val;
                }
                let x = diesel::update(cdsl::combatants.filter(cdsl::id.eq(comb.id)))
                    .set(cdsl::cur_hp.eq(val))
                    .get_result::<Combatant>(&self.db)?;
                let st = self.print_combatant(&x, true);
                self.msg(&to, &st)?;
            },
            &["roll", dice] => {
                let r = self.roll_dice(dice)?;
                self.msg(to, &format!("rolling {}: {}", dice, r))?;
            },
            &["reroll"] => {
                match self.last_roll.clone() {
                    Some(s) => {
                        let r = self.roll_dice(&s)?;
                        self.msg(to, &format!("rerolling {}: {}", s, r))?;
                    },
                    None => self.msg(to, &format!("You'll have to roll something first."))?
                }
            },
            &["quit"] => {
                self.check_admin(nick)?;
                self.end_encounter()?;
                self.msg(to, &format!("So long, and thanks for all the fish!"))?;
                panic!("They asked us to quit, so we did.");
            },
            a @ _ => bail!(format!("Bad command: {:?}", a))
        };
        Ok(())
    }
    fn on_new_msg(&mut self, nick: &str, to: &str, mut msg: String) {
        println!("<{}> to {}: {}", nick, to, msg);
        if nick == self.client.user_id() { return; }
        if msg.len() < 1 { return; }
        if msg.chars().nth(0).unwrap() == ',' {
            msg.remove(0);
            let args = msg.split("/").collect::<Vec<&str>>();
            if let Err(e) = self.on_command(&nick, &to, &args) {
                println!("<{}> encountered error: {}", nick, e);
                let _ = self.msg(&to, &format!("ERROR: {}", e));
            }
            else {
                println!("<{}> executed command: {:?}", nick, args);
            }
        }
    }
    fn main(&mut self) -> Result<()> {
        let sync = self.client.sync(10000)?;
        for (rid, room) in sync.rooms.join {
            for event in room.timeline.events {
                match event.content {
                    Content::RoomMessage(m) => {
                        if let Message::Text { body } = m {
                            self.on_new_msg(&event.sender, &rid, body);
                        }
                    },
                    _ => {}
                }
                self.client.read_receipt(&rid, &event.event_id)?;
            }
        }
        Ok(())
    }
}

fn main() {
    println!("[+] Reading environment variables");
    dotenv().ok();
    let server = env::var("SERVER").expect("set the server variable");
    let username = env::var("USERNAME").expect("set the username variable");
    let password = env::var("PASSWORD").expect("set the password variable");
    let admin = env::var("ADMIN").expect("set the admin variable");
    println!("[+] Establishing a database connection");
    let connection = establish_connection();
    println!("[+] Establishing a Matrix connection");
    let mut client = MatrixClient::login(&username, &password, &server).unwrap();
    println!("[+] Performing initial sync");
    client.sync(0).unwrap();
    println!("[+] Setting online status");
    client.update_presence(Presence::Online).unwrap();
    println!("[+] Starting event loop!");
    let mut conn = Conn {
        client: client,
        last_roll: None,
        db: connection,
        admin: admin,
        cur_combatant: None
    };
    loop {
        conn.main().unwrap();
    }
}
