//! Mods: `--mod <dir>` (repeatable) overlays a folder onto the base assets. The merged tree is built once per launch under
//! `<assets>/../local/overlay` as symlinks (copies off Unix) so every loader, Bevy's included, sees a single root.
//! Rules: a mod file replaces the base file of the same path (a new `ships/<id>/` or `maps/<id>/` folder is new content);
//! a JSON with `"extends": "Ship_Navy_Cutter"` is deep-merged over the same file of that base ship/map, so a new ship is a few lines.
use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn walk(dir: &Path, rel: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let r = rel.join(e.file_name());
        if e.file_type().is_ok_and(|t| t.is_dir()) { walk(&e.path(), &r, out) } else { out.push(r) }
    }
}

fn link(from: &Path, to: &Path) {
    let from = std::fs::canonicalize(from).expect("canonical asset path");
    let _ = std::fs::remove_file(to);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&from, to).expect("symlink");
    #[cfg(not(unix))]
    std::fs::copy(&from, to).expect("copy");
}

/// Objects merge key by key, anything else is replaced.
pub fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                merge(b.entry(k.clone()).or_insert(Value::Null), v)
            }
        }
        (b, o) => *b = o.clone(),
    }
}

/// A mod folder found in `<user dir>/mods/`.
#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// `mod.json` `category` ("Ships", "Visuals", ...); "Other" when absent.
    pub category: String,
    pub dir: PathBuf,
    pub enabled: bool,
}

/// Mods are installed by unzipping into this folder; `mods.json` beside it (`{"enabled": ["id", ...]}`, load order, later wins) lists the
/// ones that are on. A mod never listed stays off, so dropping a folder in changes nothing until the player enables it.
pub fn mods_dir() -> Option<PathBuf> {
    Some(crate::campaign::user_dir()?.join("mods"))
}

/// Every mod folder in `<user>/mods/` and in `bundled` (the `mods/` that ships with the game; a user folder of the same id wins), with
/// its `mod.json` (`id` defaults to the folder name, `name` to the id). Enabled ones first in `mods.json` order, the rest by id.
pub fn installed_in(user: &Path, bundled: Option<&Path>) -> Vec<Installed> {
    let on: Vec<String> = read(&user.join("mods.json")).and_then(|v| serde_json::from_value(v["enabled"].clone()).ok()).unwrap_or_default();
    let mut found: Vec<Installed> = vec![];
    for dir in [Some(user.join("mods")), bundled.map(Path::to_path_buf)].into_iter().flatten() {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.path().is_dir()) {
            let meta = read(&e.path().join("mod.json")).unwrap_or_default();
            let text = |k: &str| meta[k].as_str().unwrap_or_default().to_string();
            let id = Some(text("id")).filter(|s| !s.is_empty()).unwrap_or_else(|| e.file_name().to_string_lossy().into_owned());
            if found.iter().any(|m| m.id == id) {
                continue;
            }
            let name = Some(text("name")).filter(|s| !s.is_empty()).unwrap_or_else(|| id.clone());
            let category = Some(text("category")).filter(|s| !s.is_empty()).unwrap_or_else(|| "Other".into());
            found.push(Installed { enabled: on.contains(&id), id, name, version: text("version"), description: text("description"), category, dir: e.path() });
        }
    }
    found.sort_by_key(|m| (on.iter().position(|i| *i == m.id).unwrap_or(usize::MAX), m.id.clone()));
    found
}

pub fn installed(bundled: Option<&Path>) -> Vec<Installed> {
    crate::campaign::user_dir().map(|d| installed_in(&d, bundled)).unwrap_or_else(|| installed_in(Path::new(""), bundled))
}

/// Writes `mods.json` from the list's enabled mods, in list order.
pub fn save_enabled_in(dir: &Path, mods: &[Installed]) {
    let on: Vec<&str> = mods.iter().filter(|m| m.enabled).map(|m| m.id.as_str()).collect();
    let _ = std::fs::create_dir_all(dir);
    if let Err(e) = std::fs::write(dir.join("mods.json"), serde_json::to_string_pretty(&serde_json::json!({ "enabled": on })).unwrap_or_default()) {
        eprintln!("could not save mods.json: {e}");
    }
}

/// Object files that mods add keys to instead of replacing.
const MERGED: &[&str] = &["hud/scales.json", "data/weapons/guns.json", "data/weapons/bullets.json", "data/ai/weapons.json"];

fn read(p: &Path) -> Option<Value> {
    crate::data::try_json(p)
}

/// Builds the overlay and returns its root; with no mods returns `base` unchanged.
pub fn overlay(base: &Path, mods: &[PathBuf]) -> PathBuf {
    if mods.is_empty() {
        return base.to_path_buf();
    }
    let root = base.join("../local/overlay");
    let _ = std::fs::remove_dir_all(&root);
    let mut files = vec![];
    walk(base, Path::new(""), &mut files);
    for rel in &files {
        let to = root.join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        link(&base.join(rel), &to);
    }
    for m in mods {
        let mut mf = vec![];
        walk(m, Path::new(""), &mut mf);
        // Files that extend another go last (stable sort over sorted names), so they see sibling mod files, not only the base.
        // shortcut: one level; a chain of extends inside one mod needs a topological order.
        mf.sort();
        mf.sort_by_key(|r| read(&m.join(r)).is_some_and(|v| v.get("extends").is_some()));
        for rel in mf {
            let (from, to) = (m.join(&rel), root.join(&rel));
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            let name = rel.to_string_lossy().replace('\\', "/");
            if name == "mod.json" {
                let v = read(&from).unwrap_or_default();
                eprintln!("mod {} {} {}", v["name"].as_str().unwrap_or(&m.to_string_lossy()), v["version"].as_str().unwrap_or(""), v["description"].as_str().unwrap_or(""));
                continue;
            }
            if MERGED.contains(&name.as_str()) {
                let mut b = read(&root.join(&rel)).unwrap_or(Value::Null);
                if !b.is_object() {
                    b = Value::Object(Default::default())
                }
                merge(&mut b, &read(&from).unwrap_or_default());
                let _ = std::fs::remove_file(&to);
                std::fs::write(&to, serde_json::to_string(&b).unwrap()).unwrap();
                continue;
            }
            if let (true, Some(v)) = (name.ends_with(".json"), read(&from).filter(|v| v.get("extends").is_some())) {
                let parent = v["extends"].as_str().unwrap_or_default();
                // ships/<id>/ship.json extends ships/<parent>/ship.json; other files extend a sibling <parent>.json.
                let base = match name.split('/').count() == 3 && (name.starts_with("ships/") || name.starts_with("maps/")) {
                    true => to.parent().and_then(Path::parent).map(|d| d.join(parent).join(to.file_name().unwrap())).unwrap_or_default(),
                    false => to.with_file_name(format!("{parent}.json")),
                };
                let mut b = read(&base).unwrap_or_else(|| panic!("mod {}: extends unknown '{parent}'", m.display()));
                merge(&mut b, &v);
                b.as_object_mut().unwrap().remove("extends");
                // "set": {"/engines/0/fx": "X"} replaces single values inside arrays, which a merge cannot reach.
                if let Some(Value::Object(set)) = b.as_object_mut().unwrap().remove("set") {
                    for (ptr, value) in set {
                        match b.pointer_mut(&ptr) {
                            Some(slot) => *slot = value,
                            None => panic!("mod {}: {name} sets '{ptr}', which does not exist", m.display()),
                        }
                    }
                }
                let _ = std::fs::remove_file(&to);
                std::fs::write(&to, serde_json::to_string(&b).unwrap()).unwrap();
            } else {
                link(&from, &to);
            }
        }
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extends_sees_a_sibling_mod_file_whatever_the_directory_order() {
        let dir = std::env::temp_dir().join(format!("procyon-mod-test-{}", std::process::id()));
        let (base, m) = (dir.join("assets"), dir.join("mod"));
        for d in [base.join("data"), m.join("data")] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(base.join("data/a.json"), r#"{"x": 1, "y": 1}"#).unwrap();
        std::fs::write(m.join("data/a.json"), r#"{"x": 2}"#).unwrap();
        std::fs::write(m.join("data/b.json"), r#"{"extends": "a", "y": 3}"#).unwrap();
        let root = overlay(&base, &[m]);
        let b: Value = read(&root.join("data/b.json")).unwrap();
        assert_eq!((b["x"].as_i64(), b["y"].as_i64()), (Some(2), Some(3)), "b extends the mod's own a.json, not the base one");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn discovery_lists_folders_and_keeps_enabled_order() {
        let dir = std::env::temp_dir().join(format!("procyon-mods-list-{}", std::process::id()));
        for (f, meta) in [("a", r#"{"name": "Alpha", "version": "1"}"#), ("b", r#"{"id": "bee"}"#), ("c", "")] {
            std::fs::create_dir_all(dir.join("mods").join(f)).unwrap();
            if !meta.is_empty() {
                std::fs::write(dir.join("mods").join(f).join("mod.json"), meta).unwrap();
            }
        }
        std::fs::write(dir.join("mods.json"), r#"{"enabled": ["c", "bee", "gone"]}"#).unwrap();
        let bundled = dir.join("bundled");
        std::fs::create_dir_all(bundled.join("a")).unwrap();
        std::fs::create_dir_all(bundled.join("zz")).unwrap();
        std::fs::write(bundled.join("a/mod.json"), r#"{"name": "Shadowed"}"#).unwrap();
        let mut list = installed_in(&dir, Some(&bundled));
        let ids: Vec<_> = list.iter().map(|m| (m.id.as_str(), m.enabled)).collect();
        assert_eq!(ids, [("c", true), ("bee", true), ("a", false), ("zz", false)], "enabled first in file order, unknown ids ignored, bundled listed too");
        assert_eq!(list[2].name, "Alpha", "the user folder wins over a bundled mod of the same id");
        list[2].enabled = true;
        save_enabled_in(&dir, &list);
        assert_eq!(installed_in(&dir, Some(&bundled)).iter().filter(|m| m.enabled).count(), 3);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn set_replaces_one_value_inside_an_array() {
        let dir = std::env::temp_dir().join(format!("procyon-mod-set-{}", std::process::id()));
        let (base, m) = (dir.join("assets"), dir.join("mod"));
        for d in [base.join("data"), m.join("data")] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(base.join("data/a.json"), r#"{"list": [{"fx": "x", "pos": 1}, {"fx": "y"}]}"#).unwrap();
        std::fs::write(m.join("data/a.json"), r#"{"extends": "a", "set": {"/list/1/fx": "z"}}"#).unwrap();
        let root = overlay(&base, &[m]);
        let a: Value = read(&root.join("data/a.json")).unwrap();
        assert_eq!(a, serde_json::json!({"list": [{"fx": "x", "pos": 1}, {"fx": "z"}]}));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn merge_overrides_nested_keys_only() {
        let mut b = serde_json::json!({"physics": {"MaxSpeed": 10.0, "Mass": 5}, "name": "a"});
        merge(&mut b, &serde_json::json!({"physics": {"MaxSpeed": 20.0}}));
        assert_eq!(b["physics"]["MaxSpeed"], 20.0);
        assert_eq!(b["physics"]["Mass"], 5);
    }
}
