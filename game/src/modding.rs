//! Mods: `--mod <dir>` (repeatable) overlays a folder onto the base assets. The merged tree is built once per launch under
//! `<assets>/../local/overlay` as symlinks (hard links, or copies, off Unix) so every loader, Bevy's included, sees a single root.
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

/// Object files that mods add keys to instead of replacing: the one list (`render/mod.rs` reads the two render files it names,
/// `docs/modding.md` lists them all; a test keeps the doc in step).
pub const MERGED: &[&str] = &[
    "hud/scales.json",
    "data/collision.json",
    "data/docks.json",
    "data/weapons/guns.json",
    "data/weapons/bullets.json",
    "data/ai/weapons.json",
    "menu/strings.json",
    crate::render::SKIES,
    crate::render::NEBULAE,
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
            } else if name.ends_with(".glb") && (name.starts_with("ships/") || name.starts_with("models/")) {
                // A model that names retail textures (`retail:<name>`) gets them from the player's import (retail_tex.rs).
                match std::fs::read(&from).ok().and_then(|bytes| crate::retail_tex::bake(&bytes, base)) {
                    Some(baked) => {
                        let _ = std::fs::remove_file(&to);
                        std::fs::write(&to, baked).unwrap();
                    }
                    None => link(&from, &to),
                }
            } else {
                link(&from, &to);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_merged_list_in_docs_modding_md_is_this_one() {
        let doc = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/modding.md")).unwrap();
        let row = doc.lines().find(|l| l.contains("**merged**, deep")).expect("the merge rule row of docs/modding.md");
        let mut listed: Vec<&str> = row.split('|').nth(1).unwrap().split('`').skip(1).step_by(2).collect();
        let mut code: Vec<&str> = MERGED.to_vec();
        listed.sort();
        code.sort();
        assert_eq!(listed, code, "docs/modding.md and modding::MERGED list different files");
    }

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
    #[test]
    fn merge_overrides_nested_keys_only() {
        let mut b = serde_json::json!({"physics": {"MaxSpeed": 10.0, "Mass": 5}, "name": "a"});
        merge(&mut b, &serde_json::json!({"physics": {"MaxSpeed": 20.0}}));
        assert_eq!(b["physics"]["MaxSpeed"], 20.0);
        assert_eq!(b["physics"]["Mass"], 5);
    }
}
