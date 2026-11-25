//! Typed `maps/<id>/map.json` (layout: docs/data-format.md, docs/missions.md section 2) and its JSON Schema (`map.schema.json`).
//!
//! This is a typed view of what the engine already reads, not a format change. Every key keeps its name, type and meaning; every
//! field is optional; unknown keys are allowed and ignored. Each field is read as the `Value` readers read it (`as_str()`,
//! `as_f64()`, ... with a default): a value of the wrong type counts as absent, and an element of a list that is not an object reads as
//! an empty one. So `MapDoc::from_value` never fails, whatever the file holds, and returns what `map["key"].as_*()` would.
//!
//! The retail exporter (`tools/export_maps.py`, `tools/export_nebulae.py`) writes most keys; a custom mission or skirmish map may
//! write any subset. Which readers use this document and which still read `Value` is listed in `docs/data-format.md`.
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// A list read as `as_array().into_iter().flatten()` was: a value that is not an array is an empty list, and an element that does not
/// fit `T` is `T::default()` (all its fields absent), so the positions of the other elements do not change.
pub(crate) fn lenient_list<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(d: D) -> Result<Vec<T>, D::Error> {
    lenient_array(d).map(Option::unwrap_or_default)
}

/// Like [`lenient_list`], but `None` when the value is absent or not an array (a reader that tells "no list" from "an empty list").
pub(crate) fn lenient_array<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(d: D) -> Result<Option<Vec<T>>, D::Error> {
    Ok(match Value::deserialize(d) {
        Ok(Value::Array(items)) => Some(items.into_iter().map(|e| serde_json::from_value(e).unwrap_or_default()).collect()),
        _ => None,
    })
}

/// A list of numbers read by position (`p["pos"][i].as_f64()`): an element that is not a number is `None`, and the rest keep their index.
/// The schema says an array of numbers.
#[derive(Clone, Debug, Default, PartialEq, JsonSchema)]
#[schemars(with = "Vec<f64>")]
pub struct Numbers(pub Vec<Option<f64>>);
impl Numbers {
    /// The number at `i`, if there is one.
    pub fn at(&self, i: usize) -> Option<f64> {
        self.0.get(i).copied().flatten()
    }
    /// The numbers only, in order (as a reader that skips what is not a number sees the list).
    pub fn numbers(&self) -> impl Iterator<Item = f64> + '_ {
        self.0.iter().copied().flatten()
    }
}
impl<'de> Deserialize<'de> for Numbers {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Array(items) => Ok(Numbers(items.iter().map(Value::as_f64).collect())),
            other => Err(serde::de::Error::custom(format!("expected a list of numbers, found {other}"))),
        }
    }
}

/// `maps/<id>/map.json`. Unknown keys are allowed and ignored.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[schemars(title = "map.json")]
pub struct MapDoc {
    /// Editor hint: the schema this file follows. Ignored by the loader.
    #[serde(rename = "$schema", default, deserialize_with = "crate::data::lenient")]
    pub schema: Option<String>,
    /// Informational: the folder name is the map's id.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub id: Option<String>,
    /// Name shown in the skirmish and custom mission lists (default: the map's id, or empty).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub name: Option<String>,
    /// Description shown in the lists (default empty).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub description: Option<String>,
    /// `true`: the map is offered in the skirmish menu (default `false`).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub skirmish: Option<bool>,
    /// `true`: an open map where the players assemble their fleets; `false`: a historical map with pre-placed `ships` (default `false`).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub assemble: Option<bool>,
    /// A sky name: a retail one (`skybox_22_Layer1`, the `_LayerN` suffix is dropped) or a mod's own sky mesh or HD sky (default `skybox_07`).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub sky: Option<String>,
    /// Music track of a skirmish map (default `BTL_NavyBig_Full02`). Not read for a mission: its `Play Music Track` rule chooses.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub music: Option<String>,
    /// The star map picture: a name (retail) or `true`. Either one turns the picture on; the file is `maps/<id>/backdrop.png`. PROVISIONAL:
    /// the exporter writes the texture name; any other type counts as no backdrop.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub backdrop: Option<Backdrop>,
    /// The roof/floor/ambient light the map authors. Default: plain white light from above.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub lighting: Option<LightingDoc>,
    /// Half the side of the map's square (default 1000).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub world_half: Option<f64>,
    /// `true`: the lobby may change alliances (default `false`). Skirmish maps.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub alliance_change: Option<bool>,
    /// Retail flag, exported but not read by the engine (PROVISIONAL: a boolean; the exporter's default is `true`).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub islands_make_sounds: Option<bool>,
    /// Retail string ids of the map's name and description, exported but not read (PROVISIONAL: `name` and `description` are the English text).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub name_id: Option<String>,
    /// See `name_id`.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub desc_id: Option<String>,
    /// Lobby slots (skirmish). Not used by a custom mission, whose players are in `mission.json`.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<PlayerDoc>")]
    pub players: Vec<PlayerDoc>,
    /// Teams of a skirmish map, indexed by a player's `team`.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<TeamDoc>")]
    pub teams: Vec<TeamDoc>,
    /// Pre-placed ships of a historical skirmish map.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<MapShipDoc>")]
    pub ships: Vec<MapShipDoc>,
    /// Scenery: islands, asteroids, bases, black holes (`docs/missions.md`).
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<ObjectDoc>")]
    pub objects: Vec<ObjectDoc>,
    /// Clouds and storms.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<NebulaDoc>")]
    pub nebulae: Vec<NebulaDoc>,
    /// Mines resting where the map put them (skirmish maps, and custom missions).
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<MineDoc>")]
    pub mines: Vec<MineDoc>,
    /// The map's own end rules (skirmish): the first whose conditions all hold ends the game. A map without usable rules keeps the plain
    /// "no enemy ships left" outcome. (In `script.json`, `victory` is the string `"script"`; here it is a list.)
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<VictoryRuleDoc>")]
    pub victory: Vec<VictoryRuleDoc>,
}

impl MapDoc {
    /// The typed view of a `map.json` value. Never fails: what is not there or has the wrong type is absent (a file that is not an
    /// object has nothing).
    pub fn from_value(v: &Value) -> MapDoc {
        serde_json::from_value(v.clone()).unwrap_or_default()
    }
    /// `maps/<id>/map.json` of the active assets (and mods), if that map was imported.
    pub fn load(id: &str) -> Option<MapDoc> {
        super::map_json(id).map(|v| MapDoc::from_value(&v))
    }
    /// `name`, or empty.
    pub fn name_or_empty(&self) -> &str {
        self.name.as_deref().unwrap_or_default()
    }
    /// `description`, or empty.
    pub fn description_or_empty(&self) -> &str {
        self.description.as_deref().unwrap_or_default()
    }
    /// `world_half`, default 1000.
    pub fn half(&self) -> f64 {
        self.world_half.unwrap_or(1000.0)
    }
    /// Whether the map asks for a star map picture: `true` or a non-empty name.
    pub fn wants_backdrop(&self) -> bool {
        match &self.backdrop {
            Some(Backdrop::Flag(on)) => *on,
            Some(Backdrop::Name(name)) => !name.is_empty(),
            None => false,
        }
    }
    /// How many lobby slots a player can take (`playable`).
    pub fn playable_players(&self) -> usize {
        self.players.iter().filter(|p| p.playable.unwrap_or(false)).count()
    }
}

/// `backdrop`: the star map picture is on for `true` and for a non-empty name.
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Backdrop {
    /// `true`: use `maps/<id>/backdrop.png`.
    Flag(bool),
    /// A texture name (retail); the picture is still `maps/<id>/backdrop.png` for a mission.
    Name(String),
}

/// `lighting`: colours are `[r, g, b, a]`, 0..1 (three numbers are enough).
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct LightingDoc {
    /// Ambient colour (not used: the engine takes `floor` as the ambient light, PROVISIONAL).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub ambient: Option<Numbers>,
    /// Direction the roof light travels, game axes.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub roof_direction: Option<Numbers>,
    /// Floor light colour (the engine's ambient light).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub floor: Option<Numbers>,
    /// Roof light colour (the engine's sun).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub roof: Option<Numbers>,
}

/// One lobby slot of `players`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct PlayerDoc {
    /// The player's name; scripts and the owner of pre-placed ships use it.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub name: Option<String>,
    /// Index into `teams`; -1 (default) for none.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub team: Option<i64>,
    /// Start point, game axes.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub start: Option<Numbers>,
    /// Start facing, game axes.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub forward: Option<Numbers>,
    /// Formation index of the player's fleet (default 0).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub formation: Option<u64>,
    /// `true`: a slot a player can take (default `false`).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub playable: Option<bool>,
    /// Fleet type: 0 Navy, 2 Pirate, 3 Procyon, 4 open (default 4; other numbers are unverified, PROVISIONAL).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub race: Option<u64>,
    /// Banner colour `[r, g, b, a]`, 0..1 (three numbers are enough).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub color: Option<Numbers>,
}

/// One entry of `teams`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct TeamDoc {
    /// The team's string id (what `Team X Wins` and a script's team rules name).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub name_id: Option<String>,
    /// Fleet type of the team, as a player's `race` (default 4).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub race: Option<u64>,
    /// The exporter's "Race Lock" as the game file holds it. Not read by the engine; any JSON value (PROVISIONAL: its type is unverified).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub lock: Option<Value>,
}

/// One pre-placed ship of `ships`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct MapShipDoc {
    /// The world object id (rules and end conditions name it).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub id: Option<i64>,
    /// The ship class (a folder of `ships/`); a class that does not exist drops the ship.
    #[serde(rename = "type", default, deserialize_with = "crate::data::lenient")]
    pub kind: Option<String>,
    /// Position, game axes (missing numbers read 0).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub pos: Option<Numbers>,
    /// Index into `players` of the owner.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub player: Option<u64>,
    /// The ship's name (default: its class's name pool).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub name: Option<String>,
    /// `true`: the player's command ship (default `false`).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub primary: Option<bool>,
}

/// One piece of scenery of `objects`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct ObjectDoc {
    /// The world object type (`Island_Large01`, `Base_Grant`, `Asteroid_...`, `Terrain_BlackHole...`): keys the dock, collision and crew data.
    #[serde(rename = "type", default, deserialize_with = "crate::data::lenient")]
    pub kind: Option<String>,
    /// The scenery model: `models/<scene>.glb`.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub scene: Option<String>,
    /// The world object id: scripts and groups address the object by it; objects without one are not addressable.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub id: Option<i64>,
    /// Position, game axes (x east, y north, z up).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub pos: Option<Numbers>,
    /// Nine numbers: a 3x3 rotation matrix, column by column (the second triple is forward); missing entries read as the identity.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub rot: Option<Numbers>,
    /// Index into `players` of the owner (pre-placed bases of a skirmish map).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub player: Option<i64>,
    /// The script group a campaign asteroid belongs to.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub group: Option<String>,
    /// Engine-added, not written by the exporter: the model-frame point that sits at `pos` (game axes), overriding the first dock's height datum.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub scene_origin: Option<Numbers>,
}

/// One entry of `mines`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct MineDoc {
    /// The bullet that is the mine (`Bullet_StarMine_Med`). An entry without a `type` or an `id` is skipped.
    #[serde(rename = "type", default, deserialize_with = "crate::data::lenient")]
    pub kind: Option<String>,
    /// The mine's id: unique; a group can list it.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub id: Option<i64>,
    /// Position, game axes.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub pos: Option<Numbers>,
}

/// One entry of `nebulae`: a cloud, a solar storm and a meteor shower sharing a polygon (layout of the retail maps, `tools/export_nebulae.py`).
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct NebulaDoc {
    /// The setup's name; a rule that moves a storm addresses it through `groups`.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub name: Option<String>,
    /// The script groups that hold the nebula's world object.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<String>")]
    pub groups: Vec<String>,
    /// The region it covers: `[x, y]` points, game axes.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<Numbers>")]
    pub polygon: Vec<Numbers>,
    /// The cloud: drawn when `effect` is `Nebula`; drains speed when `drain`.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub cloud: Option<CloudDoc>,
    /// The storm: drawn and damaging when `effect` is `SolarStorm`.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub storm: Option<StormDoc>,
    /// The meteor shower.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub meteors: Option<MeteorsDoc>,
    /// Distance the ambient sound carries (default 300).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub sound_distance: Option<f64>,
}

/// A point of a nebula effect: where one volume sits and how big it is.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct EffectPointDoc {
    /// Centre, game axes.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub pos: Option<Numbers>,
    /// Radius (default 0).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub magnitude: Option<f64>,
}

/// `nebulae[].cloud`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct CloudDoc {
    /// The effect (`Nebula`; the exporter writes `EFFECT` for none).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub effect: Option<String>,
    /// One volume per point.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<EffectPointDoc>")]
    pub points: Vec<EffectPointDoc>,
    /// `true`: the cloud slows ships by how deep they are (default `false`).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub drain: Option<bool>,
    /// `true`: the cloud blocks sensors (default `false`).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub occlusion: Option<bool>,
}

/// `nebulae[].storm`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct StormDoc {
    /// The effect (`SolarStorm`; `EFFECT` for none).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub effect: Option<String>,
    /// One volume per point.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<EffectPointDoc>")]
    pub points: Vec<EffectPointDoc>,
    /// `true`: lightning strikes (default `false`).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub lightning: Option<bool>,
    /// Seconds between lightning blasts (the engine uses at least 3).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub lightning_recharge: Option<f64>,
    /// `true`: rotational winds damage ships (default `false`).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub winds: Option<bool>,
    /// Wind strength. Exported; the engine does not read it.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub wind_magnitude: Option<f64>,
    /// Seconds between wind damage ticks (0: no wind damage).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub wind_damage_frequency: Option<f64>,
}

/// `nebulae[].meteors`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct MeteorsDoc {
    /// The effect (`MeteorStorm`; `EFFECT` for none).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub effect: Option<String>,
    /// One shower per point.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<EffectPointDoc>")]
    pub points: Vec<EffectPointDoc>,
    /// `true`: meteors fall (default `false`).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub on: Option<bool>,
    /// Seconds between strikes (the engine uses at least 2).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub recharge: Option<f64>,
}

/// One rule of `victory`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct VictoryRuleDoc {
    /// The rule's name (informational).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub name: Option<String>,
    /// The conditions, all of which must hold. The reader needs this key: a rule without it (or with a value that is not a list) makes
    /// all of the map's rules unusable, which `MapDoc` tells by `None` (an empty list is a rule that always holds).
    #[serde(rename = "if", default, deserialize_with = "lenient_array")]
    #[schemars(with = "Vec<VictoryConditionDoc>")]
    pub conditions: Option<Vec<VictoryConditionDoc>>,
    /// The winning team's `name_id`; none: the survivors when they are one team.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub winner: Option<String>,
    /// Message ids (`IDGS_...`) shown to the winners and the losers; others are ignored.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub end: Option<VictoryEndDoc>,
}

/// `victory[].end`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct VictoryEndDoc {
    /// Message id for the winners (a string starting `IDGS_`).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub winner: Option<String>,
    /// Message id for the losers.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub loser: Option<String>,
}

/// A world object a victory condition names: an id, or (with a script) a group's name.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)] // only the schema reads this: the condition keeps the raw values (anything else in the list is skipped)
enum ObjectRef {
    /// A world object id.
    Id(i64),
    /// A group of the map's `script.json` (PROVISIONAL: skirmish scripts only).
    Group(String),
}

/// One condition of a victory rule: a rule condition record of the retail game (`Type` and its own fields, spelled as the game
/// does, see `docs/rule-types.md`) plus what the exporter adds (`objects`, `polygon`). Only the keys the engine reads are listed;
/// others are allowed.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct VictoryConditionDoc {
    /// The condition type: `Group Destroyed`, `Team has X points`, `No Team Has Ships`, `Enter Volume`, ... An unlisted type makes the
    /// map's rules unusable.
    #[serde(rename = "Type", default, deserialize_with = "crate::data::lenient")]
    pub kind: Option<String>,
    /// The world object ids (or script group names) the condition is about.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    #[schemars(with = "Option<Vec<ObjectRef>>")]
    pub objects: Option<Vec<Value>>,
    /// A group of the map's script, as an alternative to listing `objects` (PROVISIONAL: engine-added).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub group: Option<String>,
    /// `Enter Volume`: the volume's points, game x/y (at least 3).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub polygon: Option<Vec<Numbers>>,
    /// `>`, `<` or `=` of a damage test.
    #[serde(rename = "Equivalence", default, deserialize_with = "crate::data::lenient")]
    pub equivalence: Option<String>,
    /// The damage a damage test compares with.
    #[serde(rename = "Damage Percent", default, deserialize_with = "crate::data::lenient")]
    pub damage_percent: Option<f64>,
    /// The team of a team condition (a `teams` `name_id`).
    #[serde(rename = "Team Name", default, deserialize_with = "crate::data::lenient")]
    pub team_name: Option<String>,
    /// The points of `Team has X points`.
    #[serde(rename = "Points", default, deserialize_with = "crate::data::lenient")]
    pub points: Option<i64>,
    /// `Does Group Contain Unit Name`: the game's `"TRUE"` / `"FALSE"` string (any case; anything else is false).
    #[serde(rename = "Exists?", default, deserialize_with = "crate::data::lenient")]
    pub exists: Option<String>,
}

/// The JSON Schema of `map.json`, generated from [`MapDoc`].
pub fn schema() -> Value {
    let mut v = serde_json::to_value(schemars::SchemaGenerator::default().into_root_schema_for::<MapDoc>()).expect("schema is JSON");
    // `extends` and `set` are consumed by the overlay merge before the loader reads the map (docs/modding.md "Extending a file").
    if let Some(props) = v["properties"].as_object_mut() {
        props.insert("extends".into(), serde_json::json!({"type": "string", "description": "Not part of the map: the id of the map this file extends. The mod merge lays this file over `maps/<id>/map.json` before the game reads it."}));
        props.insert(
            "set".into(),
            serde_json::json!({"type": "object", "description": "Not part of the map: with `extends`, JSON pointers (`/objects/0/pos`) mapped to the value that replaces what the merge left there; reaches inside arrays."}),
        );
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A synthetic map with every key the engine reads (invented values; no game content).
    fn sample() -> Value {
        json!({
            "$schema": "map.schema.json", "id": "arena", "name": "Arena", "description": "A test arena.", "skirmish": true, "assemble": false,
            "sky": "skybox_99_Layer1", "music": "Track", "backdrop": "Pic", "world_half": 1500.5,
            "lighting": {"ambient": [0, 0, 0, 1], "roof_direction": [0.0, 1.0, -1.0], "floor": [0.4, 0.45, 0.8, 1.0], "roof": [1, 1, 1, 1]},
            "alliance_change": true, "islands_make_sounds": true, "name_id": "IDGS_X", "desc_id": "IDGS_Y",
            "players": [
                {"name": "A", "team": 0, "start": [0, 0, 0], "forward": [0, 1, 0], "formation": 2, "playable": true, "race": 0, "color": [1, 0, 0, 1]},
                {"name": "B", "team": 1, "start": [10, 0, 0], "forward": [0, -1, 0], "formation": 0, "playable": false, "race": 2, "color": null}
            ],
            "teams": [{"name_id": "T0", "race": 0, "lock": "TRUE"}, {"name_id": "T1", "race": 2, "lock": false}],
            "ships": [{"id": 3, "type": "Ship_Test", "pos": [1, 2, 3], "player": 1, "name": "Gull", "primary": true}],
            "objects": [
                {"type": "Island_Test", "scene": "Island", "id": 7, "pos": [5, 6, 7], "rot": [1, 0, 0, 0, 1, 0, 0, 0, 1], "player": -1, "group": "Rocks", "scene_origin": [0, 0, 2]}
            ],
            "nebulae": [{
                "name": "Storm", "groups": ["g"], "polygon": [[0, 0], [10, 0], [10, 10]],
                "cloud": {"effect": "Nebula", "points": [{"pos": [1, 2, 3], "magnitude": 40.0}], "drain": true, "occlusion": false},
                "storm": {"effect": "SolarStorm", "points": [], "lightning": true, "lightning_recharge": 5.0, "winds": true, "wind_magnitude": 2.0, "wind_damage_frequency": 1.5},
                "meteors": {"effect": "MeteorStorm", "points": [], "on": true, "recharge": 3.0}, "sound_distance": 250.0
            }],
            "mines": [{"type": "Bullet_StarMine_Med", "id": 9, "pos": [0, 5, 0]}],
            "victory": [{"name": "r", "if": [{"Type": "Group Destroyed", "objects": [1, "Convoy"], "group": "G"}, {"Type": "Team has X points", "Team Name": "T0", "Points": 100},
                {"Type": "Enter Volume", "polygon": [[0, 0], [1, 0], [1, 1]]}, {"Type": "Group/Unit Vital Section has >,<,= X damage", "Equivalence": ">", "Damage Percent": 0.5},
                {"Type": "Does Group Contain Unit Name", "Exists?": "TRUE"}], "winner": "T0", "end": {"winner": "IDGS_W", "loser": "IDGS_L"}}]
        })
    }

    #[test]
    fn every_key_is_read_by_name_with_its_type() {
        let d = MapDoc::from_value(&sample());
        assert_eq!((d.name_or_empty(), d.description_or_empty(), d.id.as_deref()), ("Arena", "A test arena.", Some("arena")));
        assert_eq!((d.skirmish, d.assemble, d.alliance_change, d.islands_make_sounds), (Some(true), Some(false), Some(true), Some(true)));
        assert_eq!((d.sky.as_deref(), d.music.as_deref(), d.half()), (Some("skybox_99_Layer1"), Some("Track"), 1500.5));
        assert!(d.wants_backdrop() && d.schema.is_some() && d.name_id.is_some() && d.desc_id.is_some());
        let l = d.lighting.as_ref().unwrap();
        assert_eq!(
            (l.roof.as_ref().unwrap().at(2), l.roof_direction.as_ref().unwrap().at(2), l.floor.as_ref().unwrap().numbers().count(), l.ambient.as_ref().unwrap().at(3)),
            (Some(1.0), Some(-1.0), 4, Some(1.0))
        );
        assert_eq!(d.players.len(), 2);
        let p = &d.players[0];
        assert_eq!((p.name.as_deref(), p.team, p.formation, p.playable, p.race), (Some("A"), Some(0), Some(2), Some(true), Some(0)));
        assert_eq!((p.start.as_ref().unwrap().at(0), p.forward.as_ref().unwrap().at(1), p.color.as_ref().unwrap().at(0)), (Some(0.0), Some(1.0), Some(1.0)));
        assert!(d.players[1].color.is_none() && d.players[1].playable == Some(false));
        assert_eq!(d.playable_players(), 1);
        assert_eq!((d.teams[1].name_id.as_deref(), d.teams[1].race, d.teams[0].lock.clone()), (Some("T1"), Some(2), Some(json!("TRUE"))));
        let s = &d.ships[0];
        assert_eq!(
            (s.id, s.kind.as_deref(), s.player, s.name.as_deref(), s.primary, s.pos.as_ref().unwrap().at(2)),
            (Some(3), Some("Ship_Test"), Some(1), Some("Gull"), Some(true), Some(3.0))
        );
        let o = &d.objects[0];
        assert_eq!((o.kind.as_deref(), o.scene.as_deref(), o.id, o.player, o.group.as_deref()), (Some("Island_Test"), Some("Island"), Some(7), Some(-1), Some("Rocks")));
        assert_eq!((o.pos.as_ref().unwrap().at(1), o.rot.as_ref().unwrap().numbers().count(), o.scene_origin.as_ref().unwrap().at(2)), (Some(6.0), 9, Some(2.0)));
        let n = &d.nebulae[0];
        assert_eq!((n.name.as_deref(), n.groups.as_slice(), n.polygon.len(), n.sound_distance), (Some("Storm"), ["g".to_string()].as_slice(), 3, Some(250.0)));
        let (c, st, me) = (n.cloud.as_ref().unwrap(), n.storm.as_ref().unwrap(), n.meteors.as_ref().unwrap());
        assert_eq!(
            (c.effect.as_deref(), c.drain, c.occlusion, c.points[0].magnitude, c.points[0].pos.as_ref().unwrap().at(1)),
            (Some("Nebula"), Some(true), Some(false), Some(40.0), Some(2.0))
        );
        assert_eq!((st.lightning, st.lightning_recharge, st.winds, st.wind_magnitude, st.wind_damage_frequency), (Some(true), Some(5.0), Some(true), Some(2.0), Some(1.5)));
        assert_eq!((me.on, me.recharge, me.effect.as_deref()), (Some(true), Some(3.0), Some("MeteorStorm")));
        assert_eq!((d.mines[0].kind.as_deref(), d.mines[0].id, d.mines[0].pos.as_ref().unwrap().at(1)), (Some("Bullet_StarMine_Med"), Some(9), Some(5.0)));
        let v = &d.victory[0];
        assert_eq!((v.name.as_deref(), v.winner.as_deref(), v.end.as_ref().unwrap().loser.as_deref()), (Some("r"), Some("T0"), Some("IDGS_L")));
        let c = v.conditions.as_ref().unwrap();
        assert_eq!((c.len(), c[0].kind.as_deref(), c[0].objects.as_ref().unwrap().len(), c[0].group.as_deref()), (5, Some("Group Destroyed"), 2, Some("G")));
        assert_eq!((c[1].team_name.as_deref(), c[1].points, c[2].polygon.as_ref().unwrap().len()), (Some("T0"), Some(100), 3));
        assert_eq!((c[3].equivalence.as_deref(), c[3].damage_percent, c[4].exists.as_deref()), (Some(">"), Some(0.5), Some("TRUE")));
    }

    #[test]
    fn a_bare_or_odd_file_reads_as_the_value_readers_did() {
        let d = MapDoc::from_value(&json!({}));
        assert_eq!((d.half(), d.name_or_empty(), d.wants_backdrop(), d.playable_players()), (1000.0, "", false, 0));
        assert!(d.players.is_empty() && d.objects.is_empty() && d.lighting.is_none() && d.skirmish.is_none());
        // not an object: nothing is there
        for v in [json!(null), json!([1, 2]), json!("map"), json!(3)] {
            assert_eq!(MapDoc::from_value(&v).half(), 1000.0, "{v}");
        }
        // wrong types count as absent, key by key, and a list's other elements keep their positions
        let d = MapDoc::from_value(&json!({
            "name": 5, "world_half": "wide", "skirmish": "true", "backdrop": 3, "sky": ["x"], "players": {"a": 1}, "teams": "none",
            "objects": [7, {"type": "Rock", "pos": [1, "y", 3]}, null, {"type": 4}], "mines": [{"id": "x"}],
            "lighting": {"roof": "white", "floor": [1, 2]}, "nebulae": [{"cloud": 1, "polygon": [[0, 0], "x"]}]
        }));
        assert_eq!((d.name.is_none(), d.world_half, d.skirmish, d.wants_backdrop(), d.sky.is_none(), d.players.len(), d.teams.len()), (true, None, None, false, true, 0, 0));
        assert_eq!(d.objects.len(), 4);
        assert!(d.objects[0].kind.is_none() && d.objects[2].pos.is_none() && d.objects[3].kind.is_none());
        let pos = d.objects[1].pos.as_ref().unwrap();
        assert_eq!((d.objects[1].kind.as_deref(), pos.at(0), pos.at(1), pos.at(2), pos.numbers().count()), (Some("Rock"), Some(1.0), None, Some(3.0), 2));
        assert!(d.mines[0].id.is_none());
        let l = d.lighting.unwrap();
        assert_eq!((l.roof.is_none(), l.floor.unwrap().numbers().count()), (true, 2));
        assert_eq!((d.nebulae[0].cloud.is_none(), d.nebulae[0].polygon.len(), d.nebulae[0].polygon[1].0.len()), (true, 2, 0));
        // backdrop: `true` and a name turn it on, an empty name or `false` do not
        for (v, want) in [(json!(true), true), (json!(false), false), (json!("Pic"), true), (json!(""), false)] {
            assert_eq!(MapDoc::from_value(&json!({"backdrop": v})).wants_backdrop(), want, "{v}");
        }
        // an end rule without `if` is told apart from one with an empty list
        let d = MapDoc::from_value(&json!({"victory": [{"name": "a"}, {"if": []}, {"if": "x"}, 5]}));
        assert_eq!(d.victory.iter().map(|r| r.conditions.as_ref().map(Vec::len)).collect::<Vec<_>>(), [None, Some(0), None, None]);
    }

    /// The `type` keyword of the part of a JSON Schema `node` that `value` has to match, as the engine reads it: an integer is `as_i64`
    /// or `as_u64`, a number `as_f64`. Follows `$ref`, `anyOf`/`oneOf`, `properties`, `items` and `additionalProperties`. The problems
    /// as `(path, message)`. (No JSON Schema validator is a dependency; this checks the types, which is what a lenient reader hides.)
    pub(crate) fn shape_problems(root: &Value, node: &Value, value: &Value, at: &str, out: &mut Vec<String>) {
        if let Some(r) = node["$ref"].as_str() {
            if let Some(target) = root.pointer(r.trim_start_matches('#')) {
                shape_problems(root, target, value, at, out);
            }
            return;
        }
        for key in ["anyOf", "oneOf"] {
            if let Some(variants) = node[key].as_array() {
                let mut best: Option<Vec<String>> = None;
                for v in variants {
                    let mut o = vec![];
                    shape_problems(root, v, value, at, &mut o);
                    if best.as_ref().is_none_or(|b| o.len() < b.len()) {
                        best = Some(o);
                    }
                }
                out.extend(best.unwrap_or_default());
                return;
            }
        }
        let fits = |t: &str| match t {
            "null" => value.is_null(),
            "boolean" => value.is_boolean(),
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "array" => value.is_array(),
            "object" => value.is_object(),
            _ => true,
        };
        let types: Vec<&str> = match &node["type"] {
            Value::String(t) => vec![t.as_str()],
            Value::Array(ts) => ts.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        if !types.is_empty() && !types.iter().any(|t| fits(t)) {
            out.push(format!("{at}: {} is not {}", serde_json::to_string(value).unwrap_or_default().chars().take(60).collect::<String>(), types.join(" or ")));
            return;
        }
        match value {
            Value::Object(obj) => {
                for (k, v) in obj {
                    let child = if at.is_empty() { k.clone() } else { format!("{at}.{k}") };
                    match node["properties"].get(k) {
                        Some(p) => shape_problems(root, p, v, &child, out),
                        None if node["additionalProperties"].is_object() => shape_problems(root, &node["additionalProperties"], v, &child, out),
                        None => {}
                    }
                }
            }
            Value::Array(items) if node.get("items").is_some() => {
                for (i, v) in items.iter().enumerate() {
                    shape_problems(root, &node["items"], v, &format!("{at}[{i}]"), out);
                }
            }
            _ => {}
        }
    }

    fn problems(map: &Value) -> Vec<String> {
        let mut out = vec![];
        let schema = schema();
        shape_problems(&schema, &schema, map, "", &mut out);
        out
    }

    #[test]
    fn the_schema_describes_a_map_and_lets_unknown_keys_through() {
        let s = schema();
        assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
        assert_eq!(s["title"], "map.json");
        assert!(s.get("additionalProperties") != Some(&Value::Bool(false)), "unknown keys stay allowed");
        for key in [
            "$schema",
            "id",
            "name",
            "description",
            "skirmish",
            "assemble",
            "sky",
            "music",
            "backdrop",
            "lighting",
            "world_half",
            "alliance_change",
            "islands_make_sounds",
            "name_id",
            "desc_id",
            "players",
            "teams",
            "ships",
            "objects",
            "nebulae",
            "mines",
            "victory",
            "extends",
            "set",
        ] {
            assert!(s["properties"][key].is_object(), "{key}");
        }
        assert!(s["properties"]["world_half"]["description"].as_str().unwrap().contains("1000"));
        assert_eq!(s["properties"]["objects"]["type"], "array");
        assert!(s["$defs"]["ObjectDoc"]["properties"]["scene_origin"].is_object());
        assert!(s["$defs"]["ObjectDoc"]["properties"]["type"]["description"].as_str().unwrap().contains("world object type"));
        // what a modder may leave out: the published schema has no `required` (the loader reads every field as optional anyway)
        assert!(!s.to_string().contains("\"required\""));
        // unknown keys at every level: none are reported by the shape check, and the key check names them
        let extra = json!({"zzz": 1, "objects": [{"type": "Rock", "yyy": 2}]});
        assert!(problems(&extra).is_empty());
        let unknown: Vec<String> = crate::authoring::unknown_keys(&s, &extra).into_iter().map(|u| u.0).collect();
        assert_eq!(unknown, ["zzz", "objects[0].yyy"]);
        // the published list has it
        assert!(crate::authoring::schemas().iter().any(|(n, v)| *n == "map.schema.json" && v["title"] == "map.json"));
    }

    #[test]
    fn a_complete_map_and_a_minimal_one_match_the_schema_and_a_misshapen_one_does_not() {
        assert_eq!(problems(&sample()), Vec::<String>::new());
        assert!(problems(&json!({})).is_empty());
        assert!(problems(&json!({"backdrop": true, "world_half": 5, "extends": "other", "set": {"/a": 1}})).is_empty());
        let bad = problems(&json!({"world_half": "wide", "objects": [{"pos": [1, "y", 3], "id": 1.5}], "victory": [{"if": [{"Type": "x", "objects": [true]}]}], "backdrop": 3}));
        let text = bad.join("\n");
        for want in ["world_half", "objects[0].pos[1]", "objects[0].id", "victory[0].if[0].objects[0]", "backdrop"] {
            assert!(text.contains(want), "{want} in\n{text}");
        }
        assert_eq!(bad.len(), 5, "{text}");
    }

    /// Every `maps/*/map.json` of the imported assets reads through the typed document and matches the generated schema. Run with
    /// `cargo test --manifest-path game/Cargo.toml -- --ignored every_imported_map_json`. Without a JSON Schema validator in the
    /// dependencies the match is the `type` check of `shape_problems`; unknown keys are printed (the schema allows them, but a retail key
    /// the schema does not describe is one an editor would not show).
    #[test]
    #[ignore = "needs imported assets"]
    fn every_imported_map_json_reads_through_the_typed_document_and_matches_the_schema() {
        let schema = schema();
        let (mut maps, mut failures, mut unknown) = (0, vec![], std::collections::BTreeSet::new());
        for entry in std::fs::read_dir(crate::assets_dir().join("maps")).unwrap().flatten() {
            let path = entry.path().join("map.json");
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let value: Value = serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let id = entry.file_name().to_string_lossy().into_owned();
            maps += 1;
            let mut found = vec![];
            shape_problems(&schema, &schema, &value, "", &mut found);
            failures.extend(found.into_iter().map(|p| format!("{id}: {p}")));
            unknown.extend(crate::authoring::unknown_keys(&schema, &value).into_iter().map(|(path, _)| {
                // one line per key kind, not per element: `objects[12].foo` -> `objects[].foo`
                let mut out = String::new();
                let mut skip = false;
                for c in path.chars() {
                    match c {
                        '[' => (out.push_str("[]"), skip = true).1,
                        ']' => skip = false,
                        c if !skip => out.push(c),
                        _ => {}
                    }
                }
                format!("{id}: {out}")
            }));
            // the typed document holds what the Value does: every list has its elements, and the positions are numbers
            let doc = MapDoc::from_value(&value);
            for (key, n) in [
                ("objects", doc.objects.len()),
                ("players", doc.players.len()),
                ("teams", doc.teams.len()),
                ("ships", doc.ships.len()),
                ("nebulae", doc.nebulae.len()),
                ("mines", doc.mines.len()),
                ("victory", doc.victory.len()),
            ] {
                assert_eq!(value[key].as_array().map_or(0, Vec::len), n, "{id}: {key}");
            }
            for (v, o) in value["objects"].as_array().into_iter().flatten().zip(&doc.objects) {
                assert_eq!(v["type"].as_str(), o.kind.as_deref(), "{id}");
                assert_eq!(v["pos"][2].as_f64(), o.pos.as_ref().and_then(|p| p.at(2)), "{id}: {}", v["type"]);
                assert_eq!(v["id"].as_i64(), o.id, "{id}");
            }
            assert_eq!(value["world_half"].as_f64(), doc.world_half, "{id}");
            assert_eq!(value["name"].as_str().unwrap_or_default(), doc.name_or_empty(), "{id}");
        }
        assert!(maps > 0, "no maps under {}: import the game assets first", crate::assets_dir().display());
        for line in &unknown {
            eprintln!("key the schema does not describe: {line}");
        }
        assert!(failures.is_empty(), "{} map.json value(s) do not match the schema:\n{}", failures.len(), failures.join("\n"));
    }
}
