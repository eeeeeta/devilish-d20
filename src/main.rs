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
#[macro_use] extern crate ketos;

use ketos::{ForeignValue, Interpreter};
use ketos::Value as KetosValue;
use ketos::Error as KetosError;
type KetosResult<T> = ::std::result::Result<T, KetosError>;
use diesel::prelude::*;
use diesel::pg::PgConnection;
use diesel::ArrayExpressionMethods;
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
use std::rc::Rc;
use std::cell::RefCell;

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
        errors {
            Ketos(d: String) {
                display("Error in Ketos code: {}", d)
            }
        }
    }
    use ketos::Error as KError;
    impl From<KError> for Error {
        fn from(e: KError) -> Error {
            Error::from_kind(ErrorKind::Ketos(format!("{:?}", e)))
        }
    }
}
use errors::*;
pub mod schema;
pub mod models;
pub mod import;
pub mod matrix;
pub mod scripts;
use import::{SrdMonster, Weapon, MonsterAbility, Datafile};
use schema::combatants::dsl as cdsl;
use schema::monsters::dsl as mdsl;
use schema::abilities::dsl as adsl;
use schema::players::dsl as pdsl;
use schema::rooms::dsl as rdsl;
use schema::items::dsl as idsl;
use schema::spells::dsl as sdsl;
use schema::buffs::dsl as bdsl;
use models::*;
use models::Room;

sql_function!(lower, lower_t, (a: diesel::types::VarChar) -> diesel::types::VarChar);

pub fn establish_connection() -> PgConnection {
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    PgConnection::establish(&database_url)
        .expect(&format!("Error connecting to {}", database_url))
}
pub fn roll_dice(spec: &str) -> Result<i64> {
    panic::catch_unwind(|| {
        Roller::new(spec).total()
    }).map_err(|_| "Invalid dicespec".into())
}
pub fn k_roll_dice(spec: &str) -> KetosResult<i64> {
    roll_dice(spec).map_err(|e| (Box::new(e) as Box<::std::error::Error>).into())
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
    client: Rc<RefCell<MatrixClient>>,
    admin: String,
    db: Rc<RefCell<PgConnection>>,
    interp: Interpreter,
    last_roll: Option<String>,
    cur_combatant: Option<i32>,
    cur_room: Option<i32>
}

impl Conn {
    fn msg(&mut self, to: &str, msg: &str) -> Result<()> {
        let m = Message::Notice { body: msg.into(), formatted_body: Some(msg.replace("\n", "<br/>")), format: Some("org.matrix.custom.html".into()) };
        self.client.borrow_mut().send(to, m)?;
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
    fn load_spells(&mut self, to: &str) -> Result<usize> {
        self.msg(&to, "Reading spells from srd_spells.json...")?;
        let mons = File::open("srd_spells.json")?;
        let mut buf_reader = BufReader::new(mons);
        let mut contents = String::new();
        buf_reader.read_to_string(&mut contents)?;
        self.msg(&to, "Parsing spells...")?;
        let mons: Vec<NewSpell> = serde_json::from_str(&contents)?;
        self.msg(&to, &format!("{} spells in file.", mons.len()))?;
        let n_spells = diesel::insert(&mons)
            .into(schema::spells::table)
            .execute(&*self.db.borrow())?;
        Ok(n_spells)
    }
    fn print_spell(&mut self, spell: &Spell, short: bool) -> String {
        let mut ret = format!("#{}: <b>{}</b> ({})",
                              spell.id,
                              spell.name,
                              spell.typ);
        if !short {
            ret += &format!("\nrange {} | time {} | level {}\n\n{}",
                            spell.range,
                            spell.casting_time,
                            spell.level,
                            spell.descrip);
        }
        ret
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
        let Datafile { mut items, rooms, mut abilities, players, monsters, weapons, buffs } = df;
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
            let room_id = None;
            items.push(NewItem { name, descrip, qty, player_id, room_id });
        }
        let n_items = diesel::insert(&items).into(schema::items::table)
            .execute(&*self.db.borrow())?;
        let n_rooms = diesel::insert(&rooms).into(schema::rooms::table)
            .execute(&*self.db.borrow())?;
        let n_abilities = diesel::insert(&abilities).into(schema::abilities::table)
            .execute(&*self.db.borrow())?;
        let n_players = diesel::insert(&players).into(schema::players::table)
            .execute(&*self.db.borrow())?;
        let n_monsters = diesel::insert(&monsters).into(schema::monsters::table)
            .execute(&*self.db.borrow())?;
        let n_buffs = diesel::insert(&buffs).into(schema::buffs::table)
            .execute(&*self.db.borrow())?;
        self.msg(&to, &format!("Inserted {} item(s), {} room(s), {} abilities, {} player(s), {} buff(s) and {} monster(s).", n_items, n_rooms, n_abilities, n_players, n_buffs, n_monsters))?;
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
        self.msg(&to, &format!("{} monsters in file.", mons.len()))?;
        self.msg(&to, "Inserting monsters into database...")?;
        let mut inserted = 0;
        for m in mons {
            let SrdMonster { name, typ, armor_class, hit_points, strength, intelligence, dexterity,
                             constitution, wisdom, charisma, challenge_rating, special_abilities,
                             actions } = m;
            let room_id = vec![];
            let newmons = NewMonster { name, typ, armor_class, hit_points, strength, intelligence, dexterity,
                         constitution, wisdom, charisma, challenge_rating, room_id };
            let newmons: Monster = diesel::insert(&newmons).into(schema::monsters::table)
                .get_result(&*self.db.borrow())?;
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
                .execute(&*self.db.borrow())?;
            diesel::insert(&actions).into(schema::abilities::table)
                .execute(&*self.db.borrow())?;
            inserted += 1;
        }
        Ok(inserted)
    }
    fn roll_dice(&mut self, spec: &str) -> Result<i64> {
        let roll = roll_dice(spec)?;
        self.last_roll = Some(spec.into());
        Ok(roll)
    }
    fn print_player(&mut self, p: &Player) -> String {
        format!("#{}: <b>{}</b> the {} HP {} AC {}\nStr {} <i>({})</i> Int {} <i>({})</i> Dex {} <i>({})</i> Con {} <i>({})</i> Wis {} <i>({})</i> Cha {} <i>({})</i>{}",
                p.id,
                p.name,
                p.typ,
                p.hit_points,
                p.armor_class,
                p.strength,
                score_to_mod(p.strength),
                p.intelligence,
                score_to_mod(p.intelligence),
                p.dexterity,
                score_to_mod(p.dexterity),
                p.constitution,
                score_to_mod(p.constitution),
                p.wisdom,
                score_to_mod(p.wisdom),
                p.charisma,
                score_to_mod(p.charisma),
                if p.buffs.len() < 1 {
                    "".into()
                }
                else {
                    format!("\nActive buffs: <b>{}</b>", p.buffs.join(", "))
                }
        )
    }
    fn print_monster(&mut self, m: &Monster) -> String {
        format!("* <b>{}</b>, a {}. HP {} AC {}", m.name, m.typ, m.hit_points, m.armor_class)
    }
    fn wound_descriptions(c: i32, m: i32) -> &'static str {
        let perc = ((c as f64 / m as f64) * 100.0f64) as i32;
        match perc {
            x if x > 100 => "<font color=\"green\">radiating health</font>",
            100 => "<font color=\"green\">completely unscathed</font>",
            75...99 => "<font color=\"blue\">a trifle bashed up</font>",
            40...74 => "<font color=\"orange\">mildly bleeding</font>",
            20...39 => "<font color=\"red\">heavily bleeding</font>",
            1...19 => "<font color=\"darkred\">on the road to Deadsville</font>",
            _ => "<font color=\"darkred\">slightly dead</font>"
        }
    }
    fn print_combatant(&mut self, c: &Combatant, short: bool) -> String {
        let mut msg = String::new();
        if c.monster_id.is_some() {
            msg = format!("* <b>{}</b> is <i>{}</i> [initiative: {}]",
                          c.name, Self::wound_descriptions(c.cur_hp, c.max_hp), c.initiative);
        }
        else {
            msg = format!("* <b>{}</b> - HP {}/{} [initiative: {}]",
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
        let r = cdsl::combatants.order(cdsl::initiative.desc()).load::<Combatant>(&*self.db.borrow())?;
        let mut msg = format!("{} combatants active:", r.len());
        for c in r {
            msg.push_str("\n");
            msg.push_str(&self.print_combatant(&c, true));
        }
        Ok(msg)
    }
    fn print_item(&mut self, i: &Item, short: bool) -> String {
        let qty = if i.qty == -1 { "∞".into() } else { i.qty.to_string() };
        let mut ret = format!("#{}: {}x <b>{}</b>", i.id, qty, i.name);
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
        let mut ret = format!("#{}: {}x <b>{}</b>{}{}", a.id, uses, a.name, dmg, atkb);
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
            .load::<Item>(&*self.db.borrow())?;
        Ok(self.print_items(&results, true))
    }
    fn print_player_abilities(&mut self, pid: i32) -> Result<String> {
        let results = adsl::abilities.filter(adsl::player_id.eq(pid))
            .order(adsl::uses_left.desc())
            .load::<Ability>(&*self.db.borrow())?;
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
            .get_result(&*self.db.borrow())?;
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
            .get_result(&*self.db.borrow())?;
        Ok(res)
    }
    fn spell_to_player_ability(&mut self, p: &Player, s: &Spell) -> Result<Ability> {
        let abi = NewAbility {
            name: format!("Spell: {} ({})", s.name, s.typ),
            descrip: format!("range {} | time {}\n\n{}",
                             s.range,
                             s.casting_time,
                             s.descrip),
            uses: -1,
            damage_dice: None,
            attack_bonus: None,
            uses_left: -1,
            monster_id: None,
            player_id: Some(p.id)
        };
        let res = diesel::insert(&abi).into(adsl::abilities)
            .get_result(&*self.db.borrow())?;
        Ok(res)
    }
    fn print_room(&mut self, room: &Room) -> Result<String> {
        let mut ret = format!("* {}\n{}", room.name, room.descrip);
        let items = idsl::items.filter(idsl::room_id.eq(room.id))
            .load::<Item>(&*self.db.borrow())?;
        if items.len() > 0 {
            ret += "\n* There are the following items in this room:";
            for i in items {
                ret += "\n";
                ret += &self.print_item(&i, true);
            }
        }
        Ok(ret)
    }
    fn pick_up(&mut self, player: &Player, item: &Item) -> Result<String> {
        let room = self.get_current_room()?;
        if item.room_id.is_none() || item.room_id.unwrap() != room.id {
            bail!("That item isn't in the current room.");
        }
        if item.player_id.is_some() {
            bail!("Another player has that item (this error shouldn't occur)");
        }
        let room_id: Option<i32> = None;
        diesel::update(idsl::items.filter(idsl::id.eq(item.id)))
            .set((idsl::player_id.eq(player.id), idsl::room_id.eq(room_id)))
            .execute(&*self.db.borrow())?;
        let mut ret = format!("* {} picks up {}.\n", player.name, item.name);
        ret += &self.print_item(item, true);
        let mut ret = format!("* The item adds the following abilities:\n");
        let abis = diesel::update(adsl::abilities.filter(adsl::item_id.eq(item.id)))
            .set(adsl::player_id.eq(player.id))
            .get_results(&*self.db.borrow())?;
        ret += &self.print_abilities(&abis, true);
        Ok(ret)
    }
    fn drop(&mut self, player: &Player, item: &Item) -> Result<String> {
        let room = self.get_current_room()?;
        if item.player_id.is_none() || item.player_id.unwrap() != player.id {
            bail!("You don't own that item.");
        }
        let player_id: Option<i32> = None;
        diesel::update(idsl::items.filter(idsl::id.eq(item.id)))
            .set((idsl::player_id.eq(player_id), idsl::room_id.eq(room.id)))
            .execute(&*self.db.borrow())?;
        diesel::update(adsl::abilities.filter(adsl::item_id.eq(item.id)))
            .set(adsl::player_id.eq(player_id))
            .execute(&*self.db.borrow())?;
        Ok(format!("* {} drops {}.\n", player.name, item.name))
    }
    fn get_room_monsters(&mut self, room: &Room) -> Result<Vec<Monster>> {
        let mons = mdsl::monsters.filter(mdsl::room_id.contains(vec![room.id]))
            .load::<Monster>(&*self.db.borrow())?;
        Ok(mons)
    }
    fn enter_room(&mut self, room: &Room) -> Result<String> {
        self.end_encounter()?;
        let mut ret = self.print_room(room)?;
        let mons = self.get_room_monsters(room)?;
        if mons.len() > 0 {
            ret += "\n* There are the following monsters in this room:";
            for m in mons {
                let comb = self.monster_to_combatant(&m)?;
                ret += "\n";
                ret += &self.print_combatant(&comb, true);
            }
        }
        self.cur_room = Some(room.id);
        Ok(ret)
    }
    fn authenticate_nick(&mut self, nick: &str) -> Result<Player> {
        Ok(pdsl::players.filter(pdsl::nick.eq(nick))
            .get_result(&*self.db.borrow())?)
    }
    fn authenticate_nick_or_dm(&mut self, code: &str, nick: &str) -> Result<Player> {
        match code.parse::<i32>() {
            Ok(x) => {
                self.check_admin(nick)?;
                Ok(pdsl::players.filter(pdsl::id.eq(x))
                   .get_result(&*self.db.borrow())?)
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
            .get_result::<Monster>(&*self.db.borrow())?;
        Ok(mons)
    }
    fn query_ability(&mut self, id: &str) -> Result<Ability> {
        let id = id.parse::<i32>()?;
        let item = adsl::abilities.filter(adsl::id.eq(id))
            .get_result::<Ability>(&*self.db.borrow())?;
        Ok(item)
    }
    fn query_item(&mut self, id: &str) -> Result<Item> {
        let id = id.parse::<i32>()?;
        let item = idsl::items.filter(idsl::id.eq(id))
            .get_result::<Item>(&*self.db.borrow())?;
        Ok(item)
    }
    fn query_combatant(&mut self, id: &str) -> Result<Combatant> {
        let id = format!("%{}%", id.to_lowercase());
        let item = cdsl::combatants.filter(lower(cdsl::name).like(id))
            .get_result::<Combatant>(&*self.db.borrow())?;
        Ok(item)
    }
    fn query_buff(&mut self, id: &str) -> Result<Buff> {
        let id = format!("%{}%", id.to_lowercase());
        let item = bdsl::buffs.filter(lower(bdsl::name).like(id))
            .get_result::<Buff>(&*self.db.borrow())?;
        Ok(item)
    }
    fn query_player(&mut self, id: &str) -> Result<Player> {
        let id = format!("%{}%", id.to_lowercase());
        let item = pdsl::players.filter(lower(pdsl::name).like(id))
            .get_result::<Player>(&*self.db.borrow())?;
        Ok(item)
    }
    fn query_spell(&mut self, id: &str) -> Result<Spell> {
        let id = format!("%{}%", id.to_lowercase());
        let item = sdsl::spells.filter(lower(sdsl::name).like(id))
            .get_result::<Spell>(&*self.db.borrow())?;
        Ok(item)
    }
    fn query_room(&mut self, id: &str) -> Result<Room> {
        let item = rdsl::rooms.filter(lower(rdsl::shortid).eq(id))
            .get_result::<Room>(&*self.db.borrow())?;
        Ok(item)
    }
    fn get_current_combatant(&mut self) -> Result<Combatant> {
        let id = self.cur_combatant.ok_or("An encounter is not taking place.")?;
        let comb = cdsl::combatants.filter(cdsl::id.eq(id))
            .get_result::<Combatant>(&*self.db.borrow())?;
        Ok(comb)
    }
    fn get_current_room(&mut self) -> Result<Room> {
        let id = self.cur_room.ok_or("We're in limbo!")?;
        let room = rdsl::rooms.filter(rdsl::id.eq(id))
            .get_result::<Room>(&*self.db.borrow())?;
        Ok(room)
    }
    fn describe_turn_options(&mut self) -> Result<String> {
        let cc = self.get_current_combatant()?;
        let mut ret = "Abilities for the current combatant:\n\n".to_string();
        let mut abis = vec![];
        if let Some(pid) = cc.player_id {
            abis = adsl::abilities.filter(adsl::player_id.eq(pid))
                .get_results(&*self.db.borrow())?;
        }
        else if let Some(mid) = cc.monster_id {
            abis = adsl::abilities.filter(adsl::monster_id.eq(mid))
                .get_results(&*self.db.borrow())?;
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
            .load::<Player>(&*self.db.borrow())?;
        self.cur_combatant = None;
        for p in players {
            self.player_to_combatant(&p)?;
        }
        ret += &self.roll_initiative()?;
        ret += "\n\n";
        ret += &self.print_combatants()?;
        let first = cdsl::combatants.order(cdsl::initiative.desc())
            .limit(1)
            .get_result::<Combatant>(&*self.db.borrow())?;
        ret += &format!("\n\nIt's now {}'s turn.\n", first.name);
        self.cur_combatant = Some(first.id);
        ret += &self.describe_turn_options()?;
        Ok(ret)
    }
    fn end_encounter(&mut self) -> Result<String> {
        diesel::delete(cdsl::combatants)
            .execute(&*self.db.borrow())?;
        self.cur_combatant = None;
        Ok("Encounter ended.".to_string())
    }
    fn advance_turn(&mut self) -> Result<String> {
        let cc = self.cur_combatant.ok_or("It's nobody's turn!".to_string())?;
        let res = cdsl::combatants.order(cdsl::initiative.desc())
            .load::<Combatant>(&*self.db.borrow())?;
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
    fn load_buff(&mut self, name: &str) -> Result<()> {
        let buff = self.query_buff(name)?;
        self.interp.run_single_expr(&buff.code, None).map_err(|e| self.interp.format_error(&e))?;
        Ok(())
    }
    fn roll_initiative(&mut self) -> Result<String> {
        let res = cdsl::combatants.load::<Combatant>(&*self.db.borrow())?;
        let mut ret = "Rolling initiative...\n".to_string();
        for c in res {
            let initiative = if let Some(pid) = c.player_id {
                let player = pdsl::players.filter(pdsl::id.eq(pid))
                    .get_result::<Player>(&*self.db.borrow())?;
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
                    .get_result::<Monster>(&*self.db.borrow())?;
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
                .execute(&*self.db.borrow())?;
        }
        Ok(ret)
    }
    fn recover_uses(&mut self) -> Result<usize> {
        let changed = diesel::update(adsl::abilities)
            .set(adsl::uses_left.eq(adsl::uses))
            .execute(&*self.db.borrow())?;
        Ok(changed)
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
            Ok(format!("<i>[roll {}] + [modifier {}]</i> => result <b>{}</b>", roll, md, roll + md))
        }
    }
    fn attack(&mut self, from: &Combatant, to: &Combatant) -> Result<String> {
        let mut ret = String::new();
        ret.push_str(&format!("<b>{}</b> [to-hit: {}] attacks <b>{}</b> [AC: {}]!\n",
                              from.name,
                              from.attack_bonus,
                              to.name,
                              to.armor_class));
        let target = to.armor_class - from.attack_bonus;
        ret.push_str(&format!("Checking AC: dice roll required = <b>{}</b>\n", target));
        let roll = self.roll_dice("1d20")?;
        ret.push_str(&format!("rolling 1d20: result = <b>{}</b>\n\n", roll));
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
        ret.push_str(&format!("Dealing {} damage: result = <b>{}</b>\n", from.attack, dmg));
        if roll == 20 {
            dmg += self.roll_dice(&from.attack)?;
            ret.push_str(&format!("Critical hit! Dealing extra damage. New damage = <b>{}</b>\n", dmg));
        }
        let to = diesel::update(cdsl::combatants.filter(cdsl::id.eq(to.id)))
            .set(cdsl::cur_hp.eq(to.cur_hp - dmg as i32))
            .get_result::<Combatant>(&*self.db.borrow())?;
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
                self.client.borrow_mut().join(roomid)?;
                self.msg(roomid, &format!("{} asked me to join, so here I am.", nick))?;
                self.msg(to, "Done.")?;
            },
            &["sql", query] => {
                self.check_admin(nick)?;
                let res = diesel::expression::dsl::sql::<diesel::types::Integer>(query)
                    .execute(&*self.db.borrow());
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
                        .load::<Item>(&*self.db.borrow())?;
                    let st = self.print_items(&res, true);
                    self.msg(&to, &st)?;
                }
                else {
                    let res = adsl::abilities.filter(adsl::player_id.is_null())
                        .load::<Ability>(&*self.db.borrow())?;
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
            &["ketos", "eval", script] => {
                self.check_admin(nick)?;
                self.interp.run_code(script, None).map_err(|e| self.interp.format_error(&e))?;
                let val = self.interp.call("custom", vec![
                    to.into(),
                ]).map_err(|e| self.interp.format_error(&e))?;
                if let ketos::Value::Unit = val {
                    /* how boring! let's not print anything */
                }
                else {
                    let st = format!("Return value: {:?}", val);
                    self.msg(&to, &st)?;
                }
            },
            &["ketos", "player", player, script] => {
                self.check_admin(nick)?;
                let player = self.query_player(player)?;
                self.interp.run_code(script, None).map_err(|e| self.interp.format_error(&e))?;
                let val = self.interp.call("custom", vec![
                    to.into(),
                    player.id.into()
                ]).map_err(|e| self.interp.format_error(&e))?;
                if let ketos::Value::Unit = val {
                    /* how boring! let's not print anything */
                }
                else {
                    let st = format!("Return value: {:?}", val);
                    self.msg(&to, &st)?;
                }
            },
            &["buff", "describe", name] => {
                let buff = self.query_buff(name)?;
                let st = format!("#{}: <b>{}</b>\n{}", buff.id, buff.name, buff.descrip);
                self.msg(&to, &st)?;
            },
            &["buff", "add", player, name] => {
                self.check_admin(nick)?;
                let mut player = self.query_player(player)?;
                self.load_buff(name)?;
                self.interp.call("buff", vec![
                    to.into(),
                    player.id.into()
                ]).map_err(|e| self.interp.format_error(&e))?;
                player.buffs.push(name.into());
                diesel::update(pdsl::players.filter(pdsl::id.eq(player.id)))
                    .set(pdsl::buffs.eq(player.buffs))
                    .execute(&*self.db.borrow())?;
                self.msg(&to, "Buff applied.")?;
            },
            &["buff", "remove", player, name] => {
                self.check_admin(nick)?;
                let mut player = self.query_player(player)?;
                if !player.buffs.contains(&name.into()) {
                    bail!("No such buff is acting on that player at this time.");
                }
                self.load_buff(name)?;
                self.interp.call("debuff", vec![
                    to.into(),
                    player.id.into()
                ]).map_err(|e| self.interp.format_error(&e))?;
                player.buffs.retain(|b| b != name);
                diesel::update(pdsl::players.filter(pdsl::id.eq(player.id)))
                    .set(pdsl::buffs.eq(player.buffs))
                    .execute(&*self.db.borrow())?;
                self.msg(&to, "Buff removed.")?;
            },
            &["room", "enter", room] => {
                self.check_admin(nick)?;
                let rm = self.query_room(room)?;
                let st = self.enter_room(&rm)?;
                self.msg(&to, &st)?;
            },
            &["room", "describe", room] => {
                self.check_admin(nick)?;
                let rm = self.query_room(room)?;
                let st = self.print_room(&rm)?;
                self.msg(&to, &st)?;
            },
            &["look"] => {
                let rm = self.get_current_room()?;
                let st = self.print_room(&rm)?;
                self.msg(&to, &st)?;
            },
            &["teachspell", x, spell] => {
                let player = self.authenticate_nick_or_dm(x, nick)?;
                let spell = self.query_spell(spell)?;
                let abi = self.spell_to_player_ability(&player, &spell)?;
                let st = self.print_ability(&abi, false);
                self.msg(&to, &st)?;
            },
            &[x @ "pickup", item] | &["ppickup", x, item] => {
                let player = self.authenticate_nick_or_dm(x, nick)?;
                let item = self.query_item(item)?;
                let st = self.pick_up(&player, &item)?;
                self.msg(&to, &st)?;
            },
            &[x @ "drop", item] | &["pdrop", x, item] => {
                let player = self.authenticate_nick_or_dm(x, nick)?;
                let item = self.query_item(item)?;
                let st = self.drop(&player, &item)?;
                self.msg(&to, &st)?;
            },
            &["recover_uses"] => {
                self.check_admin(nick)?;
                let st = format!("{} abilities changed.", self.recover_uses()?);
                self.msg(&to, &st)?;
            }
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
                    .execute(&*self.db.borrow())?;
                let st = self.print_combatant(&comb, true);
                self.msg(&to, &st)?;
            },
            &["mabis", id] => {
                let mons = self.query_monster(id)?;
                self.msg(to, &format!("Abilities for a(n) {}:", mons.name))?;
                let abis = adsl::abilities.filter(adsl::monster_id.eq(mons.id))
                    .get_results::<Ability>(&*self.db.borrow())?;
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
            &["sdesc", id] | &["describe", "spell", id] => {
                let sp = self.query_spell(id)?;
                let st = self.print_spell(&sp, false);
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
                    .get_result::<Ability>(&*self.db.borrow())?;
                if abi.uses_left == 0 {
                    bail!("That ability has no uses left!");
                }
                let mut ret = format!("{} uses {}!\n", player.name, abi.name);
                let st = self.print_ability(&abi, false);
                ret.push_str(&st);
                if let Some(ref dice) = abi.damage_dice {
                    let comb = diesel::update(cdsl::combatants.filter(cdsl::player_id.eq(player.id)))
                        .set(cdsl::attack.eq(dice))
                        .get_result::<Combatant>(&*self.db.borrow())?;
                    ret.push_str(&format!("\n{}'s new attack: {}", comb.name, comb.attack));
                }
                if let Some(ab) = abi.attack_bonus {
                    let comb = diesel::update(cdsl::combatants.filter(cdsl::player_id.eq(player.id)))
                        .set(cdsl::attack_bonus.eq(ab))
                        .get_result::<Combatant>(&*self.db.borrow())?;
                    ret.push_str(&format!("\n{}'s new to hit bonus: {}", comb.name, comb.attack_bonus));
                }
                if abi.uses_left != -1 {
                    diesel::update(adsl::abilities.filter(adsl::id.eq(abi.id)))
                        .set(adsl::uses_left.eq(abi.uses_left - 1))
                        .execute(&*self.db.borrow())?;
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
                    .get_result::<Ability>(&*self.db.borrow())?;
                let mut ret = format!("{} uses {}!\n", comb.name, abi.name);
                let st = self.print_ability(&abi, false);
                ret.push_str(&st);
                if let Some(ref dice) = abi.damage_dice {
                    let comb = diesel::update(cdsl::combatants.filter(cdsl::id.eq(comb.id)))
                        .set(cdsl::attack.eq(dice))
                        .get_result::<Combatant>(&*self.db.borrow())?;
                    ret.push_str(&format!("\n{}'s new attack: {}", comb.name, comb.attack));
                }
                if let Some(ab) = abi.attack_bonus {
                    let comb = diesel::update(cdsl::combatants.filter(cdsl::id.eq(comb.id)))
                        .set(cdsl::attack_bonus.eq(ab))
                        .get_result::<Combatant>(&*self.db.borrow())?;
                    ret.push_str(&format!("\n{}'s new to hit bonus: {}", comb.name, comb.attack_bonus));
                }
                self.msg(&to, &st)?;
            },
            &["findmons", id] => {
                let id = format!("%{}%", id.to_lowercase());
                let mons = mdsl::monsters.filter(lower(mdsl::name).like(id))
                    .load::<Monster>(&*self.db.borrow())?;
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
            &["findspells", id] => {
                let id = format!("%{}%", id.to_lowercase());
                let mons = sdsl::spells.filter(lower(sdsl::name).like(id))
                    .load::<Spell>(&*self.db.borrow())?;
                if mons.len() == 0 {
                    bail!("No results found.");
                }
                let mut st = String::new();
                for m in mons {
                    if st != "" {
                        st.push_str("\n");
                    }
                    let x = self.print_spell(&m, true);
                    st.push_str(&x);
                }
                self.msg(&to, &st)?;
            },
            &["loadspells"] => {
                self.check_admin(nick)?;
                let res = self.load_spells(to)?;
                self.msg(to, &format!("{} spells loaded.", res))?;
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
                    .load::<Player>(&*self.db.borrow())?;
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
                                .execute(&*self.db.borrow()));
                self.msg(to, &x)?;
            },
            &["atk=", id, attack] => {
                self.check_admin(nick)?;
                self.roll_dice(attack)?;
                let comb = self.query_combatant(id)?;
                let x = diesel::update(cdsl::combatants.filter(cdsl::id.eq(comb.id)))
                    .set(cdsl::attack.eq(attack))
                    .get_result::<Combatant>(&*self.db.borrow())?;
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
                    .get_result::<Combatant>(&*self.db.borrow())?;
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
        if nick == self.client.borrow().user_id() { return; }
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
        let sync = self.client.borrow_mut().sync(10000)?;
        for (rid, room) in sync.rooms.join {
            for event in room.timeline.events {
                match event.content {
                    Content::RoomMessage(m) => {
                        if let Message::Text { body, .. } = m {
                            self.on_new_msg(&event.sender, &rid, body);
                        }
                    },
                    _ => {}
                }
                self.client.borrow_mut().read_receipt(&rid, &event.event_id)?;
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
    let connection = Rc::new(RefCell::new(establish_connection()));
    println!("[+] Establishing a Matrix connection");
    let client = Rc::new(RefCell::new(MatrixClient::login(&username, &password, &server).unwrap()));
    println!("[+] Initialising Ketos scripting environment");
    let interp = Interpreter::new();
    ketos_fn!{ interp.scope() => "roll" => fn k_roll_dice(spec: &str) -> i64 }
    scripts::register_players(interp.scope());
    scripts::register_matrix(interp.scope(), client.clone());
    interp.scope().add_named_value("db", ketos::Value::Foreign(Rc::new(scripts::Database {
        inner: connection.clone()
    })));
    println!("[+] Performing initial sync");
    client.borrow_mut().sync(0).unwrap();
    println!("[+] Setting online status");
    client.borrow_mut().update_presence(Presence::Online).unwrap();
    println!("[+] Starting event loop!");
    let mut conn = Conn {
        client: client,
        last_roll: None,
        db: connection,
        admin: admin,
        interp: interp,
        cur_combatant: None,
        cur_room: None
    };
    loop {
        conn.main().unwrap();
    }
}
