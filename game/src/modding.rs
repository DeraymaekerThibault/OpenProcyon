//! Mods: `--mod <dir>` (repeatable) overlays a folder onto the base assets. The merged tree is built once per launch under
//! `<assets>/../local/overlay` as symlinks (hard links, or copies, off Unix) so every loader, Bevy's included, sees a single root.
//! Rules: a mod file replaces the base file of the same path (a new `ships/<id>/` or `maps/<id>/` folder is new content);
//! a JSON with `"extends": "Ship_Navy_Cutter"` is deep-merged over the same file of that base ship/map, so a new ship is a few lines.
use serde_json::Value;
use std::collections::HashMap;
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
    // A hard link costs nothing (symlinks need a privilege on Windows); a copy when the overlay is on another volume.
    // Safe because the overlay never writes through a file: merged ones are removed first and written anew.
    #[cfg(not(unix))]
    if std::fs::hard_link(&from, to).is_err() {
        std::fs::copy(&from, to).expect("copy");
    }
}

static ACTIVE: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();

/// Records this launch's mod folders (switched-on installed mods, then `--mod` folders, in overlay order). Set once, at startup.
pub fn set_active(dirs: Vec<PathBuf>) {
    let _ = ACTIVE.set(dirs);
}

/// The mod folders overlaid in this launch (empty before `set_active`, as in unit tests).
pub fn active() -> &'static [PathBuf] {
    ACTIVE.get().map_or(&[], Vec::as_slice)
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
    /// A bundled mod whose `mod.json` has `"default": true`: on until the player switches it off.
    pub default: bool,
}

/// Mods are installed by unzipping into this folder; `mods.json` beside it (`{"enabled": ["id", ...], "disabled": ["id", ...]}`, load order,
/// later wins) lists the ones that are on. A mod never listed stays off, so dropping a folder in changes nothing until the player enables it;
/// only a bundled mod marked `"default": true` starts on, and `disabled` remembers that the player switched it off.
pub fn mods_dir() -> Option<PathBuf> {
    Some(crate::campaign::user_dir()?.join("mods"))
}

/// Every mod folder in `<user>/mods/` and in `bundled` (the `mods/` that ships with the game; a user folder of the same id wins), with
/// its `mod.json` (`id` defaults to the folder name, `name` to the id). Enabled ones first in `mods.json` order, the rest by id.
pub fn installed_in(user: &Path, bundled: Option<&Path>) -> Vec<Installed> {
    let saved = read(&user.join("mods.json")).unwrap_or_default();
    let list = |k: &str| -> Vec<String> { serde_json::from_value(saved[k].clone()).unwrap_or_default() };
    let (on, off) = (list("enabled"), list("disabled"));
    let mut found: Vec<Installed> = vec![];
    for (dir, is_bundled) in [(Some(user.join("mods")), false), (bundled.map(Path::to_path_buf), true)] {
        let Some(dir) = dir else { continue };
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.path().is_dir()) {
            let meta = read(&e.path().join("mod.json")).unwrap_or_default();
            let text = |k: &str| meta[k].as_str().unwrap_or_default().to_string();
            let id = Some(text("id")).filter(|s| !s.is_empty()).unwrap_or_else(|| e.file_name().to_string_lossy().into_owned());
            if found.iter().any(|m| m.id == id) {
                continue;
            }
            let name = Some(text("name")).filter(|s| !s.is_empty()).unwrap_or_else(|| id.clone());
            let category = Some(text("category")).filter(|s| !s.is_empty()).unwrap_or_else(|| "Other".into());
            let default = is_bundled && meta["default"].as_bool() == Some(true);
            let enabled = on.contains(&id) || (default && !off.contains(&id));
            found.push(Installed { enabled, default, id, name, version: text("version"), description: text("description"), category, dir: e.path() });
        }
    }
    found.sort_by_key(|m| (on.iter().position(|i| *i == m.id).unwrap_or(usize::MAX), m.id.clone()));
    found
}

pub fn installed(bundled: Option<&Path>) -> Vec<Installed> {
    crate::campaign::user_dir().map(|d| installed_in(&d, bundled)).unwrap_or_else(|| installed_in(Path::new(""), bundled))
}

/// Writes `mods.json` from the list's enabled mods, in list order, and the default mods switched off.
pub fn save_enabled_in(dir: &Path, mods: &[Installed]) {
    let ids = |keep: fn(&Installed) -> bool| -> Vec<&str> { mods.iter().filter(|m| keep(m)).map(|m| m.id.as_str()).collect() };
    let (on, off) = (ids(|m| m.enabled), ids(|m| m.default && !m.enabled));
    let _ = std::fs::create_dir_all(dir);
    let json = serde_json::json!({ "enabled": on, "disabled": off });
    if let Err(e) = std::fs::write(dir.join("mods.json"), serde_json::to_string_pretty(&json).unwrap_or_default()) {
        eprintln!("could not save mods.json: {e}");
    }
}

/// Object files that mods add keys to instead of replacing.
const MERGED: &[&str] = &[
    "hud/scales.json",
    "data/collision.json",
    "data/docks.json",
    "data/weapons/guns.json",
    "data/weapons/bullets.json",
    "data/ai/weapons.json",
    "menu/strings.json",
    "data/render/skies.json",
    "data/render/nebulae.json",
];

fn read(p: &Path) -> Option<Value> {
    crate::data::try_json(p)
}

/// Marks a `PROCYON_OVERLAY` folder as one the game built, and so may delete and rebuild.
const OVERLAY_MARK: &str = ".openprocyon-overlay";

/// True when the game may delete `dir` to rebuild an overlay there: missing, empty, or marked as an overlay it built earlier.
fn disposable(dir: &Path) -> bool {
    dir.join(OVERLAY_MARK).exists() || std::fs::read_dir(dir).map_or(true, |mut d| d.next().is_none())
}

/// Builds the overlay and returns its root; with no mods returns `base` unchanged. `own`: a folder of the launch's own
/// (PROCYON_OVERLAY, read by the caller: the tools give each checkout and each parallel run one, so runs sharing an import do not
/// rebuild each other's tree); else `<base>/../local/overlay`.
pub fn overlay(base: &Path, mods: &[PathBuf], own: Option<PathBuf>) -> PathBuf {
    if mods.is_empty() {
        return base.to_path_buf();
    }
    if let Some(dir) = &own {
        assert!(disposable(dir), "PROCYON_OVERLAY {} holds files that are not an overlay; refusing to delete it", dir.display());
    }
    let root = own.clone().unwrap_or_else(|| base.join("../local/overlay"));
    let _ = std::fs::remove_dir_all(&root);
    if own.is_some() {
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(OVERLAY_MARK), "").unwrap();
    }
    let mut files = vec![];
    walk(base, Path::new(""), &mut files);
    for rel in &files {
        let to = root.join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        link(&base.join(rel), &to);
    }
    for m in mods {
        let modname = m.file_name().map_or_else(|| m.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
        let mut mf = vec![];
        walk(m, Path::new(""), &mut mf);
        // A JSON file that does not parse is skipped with one error line naming file and mod (the base file stays); the
        // rest of the mod still loads. Parsed once here: extends detection and the merge below reuse it.
        let mut docs: HashMap<PathBuf, Value> = HashMap::new();
        mf.retain(|rel| {
            if rel.extension().is_none_or(|e| e != "json") {
                return true;
            }
            match crate::data::load_as::<Value>(&m.join(rel), Some(&modname)) {
                Ok(v) => {
                    docs.insert(rel.clone(), v);
                    true
                }
                Err(e) => {
                    eprintln!("error: mod {modname}: skipped {}: {e}", rel.display());
                    false
                }
            }
        });
        // Files that extend another go last (stable sort over sorted names), so they see sibling mod files, not only the base.
        // shortcut: one level; a chain of extends inside one mod needs a topological order.
        mf.sort();
        mf.sort_by_key(|r| docs.get(r).is_some_and(|v| v.get("extends").is_some()));
        for rel in mf {
            let (from, to) = (m.join(&rel), root.join(&rel));
            let name = rel.to_string_lossy().replace('\\', "/");
            if let Err(e) = apply_file(&modname, &name, &from, &to, base, docs.get(&rel)) {
                eprintln!("error: mod {modname}: skipped {name}: {e}");
            }
        }
    }
    // Ships the mods bring get the UI art their model implies, unless they ship their own (shipart.rs).
    let mut ids: Vec<String> =
        mods.iter().flat_map(|m| std::fs::read_dir(m.join("ships")).into_iter().flatten().flatten()).filter_map(|e| e.file_name().into_string().ok()).collect();
    ids.sort();
    ids.dedup();
    crate::shipart::generate(&root, &ids);
    root
}

/// Puts one mod file into the overlay at `to`. An `Err` leaves the base file in place; the caller logs it and goes on.
/// `doc`: the file's parsed JSON, for a `.json` file.
fn apply_file(modname: &str, name: &str, from: &Path, to: &Path, base: &Path, doc: Option<&Value>) -> Result<(), String> {
    fn io(what: &'static str) -> impl Fn(std::io::Error) -> String {
        move |e| format!("{what}: {e}")
    }
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(io("cannot create the overlay folder"))?;
    }
    let write = |v: &Value| -> Result<(), String> {
        let text = serde_json::to_string(v).map_err(|e| format!("cannot write the merged file: {e}"))?;
        let _ = std::fs::remove_file(to);
        std::fs::write(to, text).map_err(io("cannot write the merged file"))
    };
    if name == "mod.json" {
        let v = doc.cloned().unwrap_or_default();
        eprintln!("mod {} {} {}", v["name"].as_str().unwrap_or(modname), v["version"].as_str().unwrap_or(""), v["description"].as_str().unwrap_or(""));
        return Ok(());
    }
    if MERGED.contains(&name) {
        // The file earlier layers left in the overlay; a missing one starts empty, an unreadable one is an error.
        let mut b = if to.exists() { crate::data::load_as::<Value>(to, None).map_err(|e| e.to_string())? } else { Value::Null };
        if !b.is_object() {
            b = Value::Object(Default::default())
        }
        merge(&mut b, doc.unwrap_or(&Value::Null));
        return write(&b);
    }
    if let (true, Some(v)) = (name.ends_with(".json"), doc.filter(|v| v.get("extends").is_some())) {
        let parent = v["extends"].as_str().unwrap_or_default();
        // ships/<id>/ship.json extends ships/<parent>/ship.json; other files extend a sibling <parent>.json.
        let parent_path = match name.split('/').count() == 3 && (name.starts_with("ships/") || name.starts_with("maps/")) {
            true => to.parent().and_then(Path::parent).zip(to.file_name()).map(|(d, f)| d.join(parent).join(f)),
            false => Some(to.with_file_name(format!("{parent}.json"))),
        }
        .ok_or_else(|| format!("cannot place the file extended by '{parent}'"))?;
        if !parent_path.exists() {
            return Err(format!("extends unknown '{parent}' (no {})", parent_path.display()));
        }
        let mut b = crate::data::load_as::<Value>(&parent_path, None).map_err(|e| format!("extends '{parent}': {e}"))?;
        merge(&mut b, v);
        let obj = b.as_object_mut().ok_or_else(|| format!("extends '{parent}', which is not a JSON object"))?;
        obj.remove("extends");
        // "set": {"/engines/0/fx": "X"} replaces single values inside arrays, which a merge cannot reach.
        if let Some(Value::Object(set)) = obj.remove("set") {
            for (ptr, value) in set {
                match b.pointer_mut(&ptr) {
                    Some(slot) => *slot = value,
                    None => return Err(format!("sets '{ptr}', which does not exist")),
                }
            }
        }
        return write(&b);
    }
    if name.ends_with(".glb") && (name.starts_with("ships/") || name.starts_with("models/")) {
        // A model that names retail textures (`retail:<name>`) gets them from the player's import (retail_tex.rs).
        match std::fs::read(from).ok().and_then(|bytes| crate::retail_tex::bake(&bytes, base)) {
            Some(baked) => {
                let _ = std::fs::remove_file(to);
                std::fs::write(to, baked).map_err(io("cannot write the baked model"))?;
            }
            None => link(from, to),
        }
    } else {
        link(from, to);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_an_empty_or_marked_folder_may_be_rebuilt_as_an_overlay() {
        let dir = std::env::temp_dir().join(format!("procyon_overlay_guard_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(disposable(&dir), "a missing folder");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(disposable(&dir), "an empty folder");
        std::fs::write(dir.join("notes.txt"), "mine").unwrap();
        assert!(!disposable(&dir), "somebody's files are never deleted");
        std::fs::write(dir.join(OVERLAY_MARK), "").unwrap();
        assert!(disposable(&dir), "an overlay the game built");
        std::fs::remove_dir_all(dir).unwrap();
    }

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
        let root = overlay(&base, &[m], None);
        let b: Value = read(&root.join("data/b.json")).unwrap();
        assert_eq!((b["x"].as_i64(), b["y"].as_i64()), (Some(2), Some(3)), "b extends the mod's own a.json, not the base one");
        // overlay files may be links to the base: replacing one must never write through to the original
        assert_eq!(std::fs::read_to_string(base.join("data/a.json")).unwrap(), r#"{"x": 1, "y": 1}"#);
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
    fn default_bundled_mods_start_on_until_switched_off() {
        let dir = std::env::temp_dir().join(format!("procyon-mods-default-{}", std::process::id()));
        let bundled = dir.join("bundled");
        for (root, f) in [(&bundled, "speed"), (&bundled, "plain"), (&dir.join("mods"), "mine")] {
            std::fs::create_dir_all(root.join(f)).unwrap();
            std::fs::write(root.join(f).join("mod.json"), if f == "plain" { "{}" } else { r#"{"default": true}"# }).unwrap();
        }
        let on = |list: &[Installed]| list.iter().filter(|m| m.enabled).map(|m| m.id.clone()).collect::<Vec<_>>();
        let mut list = installed_in(&dir, Some(&bundled));
        assert_eq!(on(&list), ["speed"], "no mods.json yet: only the bundled default is on; a user folder cannot switch itself on");
        let i = list.iter().position(|m| m.id == "speed").unwrap();
        list[i].enabled = false;
        save_enabled_in(&dir, &list);
        assert!(on(&installed_in(&dir, Some(&bundled))).is_empty(), "switching a default mod off is remembered");
        std::fs::write(dir.join("mods.json"), r#"{"enabled": ["plain"]}"#).unwrap();
        assert_eq!(on(&installed_in(&dir, Some(&bundled))), ["plain", "speed"], "an older mods.json without `disabled` keeps the default on");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn collision_shapes_merge_by_type() {
        let dir = std::env::temp_dir().join(format!("procyon-mod-collision-{}", std::process::id()));
        let (base, m) = (dir.join("assets"), dir.join("mod"));
        for d in [base.join("data"), m.join("data")] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(base.join("data/collision.json"), r#"{"Island01": {"min": [0, 0, 0], "max": [1, 1, 1]}}"#).unwrap();
        std::fs::write(m.join("data/collision.json"), r#"{"Island_Ours": {"min": [0, 0, 0], "max": [2, 2, 2]}}"#).unwrap();
        let c: Value = read(&overlay(&base, &[m], None).join("data/collision.json")).unwrap();
        assert!(c["Island01"].is_object() && c["Island_Ours"].is_object(), "a mod's scenery type is added beside the retail ones");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn dock_points_merge_by_type() {
        let dir = std::env::temp_dir().join(format!("procyon-mod-docks-{}", std::process::id()));
        let (base, m) = (dir.join("assets"), dir.join("mod"));
        for d in [base.join("data"), m.join("data")] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(base.join("data/docks.json"), r#"{"Base_X": {"points": [{"label": "DockPoint1", "pos": [0, 0, 0], "yaw": 0}], "tender": true}}"#).unwrap();
        std::fs::write(m.join("data/docks.json"), r#"{"FerrumTown": {"points": [{"label": "Berth1", "pos": [1, 2, 0], "yaw": 1.5}], "tender": true}}"#).unwrap();
        let c: Value = read(&overlay(&base, &[m], None).join("data/docks.json")).unwrap();
        assert!(c["Base_X"]["points"].is_array() && c["FerrumTown"]["points"].is_array(), "a mod's dock points are added beside the retail ones");
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
        let root = overlay(&base, &[m], None);
        let a: Value = read(&root.join("data/a.json")).unwrap();
        assert_eq!(a, serde_json::json!({"list": [{"fx": "x", "pos": 1}, {"fx": "z"}]}));
        std::fs::remove_dir_all(dir).unwrap();
    }
    /// A synthetic base + one mod in a temp dir; `files` are the mod's (path, text). Returns the overlay root and the stderr-free state.
    fn bad_mod(tag: &str, base_files: &[(&str, &str)], files: &[(&str, &str)]) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("procyon-mod-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (root, list) in [("assets", base_files), ("mod", files)] {
            for (f, text) in list {
                let p = dir.join(root).join(f);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, text).unwrap();
            }
        }
        let root = overlay(&dir.join("assets"), &[dir.join("mod")], None);
        (dir, root)
    }
    #[test]
    fn bad_json_in_a_merged_file_is_skipped_and_the_base_file_survives() {
        let (dir, root) = bad_mod(
            "bad-merged",
            &[("data/collision.json", r#"{"Island01": 1}"#), ("data/a.json", r#"{"x": 1}"#)],
            &[("data/collision.json", r#"{"Island_Ours": "#), ("data/a.json", r#"{"x": 2}"#)],
        );
        let c: Value = read(&root.join("data/collision.json")).unwrap();
        assert_eq!(c, serde_json::json!({"Island01": 1}), "neither an empty value nor a half merge");
        assert_eq!(read(&root.join("data/a.json")).unwrap()["x"], 2, "the mod's other files still load");
        let e = crate::data::load_as::<Value>(&dir.join("mod/data/collision.json"), Some("mod")).unwrap_err().to_string();
        assert!(e.contains("collision.json") && e.contains("mod \"mod\"") && e.contains("line 1 column"), "{e}");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn bad_json_in_mod_json_does_not_stop_the_mod_or_panic() {
        let (dir, root) = bad_mod("bad-modjson", &[("data/a.json", r#"{"x": 1}"#)], &[("mod.json", "{not json"), ("data/a.json", r#"{"x": 2}"#)]);
        assert_eq!(read(&root.join("data/a.json")).unwrap()["x"], 2);
        assert!(!root.join("mod.json").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn bad_json_in_an_extends_file_and_a_bad_extends_leave_the_base_file() {
        let (dir, root) = bad_mod(
            "bad-extends",
            &[("data/a.json", r#"{"x": 1}"#), ("data/c.json", r#"{"x": 1}"#), ("data/d.json", r#"{"list": [1]}"#)],
            &[
                ("data/a.json", r#"{"extends": "a", "y": "#),
                ("data/b.json", r#"{"extends": "missing", "y": 1}"#),
                ("data/c.json", r#"{"extends": "c", "y": 3}"#),
                ("data/d.json", r#"{"extends": "d", "set": {"/list/5": 1}}"#),
            ],
        );
        assert_eq!(read(&root.join("data/a.json")).unwrap(), serde_json::json!({"x": 1}), "unparsable extends file: base stays");
        assert!(!root.join("data/b.json").exists(), "unknown parent: nothing added");
        assert_eq!(read(&root.join("data/c.json")).unwrap(), serde_json::json!({"x": 1, "y": 3}), "a good extends file in the same mod still applies");
        assert_eq!(read(&root.join("data/d.json")).unwrap(), serde_json::json!({"list": [1]}), "a set path that does not exist leaves the base");
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
