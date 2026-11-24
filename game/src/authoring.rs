//! The surface for tools built on this crate, such as the mission and mod editor. Everything here is a thin, documented wrapper over
//! the code the game itself runs (no second implementation), and none of it starts a window, reads a command line or touches the
//! game's own folders: a tool should call these instead of reaching into `lib.rs`, `modding` or `rules` internals, which may change.
//!
//! - [`base_assets`]: where the imported assets are, with no side effects.
//! - [`ENGINE_VERSION`]: the version of this engine, for "made with" stamps and compatibility checks.
//! - [`overlay_into`]: the merged asset tree of a base plus mods, built into a folder the tool chooses. The game's shared
//!   `local/overlay` is never used, so several tools and game instances can run side by side (#136).
//! - [`rule_schema`]: the mission rule vocabulary as JSON, the same document `--dump-rule-schema` writes.
//! - [`check_mission`] / [`known_names`]: the mission checker behind `--check-mission`, on JSON values in memory.
//!
//! Shared code: no UI and no view imports.

use std::path::{Path, PathBuf};

use serde_json::Value;

pub use crate::modding::{ModEntry, ModReport};
pub use crate::rules::check::{Level, Problem};

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
