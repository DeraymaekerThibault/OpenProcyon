//! Typed views of `maps/<id>/mission.json` (also the layout of a skirmish map's `script.json`) and `maps/<id>/media.json`, and their
//! JSON Schemas (`mission.schema.json`, `script.schema.json`, `media.schema.json`).
//!
//! PROVISIONAL scope: this is a typed description of the keys the engine and the retail exporter (`tools/export_campaign.py`,
//! `tools/export_campaign_media.py`) use, built the way [`super::MapDoc`] is, so an editor can list and explain keys without spelling
//! them itself. It is not what the game loads from: the interpreter, the briefing, the environment and the star map still read the file
//! as a `serde_json::Value` (`rules::Script::new`, `campaign::try_load_id`), so a key the game starts to read must be added here too.
//! The schema, like the map's, lists keys with their types, every key is optional (a file that extends another is partial), unknown
//! keys are allowed, and a value of the wrong type counts as absent in the typed view. The entries of `conditions`, `actions`,
//! `complete_when` and `fail_when` are the Types of the rule schema (`authoring::rule_schema`), not described here.
//!
//! `format` is an optional integer in each document (PROVISIONAL): the layout version the file was written for, `1` when absent. The
//! game ignores it; it is there so a later change of layout can be told apart (the v0.3 freeze).
use super::maps::{Numbers, lenient_list};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// The layout version these schemas describe (the `format` key; PROVISIONAL).
pub const MISSION_FORMAT: u32 = 1;

/// `maps/<id>/mission.json`, and `script.json` of a skirmish map (the same layout; its `players`, `ships` and `groups` come from the map).
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[schemars(title = "mission.json")]
pub struct MissionDoc {
    /// Editor hint: the schema this file follows. Ignored by the loader.
    #[serde(rename = "$schema", default, deserialize_with = "crate::data::lenient")]
    pub schema: Option<String>,
    /// PROVISIONAL, ignored by the game: the layout version of this file (`1` when absent).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub format: Option<u32>,
    /// Retail exporter: the game file the mission was converted from. Not read.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub source: Option<String>,
    /// The player names. Every pair of listed players is hostile unless `alliances` says otherwise.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<String>")]
    pub players: Vec<String>,
    /// `[[a, b], ...]`: allied pairs of player names.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<Vec<String>>")]
    pub alliances: Vec<Vec<String>>,
    /// The human's player (`name`) and the stand-in start.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub player_start: Option<PlayerStartDoc>,
    /// The ships placed at the start.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<MissionShipDoc>")]
    pub ships: Vec<MissionShipDoc>,
    /// Named groups a rule can address: name to the world object ids (ships, scenery, animals).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub groups: Option<BTreeMap<String, Vec<i64>>>,
    /// Closed polygons: name to `[x, y]` points (at least three).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub volumes: Option<BTreeMap<String, Vec<Numbers>>>,
    /// Waypoint paths: name to `[x, y, z]` points.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub paths: Option<BTreeMap<String, Vec<Numbers>>>,
    /// Objective markers.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<PointDoc>")]
    pub points: Vec<PointDoc>,
    /// Animals, nebula setups, point sets and the whale and dragon definitions.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub environment: Option<EnvironmentDoc>,
    /// The objectives.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<TaskDoc>")]
    pub tasks: Vec<TaskDoc>,
    /// The rules, evaluated in order.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<RuleDoc>")]
    pub rules: Vec<RuleDoc>,
    /// Labels on the star map.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<MapTextDoc>")]
    pub map_text: Vec<MapTextDoc>,
    /// Engine-added: `"objectives"` (in `script.json`: `"script"` or `"objectives"`) lets the tasks (or the script) end the game.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub victory: Option<String>,
    /// Engine-added: a `media.json` `messages` id shown on a win.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub victory_message: Option<String>,
    /// Engine-added: a `media.json` `messages` id shown on a loss.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub defeat_message: Option<String>,
    /// Retail: the scripted opening's four speech cues (mission 1 only); each `{name, text, duration}`.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<Value>")]
    pub opening_speech: Vec<Value>,
    /// Retail exporter, not read by a custom mission: map text name to position.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub landmarks: Option<BTreeMap<String, Numbers>>,
    /// Retail: the crew list the Crew screen offers.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<Value>")]
    pub crew_pool: Vec<Value>,
    /// Retail: the arms list the Arms screen offers.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<Value>")]
    pub arms_pool: Vec<Value>,
    /// Retail: play the end movie after the mission.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub play_end_movie: Option<bool>,
    /// Retail: the journal screen's music track.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub journal_music: Option<String>,
    /// Retail: the journal screen's entry, `{title_id, pages: [{text_id, speech, picture}]}`.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub journal: Option<Value>,
    /// Retail exporter: scenery of the mission that could not be converted. Not read.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<Value>")]
    pub unresolved_scenery: Vec<Value>,
    /// Retail exporter: ship classes of the mission that could not be converted. Not read.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<String>")]
    pub unresolved_ships: Vec<String>,
    /// Retail exporter: the music the scripted opening hands over to.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub handoff_music: Option<String>,
}

impl MissionDoc {
    /// The typed view of a `mission.json` value; never fails (what is missing or of the wrong type is absent).
    pub fn from_value(v: &Value) -> MissionDoc {
        serde_json::from_value(v.clone()).unwrap_or_default()
    }
}

/// `player_start`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct PlayerStartDoc {
    /// The human's player: the ship placed with `primary` and this `group` is the human's.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub name: Option<String>,
    /// Where the stand-in ship appears when the human has no placed ship, game axes.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub start: Option<Numbers>,
    /// Facing of the stand-in ship, game axes.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub forward: Option<Numbers>,
}

/// One entry of `ships`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct MissionShipDoc {
    /// An integer id, unique in the mission (a ship without one stops the game at the start).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub id: Option<i64>,
    /// The ship class (a folder of `ships/`); a class that does not exist drops the ship.
    #[serde(rename = "type", default, deserialize_with = "crate::data::lenient")]
    pub kind: Option<String>,
    /// The owning player (a name of `players`).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub group: Option<String>,
    /// The ship's name: `Group,Unit` finds it, the banner shows it.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub name: Option<String>,
    /// `true`: the human's flagship.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub primary: Option<bool>,
    /// Position, game axes.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub pos: Option<Numbers>,
    /// Nine numbers: a 3x3 rotation matrix, column by column.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub rot: Option<Numbers>,
}

/// One entry of `points`: an objective marker.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct PointDoc {
    /// The point's name (the key of an `extends` merge).
    #[serde(rename = "Name", default, deserialize_with = "crate::data::lenient")]
    pub name: Option<String>,
    /// Position, game axes.
    #[serde(rename = "Position", default, deserialize_with = "crate::data::lenient")]
    pub position: Option<Numbers>,
}

/// One entry of `tasks`: an objective.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct TaskDoc {
    /// The task's name (the key of an `extends` merge; rules name it).
    #[serde(rename = "Name", default, deserialize_with = "crate::data::lenient")]
    pub name: Option<String>,
    /// What the player reads.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub text: Option<String>,
    /// A JSON boolean: the task starts active.
    #[serde(rename = "Active", default, deserialize_with = "crate::data::lenient")]
    pub active: Option<bool>,
    /// Retail string id of the text. Not read.
    #[serde(rename = "TextStringID", default, deserialize_with = "crate::data::lenient")]
    pub text_string_id: Option<String>,
    /// Engine-added: conditions, ANDed; when they hold the task is complete.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<Value>")]
    pub complete_when: Vec<Value>,
    /// Engine-added: conditions, ANDed; when they hold the task is failed.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<Value>")]
    pub fail_when: Vec<Value>,
    /// Engine-added: with `victory`, a failed required task loses and all complete win.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub required: Option<bool>,
    /// Engine-added (skirmish): the team (`name_id`) the task decides for.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub team: Option<String>,
}

/// One entry of `rules`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct RuleDoc {
    /// The rule's name (the key of an `extends` merge; `--fire-rule`, `OP Set Rule Active` and the console find it).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub name: Option<String>,
    /// Retail exporter: the line of the rule in the game file. Not read.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub line: Option<i64>,
    /// A JSON boolean; `false` means the rule is not evaluated (default `true`).
    #[serde(rename = "Is Active", default, deserialize_with = "crate::data::lenient")]
    pub is_active: Option<bool>,
    /// A JSON boolean: the rule fires once (default `true`).
    #[serde(rename = "Run Once", default, deserialize_with = "crate::data::lenient")]
    pub run_once: Option<bool>,
    /// The conditions, ANDed: objects with a `Type` of the rule schema.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<Value>")]
    pub conditions: Vec<Value>,
    /// The actions, run in order: objects with a `Type` of the rule schema.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<Value>")]
    pub actions: Vec<Value>,
}

/// One entry of `map_text`: a star map label.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct MapTextDoc {
    /// The label's name (`Set Map Text Visibility` names it).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub name: Option<String>,
    /// The text shown.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub text: Option<String>,
    /// Position on the map, game axes.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub pos: Option<Numbers>,
    /// `true`: shown from the start.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub visible: Option<bool>,
}

/// `environment`: what a mission or script puts in the world besides its ships. The keys are those the loaders read (`environment.rs`,
/// `dragons.rs`, `nebula.rs`, `rules/eval.rs`) and the exporter writes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct EnvironmentDoc {
    /// Whale placements: `{id, type, pos, rot, group}` (each needs a `group`).
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<AnimalDoc>")]
    pub animals: Vec<AnimalDoc>,
    /// The whale's definition: `scene` (a model name), `min` and `max` (the bounding box, game axes) and the sound keys `sounds`,
    /// `sound_distance`, `sound_interval`.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub whale: Option<Value>,
    /// Dragon placements, as `animals`.
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<AnimalDoc>")]
    pub dragons: Vec<AnimalDoc>,
    /// The dragon's definition: `hp`, `scene`, `physics.MaxSpeed`.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub dragon: Option<Value>,
    /// The `*State Init* Setup Nebula` records of the mission, one per nebula (the retail exporter copies the action).
    #[serde(default, deserialize_with = "lenient_list")]
    #[schemars(with = "Vec<Value>")]
    pub nebulae: Vec<Value>,
    /// Point sets for `Teleport Group/Unit` and the like: name to `[{pos, magnitude}]`.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub point_sets: Option<BTreeMap<String, Vec<PointSetPointDoc>>>,
}

/// One whale or dragon placement.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct AnimalDoc {
    /// The world object id.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub id: Option<i64>,
    /// The retail animal type (`Animal_SpaceWhale...`).
    #[serde(rename = "type", default, deserialize_with = "crate::data::lenient")]
    pub kind: Option<String>,
    /// Position, game axes.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub pos: Option<Numbers>,
    /// Nine numbers: a 3x3 rotation matrix.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub rot: Option<Numbers>,
    /// The script group the animal belongs to.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub group: Option<String>,
}

/// One point of a point set.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct PointSetPointDoc {
    /// Position, game axes.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub pos: Option<Numbers>,
    /// Retail: the point's spread. Not read by `Teleport`.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub magnitude: Option<f64>,
}

/// `maps/<id>/media.json`: the words.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[schemars(title = "media.json")]
pub struct MediaDoc {
    /// Editor hint: the schema this file follows. Ignored by the loader.
    #[serde(rename = "$schema", default, deserialize_with = "crate::data::lenient")]
    pub schema: Option<String>,
    /// PROVISIONAL, ignored by the game: the layout version of this file (`1` when absent).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub format: Option<u32>,
    /// Speech events by name (`Play speech event` names one; names are case-sensitive).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub speech: Option<BTreeMap<String, SpeechDoc>>,
    /// End-of-game texts by id (`End Game`, `victory_message`, `defeat_message`).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub messages: Option<BTreeMap<String, String>>,
    /// Ship display names by id (`Set Ship Name`).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub ship_names: Option<BTreeMap<String, String>>,
    /// Retail: the journal screen's texts, `{title, pages: [{text, speech, duration, picture}]}`.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub journal: Option<Value>,
}

impl MediaDoc {
    /// The typed view of a `media.json` value; never fails.
    pub fn from_value(v: &Value) -> MediaDoc {
        serde_json::from_value(v.clone()).unwrap_or_default()
    }
}

/// One entry of `speech`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct SpeechDoc {
    /// The subtitle.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub text: Option<String>,
    /// Seconds the line is held (the engine adds 0.3). Without it the line is text only: no sound.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub duration: Option<f64>,
    /// The sound `<file>.ogg` (default: the event name).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub file: Option<String>,
    /// The talking-head sprite (`TalkingHead_Character_*`).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub face: Option<String>,
    /// The subtitle tint, RGBA 0..1 (default white).
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub color: Option<Numbers>,
    /// Retail exporter: the head's screen location. Not read.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub head: Option<Value>,
    /// Retail exporter: the speaker id. Not read.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub speaker: Option<Value>,
}

fn schema_of<T: JsonSchema>() -> Value {
    let mut v = serde_json::to_value(schemars::SchemaGenerator::default().into_root_schema_for::<T>()).expect("schema is JSON");
    // `extends`, `merge` and `set` are consumed by the overlay merge before the loader reads the file (docs/modding.md "Extending a file").
    if let Some(props) = v["properties"].as_object_mut() {
        props.insert("extends".into(), serde_json::json!({"type": "string", "description": "Not part of the file: the id of the map whose file this one extends. The mod merge lays this file over it before the game reads it."}));
        props.insert(
            "merge".into(),
            serde_json::json!({"type": "object", "description": "Not part of the file (PROVISIONAL spelling): with `extends`, lists merged by key, as {\"/rules\": \"name\"}."}),
        );
        props.insert("set".into(), serde_json::json!({"type": "object", "description": "Not part of the file: with `extends`, JSON pointers mapped to the value that replaces what the merge left there."}));
    }
    v
}

/// The JSON Schema of `mission.json` (`mission.schema.json`).
pub fn mission_schema() -> Value {
    schema_of::<MissionDoc>()
}

/// The JSON Schema of a skirmish map's `script.json` (`script.schema.json`): the `mission.json` layout.
pub fn script_schema() -> Value {
    let mut v = schema_of::<MissionDoc>();
    v["title"] = "script.json".into();
    v
}

/// The JSON Schema of `media.json` (`media.schema.json`).
pub fn media_schema() -> Value {
    schema_of::<MediaDoc>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A synthetic mission with every key the docs and the exporter name (invented values).
    fn sample() -> Value {
        json!({
            "$schema": "mission.schema.json", "format": 1, "source": "x", "players": ["A", "B"], "alliances": [["A", "B"]],
            "player_start": {"name": "A", "start": [0, 0, 0], "forward": [0, 1, 0]},
            "ships": [{"id": 1, "type": "Ship_T", "group": "A", "name": "Gull", "primary": true, "pos": [0, 0, 0], "rot": [1, 0, 0, 0, 1, 0, 0, 0, 1]}],
            "groups": {"G": [1]}, "volumes": {"V": [[0, 0], [1, 0], [1, 1]]}, "paths": {"P": [[0, 0, 0], [1, 1, 0]]},
            "points": [{"Name": "Gate", "Position": [1, 2, 3]}],
            "environment": {"animals": [{"id": 4, "type": "Animal_X", "pos": [0, 0, 0], "rot": [1, 0, 0, 0, 1, 0, 0, 0, 1], "group": "W"}], "whale": {"scene": "W"},
                            "dragons": [], "dragon": {"hp": 5}, "nebulae": [{"Type": "*State Init* Setup Nebula"}], "point_sets": {"S": [{"pos": [0, 0, 0], "magnitude": 1.5}]}},
            "tasks": [{"Name": "T", "text": "Go", "Active": true, "TextStringID": "I", "complete_when": [], "fail_when": [], "required": true, "team": "T0"}],
            "rules": [{"name": "R", "line": 3, "Is Active": true, "Run Once": true, "conditions": [], "actions": []}],
            "map_text": [{"name": "L", "text": "Label", "pos": [0, 0, 0], "visible": false}],
            "victory": "objectives", "victory_message": "W", "defeat_message": "L", "opening_speech": [], "landmarks": {"L": [0, 0, 0]},
            "crew_pool": [], "arms_pool": [], "play_end_movie": false, "journal_music": "M", "journal": {}, "unresolved_scenery": [], "unresolved_ships": [],
            "handoff_music": "H"
        })
    }

    #[test]
    fn a_mission_with_every_documented_key_has_no_unknown_key() {
        let schema = mission_schema();
        assert_eq!(crate::authoring::unknown_keys(&schema, &sample()), vec![]);
        let typo = json!({"ships": [{"id": 1, "tpye": "x"}], "tasks": [{"Name": "T", "requierd": true}], "rulez": []});
        let mut found: Vec<String> = crate::authoring::unknown_keys(&schema, &typo).into_iter().map(|(p, _)| p).collect();
        found.sort();
        assert_eq!(found, ["rulez", "ships[0].tpye", "tasks[0].requierd"]);
    }

    #[test]
    fn the_typed_view_reads_what_the_value_readers_read() {
        let doc = MissionDoc::from_value(&sample());
        assert_eq!((doc.ships[0].id, doc.ships[0].kind.as_deref(), doc.ships[0].primary), (Some(1), Some("Ship_T"), Some(true)));
        assert_eq!((doc.tasks[0].name.as_deref(), doc.tasks[0].required, doc.tasks[0].team.as_deref()), (Some("T"), Some(true), Some("T0")));
        assert_eq!(doc.points[0].position.as_ref().and_then(|p| p.at(2)), Some(3.0));
        assert_eq!(doc.rules[0].run_once, Some(true));
        assert_eq!(doc.environment.unwrap().point_sets.unwrap()["S"][0].magnitude, Some(1.5));
        // the wrong type is absent, never an error
        let odd = MissionDoc::from_value(&json!({"ships": 3, "tasks": [7, {"Name": 4}], "victory": 1}));
        assert!(odd.ships.is_empty() && odd.tasks.len() == 2 && odd.tasks[1].name.is_none() && odd.victory.is_none());
        assert!(MissionDoc::from_value(&json!([])).rules.is_empty());
    }

    #[test]
    fn media_has_its_keys_and_the_three_schemas_are_titled() {
        let media = json!({"format": 1, "speech": {"E": {"text": "t", "duration": 1.5, "file": "f", "face": "TalkingHead_X", "color": [1, 1, 1, 1], "head": 1, "speaker": 2}},
                           "messages": {"M": "m"}, "ship_names": {"S": "s"}, "journal": {}});
        assert_eq!(crate::authoring::unknown_keys(&media_schema(), &media), vec![]);
        assert_eq!(MediaDoc::from_value(&media).speech.unwrap()["E"].duration, Some(1.5));
        let typo = json!({"speech": {"E": {"txt": "t"}}, "mesages": {}});
        let mut found: Vec<String> = crate::authoring::unknown_keys(&media_schema(), &typo).into_iter().map(|(p, _)| p).collect();
        found.sort();
        assert_eq!(found, ["mesages", "speech.E.txt"]);
        let titles: Vec<String> = [mission_schema(), script_schema(), media_schema()].iter().map(|s| s["title"].as_str().unwrap().to_string()).collect();
        assert_eq!(titles, ["mission.json", "script.json", "media.json"]);
        assert!(mission_schema()["properties"]["extends"].is_object() && mission_schema()["properties"]["rules"].is_object());
    }
}
