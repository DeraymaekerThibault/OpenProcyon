//! Tests of the engine-added rule features (`eval/ext.rs`). They build synthetic missions and need no imported assets.

use super::*;
use bevy::prelude::*;
use serde_json::{Value, json};

fn script(data: Value, media: Value) -> Script {
    let mut data = data;
    data["ships"] = json!([]);
    Script::new(&data, media)
}

fn rule(name: &str, once: bool, conds: Value, acts: Value) -> Value {
    json!({"name": name, "Run Once": once, "Is Active": true, "conditions": conds, "actions": acts})
}

fn flag_is(name: &str, on: bool) -> Value {
    json!({"Type": "Flag Condition", "Flag Name": name, "Boolean Value": if on { "TRUE" } else { "FALSE" }})
}
fn set_flag(name: &str) -> Value {
    json!({"Type": "Set Flag Action", "Flag Name": name, "Boolean Value": "TRUE"})
}
fn add(name: &str, v: f64) -> Value {
    json!({"Type": "OP Add To Counter", "Counter Name": name, "Value": v})
}
fn wait(secs: f64) -> Value {
    json!({"Type": "OP Wait", "Seconds": secs})
}
fn counter_is(name: &str, eq: &str, v: f64) -> Value {
    json!({"Type": "OP Counter Condition", "Counter Name": name, "Equivalence": eq, "Value": v})
}

fn ship_at(owner: &str, x: f32, y: f32) -> Unit {
    let mut u = Unit::new(Entity::from_raw_u32(1).unwrap(), -1, Vec3::new(x, 0.0, -y));
    u.owner = owner.into();
    u
}
const SQUARE: [[f32; 2]; 4] = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];

#[test]
fn any_of_is_true_when_one_condition_is_and_not_inverts() {
    let any = json!({"Type": "OP Any Of", "Conditions": [flag_is("a", true), flag_is("b", true)]});
    let none = json!({"Type": "OP Not", "Condition": flag_is("a", true)});
    let data = json!({"rules": [rule("any", true, json!([any]), json!([set_flag("any")])), rule("not", false, json!([none]), json!([add("not", 1.0)]))]});
    let mut s = script(data, Value::Null);
    s.step(&[], 0.1, false);
    assert!(!s.flag("any"), "neither flag is set");
    assert_eq!(s.ext.counter("not"), 1.0, "a is not set, so Not holds");
    s.flags.insert("b".into(), true);
    s.step(&[], 0.1, false);
    assert!(s.flag("any"), "one of the two is enough");
    s.flags.insert("a".into(), true);
    s.step(&[], 0.1, false);
    assert_eq!(s.ext.counter("not"), 2.0, "Not stops firing once a is set");
}

#[test]
fn an_empty_any_of_is_false_and_a_not_without_a_condition_is_false() {
    let data = json!({"rules": [
        rule("any", true, json!([{"Type": "OP Any Of", "Conditions": []}]), json!([set_flag("any")])),
        rule("not", true, json!([{"Type": "OP Not"}]), json!([set_flag("not")])),
    ]});
    let mut s = script(data, Value::Null);
    s.step(&[], 0.1, false);
    assert!(!s.flag("any") && !s.flag("not"));
}

/// Enter Volume of a repeating rule is an edge. Inside Any Of every member is evaluated, so the edge state advances even while an
/// earlier member already made the group true.
#[test]
fn edge_conditions_inside_any_of_advance_their_state() {
    let enter = json!({"Type": "Enter Volume", "Group Name": "g", "Volume Name": "v"});
    let any = json!({"Type": "OP Any Of", "Conditions": [flag_is("early", true), enter]});
    let data = json!({"volumes": {"v": SQUARE}, "rules": [rule("r", false, json!([any]), json!([add("n", 1.0)]))]});
    let mut s = script(data, Value::Null);
    let inside = [ship_at("g", 5.0, 5.0)];
    s.flags.insert("early".into(), true);
    s.step(&inside, 0.1, false); // the flag holds; the ship is already inside, which the nested Enter Volume must remember
    assert_eq!(s.ext.counter("n"), 1.0);
    s.flags.insert("early".into(), false);
    s.step(&inside, 0.1, false);
    assert_eq!(s.ext.counter("n"), 1.0, "still inside is not entering");
    s.step(&[ship_at("g", 50.0, 50.0)], 0.1, false);
    s.step(&inside, 0.1, false);
    assert_eq!(s.ext.counter("n"), 2.0, "leaving and coming back is an entry");
}

/// Documented behaviour: Not inverts the frame's answer, so Not(Enter Volume) of a repeating rule is false only on the entry frame.
#[test]
fn not_of_an_edge_condition_is_false_only_on_the_edge_frame() {
    let enter = json!({"Type": "Enter Volume", "Group Name": "g", "Volume Name": "v"});
    let data = json!({"volumes": {"v": SQUARE}, "rules": [rule("r", false, json!([{"Type": "OP Not", "Condition": enter}]), json!([add("n", 1.0)]))]});
    let mut s = script(data, Value::Null);
    let (out, inside) = ([ship_at("g", 50.0, 50.0)], [ship_at("g", 5.0, 5.0)]);
    s.step(&out, 0.1, false);
    assert_eq!(s.ext.counter("n"), 1.0);
    s.step(&inside, 0.1, false); // the entry frame
    assert_eq!(s.ext.counter("n"), 1.0);
    s.step(&inside, 0.1, false);
    assert_eq!(s.ext.counter("n"), 2.0);
}

#[test]
fn counters_set_add_and_compare() {
    let data = json!({"rules": [
        rule("init", true, json!([{"Type": "World Initialize"}]), json!([{"Type": "OP Set Counter", "Counter Name": "k", "Value": 2}, add("k", 3.0)])),
        rule("five", true, json!([counter_is("k", "Equal To", 5.0)]), json!([set_flag("five")])),
        rule("more", true, json!([counter_is("k", "Greater Than", 5.0)]), json!([set_flag("more")])),
        rule("unset", true, json!([counter_is("never set", "Equal To", 0.0)]), json!([set_flag("unset")])),
    ]});
    let mut s = script(data, Value::Null);
    s.step(&[], 0.1, false);
    s.step(&[], 0.1, false);
    assert_eq!(s.ext.counter("k"), 5.0);
    assert!(s.flag("five") && s.flag("unset") && !s.flag("more"), "an unset counter reads 0");
}

#[test]
fn a_cutscene_is_one_rule_with_waits() {
    let data = json!({"environment": {"point_sets": {"p": [{"pos": [1.0, 2.0, 3.0]}]}}, "rules": [rule(
        "cutscene",
        true,
        json!([{"Type": "World Initialize"}]),
        json!([
            {"Type": "NIS Start"}, wait(1.0),
            {"Type": "Teleport Group/Unit", "Group/Unit Name": "g", "Point Set Name": "p"}, wait(1.0),
            {"Type": "NIS End"}, set_flag("done")
        ]),
    )]});
    let mut s = script(data, Value::Null);
    let w = [ship_at("g", 0.0, 0.0)];
    let mut teleports = 0;
    let mut seen = vec![];
    for _ in 0..30 {
        let (fx, _) = s.step(&w, 0.1, false);
        teleports += fx.iter().filter(|f| matches!(f, Fx::Teleport(..))).count();
        seen.push((s.nis, teleports, s.flag("done")));
    }
    assert_eq!(seen[0], (true, 0, false), "NIS Start runs at once, the teleport waits");
    assert_eq!(seen[5], (true, 0, false));
    assert_eq!(seen[12], (true, 1, false), "the teleport comes after the first wait");
    assert_eq!(seen[29], (false, 1, true), "NIS End after the second wait; nothing ran twice");
}

/// A repeating rule that fires again while its sequence waits: the firing is skipped (not queued, not overlapped).
#[test]
fn a_repeating_rule_does_not_restart_while_its_sequence_waits() {
    let data = json!({"rules": [rule("r", false, json!([flag_is("go", true)]), json!([add("start", 1.0), wait(1.0), add("end", 1.0)]))]});
    let mut s = script(data, Value::Null);
    s.flags.insert("go".into(), true);
    for _ in 0..10 {
        s.step(&[], 0.1, false);
    }
    assert_eq!((s.ext.counter("start"), s.ext.counter("end")), (1.0, 0.0), "pending: no second start");
    s.step(&[], 0.1, false);
    s.step(&[], 0.1, false);
    assert_eq!(s.ext.counter("end"), 1.0, "the sequence finishes");
    assert_eq!(s.ext.counter("start"), 2.0, "and the still-true condition starts the next run");
    s.flags.insert("go".into(), false);
    for _ in 0..20 {
        s.step(&[], 0.1, false);
    }
    assert_eq!((s.ext.counter("start"), s.ext.counter("end")), (2.0, 2.0), "a started sequence always finishes");
}

#[test]
fn waits_stand_still_during_the_opening_briefing_pause() {
    let data = json!({"rules": [rule("r", true, json!([]), json!([wait(1.0), set_flag("after")]))]});
    let mut s = script(data, Value::Null);
    for _ in 0..30 {
        s.step(&[], 0.1, true);
    }
    assert!(!s.flag("after"), "paused");
    for _ in 0..9 {
        s.step(&[], 0.1, false);
    }
    assert!(!s.flag("after"));
    s.step(&[], 0.2, false);
    assert!(s.flag("after"));
}

#[test]
fn waiting_for_a_speech_event_or_for_the_whole_queue() {
    let media = json!({"speech": {"A": {"text": "x", "duration": 2.0}, "B": {"text": "y", "duration": 1.0}}});
    let data = json!({"rules": [
        rule("one", true, json!([{"Type": "World Initialize"}]), json!([
            {"Type": "Play speech event", "Speech Event Name": "A"},
            {"Type": "Play speech event", "Speech Event Name": "B"},
            {"Type": "OP Wait Until Speech Done", "Speech Event Name": "A"}, set_flag("a done")])),
        rule("all", true, json!([{"Type": "World Initialize"}]), json!([
            {"Type": "OP Wait Until Speech Done", "Speech Event Name": ""}, set_flag("queue empty")])),
    ]});
    let mut s = script(data, media);
    let at = |s: &mut Script, flag: &str| {
        for k in 0..100 {
            s.step(&[], 0.1, false);
            if s.flag(flag) {
                return Some(k);
            }
        }
        None
    };
    let a = at(&mut s, "a done").expect("A finishes");
    assert!((20..=26).contains(&a), "A lasts about 2.3 s, took {a} steps");
    assert!(s.played.contains("A") && !s.played.contains("B"), "B is still to be played");
    assert!(!s.flag("queue empty"));
    at(&mut s, "queue empty").expect("the queue empties");
    assert!(s.played.contains("B"));
}

#[test]
fn a_wait_for_a_line_that_is_not_pending_ends_at_once() {
    let data = json!({"rules": [rule("r", true, json!([]), json!([{"Type": "OP Wait Until Speech Done", "Speech Event Name": "Nobody"}, set_flag("go")]))]});
    let mut s = script(data, Value::Null);
    s.step(&[], 0.1, false);
    assert!(s.flag("go"));
}

#[test]
fn inactive_rules_are_not_evaluated_until_a_rule_activates_them() {
    let mut off = rule("sleeper", true, json!([]), json!([set_flag("woke")]));
    off["Is Active"] = json!(false);
    let data = json!({"rules": [
        off,
        rule("later", true, json!([flag_is("wake", true)]), json!([{"Type": "OP Set Rule Active", "Rule Name": "sleeper", "Boolean": "TRUE"}])),
    ]});
    let mut s = script(data, Value::Null);
    for _ in 0..3 {
        s.step(&[], 0.1, false);
    }
    assert!(!s.flag("woke") && s.runs[0].count == 0, "inactive");
    s.flags.insert("wake".into(), true);
    s.step(&[], 0.1, false);
    s.step(&[], 0.1, false);
    assert!(s.flag("woke"), "activated");
    s.act(&json!({"Type": "OP Set Rule Active", "Rule Name": "sleeper", "Boolean": "FALSE"}), &[], &mut vec![], "t");
    assert!(!s.ext.is_active(0), "retail-style FALSE");
}

#[test]
fn a_rule_without_is_active_is_active_and_unknown_op_types_are_ignored() {
    let data = json!({"rules": [{"name": "plain", "conditions": [], "actions": [{"Type": "OP Nonsense"}, set_flag("ran")]}]});
    let mut s = script(data, Value::Null);
    s.step(&[], 0.1, false);
    assert!(s.flag("ran"));
}

fn tasks_data(victory: bool) -> Value {
    let mut d = json!({"rules": [], "tasks": [
        {"Name": "t1", "text": "one", "Active": true, "required": true, "complete_when": [flag_is("one", true)], "fail_when": [flag_is("lost", true)]},
        {"Name": "t2", "text": "two", "Active": true, "required": true, "complete_when": [flag_is("two", true)]},
        {"Name": "bonus", "text": "bonus", "Active": true, "complete_when": [flag_is("bonus", true)]},
        {"Name": "plain", "text": "plain", "Active": true},
    ]});
    if victory {
        d["victory"] = json!("objectives");
    }
    d
}

#[test]
fn tasks_complete_and_fail_themselves_once_active() {
    let mut s = script(tasks_data(false), Value::Null);
    s.step(&[], 0.1, false);
    assert!(!s.task_complete("t1"));
    s.flags.insert("one".into(), true);
    s.flags.insert("bonus".into(), true);
    s.step(&[], 0.1, false);
    assert!(s.task_complete("t1") && s.task_complete("bonus") && !s.task_complete("plain"), "tasks without keys behave as before");
    assert!(s.ended.is_none(), "no victory key: the rules decide");
    s.flags.insert("lost".into(), true);
    s.step(&[], 0.1, false);
    assert!(s.task_complete("t1") && !s.tasks[0].failed, "a decided task stays decided");
}

#[test]
fn an_inactive_task_does_not_complete_until_it_is_activated() {
    let mut d = tasks_data(false);
    d["tasks"][0]["Active"] = json!(false);
    let mut s = script(d, Value::Null);
    s.flags.insert("one".into(), true);
    s.step(&[], 0.1, false);
    assert!(!s.task_complete("t1"));
    s.act(&json!({"Type": "Set Objective Task Active State", "Objective Task": "t1", "Active State": "TRUE"}), &[], &mut vec![], "t");
    s.step(&[], 0.1, false);
    assert!(s.task_complete("t1"));
}

#[test]
fn objectives_victory_wins_when_every_required_task_is_complete_and_loses_when_one_fails() {
    let mut s = script(tasks_data(true), Value::Null);
    s.flags.insert("one".into(), true);
    s.step(&[], 0.1, false);
    assert!(s.ended.is_none(), "t2 is still open; the bonus task does not matter");
    s.flags.insert("two".into(), true);
    s.step(&[], 0.1, false);
    assert_eq!(s.ended.as_ref().map(|e| e.1), Some(true));

    let mut s = script(tasks_data(true), Value::Null);
    s.flags.insert("lost".into(), true);
    s.flags.insert("two".into(), true);
    s.step(&[], 0.1, false);
    assert_eq!(s.ended.as_ref().map(|e| e.1), Some(false), "a failed required task loses even when the others are done");
}

#[test]
fn objectives_victory_needs_a_required_task() {
    let mut d = tasks_data(true);
    for t in d["tasks"].as_array_mut().unwrap() {
        t["required"] = json!(false);
    }
    let mut s = script(d, Value::Null);
    s.flags.insert("one".into(), true);
    s.flags.insert("two".into(), true);
    s.step(&[], 0.1, false);
    assert!(s.ended.is_none());
}

#[test]
fn deactivating_a_rule_stops_its_waiting_sequence_and_a_rule_may_switch_itself_off() {
    let data = json!({"rules": [
        rule("cutscene", true, json!([]), json!([add("a", 1.0), wait(1.0), add("b", 1.0)])),
        rule("stop", true, json!([flag_is("stop", true)]), json!([{"Type": "OP Set Rule Active", "Rule Name": "cutscene", "Boolean": "FALSE"}])),
        rule("self", true, json!([]), json!([add("x", 1.0), {"Type": "OP Set Rule Active", "Rule Name": "self", "Boolean": "FALSE"}, add("y", 1.0)])),
    ]});
    let mut s = script(data, Value::Null);
    s.step(&[], 0.1, false);
    s.flags.insert("stop".into(), true);
    for _ in 0..20 {
        s.step(&[], 0.1, false);
    }
    assert_eq!((s.ext.counter("a"), s.ext.counter("b")), (1.0, 0.0), "the cutscene stopped at its wait");
    assert_eq!((s.ext.counter("x"), s.ext.counter("y")), (1.0, 0.0), "a rule that switches itself off drops the rest of its actions");
}

#[test]
fn objectives_victory_reads_its_messages_from_the_media() {
    let mut d = tasks_data(true);
    d["victory_message"] = json!("won");
    d["defeat_message"] = json!("lost");
    let media = json!({"messages": {"won": "Well sailed.", "lost": "The Comet is gone."}});
    let mut s = script(d.clone(), media.clone());
    s.flags.insert("one".into(), true);
    s.flags.insert("two".into(), true);
    s.step(&[], 0.1, false);
    assert_eq!(s.ended, Some(("Well sailed.".into(), true)));
    let mut s = script(d, media);
    s.flags.insert("lost".into(), true);
    s.step(&[], 0.1, false);
    assert_eq!(s.ended, Some(("The Comet is gone.".into(), false)));
}

/// A mission that uses every engine-added Type, key and field: it must pass `--check-mission` with no error and no warning, and run.
fn fixture() -> (Value, Value) {
    let mission = json!({
        "players": ["Player0 Group"],
        "groups": {"Fleet": [1]},
        "ships": [{"id": 1, "name": "RLS Comet", "group": "Player0 Group"}],
        "volumes": {"Area": [[0, 0], [10, 0], [10, 10], [0, 10]]},
        "environment": {"point_sets": {"Spot": [{"pos": [1.0, 2.0, 3.0]}]}},
        "victory": "objectives",
        "victory_message": "won",
        "defeat_message": "lost",
        "tasks": [
            {"Name": "Reach", "Active": true, "required": true,
             "complete_when": [{"Type": "OP Any Of", "Conditions": [counter_is("kills", "Greater Than", 2.0), flag_is("shortcut", true)]}],
             "fail_when": [{"Type": "Group Destroyed", "Group Name": "Fleet"}]},
            {"Name": "Bonus", "Active": true, "required": false, "complete_when": [flag_is("bonus", true)]},
        ],
        "rules": [
            rule("Start", true, json!([{"Type": "World Initialize"}]), json!([
                {"Type": "OP Set Counter", "Counter Name": "kills", "Value": 0}, set_flag("go"), set_flag("shortcut"), set_flag("bonus")])),
            rule("Cutscene", true, json!([{"Type": "OP Any Of", "Conditions": [flag_is("go", true), {"Type": "OP Not", "Condition": counter_is("kills", "Less Than", 1.0)}]}]), json!([
                {"Type": "NIS Start"}, wait(1.0),
                {"Type": "Teleport Group/Unit", "Group/Unit Name": "Fleet", "Point Set Name": "Spot"},
                {"Type": "Play speech event", "Speech Event Name": "Hello"},
                {"Type": "OP Wait Until Speech Done", "Speech Event Name": "Hello"},
                {"Type": "OP Wait Until Speech Done", "Speech Event Name": ""},
                {"Type": "NIS End"}, add("kills", 3.0),
                {"Type": "OP Set Rule Active", "Rule Name": "Later", "Boolean": "TRUE"}])),
            {"name": "Later", "Run Once": true, "Is Active": false, "conditions": [counter_is("kills", "Greater Than", 2.0)], "actions": [set_flag("later")]},
        ],
    });
    let media = json!({"speech": {"Hello": {"text": "Hi", "duration": 1.0}}, "messages": {"won": "Well sailed.", "lost": "Lost."}});
    (mission, media)
}

#[test]
fn the_fixture_mission_passes_check_mission_and_runs() {
    let (mission, media) = fixture();
    let problems = crate::rules::check::check(&mission, &Value::Null, &media);
    assert_eq!(problems, vec![], "{}", problems.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"));
    let mut s = Script::new(&mission, media);
    let w = [ship_at("Fleet", 0.0, 0.0)];
    for _ in 0..120 {
        s.step(&w, 0.1, false);
    }
    assert!(s.flag("later"), "the cutscene ran to its end and switched the later rule on");
    assert_eq!(s.ended, Some(("Well sailed.".into(), true)), "the required task completed through its counter");
}

#[test]
fn check_mission_reports_mistakes_in_the_new_features() {
    let (mut mission, media) = fixture();
    mission["rules"][1]["conditions"][0]["Conditions"][0] = json!({"Type": "Flag Condition", "Flag Name": "typo", "Boolean Value": "TRUE"});
    mission["rules"][2]["conditions"] = json!([counter_is("never written", "Greater Than", 0.0)]);
    mission["rules"][0]["actions"][0] = json!({"Type": "OP Set Counter", "Counter Name": "kills", "Value": 0});
    mission["rules"][0]["actions"][1] = json!({"Type": "OP Set Rule Active", "Rule Name": "Lter", "Boolean": true});
    mission["tasks"][0]["complete_when"] = json!([{"Type": "Flag Condiction"}]);
    mission["tasks"][1]["required"] = json!("yes");
    mission["victory"] = json!("objectves");
    mission["victory_message"] = json!(["won"]);
    let text = crate::rules::check::check(&mission, &Value::Null, &media).iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
    for want in [
        "counter 'never written' is never set (it reads 0), so this can never hold",
        "unknown rule 'Lter' (did you mean 'Later'?)",
        "field \"Boolean\": the JSON boolean true is not allowed",
        "unknown condition Type (did you mean 'Flag Condition'?)",
        "\"yes\" is not true/false",
        "\"objectves\" is not \"objectives\"",
        "[\"won\"] is not a message id",
    ] {
        assert!(text.contains(want), "missing {want:?} in\n{text}");
    }
    assert!(text.contains("warning: rule \"Cutscene\": condition #1 'OP Any Of', nested condition #1 'Flag Condition'"), "a nested name that is never set is a warning: {text}");
}

#[test]
fn a_sunk_ship_removed_from_the_world_still_counts_as_destroyed() {
    let (mission, media) = fixture();
    let mut s = Script::new(&mission, media);
    let cond = json!({"Type": "Group Destroyed", "Group Name": "Fleet"});
    let mut dead = ship_at("Fleet", 0.0, 0.0);
    dead.alive = false;
    // The frame it is destroyed, it is in the world.
    let mut w = vec![dead];
    s.keep_sunk(&mut w);
    assert_eq!(w.len(), 1);
    // After its removal the world has no such unit; the rules still see it destroyed, once.
    let mut w = vec![];
    s.keep_sunk(&mut w);
    assert_eq!(w.len(), 1);
    assert!(!w[0].alive);
    assert!(s.cond(&cond, &w, false), "'Group Destroyed' holds for a group whose ships are all gone");
    let mut again = w.clone();
    s.keep_sunk(&mut again);
    assert_eq!(again.len(), 1, "a unit present in the world is not added twice");
}

// ---- skirmish scripts that decide their own game (`"victory": "script"`) -------------------------------------------------------

fn deciding(data: Value) -> Script {
    let mut s = script(data, Value::Null);
    s.decide_game(true);
    s
}

#[test]
fn an_end_rule_sets_the_winner_and_the_messages_only_for_a_script_that_decides() {
    let data = json!({"rules": [rule("win", true, json!([]), json!([
        {"Type": "Team X Wins", "Team Name": "NAVY"},
        {"Type": "End Game", "Winner - Custom Message String ID": "IDGS_WON", "Loser - Custom Message String ID": "GAME STRING"},
    ]))]});
    let mut s = deciding(data.clone());
    assert_eq!(s.end, None);
    s.step(&[], 0.1, false);
    assert_eq!(s.end, Some(ScriptEnd { kind: EndKind::Team("NAVY".into()), winner_message: Some("IDGS_WON".into()), loser_message: None }));
    let mut plain = script(data, Value::Null);
    plain.step(&[], 0.1, false);
    assert_eq!(plain.end, None, "a script that does not opt in leaves the ending to victory.rs");
}

#[test]
fn remaining_team_wins_and_a_bare_end_game_leave_the_winner_open_and_the_first_rule_wins() {
    let data = json!({"rules": [
        rule("one", true, json!([]), json!([{"Type": "Remaining Team Wins"}])),
        rule("two", true, json!([]), json!([{"Type": "Team X Wins", "Team Name": "PIRATES"}])),
    ]});
    let mut s = deciding(data);
    s.step(&[], 0.1, false);
    assert_eq!(s.end.as_ref().map(|e| &e.kind), Some(&EndKind::Winner), "the second rule fired in the same step but speaks second");
    s.step(&[], 0.1, false);
    assert_eq!(s.end.as_ref().map(|e| &e.kind), Some(&EndKind::Winner));
}

#[test]
fn a_required_task_decides_for_its_team() {
    let task = |name: &str, team: &str| json!({"Name": name, "Active": true, "required": true, "team": team, "complete_when": [flag_is(&format!("{name}-done"), true)], "fail_when": [flag_is(&format!("{name}-lost"), true)]});
    let data = json!({"rules": [], "victory": "script", "tasks": [task("A", "NAVY"), task("B", "NAVY"), task("C", "PIRATES")]});
    let mut s = deciding(data);
    s.step(&[], 0.1, false);
    assert_eq!(s.end, None);
    s.flags.insert("A-done".into(), true);
    s.step(&[], 0.1, false);
    assert_eq!(s.end, None, "NAVY still has B to do");
    s.flags.insert("B-done".into(), true);
    s.step(&[], 0.1, false);
    assert_eq!(s.end.as_ref().map(|e| &e.kind), Some(&EndKind::Team("NAVY".into())));
    let mut lost = deciding(json!({"rules": [], "tasks": [task("C", "PIRATES")]}));
    lost.flags.insert("C-lost".into(), true);
    lost.step(&[], 0.1, false);
    assert_eq!(lost.end.as_ref().map(|e| &e.kind), Some(&EndKind::Loser("PIRATES".into())));
}

#[test]
fn a_required_task_without_a_team_never_decides_a_skirmish() {
    let t = json!({"Name": "A", "Active": true, "required": true, "complete_when": [flag_is("done", true)]});
    let mut s = deciding(json!({"rules": [], "victory": "objectives", "tasks": [t]}));
    s.flags.insert("done".into(), true);
    s.step(&[], 0.1, false);
    assert_eq!((s.end.clone(), s.ended.clone()), (None, None));
}

// ---- Orders and conditions that act on ships: OP Board Ship, OP Abandon Ship, OP Teleport, OP Follow Group, OP Place Mines,
// ---- OP Etherium Current, the campaign counters, OP Group Is Cloaked, OP Group Speed, OP Task State.

fn unit(n: u32, owner: &str, x: f32, y: f32) -> Unit {
    let mut u = Unit::new(e(n), -1, Vec3::new(x, 0.0, -y));
    u.owner = owner.into();
    u.speed = Some(0.0);
    u
}
fn e(n: u32) -> Entity {
    Entity::from_raw_u32(n).unwrap()
}
/// Runs one world-initialise rule whose only action is `action` and returns what the interpreter asked the world to do.
fn effects(action: Value, extra: Value, w: &[Unit]) -> (Script, Vec<Fx>) {
    let mut data = json!({"rules": [rule("go", true, json!([{"Type": "World Initialize"}]), json!([action]))]});
    for (k, v) in extra.as_object().into_iter().flatten() {
        data[k] = v.clone();
    }
    let mut s = script(data, Value::Null);
    let (fx, _) = s.step(w, 0.1, false);
    (s, fx)
}
/// Whether a rule with the condition `cond` fires on the world `w`.
fn holds(cond: Value, w: &[Unit]) -> bool {
    let mut s = script(json!({"rules": [rule("r", true, json!([cond]), json!([set_flag("yes")]))]}), Value::Null);
    s.step(w, 0.1, false);
    s.flag("yes")
}

#[test]
fn board_ship_hands_the_boarders_and_the_target_group_to_the_world() {
    let w = [unit(1, "Raiders", 0.0, 0.0), unit(2, "Raiders", 5.0, 0.0), unit(3, "Merchant", 50.0, 0.0), unit(4, "Merchant", 90.0, 0.0)];
    let (_, fx) = effects(json!({"Type": "OP Board Ship", "Group Name": "Raiders", "Target Group": "Merchant"}), json!({}), &w);
    assert!(matches!(fx.as_slice(), [Fx::Board(b, t)] if b == &[e(1), e(2)] && t == &[e(3), e(4)]), "{}", fx.len());
}

#[test]
fn abandon_ship_names_the_group_and_the_hull_stays_alive_for_the_rules() {
    let w = [unit(1, "Hulk", 0.0, 0.0), unit(2, "Other", 5.0, 0.0)];
    let (_, fx) = effects(json!({"Type": "OP Abandon Ship", "Group Name": "Hulk"}), json!({}), &w);
    assert!(matches!(fx.as_slice(), [Fx::Abandon(u)] if u == &[e(1)]));
    // The unit the world reports stays alive, so `Group Destroyed` is false for the abandoned hull.
    assert!(!holds(json!({"Type": "Group Destroyed", "Group Name": "Hulk"}), &w));
    let mut sunk = unit(1, "Hulk", 0.0, 0.0);
    sunk.alive = false;
    assert!(holds(json!({"Type": "Group Destroyed", "Group Name": "Hulk"}), &[sunk]));
}

#[test]
fn teleport_keeps_the_layout_of_the_group_and_turns_it_with_a_heading() {
    let points = json!({"environment": {"point_sets": {"harbour": [{"pos": [0.0, 0.0, 0.0]}, {"pos": [100.0, 200.0, 0.0]}]}}});
    // Two ships 20 apart along x, the group's centre at (10, 0).
    let mut w = [unit(1, "Fleet", 0.0, 0.0), unit(2, "Fleet", 20.0, 0.0)];
    w[0].heading = 0.3;
    let place = |action: Value, w: &[Unit]| match effects(action, points.clone(), w).1.as_slice() {
        [Fx::Place { units, heading }] => (units.clone(), *heading),
        other => panic!("expected one Place, got {} effects", other.len()),
    };
    let (units, heading) = place(json!({"Type": "OP Teleport", "Group/Unit Name": "Fleet", "Point Set Name": "harbour", "Point Index": 1}), &w);
    assert_eq!(heading, None, "no Heading: the ships keep theirs");
    // Game (100, 200) is Bevy (100, 0, -200); the ships keep their 20 units between them, centred on the point.
    assert_eq!(units, vec![(e(1), Vec3::new(90.0, 0.0, -200.0)), (e(2), Vec3::new(110.0, 0.0, -200.0))]);
    // A heading of 90 degrees (toward +x, a compass bearing) turns the layout with the first ship: from 0.3 rad to -pi/2.
    let (units, heading) = place(json!({"Type": "OP Teleport", "Group/Unit Name": "Fleet", "Point Set Name": "harbour", "Heading": 90.0}), &w);
    assert_eq!(heading, Some(-std::f32::consts::FRAC_PI_2), "game axes: bearing 90 is toward +x, which is heading -pi/2 about +Y");
    let turn = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2 - 0.3);
    assert!(units[0].1.distance(turn * Vec3::new(-10.0, 0.0, 0.0)) < 1e-4 && units[1].1.distance(turn * Vec3::new(10.0, 0.0, 0.0)) < 1e-4, "{units:?}");
    // The default index is the first point and a lone ship goes exactly there.
    let mut lone = unit(3, "Fleet", 7.0, 7.0);
    lone.name = "Lone".into();
    let (units, _) = place(json!({"Type": "OP Teleport", "Group/Unit Name": "Fleet,Lone", "Point Set Name": "harbour"}), &[lone]);
    assert_eq!(units, vec![(e(3), Vec3::ZERO)]);
}

#[test]
fn teleport_to_a_point_that_is_not_there_does_nothing_and_warns_once() {
    let points = json!({"environment": {"point_sets": {"harbour": [{"pos": [0.0, 0.0, 0.0]}]}}});
    let (s, fx) = effects(json!({"Type": "OP Teleport", "Group/Unit Name": "Fleet", "Point Set Name": "harbour", "Point Index": 3}), points, &[unit(1, "Fleet", 0.0, 0.0)]);
    assert!(fx.is_empty());
    assert!(s.warned.iter().any(|w| w.contains("point 3 of point set 'harbour'")), "{:?}", s.warned);
}

#[test]
fn the_retail_teleport_still_sends_every_member_to_the_first_point() {
    let points = json!({"environment": {"point_sets": {"p": [{"pos": [1.0, 2.0, 0.0]}]}}});
    let w = [unit(1, "Fleet", 0.0, 0.0), unit(2, "Fleet", 20.0, 0.0)];
    let (_, fx) = effects(json!({"Type": "Teleport Group/Unit", "Group/Unit Name": "Fleet", "Point Set Name": "p"}), points, &w);
    assert!(matches!(fx.as_slice(), [Fx::Teleport(u, p)] if u == &[e(1), e(2)] && *p == Vec3::new(1.0, 0.0, -2.0)));
}

#[test]
fn follow_group_names_the_station_and_a_wrong_station_is_ignored_with_a_warning() {
    let w = [unit(1, "Escort", 0.0, 0.0), unit(2, "Convoy", 50.0, 0.0)];
    let follow = |station: &str| json!({"Type": "OP Follow Group", "Group Name": "Escort", "Target Group": "Convoy", "Station": station, "Distance": 40});
    let (_, fx) = effects(follow("Abeam Port"), json!({}), &w);
    assert!(matches!(fx.as_slice(), [Fx::Follow { units, group, station: Station::AbeamPort, distance }] if units == &[e(1)] && group == "Convoy" && *distance == 40.0));
    let (s, fx) = effects(follow("Sideways"), json!({}), &w);
    assert!(fx.is_empty() && s.warned.iter().any(|w| w.contains("station 'Sideways'")));
    // Stations in the leader's frame: bow is -Z, starboard +X.
    assert_eq!(Station::Astern.offset(40.0), Vec3::new(0.0, 0.0, 40.0));
    assert_eq!(Station::Ahead.offset(40.0), Vec3::new(0.0, 0.0, -40.0));
    assert_eq!(Station::AbeamStarboard.offset(40.0), Vec3::new(40.0, 0.0, 0.0));
    assert_eq!(Station::AbeamPort.offset(40.0), Vec3::new(-40.0, 0.0, 0.0));
}

#[test]
fn group_is_cloaked_wants_every_live_member_fully_cloaked() {
    let cloaked = |n, x, c: bool| {
        let mut u = unit(n, "Sub", x, 0.0);
        u.cloaked = c;
        u
    };
    let on = json!({"Type": "OP Group Is Cloaked", "Group Name": "Sub"});
    let off = json!({"Type": "OP Group Is Cloaked", "Group Name": "Sub", "Boolean": "FALSE"});
    assert!(holds(on.clone(), &[cloaked(1, 0.0, true), cloaked(2, 5.0, true)]));
    assert!(!holds(on.clone(), &[cloaked(1, 0.0, true), cloaked(2, 5.0, false)]), "one member shows");
    assert!(holds(off.clone(), &[cloaked(1, 0.0, false), cloaked(2, 5.0, false)]));
    assert!(!holds(off, &[cloaked(1, 0.0, true), cloaked(2, 5.0, false)]));
    let mut dead = cloaked(2, 5.0, false);
    dead.alive = false;
    assert!(holds(on.clone(), &[cloaked(1, 0.0, true), dead]), "a destroyed member does not count");
    assert!(!holds(on, &[]), "no live member: neither answer holds");
}

#[test]
fn group_speed_compares_the_fastest_or_slowest_live_ship() {
    let moving = |n, v: f32| {
        let mut u = unit(n, "Convoy", 0.0, 0.0);
        u.speed = Some(v);
        u
    };
    let speed = |which: &str, eq: &str, v: f64, w: &[Unit]| holds(json!({"Type": "OP Group Speed", "Group Name": "Convoy", "Which": which, "Equivalence": eq, "Speed": v}), w);
    let w = [moving(1, 0.2), moving(2, 4.0)];
    assert!(speed("Fastest", "Greater Than", 3.0, &w) && !speed("Slowest", "Greater Than", 3.0, &w));
    assert!(speed("Slowest", "Less Than", 0.5, &w) && !speed("Fastest", "Less Than", 0.5, &w));
    assert!(speed("Fastest", "Less Than", 0.5, &[moving(1, 0.2), moving(2, 0.1)]), "stopped alongside: even the fastest is slow");
    // What is not a ship has no speed; a group of such things never holds.
    let mut rock = unit(3, "Convoy", 0.0, 0.0);
    rock.speed = None;
    assert!(!speed("Fastest", "Less Than", 99.0, &[rock]));
    assert!(!speed("Fastest", "Less Than", 99.0, &[]));
}

#[test]
fn task_state_reads_what_the_rules_set() {
    let cond = |task: &str, state: &str| json!({"Type": "OP Task State", "Objective Task": task, "State": state});
    let mut data = json!({"rules": [
        rule("done", true, json!([flag_is("go", true)]), json!([{"Type": "Set Objective Task Complete State", "Objective Task": "Escort", "Complete State": "TRUE"}])),
        rule("failed", true, json!([flag_is("go", true)]), json!([{"Type": "Set Objective Task Failed State", "Objective Task": "Later", "Failed State": "TRUE"}])),
        rule("active", false, json!([cond("Escort", "Active")]), json!([add("active", 1.0)])),
        rule("complete", false, json!([cond("Escort", "Complete")]), json!([add("complete", 1.0)])),
        rule("failed seen", false, json!([cond("Later", "Failed")]), json!([add("failed", 1.0)])),
        rule("not active", false, json!([cond("Later", "Active")]), json!([add("later active", 1.0)])),
        rule("unknown", false, json!([cond("Nope", "Active")]), json!([add("unknown", 1.0)])),
    ]});
    data["tasks"] = json!([{"Name": "Escort", "Active": true}, {"Name": "Later", "Active": true}]);
    let mut s = script(data, Value::Null);
    s.step(&[], 0.1, false);
    assert_eq!((s.ext.counter("active"), s.ext.counter("complete"), s.ext.counter("later active")), (1.0, 0.0, 1.0), "switched on, not decided");
    s.flags.insert("go".into(), true);
    s.step(&[], 0.1, false);
    assert_eq!((s.ext.counter("complete"), s.ext.counter("failed")), (1.0, 1.0));
    assert_eq!((s.ext.counter("active"), s.ext.counter("later active")), (1.0, 1.0), "a decided task is no longer active");
    assert_eq!(s.ext.counter("unknown"), 0.0, "a task the mission does not have is in no state");
}

#[test]
fn place_mines_and_the_etherium_current_hand_their_settings_to_the_world() {
    let extra =
        json!({"environment": {"point_sets": {"field": [{"pos": [10.0, 20.0, 0.0]}, {"pos": [30.0, 40.0, 0.0]}]}}, "paths": {"lane": [[0.0, 0.0, 0.0], [0.0, 100.0, 0.0]]}});
    let (_, fx) = effects(json!({"Type": "OP Place Mines", "Point Set Name": "field", "Player/Owner": "Pirates"}), extra.clone(), &[]);
    assert!(matches!(fx.as_slice(), [Fx::Mines { set, bullet, owner }] if set == "field" && bullet == "Bullet_StarMine_Med" && owner == "Pirates"), "the default mine");
    let (_, fx) = effects(json!({"Type": "OP Etherium Current", "Etherium Path": "lane", "Strength": 3.0, "Boolean": "FALSE"}), extra.clone(), &[]);
    match fx.as_slice() {
        [Fx::CurrentSet { name, path, strength: Some(s), width: None, on: Some(false) }] => {
            assert_eq!((name.as_str(), *s, path.len()), ("lane", 3.0, 2));
        }
        other => panic!("{} effects", other.len()),
    }
    let (s, fx) = effects(json!({"Type": "OP Etherium Current", "Etherium Path": "nowhere"}), extra, &[]);
    assert!(fx.is_empty() && s.warned.iter().any(|w| w.contains("path 'nowhere'")));
}

#[test]
fn campaign_counters_are_loaded_changed_compared_and_handed_on_once_when_the_mission_is_won() {
    let data = json!({"rules": [
        rule("start", true, json!([{"Type": "World Initialize"}]), json!([
            {"Type": "OP Add To Campaign Counter", "Counter Name": "rescued", "Value": 2}, {"Type": "OP Set Campaign Counter", "Counter Name": "grudge", "Value": 5}])),
        rule("seen", true, json!([{"Type": "OP Campaign Counter Condition", "Counter Name": "rescued", "Equivalence": "Equal To", "Value": 3}]), json!([set_flag("three")])),
        rule("win", true, json!([flag_is("three", true)]), json!([{"Type": "Goto Next Level", "World": "Mission02.twt"}])),
    ]});
    let mut s = script(data, Value::Null);
    s.set_campaign_values([("rescued".to_string(), 1.0)].into());
    assert!(s.campaign_commit().is_none(), "nothing is handed on before the mission is won");
    for _ in 0..4 {
        s.step(&[], 0.1, false);
    }
    assert!(s.flag("three"), "the value of the earlier mission (1) plus this mission's 2");
    let on = s.campaign_commit().expect("won");
    assert_eq!((on.get("rescued"), on.get("grudge")), (Some(&3.0), Some(&5.0)));
    assert!(s.campaign_commit().is_none(), "handed on once");
    assert_eq!(s.ext.counter("rescued"), 0.0, "the mission's own counters are a separate set");
}

#[test]
fn check_mission_knows_the_ship_and_world_types() {
    let mission = |conds: Value, acts: Value| {
        json!({
            "players": ["Player0 Group"],
            "groups": {"Fleet": [1]},
            "ships": [{"id": 1, "name": "RLS Hawk", "group": "Fleet"}],
            "paths": {"lane": [[0.0, 0.0, 0.0], [0.0, 100.0, 0.0]]},
            "tasks": [{"Name": "Escort"}],
            "environment": {"point_sets": {"harbour": [{"pos": [0.0, 0.0, 0.0]}, {"pos": [1.0, 1.0, 0.0]}]}},
            "rules": [rule("r", true, conds, acts)],
        })
    };
    let problems =
        |conds: Value, acts: Value| crate::rules::check::check(&mission(conds, acts), &Value::Null, &Value::Null).iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
    // A correct use of every new Type passes cleanly, and a campaign counter nobody sets in this mission is not an error.
    let clean = problems(
        json!([
            {"Type": "OP Group Is Cloaked", "Group Name": "Fleet"},
            {"Type": "OP Group Speed", "Group Name": "Fleet", "Which": "Fastest", "Equivalence": "Less Than", "Speed": 0.5},
            {"Type": "OP Task State", "Objective Task": "Escort", "State": "Complete"},
            {"Type": "OP Campaign Counter Condition", "Counter Name": "rescued", "Equivalence": "Greater Than", "Value": 1},
        ]),
        json!([
            {"Type": "OP Board Ship", "Group Name": "Fleet", "Target Group": "Fleet"},
            {"Type": "OP Abandon Ship", "Group Name": "Fleet"},
            {"Type": "OP Teleport", "Group/Unit Name": "Fleet", "Point Set Name": "harbour", "Point Index": 1, "Heading": 90},
            {"Type": "OP Follow Group", "Group Name": "Fleet", "Target Group": "Fleet", "Station": "Astern", "Distance": 50},
            {"Type": "OP Place Mines", "Point Set Name": "harbour"},
            {"Type": "OP Etherium Current", "Etherium Path": "lane", "Strength": 2},
            {"Type": "OP Set Campaign Counter", "Counter Name": "rescued", "Value": 1},
        ]),
    );
    assert_eq!(clean, "", "{clean}");
    let text = problems(
        json!([
            {"Type": "OP Group Speed", "Group Name": "Flete", "Which": "Quickest", "Equivalence": "Less Than", "Speed": 0.5},
            {"Type": "OP Task State", "Objective Task": "Escrot", "State": "Done"},
        ]),
        json!([
            {"Type": "OP Teleport", "Group/Unit Name": "Fleet", "Point Set Name": "harbour", "Point Index": 2},
            {"Type": "OP Teleport", "Group/Unit Name": "Fleet", "Point Set Name": "harbour", "Point Index": 0.5},
            {"Type": "OP Teleport", "Group/Unit Name": "Fleet", "Point Set Name": "habour"},
            {"Type": "OP Follow Group", "Group Name": "Fleet", "Target Group": "Fleet", "Station": "Behind", "Distance": 50},
            {"Type": "OP Follow Group", "Group Name": "Fleet", "Target Group": "Fleet", "Station": "Astern"},
            {"Type": "OP Etherium Current", "Etherium Path": "lan"},
            {"Type": "OP Place Mines", "Point Set Name": "habour"},
        ]),
    );
    for want in [
        "field \"Group Name\": unknown group 'Flete'",
        "field \"Which\": 'Quickest' is not one of",
        "unknown objective task 'Escrot'",
        "field \"State\": 'Done' is not one of",
        "field \"Point Index\": point set 'harbour' has 2 points (0..1); index 2 is past its end",
        "field \"Point Index\": 0.5 is not a whole number",
        "unknown point set 'habour'",
        "field \"Station\": 'Behind' is not one of",
        "field \"Distance\": missing; reads as 0",
        "unknown path 'lan'",
    ] {
        assert!(text.contains(want), "missing {want:?} in\n{text}");
    }
}

#[test]
fn a_teleport_index_that_is_not_a_whole_number_from_zero_moves_nothing() {
    let points = json!({"environment": {"point_sets": {"harbour": [{"pos": [0.0, 0.0, 0.0]}, {"pos": [9.0, 9.0, 0.0]}]}}});
    let w = [unit(1, "Fleet", 5.0, 5.0)];
    for (bad, said) in [(json!(-1), "point -1 of"), (json!(1.5), "point 1.5 of"), (json!(0.5), "point 0.5 of")] {
        let (s, fx) = effects(json!({"Type": "OP Teleport", "Group/Unit Name": "Fleet", "Point Set Name": "harbour", "Point Index": bad}), points.clone(), &w);
        assert!(fx.is_empty(), "{bad}: the checker says nothing is moved, and nothing is");
        assert!(s.warned.iter().any(|w| w.contains(said)), "{:?}", s.warned);
    }
    // An index written as "" is left out (the first point); a string that is not a number reads 0 like every number field.
    for index in [json!(""), json!("1")] {
        let (_, fx) = effects(json!({"Type": "OP Teleport", "Group/Unit Name": "Fleet", "Point Set Name": "harbour", "Point Index": index}), points.clone(), &w);
        assert!(matches!(fx.as_slice(), [Fx::Place { units, .. }] if units == &[(e(1), Vec3::ZERO)]));
    }
}

#[test]
fn an_optional_field_written_as_an_empty_string_is_the_same_as_leaving_it_out() {
    let extra = json!({"environment": {"point_sets": {"harbour": [{"pos": [0.0, 0.0, 0.0]}]}}, "paths": {"lane": [[0.0, 0.0, 0.0], [0.0, 100.0, 0.0]]}});
    let mut ship = unit(1, "Fleet", 0.0, 0.0);
    ship.heading = 1.0;
    let (_, fx) = effects(json!({"Type": "OP Teleport", "Group/Unit Name": "Fleet", "Point Set Name": "harbour", "Heading": ""}), extra.clone(), &[ship]);
    assert!(matches!(fx.as_slice(), [Fx::Place { heading: None, .. }]), "\"\" is no heading, not heading 0");
    let (_, fx) = effects(json!({"Type": "OP Etherium Current", "Etherium Path": "lane", "Strength": "", "Width": "", "Boolean": ""}), extra, &[]);
    assert!(matches!(fx.as_slice(), [Fx::CurrentSet { strength: None, width: None, on: None, .. }]), "nothing is changed, the current is not switched off");
    let mut sub = unit(2, "Sub", 0.0, 0.0);
    sub.cloaked = true;
    assert!(holds(json!({"Type": "OP Group Is Cloaked", "Group Name": "Sub", "Boolean": ""}), &[sub]), "\"\" is the default, TRUE");
    // The checker agrees: no complaint about the empty optional fields of an engine Type.
    let mission = json!({
        "players": ["Player0 Group"], "groups": {"Fleet": [1]}, "ships": [{"id": 1, "name": "RLS Hawk", "group": "Fleet"}],
        "paths": {"lane": [[0.0, 0.0, 0.0], [0.0, 100.0, 0.0]]}, "environment": {"point_sets": {"harbour": [{"pos": [0.0, 0.0, 0.0]}]}},
        "rules": [rule("r", true, json!([{"Type": "OP Group Is Cloaked", "Group Name": "Fleet", "Boolean": ""}]), json!([
            {"Type": "OP Teleport", "Group/Unit Name": "Fleet", "Point Set Name": "harbour", "Point Index": "", "Heading": ""},
            {"Type": "OP Etherium Current", "Etherium Path": "lane", "Strength": "", "Width": "", "Boolean": ""},
        ]))],
    });
    let text = crate::rules::check::check(&mission, &Value::Null, &Value::Null).iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
    assert_eq!(text, "");
}

#[test]
fn a_group_cannot_follow_itself() {
    let w = [unit(1, "Convoy", 0.0, 0.0), unit(2, "Convoy", 50.0, 0.0), unit(3, "Escort", 90.0, 0.0)];
    let follow = |who: &str| json!({"Type": "OP Follow Group", "Group Name": who, "Target Group": "Convoy", "Station": "Astern", "Distance": 40});
    let (s, fx) = effects(follow("Convoy"), json!({}), &w);
    assert!(matches!(fx.as_slice(), [Fx::Follow { units, .. }] if units.is_empty()), "no ship is told to follow its own group");
    assert!(s.warned.iter().any(|w| w.contains("are in the group 'Convoy' they should follow")), "{:?}", s.warned);
    let (s, fx) = effects(follow("Escort"), json!({}), &w);
    assert!(matches!(fx.as_slice(), [Fx::Follow { units, .. }] if units == &[e(3)]) && !s.warned.iter().any(|w| w.contains("should follow")));
}

#[test]
fn a_fade_is_cleared_when_the_mission_ends_and_a_restarted_mission_has_none() {
    let data = json!({"rules": [
        rule("fade", true, json!([{"Type": "World Initialize"}]), json!([{"Type": "OP Fade", "Seconds": 4.0}])),
        rule("lose", true, json!([flag_is("lose", true)]), json!([{"Type": "End Game"}])),
    ]});
    let mut s = script(data.clone(), Value::Null);
    s.step(&[], 0.1, false);
    s.step(&[], 1.9, false);
    assert!(s.fade_alpha() > 0.9, "black at the middle");
    s.flags.insert("lose".into(), true);
    s.step(&[], 0.1, false);
    assert!(s.ended.is_some());
    assert_eq!(s.fade_alpha(), 0.0, "the end text is shown on a clear picture");
    // A restart loads the mission again: a new script, no fade left over.
    assert_eq!(script(data, Value::Null).fade_alpha(), 0.0);
}

/// `--rule-report`: the report is built from the interpreter's own run counts, in script order.
#[test]
fn the_rule_report_says_what_each_rule_did() {
    use crate::authoring::{RuleReport, RuleReportOutcome};
    let mut asleep = rule("asleep", true, json!([]), json!([add("never", 1.0)]));
    asleep["Is Active"] = json!(false);
    let data = json!({"rules": [
        rule(" tick ", false, json!([]), json!([add("ticks", 1.0)])),
        rule("go", true, json!([flag_is("go", true)]), json!([set_flag("went")])),
        rule("never", true, json!([flag_is("never", true)]), json!([])),
        asleep,
        rule("mute", true, json!([flag_is("went", true)]), json!([{"Type": "OP Set Rule Active", "Rule Name": "tick", "Boolean": "FALSE"}])),
    ]});
    let mut s = script(data, Value::Null);
    s.step(&[], 0.5, false);
    s.step(&[], 0.5, false);
    s.flags.insert("go".into(), true);
    s.step(&[], 0.5, false); // "go" runs at 1.5 s, and "mute", after it, sees "went" and switches "tick" off
    s.step(&[], 0.5, false);
    s.step(&[], 0.5, false);
    let report = s.rule_report("mission_x", RuleReportOutcome::Quit);
    let seen: Vec<_> = report.rules.iter().map(|r| (r.name.as_str(), r.fired, r.runs, r.first_run, r.last_run, r.active)).collect();
    assert_eq!(
        seen,
        [
            ("tick", true, 3, Some(0.5), Some(1.5), false),
            ("go", true, 1, Some(1.5), Some(1.5), true),
            ("never", false, 0, None, None, true),
            ("asleep", false, 0, None, None, false),
            ("mute", true, 1, Some(1.5), Some(1.5), true),
        ]
    );
    assert_eq!((report.mission.as_str(), report.outcome, report.play_time), ("mission_x", RuleReportOutcome::Quit, 2.5));
    // The autoplay log's list comes from the same counts.
    assert_eq!(s.never_fired(), ["never", "asleep"]);
    // The file is JSON an editor parses back; a rule that never ran carries no times.
    let text = serde_json::to_string_pretty(&report).unwrap();
    assert!(text.contains("\"outcome\": \"quit\"") && !text.contains("null"), "{text}");
    assert_eq!(serde_json::from_str::<RuleReport>(&text).unwrap(), report);
}
