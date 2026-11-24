//! "Compare with the import": which files of a mod are the player's game content, by meaning (what `tools/compare_assets.py` does for
//! two imports). A mod must not ship these (AGENTS.md hard rule 1), and a copy that was only reformatted is still a copy.
//!
//! PROVISIONAL limits, all of them misses (a file listed here is always equal in meaning to the import's file):
//! - A file is compared with the import's file of the same path only (and a `ships/*/model.glb` with every model of the import), so
//!   a retail file moved to another path is not found, except those models.
//! - JSON is equal when the parsed values are equal (key order and layout do not matter; numbers within 1e-4, the compare script's
//!   tolerance). A file that was edited and then derived from, or that holds only some of the import's keys, is not found.
//! - PNG is compared by bytes: comparing decoded pixels needs an image decoder, and the crate has none (no dependency was added for
//!   this). A re-encoded copy of a retail picture is therefore not found.
//! - Every other file (models, sounds, ...) is compared by bytes.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

/// How a mod's file was found equal to the import's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    /// Both parse as JSON and the values are equal (numbers within 1e-4).
    Json,
    /// The bytes are equal.
    Bytes,
}

/// A file of a mod that is game content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ContentMatch {
    /// The mod's file, relative to the mod folder, with `/` separators.
    pub path: String,
    /// The file of the import it equals, relative to the import's folder (the same path, or for a model another ship's model).
    pub import_path: String,
    /// How they were found equal.
    pub kind: MatchKind,
}

/// The files of `mod_dir` that are game content: equal in meaning to a file of the import `base` at the same path (JSON by value,
/// everything else by bytes), or, for `ships/<id>/model.glb`, to any `ships/*/model.glb` of `base`. Sorted by path. Dot-files and
/// folders, symlinks and unreadable files are left out. Reads only; writes nothing. See the module doc for what is not found.
pub fn game_content_in(mod_dir: &Path, base: &Path) -> Vec<ContentMatch> {
    let mut models: Option<Vec<(String, PathBuf)>> = None;
    let mut found = vec![];
    for rel in list(mod_dir) {
        let (ours, theirs) = (mod_dir.join(&rel), base.join(&rel));
        if let Some(kind) = same_meaning(&ours, &theirs) {
            found.push(ContentMatch { import_path: rel.clone(), path: rel, kind });
        } else if is_model(&rel) {
            let models = models.get_or_insert_with(|| list(base).into_iter().filter(|r| is_model(r)).map(|r| (r.clone(), base.join(r))).collect());
            if let Some((other, _)) = models.iter().find(|(_, path)| same_bytes(&ours, path)) {
                found.push(ContentMatch { path: rel.clone(), import_path: other.clone(), kind: MatchKind::Bytes });
            }
        }
    }
    found
}

fn is_model(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    parts.len() == 3 && parts[0] == "ships" && parts[2] == "model.glb"
}

/// Every regular file under `dir` (relative, `/` separators, sorted), skipping dot-files and dot-folders and symlinks.
fn list(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, at: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(at) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else { continue };
            if name.starts_with('.') || kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                walk(root, &entry.path(), out);
            } else if kind.is_file()
                && let Ok(rel) = entry.path().strip_prefix(root)
            {
                out.push(rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"));
            }
        }
    }
    let mut out = vec![];
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) else { return false };
    ma.is_file() && mb.is_file() && ma.len() == mb.len() && matches!((std::fs::read(a), std::fs::read(b)), (Ok(x), Ok(y)) if x == y)
}

fn same_meaning(ours: &Path, theirs: &Path) -> Option<MatchKind> {
    if !theirs.is_file() {
        return None;
    }
    if same_bytes(ours, theirs) {
        return Some(MatchKind::Bytes);
    }
    if ours.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) {
        let read = |p: &Path| serde_json::from_slice::<Value>(&std::fs::read(p).ok()?).ok();
        if let (Some(a), Some(b)) = (read(ours), read(theirs))
            && json_equal(&a, &b)
        {
            return Some(MatchKind::Json);
        }
    }
    None
}

/// JSON values equal, numbers within `1e-4` relative or absolute (as `tools/compare_assets.py`); key order does not matter.
fn json_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| json_equal(v, w))),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(v, w)| json_equal(v, w)),
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => x == y || (x - y).abs() <= 1e-4_f64.max(1e-4 * x.abs().max(y.abs())),
            _ => x == y,
        },
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn put(root: &Path, rel: &str, bytes: &[u8]) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn files_equal_in_meaning_to_the_import_are_found_and_the_mods_own_work_is_not() {
        let dir = std::env::temp_dir().join(format!("procyon-content-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (base, ours) = (dir.join("import"), dir.join("mod"));
        let model = crate::data::derive::tests::model();
        put(&base, "data/a.json", br#"{"x": 1.0, "y": [1, 2, {"z": "t"}]}"#);
        put(&base, "data/edited.json", br#"{"x": 1}"#);
        put(&base, "data/bytes.json", b"not json");
        put(&base, "textures/t.png", b"\x89PNG synthetic");
        put(&base, "textures/u.png", b"\x89PNG other");
        put(&base, "sound/s.ogg", b"OggS synthetic");
        put(&base, "ships/Foo/model.glb", &model);
        // the same values, other layout, order and a rounding below the tolerance
        put(&ours, "data/a.json", br#"{"y":[1,2,{"z":"t"}],"x":1.00001}"#);
        put(&ours, "data/edited.json", br#"{"x": 2}"#);
        put(&ours, "data/bytes.json", b"not json");
        put(&ours, "textures/t.png", b"\x89PNG synthetic");
        put(&ours, "textures/u.png", b"\x89PNG changed");
        put(&ours, "sound/renamed.ogg", b"OggS synthetic");
        put(&ours, "ships/Mine/model.glb", &model);
        put(&ours, "ships/Mine/ship.json", b"{}");
        put(&ours, "mod.json", b"{}");
        put(&ours, ".hidden/data/a.json", br#"{"x": 1.0, "y": [1, 2, {"z": "t"}]}"#);
        let found: Vec<_> = game_content_in(&ours, &base).into_iter().map(|m| (m.path, m.import_path, m.kind)).collect();
        let want = [
            ("data/a.json", "data/a.json", MatchKind::Json),
            ("data/bytes.json", "data/bytes.json", MatchKind::Bytes),
            ("ships/Mine/model.glb", "ships/Foo/model.glb", MatchKind::Bytes),
            ("textures/t.png", "textures/t.png", MatchKind::Bytes),
        ];
        assert_eq!(
            found,
            want.map(|(a, b, k)| (a.to_string(), b.to_string(), k)),
            "an edited file, another path (not a model), a hidden folder and the mod's own files are not game content"
        );
        assert!(game_content_in(&dir.join("missing"), &base).is_empty() && game_content_in(&ours, &dir.join("missing")).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn json_values_are_compared_not_text() {
        let a = json!({"a": [1, 2.5, "x", null, true], "b": {"c": 0}});
        assert!(json_equal(&a, &json!({"b": {"c": 0.0}, "a": [1.00001, 2.5, "x", null, true]})));
        for other in
            [json!({"a": [1, 2.5, "x", null, true]}), json!({"a": [1, 2.6, "x", null, true], "b": {"c": 0}}), json!({"a": [1, 2.5, "x", null, false], "b": {"c": 0}}), json!([])]
        {
            assert!(!json_equal(&a, &other), "{other}");
        }
    }
}
