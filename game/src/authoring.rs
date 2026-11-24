//! The surface for tools built on this crate, such as the mission and mod editor. Everything here is a thin, documented wrapper over
//! the code the game itself runs (no second implementation), and none of it starts a window, reads a command line or touches the
//! game's own folders: a tool should call these instead of reaching into `lib.rs`, `modding` or `rules` internals, which may change.
//!
//! - [`base_assets`]: where the imported assets are, with no side effects.
//! - [`ENGINE_VERSION`]: the version of this engine, for "made with" stamps and compatibility checks.
//! - [`overlay_into`]: the merged asset tree of a base plus mods, built into a folder the tool chooses. The game's shared
//!   `local/overlay` is never used, so several tools and game instances can run side by side (#136).
//! - [`rule_schema`]: the mission rule vocabulary as JSON, the same document `--dump-rule-schema` writes.
//! - [`schemas`]: JSON Schemas of `ship.json`, `guns.json`, `bullets.json`, `data/crew_nodes.json` and `mod.json` (`--dump-schemas <dir>`).
//! - [`check_mission`] / [`known_names`]: the mission checker behind `--check-mission`, on JSON values in memory.
//!
//! Shared code: no UI and no view imports.

use std::path::{Path, PathBuf};

use serde_json::Value;

mod check_mod;
mod json;
pub use check_mod::{check_mod, check_mod_with, unknown_keys};
pub use json::to_json_string;

pub use crate::modding::{ModEntry, ModReport};
pub use crate::rules::check::{Finding, Level, Problem, defined_names};

/// The version of this engine (the crate version).
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where the converted (base) assets are: `PROCYON_ASSETS`, else the checkout's `assets/`, else the per-user folder the importer
/// fills; the same lookup the game uses. `None` when that folder does not exist (nothing imported). Builds no overlay and writes
/// nothing, unlike `assets_dir()`.
pub fn base_assets() -> Option<PathBuf> {
    crate::find_base_assets().filter(|dir| dir.is_dir())
}

/// Builds the asset tree of `base` with `mods` laid over it (in order, later ones win; the same merge the game applies to `--mod`
/// folders) into `out`, and says what each mod did. `out` is replaced whole, so it must be missing, empty or an overlay this function
/// built earlier; any other folder is an error and is left alone. Nothing outside `out` is written. With no mods nothing is built:
/// use `base` itself as the root.
pub fn overlay_into(base: &Path, mods: &[PathBuf], out: &Path) -> Result<ModReport, String> {
    crate::modding::overlay_into(base, mods, out)
}

/// The mission rule schema (every condition and action `Type`, its fields and meaning) as the JSON `--dump-rule-schema` writes.
/// Needs no assets.
pub fn rule_schema() -> Value {
    crate::rules::schema::to_json()
}

/// The JSON Schemas (draft 2020-12) of the files a modder writes, as `(file name, schema)`: `ship.schema.json`, `guns.schema.json`,
/// `bullets.schema.json`, `crew_nodes.schema.json`, `mod.schema.json`; the files `--dump-schemas <dir>` writes. Generated from the
/// types the loader reads, so they cannot drift from it. Unknown keys stay allowed, as in the loader. No key is `required`: a mod's
/// files are merged over a parent (`extends`, or a retail entry) before the loader reads them, so a partial file is valid; the loader's
/// own validation of the merged result is unchanged. Needs no assets.
pub fn schemas() -> Vec<(&'static str, Value)> {
    let mut all = crate::data::schemas();
    all.push(("mod.schema.json", crate::modding::schema()));
    for (_, schema) in &mut all {
        strip_required(schema);
    }
    // `extends` and `set` are consumed by the overlay merge before the loader reads a ship (docs/modding.md "Extending a file").
    if let Some(props) = all[0].1["properties"].as_object_mut() {
        props.insert("extends".into(), serde_json::json!({"type": "string", "description": "Not part of the ship: the id of the ship this file extends. The mod merge lays this file over `ships/<id>/ship.json` before the game reads it."}));
        props.insert(
            "set".into(),
            serde_json::json!({"type": "object", "description": "Not part of the ship: with `extends`, JSON pointers (`/engines/0/fx`) mapped to the value that replaces what the merge left there; reaches inside arrays."}),
        );
    }
    all
}

/// Removes the `required` keyword from a JSON Schema, wherever it is a keyword (not a property that is itself named `required`).
fn strip_required(node: &mut Value) {
    match node {
        Value::Object(map) => {
            map.remove("required");
            for (key, child) in map.iter_mut() {
                match (key.as_str(), child) {
                    // A table of schemas by name: the names are not keywords, their values are schemas.
                    ("properties" | "$defs" | "definitions" | "patternProperties", Value::Object(table)) => table.values_mut().for_each(strip_required),
                    ("enum" | "const" | "default" | "examples", _) => {}
                    (_, child) => strip_required(child),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(strip_required),
        _ => {}
    }
}

/// Checks a mission's rules like `--check-mission`: `mission` is `mission.json` (or a skirmish map's `script.json`, then `skirmish`
/// is true), `map` and `media` are `map.json` and `media.json` (`Value::Null` when there is none). References to ships, sounds and
/// pictures are not looked up here (that needs the folders; see `rules::cli::check_map`).
pub fn check_mission(mission: &Value, map: &Value, media: &Value, skirmish: bool) -> Vec<Problem> {
    crate::rules::check::check_with(mission, map, media, skirmish)
}

/// Every name a group spec of this mission can mean (groups, players, placed ships, environment); see `rules::check::known_names`.
pub use crate::rules::check::known_names;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("procyon-authoring-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn overlay_into_writes_only_into_the_chosen_folder() {
        let dir = scratch("overlay");
        let (base, m, out) = (dir.join("assets"), dir.join("mod"), dir.join("mine"));
        for d in [base.join("data"), m.join("data")] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(base.join("data/a.json"), r#"{"x": 1, "y": 1}"#).unwrap();
        std::fs::write(base.join("data/b.json"), "{}").unwrap();
        std::fs::write(m.join("data/a.json"), r#"{"x": 2}"#).unwrap();
        let report = overlay_into(&base, &[m], &out).unwrap();
        assert_eq!(report.mods.len(), 1);
        let merged: Value = serde_json::from_str(&std::fs::read_to_string(out.join("data/a.json")).unwrap()).unwrap();
        assert_eq!(merged["x"].as_i64(), Some(2), "the mod's file wins");
        assert!(out.join("data/b.json").is_file(), "base files not touched by the mod are visible in the overlay");
        // the game's default overlay location (<base>/../local/overlay) is untouched, and so is the base
        assert!(!base.join("../local").exists());
        assert_eq!(std::fs::read_to_string(base.join("data/a.json")).unwrap(), r#"{"x": 1, "y": 1}"#);
        let mut entries: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        entries.sort();
        assert_eq!(entries, ["assets", "mine", "mod"]);
        // building again replaces it; a folder with somebody's files is refused and kept
        overlay_into(&base, &[dir.join("mod")], &out).unwrap();
        let theirs = dir.join("theirs");
        std::fs::create_dir_all(&theirs).unwrap();
        std::fs::write(theirs.join("notes.txt"), "mine").unwrap();
        assert!(overlay_into(&base, &[dir.join("mod")], &theirs).is_err());
        assert!(theirs.join("notes.txt").is_file());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn base_assets_creates_nothing() {
        let found = base_assets();
        // asking again changes nothing, and a folder that was not there is still not there
        assert_eq!(base_assets(), found);
        if let Some(dir) = crate::find_base_assets() {
            assert_eq!(dir.is_dir(), found.is_some());
        }
    }

    #[test]
    fn the_schema_in_process_is_what_the_flag_writes() {
        let dir = scratch("schema");
        let path = dir.join("schema.json");
        let args = ["game".to_string(), "--dump-rule-schema".into(), path.to_string_lossy().into_owned()];
        assert_eq!(crate::rules::cli::dump_schema(&args), Some(0));
        let written: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written, rule_schema());
        assert!(!ENGINE_VERSION.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Paths of the keys of `value` that `schema` does not list.
    fn unknown(schema: &Value, value: &Value) -> Vec<String> {
        unknown_keys(schema, value).into_iter().map(|(path, _)| path).collect()
    }

    #[test]
    fn schemas_are_2020_12_for_every_file_a_modder_writes() {
        let all = schemas();
        let names: Vec<_> = all.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["ship.schema.json", "guns.schema.json", "bullets.schema.json", "crew_nodes.schema.json", "mod.schema.json"]);
        for (name, schema) in &all {
            let text = serde_json::to_string(schema).unwrap();
            let back: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(back["$schema"], "https://json-schema.org/draft/2020-12/schema", "{name}");
            assert!(schema.get("additionalProperties") != Some(&Value::Bool(false)), "{name}: unknown keys stay allowed");
        }
        let ship = &all[0].1;
        assert!(ship["properties"]["tow"]["description"].as_str().unwrap().contains("tow line"));
        assert!(ship["properties"]["core_hp"]["description"].as_str().unwrap().contains("Core hit points"));
        assert!(ship["properties"]["$schema"].is_object(), "an editor may write the $schema key");
        assert!(all[4].1["properties"]["requires"]["description"].as_str().unwrap().contains("mods"));
    }

    fn has_required(node: &Value) -> bool {
        match node {
            Value::Object(map) => map.iter().any(|(k, v)| k == "required" && v.is_array()) || map.values().any(has_required),
            Value::Array(items) => items.iter().any(has_required),
            _ => false,
        }
    }

    #[test]
    fn no_published_schema_requires_a_key_so_partial_mod_files_validate() {
        for (name, schema) in schemas() {
            assert!(!has_required(&schema), "{name} still has a `required` list");
        }
        // The loader's own types do list them (the strip is the published view only), and the keys are still described.
        assert!(has_required(&crate::data::schemas()[0].1), "the loader's schema still knows what a complete file has");
        let all = schemas();
        let ship = &all[0].1;
        assert!(ship["properties"]["sections"]["additionalProperties"].is_object() || ship["properties"]["sections"].is_object());
        // A minimal extending file: nothing is demanded, both keys are described.
        let mini = json!({"extends": "Ship_X", "points": 100});
        assert!(unknown(ship, &mini).is_empty(), "{:?}", unknown(ship, &mini));
        assert_eq!(ship["properties"]["points"]["type"], "integer");
    }

    #[test]
    fn a_representative_file_of_each_kind_uses_only_keys_the_schema_knows() {
        let all = schemas();
        let ship = json!({
            "$schema": "ship.schema.json", "race": "Test", "skirmish": true, "core_hp": 60, "points": 10, "fuel": false,
            "physics": {"Mass": 100, "MaxThrust": 10, "MaxSpeed": 5, "RotationalFriction": 1, "MaxAngularAcceleration": 1, "MinExtents": [0, 0, 0]},
            "sections": {"Hull": {"layer": "Hull", "material": "Wood", "hp": 10, "core_pct": 0.5, "vital_ship": true, "vital_speed": false, "vital_turn": false,
                                  "destruct": {"fx": "X", "start": 1, "end": 2, "point": [0, 0, 0]}, "debris": {"model": "m", "pivot": [0, 0, 0]}}},
            "guns": {"Gun_A": {"pos": [0, 1, 2], "rot": [0, 0, 0], "arc_deg": 90, "gun": "Gun", "bank": "Bank1"}},
            "engines": [{"label": "E", "pos": [0, 0, 0], "rot": [0, 0, 0], "fx": "F"}], "wakes": [{"pos": [0, 0, 0], "fx": "W"}], "flags": [{"label": "Flag_Top", "pos": [0, 0, 0]}],
            "crew_posts": {"Gunners": 2}, "sphere": {"center": [0, 0, 0], "radius": 3},
            "crew_nodes": [{"name": "Crew_Walk_P_0", "pos": [0, 0, 0], "rot": [0, 0, 0]}],
            "tow": {"tower": [0, 1, 2], "towee": [0, -1, 2]}, "homing": [0, 0, 1]
        });
        let gun = json!({"$schema": "x", "Gun": {"MuzzleSpeed": 1, "TimeBetweenBursts": 1, "BulletsPerBurst": 1, "DuringBurstReloadTime": 1, "EffectiveRange_Range": 1, "LongRange_Range": 1,
            "MaximumRange_Range": 1, "EffectiveRange_AccuracyDeviation": 1, "LongRange_AccuracyDeviation": 1, "MaximumRange_AccuracyDeviation": 1, "BulletAffectedByGravity": false,
            "BulletTypeName_Primary": "B", "visual": {"base": "a", "pivot": [0, 0, 0], "muzzles": [[0, 0, 0]]}, "HorizontalRotationSpeed": 1, "VerticalRotationSpeed": 1,
            "VerticalMinAngle": 0, "VerticalMaxAngle": 1, "Mount Type": "Medium Mount", "beam": {"charge": 1}}});
        let bullet = json!({"B": {"lifetime": 1, "impact_vol": 1, "impact_max": 1, "impact_snd": {"Wood": ["a"]}, "damage": {"Wood": [1, 2, 0, 0, 0]}, "hit_fx": "H", "physics": {"Mass": 1}}});
        let crew = json!({"$schema": "x", "Town": [{"name": "Crew_Walk_A_0", "pos": [0, 0, 0]}]});
        let modjson = json!({"$schema": "mod.schema.json", "id": "m", "name": "M", "version": "1", "description": "d", "category": "Ships", "missions": ["a"], "default": true,
            "format": 1, "game_version": "0.2.0", "requires": ["other"]});
        for ((name, schema), doc) in all.iter().zip([ship, gun, bullet, crew, modjson]) {
            let unknown = unknown(schema, &doc);
            assert!(unknown.is_empty(), "{name}: keys the schema does not know: {unknown:?}");
        }
        // The checker itself notices a typo.
        assert_eq!(unknown(&all[0].1, &json!({"sigth": 1, "tow": {"tower": [0, 0, 0], "tow": 1}})), ["sigth", "tow.tow"]);
        // ... and offers what it knows there, so a tool can suggest the nearest key
        let typo = unknown_keys(&all[0].1, &json!({"sigth": 1}));
        assert!(typo[0].1.iter().any(|k| k == "sight"), "{typo:?}");
    }

    #[test]
    fn the_schema_key_is_ignored_by_the_loaders_of_the_whole_file_types() {
        let dir = scratch("schemakey");
        std::fs::create_dir_all(dir.join("data/weapons")).unwrap();
        std::fs::write(dir.join("data/weapons/guns.json"), r#"{"$schema": "x"}"#).unwrap();
        std::fs::write(dir.join("data/weapons/bullets.json"), r#"{"$schema": "x"}"#).unwrap();
        std::fs::write(dir.join("data/crew_nodes.json"), r#"{"$schema": "x", "Town": [{"name": "Crew_Walk_A_0", "pos": [0, 0, 0]}]}"#).unwrap();
        let (guns, bullets) = crate::data::load_weapons(&dir);
        assert!(guns.is_empty() && bullets.is_empty());
        assert_eq!(crate::data::crew_nodes::load_scenery(&dir).len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn check_mission_finds_an_unknown_volume() {
        let mission = json!({
            "players": ["Hero"],
            "volumes": {"Gate Area": [[0, 0], [10, 0], [10, 10]]},
            "rules": [{"name": "R", "Run Once": true, "conditions": [{"Type": "Enter Volume", "Group Name": "Hero", "Volume Name": "Gate Aera"}],
                       "actions": [{"Type": "NIS Start"}]}]
        });
        let problems = check_mission(&mission, &Value::Null, &Value::Null, false);
        assert!(problems.iter().any(|p| p.level == Level::Error && p.field == "Volume Name" && p.message.contains("unknown volume 'Gate Aera'")), "{problems:#?}");
        assert!(known_names(&mission).contains("Hero"));
    }
}
