//! `--rule-report <file>`: when a battle with mission rules ends, write what each rule did (`authoring::RuleReport`). The numbers are the
//! interpreter's own (`Script::rule_report`); this file decides when to write and where.
//!
//! A report is written when the rules end the mission (a few seconds later, so the end rules run), when the player leaves the
//! battle, when the game closes, and when an unattended autoplay run stops (`smoke::watch`). Each write replaces the file, so the last
//! state of the play stays.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use bevy::app::AppExit;
use bevy::prelude::*;

use super::*;
use crate::authoring::RuleReportOutcome;
use crate::menu::Screen;
use crate::rules::skirmish::SkirmishScript;
use crate::scenario::Outcome;

/// Seconds after the mission ends before its report is written (the same wait autoplay gives the end rules: score, speech).
const END_WAIT: f32 = 3.0;

static PATH: OnceLock<PathBuf> = OnceLock::new();

/// Set once by `run` from `--rule-report`; every battle afterwards writes its report there.
pub fn set_path(path: PathBuf) {
    let _ = PATH.set(path);
}

/// The file `--rule-report` names, from the command line: `Ok(None)` without the flag, an error message when it has no file after it.
pub fn path_arg(args: &[String]) -> Result<Option<PathBuf>, String> {
    let Some(i) = args.iter().position(|a| a == "--rule-report") else { return Ok(None) };
    match args.get(i + 1).filter(|a| !a.starts_with("--")) {
        Some(file) => Ok(Some(PathBuf::from(file))),
        None => Err("--rule-report takes the file to write the report to".into()),
    }
}

/// How the play ended: the rules' own end first, else the skirmish's decided outcome, else no end (a quit).
fn outcome_of(script: &Script, decided: Option<Outcome>) -> RuleReportOutcome {
    match (&script.ended, decided) {
        (Some((_, true)), _) | (None, Some(Outcome::Victory)) => RuleReportOutcome::Victory,
        (Some((_, false)), _) | (None, Some(Outcome::Defeat)) => RuleReportOutcome::Defeat,
        _ => RuleReportOutcome::Quit,
    }
}

/// Writes the report of `script` when `--rule-report` was given; a failed write is a warning, never an end of the run.
pub fn write(id: &str, script: &Script, decided: Option<Outcome>) {
    let Some(path) = PATH.get() else { return };
    write_to(path, &script.rule_report(id, outcome_of(script, decided)));
}

fn write_to(path: &Path, report: &crate::authoring::RuleReport) {
    let text = serde_json::to_string_pretty(report).expect("a report is JSON") + "\n";
    match crate::data::write_atomic(path, &text) {
        Ok(()) => info!("rule report written to {}", path.display()),
        Err(e) => warn!("cannot write the rule report to {}: {e}", path.display()),
    }
}

/// The report of whatever mission or skirmish script the world holds now.
pub fn write_world(world: &World) {
    if PATH.get().is_none() {
        return;
    }
    let decided = world.get_resource::<Outcome>().copied();
    if let Some(m) = world.get_resource::<Mission>() {
        write(&m.id, &m.script, decided);
    } else if let (Some(s), Some(cfg)) = (world.get_resource::<SkirmishScript>(), world.get_resource::<crate::Config>()) {
        write(cfg.map.as_deref().unwrap_or(""), &s.script, decided);
    }
}

/// The mission ended: wait `END_WAIT` seconds of play, then write once per ending.
fn when_ended(world: &World, mut since: Local<Option<f32>>) {
    let ended = world.get_resource::<Mission>().is_some_and(|m| m.script.ended.is_some())
        || world.get_resource::<Outcome>().is_some_and(|o| *o != Outcome::Playing) && world.contains_resource::<SkirmishScript>();
    if !ended {
        *since = None;
        return;
    }
    let dt = world.resource::<Time>().delta_secs();
    let waited = since.get_or_insert(0.0);
    let before = *waited;
    *waited += dt;
    if before < END_WAIT && *waited >= END_WAIT {
        write_world(world);
    }
}

/// The game closes during a battle.
fn on_exit(world: &World, mut exit: MessageReader<AppExit>) {
    if exit.read().next().is_some() {
        write_world(world);
    }
}

/// The player leaves the battle (to the menu); runs before the battle's resources are removed.
fn on_leave(world: &World) {
    write_world(world);
}

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, when_ended.run_if(in_state(Screen::Battle)))
        .add_systems(Last, on_exit)
        .add_systems(OnExit(Screen::Battle), on_leave.before(crate::scenario::reset_fleets));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_flag_names_the_file_and_needs_one() {
        assert_eq!(path_arg(&args(&["game", "--mission", "1"])), Ok(None));
        assert_eq!(path_arg(&args(&["game", "--rule-report", "out/r.json", "--mission", "1"])), Ok(Some(PathBuf::from("out/r.json"))));
        assert!(path_arg(&args(&["game", "--rule-report"])).is_err());
        assert!(path_arg(&args(&["game", "--rule-report", "--mission", "1"])).is_err(), "the next flag is not a file");
    }

    #[test]
    fn the_rules_end_decides_the_outcome_and_a_skirmish_falls_back_to_its_own() {
        let mut s = Script::new(&serde_json::json!({"rules": [], "ships": []}), serde_json::Value::Null);
        assert_eq!(outcome_of(&s, None), RuleReportOutcome::Quit);
        assert_eq!(outcome_of(&s, Some(Outcome::Playing)), RuleReportOutcome::Quit);
        assert_eq!(outcome_of(&s, Some(Outcome::Defeat)), RuleReportOutcome::Defeat);
        s.ended = Some(("won".into(), true));
        assert_eq!(outcome_of(&s, None), RuleReportOutcome::Victory);
        s.ended = Some(("lost".into(), false));
        assert_eq!(outcome_of(&s, Some(Outcome::Victory)), RuleReportOutcome::Defeat);
    }

    #[test]
    fn the_report_is_written_whole_to_the_named_file_and_parses_back() {
        let dir = std::env::temp_dir().join(format!("procyon-report-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("nested/report.json");
        let data = serde_json::json!({"rules": [{"name": "Always", "Run Once": false, "conditions": [], "actions": []}], "ships": []});
        let mut s = Script::new(&data, serde_json::Value::Null);
        s.step(&[], 1.0, false);
        let report = s.rule_report("m", RuleReportOutcome::Victory);
        write_to(&file, &report);
        let back: crate::authoring::RuleReport = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(back, report);
        assert_eq!((back.rules[0].runs, back.play_time), (1, 1.0));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
