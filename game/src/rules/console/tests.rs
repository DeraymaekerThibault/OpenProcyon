use super::*;
use serde_json::json;

fn e(n: u32) -> Entity {
    Entity::from_raw_u32(n).unwrap()
}
fn unit(n: u32, name: &str, owner: &str) -> Unit {
    Unit { name: name.into(), owner: owner.into(), primary: name == "Flagship", ..Unit::new(e(n), -1, Vec3::ZERO) }
}
fn script() -> Script {
    let never = json!({"Type": "Timer Condition", "Timer Name": "t", "Equivalence": "Greater Than", "Time in seconds": 5});
    let data = json!({
        "player_start": {"name": "Hero"},
        "rules": [
            {"name": "Open the gate", "conditions": [{"Type": "Flag Condition", "Flag Name": "key", "Boolean Value": "TRUE"}, never],
             "actions": [{"Type": "Set Flag Action", "Flag Name": "gate", "Boolean Value": "TRUE"}]},
            {"name": "Open the vault", "conditions": [], "actions": [{"Type": "Set Flag Action", "Flag Name": "vault", "Boolean Value": "TRUE"}]},
        ],
        "environment": {"point_sets": {"Harbour": [{"pos": [10.0, 20.0, 0.0]}]}},
        "tasks": [{"Name": "Escape", "text": "Get away", "Active": true}],
        "volumes": null, "ships": []
    });
    Script::new(&data, Value::Null)
}
fn world() -> Vec<Unit> {
    vec![unit(1, "Flagship", "Hero"), unit(2, "Escort", "Hero"), Unit { visible: false, ..unit(3, "Raider", "Pirates") }]
}
fn run(sc: &mut Script, line: &str, w: &mut [Unit], target: Option<Entity>) -> (Vec<Fx>, Outcome) {
    let cmd = parse(line, sc.player(), false).unwrap_or_else(|e| panic!("{line}: {e}"));
    let mut out = vec![];
    let o = sc.console_run(&cmd, w, target, &mut out);
    (out, o)
}

#[test]
fn short_commands_build_the_rule_actions() {
    assert_eq!(parse("god", "Hero", false), Ok(Command::Acts { acts: vec![vulnerability("Hero", 1.0)], god: Some(true) }));
    assert_eq!(parse("GOD", "Hero", true), Ok(Command::Acts { acts: vec![vulnerability("Hero", 0.0)], god: Some(false) }), "god toggles");
    assert_eq!(parse("god on", "Hero", true), Ok(Command::Acts { acts: vec![vulnerability("Hero", 1.0)], god: Some(true) }));
    assert_eq!(parse("set key off", "Hero", false), Ok(Command::Acts { acts: vec![set_flag("key", false)], god: None }));
    assert_eq!(
        parse("task Escape complete", "Hero", false),
        Ok(Command::Acts { acts: vec![json!({"Type": "Set Objective Task Complete State", "Objective Task": "Escape", "Complete State": "TRUE"})], god: None })
    );
    assert_eq!(parse("timer t start", "Hero", false), Ok(Command::Acts { acts: vec![json!({"Type": "Start Timer", "Timer Name": "t"})], god: None }));
    assert_eq!(parse("repair", "Hero", false), Ok(Command::Repair("Hero".into())));
    assert_eq!(parse("repair Pirates", "Hero", false), Ok(Command::Repair("Pirates".into())));
    assert_eq!(parse("tp 10 -20", "Hero", false), Ok(Command::Teleport(Dest::Xy(10.0, -20.0, 0.0))));
    assert_eq!(parse("tp Harbour", "Hero", false), Ok(Command::Teleport(Dest::Set("Harbour".into()))));
    assert_eq!(parse("fire \"Open the gate\"", "Hero", false), Ok(Command::Fire("Open the gate".into())));
    assert_eq!(parse("   ", "Hero", false), Ok(Command::Nothing));
}

#[test]
fn bad_lines_say_what_is_wrong() {
    for line in ["frobnicate", "god maybe", "tp", "fire", "set", "task Escape sideways", "act {not json}", "act {\"x\": 1}", "timer t pause"] {
        assert!(parse(line, "Hero", false).is_err(), "{line}");
    }
    assert!(parse("spawn Ship_Pirate_Sloop", "Hero", false).unwrap_err().contains("no rule action creates ships"));
}

#[test]
fn raw_actions_take_json_booleans() {
    let line = r#"act {"Type": "Set Flag Action", "Flag Name": "x", "Boolean Value": true}"#;
    assert_eq!(parse(line, "Hero", false), Ok(Command::Acts { acts: vec![set_flag("x", true)], god: None }));
    let mut sc = script();
    run(&mut sc, line, &mut world(), None);
    assert!(sc.flag("x"));
}

#[test]
fn only_commands_that_change_the_game_count_as_cheats() {
    let cheats = |l: &str| parse(l, "Hero", false).unwrap().cheats();
    for l in ["god", "repair", "kill", "reveal", "tp 1 2", "fire x", "set a", "act {\"Type\": \"Start Timer\", \"Timer Name\": \"t\"}"] {
        assert!(cheats(l), "{l}");
    }
    for l in ["help", "why x", "flags", "timers", "tasks", "schema flag", ""] {
        assert!(!cheats(l), "{l}");
    }
}

#[test]
fn god_runs_vulnerability_on_the_players_fleet() {
    let mut sc = script();
    let (fx, _) = run(&mut sc, "god", &mut world(), None);
    assert!(matches!(&fx[..], [Fx::Invuln(u, p)] if u == &[e(1), e(2)] && *p == 1.0), "the player's two ships, not the raider");
}

#[test]
fn kill_destroys_the_target_only() {
    let mut sc = script();
    let (fx, _) = run(&mut sc, "kill", &mut world(), Some(e(3)));
    assert!(matches!(&fx[..], [Fx::Destroy(u)] if u == &[e(3)]));
    let (fx, o) = run(&mut sc, "kill", &mut world(), None);
    assert!(fx.is_empty() && o.lines[0].contains("no target"));
}

#[test]
fn reveal_shows_hidden_units_of_every_owner() {
    let mut sc = script();
    let mut w = world();
    let (fx, o) = run(&mut sc, "reveal", &mut w, None);
    assert!(fx.iter().all(|f| matches!(f, Fx::Visible(_, true))) && fx.len() == 2, "one action per owner");
    assert!(w.iter().all(|u| u.visible), "the unit list sees it at once");
    assert!(o.lines[0].starts_with("1 hidden"));
}

#[test]
fn tp_moves_the_flagship_to_coordinates_or_a_point_set() {
    let mut sc = script();
    let (fx, _) = run(&mut sc, "tp 30 40", &mut world(), None);
    assert!(matches!(&fx[..], [Fx::Teleport(u, p)] if u == &[e(1)] && *p == crate::data::conv(30.0, 40.0, 0.0)));
    let (fx, _) = run(&mut sc, "tp Harbour", &mut world(), None);
    assert!(matches!(&fx[..], [Fx::Teleport(_, p)] if *p == crate::data::conv(10.0, 20.0, 0.0)));
    let (fx, o) = run(&mut sc, "tp Nowhere", &mut world(), None);
    assert!(fx.is_empty() && o.lines[0].contains("Harbour") && !o.lines[0].contains(TP_SET), "an unknown set lists the mission's own");
}

#[test]
fn repair_names_the_units_for_the_ecs_side() {
    let mut sc = script();
    let (fx, o) = run(&mut sc, "repair", &mut world(), None);
    assert!(fx.is_empty());
    assert_eq!(o.repair, [e(1), e(2)]);
}

#[test]
fn fire_runs_the_actions_once_and_a_part_of_the_name_is_enough() {
    let mut sc = script();
    run(&mut sc, "fire vault", &mut world(), None);
    assert!(sc.flag("vault"));
    assert_eq!(sc.never_fired(), ["Open the gate"], "the rule counts as having run");
    let (_, o) = run(&mut sc, "fire Open", &mut world(), None);
    assert!(o.lines[0].contains("matches 2 rules"), "{:?}", o.lines);
    let (_, o) = run(&mut sc, "fire nothing like it", &mut world(), None);
    assert!(o.lines[0].contains("no rule named"));
}

#[test]
fn why_lists_each_condition_and_leaves_the_rules_state_alone() {
    let mut sc = script();
    let (_, o) = run(&mut sc, "why gate", &mut world(), None);
    assert!(o.lines[0].contains("has not run"));
    assert!(o.lines[1].contains("NO") && o.lines[1].contains("Flag Condition"), "{:?}", o.lines);
    assert!(o.lines[2].contains("NO") && o.lines[2].contains("Timer Condition"));
    sc.flags.insert("key".into(), true);
    sc.timers.insert("t".into(), 6.0);
    let (_, o) = run(&mut sc, "why gate", &mut world(), None);
    assert!(o.lines.last().unwrap().contains("runs on the next step"), "{:?}", o.lines);
    assert!(!sc.flag("gate") && sc.never_fired().contains(&"Open the gate"), "asking does not run the rule");
}

#[test]
fn cond_answers_true_or_false() {
    let mut sc = script();
    let line = r#"cond {"Type": "Flag Condition", "Flag Name": "key", "Boolean Value": "TRUE"}"#;
    assert_eq!(run(&mut sc, line, &mut world(), None).1.lines, ["false"]);
    sc.flags.insert("key".into(), true);
    assert_eq!(run(&mut sc, line, &mut world(), None).1.lines, ["true"]);
}

#[test]
fn state_queries_print_flags_timers_and_tasks() {
    let mut sc = script();
    run(&mut sc, "set key", &mut world(), None);
    run(&mut sc, "timer t start", &mut world(), None);
    assert_eq!(run(&mut sc, "flags", &mut world(), None).1.lines, ["key = true"]);
    assert_eq!(run(&mut sc, "timers", &mut world(), None).1.lines, ["t = 0.0 s"]);
    assert!(run(&mut sc, "tasks", &mut world(), None).1.lines[0].starts_with("active    Escape"));
}

#[test]
fn the_line_editor_types_recalls_and_submits() {
    let mut c = Console::new(true);
    for ch in ["g", "o", "d"] {
        assert_eq!(c.key(&Key::Character(ch.into())), Edit::None);
    }
    assert_eq!(c.line, "god");
    c.key(&Key::Backspace);
    assert_eq!(c.line, "go");
    c.key(&Key::Character("d".into()));
    assert_eq!(c.key(&Key::Enter), Edit::Submit("god".into()));
    assert!(c.line.is_empty());
    assert_eq!(c.key(&Key::Enter), Edit::None, "an empty line is not submitted");
    c.key(&Key::ArrowUp);
    assert_eq!(c.line, "god", "Up brings the last line back");
    c.key(&Key::ArrowDown);
    assert!(c.line.is_empty());
    assert_eq!(c.key(&Key::Character("~".into())), Edit::Close, "the key that opens it closes it");
    assert_eq!(c.key(&Key::Escape), Edit::Close);
}

#[test]
fn the_output_log_keeps_its_last_lines() {
    let mut c = Console::new(true);
    for i in 0..LOG_LINES + 20 {
        c.say(format!("line {i}"));
    }
    assert_eq!(c.log.len(), LOG_LINES);
    assert_eq!(c.log.last().unwrap(), &format!("line {}", LOG_LINES + 19));
}

#[test]
fn raw_actions_and_conditions_are_checked_against_the_schema() {
    let err = |l: &str| parse(l, "Hero", false).unwrap_err();
    assert!(err(r#"act {"Type": "Set Flag"}"#).contains("did you mean: Set Flag Action"), "{}", err(r#"act {"Type": "Set Flag"}"#));
    assert!(err(r#"act {"Type": "Frobnicate"}"#).contains("schema <text>"));
    assert!(err(r#"act {"Type": "Set Flag Action", "Flag Name": "x"}"#).contains("needs Boolean Value"));
    assert!(err(r#"act {"Type": "Set Flag Action", "Flag Name": "x", "Boolean Value": true, "Colour": 1}"#).contains("no field Colour"));
    assert!(err(r#"cond {"Type": "Set Flag Action"}"#).contains("no condition Type"), "an action is not a condition");
    assert!(parse(r#"cond {"Type": "Flag Condition", "Flag Name": "k", "Boolean Value": true}"#, "Hero", false).is_ok());
}

#[test]
fn schema_lists_the_fields_of_a_type() {
    let lines = schema_lines("Set Flag Action");
    assert!(lines[0].contains("action") && lines[0].contains("Set Flag Action"));
    assert!(lines.iter().any(|l| l.contains("Flag Name") && l.contains("required")), "{lines:?}");
    assert!(schema_lines("flag").len() > 3, "a short text lists the matching Types");
    assert!(schema_lines("zzz")[0].contains("no Type"));
}

#[test]
fn tab_completes_the_command_word() {
    let mut c = Console::new(true);
    c.line = "re".into();
    c.key(&Key::Tab);
    assert_eq!(c.line, "re", "repair and reveal share only 're'");
    c.line = "rep".into();
    c.key(&Key::Tab);
    assert_eq!(c.line, "repair ");
    c.line = "fire Open".into();
    c.key(&Key::Tab);
    assert_eq!(c.line, "fire Open", "only the command word is completed");
}

fn ext_script() -> Script {
    let data = json!({
        "player_start": {"name": "Hero"},
        "rules": [
            {"name": "Dormant", "Is Active": false, "conditions": [], "actions": [set_flag("late", true)]},
            {"name": "Slow", "Run Once": false, "conditions": [], "actions": [set_flag("first", true), {"Type": "OP Wait", "Seconds": 5}, set_flag("second", true)]},
        ],
        "environment": {}, "volumes": null, "ships": []
    });
    Script::new(&data, Value::Null)
}

#[test]
fn counter_sets_an_op_counter_and_counters_lists_it() {
    let mut sc = ext_script();
    assert_eq!(parse("counter kills 3", "Hero", false), Ok(Command::Acts { acts: vec![json!({"Type": "OP Set Counter", "Counter Name": "kills", "Value": 3.0})], god: None }));
    assert!(parse("counter kills many", "Hero", false).is_err());
    assert!(parse("counter kills 3", "Hero", false).unwrap().cheats() && !parse("counters", "Hero", false).unwrap().cheats());
    assert_eq!(run(&mut sc, "counters", &mut world(), None).1.lines, ["no counter has been set"]);
    run(&mut sc, "counter kills 3", &mut world(), None);
    assert_eq!(sc.ext.counter("kills"), 3.0);
    assert_eq!(run(&mut sc, "counters", &mut world(), None).1.lines, ["kills = 3"]);
}

#[test]
fn why_says_when_a_rule_is_inactive_or_partway_through_its_actions() {
    let mut sc = ext_script();
    let (_, o) = run(&mut sc, "why Dormant", &mut world(), None);
    assert!(o.lines.iter().any(|l| l.contains("inactive")), "{:?}", o.lines);
    assert!(!o.lines.iter().any(|l| l.contains("runs on the next step")));
    run(&mut sc, "fire Slow", &mut world(), None);
    assert!(sc.flag("first") && !sc.flag("second"), "fire stops at the wait like a firing does");
    let (_, o) = run(&mut sc, "why Slow", &mut world(), None);
    assert!(o.lines.iter().any(|l| l.contains("busy") && l.contains("OP Wait")), "{:?}", o.lines);
}
