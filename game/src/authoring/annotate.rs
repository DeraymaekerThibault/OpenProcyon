//! The `x-origin` and `x-default-from` annotations of the published ship, gun and bullet schemas (`authoring::schemas`).
//!
//! - `x-origin`: `"retail"` (the original has the value; the importer writes it) or `"engine"` (an extension for mods; the keys of
//!   `ENGINE_SHIP_KEYS` / `ENGINE_GUN_KEYS`, plus `$schema` and `set`, which are no data of the game).
//! - `x-required-new` (set in `authoring::schemas`, on the entry of `guns.json` / `bullets.json`): the fields a NEW entry must set.
//! - `x-default-from`: where the engine takes the value when the file leaves the key out, from the `SHIP_FILLS` table next to the
//!   ship loader (`data/ships.rs`): `model`, `race_and_size`, `navy_cutter`, `hull`, `weapon_panel`.
use serde_json::Value;

use crate::data::{ENGINE_GUN_KEYS, ENGINE_SHIP_KEYS, SHIP_FILLS};

/// Ship keys the schema lists that are not game data: the editor hint and the overlay merge's `set`.
const FILE_ONLY: [&str; 2] = ["$schema", "set"];

fn origin(engine: bool) -> Value {
    Value::from(if engine { "engine" } else { "retail" })
}

/// The `#/$defs/<name>` a property's schema points to, wherever the reference sits in it (`anyOf`, `items`, `additionalProperties`).
pub(super) fn def_name(node: &Value) -> Option<String> {
    match node {
        Value::Object(map) => map
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.strip_prefix("#/$defs/"))
            .map(String::from)
            .or_else(|| map.iter().filter(|(k, _)| !matches!(k.as_str(), "properties" | "x-origin" | "x-default-from")).find_map(|(_, v)| def_name(v))),
        Value::Array(items) => items.iter().find_map(def_name),
        _ => None,
    }
}

/// Marks every property of `props` (a schema's `properties`) with its origin.
fn mark(props: &mut Value, engine: &dyn Fn(&str) -> bool) {
    for (key, node) in props.as_object_mut().into_iter().flatten() {
        node["x-origin"] = origin(engine(key));
    }
}

/// `ship.schema.json`: origins of every key, and `x-default-from` on every key (and nested key) of `SHIP_FILLS`.
pub fn ship(schema: &mut Value) {
    mark(&mut schema["properties"], &|k| ENGINE_SHIP_KEYS.contains(&k) || FILE_ONLY.contains(&k));
    for (path, fill) in SHIP_FILLS {
        let mut parts = path.split('.');
        let (Some(top), sub) = (parts.next(), parts.next()) else { continue };
        let name = match sub {
            None => {
                schema["properties"][top]["x-default-from"] = fill.name().into();
                continue;
            }
            Some(_) => def_name(&schema["properties"][top]),
        };
        if let (Some(name), Some(sub)) = (name, sub) {
            schema["$defs"][name.as_str()]["properties"][sub]["x-default-from"] = fill.name().into();
        }
    }
}

/// `guns.schema.json` and `bullets.schema.json` (a file of entries, `$defs.entry`): origins of the entry's keys.
pub fn weapons(schema: &mut Value, engine_keys: &[&str]) {
    schema["properties"]["$schema"]["x-origin"] = origin(true);
    mark(&mut schema["$defs"]["entry"]["properties"], &|k| engine_keys.contains(&k));
}

/// Annotates the loader's schemas (`data::schemas()` order: ship, guns, bullets, ...).
pub fn all(schemas: &mut [(&'static str, Value)]) {
    for (name, schema) in schemas {
        match *name {
            "ship.schema.json" => ship(schema),
            "guns.schema.json" => weapons(schema, ENGINE_GUN_KEYS),
            "bullets.schema.json" => weapons(schema, &[]),
            _ => {}
        }
    }
}
