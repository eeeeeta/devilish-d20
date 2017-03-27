use super::schema::{combatants, monsters, abilities, items, rooms, players, props};

#[derive(Queryable)]
pub struct Monster {
    pub id: i32,
    pub name: String,
    pub typ: String,
    pub armor_class: i32,
    pub hit_points: i32,
    pub strength: i32,
    pub intelligence: i32,
    pub dexterity: i32,
    pub constitution: i32,
    pub wisdom: i32,
    pub charisma: i32,
    pub challenge_rating: String,
    pub room_id: Vec<i32>,
}
#[derive(Insertable, Serialize, Deserialize)]
#[table_name="monsters"]
pub struct NewMonster {
    pub name: String,
    pub typ: String,
    pub armor_class: i32,
    pub hit_points: i32,
    pub strength: i32,
    pub intelligence: i32,
    pub dexterity: i32,
    pub constitution: i32,
    pub wisdom: i32,
    pub charisma: i32,
    pub challenge_rating: String,
    #[serde(default)]
    pub room_id: Vec<i32>,
}
#[derive(Queryable)]
pub struct Player {
    pub id: i32,
    pub name: String,
    pub nick: String,
    pub typ: String,
    pub armor_class: i32,
    pub hit_points: i32,
    pub strength: i32,
    pub intelligence: i32,
    pub dexterity: i32,
    pub constitution: i32,
    pub wisdom: i32,
    pub charisma: i32,
    pub initiative_bonus: i32
}
#[derive(Insertable, Serialize, Deserialize)]
#[table_name="players"]
pub struct NewPlayer {
    pub id: Option<i32>,
    pub name: String,
    pub nick: String,
    pub typ: String,
    pub armor_class: i32,
    pub hit_points: i32,
    pub strength: i32,
    pub intelligence: i32,
    pub dexterity: i32,
    pub constitution: i32,
    pub wisdom: i32,
    pub charisma: i32,
    #[serde(default)]
    pub initiative_bonus: i32
}
#[derive(Queryable)]
pub struct Ability {
    pub id: i32,
    pub name: String,
    pub descrip: String,
    pub damage_dice: Option<String>,
    pub attack_bonus: Option<i32>,
    pub uses_left: i32,
    pub uses: i32,
    pub monster_id: Option<i32>,
    pub player_id: Option<i32>,
    pub item_id: Option<i32>
}
#[derive(Insertable, Serialize, Deserialize)]
#[table_name="abilities"]
pub struct NewAbility {
    pub name: String,
    pub descrip: String,
    #[serde(default)]
    pub damage_dice: Option<String>,
    #[serde(default)]
    pub attack_bonus: Option<i32>,
    #[serde(default)]
    pub uses_left: i32,
    pub uses: i32,
    #[serde(default)]
    pub monster_id: Option<i32>,
    #[serde(default)]
    pub player_id: Option<i32>
}
#[derive(Queryable)]
pub struct Room {
    pub id: i32,
    pub shortid: String,
    pub name: String,
    pub descrip: String
}
#[derive(Insertable, Serialize, Deserialize)]
#[table_name="rooms"]
pub struct NewRoom {
    pub shortid: String,
    pub name: String,
    pub descrip: String
}
#[derive(Queryable)]
pub struct Item {
    pub id: i32,
    pub name: String,
    pub descrip: String,
    pub qty: i32,
    pub player_id: Option<i32>,
    pub room_id: Option<i32>,
}
#[derive(Insertable, Serialize, Deserialize)]
#[table_name="items"]
pub struct NewItem {
    pub name: String,
    pub descrip: String,
    pub qty: i32,
    #[serde(default)]
    pub player_id: Option<i32>,
    #[serde(default)]
    pub room_id: Option<i32>
}
#[derive(Queryable, Clone)]
pub struct Combatant {
    pub id: i32,
    pub name: String,
    pub initiative: i32,
    pub cur_hp: i32,
    pub max_hp: i32,
    pub armor_class: i32,
    pub attack: String,
    pub attack_bonus: i32,
    pub player_id: Option<i32>,
    pub monster_id: Option<i32>
}

#[derive(Insertable)]
#[table_name="combatants"]
pub struct NewCombatant<'a> {
    pub name: &'a str,
    pub attack: &'a str,
    pub max_hp: i32,
    pub cur_hp: i32,
    pub armor_class: i32,
    pub monster_id: Option<i32>,
    pub player_id: Option<i32>,
}
#[derive(Serialize, Deserialize, Queryable)]
pub struct Property {
    pub id: i32,
    pub name: String,
    pub value: String
}
#[derive(Insertable)]
#[table_name="props"]
pub struct NewProperty<'a> {
    name: &'a str,
    value: &'a str
}
