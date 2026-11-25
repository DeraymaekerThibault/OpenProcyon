use super::*;
use serde_json::json;

fn e(n: u32) -> Entity {
    Entity::from_raw_u32(n).unwrap()
}
fn script() -> Script {
    let data = json!({
        "player_start": {"name": "Hero"},
        "rules": [
            {"name": "Gate", "conditions": [], "actions": [{"Type": "Set Flag Action", "Flag Name": "g", "Boolean Value": "TRUE"}]},
            {"name": "Patrol", "Run Once": false, "conditions": [], "actions": []},
            {"name": "Dormant", "Is Active": false, "conditions": [], "actions": []},
            {"name": "Never", "conditions": [], "actions": []},
        ],
        "environment": {"point_sets": {"Harbour": [{"pos": [10.0, 20.0, 3.0]}, {"pos": [30.0, 40.0, 3.0]}]}},
        "volumes": {"Gate Area": [[0, 0], [10, 0], [10, 10]]},
        "paths": {"Route": [[0, 0, 0], [100, 50, 0]]},
        "points": [{"Name": "Objective", "Position": [5.0, 6.0, 0.0]}],
        "ships": []
    });
    Script::new(&data, Value::Null)
}
fn unit(n: u32, name: &str, owner: &str, pos: Vec3) -> Unit {
    Unit { name: name.into(), owner: owner.into(), primary: n == 1, groups: vec![owner.into()], ..Unit::new(e(n), n as i64, pos) }
}
fn world() -> Vec<Unit> {
    vec![unit(1, "Flagship", "Hero", Vec3::new(10.0, 5.0, -20.0)), unit(2, "Escort", "Hero", Vec3::ZERO), unit(3, "Raider", "Raiders", Vec3::new(1.0, 0.0, -2.0))]
}
fn run(sc: &mut Script, line: &str, env: &Env) -> (Vec<Fx>, Outcome) {
    let cmd = parse(line, sc.player(), false).unwrap_or_else(|err| panic!("{line}: {err}"));
    let mut out = vec![];
    let o = sc.console_run_with(&cmd, &mut world(), env, &mut out);
    (out, o)
}
fn quick(sc: &mut Script, line: &str) -> (Vec<Fx>, Outcome) {
    run(sc, line, &Env::default())
}

#[test]
fn rules_lists_every_rule_with_its_state() {
    let mut sc = script();
    sc.mark_ran(0);
    sc.fired[0] = true;
    sc.mark_ran(1);
    let text = quick(&mut sc, "rules").1.lines.join("\n");
    for (state, name) in [("fired", "Gate"), ("ran", "Patrol"), ("inactive", "Dormant"), ("never fired", "Never")] {
        assert!(text.lines().any(|l| l.starts_with(state) && l.contains(name)), "{state} {name}\n{text}");
    }
    assert!(text.contains("(repeats)"));
    let only = quick(&mut sc, "rules dorm").1.lines;
    assert_eq!(only.len(), 1);
    assert!(quick(&mut sc, "rules zzz").1.lines[0].contains("no rule contains"));
}

#[test]
fn skip_ends_the_line_that_is_playing() {
    let mut sc = script();
    assert!(quick(&mut sc, "skip").1.lines[0].contains("no speech line"));
    sc.speaking = Some(("LINE".into(), 3.0));
    let o = quick(&mut sc, "skip").1;
    assert!(o.skip_voice && o.lines[0].contains("counts as played"));
    sc.speech_tick(0.1);
    assert!(sc.played.contains("LINE"), "the next tick sees it as played");
}

#[test]
fn win_and_lose_end_the_mission_through_the_rules_actions() {
    let mut sc = script();
    let env = Env { mission: "mission03".into(), ..Env::default() };
    run(&mut sc, "win", &env);
    assert_eq!(sc.ended, Some(("Mission 3 complete.".into(), true)));
    let mut sc = script();
    run(&mut sc, "lose", &env);
    assert!(matches!(&sc.ended, Some((_, false))));
    assert_eq!(win_action("mission13")["World"], "Mission14.twt");
    assert_eq!(win_action("my_custom_map")["World"], "");
}

#[test]
fn nis_end_restart_and_the_flags_of_what_marks_a_cheat() {
    let mut sc = script();
    sc.nis = true;
    quick(&mut sc, "nis end");
    assert!(!sc.nis);
    assert!(quick(&mut sc, "restart").1.restart);
    let cheats = |l: &str| parse(l, "Hero", false).unwrap().cheats();
    for l in [
        "skip",
        "nis end",
        "win",
        "lose",
        "primary Hero",
        "damage 0.5",
        "attack Hero Raiders",
        "stance Hero neutral",
        "ally Hero Raiders",
        "hostile Hero Raiders",
        "tow Hero Raiders",
        "untow Hero Raiders",
        "say Opening",
    ] {
        assert!(cheats(l), "{l}");
    }
    for l in ["rules", "restart", "ships", "inspect", "where", "mark x", "show volumes", "hide all", "cam free", "cam follow Raiders", "hud off", "music Battle"] {
        assert!(!cheats(l), "{l}");
    }
}

#[test]
fn ships_lists_units_with_their_state_and_filters() {
    let mut sc = script();
    let env = Env { ships: vec![ShipInfo { e: Some(e(3)), stance: "Aggressive".into(), ..ShipInfo::default() }], ..Env::default() };
    let all = run(&mut sc, "ships", &env).1.lines;
    assert_eq!(all.len(), 4, "three units and the count: {all:?}");
    let raider = all.iter().find(|l| l.contains("Raider")).unwrap();
    assert!(raider.contains("#3") && raider.contains("[Raiders]") && raider.contains("(1, 2)") && raider.contains("Aggressive") && raider.contains("dmg 0%"), "{raider}");
    assert_eq!(run(&mut sc, "ships raiders", &env).1.lines.len(), 2, "filtered by owner or group");
    let mut w = world();
    w[2].visible = false;
    w[2].damage = 0.25;
    let cmd = parse("ships raider", "Hero", false).unwrap();
    let l = sc.console_run_with(&cmd, &mut w, &env, &mut vec![]).lines;
    assert!(l[0].contains("hidden") && l[0].contains("dmg 25%"), "{l:?}");
}

#[test]
fn inspect_shows_orders_and_tow_of_the_target() {
    let mut sc = script();
    let info = ShipInfo { e: Some(e(3)), order: 2, waypoints: 4, attack: vec!["Hero".into()], ram: true, speed: 7.5, ..ShipInfo::default() };
    let env = Env { target: Some(e(3)), ships: vec![info], ..Env::default() };
    sc.set_tow(e(1), e(3), true);
    let text = run(&mut sc, "inspect", &env).1.lines.join("\n");
    assert!(
        text.contains("Raider") && text.contains("order 2, 4 waypoint(s)") && text.contains("ramming") && text.contains("attacking Hero") && text.contains("towed by Flagship"),
        "{text}"
    );
    assert!(quick(&mut sc, "inspect").1.lines[0].contains("no target"));
}

#[test]
fn the_ship_commands_run_the_rule_actions() {
    let mut sc = script();
    assert!(matches!(&quick(&mut sc, "primary Hero,Escort").0[..], [Fx::Primary(u)] if u == &[e(2)]));
    let (_, o) = quick(&mut sc, "primary Escort");
    assert!(o.rejected && o.lines[0].contains("written 'Hero,Escort'"), "{:?}", o.lines);
    assert!(matches!(&quick(&mut sc, "damage 0.3 Raiders").0[..], [Fx::Damage(u, p)] if u == &[e(3)] && *p == 0.3));
    assert!(matches!(&quick(&mut sc, "damage 0.3").0[..], [Fx::Damage(u, _)] if u == &[e(1), e(2)]), "the player's fleet by default");
    assert!(matches!(&quick(&mut sc, "attack Raiders Hero").0[..], [Fx::Attack(u, g)] if u == &[e(3)] && g == "Hero"));
    assert!(matches!(&quick(&mut sc, "stance Raiders aggressive").0[..], [Fx::Stance(u, Stance::Aggressive)] if u == &[e(3)]));
    quick(&mut sc, "hostile Hero Raiders");
    assert!(sc.hostile("Hero", "Raiders"));
    quick(&mut sc, "ally Hero Raiders");
    assert!(!sc.hostile("Hero", "Raiders"));
    quick(&mut sc, "tow Hero Raiders");
    assert_eq!(sc.tow_specs, [("Hero".to_string(), "Raiders".to_string())]);
    quick(&mut sc, "untow Hero Raiders");
    assert!(sc.tow_specs.is_empty());
    assert!(parse("damage 1.5", "Hero", false).is_err() && parse("stance Hero grumpy", "Hero", false).is_err());
    let (_, o) = quick(&mut sc, "attack Raider Hero");
    assert!(o.rejected && o.lines[0].contains("did you mean 'Raiders'"), "names are checked: {:?}", o.lines);
}

#[test]
fn where_reports_game_units_and_mark_writes_a_point_line() {
    let mut sc = script();
    let text = quick(&mut sc, "where").1.lines.join("");
    assert!(text.contains("x 10.0, y 20.0, z 5.0"), "game y is Bevy -z: {text}");
    let path = std::env::temp_dir().join(format!("procyon-marks-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let env = Env { marks: Some(path.clone()), ..Env::default() };
    let o = run(&mut sc, "mark Rally point", &env).1;
    assert!(o.lines.join("\n").contains(&path.display().to_string()), "{:?}", o.lines);
    run(&mut sc, "mark second", &env);
    let written = std::fs::read_to_string(&path).unwrap();
    let first: Value = serde_json::from_str(written.lines().next().unwrap()).unwrap();
    assert_eq!(first, json!({"name": "Rally point", "pos": [10.0, 20.0, 5.0]}), "a point line to paste into a mission");
    assert_eq!(written.lines().count(), 2, "marks are appended");
    let _ = std::fs::remove_file(path);
    assert!(quick(&mut sc, "mark x").1.lines[0].contains("no user data folder"));
}

#[test]
fn shapes_come_from_the_volumes_paths_and_points_of_the_mission() {
    let sc = script();
    let v = sc.debug_shapes(ShapeKind::Volumes, None, 2.0);
    assert_eq!((v.len(), v[0].label.as_str(), v[0].closed, v[0].points.len()), (1, "Gate Area", true, 3));
    assert_eq!(v[0].points[1], Vec3::new(10.0, 2.0, 0.0), "game (x, y) at height h is Bevy (x, h, -y)");
    let p = sc.debug_shapes(ShapeKind::Paths, Some("Route"), 0.0);
    assert_eq!(p[0].points, [Vec3::ZERO, Vec3::new(100.0, 0.0, -50.0)]);
    assert!(sc.debug_shapes(ShapeKind::Paths, Some("nope"), 0.0).is_empty());
    let pts = sc.debug_shapes(ShapeKind::Points, None, 0.0);
    let labels: Vec<&str> = pts.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["Harbour", "Harbour 2", "Objective"], "every point of a set, and the objective points");
    assert_eq!(sc.shape_names(ShapeKind::Points), ["Harbour", "Harbour 2", "Objective"]);
}

#[test]
fn show_and_hide_change_the_developer_view() {
    let mut sc = script();
    let mut view = DevView::default();
    for c in quick(&mut sc, "show volumes").1.view.iter().chain(&quick(&mut sc, "show points Objective").1.view).chain(&quick(&mut sc, "show paths").1.view) {
        view.apply(c);
    }
    assert_eq!(view.shown.len(), 3);
    for c in &quick(&mut sc, "hide volumes").1.view {
        view.apply(c);
    }
    assert!(!view.shown.iter().any(|s| s.0 == ShapeKind::Volumes) && view.shown.len() == 2);
    for c in &quick(&mut sc, "hide points Objective").1.view {
        view.apply(c);
    }
    assert_eq!(view.shown.len(), 1);
    for c in &quick(&mut sc, "hide all").1.view {
        view.apply(c);
    }
    assert!(view.shown.is_empty());
    let (_, o) = quick(&mut sc, "show volumes Gate Aera");
    assert!(o.rejected && o.lines[0].contains("did you mean 'Gate Area'"), "{:?}", o.lines);
    assert!(parse("show everything", "Hero", false).is_err());
}

/// `apply` and `reset` write process-wide statics: the tests that touch them take turns.
static VIEW_STATICS: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn reset_clears_the_developer_view_and_the_statics_the_views_read() {
    let _g = VIEW_STATICS.lock().unwrap_or_else(|e| e.into_inner());
    let mut view = DevView::default();
    view.apply(&ViewChange::Hud(false));
    view.apply(&ViewChange::FreeCam(true));
    view.apply(&ViewChange::Show(ShapeKind::Volumes, Some("Gate Area".into())));
    assert!(hud_hidden() && free_cam());
    view.reset();
    assert!(!view.hud_hidden && !view.free_cam && !view.free_fresh && view.shown.is_empty());
    assert!(!hud_hidden() && !free_cam());
}

#[test]
fn cam_and_hud_ask_for_view_changes_and_use_the_rule_action_for_following() {
    let _g = VIEW_STATICS.lock().unwrap_or_else(|e| e.into_inner());
    let mut sc = script();
    assert_eq!(quick(&mut sc, "cam free").1.view, [ViewChange::FreeCam(true)]);
    let (fx, o) = quick(&mut sc, "cam follow Raiders");
    assert!(matches!(&fx[..], [Fx::Orbit { unit: Some(u), .. }] if *u == e(3)) && o.view == [ViewChange::FreeCam(false)]);
    let (fx, _) = quick(&mut sc, "cam player");
    assert!(matches!(&fx[..], [Fx::Orbit { unit: None, .. }]), "the player's own ship is the default focus");
    assert_eq!(quick(&mut sc, "hud off").1.view, [ViewChange::Hud(false)]);
    assert_eq!(quick(&mut sc, "hud on").1.view, [ViewChange::Hud(true)]);
    let mut view = DevView::default();
    view.apply(&ViewChange::FreeCam(true));
    assert!(view.free_cam && view.free_fresh, "a new free camera starts where the camera is");
    view.apply(&ViewChange::FreeCam(false));
    assert!(!view.free_cam);
}

#[test]
fn say_and_music_play_through_the_rule_actions() {
    let mut sc = script();
    quick(&mut sc, "say \"Opening line\"");
    assert_eq!(sc.queue.back().map(String::as_str), Some("Opening line"));
    quick(&mut sc, "music Battle");
    assert_eq!(sc.music.len(), 1);
    assert_eq!(sc.music[0].file, "Battle");
}
