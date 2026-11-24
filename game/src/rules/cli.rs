//! The mission-authoring command line flags that run without starting a battle (`docs/modding.md`, "Authoring missions"):
//! `--dump-rule-schema [path]` and `--check-mission <map id | all>`; and `--rule-trace` / `--why "<rule>"`, which only configure the
//! interpreter's trace for the battle that follows.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::check::{Level, Problem, check_with};
use super::{TraceOptions, schema, set_trace};

/// `--dump-rule-schema [path]`: writes the rule schema as JSON to `path` (stdout without one) and returns the exit status; `None`
/// when the flag is not given. Needs no game assets.
pub fn dump_schema(args: &[String]) -> Option<i32> {
    let i = args.iter().position(|a| a == "--dump-rule-schema")?;
    let json = serde_json::to_string_pretty(&schema::to_json()).expect("schema is JSON");
    match args.get(i + 1).filter(|a| !a.starts_with("--")) {
        Some(path) => match std::fs::write(path, json + "\n") {
            Ok(()) => {
                eprintln!("wrote {} rule Types to {path}", schema::RULE_TYPES.len());
                Some(0)
            }
            Err(e) => {
                eprintln!("cannot write {path}: {e}");
                Some(2)
            }
        },
        None => {
            // A closed pipe (`| head`) is not an error.
            let _ = writeln!(std::io::stdout(), "{json}");
            Some(0)
        }
    }
}

/// Reads the trace flags, then runs `--check-mission` if asked (returning the exit status). Call once the `--mod` overlay is set, so
/// a mod's missions are found like in a normal launch.
pub fn apply(args: &[String]) -> Option<i32> {
    set_trace(TraceOptions { all: args.iter().any(|a| a == "--rule-trace"), why: args.windows(2).filter(|w| w[0] == "--why").map(|w| w[1].clone()).collect() });
    let id = args.iter().position(|a| a == "--check-mission").map(|i| args.get(i + 1).filter(|a| !a.starts_with("--")))?;
    let Some(id) = id else {
        eprintln!("--check-mission takes a map id (the folder under maps/), or `all`");
        return Some(2);
    };
    let maps = crate::assets_dir().join("maps");
    let ids: Vec<String> = if id == "all" { all_ids(&maps) } else { vec![id.clone()] };
    let (mut errors, mut warnings) = (0, 0);
    for id in &ids {
        match check_map(&maps.join(id)) {
            Ok(problems) => {
                let out = &mut std::io::stdout();
                let file = rules_file(&maps.join(id)).and_then(|f| f.file_name()?.to_str().map(String::from)).unwrap_or_default();
                for p in &problems {
                    let _ = writeln!(out, "{id}/{file}: {p}");
                }
                let e = problems.iter().filter(|p| p.level == Level::Error).count();
                let _ = writeln!(out, "{id}: {e} errors, {} warnings", problems.len() - e);
                errors += e;
                warnings += problems.len() - e;
            }
            Err(message) => {
                eprintln!("{id}: {message}");
                return Some(2);
            }
        }
    }
    if ids.len() > 1 {
        println!("{} maps: {errors} errors, {warnings} warnings", ids.len());
    }
    Some(i32::from(errors > 0))
}

/// `check_map` for the log: one line per finding, prefixed with the file as `--check-mission` prints it, and a count line. A folder
/// that cannot be checked at all is one error line.
pub fn report_lines(id: &str, dir: &Path) -> Vec<String> {
    let file = rules_file(dir).and_then(|f| f.file_name()?.to_str().map(String::from)).unwrap_or_else(|| "mission.json".into());
    match check_map(dir) {
        Ok(problems) => {
            let errors = problems.iter().filter(|p| p.level == Level::Error).count();
            let mut lines: Vec<String> = problems.iter().map(|p| format!("{id}/{file}: {p}")).collect();
            lines.push(format!("{id}: {errors} errors, {} warnings", problems.len() - errors));
            lines
        }
        Err(message) => vec![format!("{id}: ERROR: {message}")],
    }
}

/// Every folder under `maps/` with rules (a campaign mission or a skirmish script), sorted.
fn all_ids(maps: &Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(maps)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| ["mission.json", "script.json"].iter().any(|f| e.path().join(f).is_file()))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    ids.sort();
    ids
}

/// A map folder's rules file: `mission.json`, or a skirmish map's `script.json`.
fn rules_file(dir: &Path) -> Option<PathBuf> {
    ["mission.json", "script.json"].iter().map(|f| dir.join(f)).find(|p| p.is_file())
}

/// Loads `map.json`, `mission.json` (or a skirmish map's `script.json`) and `media.json` of a map folder and checks its rules.
pub fn check_map(dir: &Path) -> Result<Vec<Problem>, String> {
    let rules_file = rules_file(dir).ok_or_else(|| format!("no mission.json or script.json in {}", dir.display()))?;
    let mission = crate::data::try_json(&rules_file).ok_or_else(|| format!("{} is not valid JSON", rules_file.display()))?;
    let (map, media) = (crate::data::try_json(&dir.join("map.json")), crate::data::try_json(&dir.join("media.json")));
    let skirmish = rules_file.ends_with("script.json");
    let mut problems = check_with(&mission, map.as_ref().unwrap_or(&Value::Null), media.as_ref().unwrap_or(&Value::Null), skirmish);
    let note = |level, field: &str, message: &str| Problem { level, rule: "(mission)".into(), at: "files".into(), field: field.into(), message: message.into() };
    // What the mission points at: skipped without imported assets, since there is nothing to look in.
    let root = crate::assets_dir();
    let ship = |c: &str| root.join("ships").join(c).join("ship.json").is_file();
    let sound = |f: &str| root.join(crate::audio::sound(f)).is_file();
    let picture = |f: &str| dir.join(f).is_file();
    let look = super::refs::Lookups { ship: root.join("ships").is_dir().then_some(&ship), sound: root.join("sound").is_dir().then_some(&sound), picture: Some(&picture) };
    problems.extend(super::refs::check_references(&mission, map.as_ref().unwrap_or(&Value::Null), media.as_ref().unwrap_or(&Value::Null), &look));
    if media.is_none() && rules_file.ends_with("mission.json") {
        problems.insert(0, note(Level::Warning, "media.json", "missing: speech events, end messages and ship names are not checked"));
    }
    if map.is_none() {
        problems.insert(0, note(Level::Error, "map.json", "missing or not valid JSON; the map cannot be played"));
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_map_folder_is_checked_with_its_files() {
        let dir = std::env::temp_dir().join(format!("procyon-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rules = json!({"players": [], "rules": [{"name": "R", "Run Once": true, "conditions": [{"Type": "World Initialize"}], "actions": [{"Type": "Nonsense"}]}]});
        std::fs::write(dir.join("mission.json"), rules.to_string()).unwrap();
        let lines: Vec<String> = check_map(&dir).unwrap().iter().map(ToString::to_string).collect();
        assert!(lines.iter().any(|l| l.starts_with("ERROR: rule \"R\": action #1 'Nonsense', field \"Type\": unknown action Type")), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("map.json") && l.starts_with("ERROR")), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("media.json") && l.starts_with("warning")), "{lines:?}");
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(check_map(&dir).is_err());
    }

    #[test]
    fn a_missing_backdrop_picture_is_an_error_in_the_map_check() {
        let dir = std::env::temp_dir().join(format!("procyon-backdrop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mission.json"), json!({"players": [], "rules": []}).to_string()).unwrap();
        std::fs::write(dir.join("map.json"), json!({"backdrop": true}).to_string()).unwrap();
        let lines: Vec<String> = check_map(&dir).unwrap().iter().map(ToString::to_string).collect();
        assert!(lines.iter().any(|l| l.starts_with("ERROR") && l.contains("field \"backdrop\"") && l.contains("backdrop.png")), "{lines:?}");
        std::fs::write(dir.join("backdrop.png"), b"x").unwrap();
        let lines: Vec<String> = check_map(&dir).unwrap().iter().map(ToString::to_string).collect();
        assert!(!lines.iter().any(|l| l.contains("backdrop.png")), "{lines:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Retail passes `--check-mission`: its own oddities are warnings, and every Type it uses is in the schema.
    #[test]
    #[ignore = "needs imported assets"]
    fn every_retail_map_checks_without_errors() {
        let maps = crate::assets_dir().join("maps");
        let ids = all_ids(&maps);
        assert!(ids.len() >= 29, "{ids:?}");
        for id in ids {
            let errors: Vec<String> = check_map(&maps.join(&id)).unwrap().iter().filter(|p| p.level == Level::Error).map(ToString::to_string).collect();
            assert!(errors.is_empty(), "{id}: {errors:#?}");
        }
    }

    #[test]
    fn the_schema_dump_needs_no_assets() {
        assert_eq!(dump_schema(&["game".into()]), None);
        let path = std::env::temp_dir().join(format!("procyon-schema-{}.json", std::process::id()));
        let args = ["game".to_string(), "--dump-rule-schema".into(), path.to_string_lossy().into_owned()];
        assert_eq!(dump_schema(&args), Some(0));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(v["types"].as_array().is_some_and(|t| t.iter().any(|t| t["name"] == "Group Destroyed" && t["role"] == "condition")));
    }
}
