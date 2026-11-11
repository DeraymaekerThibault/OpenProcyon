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
    sc.briefing_opening = true; // mission 1: briefing.rs plays the opening rules
    assert_eq!(sc.never_fired(), ["Every frame", "Waits"]);
    sc.step(&[], 0.1, false);
    assert_eq!(sc.never_fired(), ["Waits"]);
}

/// Only mission 1's opening rules are replaced by briefing.rs. Mission 8's "Opening NISb" (a World timer rule that plays the line ending
/// its opening cutscene) is an ordinary rule: skipping it left the mission in that cutscene for good.
#[test]
fn opening_nis_rules_run_in_missions_without_the_scripted_briefing() {
    let data = serde_json::json!({"rules": [
        {"name": "Init", "conditions": [{"Type": "World Initialize"}], "actions": [{"Type": "Start Timer", "Timer Name": "World timer"}]},
        {"name": "Opening NISb", "conditions": [{"Type": "Timer Condition", "Timer Name": "World timer", "Equivalence": "Greater Than", "Time in seconds": 1}],
         "actions": [{"Type": "Set Flag Action", "Flag Name": "spoken", "Boolean Value": "TRUE"}]},
    ], "ships": [], "volumes": null, "environment": {}});
    for briefing in [false, true] {
        let mut sc = Script::new(&data, Value::Null);
        sc.briefing_opening = briefing;
        for _ in 0..30 {
            sc.step(&[], 0.1, false);
        }
        assert_eq!(sc.flags.get("spoken").copied().unwrap_or(false), !briefing, "briefing_opening={briefing}");
    }
}

const WIN: &str = r#"[{"Type": "Goto Next Level", "World": "Mission02.twt"}]"#;
const LOSE: &str = r#"[{"Type": "End Game"}]"#;

/// A synthetic rule: `conds` and `acts` as JSON text, run once.
fn rule(name: &str, conds: &str, acts: &str) -> Value {
    serde_json::json!({"name": name, "conditions": serde_json::from_str::<Value>(conds).unwrap(), "actions": serde_json::from_str::<Value>(acts).unwrap()})
}
fn set_flag(name: &str) -> String {
    format!(r#"[{{"Type": "Set Flag Action", "Flag Name": "{name}", "Boolean Value": "TRUE"}}]"#)
}
fn flag(name: &str) -> String {
    format!(r#"{{"Type": "Flag Condition", "Flag Name": "{name}", "Boolean Value": "TRUE"}}"#)
}
fn enter(group: &str, vol: &str) -> String {
    format!(r#"{{"Type": "Enter Volume", "Group Name": "{group}", "Volume Name": "{vol}"}}"#)
}
const ME: &str = "Player0 Group,HUMAN CONTROLLED COMMAND SHIP";
fn square(x: f32) -> Value {
    serde_json::json!([[x, 100.0], [x + 100.0, 100.0], [x + 100.0, 200.0], [x, 200.0]])
}
fn id(n: u32) -> Entity {
    Entity::from_raw_u32(n).unwrap()
}

/// Rules that can lead to the win are critical, with their hops to it; those that can only lead to a loss are hazards; the rest are
/// optional. A rule on both counts as critical. Names mean nothing.
#[test]
fn rule_graph_marks_critical_hazard_and_optional_rules_by_chaining_back_from_the_endings() {
    let data = serde_json::json!({"rules": [
        rule("a", &format!("[{}]", enter(ME, "V")), &set_flag("f")),
        rule("b", &format!("[{}]", flag("f")), WIN),
        rule("c", &format!("[{}]", enter(ME, "V")), &set_flag("g")),
        rule("d", &format!("[{}]", flag("g")), LOSE),
        rule("e", &format!("[{}]", enter(ME, "V")), &set_flag("h")),
        rule("both", &format!("[{}]", enter(ME, "V")), r#"[{"Type": "Set Flag Action", "Flag Name": "f", "Boolean Value": "TRUE"}, {"Type": "Set Flag Action", "Flag Name": "g", "Boolean Value": "TRUE"}]"#),
        rule("via speech", &format!("[{}]", flag("s")), &set_flag("f")),
        rule("speaks", r#"[{"Type": "World Initialize"}]"#, r#"[{"Type": "Play speech event", "Speech Event Name": "line"}, {"Type": "Set Flag Action", "Flag Name": "s", "Boolean Value": "TRUE"}]"#),
    ], "ships": [], "volumes": {"V": square(100.0)}, "environment": {}});
    let sc = Script::new(&data, Value::Null);
    let roles: Vec<_> = (0..8).map(|i| sc.role(i)).collect();
    use Role::*;
    assert_eq!(roles, [Critical(1), Critical(0), Hazard(1), Hazard(0), Optional, Critical(1), Critical(1), Critical(2)]);
}

/// What the bot wants comes from critical rules whose gates hold, nearest to the win first; optional rules only when no critical rule
/// yields a want; hazard rules never.
#[test]
fn plan_prefers_critical_wants_over_optional_ones_and_never_follows_hazards() {
    let data = serde_json::json!({"rules": [
        rule("far step", &format!("[{}]", enter(ME, "Far")), &set_flag("b")),
        rule("near step", &format!("[{}]", enter(ME, "Near")), &set_flag("a")),
        rule("win", &format!("[{}, {}]", flag("a"), flag("b")), WIN),
        rule("deeper", &format!("[{}]", enter(ME, "Deep")), &set_flag("c")),
        rule("second step", &format!("[{}, {}]", flag("c"), enter(ME, "Win")), &set_flag("a")),
        rule("side", &format!("[{}]", enter(ME, "Side")), &set_flag("z")),
        rule("trap", &format!("[{}]", enter(ME, "Trap")), &set_flag("t")),
        rule("lose", &format!("[{}]", flag("t")), LOSE),
        rule("gated", &format!("[{}, {}]", flag("never"), enter(ME, "Gate")), &set_flag("q")),
    ], "ships": [], "volumes": {"Near": square(100.0), "Far": square(2000.0), "Deep": square(300.0), "Win": square(500.0), "Side": square(-900.0), "Trap": square(-1900.0), "Gate": square(900.0)}, "environment": {}});
    let mut sc = Script::new(&data, Value::Null);
    let w = vec![unit("RLS Comet", PLAYER, Vec3::new(0.0, 0.0, 0.0))];
    let centre = |x: f32| Want::Reach(Vec3::new(x + 50.0, 0.0, -150.0));
    let got: Vec<_> = sc.plan(&w).iter().map(|x| (x.want, x.critical, x.dist)).collect();
    // Near step and far step are one hop from the win, deeper is two hops (it enables second step, which enables the win through a):
    // nearest to the win first, then nearest to the player. "second step" waits for flag c, so only "deeper" is open. The side rule is
    // optional, the trap a hazard, the gated one waits for a flag nobody provides: none is wanted.
    assert_eq!(got, [(centre(100.0), true, 1), (centre(2000.0), true, 1), (centre(300.0), true, 2)]);
    // With the critical steps done, the optional side rule is all that is left to want.
    for i in [0, 1, 3, 4] {
        sc.fired[i] = true;
    }
    let got: Vec<_> = sc.plan(&w).iter().map(|x| (x.want, x.critical)).collect();
    assert_eq!(got, [(centre(-900.0), false)]);
}

/// Each condition type the bot turns into something to do, and the ones it cannot (`no-plan`).
#[test]
fn plan_turns_world_conditions_into_wants_and_reports_the_rest_as_no_plan() {
    let cond = |text: &str| -> Value { serde_json::from_str(text).unwrap() };
    let docked = cond(&format!(r#"{{"Type": "Group/Unit Docked", "Dockers Group/Unit Name": "{ME}", "Targets Group/Unit Name": "Camp"}}"#));
    let tow = cond(&format!(r#"{{"Type": "Is Ship In Tow", "Towing Group": "{ME}", "Target to Tow": "Convoy", "Boolean": "TRUE"}}"#));
    let shots = cond(&format!(r#"{{"Type": "Group/Unit Fired X Shots", "Group/Unit Name": "{ME}", "Equivalence": "Greater Than", "Number Of Rounds Fired": 3}}"#));
    let near = cond(&format!(r#"{{"Type": "Group to Group Distance", "GroupA Name": "{ME}", "GroupB Name": "Foe", "Equivalence": "Less Than", "Distance": 50}}"#));
    let away = cond(&format!(r#"{{"Type": "Group to Group Distance", "GroupA Name": "{ME}", "GroupB Name": "Foe", "Equivalence": "Greater Than", "Distance": 5000}}"#));
    let dead = cond(r#"{"Type": "Group Destroyed", "Group Name": "Foe"}"#);
    let boats = cond(&format!(
        r#"{{"Type": "Player Vs Player capture count", "Player Name A": "{PLAYER}", "Player Name B": "Foe", "Equivalence": "Greater Than", "Number Of Captures": 0}}"#
    ));
    let rules: Vec<Value> = [docked, tow, shots, near, away, dead, boats]
        .into_iter()
        .enumerate()
        .map(|(i, c)| serde_json::json!({"name": format!("r{i}"), "conditions": [c], "actions": serde_json::from_str::<Value>(WIN).unwrap()}))
        .collect();
    let data = serde_json::json!({"rules": rules, "ships": [], "volumes": null, "environment": {}});
    let mut sc = Script::new(&data, Value::Null);
    let mut foe = unit("Pirate", "Foe", Vec3::new(400.0, 0.0, 0.0));
    foe.e = id(2);
    let mut camp = unit("Camp", "Camp", Vec3::new(0.0, 0.0, 300.0));
    camp.e = id(3);
    let mut convoy = unit("Convoy", "Convoy", Vec3::new(0.0, 0.0, 600.0));
    convoy.e = id(4);
    let mut me = unit("RLS Comet", PLAYER, Vec3::ZERO);
    me.e = id(1);
    let w = vec![me, foe, camp, convoy];
    sc.hostile.insert(("Foe".into(), PLAYER.into()));
    let wants: Vec<Want> = sc.plan(&w).iter().map(|x| x.want).collect();
    for expected in [Want::Dock(id(3)), Want::Tow(id(4)), Want::Reach(Vec3::new(400.0, 0.0, 0.0)), Want::Leave(Vec3::new(400.0, 0.0, 0.0)), Want::Hit(id(2)), Want::Lifeboats] {
        assert!(wants.contains(&expected), "{expected:?} in {wants:?}");
    }
    // The shots condition has no plan: the report says so on the false condition.
    let report = sc.frontier_report(&w);
    let flagged: Vec<_> = report.iter().filter(|r| r.conds.iter().any(|c| c.no_plan)).map(|r| r.name.as_str()).collect();
    assert_eq!(flagged, ["r2"]);
    assert!(report.iter().all(|r| r.role == Role::Critical(0)));
}

/// The hazard rules' world conditions become constraints: a group whose loss ends the mission is protected, a volume the player must
/// not enter is avoided, and a timer is only reported.
#[test]
fn hazard_rules_give_groups_to_protect_volumes_to_avoid_and_timers_to_report() {
    let data = serde_json::json!({"rules": [
        rule("win", &format!("[{}]", flag("a")), WIN),
        rule("step", &format!("[{}]", enter(ME, "Goal")), &set_flag("a")),
        rule("lose convoy", r#"[{"Type": "Group Destroyed", "Group Name": "Convoy"}]"#, LOSE),
        rule("lose trap", &format!("[{}]", enter(ME, "Trap")), LOSE),
        rule("lose late", r#"[{"Type": "Timer Condition", "Timer Name": "clock", "Equivalence": "Greater Than", "Time in seconds": 600}]"#, LOSE),
        rule("start clock", r#"[{"Type": "World Initialize"}]"#, r#"[{"Type": "Start Timer", "Timer Name": "clock"}]"#),
    ], "ships": [], "volumes": {"Goal": square(2000.0), "Trap": square(100.0)}, "environment": {}});
    let mut sc = Script::new(&data, Value::Null);
    let mut me = unit("RLS Comet", PLAYER, Vec3::new(-500.0, 0.0, 0.0));
    me.e = id(1);
    let mut convoy = unit("Barge", "Convoy", Vec3::new(1000.0, 0.0, 0.0));
    convoy.e = id(2);
    let mut raider = unit("Pirate", "Foe", Vec3::new(1300.0, 0.0, 0.0));
    raider.e = id(3);
    let mut far = unit("Pirate", "Foe", Vec3::new(-3000.0, 0.0, 0.0));
    far.e = id(4);
    let w = vec![me.clone(), convoy, raider, far];
    sc.hostile.insert(("Foe".into(), "Convoy".into()));
    sc.hostile.insert(("Foe".into(), PLAYER.into()));
    assert_eq!(sc.protected_units(&w, &me).iter().map(|u| u.e).collect::<Vec<_>>(), [id(2)]);
    assert_eq!(sc.raider(&w, &me, |_| true).map(|u| u.e), Some(id(3)), "the pirate within reach of the convoy, not the far one");
    assert!(sc.avoided(&w, &me, Vec3::new(150.0, 0.0, -150.0)) && !sc.avoided(&w, &me, Vec3::new(2050.0, 0.0, -150.0)));
    assert!(sc.hazards_text().contains("clock Greater Than 600s"), "{}", sc.hazards_text());
    // A hazard rule is never wanted, whatever its conditions ask.
    assert!(sc.plan(&w).iter().all(|x| x.rule == 1));
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

/// Skirmish star map: a team's objective is its point's position and its task's text; 'NO OBJECTIVE POINT' has no position.
#[test]
fn team_objectives_resolve_to_a_point_and_a_task_text() {
    let data = serde_json::json!({
        "rules": [], "ships": [], "volumes": {}, "environment": {},
        "points": [{"Name": "navy target", "Position": [1.0, 2.0, 0.0]}],
        "tasks": [{"Name": "navy obj", "text": "Destroy Enemy Base.", "Active": false}, {"Name": "pirate obj", "text": "Survive", "Active": false}]
    });
    let mut sc = Script::new(&data, Value::Null);
    sc.set_teams(vec![("NAVY".into(), vec!["navy".into()]), ("PIRATES".into(), vec!["pirate".into()])]);
    let mut out = vec![];
    let w: Vec<Unit> = vec![];
    for (team, point, task) in [("NAVY", "navy target", "navy obj"), ("PIRATES", "NO OBJECTIVE POINT", "pirate obj")] {
        sc.act(&serde_json::json!({"Type": "Setup Team Objective", "Team Name": team, "Objective Point": point, "Objective Task": task}), &w, &mut out, "setup");
    }
    assert_eq!(sc.team_of("navy"), Some("NAVY"));
    let (position, text) = sc.team_objective("NAVY").unwrap();
    assert_eq!((position.unwrap()[0].as_f64(), text.as_deref()), (Some(1.0), Some("Destroy Enemy Base.")));
    let (position, text) = sc.team_objective("PIRATES").unwrap();
    assert!(position.is_none(), "no objective point, no ring");
    assert_eq!(text.as_deref(), Some("Survive"));
    // A later setup keeps older tasks active, leaves the point on an unknown name and clears it for NO OBJECTIVE POINT.
    sc.act(&serde_json::json!({"Type": "Setup Team Objective", "Team Name": "NAVY", "Objective Point": "nowhere", "Objective Task": "pirate obj"}), &w, &mut out, "setup");
    let (position, text) = sc.team_objective("NAVY").unwrap();
    assert_eq!(position.unwrap()[0].as_f64(), Some(1.0));
    assert_eq!(text.as_deref(), Some("Destroy Enemy Base.\nSurvive"));
    sc.act(&serde_json::json!({"Type": "Setup Team Objective", "Team Name": "NAVY", "Objective Point": "NO OBJECTIVE POINT", "Objective Task": "navy obj"}), &w, &mut out, "setup");
    assert!(sc.team_objective("NAVY").unwrap().0.is_none());
}

/// Synthetic mission for the stall report: a loss rule, presentation rules and story rules, some behind a flag gate. Names say
/// nothing about what the rules do (classification reads the actions).
fn stall_data() -> Value {
    let vol = serde_json::json!([[100.0, 100.0], [200.0, 100.0], [200.0, 200.0], [100.0, 200.0]]);
    let flag = |on: bool| serde_json::json!({"Type": "Flag Condition", "Flag Name": "go", "Boolean Value": if on { "TRUE" } else { "FALSE" }});
    let enter = serde_json::json!({"Type": "Enter Volume", "Group Name": "Player0 Group,HUMAN CONTROLLED COMMAND SHIP", "Volume Name": "Search"});
    let set = serde_json::json!([{"Type": "Set Flag Action", "Flag Name": "found", "Boolean Value": "TRUE"}]);
    let speak = serde_json::json!([{"Type": "Play speech event", "Speech Event Name": "x"}, {"Type": "Add Victory Points for SinglePlayer", "Victory points to be added": 5}]);
    serde_json::json!({"rules": [
        {"name": "Open story", "conditions": [flag(true), enter], "actions": set},
        {"name": "Gated story", "conditions": [flag(false), enter], "actions": set},
        {"name": "FAIL Mission", "conditions": [flag(true), enter], "actions": [{"Type": "Set Flag Action", "Flag Name": "found", "Boolean Value": "TRUE"}, {"Type": "End Game"}]},
        {"name": "Open win", "conditions": [flag(true), {"Type": "Group Destroyed", "Group Name": "Blockade"}], "actions": [{"Type": "Goto Next Level", "World": "Mission02.twt"}]},
        {"name": "Open speech", "conditions": [flag(true), enter], "actions": speak},
        {"name": "Odd", "conditions": [flag(true), {"Type": "No Such Condition"}], "actions": set},
    ], "ships": [], "volumes": {"Search": vol}, "environment": {}})
}

fn stall_world(player_at: Vec3) -> Vec<Unit> {
    let mut blockader = unit("Blockader", "Blockade", Vec3::new(500.0, 0.0, 0.0));
    blockader.e = Entity::from_raw_u32(7).unwrap();
    vec![unit("RLS Comet", PLAYER, player_at), blockader]
}

/// The report evaluates conditions with the evaluator's own path and leaves nothing behind: edge state, pending events and the
/// "unsupported" warning are as before, so the rule still fires on the player's real entry.
#[test]
fn unfired_report_gives_each_condition_its_truth_and_changes_nothing() {
    let mut sc = Script::new(&stall_data(), Value::Null);
    sc.flags.insert("go".into(), true);
    let mut w = stall_world(Vec3::ZERO);
    let before = (sc.prev_inside.clone(), sc.entered.clone(), sc.events.clone(), sc.consume.clone(), sc.warned.clone());
    let report = sc.unfired_report(&w);
    assert_eq!((sc.prev_inside.clone(), sc.entered.clone(), sc.events.clone(), sc.consume.clone(), sc.warned.clone()), before);
    assert_eq!(report.len(), 6);
    let open = &report[0];
    assert_eq!((open.name.as_str(), open.class, open.gates_hold), ("Open story", RuleClass::Story, true));
    assert_eq!(open.conds.iter().map(|c| (c.kind.as_str(), c.holds)).collect::<Vec<_>>(), [("Flag Condition", true), ("Enter Volume", false)]);
    assert!(open.conds[1].args.contains("Volume Name=Search"), "{}", open.conds[1].args);
    assert!(open.conds[1].detail.contains("player outside, 141 from edge"), "{}", open.conds[1].detail);
    assert!(open.conds[0].detail.is_empty(), "a holding condition needs no explanation");
    assert!(report[1].conds[0].detail.contains("flag is true"), "{}", report[1].conds[0].detail);
    assert!(!report[5].conds[1].holds, "an unsupported condition reads false");
    // The player's real entry still counts as the edge.
    w[0].pos = Vec3::new(150.0, 0.0, -150.0);
    let after_probe = sc.unfired_report(&w);
    assert!(after_probe[0].conds[1].holds && after_probe[0].conds[1].detail.is_empty());
    sc.step(&w, 0.1, false);
    assert!(sc.fired[0] && sc.flags["found"], "entry is still seen as an entry after probing");
}

#[test]
fn rules_are_classified_by_their_actions_not_their_names() {
    let mut sc = Script::new(&stall_data(), Value::Null);
    let classes: Vec<_> = sc.unfired_report(&stall_world(Vec3::ZERO)).iter().map(|r| (r.name.clone(), r.class)).collect();
    use RuleClass::*;
    assert_eq!(classes.iter().map(|c| c.1).collect::<Vec<_>>(), [Story, Story, Loss, Win, Presentation, Story], "{classes:?}");
    assert_eq!(sc.never_fired_by_class(), (3, 1, 1, 1));
}

/// The frontier is the unfired story or win rules whose flag gates hold; `watch` adds up how long each of their conditions has been false.
#[test]
fn frontier_keeps_open_story_and_win_rules_and_times_their_false_conditions() {
    let mut sc = Script::new(&stall_data(), Value::Null);
    sc.flags.insert("go".into(), true);
    let mut w = stall_world(Vec3::ZERO);
    for _ in 0..25 {
        sc.watch(&w, 0.1); // 2.5 s: two samples
    }
    let names: Vec<_> = sc.frontier().iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Open story", "Open win", "Odd"], "gated, loss and presentation rules are not on the frontier");
    let enter = &sc.frontier()[0].conds[1];
    assert!(!enter.holds && (enter.false_for - 2.0).abs() < 1e-3, "{enter:?}");
    let win = &sc.frontier()[1].conds[1];
    assert!(win.detail.contains("1 members, 1 alive"), "{}", win.detail);
    // The condition holds once the player is inside: its clock starts over when it is false again.
    w[0].pos = Vec3::new(150.0, 0.0, -150.0);
    sc.watch(&w, 1.0);
    assert!(sc.frontier()[0].conds[1].holds && sc.frontier()[0].conds[1].false_for == 0.0);
    w[0].pos = Vec3::ZERO;
    sc.watch(&w, 1.0);
    assert!((sc.frontier()[0].conds[1].false_for - 1.0).abs() < 1e-3);
    // Opening the gate puts the gated rule on the frontier; a rule that fired leaves it.
    sc.flags.insert("go".into(), false);
    sc.watch(&w, 1.0);
    assert_eq!(sc.frontier().iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Gated story"]);
}

/// Report fields for volume and distance conditions state the player's relation: inside or outside, and how far.
#[test]
fn distance_condition_detail_names_the_nearest_pair_and_the_players_side() {
    let data = serde_json::json!({"rules": [
        {"name": "Close", "conditions": [{"Type": "Group to Group Distance", "GroupA Name": "Player0 Group,HUMAN CONTROLLED COMMAND SHIP", "GroupB Name": "Blockade", "Equivalence": "Less Than", "Distance": 300}],
         "actions": [{"Type": "Set Flag Action", "Flag Name": "near", "Boolean Value": "TRUE"}]},
    ], "ships": [], "volumes": null, "environment": {}});
    let mut sc = Script::new(&data, Value::Null);
    let r = sc.unfired_report(&stall_world(Vec3::ZERO));
    let c = &r[0].conds[0];
    assert!(!c.holds);
    assert!(c.detail.contains("nearest pair 500 vs Less Than 300") && c.detail.contains("player is in A, nearest other 500"), "{}", c.detail);
}

/// Probing leaves no trace: a script sampled by `watch` every frame fires its enter, exit, once-per-unit and kill rules on exactly the
/// frames an unwatched copy does.
#[test]
fn watching_does_not_change_when_rules_fire() {
    let vol = serde_json::json!([[100.0, 100.0], [200.0, 100.0], [200.0, 200.0], [100.0, 200.0]]);
    let rule = |name: &str, cond: Value| serde_json::json!({"name": name, "Run Once": false, "conditions": [cond], "actions": [{"Type": "Start Timer", "Timer Name": name}]});
    let data = serde_json::json!({"rules": [
        rule("enter", serde_json::json!({"Type": "Enter Volume", "Group Name": "Blockade", "Volume Name": "Search"})),
        rule("exit", serde_json::json!({"Type": "Exit Volume", "Group Name": "Blockade", "Volume Name": "Search"})),
        rule("all", serde_json::json!({"Type": "All Units from Group have entered trigger volume Once", "Group Name": "Blockade", "Volume Name": "Search"})),
        rule("once", serde_json::json!({"Type": "Unit from Group enters trigger volume ( Once per Unit )", "Group Name": "Blockade", "Volume Name": "Search", "Team Name": ""})),
        rule("bursts", serde_json::json!({"Type": "Mission 9 - If Mortar explodes within Area", "Volume Name": "Search"})),
        rule("unknown", serde_json::json!({"Type": "No Such Condition"})),
    ], "ships": [], "volumes": {"Search": vol}, "environment": {}});
    let (mut plain, mut watched) = (Script::new(&data, Value::Null), Script::new(&data, Value::Null));
    for frame in 0..60 {
        let x = if (10..20).contains(&frame) || frame >= 40 { 150.0 } else { 500.0 };
        let w = stall_world(Vec3::ZERO).into_iter().map(|mut u| (u.name == "Blockader").then(|| u.pos = Vec3::new(x, 0.0, -150.0)).map_or(u.clone(), |_| u)).collect::<Vec<_>>();
        if frame == 50 {
            plain.mortar_bursts.push(Vec3::new(150.0, 0.0, -150.0));
            watched.mortar_bursts.push(Vec3::new(150.0, 0.0, -150.0));
        }
        watched.watch(&w, 1.0);
        watched.unfired_report(&w);
        plain.step(&w, 1.0, false);
        watched.step(&w, 1.0, false);
        assert_eq!(plain.ran, watched.ran, "frame {frame}");
        assert_eq!(plain.timers, watched.timers, "frame {frame}");
    }
    assert!(plain.ran[..5].iter().all(|r| *r), "every synthetic rule should have fired: {:?}", plain.ran);
    assert!(watched.warned.is_empty() || plain.warned == watched.warned);
}

/// With the frontier empty, the gated list names the flag that story and win rules wait on (not loss or presentation rules).
#[test]
fn gated_rules_name_the_flag_a_story_rule_waits_for() {
    let mut sc = Script::new(&stall_data(), Value::Null);
    sc.flags.insert("go".into(), true);
    let gated = sc.gated_rules();
    assert_eq!(gated.len(), 1, "{gated:?}");
    assert_eq!(gated[0].0, "Gated story");
    assert!(gated[0].1[0].contains("Flag Condition") && gated[0].1[0].contains("Flag Name=go") && gated[0].1[0].contains("flag is true"), "{:?}", gated[0].1);
}

/// A world condition about a group that some rule makes visible waits for that rule: the rule that reveals the pirates is on the way to
/// the win that needs them sunk (mission 7), and the one that makes a camp dockable is on the way to the dock.
#[test]
fn rule_graph_links_world_conditions_to_the_rules_that_reveal_their_groups() {
    let data = serde_json::json!({"rules": [
        rule("win", r#"[{"Type": "Group Destroyed", "Group Name": "Pirates"}, {"Type": "Group/Unit Docked", "Dockers Group/Unit Name": "Player0 Group", "Targets Group/Unit Name": "Camp"}]"#, WIN),
        rule("reveal", &format!("[{}]", flag("go")), r#"[{"Type": "Set Group/Unit Visibility", "Group/Unit Name": "Pirates", "Boolean": "TRUE"}]"#),
        rule("open camp", &format!("[{}]", flag("go")), r#"[{"Type": "Set Group/Unit Dockable", "Group Name": "Camp", "Boolean": "TRUE"}]"#),
        rule("go", &format!("[{}]", enter(ME, "V")), &set_flag("go")),
        rule("hide other", &format!("[{}]", flag("go")), r#"[{"Type": "Set Group/Unit Visibility", "Group/Unit Name": "Whales", "Boolean": "TRUE"}]"#),
    ], "ships": [], "volumes": {"V": square(100.0)}, "environment": {}});
    let sc = Script::new(&data, Value::Null);
    use Role::*;
    assert_eq!((0..5).map(|i| sc.role(i)).collect::<Vec<_>>(), [Critical(0), Critical(1), Critical(1), Critical(2), Optional]);
}

/// A tow condition naming a group yields one tow, of the nearest member, and none once the player tows one of them. At the same chain
/// distance the want the objective point marks comes before a nearer one.
#[test]
fn plan_tows_one_ship_and_prefers_the_want_the_objective_point_marks() {
    let tow = format!(r#"{{"Type": "Is Ship In Tow", "Towing Group": "{ME}", "Target to Tow": "Convoy", "Boolean": "TRUE"}}"#);
    let data = serde_json::json!({
        "rules": [
            rule("tow", &format!("[{tow}]"), WIN),
            rule("near", &format!("[{}]", enter(ME, "Near")), &set_flag("a")),
            rule("far", &format!("[{}]", enter(ME, "Far")), &set_flag("a")),
            rule("end", &format!("[{}]", flag("a")), WIN),
        ],
        "ships": [], "volumes": {"Near": square(100.0), "Far": square(2000.0)}, "environment": {},
        "points": [{"Name": "mark", "Position": [2050.0, 150.0, 0.0]}],
    });
    let mut sc = Script::new(&data, Value::Null);
    let mut ships = vec![unit("RLS Comet", PLAYER, Vec3::ZERO)];
    for (n, z) in [(2, 900.0), (3, 300.0), (4, 600.0)] {
        let mut c = unit("Barge", "Convoy", Vec3::new(0.0, 0.0, z));
        c.e = id(n);
        ships.push(c);
    }
    ships[0].e = id(1);
    let tows: Vec<_> = sc.plan(&ships).iter().filter(|x| matches!(x.want, Want::Tow(_))).map(|x| x.want).collect();
    assert_eq!(tows, [Want::Tow(id(3))], "one tow, of the nearest member");
    sc.tows.push((id(1), id(4)));
    assert!(!sc.plan(&ships).iter().any(|x| matches!(x.want, Want::Tow(_))), "towing one of them satisfies it");
    // Both volumes are one hop from the end rule; the point sits at the far one.
    sc.point = Some(0);
    let reach: Vec<_> = sc.plan(&ships).iter().filter(|x| x.rule > 0 && x.rule < 3).map(|x| x.rule).collect();
    assert_eq!(reach, [2, 1], "the marked volume first, then the nearer one");
}

/// A dock waits for the rule that makes the camp dockable (mission 2: the island's towers come first): until it has fired the dock is
/// not wanted, only what leads to it is.
#[test]
fn a_dock_is_not_wanted_before_the_rule_that_makes_its_target_dockable_has_fired() {
    let dock = format!(r#"{{"Type": "Group/Unit Docked", "Dockers Group/Unit Name": "{ME}", "Targets Group/Unit Name": "Camp"}}"#);
    let hit = r#"{"Type": "Group Destroyed", "Group Name": "Camp"}"#;
    let data = serde_json::json!({"rules": [
        rule("win", &format!("[{}]", flag("docked")), WIN),
        rule("dock", &format!("[{dock}]"), &set_flag("docked")),
        rule("open", &format!("[{}]", flag("down")), r#"[{"Type": "Set Group/Unit Dockable", "Group Name": "Camp", "Boolean": "TRUE"}]"#),
        rule("towers", &format!("[{hit}]"), &set_flag("down")),
    ], "ships": [], "volumes": null, "environment": {}});
    let mut sc = Script::new(&data, Value::Null);
    let mut camp = unit("Camp", "Camp", Vec3::new(0.0, 0.0, 300.0));
    camp.e = id(3);
    let mut me = unit("RLS Comet", PLAYER, Vec3::ZERO);
    me.e = id(1);
    let w = vec![me, camp];
    sc.hostile.insert(("Camp".into(), PLAYER.into()));
    let wants: Vec<Want> = sc.plan(&w).iter().map(|x| x.want).collect();
    assert_eq!(wants, [Want::Hit(id(3))], "shoot first");
    sc.ran[2] = true;
    sc.fired[2] = true;
    let wants: Vec<Want> = sc.plan(&w).iter().map(|x| x.want).collect();
    assert!(wants.contains(&Want::Dock(id(3))), "{wants:?}");
}
