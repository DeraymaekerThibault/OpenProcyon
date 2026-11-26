use super::*;
use serde_json::json;

use crate::rules::fixtures::{script_with_media, unit as base_unit};
fn script() -> Script {
    let data = json!({
        "player_start": {"name": "Hero"},
        "players": ["Hero", "Raiders"],
        "groups": {"Raiders": [25, 26], "Rocks": [40], "Empty": []},
        "rules": [
            {"name": "Open the gate", "conditions": [], "actions": [{"Type": "Set Flag Action", "Flag Name": "gate", "Boolean Value": "TRUE"}]},
        ],
        "objects": [
            {"type": "Island04", "id": 40, "pos": [-300.0, 300.0, 0.0]},
            {"type": "Base_Grant", "id": 41, "pos": [10.0, 20.0, 5.0]},
        ],
        "environment": {"point_sets": {"Harbour": [{"pos": [10.0, 20.0, 3.0]}, {"pos": [30.0, 40.0, 3.0]}]}},
        "volumes": {"Gate Area": [[0, 0], [10, 0], [10, 10]]},
        "paths": {"Route": [[0, 0, 0], [100, 50, 0]]},
        "points": [{"Name": "Objective", "Position": [5.0, 6.0, 0.0]}],
        "tasks": [{"Name": "Escape", "text": "Get out", "Active": true}],
        "ships": []
    });
    let media = json!({"speech": {"INTRO_1": {"text": "Welcome aboard, captain. The raiders are waiting at the gate and we have very little time left to act."}, "NOTEXT": {}}});
    script_with_media(data, media)
}
fn unit(n: u32, id: i64, name: &str, owner: &str) -> Unit {
    Unit { id, groups: vec![owner.into()], ..base_unit(n, name, owner) }
}
fn world() -> Vec<Unit> {
    vec![unit(1, 1, "Flagship", "Hero"), unit(2, 25, "Black Gull", "Raiders"), unit(3, 26, "Cutpurse", "Raiders")]
}
fn list(sc: &Script, line: &str) -> String {
    let Command::List(kind, text) = parse(line, "Hero", false).unwrap() else { panic!("{line} is not a list") };
    sc.list_lines(kind, &text, &world()).join("\n")
}

#[test]
fn kinds_parse_in_the_singular_and_the_plural() {
    assert_eq!(parse_kind("group"), Some("groups"));
    assert_eq!(parse_kind("Groups"), Some("groups"));
    assert_eq!(parse_kind("counter"), Some("counters"));
    assert_eq!(parse_kind("speech"), Some("speech"));
    assert_eq!(parse_kind("ships"), None, "`ships` is its own command");
    assert!(parse("list nonsense", "Hero", false).unwrap_err().contains("list takes groups"));
    assert!(parse("list", "Hero", false).unwrap_err().starts_with("usage: list"));
    assert!(parse("find", "Hero", false).unwrap_err().starts_with("usage: find"));
}

#[test]
fn groups_list_their_members_by_id_and_name() {
    let sc = script();
    let all = list(&sc, "list groups");
    assert!(all.contains("Raiders (2): 25 Black Gull, 26 Cutpurse"), "{all}");
    assert!(all.contains("Rocks (1): 40 Island04"), "scenery members are named by their kind\n{all}");
    assert!(all.contains("Empty: no members"), "{all}");
    let one = list(&sc, "list groups raid");
    assert!(one.contains("Raiders") && !one.contains("Rocks") && one.contains("1 of 3 groups"), "{one}");
    assert!(list(&sc, "list groups zzz").contains("no groups contain 'zzz' (3 in all)"));
}

#[test]
fn objects_show_id_type_and_position() {
    let sc = script();
    let text = list(&sc, "list objects");
    assert!(text.contains("#40 Island04 at (-300, 300, 0)") && text.contains("#41 Base_Grant at (10, 20, 5)"), "{text}");
    assert!(list(&sc, "list objects base").contains("#41") && !list(&sc, "list objects base").contains("#40"));
}

#[test]
fn speech_shows_the_event_and_the_start_of_its_subtitle() {
    let mut sc = script();
    let text = list(&sc, "list speech");
    let intro = text.lines().find(|l| l.starts_with("INTRO_1")).unwrap();
    assert!(intro.starts_with("INTRO_1: Welcome aboard, captain."), "{intro}");
    assert!(intro.ends_with("..."), "a long subtitle is cut: {intro}");
    assert!(text.contains("NOTEXT: (no subtitle)"), "{text}");
    sc.played.insert("INTRO_1".into());
    assert!(list(&sc, "list speech intro").contains("INTRO_1 [played]"));
}

#[test]
fn the_other_kinds_list_what_the_mission_defines() {
    let mut sc = script();
    sc.flags.insert("gate".into(), true);
    sc.timers.insert("t".into(), 2.5);
    let w = world();
    assert!(sc.list_lines("paths", "", &w)[0].starts_with("Route: 2 waypoint(s), from (0, 0) to (100, 50)"));
    assert!(sc.list_lines("volumes", "", &w)[0].starts_with("Gate Area: 3 corner(s), x 0..10, y 0..10"));
    let points = sc.list_lines("points", "", &w).join("\n");
    assert!(points.contains("Objective (objective point at (5, 6))") && points.contains("Harbour (point set, 2 point(s), first at (10, 20))"), "{points}");
    assert!(sc.list_lines("tasks", "", &w)[0].contains("active    Escape  Get out"));
    assert_eq!(sc.list_lines("flags", "", &w)[0], "gate = true");
    assert!(sc.list_lines("timers", "", &w)[0].starts_with("t = 2.5 s"));
    assert!(sc.list_lines("counters", "", &w)[0].contains("no counters"));
    let players = sc.list_lines("players", "", &w).join("\n");
    assert!(players.contains("Hero: 1 unit(s), the human player") && players.contains("Raiders: 2 unit(s)"), "{players}");
}

#[test]
fn find_searches_every_kind_and_names_each_hit() {
    let sc = script();
    let w = world();
    let gull = sc.find_lines("gull", &w).join("\n");
    assert!(gull.contains("group Raiders (2): 25 Black Gull, 26 Cutpurse"), "{gull}");
    assert!(gull.contains("ship #25 Black Gull [Raiders]"), "{gull}");
    let gate = sc.find_lines("gate", &w).join("\n");
    for kind in ["volume Gate Area", "rule #0 Open the gate", "speech INTRO_1"] {
        assert!(gate.contains(kind), "{kind}\n{gate}");
    }
    let last = sc.find_lines("gate", &w).pop().unwrap();
    assert!(last.ends_with("hit(s)"), "{last}");
    assert!(sc.find_lines("zzzz", &w)[0].contains("nothing is called or contains 'zzzz'"));
}

#[test]
fn list_and_find_are_queries_and_are_in_the_help_table_and_tab() {
    for line in ["list groups", "list objects x", "find gull"] {
        assert!(!parse(line, "Hero", false).unwrap().cheats(), "{line}");
    }
    for name in ["list", "find"] {
        let e = entry_named(name).unwrap();
        assert!(!e.cheats && !e.about.is_empty());
        assert!(help_lines(None).iter().any(|l| l.contains(e.usage)), "{name} is in the help list");
    }
    let n = Names::default();
    let c = completions("list ", &n);
    let shown: Vec<&str> = c.items.iter().map(|i| i.0.as_str()).collect();
    assert_eq!(shown, KINDS.to_vec());
    let c = completions("list sp", &n);
    assert_eq!(c.items, vec![("speech".to_string(), "speech".to_string())]);
}

#[test]
fn list_runs_through_the_console_without_changing_anything() {
    let mut sc = script();
    let cmd = parse("list groups", "Hero", false).unwrap();
    let mut out = vec![];
    let o = sc.console_run_with(&cmd, &mut world(), &Env::default(), &mut out);
    assert!(out.is_empty() && !o.lines.is_empty());
    assert!(o.lines.iter().any(|l| l.contains("Raiders (2)")));
}
