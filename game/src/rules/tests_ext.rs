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
    assert!(!s.flag("woke") && !s.ran[0], "inactive");
    s.flags.insert("wake".into(), true);
    s.step(&[], 0.1, false);
    s.step(&[], 0.1, false);
    assert!(s.flag("woke"), "activated");
    s.act(&json!({"Type": "OP Set Rule Active", "Rule Name": "sleeper", "Boolean": false}), &[], &mut vec![], "t");
    assert!(!s.ext.is_active(0), "a JSON boolean works too");
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
