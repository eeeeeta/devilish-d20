use super::schema::monsters;
use super::models::{NewItem, NewRoom, NewAbility, NewPlayer, NewMonster, NewBuff};
#[derive(Serialize, Deserialize)]
pub struct Weapon {
    pub name: String,
    pub descrip: String,
    pub qty: i32,
    #[serde(default)]
    pub damage_dice: Option<String>,
    #[serde(default)]
    pub attack_bonus: Option<i32>,
    #[serde(default)]
    pub player_id: Option<i32>
}
#[derive(Serialize, Deserialize)]
pub struct Datafile {
    #[serde(default)]
    pub items: Vec<NewItem>,
    #[serde(default)]
    pub rooms: Vec<NewRoom>,
    #[serde(default)]
    pub abilities: Vec<NewAbility>,
    #[serde(default)]
    pub players: Vec<NewPlayer>,
    #[serde(default)]
    pub monsters: Vec<NewMonster>,
    #[serde(default)]
    pub weapons: Vec<Weapon>,
    #[serde(default)]
    pub buffs: Vec<NewBuff>
}
#[derive(Serialize, Deserialize)]
pub struct MonsterAbility {
    pub name: String,
    pub desc: String,
    #[serde(default)]
    pub damage_dice: Option<String>,
    #[serde(default)]
    pub attack_bonus: Option<i32>
}
#[derive(Serialize, Deserialize)]
pub struct SrdMonster {
    pub name: String,
    #[serde(rename = "type")]
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
    pub special_abilities: Vec<MonsterAbility>,
    #[serde(default)]
    pub actions: Vec<MonsterAbility>
}
