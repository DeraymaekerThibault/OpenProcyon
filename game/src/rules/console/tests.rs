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

#[test]
fn the_command_table_and_the_parser_agree() {
    let mut names = std::collections::HashSet::new();
    for e in TABLE {
        assert!(names.insert(e.name), "{} is listed twice", e.name);
        let parsed = parse(e.example, "Hero", false).unwrap_or_else(|err| panic!("the example of {}: {err}", e.name));
        assert_eq!(parsed.cheats(), e.cheats, "{}: the table and Command::cheats disagree", e.name);
        assert!(e.example.split_whitespace().next().is_some_and(|w| w == e.name), "{}: the example starts with the command", e.name);
        for a in e.alias {
            assert!(parse(&format!("{a} {}", e.example.split_once(' ').map_or("", |x| x.1)), "Hero", false).is_ok() || *a == "?", "alias {a}");
        }
    }
}

/// The other direction of the check above: a word the parser answers to needs a row in `TABLE`, or `help` would not show it.
#[test]
fn every_word_the_parser_accepts_is_in_the_table() {
    let src = include_str!("../console.rs");
    let body = &src[src.find("match word.to_lowercase().as_str() {").expect("the parser's match")..];
    let body = &body[..body.find("\nfn flag_word").expect("end of parse")];
    let mut seen = 0;
    for line in body.lines().filter(|l| l.starts_with("        \"")) {
        let Some((pat, _)) = line.split_once("=>") else { continue };
        for word in pat.split('|').filter_map(|w| w.trim().strip_prefix('"')?.split('"').next()).filter(|w| !w.is_empty()) {
            seen += 1;
            assert!(entry_named(word).is_some() || PLANNED.contains(&word), "`{word}` is parsed but has no row in console::TABLE, so `help` hides it");
        }
    }
    assert!(seen > 40, "the scan found only {seen} command words: did the parser's layout change?");
}

/// The table's `about` text cites rule Types by name. Each must still exist in `RULE_TYPES` (a rename there fails here), and each
/// must still be cited by the table (a rename here fails too), so the free text cannot drift from the vocabulary.
#[test]
fn the_rule_types_the_table_cites_exist_in_the_vocabulary() {
    const CITED: &[&str] = &[
        "Break Tow",
        "Damage Group/Unit by X percent",
        "Destroy Group/Unit",
        "End Game",
        "Goto Next Level",
        "GroupA to attack GroupB",
        "NIS End",
        "OP Set Counter",
        "OP Wait",
        "Play Music Track",
        "Play speech event",
        "Set Flag Action",
        "Set Fleet Primary Ship",
        "Set Group/Unit AI Stance",
        "Set Group/Unit Visibility",
        "Set Group/Unit Vulnerability",
        "Speech Event Played Once",
        "Start Timer",
        "Stop Timer",
        "Tow Ship",
    ];
    let text: String = TABLE.iter().map(|e| format!("{} {}\n", e.usage, e.about)).collect();
    for name in CITED {
        assert!(schema::RULE_TYPES.iter().any(|t| t.name == *name), "'{name}' is not in RULE_TYPES");
        assert!(text.contains(name), "'{name}' is no longer cited by the console table; update CITED");
    }
    // Anything in the vocabulary the table also cites must be listed, so a new citation gets pinned.
    for t in schema::RULE_TYPES {
        if text.contains(t.name) {
            assert!(CITED.contains(&t.name), "the console table cites '{}': add it to CITED", t.name);
        }
    }
}

#[test]
fn help_lists_commands_by_group_and_marks_the_cheats() {
    let all = help_lines(None).join("\n");
    for g in ["Cheats:", "Mission state:", "Raw rule access:"] {
        assert!(all.contains(g), "{g}");
    }
    for e in TABLE {
        assert!(all.contains(e.usage), "{} is missing from help", e.name);
    }
    assert!(all.contains("Keys:") && all.contains("PageUp") && all.contains("(also: do)"), "keys and aliases are listed");
    assert!(all.contains("*god") && all.contains(" flags"), "cheats carry a star, queries do not");
    let one = help_lines(Some("GOD"));
    assert!(one.iter().any(|l| l.contains("usage:") && l.contains("god [on|off]")) && one.iter().any(|l| l.contains("example: god on")));
    assert!(one.last().unwrap().contains("marks the session"));
    assert!(help_lines(Some("flags")).last().unwrap().contains("does not mark"));
    assert_eq!(parse("help why", "Hero", false), Ok(Command::Help(Some("why".into()))));
}

#[test]
fn an_unknown_command_names_the_nearest_one() {
    let err = |l: &str| parse(l, "Hero", false).unwrap_err();
    assert!(err("goh").contains("did you mean god") && err("goh").contains("`help`"), "{}", err("goh"));
    assert!(err("revael").contains("did you mean reveal"));
    assert!(err("flag").contains("did you mean flags"));
    assert!(!err("xyzzyplugh").contains("did you mean"), "nothing is near");
    assert!(help_lines(Some("rapair"))[0].contains("did you mean repair"));
}

#[test]
fn kill_spares_another_unit_with_the_same_name() {
    let mut sc = script();
    let mut w = world();
    w.push(unit(4, "Raider", "Pirates"));
    let cmd = parse("kill", "Hero", false).unwrap();
    let mut out = vec![];
    sc.console_run(&cmd, &mut w, Some(e(3)), &mut out);
    assert!(matches!(&out[..], [Fx::Destroy(u)] if u == &[e(3)]), "only the target, though the spec names both");
}

/// `docs/examples/gate-ambush.mission.json` is the worked example of `docs/missions.md` ("Rules, step by step"): it must stay
/// valid for the schema and the checker, and its rules must run.
#[test]
fn the_documented_example_mission_passes_the_checker_and_plays() {
    let mission: Value = serde_json::from_str(include_str!("../../../../docs/examples/gate-ambush.mission.json")).expect("the example is JSON");
    let problems = crate::rules::check::check(&mission, &Value::Null, &Value::Null);
    let errors: Vec<String> = problems.iter().filter(|p| p.level == crate::rules::check::Level::Error).map(ToString::to_string).collect();
    assert!(errors.is_empty(), "{errors:#?}");
    let warnings: Vec<String> = problems.iter().map(ToString::to_string).collect();
    assert!(warnings.is_empty(), "the example is meant to be clean: {warnings:#?}");
    let mut sc = Script::new(&mission, Value::Null);
    let hero = |pos: Vec3| Unit { name: "Dauntless".into(), owner: "Hero".into(), primary: true, ..Unit::new(e(10), 10, pos) };
    let (fx, _) = sc.step(&[hero(Vec3::ZERO)], 0.1, false);
    assert!(fx.is_empty());
    assert!(sc.task_active("Reach the gate"), "the Start rule activates the first task");
    assert_eq!(sc.ext.counter("raiders sunk"), 0.0);
    // Sailing into the gate runs the cutscene up to its wait, and the rest four seconds later.
    sc.step(&[hero(crate::data::conv(400.0, 600.0, 0.0))], 0.1, false);
    assert!(sc.nis && !sc.task_active("Clear the gate"), "the cutscene is waiting");
    for _ in 0..45 {
        sc.step(&[hero(crate::data::conv(400.0, 600.0, 0.0))], 0.1, false);
    }
    assert!(!sc.nis && sc.task_active("Clear the gate"), "the wait ended and the actions after it ran");
}

// ------------------------------------------------------------------------------------------------------------ scrollback

fn console_with(lines: usize) -> Console {
    let mut c = Console::new(true);
    c.say((0..lines).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n"));
    c
}

#[test]
fn page_keys_and_ctrl_home_end_move_the_view_through_the_output() {
    let mut c = console_with(100);
    assert_eq!(c.scroll, 0, "output starts at the end");
    c.key(&Key::PageUp);
    assert_eq!(c.scroll, SHOWN - 1, "a page keeps one line of overlap");
    c.key(&Key::PageDown);
    assert_eq!(c.scroll, 0);
    c.key(&Key::PageDown);
    assert_eq!(c.scroll, 0, "cannot go below the end");
    assert_eq!(c.key_with(&Key::Home, true), Edit::None);
    assert_eq!(c.scroll, 100 - SHOWN, "Ctrl+Home puts the first line at the top of the panel");
    c.key(&Key::PageUp);
    assert_eq!(c.scroll, 100 - SHOWN, "cannot go above the start");
    c.key_with(&Key::End, true);
    assert_eq!(c.scroll, 0);
    c.key(&Key::Home);
    assert_eq!(c.scroll, 0, "Home alone is not a scroll key");
}

#[test]
fn scroll_by_clamps_and_a_short_log_does_not_scroll() {
    let mut c = console_with(5);
    c.scroll_by(10);
    assert_eq!(c.scroll, 0, "everything fits");
    let mut c = console_with(40);
    c.scroll_by(WHEEL_LINES as isize);
    assert_eq!(c.scroll, WHEEL_LINES);
    c.scroll_by(-100);
    assert_eq!(c.scroll, 0);
    c.scroll_by(1000);
    assert_eq!(c.scroll, 40 - SHOWN);
}

#[test]
fn new_output_follows_only_a_view_that_was_at_the_end() {
    let mut c = console_with(50);
    c.say("new at the end");
    assert_eq!(c.scroll, 0, "at the bottom: stays at the bottom");
    c.scroll_by(10);
    c.say("one\ntwo\nthree");
    assert_eq!(c.scroll, 13, "scrolled back: the same lines stay in view, the marker counts the new ones");
    let end = c.log.len() - c.scroll;
    assert_eq!(c.log[end - 1], "line 40", "the last line in view is the same one after the new output arrived");
}

#[test]
fn submitting_a_line_returns_to_the_end() {
    let mut c = console_with(50);
    c.scroll_by(20);
    c.key(&Key::Character("x".into()));
    assert_eq!(c.key(&Key::Enter), Edit::Submit("x".into()));
    assert_eq!(c.scroll, 0);
}

#[test]
fn the_scrollback_is_capped_and_the_view_stays_inside_it() {
    let mut c = console_with(LOG_LINES + 500);
    assert_eq!(c.log.len(), LOG_LINES);
    assert_eq!(c.log[0], "line 500", "the oldest lines are the ones dropped");
    c.scroll_top();
    assert_eq!(c.scroll, LOG_LINES - SHOWN);
    c.say("more");
    assert!(c.scroll <= c.max_scroll());
}

// ------------------------------------------------------------------------------------------------------------------- `fps`

#[test]
fn fps_toggles_or_sets_the_counter_without_marking_cheats() {
    assert_eq!(parse("fps", "Hero", false), Ok(Command::Fps(None)));
    assert_eq!(parse("fps on", "Hero", false), Ok(Command::Fps(Some(true))));
    assert_eq!(parse("fps off", "Hero", false), Ok(Command::Fps(Some(false))));
    assert!(parse("fps maybe", "Hero", false).is_err());
    assert!(parse("fps on off", "Hero", false).unwrap_err().starts_with("usage: fps"));
    assert!(!parse("fps", "Hero", false).unwrap().cheats(), "a view command");
    let mut sc = script();
    let (out, o) = run(&mut sc, "fps on", &mut world(), None);
    assert!(out.is_empty());
    assert_eq!(o.view, vec![ViewChange::Fps(Some(true))], "the ECS side owns the saved setting");
}

#[test]
fn fps_is_in_the_help_table_and_tab_offers_on_and_off() {
    let e = entry_named("fps").expect("in the table");
    assert!(!e.cheats && help_lines(None).iter().any(|l| l.contains("fps [on|off]")));
    let c = completions("fps o", &Names::default());
    let shown: Vec<&str> = c.items.iter().map(|i| i.0.as_str()).collect();
    assert_eq!(shown, vec!["on", "off"]);
}

/// Every row of `KEYS` (the `help` text) names keys `Console::key_with` really acts on, and a new row must be added here.
#[test]
fn the_keys_help_lists_match_what_key_with_does() {
    // Each effect is measured on a console that has lines, history and a completable word, so a dead key shows no change.
    fn state(c: &Console) -> (String, usize, Option<Edit>) {
        (c.line.clone(), c.scroll, None)
    }
    let fresh = || {
        let mut c = console_with(100);
        c.history = vec!["first".into(), "second".into()];
        c.line = "rep".into();
        c
    };
    // (the word `KEYS` uses, the key, Ctrl held, a console set up so that the key must change the line or the scroll)
    let setup_scrolled = || {
        let mut c = fresh();
        c.scroll_by(10);
        c
    };
    let cases: Vec<(&str, Key, bool, Console)> = vec![
        ("Tab", Key::Tab, false, fresh()),
        ("Up", Key::ArrowUp, false, fresh()),
        ("Down", Key::ArrowDown, false, fresh()),
        ("PageUp", Key::PageUp, false, fresh()),
        ("PageDown", Key::PageDown, false, setup_scrolled()),
        ("Ctrl+Home", Key::Home, true, fresh()),
        ("Ctrl+End", Key::End, true, setup_scrolled()),
    ];
    let mut covered = std::collections::HashSet::new();
    for (name, key, ctrl, mut c) in cases {
        let before = state(&c);
        c.key_with(&key, ctrl);
        assert_ne!(state(&c), before, "{name} is listed in KEYS but key_with does nothing with it");
        covered.insert(name);
    }
    let mut c = fresh();
    assert_eq!(c.key_with(&Key::Escape, false), Edit::Close, "Escape closes");
    covered.insert("Escape");
    // Every key word in KEYS must be one of the cases above (the mouse wheel and the console key are handled in the system, not here).
    for (keys, _) in KEYS {
        for word in keys.split(|ch: char| ch == '/' || ch == ',' || ch == ' ').filter(|w| !w.is_empty()) {
            let known = covered.contains(word) || ["or", "the", "console", "key", "mouse", "wheel"].contains(&word);
            assert!(known, "KEYS lists '{word}' but this test does not exercise it: add a case");
        }
    }
    for want in covered {
        assert!(KEYS.iter().any(|(k, _)| k.contains(want)), "key_with handles {want} but KEYS does not list it");
    }
}
