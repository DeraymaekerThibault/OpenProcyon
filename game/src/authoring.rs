//! The surface for tools built on this crate, such as the mission and mod editor. Everything here is a thin, documented wrapper over
//! the code the game itself runs (no second implementation), and none of it starts a window, reads a command line or touches the
//! game's own folders: a tool should call these instead of reaching into `lib.rs`, `modding` or `rules` internals, which may change.
//!
//! - [`base_assets`]: where the imported assets are, with no side effects.
//! - [`ENGINE_VERSION`]: the version of this engine, for "made with" stamps and compatibility checks.
//! - [`overlay_into`]: the merged asset tree of a base plus mods, built into a folder the tool chooses. The game's shared
//!   `local/overlay` is never used, so several tools and game instances can run side by side (#136).
//! - [`read_merged`] / [`read_map`]: a JSON file of such a root (or of the base folder), read like the game reads it, as a
//!   [`DataError`] that names the file and the mod that provided it.
//! - [`rule_schema`]: the mission rule vocabulary as JSON, the same document `--dump-rule-schema` writes.
//! - [`RuleReport`] / [`rule_report_schema`]: the file `--rule-report <file>` writes (what each rule of a mission did in one play), to parse.
//! - [`sound_duration`]: how long a sound file (Ogg Vorbis) plays, read with the decoder the game plays it with.
//! - [`schemas`]: JSON Schemas of `ship.json`, `guns.json`, `bullets.json`, `data/crew_nodes.json`, `mod.json`, `maps/<id>/map.json`, `mission.json`, `script.json` and `media.json` of a map (`--dump-schemas <dir>`).
//! - [`extensions`]: the engine's additions to the original (the table of docs/modding.md), as data.
//! - [`check_mission`] / [`known_names`]: the mission checker behind `--check-mission`, on JSON values in memory.
//! - [`defined_names`] / [`accepted_names`]: the names a mission defines per kind of rule field, and exactly the names a field of
//!   that kind accepts (derived from the checker's own test, so a `group` field also offers players and placed ships' groups).
//! - [`visit_fields`] / [`visit_fields_mut`] / [`split_group`]: every rule field that names something (group, volume, point, point set,
//!   speech event, rule ...) with its kind and JSON pointer, read or for rewriting; the walk the checker itself runs.
//! - [`Finding`]: what the checkers find, located by `path` (printed) and `pointer` (a JSON pointer); [`unknown_key_findings`] gives
//!   the unknown-key warnings of any document with the nearest known key.
//! - [`merge_rule`] / [`extends_parent`] / [`MERGED`]: how a mod file combines with the overlay below it, and what an `extends` file
//!   is laid over.
//! - [`keyed_lists`] / [`extend`] / [`merge_keyed`]: which lists of a map or mission file merge by key (and by which field), and the
//!   whole `extends` merge (keyed lists, `merge`, `$remove`, `set`) a mod file gets over its parent.
//! - [`scenery_transform`] / [`game_rotation`]: a map object's `pos` / `rot` as the world transform the map spawner places it at.
//! - [`Formation`] / [`formation_slots`]: the fleet formations and their slot offsets in the leader's frame.
//! - [`game_to_bevy`] / [`bevy_to_game`] / [`mount_rotation`] / [`mount_rot`]: the files' axes and `rot` triples as the loader reads them.
//! - [`resolved_ship`]: a ship as the loader sees it ([`ResolvedShip`]): speed, thrust, mass, hit points, mounts and every hardpoint
//!   in game axes, each value with its [`Source`] (a JSON pointer into `ship.json`, a model node, or a fallback rule), serializable.
//!   It is built from the merged document the loader hands back, so which of `ship.json` and the model wins is decided once.
//! - [`ship_art`]: the command-bar icon and weapon-panel outline (PNG bytes) and each gun's bank the engine generates for a ship from a
//!   model, with nothing written.
//! - [`templates`] / [`expand_template`] / [`template_schema`]: the mission templates of `content/templates/` (dialogue, cutscene, failsafe,
//!   reach a point, ambush on arrival, escort, sink a group) and their expansion into plain rules and tasks.
//! - [`crew_kind`] / [`CREW_KINDS`] / [`CREW_NODE`]: crew node names, and the kind of a model's crew nodes in `data::derive::Facts`.
//!
//! Shared code: no UI and no view imports.

use std::path::{Path, PathBuf};

use serde_json::Value;

mod annotate;
mod art;
mod check_mod;
mod extensions;
mod json;
#[cfg(test)]
mod keyed_tests;
mod rule_report;
mod sound;
mod templates;
/// Whether a model's mesh layer is a damage variant (`<A>Damage_<B>`, swapped in when the section `<A>_<B>` is destroyed) rather than
/// part of the intact hull: true when the name contains `Damage_`. The one rule the game, `--check-mod` and the ship art use, so a tool
/// listing a model's sections must skip these layers too.
pub use crate::data::derive::is_damage_layer;
pub use crate::data::resolved::{Hardpoint, HardpointKind, ResolvedMount, ResolvedSection, ResolvedShip, Source, Sourced, resolved_ship};
pub use art::{GunBank, ShipArt, ship_art};
pub use check_mod::{check_mod, check_mod_with, unknown_key_findings, unknown_keys};
pub use extensions::{Extension, ExtensionKind, extensions};
pub use json::to_json_string;
pub use rule_report::{RuleReport, RuleReportEntry, RuleReportOutcome, rule_report_schema};
pub use sound::sound_duration;
pub use templates::{Expansion, PARAMETER_KINDS, Parameter, Template, expand_template, template_schema, templates};

pub use crate::data::DataError;
pub use crate::modding::{MERGED, MergeRule, ModEntry, ModReport, extends_parent, merge_rule};
pub use crate::rules::check::{FieldVisit, Finding, JsonPath, Level, Problem, Segment, accepted_names, defined_names, split_group};

/// Calls `f` for every string field of a mission's rules that names something, as the checker reads them: each condition and action of
/// `rules`, the nested conditions of `OP Any Of` / `OP Not`, and the `complete_when` / `fail_when` lists of `tasks`. A [`FieldVisit`]
/// gives the JSON `pointer` of the field in `mission` (a `/rules/3/actions/0/Group`), its `kind()` as the rule schema names it
/// (`group`, `player`, `path`, `volume`, `task`, `point`, `timer`, `flag`, `counter`, `campaign_counter`, `rule`, `speech_event`,
/// `point_set`, `map_text`, `message`, `team`, `ship_name`, and the plain `bool`, `number`, `enum`, `text`), the field's name, the
/// `item_type()` of the condition or action, its `role()` and the `text`. Entries of a Type the schema does not know are skipped, as the
/// checker skips them. A `group` field's text is `Group` or `Group,Unit` ([`split_group`]); a `rule` field is matched by its trimmed
/// text. It is the very walk behind `--check-mission`, [`defined_names`] and the unused-definition warnings, so a rename that follows it
/// changes what the checker reads. Only rules and tasks are walked, not the definitions (`volumes`, `groups`, ...): renaming those is
/// the caller's edit.
pub fn visit_fields(mission: &Value, f: &mut dyn FnMut(&FieldVisit)) {
    crate::rules::check::visit_located(mission, f);
}

/// [`visit_fields`] for a rename: `f` gets each field and a copy of its text, and a change it makes to the copy is written back into
/// `mission` at the field's pointer after the walk (so `f` never sees its own edits). Same fields, same order.
pub fn visit_fields_mut(mission: &mut Value, f: &mut dyn FnMut(&FieldVisit, &mut String)) {
    crate::rules::check::visit_located_mut(mission, f);
}

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

/// Reads the JSON file `rel` (`data/weapons/guns.json`, `ships/<id>/ship.json`, with `/` separators) of `root`: a folder
/// [`overlay_into`] built, or the base assets themselves. The game's shared overlay is never touched (unlike `data::map_json`, which
/// goes through `assets_dir()`). Errors are the loader's: the file, the mod that provided it (its own file, not the overlay copy) and
/// serde's line and column. A `rel` that leaves `root` (absolute or with `..`) is refused.
pub fn read_merged(root: &Path, rel: &str) -> Result<Value, DataError> {
    let path = root.join(rel);
    if Path::new(rel).components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        let error = std::io::Error::new(std::io::ErrorKind::InvalidInput, "the path must stay inside the asset folder");
        return Err(DataError::Read { path, source_mod: None, error });
    }
    crate::data::load_as(&path, None)
}

/// `maps/<id>/map.json` of `root`, as [`read_merged`] reads it.
pub fn read_map(root: &Path, id: &str) -> Result<Value, DataError> {
    read_merged(root, &format!("maps/{id}/map.json"))
}

/// The mission rule schema (every condition and action `Type`, its fields and meaning) as the JSON `--dump-rule-schema` writes.
/// Needs no assets.
pub fn rule_schema() -> Value {
    crate::rules::schema::to_json()
}

/// The JSON Schemas (draft 2020-12) of the files a modder writes, as `(file name, schema)`: `ship.schema.json`, `guns.schema.json`,
/// `bullets.schema.json`, `crew_nodes.schema.json`, `mod.schema.json`, `map.schema.json`, `mission.schema.json`, `script.schema.json`, `media.schema.json`; the files `--dump-schemas <dir>` writes. Generated from the
/// types the loader reads, so they cannot drift from it. Unknown keys stay allowed, as in the loader. No key is `required`: a mod's
/// files are merged over a parent (`extends`, or a retail entry) before the loader reads them, so a partial file is valid; the loader's own
/// validation of the merged result is unchanged. The entry of `guns.json` and `bullets.json` carries `"x-required-new"`, the dotted
/// fields a NEW entry must set (what the loader needs, derived from the same types; see [`required_new`]); a change to a retail entry
/// needs none. Needs no assets.
///
/// Every property of the ship, guns and bullets schemas carries `"x-origin"`: `"retail"` or `"engine"` (a key the engine adds; the
/// `ENGINE_SHIP_KEYS` / `ENGINE_GUN_KEYS` keys). A ship key the engine fills in when the file leaves it out carries `"x-default-from"`:
/// `model`, `race_and_size`, `navy_cutter`, `hull` or `weapon_panel` (`data::SHIP_FILLS`); so do the nested keys it fills (`guns` `pos`
/// `rot` `bank` `parent_layer`, `engines` `pos` `rot` `fx` `parent_layer`, `physics` `MinExtents` ...), on their definitions.
pub fn schemas() -> Vec<(&'static str, Value)> {
    let mut all = crate::data::schemas();
    annotate::all(&mut all);
    all.push(("mod.schema.json", crate::modding::schema()));
    all.push(("map.schema.json", crate::data::map_schema()));
    all.push(("mission.schema.json", crate::data::mission_doc::mission_schema()));
    all.push(("script.schema.json", crate::data::mission_doc::script_schema()));
    all.push(("media.schema.json", crate::data::mission_doc::media_schema()));
    for (name, schema) in &mut all {
        // `guns.json` and `bullets.json` entries are merged key by key, so a partial entry is the normal case and no key is `required`;
        // what a NEW entry must set is listed apart, as the non-enforcing annotation `x-required-new` on the entry.
        if matches!(*name, "guns.schema.json" | "bullets.schema.json") {
            let mut list = vec![];
            required_paths(schema, &schema["$defs"]["entry"], "", &mut list);
            schema["$defs"]["entry"]["x-required-new"] = list.into();
        }
        strip_required(schema);
    }
    // `extends` and `set` are consumed by the overlay merge before the loader reads a ship (docs/modding.md "Extending a file").
    if let Some(props) = all[0].1["properties"].as_object_mut() {
        props.insert("extends".into(), serde_json::json!({"type": "string", "x-origin": "engine", "description": "Not part of the ship: the id of the ship this file extends. The mod merge lays this file over `ships/<id>/ship.json` before the game reads it."}));
        props.insert(
            "set".into(),
            serde_json::json!({"type": "object", "x-origin": "engine", "description": "Not part of the ship: with `extends`, JSON pointers (`/engines/0/fx`) mapped to the value that replaces what the merge left there; reaches inside arrays."}),
        );
        props.insert(
            "merge".into(),
            serde_json::json!({"type": "object", "x-origin": "engine", "description": "Not part of the ship (PROVISIONAL spelling): with `extends`, a list merged by key instead of replaced, as {\"/crew_nodes\": \"name\"}: the file's list at that pointer changes, adds or (`\"$remove\": true`) removes single entries of the parent's list, matched by that field. Applied before `set`."}),
        );
    }
    all
}

/// The fields (dotted, going into the objects they hold, such as `visual.pivot`) that the loader needs of a whole entry of
/// `data/weapons/guns.json` or `bullets.json` (a file name, or its path); what a new entry must set, since an entry that changes a
/// retail one needs none; and of a whole `ships/<id>/ship.json` (PROVISIONAL list, `data::SHIP_REQUIRED_NEW`) that extends no existing
/// ship. Empty for any other file. The list is the `x-required-new` of the published schema.
pub fn required_new(file: &str) -> Vec<String> {
    let (name, entry) = match file.rsplit('/').next() {
        Some("guns.json") => ("guns.schema.json", true),
        Some("bullets.json") => ("bullets.schema.json", true),
        Some("ship.json") => ("ship.schema.json", false),
        _ => return vec![],
    };
    schemas()
        .iter()
        .find(|(n, _)| *n == name)
        .and_then(|(_, s)| {
            let list = if entry { &s["$defs"]["entry"]["x-required-new"] } else { &s["x-required-new"] };
            list.as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        })
        .unwrap_or_default()
}

/// The `required` paths of an entry schema (before they are stripped), dotted, into the objects named by `$ref`.
fn required_paths(root: &Value, node: &Value, prefix: &str, out: &mut Vec<String>) {
    for k in node["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        out.push(format!("{prefix}{k}"));
    }
    for (k, sub) in node["properties"].as_object().into_iter().flatten() {
        if let Some(def) = sub["$ref"].as_str().and_then(|r| r.strip_prefix("#/$defs/")) {
            required_paths(root, &root["$defs"][def], &format!("{prefix}{k}."), out);
        }
    }
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

/// A point in game axes (x starboard, y forward, z up), as the files hold it, to Bevy's axes (x, z, -y): what the loader does with
/// every `pos` (`data::conv`).
pub fn game_to_bevy(p: [f64; 3]) -> bevy::math::Vec3 {
    crate::data::conv(p[0] as f32, p[1] as f32, p[2] as f32)
}

/// The inverse of [`game_to_bevy`]: a Bevy-axes vector as the `[x, y, z]` a file holds.
pub fn bevy_to_game(v: bevy::math::Vec3) -> [f64; 3] {
    [v.x as f64, -v.z as f64, v.y as f64]
}

/// A `rot` triple of a gun mount or a crew node (`ship.json`), as the rotation the loader gives the node (Bevy axes).
pub fn mount_rotation(rot: [f64; 3]) -> bevy::math::Quat {
    crate::data::mount_rotation(rot)
}

/// The inverse of [`mount_rotation`]: the `rot` triple that gives `rotation` (each angle in radians; one of the triples when there
/// are several).
pub fn mount_rot(rotation: bevy::math::Quat) -> [f64; 3] {
    let (z, y, x) = rotation.to_euler(bevy::math::EulerRot::ZYX);
    [y as f64, -z as f64, x as f64]
}

/// The lists of the file `rel` (its path inside the assets, `/` separators: `maps/<id>/map.json`) that an `extends` file merges by
/// key rather than replacing whole, as `(JSON pointer, key field)` pairs: `("/objects", "id")`. Empty for every other file. The very
/// table [`extend`] uses (PROVISIONAL, docs/modding.md "Changing one entry of a list"); a `merge` key in the diff can add lists to it
/// or switch one off, which this table does not show.
pub fn keyed_lists(rel: &str) -> Vec<(&'static str, &'static str)> {
    crate::modding::keyed_defaults(rel).to_vec()
}

/// What an `extends` file `diff` makes of its `parent` (the parsed file it extends), for the file `rel`: the merge the loader runs on
/// a mod file. Objects merge deep, everything else replaces, except the [`keyed_lists`] of `rel` and those the diff names in `merge`,
/// which merge by key (`$remove` drops an entry); then `set` pointers apply, and `extends`, `merge` and `set` are dropped. Returns the
/// merged document and the warnings (a `$remove` of an entry the parent lacks). The error is the loader's message; the loader then
/// skips the file.
pub fn extend(rel: &str, parent: Value, diff: &Value) -> Result<(Value, Vec<String>), String> {
    crate::modding::extend(parent, diff, rel)
}

/// The keyed merge on its own: `entries` merged into the list at the JSON pointer `ptr` of `base`, matched by the field `field`. A
/// matching entry is deep-merged, a new one appended, `{"<field>": k, "$remove": true}` removes one. Returns the warnings; errors as
/// for [`extend`] (`base` may be left half changed then).
pub fn merge_keyed(base: &mut Value, ptr: &str, field: &str, entries: &[Value]) -> Result<Vec<String>, String> {
    let mut warnings = vec![];
    crate::modding::merge_keyed(base, ptr, field, entries, &mut warnings)?;
    Ok(warnings)
}

/// A `rot` of a map object (the nine native matrix values) as the rotation the spawner uses (Bevy axes).
pub use crate::scenario::game_rotation;
/// A map object (`maps/<id>/map.json` `objects` entry: `pos`, `rot`, `type`, optional `scene_origin`) placed in the world: the
/// transform the map spawner gives it (Bevy axes), from `rotation` (see [`game_rotation`]) and `docks` (the parsed
/// `data/docks.json`, whose first dock point sets the default origin). Anchoring on the first dock is PROVISIONAL
/// (docs/re/scenery-placement.md).
pub use crate::scenario::scenery_transform;
/// The fleet formations and the slot offsets of each, in the leader's frame (Bevy axes); `radii` are the ships' bounding-sphere radii.
pub use crate::ship::{Formation, formation_slots};

pub use crate::data::crew_nodes::{CREW_KINDS, crew_kind};
pub use crate::data::derive::CREW_NODE;

/// Every name a group spec of this mission can mean (groups, players, placed ships, environment); see `rules::check::known_names`.
pub use crate::rules::check::known_names;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keyed_lists_are_the_loaders_table() {
        assert_eq!(keyed_lists("maps/x/map.json")[0], ("/objects", "id"));
        assert!(keyed_lists("maps/x/mission.json").contains(&("/points", "Name")));
        assert!(keyed_lists("maps/x/script.json").contains(&("/rules", "name")));
        assert!(keyed_lists("ships/A/ship.json").is_empty());
        assert!(keyed_lists("maps/x/other.json").is_empty());
    }

    #[test]
    fn extend_merges_keyed_lists_removes_and_sets() {
        let parent = json!({"name": "P", "objects": [{"id": 1, "pos": [0, 0, 0]}, {"id": 2, "pos": [1, 1, 1]}], "tags": ["a", "b"]});
        let diff = json!({"extends": "p", "objects": [{"id": 2, "pos": [9, 9, 9]}, {"id": 1, "$remove": true}, {"id": 3, "pos": [3, 3, 3]}, {"id": 7, "$remove": true}], "set": {"/tags/1": "z"}});
        let (out, warnings) = extend("maps/m/map.json", parent.clone(), &diff).unwrap();
        assert_eq!(out["objects"], json!([{"id": 2, "pos": [9, 9, 9]}, {"id": 3, "pos": [3, 3, 3]}]));
        assert_eq!(out["tags"], json!(["a", "z"]));
        assert!(out.get("extends").is_none() && out.get("set").is_none());
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        // another file replaces the list whole, unless `merge` names it
        let (out, _) = extend("ships/A/ship.json", parent.clone(), &diff).unwrap();
        assert_eq!(out["objects"].as_array().unwrap().len(), 4);
        let by_id = json!({"merge": {"/objects": "id"}, "objects": [{"id": 2, "pos": [5, 5, 5]}]});
        let (out, _) = extend("ships/A/ship.json", parent.clone(), &by_id).unwrap();
        assert_eq!(out["objects"], json!([{"id": 1, "pos": [0, 0, 0]}, {"id": 2, "pos": [5, 5, 5]}]));
        // errors are the loader's
        let bad = json!({"set": {"/nope": 1}});
        assert!(extend("maps/m/map.json", parent, &bad).unwrap_err().contains("does not exist"));
    }

    #[test]
    fn visit_fields_gives_kind_and_pointer_of_every_named_field_and_renames() {
        let mut mission = json!({
            "rules": [{"name": "r", "conditions": [{"Type": "OP Any Of", "Conditions": [{"Type": "Group Destroyed", "Group Name": "Pirates,Flagship"}]}],
                       "actions": [{"Type": "Set Flag Action", "Flag Name": "seen"}]}],
            "tasks": [{"Name": "T", "complete_when": [{"Type": "Group Destroyed", "Group Name": "Pirates"}], "fail_when": []}]
        });
        let mut seen = vec![];
        visit_fields(&mission, &mut |v| seen.push((v.pointer.clone(), v.kind(), v.item_type(), v.role(), v.text.to_string())));
        assert!(
            seen.iter().any(|(p, k, ty, role, t)| p == "/rules/0/conditions/0/Conditions/0/Group Name"
                && *k == "group"
                && ty == &"Group Destroyed"
                && *role == "condition"
                && t == "Pirates,Flagship"),
            "{seen:?}"
        );
        assert!(seen.iter().any(|(p, k, _, role, _)| p == "/rules/0/actions/0/Flag Name" && *k == "flag" && *role == "action"));
        assert!(seen.iter().any(|(p, k, _, _, _)| p == "/tasks/0/complete_when/0/Group Name" && *k == "group"));
        assert_eq!(split_group("Pirates,Flagship"), ("Pirates", Some("Flagship")));
        assert_eq!(split_group("Pirates"), ("Pirates", None));
        // every pointer finds its text back in the document
        visit_fields(&mission, &mut |v| assert_eq!(mission.pointer(&v.pointer).and_then(Value::as_str), Some(v.text)));
        // a rename through the walk
        visit_fields_mut(&mut mission, &mut |v, text| {
            let (group, unit) = split_group(text);
            if v.kind() == "group" && group == "Pirates" {
                *text = unit.map_or_else(|| "Raiders".to_string(), |u| format!("Raiders,{u}"));
            }
        });
        assert_eq!(mission["rules"][0]["conditions"][0]["Conditions"][0]["Group Name"], "Raiders,Flagship");
        assert_eq!(mission["tasks"][0]["complete_when"][0]["Group Name"], "Raiders");
        assert_eq!(mission["rules"][0]["actions"][0]["Flag Name"], "seen");
    }

    #[test]
    fn merge_keyed_alone() {
        let mut base = json!({"rules": [{"name": "a", "x": 1}]});
        let w = merge_keyed(&mut base, "/rules", "name", &[json!({"name": "a", "x": 2}), json!({"name": "b"}), json!({"name": "c", "$remove": true})]).unwrap();
        assert_eq!(base["rules"], json!([{"name": "a", "x": 2}, {"name": "b"}]));
        assert_eq!(w.len(), 1);
        assert!(merge_keyed(&mut base, "/rules", "name", &[json!({"x": 1})]).is_err());
    }

    #[test]
    fn scenery_is_placed_as_the_spawner_places_it() {
        let docks = json!({"Rock": {"points": [{"pos": [0, 0, 4]}]}});
        let object = json!({"type": "Rock", "pos": [10, 20, 30], "rot": [0, 1, 0, -1, 0, 0, 0, 0, 1]});
        let q = game_rotation(&object["rot"]);
        let tf = scenery_transform(&object, q, &docks);
        let origin = bevy::math::Vec3::Y * 4.0;
        assert!((tf.translation - (game_to_bevy([10.0, 20.0, 30.0]) - q * origin)).length() < 1e-4);
        // a quarter turn about game z: the game's +y (forward) leads to Bevy's -z
        assert!((q * bevy::math::Vec3::NEG_Z - bevy::math::Vec3::X).length() < 1e-4 || (q * bevy::math::Vec3::NEG_Z + bevy::math::Vec3::X).length() < 1e-4);
        // a missing rot is the identity
        assert!(game_rotation(&json!(null)).angle_between(bevy::math::Quat::IDENTITY) < 1e-5);
    }

    #[test]
    fn formations_give_one_slot_per_ship() {
        let slots = formation_slots(Formation::Column, &[10.0, 5.0, 5.0]);
        assert_eq!(slots.len(), 3);
        assert_eq!(slots[0], bevy::math::Vec3::ZERO);
        assert!(slots[1].z > 0.0 && slots[2].z > slots[1].z);
        assert_eq!(Formation::BUTTONS.len(), 6);
    }

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
    fn read_merged_reads_the_tools_overlay_and_names_the_mods_own_file() {
        let dir = scratch("read-merged");
        let (base, m, out) = (dir.join("assets"), dir.join("mymod"), dir.join("mine"));
        std::fs::create_dir_all(base.join("maps/red")).unwrap();
        std::fs::create_dir_all(base.join("ships/S")).unwrap();
        std::fs::create_dir_all(m.join("ships/S")).unwrap();
        std::fs::write(base.join("maps/red/map.json"), r#"{"name": "Red"}"#).unwrap();
        std::fs::write(base.join("ships/S/ship.json"), r#"{"core_hp": 100, "race": "navy"}"#).unwrap();
        std::fs::write(m.join("ships/S/ship.json"), r#"{"extends": "S", "core_hp": 5}"#).unwrap();
        overlay_into(&base, std::slice::from_ref(&m), &out).unwrap();
        assert_eq!(read_map(&out, "red").unwrap()["name"], "Red", "base files are visible in the overlay");
        let ship = read_merged(&out, "ships/S/ship.json").unwrap();
        assert_eq!((ship["core_hp"].as_i64(), ship["race"].as_str()), (Some(5), Some("navy")), "the merged file");
        assert_eq!(read_merged(&base, "maps/red/map.json").unwrap()["name"], "Red", "a base folder reads too");
        // a missing file and a file that is not JSON are errors naming the file
        let error = read_map(&out, "nowhere").unwrap_err();
        assert!(matches!(error, DataError::Read { .. }) && error.to_string().contains("maps/nowhere/map.json"), "{error}");
        std::fs::create_dir_all(out.join("data")).unwrap();
        std::fs::write(out.join("data/bad2.json"), "not json").unwrap();
        let error = read_merged(&out, "data/bad2.json").unwrap_err();
        assert!(matches!(error, DataError::Parse { .. }) && error.to_string().contains("column"), "{error}");
        // a merged copy names the mod's own file as its source, and the mod
        std::fs::write(m.join("ships/S/ship.json"), r#"{"extends": "S", "core_hp": 5}"#).unwrap();
        overlay_into(&base, std::slice::from_ref(&m), &out).unwrap();
        let context = crate::data::asset_context(&out.join("ships/S/ship.json"));
        assert!(context.contains(&format!("(source {}", std::fs::canonicalize(m.join("ships/S/ship.json")).unwrap().display())), "{context}");
        assert_eq!(crate::data::mod_of(&out.join("ships/S/ship.json")).as_deref(), Some("mymod"));
        // nothing outside the root, and the game's shared overlay was not built
        assert!(read_merged(&out, "../assets/maps/red/map.json").is_err());
        assert!(read_merged(&out, "/etc/hostname").is_err());
        assert!(!base.join("../local").exists());
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
    fn the_rule_report_has_a_schema_and_parses_what_the_game_writes() {
        let schema = rule_report_schema();
        assert_eq!(schema["$schema"], "https://json-schema.org/draft/2020-12/schema");
        assert!(schema["properties"]["rules"].is_object() && schema["properties"]["play_time"].is_object());
        let text = r#"{"mission": "mission01", "outcome": "victory", "play_time": 61.5, "rules": [
            {"name": "Start", "fired": true, "runs": 1, "first_run": 0.5, "last_run": 0.5, "active": true},
            {"name": "Never", "fired": false, "runs": 0, "active": false}]}"#;
        let report: RuleReport = serde_json::from_str(text).unwrap();
        assert_eq!((report.outcome, report.rules[1].first_run, report.rules[1].active), (RuleReportOutcome::Victory, None, false));
    }

    #[test]
    fn schemas_are_2020_12_for_every_file_a_modder_writes() {
        let all = schemas();
        let names: Vec<_> = all.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            [
                "ship.schema.json",
                "guns.schema.json",
                "bullets.schema.json",
                "crew_nodes.schema.json",
                "mod.schema.json",
                "map.schema.json",
                "mission.schema.json",
                "script.schema.json",
                "media.schema.json"
            ]
        );
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

    /// The definition a property's schema points to, or the property itself.
    fn resolve<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
        fn find(v: &Value) -> Option<&str> {
            match v {
                Value::Object(m) => m.get("$ref").and_then(Value::as_str).or_else(|| m.iter().filter(|(k, _)| !k.starts_with("x-")).find_map(|(_, v)| find(v))),
                Value::Array(a) => a.iter().find_map(find),
                _ => None,
            }
        }
        find(node).and_then(|r| r.strip_prefix("#/$defs/")).map_or(node, |n| &root["$defs"][n])
    }

    #[test]
    fn schema_annotations_equal_the_engine_constants_and_the_fill_table() {
        use crate::data::{ENGINE_GUN_KEYS, ENGINE_SHIP_KEYS, SHIP_FILLS};
        let all = schemas();
        let (ship, guns, bullets) = (&all[0].1, &all[1].1, &all[2].1);
        let origins = |props: &Value| -> (Vec<String>, Vec<String>) {
            let (mut engine, mut retail) = (vec![], vec![]);
            for (k, v) in props.as_object().unwrap() {
                match v["x-origin"].as_str() {
                    Some("engine") => engine.push(k.clone()),
                    Some("retail") => retail.push(k.clone()),
                    other => panic!("{k} has x-origin {other:?}"),
                }
            }
            engine.sort();
            (engine, retail)
        };
        let (engine, retail) = origins(&ship["properties"]);
        let mut want: Vec<String> = ENGINE_SHIP_KEYS.iter().map(|s| s.to_string()).chain(["$schema", "set", "merge"].map(String::from)).collect();
        want.sort();
        assert_eq!(engine, want, "the ship keys marked engine are ENGINE_SHIP_KEYS (plus the file-only `$schema`, `set` and `merge`)");
        assert!(["race", "core_hp", "sections", "guns", "physics"].iter().all(|k| retail.contains(&k.to_string())), "{retail:?}");
        let (engine, retail) = origins(&guns["$defs"]["entry"]["properties"]);
        assert_eq!(engine, ENGINE_GUN_KEYS, "only `beam` is ours");
        assert!(retail.contains(&"MuzzleSpeed".to_string()));
        let (engine, retail) = origins(&bullets["$defs"]["entry"]["properties"]);
        assert!(engine.is_empty() && retail.contains(&"lifetime".to_string()), "every bullet key is retail");
        // x-default-from: exactly the table, at the top level and below it
        let mut seen = 0;
        for (path, fill) in SHIP_FILLS {
            let mut parts = path.split('.');
            let node = &ship["properties"][parts.next().unwrap()];
            let node = match parts.next() {
                None => node,
                Some(sub) => &resolve(ship, node)["properties"][sub],
            };
            assert_eq!(node["x-default-from"], fill.name(), "{path}");
            seen += 1;
        }
        fn count(v: &Value) -> usize {
            match v {
                Value::Object(m) => m.iter().map(|(k, v)| usize::from(k == "x-default-from") + count(v)).sum(),
                Value::Array(a) => a.iter().map(count).sum(),
                _ => 0,
            }
        }
        assert_eq!(count(ship), seen, "no annotation outside the table");
        assert_eq!(count(guns) + count(bullets), 0);
        assert_eq!(ship["properties"]["engines"]["x-default-from"], "model");
        assert_eq!(ship["properties"]["crew_posts"]["x-default-from"], "race_and_size");
        assert_eq!(ship["properties"]["tow"]["x-default-from"], "hull");
        assert_eq!(resolve(ship, &ship["properties"]["guns"])["properties"]["bank"]["x-default-from"], "weapon_panel");
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

    fn remove_path(v: &mut Value, path: &str) {
        let mut parts: Vec<&str> = path.split('.').collect();
        let last = parts.pop().unwrap();
        let mut at = v;
        for p in parts {
            at = &mut at[p];
        }
        at.as_object_mut().unwrap().remove(last);
    }

    #[test]
    fn x_required_new_of_guns_and_bullets_is_what_the_loader_needs() {
        let all = schemas();
        let full_gun = json!({"Mount Type": "Medium Mount", "Victory Point Cost": 3, "MuzzleFlashEffect": "Fx", "SoundName": "s", "SoundVolume": 1, "Is Lobbing Gun": false,
            "MuzzleSpeed": 1, "TimeBetweenBursts": 1, "BulletsPerBurst": 1, "DuringBurstReloadTime": 1, "EffectiveRange_Range": 1, "LongRange_Range": 1,
            "MaximumRange_Range": 1, "EffectiveRange_AccuracyDeviation": 1, "LongRange_AccuracyDeviation": 1, "MaximumRange_AccuracyDeviation": 1, "BulletAffectedByGravity": false,
            "BulletTypeName_Primary": "B", "visual": {"base": "a", "barrel": "b", "pivot": [0, 0, 0], "muzzles": [[0, 0, 0]]}, "HorizontalRotationSpeed": 1, "VerticalRotationSpeed": 1,
            "VerticalMinAngle": 0, "VerticalMaxAngle": 1, "beam": {"charge": 1}});
        let full_bullet = json!({"lifetime": 1, "impact_vol": 1, "impact_max": 1, "impact_snd": {"Wood": ["a"]}, "damage": {"Wood": [1, 2, 0, 0, 0]}, "hit_fx": "H", "decal": "Small", "hitpoints": 2});
        for (name, entry, full) in [("guns.schema.json", &all[1].1, full_gun), ("bullets.schema.json", &all[2].1, full_bullet)] {
            let is_gun = name.starts_with("guns");
            let load = |v: &Value| if is_gun { crate::data::gun_from_value("G", v.clone()).map(|_| ()) } else { crate::data::bullet_from_value("B", v.clone()).map(|_| ()) };
            let required = required_new(if is_gun { "data/weapons/guns.json" } else { "data/weapons/bullets.json" });
            assert_eq!(entry["$defs"]["entry"]["x-required-new"], json!(required));
            // the loader's own (unstripped) types say the same
            let mut own = vec![];
            required_paths(&crate::data::schemas()[if is_gun { 1 } else { 2 }].1, &crate::data::schemas()[if is_gun { 1 } else { 2 }].1["$defs"]["entry"], "", &mut own);
            assert_eq!(own, required);
            assert!(required.len() >= 5, "{name}: {required:?}");
            assert_eq!(load(&full), Ok(()), "{name}: the full entry loads");
            // with only the required fields, it loads; without any one of them, it does not
            let mut least = full.clone();
            let optional: Vec<String> = {
                let mut all_paths = vec![];
                for (k, v) in full.as_object().unwrap() {
                    all_paths.push(k.clone());
                    if let Some(o) = v.as_object().filter(|_| k == "visual") {
                        all_paths.extend(o.keys().map(|n| format!("{k}.{n}")));
                    }
                }
                all_paths.into_iter().filter(|p| !required.contains(p)).collect()
            };
            for p in &optional {
                if !p.contains('.') {
                    remove_path(&mut least, p);
                }
            }
            assert_eq!(load(&least), Ok(()), "{name}: the required fields alone load ({required:?})");
            for p in &optional {
                let mut one = full.clone();
                remove_path(&mut one, p);
                assert_eq!(load(&one), Ok(()), "{name}: `{p}` is optional");
            }
            for p in &required {
                let mut one = full.clone();
                assert!(full.pointer(&format!("/{}", p.replace('.', "/"))).is_some(), "{name}: the fixture lacks required `{p}`");
                remove_path(&mut one, p);
                assert!(load(&one).is_err(), "{name}: `{p}` is listed required but the loader does without it");
            }
        }
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
    fn a_wake_entry_with_the_retail_label_and_rot_has_no_unknown_key() {
        let all = schemas();
        let wake = json!({"wakes": [{"label": "Wake_0", "pos": [0, 1, 0], "rot": [0, 0, 0], "fx": "wake"}]});
        assert!(unknown_keys(&all[0].1, &wake).is_empty(), "{:?}", unknown_keys(&all[0].1, &wake));
        let typo = json!({"wakes": [{"pos": [0, 1, 0], "fx": "wake", "lable": "x"}]});
        assert_eq!(unknown_keys(&all[0].1, &typo)[0].0, "wakes[0].lable");
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

    #[test]
    fn axes_and_rotations_round_trip_as_the_loader_reads_them() {
        for p in [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0], [-4.5, 10.25, -0.5]] {
            assert_eq!(bevy_to_game(game_to_bevy(p)), p);
            assert_eq!(game_to_bevy(p), crate::data::conv(p[0] as f32, p[1] as f32, p[2] as f32));
        }
        // forward in the files is Bevy -z
        assert_eq!(game_to_bevy([0.0, 1.0, 0.0]), bevy::math::Vec3::NEG_Z);
        for rot in [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0], [0.3, -0.2, 0.1], [-1.2, 0.4, 0.7]] {
            let q = mount_rotation(rot);
            let back = mount_rot(q);
            assert!(mount_rotation(back).angle_between(q) < 1e-5, "{rot:?} -> {back:?}");
            for k in 0..3 {
                assert!((back[k] - rot[k]).abs() < 1e-5, "{rot:?} -> {back:?}");
            }
        }
        assert_eq!(crew_kind("Crew_Walk_P_0"), Some("Walk"));
        assert_eq!(crew_kind("Crew_Climb_Main_a"), Some("Climb"));
        assert_eq!(crew_kind("Crew_Dance_1"), None);
        assert_eq!(crew_kind("Gun_Mount"), None);
    }

    #[test]
    fn a_models_crew_nodes_are_points_of_their_own_kind() {
        use crate::data::derive;
        let node = |name: &str| serde_json::json!({"name": name, "translation": [1.0, 2.0, 3.0]});
        // `point_kind` on single nodes: a crew name wins over the name guesses; an explicit kind wins over both
        assert_eq!(derive::point_kind(&node("Crew_Work_Engine"), "Crew_Work_Engine").as_deref(), Some(CREW_NODE));
        assert_eq!(derive::point_kind(&node("Crew_Gun_Mount_1"), "Crew_Gun_Mount_1").as_deref(), Some(CREW_NODE));
        assert_eq!(derive::point_kind(&node("Engine_1"), "Engine_1").as_deref(), Some("EnginePortPlacement"));
        let tagged = serde_json::json!({"name": "Crew_Walk_A_0", "extras": {"kind": "WakePlacement"}});
        assert_eq!(derive::point_kind(&tagged, "Crew_Walk_A_0").as_deref(), Some("WakePlacement"));
        assert_eq!(derive::point_kind(&node("Crew_Dance_1"), "Crew_Dance_1"), None, "not a crew kind");
    }
}
