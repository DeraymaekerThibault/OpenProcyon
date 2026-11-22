use super::*;
use serde_json::json;

fn names() -> Names {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    Names {
        rules: s(&["Open the gate", "Open the vault", "Fail Mission"]),
        groups: s(&["Hero", "Pirates", "Raiders"]),
        units: s(&["Flagship", "Raider"]),
        point_sets: s(&["Harbour", "Hideout"]),
        volumes: s(&["Gate Area"]),
        paths: s(&["Patrol"]),
        flags: s(&["Battle @ Nebula", "key"]),
        timers: s(&["t", "Clock"]),
        tasks: s(&["Escape", "Head West"]),
        counters: s(&["kills"]),
        points: s(&["Harbour", "Objective"]),
        speech: s(&["Opening"]),
    }
}
fn shown(c: &Completion) -> Vec<&str> {
    c.items.iter().map(|i| i.0.as_str()).collect()
}
fn apply(line: &str, c: &Completion, k: usize) -> String {
    format!("{}{}", &line[..c.start], c.items[k].1)
}

#[test]
fn tokens_keep_quoted_names_whole() {
    let t = tokens(r#"set "Battle @ Nebula" on"#, false);
    assert_eq!(t.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["set", "Battle @ Nebula", "on"]);
    assert_eq!(t[1].start, 4, "the start is the opening quote");
    assert_eq!(tokens("a ", true).len(), 2, "a space starts a new empty word");
    assert_eq!(tokens("a b", true).last().unwrap().text, "b");
}

#[test]
fn the_command_word_completes() {
    assert_eq!(shown(&completions("fl", &names())), ["flags"]);
    assert_eq!(shown(&completions("re", &names())), ["repair", "restart", "reveal"]);
    assert!(completions("zzz", &names()).items.is_empty());
}

#[test]
fn rule_names_complete_for_fire_and_why_and_are_quoted_when_they_hold_spaces() {
    let c = completions("fire open", &names());
    assert_eq!(shown(&c), ["Open the gate", "Open the vault"]);
    assert_eq!(apply("fire open", &c, 0), "fire \"Open the gate\"");
    assert_eq!(shown(&completions("why fail", &names())), ["Fail Mission"]);
    assert_eq!(shown(&completions("why vault", &names())), ["Open the vault"], "a part inside the name matches when no name starts with it");
    assert_eq!(shown(&completions("why \"Open the g", &names())), ["Open the gate"], "an open quote is undone");
}

#[test]
fn arguments_complete_by_command_and_position() {
    let n = names();
    assert_eq!(shown(&completions("repair r", &n)), ["Raiders"]);
    assert_eq!(shown(&completions("tp h", &n)), ["Harbour", "Hideout"]);
    assert_eq!(apply("set bat", &completions("set bat", &n), 0), "set \"Battle @ Nebula\"");
    assert_eq!(shown(&completions("set key ", &n)), ["on", "off"]);
    assert_eq!(shown(&completions("timer c", &n)), ["Clock"]);
    assert_eq!(shown(&completions("timer Clock st", &n)), ["start", "stop"]);
    assert_eq!(shown(&completions("task head", &n)), ["Head West"]);
    assert_eq!(shown(&completions("task Escape c", &n)), ["complete"]);
    assert_eq!(shown(&completions("task Escape complete o", &n)), ["on", "off"]);
    assert_eq!(shown(&completions("counter k", &n)), ["kills"]);
    assert_eq!(shown(&completions("help go", &n)), ["god"]);
    assert!(completions("tp 10 ", &n).items.is_empty(), "a coordinate has nothing to complete");
}

#[test]
fn schema_types_complete_for_schema_act_and_cond() {
    let n = names();
    assert!(shown(&completions("schema Set Flag", &n)).contains(&"Set Flag Action"));
    let c = completions("act {\"Type\": \"Set Fla", &n);
    assert_eq!(shown(&c), ["Set Flag Action"]);
    assert_eq!(apply("act {\"Type\": \"Set Fla", &c, 0), "act {\"Type\": \"Set Flag Action\"");
    assert!(!shown(&completions("cond {\"Type\": \"Flag", &n)).contains(&"Set Flag Action"), "conditions only");
    assert!(shown(&completions("cond {\"Type\": \"Flag", &n)).contains(&"Flag Condition"));
}

#[test]
fn json_keys_and_values_complete_from_the_type() {
    let n = names();
    let line = r#"act {"Type": "Set Flag Action", "Fl"#;
    assert_eq!(shown(&completions(line, &n)), ["Flag Name"]);
    assert_eq!(apply(line, &completions(line, &n), 0), r#"act {"Type": "Set Flag Action", "Flag Name": "#);
    let used = r#"act {"Type": "Set Flag Action", "Flag Name": "key", ""#;
    assert_eq!(shown(&completions(used, &n)), ["Boolean Value"], "keys already present are not offered again");
    assert_eq!(shown(&completions(r#"act {"Type": "Set Flag Action", "Flag Name": "ba"#, &n)), ["Battle @ Nebula"]);
    assert_eq!(shown(&completions(r#"act {"Type": "Set Flag Action", "Boolean Value": "T"#, &n)), ["TRUE"]);
    assert_eq!(shown(&completions(r#"act {"Type": "Teleport Group/Unit", "Point Set Name": "h"#, &n)), ["Harbour", "Hideout"]);
    assert_eq!(shown(&completions(r#"act {"Ty"#, &n)), ["Type"]);
}

#[test]
fn repeated_tab_lists_then_cycles() {
    let mut c = Console::new(true);
    c.names = names();
    c.line = "fire open".into();
    c.key(&Key::Tab);
    assert_eq!(c.line, "fire \"Open the ", "the shared start goes in");
    assert!(c.log.last().unwrap().contains("Open the gate") && c.log.last().unwrap().contains("Open the vault"), "the candidates are listed");
    c.key(&Key::Tab);
    assert_eq!(c.line, "fire \"Open the gate\"");
    c.key(&Key::Tab);
    assert_eq!(c.line, "fire \"Open the vault\"");
    c.key(&Key::Tab);
    assert_eq!(c.line, "fire \"Open the gate\"", "it wraps");
    c.key(&Key::Backspace);
    c.key(&Key::Tab);
    assert_ne!(c.line, "fire \"Open the vault\"", "an edit ends the cycle");
    c.line = "set ke".into();
    c.key(&Key::Tab);
    assert_eq!(c.line, "set key ", "one candidate goes in, with a space for the next word");
}

#[test]
fn misspellings_get_the_closest_name() {
    assert_eq!(nearest_name("Raider", ["Hero", "Raiders"]), Some("Raiders"));
    assert_eq!(nearest_name("raiders", ["Raiders"]), Some("Raiders"), "case");
    assert_eq!(nearest_name("Heroo", ["Hero", "Pirates"]), Some("Hero"));
    assert_eq!(nearest_name("zz", ["zy"]), None, "short words are not guessed");
    assert_eq!(nearest_name("Banana", ["Hero", "Pirates"]), None);
}

fn script() -> Script {
    let data = json!({
        "player_start": {"name": "Hero"},
        "rules": [
            {"name": "Gate", "conditions": [{"Type": "Flag Condition", "Flag Name": "Gate Open", "Boolean Value": "TRUE"}],
             "actions": [{"Type": "Start Timer", "Timer Name": "Countdown"}]},
        ],
        "environment": {"point_sets": {"Harbour": [{"pos": [1.0, 2.0, 0.0]}]}},
        "tasks": [{"Name": "Escape", "text": "x", "Active": true}],
        "volumes": null, "ships": []
    });
    Script::new(&data, Value::Null)
}
fn world() -> Vec<Unit> {
    let unit = |n: u32, name: &str, owner: &str| Unit { name: name.into(), owner: owner.into(), primary: n == 1, ..Unit::new(Entity::from_raw_u32(n).unwrap(), -1, Vec3::ZERO) };
    vec![unit(1, "Flagship", "Hero"), unit(2, "Raider", "Raiders")]
}
fn answer(sc: &mut Script, line: &str) -> (bool, String) {
    let cmd = parse(line, "Hero", false).unwrap_or_else(|e| panic!("{line}: {e}"));
    let o = sc.console_run(&cmd, &mut world(), None, &mut vec![]);
    (o.rejected, o.lines.join(" | "))
}

#[test]
fn names_come_from_the_mission_and_its_rules() {
    let n = script().console_names(&world());
    assert_eq!(n.rules, ["Gate"]);
    assert!(n.flags.contains(&"Gate Open".to_string()) && n.timers.contains(&"Countdown".to_string()), "names rules mention count");
    assert_eq!(n.tasks, ["Escape"]);
    assert_eq!(n.point_sets, ["Harbour"]);
    assert!(n.groups.contains(&"Raiders".to_string()) && n.units.contains(&"Raider".to_string()));
}

#[test]
fn a_name_the_mission_does_not_have_is_refused_with_the_closest_one() {
    let mut sc = script();
    let (rejected, text) = answer(&mut sc, "repair Raider");
    assert!(rejected && text.contains("no group 'Raider'") && text.contains("did you mean 'Raiders'?"), "{text}");
    assert!(!answer(&mut sc, "repair Raiders").0);
    let (rejected, text) = answer(&mut sc, "tp Harbor");
    assert!(rejected && text.contains("did you mean 'Harbour'"), "{text}");
    let (_, text) = answer(&mut sc, "fire gat");
    assert!(!text.contains("no rule"), "a part of a name still finds the rule: {text}");
    let (_, text) = answer(&mut sc, "why Gaet");
    assert!(text.contains("did you mean 'Gate'"), "{text}");
    let (rejected, text) = answer(&mut sc, "task Escpe complete");
    assert!(rejected && text.contains("did you mean 'Escape'"), "{text}");
    let (rejected, text) = answer(&mut sc, "set Gate_Open");
    assert!(rejected && text.contains("did you mean 'Gate Open'"), "{text}");
    let (rejected, _) = answer(&mut sc, "set \"Gate Open\"");
    assert!(!rejected);
    let (rejected, _) = answer(&mut sc, "set a_brand_new_flag");
    assert!(!rejected, "a flag nothing is near may be new");
}

#[test]
fn raw_json_names_are_checked_too() {
    let mut sc = script();
    let (rejected, text) = answer(&mut sc, r#"act {"Type": "Teleport Group/Unit", "Group/Unit Name": "Raider", "Point Set Name": "Harbour"}"#);
    assert!(rejected && text.contains("did you mean 'Raiders'"), "{text}");
    let (rejected, text) = answer(&mut sc, r#"act {"Type": "Teleport Group/Unit", "Group/Unit Name": "Raiders", "Point Set Name": "Harbor"}"#);
    assert!(rejected && text.contains("point_set") && text.contains("did you mean 'Harbour'"), "{text}");
    assert!(!answer(&mut sc, r#"act {"Type": "Teleport Group/Unit", "Group/Unit Name": "Raiders", "Point Set Name": "Harbour"}"#).0);
    let (rejected, text) = answer(&mut sc, r#"cond {"Type": "Group Destroyed", "Group Name": "Raiderz"}"#);
    assert!(rejected && text.contains("did you mean 'Raiders'"), "{text}");
}

#[test]
fn the_second_set_of_commands_completes_its_arguments() {
    let n = names();
    assert_eq!(shown(&completions("stance Raiders agg", &n)), ["aggressive"]);
    assert_eq!(shown(&completions("attack Pi", &n)), ["Pirates"]);
    assert_eq!(shown(&completions("attack Pirates He", &n)), ["Hero"]);
    assert_eq!(shown(&completions("tow h", &n)), ["Hero"]);
    assert_eq!(shown(&completions("damage 0.5 rai", &n)), ["Raiders"]);
    assert_eq!(shown(&completions("show v", &n)), ["volumes"]);
    assert_eq!(shown(&completions("show volumes g", &n)), ["Gate Area"]);
    assert_eq!(shown(&completions("show points obj", &n)), ["Objective"]);
    assert_eq!(shown(&completions("hide a", &n)), ["all"]);
    assert_eq!(shown(&completions("cam f", &n)), ["free", "follow"]);
    assert_eq!(shown(&completions("cam follow rai", &n)), ["Raiders"]);
    assert_eq!(shown(&completions("hud o", &n)), ["on", "off"]);
    assert_eq!(shown(&completions("say op", &n)), ["Opening"]);
    assert_eq!(shown(&completions("nis e", &n)), ["end"]);
    assert_eq!(shown(&completions("rules fail", &n)), ["Fail Mission"]);
}
