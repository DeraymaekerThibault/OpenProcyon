//! Typed asteroid definitions (`data/asteroids.json`; layout: docs/data-format.md).
//! Same fields and same rules as the untyped reader had: every field is required, the numbers are positive and finite, a rock has
//! ten breakup slots (empty = unused), each named child exists and a breakup chain ends. A fault now names the file, its mod and
//! the entry and field. Unknown fields are allowed.
use super::*;
use serde::Deserialize;
use std::collections::BTreeMap;

/// One entry of `asteroids.json`.
#[derive(Deserialize)]
pub(super) struct AsteroidDoc {
    scene: String,
    hp: f64,
    mass: f64,
    radius: f64,
    acceleration: f64,
    children: Vec<String>,
    effect: String,
}

/// Breakup slots of a rock (the retail definitions' death-spawn list).
const SLOTS: usize = 10;

fn positive(name: &str, key: &str, v: f64) -> Result<f32, String> {
    let v = v as f32;
    if v.is_finite() && v > 0.0 { Ok(v) } else { Err(format!("asteroid {name}: {key} must be a positive number")) }
}

fn visit(name: &str, defs: &HashMap<String, AsteroidDef>, ancestors: &mut Vec<String>) -> Result<(), String> {
    if ancestors.iter().any(|a| a == name) {
        return Err(format!("asteroid {name}: its breakup chain comes back to it ({} -> {name})", ancestors.join(" -> ")));
    }
    let Some(d) = defs.get(name) else { return Err(format!("asteroid {}: child {name:?} is not defined", ancestors.last().map_or("?", String::as_str))) };
    ancestors.push(name.into());
    for child in d.children.iter().filter(|c| !c.is_empty()) {
        visit(child, defs, ancestors)?;
    }
    ancestors.pop();
    Ok(())
}

/// The typed entries as definitions, checked as the exporter checks them (tools/export_asteroids.py).
pub(super) fn build(docs: BTreeMap<String, AsteroidDoc>) -> Result<HashMap<String, AsteroidDef>, String> {
    let mut defs = HashMap::new();
    for (name, d) in docs {
        if d.children.len() != SLOTS {
            return Err(format!("asteroid {name}: children needs {SLOTS} slots (empty text = unused), found {}", d.children.len()));
        }
        let def = AsteroidDef {
            scene: d.scene,
            hp: positive(&name, "hp", d.hp)?,
            mass: positive(&name, "mass", d.mass)?,
            radius: positive(&name, "radius", d.radius)?,
            acceleration: positive(&name, "acceleration", d.acceleration)?,
            children: d.children,
            effect: d.effect,
        };
        defs.insert(name, def);
    }
    let mut names: Vec<_> = defs.keys().cloned().collect();
    names.sort();
    for name in &names {
        visit(name, &defs, &mut vec![])?;
    }
    Ok(defs)
}

/// The asteroid file at `path`, or the error text (file, mod, serde's line/column/field message, or the entry and field).
pub fn try_load(path: &std::path::Path) -> Result<HashMap<String, AsteroidDef>, String> {
    build(load::<BTreeMap<String, AsteroidDoc>>(path).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", asset_context(path)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rock() -> Value {
        json!({"scene": "AsteroidSmall", "hp": 50, "mass": 10.5, "radius": 6, "acceleration": 2, "children": ["", "", "", "", "", "", "", "", "", ""], "effect": "fx"})
    }
    fn children(list: &[&str]) -> Value {
        let mut c: Vec<&str> = list.to_vec();
        c.resize(SLOTS, "");
        json!(c)
    }
    fn file(tag: &str, doc: &Value) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("procyon-asteroids-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("asteroids.json");
        std::fs::write(&path, doc.to_string()).unwrap();
        path
    }
    fn fails(tag: &str, doc: Value) -> String {
        let path = file(tag, &doc);
        let e = try_load(&path).err().unwrap_or_else(|| panic!("{tag}: loaded"));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
        e
    }

    #[test]
    fn a_valid_file_loads_with_its_breakup_chain() {
        let mut big = rock();
        big["children"] = children(&["Small", "Small"]);
        let path = file("ok", &json!({"Big": big, "Small": rock()}));
        let defs = try_load(&path).unwrap();
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert_eq!(defs["Big"].children.iter().filter(|c| !c.is_empty()).count(), 2);
        assert_eq!((defs["Small"].hp, defs["Small"].mass, defs["Small"].acceleration), (50.0, 10.5, 2.0));
    }

    #[test]
    fn a_missing_or_mistyped_field_names_the_file_and_the_field() {
        let mut no_hp = rock();
        no_hp.as_object_mut().unwrap().remove("hp");
        let e = fails("nohp", json!({"Rock": no_hp}));
        assert!(e.contains("asteroids.json") && e.contains("missing field `hp`"), "{e}");
        let mut text_mass = rock();
        text_mass["mass"] = json!("heavy");
        let e = fails("text", json!({"Rock": text_mass}));
        assert!(e.contains("asteroids.json") && e.contains("invalid type"), "{e}");
        assert!(fails("shape", json!([1, 2])).contains("asteroids.json"));
    }

    #[test]
    fn numbers_must_be_positive_and_the_slots_ten() {
        for key in ["hp", "mass", "radius", "acceleration"] {
            let mut r = rock();
            r[key] = json!(0);
            let e = fails(key, json!({"Rock": r}));
            assert!(e.contains(&format!("asteroid Rock: {key} must be a positive number")), "{e}");
        }
        let mut r = rock();
        r["children"] = json!(["", ""]);
        let e = fails("slots", json!({"Rock": r}));
        assert!(e.contains("asteroid Rock: children needs 10 slots") && e.contains("found 2"), "{e}");
    }

    #[test]
    fn an_unknown_child_and_a_cycle_are_named() {
        let mut r = rock();
        r["children"] = children(&["Ghost"]);
        let e = fails("ghost", json!({"Rock": r}));
        assert!(e.contains("asteroid Rock: child \"Ghost\" is not defined"), "{e}");
        let (mut a, mut b) = (rock(), rock());
        (a["children"], b["children"]) = (children(&["B"]), children(&["A"]));
        let e = fails("cycle", json!({"A": a, "B": b}));
        assert!(e.contains("breakup chain comes back"), "{e}");
    }

    #[test]
    fn a_file_that_is_not_json_or_is_missing_gives_an_error_not_a_panic() {
        let dir = std::env::temp_dir().join(format!("procyon-asteroids-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("asteroids.json"), "not json").unwrap();
        assert!(try_load(&dir.join("asteroids.json")).unwrap_err().contains("asteroids.json"));
        assert!(try_load(&dir.join("absent.json")).unwrap_err().contains("absent.json"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
