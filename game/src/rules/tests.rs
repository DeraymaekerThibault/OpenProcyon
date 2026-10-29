use super::*;
use bevy::prelude::*;
use serde_json::Value;
use std::collections::HashMap;
fn unit(name: &str, owner: &str, pos: Vec3) -> Unit {
    Unit {
        e: Entity::PLACEHOLDER,
        id: -1,
        name: name.into(),
        owner: owner.into(),
        owner_is_named_group: false,
        groups: vec![],
        pos,
        alive: true,
        visible: true,
        primary: name == "RLS Comet",
        damage: 0.0,
        flag: String::new(),
        attack: vec![],
    }
}
fn script() -> Script {
    let m = crate::campaign::load();
    m.script
}

#[test]
#[ignore = "needs imported assets"]
fn mission_two_sos_rockets_pick_up_by_entering_their_volume_without_clicking() {
    for (rule, set, gate, flag) in
        [("BottleOnePickup1", "Schooner1SOS", "PlayerHasDockedWithCamp", "PlayerHasPickedUpBottleOne"), ("BottleTwoPickup1", "Schooner2SOS", "NotTowed", "BottleTwoNotPickedUp")]
    {
        let mut s = crate::campaign::load_id("mission02").script;
        s.rules.retain(|r| r.name == rule);
        s.flags.insert(gate.into(), true);
        let point = crate::game_to_bevy(&s.point_sets[set][0]["pos"]);
        let mut player = unit("RLS Comet", s.player(), point + Vec3::X * 1000.0);
        let (fx, _) = s.step(&[player.clone()], 0.1, false);
        assert!(!fx.iter().any(|f| matches!(f, Fx::Event(..))));
        player.pos = point;
        let (fx, _) = s.step(&[player.clone()], 0.1, false);
        assert!(s.flags[flag], "{rule} should advance story state");
        assert!(fx.iter().any(|f| matches!(f, Fx::Event(p, n, false) if p == set && n == "BottleRocket_Light_Impact")), "pickup releases the beacon");
        assert!(fx.iter().any(|f| matches!(f, Fx::Sound(n) if n == "BottleRocket_Pickup")));
        let (fx, _) = s.step(&[player], 0.1, false);
        assert!(fx.is_empty(), "pickup runs once");
    }
}

#[test]
#[ignore = "needs imported assets"]
fn mission_three_hidden_sloop_does_not_start_the_hunt_at_initialization() {
    let m = crate::campaign::load_id("mission03");
    let mut w: Vec<_> = m.data["ships"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            let mut u = unit(s["name"].as_str().unwrap(), s["group"].as_str().unwrap(), crate::game_to_bevy(&s["pos"]));
            u.id = s["id"].as_i64().unwrap();
            u.e = Entity::from_raw_u32(u.id as u32).unwrap();
            u.primary = s["primary"] == true;
            u
        })
        .collect();
    let mut sc = m.script;
    sc.tag(&mut w);
    let hunt = sc.rules.iter().position(|r| r.name == "Sloop triggers hunt").unwrap();
    sc.step(&w, 0.1, false);
    assert!(!sc.fired[hunt], "world-init visibility must apply before spatial triggers");
    for u in &mut w {
        if u.owner == "Pirate Silver" {
            u.visible = false;
        }
    }
    sc.step(&w, 0.1, false);
    assert!(!sc.fired[hunt], "hidden placement must not advance the hunt");
    w.iter_mut().find(|u| u.id == 32).unwrap().visible = true;
    sc.step(&w, 0.1, false);
    assert!(!sc.fired[hunt], "revealing a ship already inside is not entering the hunt trigger");
    let exile = w.iter().position(|u| u.id == 32).unwrap();
    w[exile].pos = crate::game_to_bevy(&sc.paths["Sloop Patrolling"][0]);
    sc.step(&w, 0.1, false);
    assert!(!sc.fired[hunt], "patrolling outside the fleet must not start the hunt");
    let patrol_pos = w[exile].pos;
    w.iter_mut().find(|u| u.primary).unwrap().pos = patrol_pos + Vec3::X * 100.0;
    let (fx, _) = sc.step(&w, 0.1, false);
    assert!(fx.iter().any(|f| matches!(f, Fx::Path { path, .. } if path == "Sloop Retreats")), "spotting Exile orders a retreat, not an attack");
    assert!(sc.holds_fire("Pirate Silver"), "the pirates must not engage before Exile returns");
    assert!(!sc.fired[hunt]);
    w[exile].pos = Vec3::new(1490.0, 0.0, -1480.0);
    let (fx, _) = sc.step(&w, 0.1, false);
    assert!(sc.fired[hunt], "returning through the trigger starts the hunt");
    assert!(fx.iter().any(|f| matches!(f, Fx::Path { path, .. } if path == "Small ships Attack")));
    let intercept: Vec<_> = w.iter().filter(|u| [41, 89].contains(&u.id)).map(|u| u.e).collect();
    let punishers: Vec<_> = w.iter().filter(|u| [422, 423, 424].contains(&u.id)).map(|u| u.e).collect();
    let reserves: Vec<_> = intercept.iter().chain(&punishers).copied().collect();
    for effect in &fx {
        if let Fx::Path { units, .. } | Fx::Stance(units, _) | Fx::Visible(units, true) = effect {
            assert!(units.iter().all(|e| !reserves.contains(e)), "hunt orders must not launch or reveal either reserve fleet");
        }
    }
    let actions = sc.rules.iter().find(|r| r.name == "Player Runs & Big Ships Attack").unwrap().acts.clone();
    let mut fx = vec![];
    for action in actions {
        sc.act(&action, &w, &mut fx, "Player Runs & Big Ships Attack");
    }
    for effect in &fx {
        if let Fx::Attack(units, _) | Fx::Stance(units, _) = effect {
            assert!(units.iter().all(|e| !reserves.contains(e)), "large-fleet orders must not launch either reserve fleet");
        }
    }
    sc.rules.retain(|r| matches!(r.name.as_str(), "Pirates On Intercept Course" | "Player Needs Punishment"));
    sc.fired = vec![false; sc.rules.len()];
    sc.flags.insert("Player Obeying Mission Flow".into(), true);
    let trigger = &sc.volumes["Player Triggers Chase Group"];
    let c = trigger.centroid().unwrap();
    let center = Vec3::new(c.x, 0.0, -c.y);
    assert!(in_volume(trigger, center));
    w.iter_mut().find(|u| u.primary).unwrap().pos = center;
    let (fx, _) = sc.step(&w, 0.1, false);
    assert!(fx.iter().any(|f| matches!(f, Fx::Attack(units, _) if units == &intercept)), "the authored chase trigger launches exactly the two intercept ships");
    assert!(
        !fx.iter().any(|f| matches!(f, Fx::Attack(units, _) | Fx::Stance(units, _) | Fx::Visible(units, true) if units.iter().any(|e| punishers.contains(e)))),
        "normal mission flow must not launch or reveal the Punishers"
    );
    // The alternate retail branch intentionally punishes entering the chase area before the main-fleet encounter.
    w.iter_mut().find(|u| u.primary).unwrap().pos = center + Vec3::X * 10000.0;
    sc.step(&w, 0.1, false);
    sc.flags.insert("Player Obeying Mission Flow".into(), false);
    w.iter_mut().find(|u| u.primary).unwrap().pos = center;
    let (fx, _) = sc.step(&w, 0.1, false);
    assert!(fx.iter().any(|f| matches!(f, Fx::Visible(units, true) if units == &punishers)));
    assert!(fx.iter().any(|f| matches!(f, Fx::Stance(units, Stance::Aggressive) if units == &punishers)));
    assert!(fx.iter().any(|f| matches!(f, Fx::Attack(units, _) if units == &punishers)), "only their own early-entry trigger launches the Punishers");
}

#[test]
#[ignore = "needs imported assets"]
fn mission_three_nebula_entry_tracks_the_travelling_storm() {
    let m = crate::campaign::load_id("mission03");
    let mut sc = m.script;
    let mut player = unit("RLS Comet", sc.player(), Vec3::new(600.0, 0.0, 1400.0));
    let cond = serde_json::json!({"Type":"Unit is Within any Nebula", "Unit Name":"Player0 Group,HUMAN CONTROLLED COMMAND SHIP"});
    assert!(!sc.cond(&cond, &[player.clone()], false));
    let mut shift = crate::nebula::StormShift::default();
    shift.0.insert("Solar Storm".into(), (Vec3::X * 1300.0, Vec3::ZERO));
    sc.nebulae = crate::nebula::polygons(&m.data, &shift);
    assert!(sc.cond(&cond, &[player.clone()], false), "storm moved over the ship");
    player.pos.x -= 1300.0;
    assert!(!sc.cond(&cond, &[player], false), "original boundary must no longer count");
}

#[test]
#[ignore = "needs imported assets"]
fn fleet_rules_emit_real_named_fleet_and_primary_effects() {
    let mut s = script();
    for (name, expected, hold) in
        [("Diamond", crate::ship::Formation::Diamond, true), ("Convoy", crate::ship::Formation::Convoy, true), ("None", crate::ship::Formation::None, false)]
    {
        let mut out = vec![];
        s.act(&serde_json::json!({"Type":"Set FleetFormation Type", "Player Name":"Small Ships (Red Team)", "Formation Type":name, "Hold Formation":if hold { "TRUE" } else { "FALSE" }}), &[], &mut out, "synthetic M1");
        assert!(matches!(&out[0], Fx::Formation(owner, Some(kind), h) if owner == "Small Ships (Red Team)" && *kind == expected && *h == hold));
    }
    let mut out = vec![];
    s.act(&serde_json::json!({"Type":"Set Fleet Hold Formation", "Player Name":"Player", "Hold Formation":"TRUE"}), &[], &mut out, "t");
    assert!(matches!(&out[0], Fx::Formation(owner, None, true) if owner == "Player"));
    for (mission, group, ship) in [(5, "Jim", "RLS Superior"), (9, "Players Ships", "RLS St Roch")] {
        let mut s = crate::campaign::load_id(&format!("mission{mission:02}")).script;
        let mut candidate = unit(ship, group, Vec3::ZERO);
        candidate.e = Entity::from_raw_u32(30).unwrap();
        let mut out = vec![];
        s.act(&serde_json::json!({"Type":"Set Fleet Primary Ship", "Group/Unit Name":format!("{group},{ship}")}), &[candidate.clone()], &mut out, "synthetic primary");
        assert!(matches!(&out[0], Fx::Primary(ids) if ids == &[candidate.e]));
    }
}

#[test]
#[ignore = "needs imported assets"]
fn human_captain_actions_hand_over_the_primary_ship() {
    for (mission, rule, ship) in [
        (3, "Setup New Sloop & Essential Ship", "RLS Endurance"),
        (5, "(END NIS) Player Undocks @ Grant  XXXMUSICXXX", "RLS Superior"),
        (9, "Charge Longboat", "Longboat"),
        (9, "Docks with Frigate", "RLS St Roch"),
    ] {
        let m = crate::campaign::load_id(&format!("mission{mission:02}"));
        let action = m.data["rules"].as_array().unwrap().iter().find(|r| r["name"] == rule).unwrap()["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["Type"] == "Set Group/Unit AI Captain")
            .unwrap();
        let mut s = m.script;
        let mut candidate = unit(ship, s.player(), Vec3::ZERO);
        candidate.e = Entity::from_raw_u32(30).unwrap();
        let mut out = vec![];
        s.act(action, &[candidate.clone()], &mut out, rule);
        assert!(matches!(&out[0], Fx::Captain(ids, true) if ids == &[candidate.e]), "{rule}");
        assert!(matches!(&out[1], Fx::Primary(ids) if ids == &[candidate.e]), "{rule}");
        let mut ai_action = action.clone();
        ai_action["AI Stance"] = serde_json::json!("Captain_AI");
        out.clear();
        s.act(&ai_action, &[candidate.clone()], &mut out, rule);
        assert_eq!(out.len(), 1, "AI captains must not take player control");
        assert!(matches!(&out[0], Fx::Captain(ids, false) if ids == &[candidate.e]));
    }
}

#[test]
#[ignore = "needs imported assets"]
fn menu_tow_links_are_kept_per_tower_and_broken() {
    let mut sc = script();
    let mut world = World::new();
    let (a, b, c) = (world.spawn_empty().id(), world.spawn_empty().id(), world.spawn_empty().id());
    assert_eq!(sc.towee_of(a), None);
    sc.set_tow(a, b, true);
    sc.set_tow(a, b, true);
    assert_eq!(sc.tows().iter().filter(|t| **t == (a, b)).count(), 1, "one link per pair");
    assert_eq!(sc.towee_of(a), Some(b));
    sc.set_tow(c, b, true);
    sc.break_tows(a);
    assert_eq!(sc.towee_of(a), None);
    assert_eq!(sc.tows(), &[(c, b)], "other towers keep their links");
    sc.set_tow(c, b, false);
    assert!(sc.tows().is_empty());
}

#[test]
#[ignore = "needs imported assets"]
fn asteroid_setup_selects_named_groups_and_preserves_kill_attribution() {
    let m = crate::campaign::load();
    let mut sc = m.script;
    let mut world = World::new();
    let mut belt = unit("Asteroid_Large", "Asteroid Belt", Vec3::ZERO);
    belt.e = world.spawn_empty().id();
    let mut montressor = unit("Asteroid_Med", "Montressor Asteroids", Vec3::ZERO);
    montressor.e = world.spawn_empty().id();
    let mut effects = vec![];
    for r in m.data["rules"].as_array().unwrap() {
        for action in r["actions"].as_array().unwrap().iter().filter(|a| a["Type"] == "Setup Asteroid Belt") {
            sc.act(action, &[belt.clone(), montressor.clone()], &mut effects, "test");
        }
    }
    assert!(effects.iter().any(|e| matches!(e, Fx::Asteroids { units, speed: [0.0, 0.0], tumble: [0.5, 0.9], .. } if units == &[belt.e])));
    assert!(effects.iter().any(|e| matches!(e, Fx::Asteroids { units, tumble: [0.5, 0.75], .. } if units == &[montressor.e])));
    sc.kills.insert((PLAYER.into(), true), 1);
    assert!(sc.cond(&serde_json::json!({"Type":"Player Killed A Object", "Player Name":PLAYER, "World Object Type":"Asteroid"}), &[], false));
}

#[test]
#[ignore = "needs imported assets"]
fn all_rule_condition_and_action_types_are_handled() {
    let m = crate::campaign::load();
    let mut s = m.script;
    let w = vec![unit("RLS Comet", PLAYER, Vec3::ZERO)];
    let mut out = vec![];
    for r in m.data["rules"].as_array().unwrap() {
        for c in r["conditions"].as_array().unwrap() {
            s.cond(c, &w, false);
        }
        for a in r["actions"].as_array().unwrap() {
            s.act(a, &w, &mut out, "test");
        }
    }
    assert!(s.warned.iter().all(|x| x.starts_with("formations")), "unhandled: {:?}", s.warned);
}
/// Convoy Raid's own script (maps/convoy_raid/script.json): points by team for convoy kills (credited to the killer's team),
/// per-unit arrivals in the safety area, and each event scored once by its repeating rule.
#[test]
#[ignore = "needs imported assets"]
fn convoy_raid_scores_team_points_once_per_event() {
    let Some(data) = crate::data::try_json(&crate::assets_dir().join("maps/convoy_raid/script.json")) else { return };
    let mut s = Script::new(&data, Value::Null);
    let (navy, pirates) = ("IDGS_TPTEAMNAMES_NAVY", "IDGS_TPTEAMNAMES_PIRATES");
    s.set_teams(vec![(navy.into(), vec!["Navy1".into(), "convoys".into()]), (pirates.into(), vec!["pirate1".into()])]);
    let mut world = World::new();
    let safety = s.volume("convoy safety area").centroid().unwrap();
    let ship = |world: &mut World, id: i64, name: &str| Unit { e: world.spawn_empty().id(), id, ..unit(name, "convoys", Vec3::new(5000.0, 0.0, 5000.0)) };
    // convoy 1: SS Labrador 127 .. SS Nova Scotia 130
    let mut w: Vec<Unit> = (127..=130).map(|id| ship(&mut world, id, "convoy ship")).collect();
    s.tag(&mut w);
    let pts = |s: &Script, t: &str| s.team_points.get(t).copied().unwrap_or(0);
    s.step(&w, 0.1, false);
    assert_eq!((pts(&s, navy), pts(&s, pirates)), (0, 0));
    // two convoy ships reach safety: +100 each, one per rule firing, never again for the same ship
    for u in &mut w[..2] {
        u.pos = Vec3::new(safety.x, 0.0, -safety.y);
    }
    for _ in 0..4 {
        s.step(&w, 0.1, false);
    }
    assert_eq!(pts(&s, navy), 200);
    // a pirate sinks one (+50 to the pirates); the Navy sinking another is a screw-up (+70 to the pirates)
    w[2].alive = false;
    s.record_destroyed("pirate1", w[2].e);
    w[3].alive = false;
    s.record_destroyed("Navy1", w[3].e);
    for _ in 0..3 {
        s.step(&w, 0.1, false);
    }
    assert_eq!((pts(&s, navy), pts(&s, pirates)), (200, 120));
    assert!(s.cond(&serde_json::json!({"Type": "Team has X points", "Team Name": pirates, "Points": 120}), &w, false));
    assert!(!s.cond(&serde_json::json!({"Type": "Team has X points", "Team Name": pirates, "Points": 400}), &w, false));
}

/// Every condition and action the skirmish maps' own scripts use is handled (end rules belong to victory.rs).
#[test]
#[ignore = "needs imported assets"]
fn skirmish_script_rule_types_are_handled() {
    let w = vec![unit("RLS Comet", PLAYER, Vec3::ZERO)];
    for map in [
        "ambush",
        "bayles_stand",
        "border_dispute",
        "convoy_raid",
        "diablo_straight",
        "dragons_nest",
        "iron_war",
        "locusts",
        "mousetrap",
        "red_rover",
        "shadow_dance",
        "storm_front",
        "the_maw",
        "zemyatin",
    ] {
        let Some(data) = crate::data::try_json(&crate::assets_dir().join(format!("maps/{map}/script.json"))) else { return };
        let data = without_end_rules(&data);
        let mut s = Script::new(&data, Value::Null);
        let mut out = vec![];
        for r in data["rules"].as_array().unwrap() {
            for c in r["conditions"].as_array().unwrap() {
                s.cond(c, &w, false);
            }
            for a in r["actions"].as_array().unwrap() {
                s.act(a, &w, &mut out, "test");
            }
        }
        // PROVISIONAL gaps, one map each: Locusts' 'Team Member Enters Volume'.
        assert!(s.warned.iter().all(|x| x.contains("Team Member Enters Volume")), "{map}: unhandled {:?}", s.warned);
    }
}

/// A script with no rules of its own, for evaluating single rule records against the given media.
fn bare_script(data: Value, media: Value) -> Script {
    let data = serde_json::json!({"rules": [], "ships": [], "volumes": data, "environment": {}});
    Script::new(&data, media)
}

#[test]
fn victory_line_names_the_mission_just_won() {
    assert_eq!(victory_text("Mission02.twt"), "Mission 1 complete.");
    assert_eq!(victory_text("mission11.twt"), "Mission 10 complete.");
    assert_eq!(victory_text("???"), "Mission complete.");
    let mut sc = bare_script(Value::Null, Value::Null);
    sc.act(&serde_json::json!({"Type": "Goto Next Level", "World": "Mission03.twt"}), &[], &mut vec![], "test");
    assert_eq!(sc.ended, Some(("Mission 2 complete.".into(), true)));
}

#[test]
fn set_ship_name_labels_the_group_from_the_media_string() {
    let mut sc = bare_script(Value::Null, serde_json::json!({"ship_names": {"KEY_SILVER": "Robot Silver"}}));
    let mut world = World::new();
    let mut silver = unit("The Argentum", "Robot Silver", Vec3::ZERO);
    silver.e = world.spawn_empty().id();
    let mut out = vec![];
    let act = serde_json::json!({"Type": "Set Ship Name", "Group/Unit Name": "Robot Silver", "New Ship Name": "KEY_SILVER"});
    sc.act(&act, std::slice::from_ref(&silver), &mut out, "test");
    assert!(matches!(&out[..], [Fx::Label(u, t)] if u == &[silver.e] && t == "Robot Silver"));
    // Unknown string keys are reported, not shown raw.
    let mut out = vec![];
    sc.act(&serde_json::json!({"Type": "Set Ship Name", "Group/Unit Name": "Robot Silver", "New Ship Name": "NOPE"}), &[silver], &mut out, "test");
    assert!(out.is_empty() && sc.warned.contains("ship name 'NOPE'"));
}

#[test]
fn top_speed_percentage_caps_the_named_ships_throttle() {
    let mut sc = bare_script(Value::Null, Value::Null);
    let mut world = World::new();
    let mut player = unit("RLS Comet", PLAYER, Vec3::ZERO);
    player.e = world.spawn_empty().id();
    let mut out = vec![];
    let act = serde_json::json!({"Type": "Set Ship Top Speed Percentage", "Group Name": PLAYER, "Throttle Speed": 0.15});
    sc.act(&act, std::slice::from_ref(&player), &mut out, "test");
    assert!(matches!(&out[..], [Fx::Throttle(u, v)] if u == &[player.e] && (*v - 0.15).abs() < 1e-6));
}

#[test]
#[ignore = "needs imported assets"]
fn unit_within_any_nebula_uses_the_setup_polygons() {
    // Mission 1's only nebula is the 'Clarke' polygon (game x, y); its vertex average lies inside it.
    let m = crate::campaign::load();
    let Vec2 { x, y } = Region::from_json(&m.data["volumes"]["Clarke"]).centroid().unwrap();
    let mut world = World::new();
    let mut ship = unit("RLS Comet", PLAYER, Vec3::new(x, 0.0, -y));
    ship.e = world.spawn_empty().id();
    let cond = serde_json::json!({"Type": "Unit is Within any Nebula", "Unit Name": PLAYER});
    let mut sc = m.script;
    assert!(sc.nebulae.len() == 1, "one nebula in mission 1");
    assert!(sc.cond(&cond, std::slice::from_ref(&ship), false));
    ship.pos = Vec3::new(1.0e6, 0.0, 0.0);
    assert!(!sc.cond(&cond, &[ship], false));
}

#[test]
#[ignore = "needs imported assets"]
fn whales_receive_paths_visibility_and_reprimand_after_attack() {
    let mut sc = script();
    let mut world = World::new();
    let mut player = unit("RLS Comet", PLAYER, Vec3::ZERO);
    player.e = world.spawn_empty().id();
    let mut whale = unit("Whales", "Whales", Vec3::X * 100.0);
    whale.e = world.spawn_empty().id();
    whale.groups = vec!["Whales".into()];
    // As spawned: owned by its group until 'Assign Ownership to Whales' hands it to the Whales player.
    let mut second_group = unit("Whales 2", "Whales 2", Vec3::X * 150.0);
    second_group.groups = vec!["Whales 2".into()];
    second_group.e = world.spawn_empty().id();
    let units = [player, whale, second_group];
    assert_eq!(members(&units, "Whales").len(), 1);
    assert_eq!(members(&units, "Whales 2").len(), 1);
    sc.init_done = true;
    sc.flags.insert("All Ships @ Nebula".into(), true);
    let (effects, _) = sc.step(&units, 0.1, false);
    assert!(effects.iter().any(|f| matches!(f, Fx::Path { units: u, path, looping: true, .. } if u.contains(&units[1].e) && path == "Space Whale Path")));
    assert!(effects.iter().any(|f| matches!(f, Fx::Velocity(u, v) if u.contains(&units[1].e) && *v == 12.0)));
    assert!(effects.iter().any(|f| matches!(f, Fx::Visible(u, true) if u.contains(&units[1].e))));
    assert!(sc.hostile(PLAYER, "Whales"));
    sc.record_fire(units[0].e, Some(units[1].e));
    sc.step(&units, 0.1, false);
    assert!(sc.timers.contains_key("WhaleTimer"));
    sc.step(&units, 3.1, false);
    assert!(!sc.hostile(PLAYER, "Whales"));
    assert!(sc.queue.iter().any(|s| s == "M01AMELIA06") || sc.speaking.as_ref().is_some_and(|s| s.0 == "M01AMELIA06"));
}
#[test]
#[ignore = "needs imported assets"]
fn world_init_sets_flags_objective_and_timers_then_world_timer_starts_fleets() {
    let mut s = script();
    let w = vec![unit("RLS Comet", PLAYER, Vec3::ZERO), unit("RLS Sovereign", "Big Ships (Blue Team)", Vec3::X)];
    let (fx, _) = s.step(&w, 0.1, true);
    assert!(s.flags["Player Ships Exists"] && !s.flags["Ships Start Around Archapeligo"]);
    assert_eq!(s.point, Some(0));
    assert!(s.timers.contains_key("World Timer"));
    assert!(fx.iter().any(|f| matches!(f, Fx::Path { path, .. } if path == "Jim Arriving @ Start of Mission")));
    assert!(!s.music.is_empty());
    // timers do not run during the opening briefing: no fleet moves yet
    let (fx, _) = s.step(&w, 47.0, true);
    assert!(!fx.iter().any(|f| matches!(f, Fx::Path { path, .. } if path == "Big Ships (Blue Team)")));
    // the Blue fleet leaves at world timer > 46
    let (fx, _) = s.step(&w, 47.0, false);
    assert!(fx.iter().any(|f| matches!(f, Fx::Path { path, .. } if path == "Big Ships (Blue Team)")));
    assert!(!fx.iter().any(|f| matches!(f, Fx::Path { path, .. } if path == "Small Ships Path Through Archipelago")));
    let (fx, _) = s.step(&w, 5.0, false);
    assert!(fx.iter().any(|f| matches!(f, Fx::Path { path, .. } if path == "Small Ships Path Through Archipelago")));
    assert!(s.flags["Ships Start Around Archapeligo"]);
}
#[test]
#[ignore = "needs imported assets"]
fn alliances_hold_fire_and_run_once() {
    let mut s = script();
    assert!(s.hostile("Pirate Schooner", PLAYER) && !s.hostile(PLAYER, "Big Ships (Blue Team)"));
    let mut out = vec![];
    let w = vec![unit("RLS Comet", PLAYER, Vec3::ZERO)];
    let a =
        serde_json::json!({"Type": "Set alliance between PlayerA and PlayerB to TRUE/FALSE", "PLAYER A": PLAYER, "PLAYER B": "Big Ships (Blue Team)", "SET ALLIANCE TO": "FALSE"});
    s.act(&a, &w, &mut out, "t");
    assert!(s.hostile(PLAYER, "Big Ships (Blue Team)"));
    let a =
        serde_json::json!({"Type": "Set alliance between PlayerA and PlayerB to TRUE/FALSE", "PLAYER A": PLAYER, "PLAYER B": "Big Ships (Blue Team)", "SET ALLIANCE TO": "TRUE"});
    s.act(&a, &w, &mut out, "t");
    assert!(!s.hostile(PLAYER, "Big Ships (Blue Team)"));
}
#[test]
#[ignore = "needs imported assets"]
fn every_mission_loads_and_aliases_the_player_group() {
    for n in 1..=13 {
        let m = crate::campaign::load_id(&format!("mission{n:02}"));
        assert!(!m.script.player().is_empty(), "mission {n}");
        if m.script.player() != PLAYER {
            assert!(!m.data["rules"].to_string().is_empty());
            assert!(m.script.rules.iter().all(|r| !format!("{:?}{:?}", r.conds, r.acts).contains(PLAYER)), "mission {n} keeps the Player0 alias");
        }
    }
}
#[test]
#[ignore = "needs imported assets"]
fn dock_rules_emit_effects_and_read_the_docked_relation() {
    let mut s = script();
    let (mut a, mut b) = (unit("Schooner", "Schooner1", Vec3::ZERO), unit("Camp", "Norville", Vec3::X));
    let mut world = World::new();
    (a.e, b.e) = (world.spawn_empty().id(), world.spawn_empty().id());
    let w = [a.clone(), b.clone()];
    let mut out = vec![];
    s.act(&serde_json::json!({"Type": "Dock Ships", "Docking Group": "Schooner1", "Target to Dock to": "Norville"}), &w, &mut out, "t");
    s.act(&serde_json::json!({"Type": "Set Dock Time", "Docking GroupUnit": "Schooner1", "Dock Time": 300.0}), &w, &mut out, "t");
    assert!(matches!(&out[0], Fx::Dock(d, t) if d == &[a.e] && t == &[b.e]));
    assert!(matches!(&out[1], Fx::DockTime(u, secs) if u == &[a.e] && *secs == 300.0));
    let c = serde_json::json!({"Type": "Group/Unit Is Docked", "Dockers Group/Unit Name": "Schooner1", "Targets Group/Unit Name": "Norville"});
    assert!(!s.cond(&c, &w, false));
    s.set_docked(a.e, b.e, true);
    assert!(s.cond(&c, &w, false));
    s.set_docked(a.e, b.e, false);
    assert!(!s.cond(&c, &w, false));
}
#[test]
#[ignore = "needs imported assets"]
fn boarding_feeds_team_capture_not_the_lifeboat_capture_count() {
    let mut s = crate::campaign::load_id("mission06").script;
    let mut world = World::new();
    let mut thief = unit("The Little Thief", "PirateRunner", Vec3::ZERO);
    thief.e = world.spawn_empty().id();
    let pvp = serde_json::json!({"Type": "Player Vs Player capture count", "Player Name A": "PlayerStart", "Player Name B": "PirateRunner", "Equivalence": "Greater Than", "Number Of Captures": 0});
    let team = serde_json::json!({"Type": "Team has captured a ship from Group/Unit", "Group/Unit": "PirateRunSloop,The Little Thief", "Number Of Captures": 0});
    assert!(!s.cond(&pvp, &[thief.clone()], false) && !s.cond(&team, &[thief.clone()], false));
    // boarding.rs reports a ship capture: the unit now belongs to its captor but still counts as taken from the group
    s.record_capture("PlayerStart", "PirateRunner", thief.e, false);
    thief.owner = "PlayerStart".into();
    thief.groups = vec!["PirateRunSloop".into()];
    assert!(s.cond(&team, &[thief.clone()], false));
    assert!(!s.cond(&pvp, &[thief], false), "boarding does not increment the lifeboat table");
    // the lifeboat route of the same rule: the table counts, exactly (collector, owner), no ally sharing
    s.lifeboat_saved("PlayerStart", "PirateRunner");
    assert!(s.cond(&pvp, &[], false));
    let eq = |n: u32, eq: &str, a: &str| serde_json::json!({"Type": "Player Vs Player capture count", "Player Name A": a, "Player Name B": "PirateRunner", "Equivalence": eq, "Number Of Captures": n});
    assert!(
        s.cond(&eq(1, "Equal To", "PlayerStart"), &[], false)
            && s.cond(&eq(2, "Less Than", "PlayerStart"), &[], false)
            && !s.cond(&eq(1, "Greater Than", "PlayerStart"), &[], false)
    );
    assert!(!s.cond(&eq(0, "Greater Than", "FoundCutter"), &[], false), "an ally's own count is separate");
    assert!(!s.cond(&eq(0, "Greater Than", "Nobody"), &[], false), "an unknown player is false");
    assert!(!s.cond(&eq(1, "Roughly", "PlayerStart"), &[], false), "an unknown comparison is false");
}
#[test]
#[ignore = "needs imported assets"]
fn mission12_defeat_keeps_a_provisional_base_boarding_bridge() {
    let mut m12 = crate::campaign::load_id("mission12").script;
    let e = World::new().spawn_empty().id();
    let procs =
        serde_json::json!({"Type": "Player Vs Player capture count", "Player Name A": "Procs", "Player Name B": "RoyalPalace", "Equivalence": "Equal To", "Number Of Captures": 1});
    assert!(!m12.cond(&procs, &[], false));
    m12.record_capture("Procs2", "RoyalPalace", e, false);
    assert!(!m12.cond(&procs, &[], false), "a boarded ship is not the base bridge");
    // the ironclads belong to Procs2, allied with Procs, and taking the palace (a base) counts as Procs' capture
    m12.record_capture("Procs2", "RoyalPalace", e, true);
    assert!(m12.cond(&procs, &[], false), "an ally's base capture is credited");
    let player = serde_json::json!({"Type": "Player Vs Player capture count", "Player Name A": "PlayerStart", "Player Name B": "RoyalPalace", "Equivalence": "Greater Than", "Number Of Captures": 0});
    assert!(!m12.cond(&player, &[], false), "an enemy's is not");
}
#[test]
#[ignore = "needs imported assets"]
fn player_has_no_lifeboats_follows_the_owners_of_existing_boats() {
    let mut m1 = script();
    let none = serde_json::json!({"Type": "Player has no lifeboats", "Player Name": "Pirate Schooner"});
    assert!(m1.cond(&none, &[], false), "true before any lifeboat exists");
    m1.set_lifeboat_owners(vec!["Pirate Schooner".into(), "Pirate Schooner".into()]);
    assert!(!m1.cond(&none, &[], false));
    assert!(m1.cond(&serde_json::json!({"Type": "Player has no lifeboats", "Player Name": "Small Ships (Red Team)"}), &[], false), "other owners' boats do not count");
    m1.set_lifeboat_owners(vec!["Pirate Schooner".into()]);
    assert!(!m1.cond(&none, &[], false), "one boat is still one");
    m1.set_lifeboat_owners(vec![]);
    assert!(m1.cond(&none, &[], false), "true after the last is collected");
    m1.set_lifeboat_owners(vec!["Nobody".into()]);
    assert!(m1.cond(&serde_json::json!({"Type": "Player has no lifeboats", "Player Name": "Nobody"}), &[], false), "an unknown player is true");
}
#[test]
#[ignore = "needs imported assets"]
fn local_player_scores_only_for_boats_of_players_it_is_not_allied_with() {
    let mut m1 = script(); // player "Player0 Group": allied with 'Navy Tenders', hostile to 'Pirate Schooner'
    m1.lifeboat_saved(PLAYER, "Pirate Schooner");
    assert_eq!(m1.score, 2);
    m1.lifeboat_saved(PLAYER, "Navy Tenders");
    m1.lifeboat_saved(PLAYER, PLAYER);
    m1.lifeboat_saved("Navy Tenders", "Pirate Schooner");
    assert_eq!(m1.score, 2, "allied, own and AI collections score nothing");
    assert_eq!(
        (m1.lifeboats_saved_by(PLAYER, "Pirate Schooner"), m1.lifeboats_saved_by("Navy Tenders", "Pirate Schooner"), m1.lifeboats_saved_by(PLAYER, PLAYER)),
        (1, 1, 1),
        "but the table counts them"
    );
}
#[test]
#[ignore = "needs imported assets"]
fn set_lifeboat_creation_state_targets_the_named_ships() {
    let mut s = script();
    let mut world = World::new();
    let mut warrior = unit("Warrior", "Pirate Schooner", Vec3::ZERO);
    let mut other = unit("RLS Explorer", "Small Ships (Red Team)", Vec3::X);
    warrior.e = world.spawn_empty().id();
    other.e = world.spawn_empty().id();
    let w = [warrior.clone(), other.clone()];
    let mut out = vec![];
    s.act(&serde_json::json!({"Type": "Set Lifeboat Creation State", "Group Name": "Pirate Schooner,Warrior", "Boolean": "TRUE"}), &w, &mut out, "t");
    s.act(&serde_json::json!({"Type": "Set Lifeboat Creation State", "Group Name": "Small Ships (Red Team)", "Boolean": "FALSE"}), &w, &mut out, "t");
    assert!(matches!(&out[0], Fx::Lifeboats(u, true) if u == &[warrior.e]));
    assert!(matches!(&out[1], Fx::Lifeboats(u, false) if u == &[other.e]));
}
#[test]
#[ignore = "needs imported assets"]
fn skip_to_tender_then_volume_and_dock_progress_the_mission() {
    let mut s = script();
    let mut world = World::new();
    let mut w = vec![
        unit("RLS Comet", PLAYER, Vec3::new(975.0, 0.0, -608.0)),
        unit("RLS Explorer", "Small Ships (Red Team)", Vec3::new(905.0, 0.0, -600.0)),
        unit("RLS Bounty", "Navy Tenders", Vec3::new(1076.0, 0.0, -608.0)),
    ];
    for u in &mut w {
        u.e = world.spawn_empty().id();
    }
    s.step(&w, 0.1, true); // world init, as during the opening
    s.skip_to_tender();
    let (fx, _) = s.step(&w, 0.1, false);
    assert!(!fx.iter().any(|f| matches!(f, Fx::Path { path, .. } if path.contains("Archipelago"))), "earlier story rules stay skipped");
    assert!(s.task_active("Dock With Tender") && s.played.is_empty() && s.queue.contains(&"M01JIM00E".to_string()));
    s.set_docked(w[0].e, w[2].e, true);
    s.step(&w, 0.1, false);
    assert_eq!(s.flags.get("Player Has Docked w/ Weapons Tender"), Some(&true));
    assert!(s.queue.contains(&"M01AMELIA12".to_string()));
    assert!(!s.task_active("Dock With Tender") && s.task_active("Destroy Asteroids"));
}
#[test]
#[ignore = "needs imported assets"]
fn destroyed_exit_and_stopped_timer_rules() {
    let mut s = script();
    let mut dead = unit("Ravage", "Pirates", Vec3::ZERO);
    dead.alive = false;
    let c = serde_json::json!({"Type": "Group Destroyed", "Group Name": "Pirates"});
    assert!(s.cond(&c, &[dead], false));
    assert!(!s.cond(&c, &[unit("Ravage", "Pirates", Vec3::ZERO)], false));
    let mut out = vec![];
    s.act(&serde_json::json!({"Type": "Start Timer", "Timer Name": "T"}), &[], &mut out, "t");
    s.act(&serde_json::json!({"Type": "Stop Timer", "Timer Name": "T"}), &[], &mut out, "t");
    s.step(&[], 5.0, false);
    assert_eq!(s.timer("T"), Some(0.0));
}
#[test]
#[ignore = "needs imported assets"]
fn volumes_timers_and_speech_conditions() {
    let mut s = script();
    let vol = s.volumes["Meet @ Clarke"].clone();
    let pts: Vec<(f32, f32)> = vol.0.iter().map(|v| (v.x, v.y)).collect();
    let (x0, x1) = (pts.iter().map(|p| p.0).fold(f32::MAX, f32::min), pts.iter().map(|p| p.0).fold(f32::MIN, f32::max));
    let (y0, y1) = (pts.iter().map(|p| p.1).fold(f32::MAX, f32::min), pts.iter().map(|p| p.1).fold(f32::MIN, f32::max));
    let inner = (0..20)
        .flat_map(|i| (0..20).map(move |j| (i, j)))
        .map(|(i, j)| Vec3::new(x0 + (x1 - x0) * (i as f32 + 0.5) / 20.0, 0.0, -(y0 + (y1 - y0) * (j as f32 + 0.5) / 20.0)))
        .find(|p| in_volume(&vol, *p))
        .expect("a point inside the volume");
    let w = vec![unit("RLS Comet", PLAYER, inner)];
    let c = serde_json::json!({"Type": "Enter Volume", "Group Name": "Player0 Group,HUMAN CONTROLLED COMMAND SHIP", "Volume Name": "Meet @ Clarke"});
    assert!(s.cond(&c, &w, false));
    assert!(!s.cond(&c, &[unit("RLS Comet", PLAYER, Vec3::splat(9999.0))], false));
    let t = serde_json::json!({"Type": "Timer Condition", "Timer Name": "T", "Equivalence": "Greater Than", "Time in seconds": 2});
    assert!(!s.cond(&t, &w, false));
    s.timers.insert("T".into(), 3.0);
    assert!(s.cond(&t, &w, false));
    let sp = serde_json::json!({"Type": "Speech Event Played Once", "Speech Event Name": "M01AMELIA04"});
    assert!(!s.cond(&sp, &w, false));
    s.queue_speech("M01AMELIA04");
    let (_, v) = s.step(&w, 0.1, false);
    assert_eq!(v.as_deref(), Some("M01AMELIA04"));
    s.step(&w, 60.0, false);
    assert!(s.cond(&sp, &w, false));
}
#[test]
#[ignore = "needs imported assets"]
fn speech_events_play_their_twt_file_and_lines_without_audio_still_complete() {
    let mut s = script();
    let w = vec![unit("RLS Comet", PLAYER, Vec3::ZERO)];
    // The TWT speech list maps event M01AMELIA13A to sound M01AMELIA13b (and its own talking head).
    s.queue_speech("M01AMELIA13A");
    let (_, v) = s.step(&w, 0.1, false);
    assert_eq!(v.as_deref(), Some("M01AMELIA13b"));
    assert_eq!(s.face(), Some("TalkingHead_Character_Amelia_Angry"));
    s.step(&w, 30.0, false);
    assert!(s.played.contains("M01AMELIA13A"));
    s.media["speech"]["Silent"] = serde_json::json!({"text": "No file.", "duration": null});
    s.queue.clear(); // rules fired by the steps above may have queued lines
    s.speaking = None;
    s.queue_speech("Silent");
    let (_, v) = s.step(&w, 0.1, false);
    assert!(v.is_none() && s.subtitle == "No file.");
    s.step(&w, 30.0, false);
    assert!(s.played.contains("Silent"));
}

#[test]
#[ignore = "needs imported assets"]
fn twt_groups_alliances_and_setup_stances_drive_later_missions() {
    let m = crate::campaign::load_id("mission03");
    let sc = m.script;
    let mut world = World::new();
    // Ships 247 and 362 form the TWT group 'Pirate Barque & Sloop' but are owned by the 'Pirate Silver' player.
    let mut w: Vec<Unit> = [247, 362, 44].into_iter().map(|id| Unit { id, e: world.spawn_empty().id(), ..unit("x", "Pirate Silver", Vec3::ZERO) }).collect();
    sc.tag(&mut w);
    assert_eq!(members(&w, "Pirate Barque & Sloop").len(), 2);
    assert_eq!(members(&w, "Pirate Silver").len(), 1, "the named group must shadow the same-named player");
    assert!(!sc.answers(247, "Pirate Silver", "x", false, "Pirate Silver"));
    assert!(sc.answers(44, "Pirate Silver", "x", false, "Pirate Silver"));
    w.retain(|u| u.id != 44);
    assert!(members(&w, "Pirate Silver").is_empty(), "an absent group must not fall back to all its owner's ships");
    w[0].owner = "Players Ship".into();
    sc.tag(&mut w);
    assert_eq!(members(&w, "Players Ship").len(), 1, "player selectors without a named group still follow ownership");
    // PlayerAllianceInfoVector: only 'Players Ship' and 'Navy' are allied; non-players are never hostile.
    assert!(sc.hostile("Players Ship", "Pirate Silver"));
    assert!(!sc.hostile("Players Ship", "Navy"));
    assert!(!sc.hostile("Players Ship", "NO PLAYER"));
    // '*State Init* Setup Ship' applies its AI stance to the placed ship (mission 2's SS Lady J is a dummy).
    let m2 = crate::campaign::load_id("mission02");
    let mut sc2 = m2.script;
    let lady = Unit { id: 61, e: world.spawn_empty().id(), ..unit("SS Lady J", "Schooner2", Vec3::ZERO) };
    let (fx, _) = sc2.step(std::slice::from_ref(&lady), 0.1, false);
    assert!(fx.iter().any(|f| matches!(f, Fx::Stance(u, Stance::Dummy) if u == &[lady.e])));
}

#[test]
#[ignore = "needs imported assets"]
fn volume_entry_and_exit_track_transitions() {
    let mut sc = script();
    sc.volumes = HashMap::from([("V".to_string(), Region::from_json(&serde_json::json!([[0, 0], [10, 0], [10, 10], [0, 10]])))]);
    let mut w = vec![unit("a", "G", Vec3::new(50.0, 0.0, -5.0))];
    let enter = serde_json::json!({"Type": "Enter Volume", "Group Name": "G", "Volume Name": "V"});
    let exit = serde_json::json!({"Type": "Exit Volume", "Group Name": "G", "Volume Name": "V"});
    assert!(!sc.cond(&enter, &w, true));
    assert!(!sc.cond(&exit, &w, true));
    w[0].pos.x = 5.0;
    assert!(sc.cond(&enter, &w, true));
    assert!(!sc.cond(&enter, &w, true));
    assert!(!sc.cond(&exit, &w, true));
    w[0].pos.x = 50.0;
    assert!(sc.cond(&exit, &w, true));
    assert!(!sc.cond(&exit, &w, true));
}

#[test]
#[ignore = "needs imported assets"]
fn all_units_entered_once_remembers_members_that_left_again() {
    let mut sc = script();
    sc.volumes = HashMap::from([("V".to_string(), Region::from_json(&serde_json::json!([[0, 0], [10, 0], [10, 10], [0, 10]])))]);
    let mut world = World::new();
    let mut w = vec![unit("a", "G", Vec3::new(5.0, 0.0, -5.0)), unit("b", "G", Vec3::new(50.0, 0.0, -5.0))];
    w[0].e = world.spawn_empty().id();
    w[1].e = world.spawn_empty().id();
    let c = serde_json::json!({"Type": "All Units from Group have entered trigger volume Once", "Group Name": "G", "Volume Name": "V"});
    assert!(!sc.cond(&c, &w, false));
    w[0].pos.x = 50.0; // a leaves, b enters
    w[1].pos.x = 5.0;
    assert!(sc.cond(&c, &w, false));
}

/// Every rule type of missions 2-13 is interpreted except the features still missing (docs/re/campaign.md "Missions 2-13").
#[test]
#[ignore = "needs imported assets"]
fn later_missions_only_warn_about_known_missing_features() {
    let mut warned = std::collections::BTreeSet::new();
    for id in (2..=13).map(|n| format!("mission{n:02}")).chain(["tutorial".into(), "basic_tutorial".into()]) {
        let m = crate::campaign::load_id(&id);
        let mut s = m.script;
        let w = vec![unit("x", PLAYER, Vec3::ZERO)];
        for r in m.data["rules"].as_array().unwrap() {
            for c in r["conditions"].as_array().unwrap() {
                s.cond(c, &w, false);
            }
            for a in r["actions"].as_array().unwrap() {
                s.act(a, &w, &mut vec![], "");
            }
        }
        warned.extend(s.warned.into_iter().map(|x| x.split(" in rule").next().unwrap().to_string()));
    }
    let known: [&str; 0] = [];
    let unknown: Vec<_> = warned.iter().filter(|x| !known.contains(&x.as_str()) && !x.starts_with("ship name")).collect();
    assert!(unknown.is_empty(), "unhandled: {unknown:?}");
}

#[test]
#[ignore = "needs imported assets"]
fn event_effects_and_special_effects_are_forwarded() {
    let mut s = crate::campaign::load_id("mission02").script;
    let w = vec![unit("x", PLAYER, Vec3::ZERO)];
    let mut out = vec![];
    s.act(&serde_json::json!({"Type": "Create/Release Event Effect", "Point Set": "Moonfish", "Effect Name": "MoonFish", "Boolean State": "TRUE"}), &w, &mut out, "");
    s.act(&serde_json::json!({"Type": "Play Special Effect", "File Name": "BottleRocket_Pickup", "Play As Dialog": "FALSE"}), &w, &mut out, "");
    assert!(matches!(&out[0], Fx::Event(set, name, true) if set == "Moonfish" && name == "MoonFish"));
    assert!(matches!(&out[1], Fx::Sound(n) if n == "BottleRocket_Pickup"));
    assert!(crate::assets_dir().join(crate::audio::sound("BottleRocket_Pickup")).is_file());
    s.act(&serde_json::json!({"Type": "Open HUD Texture Overlay", "Texture Base Name": "Credit01"}), &w, &mut out, "");
    assert_eq!(s.overlay.as_deref(), Some("credit01"));
    assert!(crate::assets_dir().join("hud/overlay/credit01.png").is_file());
    s.act(&serde_json::json!({"Type": "Close HUD Texture Overlay"}), &w, &mut out, "");
    assert_eq!(s.overlay, None);
}

#[test]
fn polygons_skip_malformed_vertices_and_unknown_volumes_contain_nothing() {
    let square = Region::from_json(&serde_json::json!([[0, 0], [10, 0], "junk", [10, 10], [0], [0, 10]]));
    assert_eq!(square.len(), 4);
    assert!(square.contains(Vec2::new(5.0, 5.0)) && !square.contains(Vec2::new(15.0, 5.0)));
    assert_eq!(square.centroid(), Some(Vec2::new(5.0, 5.0)));
    assert!(Region::from_json(&Value::Null).centroid().is_none());
    let sc = bare_script(serde_json::json!({"Zone": [[0, 0], [10, 0], [10, 10], [0, 10], [null, 3]]}), Value::Null);
    assert!(sc.volume("No Such Volume").is_empty());
    assert!(leash(&sc, "No Such Volume", None, Vec3::new(1e6, 0.0, 0.0)).is_none());
    assert!(leash(&sc, "Zone", None, Vec3::new(5.0, 2.0, -5.0)).is_none(), "inside its zone");
    assert_eq!(leash(&sc, "Zone", None, Vec3::new(50.0, 2.0, -5.0)), Some(Vec3::new(5.0, 2.0, -5.0)), "outside: back to the centre");
}
#[test]
#[ignore = "needs imported assets"]
fn full_tutorial_camera_objective_clears_by_timer_or_by_leaving_the_start_area() {
    // No rule watches the camera: 'Try Out Camera' ends 30 s after line 03D, or when the ship leaves 'Player Wander @ Start'.
    for leave in [false, true] {
        let mut sc = crate::campaign::load_id("tutorial").script;
        let mut p = unit("RLS Intrepid", sc.player(), Vec3::new(-1316.7, 0.0, 1407.8)); // the start, in Bevy axes
        p.groups = vec![sc.player().to_string()];
        p.primary = true;
        let mut units = [p];
        let active = |sc: &Script| sc.tasks.iter().any(|t| t.active && t.name == "Try Out Camera");
        let mut seen = false;
        for i in 0..1000 {
            if leave && seen {
                units[0].pos.x = -1200.0;
            }
            sc.step(&units, 0.25, false);
            seen |= active(&sc);
            if seen && sc.played.contains("MTUTAMELIA04") {
                assert!(!active(&sc));
                assert_eq!(sc.played.contains("MTUTAMELIA03E"), leave, "leave={leave} at t={}", i as f32 * 0.25);
                break;
            }
            assert!(i < 999, "tutorial stuck (leave={leave})");
        }
    }
}

#[test]
fn never_fired_lists_rules_that_did_not_fire_once_repeating_ones_included() {
    let timer = serde_json::json!({"Type": "Timer Condition", "Timer Name": "never started", "Equivalence": ">", "Time in seconds": 1});
    let data = serde_json::json!({"rules": [
        {"name": "Every frame", "Run Once": false, "conditions": [], "actions": []},
        {"name": "Waits ", "conditions": [timer], "actions": []},
        {"name": "Opening NIS 1", "conditions": [timer], "actions": []},
    ], "ships": [], "volumes": null, "environment": {}});
    let mut sc = Script::new(&data, Value::Null);
    assert_eq!(sc.never_fired(), ["Every frame", "Waits"]);
    sc.step(&[], 0.1, false);
    assert_eq!(sc.never_fired(), ["Waits"]);
}

/// A kill is an event: a repeating rule scores once per kill, by the kind of object destroyed (mission 1's rocks, mission 3's ships).
#[test]
fn killing_an_object_scores_once_per_kill_and_by_kind() {
    let data = |kind: &str| {
        serde_json::json!({"rules": [{"name": "points", "Run Once": false, "Is Active": true,
            "conditions": [{"Type": "Player Killed A Object", "Player Name": PLAYER, "World Object Type": kind}],
            "actions": [{"Type": "Add Victory Points for SinglePlayer", "Victory points to be added": 1}]}]})
    };
    let w = vec![unit("RLS Comet", PLAYER, Vec3::ZERO)];
    for (kind, rock) in [("Asteroid", true), ("Ship", false)] {
        let mut sc = Script::new(&data(kind), serde_json::Value::Null);
        sc.step(&w, 0.1, false);
        assert_eq!(sc.score, 0, "{kind}: nothing destroyed yet");
        sc.kills.insert((PLAYER.into(), rock), 1);
        sc.kills.insert((PLAYER.into(), !rock), 5); // the other kind does not count
        for _ in 0..3 {
            sc.step(&w, 0.1, false);
        }
        assert_eq!(sc.score, 1, "{kind}: one kill scores once");
        *sc.kills.get_mut(&(PLAYER.into(), rock)).unwrap() += 1;
        sc.step(&w, 0.1, false);
        sc.step(&w, 0.1, false);
        assert_eq!(sc.score, 2, "{kind}: the next kill scores again");
    }
}

#[test]
fn subtitle_colour_is_the_speech_events_own() {
    let mut sc = Script::new(&serde_json::json!({"rules": []}), serde_json::json!({"speech": {"Hello": {"color": [0.5, 0.25, 1.0, 1.0]}, "Plain": {}}}));
    assert_eq!(sc.speech_colour(), None, "nobody speaks");
    sc.speaking = Some(("Hello".into(), 1.0));
    assert_eq!(sc.speech_colour(), Some([0.5, 0.25, 1.0, 1.0]));
    sc.speaking = Some(("Plain".into(), 1.0));
    assert_eq!(sc.speech_colour(), None, "an event without a colour keeps the default gold");
}

/// Mousetrap: the Procyon supply convoy holds until a pirate ship enters the repair area ('Team Member Enters Volume').
#[test]
fn a_team_member_in_the_volume_fires_for_that_team_only() {
    let mut sc = bare_script(serde_json::json!({"area": [[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]]}), Value::Null);
    sc.set_teams(vec![("PIRATES".into(), vec!["pirate".into()]), ("PROCYON".into(), vec!["procyon".into()])]);
    let cond = serde_json::json!({"Type": "Team Member Enters Volume", "Team Name": "PIRATES", "Volume Name": "area"});
    let outside = [unit("a", "pirate", Vec3::new(500.0, 0.0, 500.0)), unit("b", "procyon", Vec3::ZERO)];
    assert!(!sc.cond(&cond, &outside, false), "only a Procyon ship is inside");
    let inside = [unit("a", "pirate", Vec3::ZERO)];
    assert!(sc.cond(&cond, &inside, false));
    let mut dead = unit("a", "pirate", Vec3::ZERO);
    dead.alive = false;
    assert!(!sc.cond(&cond, &[dead], false), "a sunk ship does not count");
}
