use super::*;
use crate::EngineOrder;
pub fn fleet_world() -> (World, [Entity; 6]) {
    let mut w = World::new();
    let lib = crate::testutil::lib();
    let ci = lib.index("Ship_Navy_Cutter");
    let class = &lib.classes[ci];
    let ids = std::array::from_fn(|i| {
        w.spawn((
            Ship { heading: 0.0, ..Ship::new(ci, if i == 5 { Side::Enemy } else { Side::Player }) },
            Health::new(class),
            Transform::from_xyz(i as f32 * 50.0, 0.0, 0.0),
            FleetMember(
                if i < 4 {
                    "Player"
                } else if i == 4 {
                    "Ally"
                } else {
                    "Enemy"
                }
                .into(),
                i,
            ),
        ))
        .id()
    });
    w.entity_mut(ids[0]).insert(PlayerShip);
    w.insert_resource(lib);
    w.insert_resource(Target::default());
    w.insert_resource(Orbit { yaw: 0.0, pitch: 0.0, dist: 60.0, focus: None });
    w.insert_resource(Fleets { local: "Player".into(), ..default() });
    // Fleet keys send P as an order (`commands::Kind::Promote`).
    w.init_resource::<crate::commands::Outbox>();
    reconcile_fleets(&mut w);
    (w, ids)
}

#[test]
#[ignore = "needs imported assets"]
fn secondary_selection_focus_and_orders_do_not_promote_or_touch_siblings() {
    let (mut w, ids) = fleet_world();
    w.resource_scope(|w, mut f: Mut<Fleets>| {
        assert!(f.select(ids[2], &mut w.resource_mut::<Orbit>()));
        assert!(!f.direct());
    });
    assert!(w.get::<PlayerShip>(ids[0]).is_some());
    assert!(w.get::<PlayerShip>(ids[2]).is_none());
    w.resource_mut::<Orbit>().focus = Some(ids[5]);
    let actor = w.resource::<Fleets>().selected.unwrap();
    w.get_mut::<FleetPilot>(actor).unwrap().move_to(Vec3::new(100.0, 0.0, -200.0));
    assert!(w.get::<FleetPilot>(ids[0]).unwrap().destination.is_none());
    assert!(w.get::<FleetPilot>(ids[1]).unwrap().destination.is_none());
    w.get_mut::<FleetPilot>(actor).unwrap().attack(ids[5]);
    assert_eq!(w.get::<FleetPilot>(actor).unwrap().target, Some(ids[5]));
    assert_eq!(w.resource::<Target>().0, None);
    assert_eq!(w.resource::<Fleets>().primary(), Some(ids[0]));
    w.resource_scope(|w, mut f: Mut<Fleets>| {
        assert!(!f.select(ids[4], &mut w.resource_mut::<Orbit>()));
    });
}

#[test]
#[ignore = "needs imported assets"]
fn promotion_checks_membership_reorders_and_preserves_speed_not_camera() {
    let (mut w, ids) = fleet_world();
    assert!(!promote(&mut w, ids[4], false));
    assert!(!promote(&mut w, ids[5], false));
    let max = full_speed(&w.resource::<Library>().classes[w.get::<Ship>(ids[2]).unwrap().class]);
    w.get_mut::<Ship>(ids[2]).unwrap().speed = max * 0.6;
    w.get_mut::<Ship>(ids[2]).unwrap().cap = 0.75;
    w.resource_mut::<Orbit>().focus = Some(ids[5]);
    assert!(promote(&mut w, ids[2], false));
    assert_eq!(w.resource::<Fleets>().roster(), &[ids[2], ids[0], ids[1], ids[3]]);
    assert_eq!(w.resource::<Orbit>().focus, Some(ids[5]));
    assert!(w.get::<PlayerShip>(ids[0]).is_none());
    assert!((w.get::<Ship>(ids[2]).unwrap().throttle.unwrap() - 0.8).abs() < 0.001);
    assert_eq!(w.get::<Ship>(ids[2]).unwrap().cap, 0.75, "command fraction must not overwrite the mission speed cap");
    assert!(promote(&mut w, ids[2], false)); // Already primary still resets.
    assert!(promote(&mut w, ids[1], false));
    assert_eq!(w.get::<Ship>(ids[1]).unwrap().throttle, Some(0.0));
    w.insert_resource(campaign::load_id("mission05"));
    assert!(!promote(&mut w, ids[2], false));
    assert!(promote(&mut w, ids[2], true));
}

#[test]
#[ignore = "needs imported assets"]
fn scripted_cross_group_primary_retires_standin_and_updates_mission_identity() {
    let (mut w, ids) = fleet_world();
    let ms = |id, owner: &str, name: &str| campaign::MissionShip { id, owner: owner.into(), name: name.into(), visible: true, ..Default::default() };
    w.entity_mut(ids[0]).insert(ms(-1, "Player", "RLS Stand-in"));
    w.entity_mut(ids[4]).insert(ms(99, "Players Ships", "RLS St Roch"));
    w.insert_resource(campaign::load_id("mission09"));
    assert!(!promote(&mut w, ids[4], false));
    assert!(promote(&mut w, ids[4], true));
    assert!(w.get_entity(ids[0]).is_err());
    assert_eq!(w.resource::<Fleets>().primary(), Some(ids[4]));
    assert_eq!(w.resource::<Fleets>().selected, Some(ids[4]));
    assert_eq!(w.get::<campaign::MissionShip>(ids[4]).unwrap().owner, "Player");
    assert_eq!(w.query_filtered::<Entity, With<PlayerShip>>().iter(&w).count(), 1);
    assert!(!w.resource::<Fleets>().roster().contains(&ids[0]));
}

#[test]
#[ignore = "needs imported assets"]
fn formation_detach_attack_rejoin_release_and_fleet_help_are_per_secondary() {
    let (mut w, ids) = fleet_world();
    set_formation(&mut w, "Player", Some(Formation::Diamond), true);
    assert!(!w.get::<FleetPilot>(ids[0]).unwrap().hold);
    w.get_mut::<FleetPilot>(ids[1]).unwrap().move_to(Vec3::X * 100.0);
    w.get_mut::<FleetPilot>(ids[2]).unwrap().attack(ids[5]);
    assert!(!w.get::<FleetPilot>(ids[1]).unwrap().hold);
    assert!(w.get::<FleetPilot>(ids[2]).unwrap().hold);
    assert!(w.get::<FleetPilot>(ids[3]).unwrap().hold);
    set_formation(&mut w, "Player", Some(Formation::Diamond), true);
    assert!(ids[1..4].iter().all(|e| w.get::<FleetPilot>(*e).unwrap().hold));
    assert!(w.get::<FleetPilot>(ids[1]).unwrap().destination.is_none());
    set_formation(&mut w, "Player", Some(Formation::Diamond), false);
    assert_eq!(w.resource::<Fleets>().groups["Player"].formation, Formation::Diamond);
    assert!(ids[1..4].iter().all(|e| !w.get::<FleetPilot>(*e).unwrap().hold));
    w.get_mut::<FleetPilot>(ids[2]).unwrap().target = None;
    fleet_help(&mut w, "Player", true); // No primary target: no invented attack.
    assert!(w.get::<FleetPilot>(ids[2]).unwrap().target.is_none());
    w.resource_mut::<Target>().0 = Some(ids[5]);
    fleet_help(&mut w, "Player", true);
    assert!(ids[1..4].iter().all(|e| w.get::<FleetPilot>(*e).unwrap().target == Some(ids[5])));
    fleet_help(&mut w, "Player", false);
    assert!(!w.get::<FleetPilot>(ids[0]).unwrap().hold_fire);
    assert!(ids[1..4].iter().all(|e| w.get::<FleetPilot>(*e).unwrap().hold_fire));
    fleet_help(&mut w, "Player", false);
    assert!(ids[1..4].iter().all(|e| !w.get::<FleetPilot>(*e).unwrap().hold_fire));
}

#[test]
#[ignore = "needs imported assets"]
fn all_layouts_custom_and_rotated_following_preserve_altitude_and_limits() {
    let radii = [10.0, 20.0, 15.0, 5.0];
    assert_eq!(formation_slots(Formation::LineAbreast, &radii), vec![Vec3::ZERO, Vec3::new(-50.0, 0.0, 0.0), Vec3::new(50.0, 0.0, 0.0), Vec3::new(-100.0, 0.0, 0.0)]);
    assert_eq!(formation_slots(Formation::Column, &radii)[3], Vec3::Z * 210.0);
    for kind in Formation::BUTTONS {
        let v = formation_slots(kind, &radii);
        assert_eq!(v.len(), 4);
        assert!(v[1].length() > 0.0);
        assert_eq!(v[1].y, 0.0);
    }
    assert_eq!(formation_slots(Formation::None, &radii), vec![Vec3::ZERO; 4]);
    let (mut w, ids) = fleet_world();
    w.get_mut::<Transform>(ids[0]).unwrap().rotation = Quat::from_rotation_y(PI / 2.0);
    set_formation(&mut w, "Player", Some(Formation::Custom), true);
    let before = w.get::<Transform>(ids[1]).unwrap().translation;
    fleet_follow(&mut w);
    assert_eq!(w.get::<Ship>(ids[1]).unwrap().waypoints, Vec::<Vec3>::new()); // Already at its captured slot.
    assert_eq!(w.get::<Transform>(ids[1]).unwrap().translation, before);
    set_formation(&mut w, "Player", Some(Formation::Column), true);
    let slot = w.get::<FleetPilot>(ids[1]).unwrap().slot;
    fleet_follow(&mut w);
    // The order is only picked up at the follower's next re-plan (retail: up to 4.2 s later): still on the old plan, so no new waypoint.
    assert_eq!(w.get::<Ship>(ids[1]).unwrap().waypoints, Vec::<Vec3>::new());
    w.get_mut::<FleetPilot>(ids[1]).unwrap().replan_in = 0.0;
    fleet_follow(&mut w);
    let leader = w.get::<Transform>(ids[0]).unwrap();
    let expected = leader.translation + leader.rotation * slot;
    assert!(w.get::<Ship>(ids[1]).unwrap().waypoints[0].distance(expected) < 0.001);
    assert!((0.0..=2.0).contains(&w.get::<Ship>(ids[1]).unwrap().throttle.unwrap()));
    w.get_mut::<Transform>(ids[1]).unwrap().translation.y = 25.0;
    w.get_mut::<Ship>(ids[1]).unwrap().cap = 0.2;
    w.get_mut::<Health>(ids[1]).unwrap().sections.iter_mut().for_each(|h| *h *= 0.25);
    fleet_follow(&mut w);
    assert_eq!(w.get::<Ship>(ids[1]).unwrap().waypoints[0].y, 25.0);
    assert_eq!(w.get::<Ship>(ids[1]).unwrap().cap, 0.2);
    assert!((0.0..=2.0).contains(&w.get::<Ship>(ids[1]).unwrap().throttle.unwrap()));
    set_formation(&mut w, "Player", Some(Formation::None), false);
    assert!(ids[1..4].iter().all(|e| !w.get::<FleetPilot>(*e).unwrap().hold));
}

#[test]
#[ignore = "needs imported assets"]
fn a_follower_closes_on_its_slot_at_its_own_emergency_speed() {
    // Retail: the formation follower's top speed is its speed at full throttle, i.e. Emergency (a Cutter closed at ~9, twice its Ahead
    // Full), so it keeps up with a leader sailing at Emergency.
    let (mut w, ids) = fleet_world();
    set_formation(&mut w, "Player", Some(Formation::Column), true);
    let slot = w.get::<Transform>(ids[0]).unwrap().translation + w.get::<FleetPilot>(ids[1]).unwrap().slot;
    w.get_mut::<Transform>(ids[1]).unwrap().translation = slot + Vec3::Z * 200.0; // well behind its slot, facing it
    w.get_mut::<Ship>(ids[0]).unwrap().speed = 8.0;
    w.get_mut::<FleetPilot>(ids[1]).unwrap().replan_in = 0.0;
    w.get_mut::<Ship>(ids[1]).unwrap().heading = w.get::<Ship>(ids[0]).unwrap().heading;
    fleet_follow(&mut w);
    let s = w.get::<Ship>(ids[1]).unwrap();
    assert_eq!((s.order, s.ordered()), (EngineOrder::Emergency, EngineOrder::Emergency.fraction()), "{:?} {:?}", s.order, s.throttle);
}

#[test]
#[ignore = "needs imported assets"]
fn a_replacement_command_ship_takes_over_the_command_cap_not_its_own() {
    // A ship a rule had stopped (cap 0) became primary after the flagship was captured, and the player could not move.
    let (mut w, ids) = fleet_world();
    w.get_mut::<Ship>(ids[1]).unwrap().cap = 0.0;
    // A rule capping the captured flagship (now the enemy's) does not pass its 0 on.
    w.get_mut::<Ship>(ids[0]).unwrap().cap = 0.0;
    w.get_mut::<FleetMember>(ids[0]).unwrap().0 = "Enemy".into(); // captured
    reconcile_fleets(&mut w);
    assert_eq!(w.resource::<Fleets>().primary(), Some(ids[1]));
    assert_eq!(w.get::<Ship>(ids[1]).unwrap().cap, 1.0);
    // A cap the script put on the command ship (a cutscene slowdown) carries over to the replacement.
    w.get_mut::<Ship>(ids[1]).unwrap().cap = 0.25;
    w.get_mut::<Ship>(ids[2]).unwrap().cap = 0.0;
    w.get_mut::<Health>(ids[1]).unwrap().destroyed = true;
    reconcile_fleets(&mut w);
    assert_eq!(w.resource::<Fleets>().primary(), Some(ids[2]));
    assert_eq!(w.get::<Ship>(ids[2]).unwrap().cap, 0.25);
    // A despawned command ship leaves no cap to inherit: the replacement can move at full command.
    w.get_mut::<Ship>(ids[3]).unwrap().cap = 0.0;
    w.despawn(ids[2]);
    reconcile_fleets(&mut w);
    assert_eq!(w.resource::<Fleets>().primary(), Some(ids[3]));
    assert_eq!(w.get::<Ship>(ids[3]).unwrap().cap, 1.0);
    // A sunk command ship stopped by an NIS (cap 0) never strands its replacement either.
    let (mut w, ids) = fleet_world();
    w.get_mut::<Ship>(ids[0]).unwrap().cap = 0.0;
    w.get_mut::<Ship>(ids[1]).unwrap().cap = 0.0;
    w.get_mut::<Health>(ids[0]).unwrap().destroyed = true;
    reconcile_fleets(&mut w);
    assert_eq!(w.resource::<Fleets>().primary(), Some(ids[1]));
    assert_eq!(w.get::<Ship>(ids[1]).unwrap().cap, 1.0);
}

#[test]
#[ignore = "needs imported assets"]
fn destruction_despawn_empty_and_scenario_reset_clear_stale_fleet_state() {
    let (mut w, ids) = fleet_world();
    w.resource_mut::<Fleets>().selected = Some(ids[1]);
    w.resource_mut::<Orbit>().focus = Some(ids[1]);
    w.get_mut::<Health>(ids[1]).unwrap().destroyed = true;
    reconcile_fleets(&mut w);
    assert_eq!(w.resource::<Fleets>().selected, Some(ids[0]));
    assert_eq!(w.resource::<Orbit>().focus, None);
    w.despawn(ids[0]);
    reconcile_fleets(&mut w);
    assert_eq!(w.resource::<Fleets>().primary(), Some(ids[2]));
    assert!(w.get::<PlayerShip>(ids[2]).is_some());
    w.get_mut::<FleetMember>(ids[2]).unwrap().0 = "Enemy".into();
    reconcile_fleets(&mut w);
    assert_eq!(w.resource::<Fleets>().roster(), &[ids[3]]);
    assert!(w.get::<PlayerShip>(ids[2]).is_none());
    w.get_mut::<FleetPilot>(ids[3]).unwrap().target = Some(ids[5]);
    w.get_mut::<Ship>(ids[5]).unwrap().side = Side::Player;
    reconcile_fleets(&mut w);
    assert!(w.get::<FleetPilot>(ids[3]).unwrap().target.is_none(), "capture/alliance changes must not leave a friendly attack target");
    w.despawn(ids[3]);
    reconcile_fleets(&mut w);
    assert_eq!(w.resource::<Fleets>().selected, None);
    w.resource_mut::<Fleets>().bar_open = true;
    w.resource_mut::<Fleets>().commands_open = true;
    w.insert_resource(Time::<Virtual>::default());
    w.resource_mut::<Time<Virtual>>().pause();
    scenario::resume_scenario(&mut w);
    assert!(!w.resource::<Time<Virtual>>().is_paused());
    assert!(w.resource::<Fleets>().groups.is_empty());
    assert!(!w.resource::<Fleets>().bar_open);
    assert!(!w.resource::<Fleets>().commands_open);
}

#[test]
#[ignore = "needs imported assets"]
fn rebound_fleet_keys_cycle_including_primary_and_never_change_time_scale() {
    use bevy::ecs::system::RunSystemOnce;
    let (mut w, ids) = fleet_world();
    w.insert_resource(bindings::Bindings::default());
    w.insert_resource(ButtonInput::<KeyCode>::default());
    w.insert_resource(Time::<Virtual>::default());
    w.insert_resource(Messages::<audio::Sfx>::default());
    let press = |w: &mut World, key| {
        w.resource_mut::<ButtonInput<KeyCode>>().reset_all();
        w.resource_mut::<ButtonInput<KeyCode>>().press(key);
        w.run_system_once(view::fleet_keys).unwrap();
    };
    press(&mut w, KeyCode::Digit3);
    assert_eq!(w.resource::<Fleets>().selected, Some(ids[2]));
    assert!(w.get::<PlayerShip>(ids[0]).is_some());
    press(&mut w, KeyCode::Digit0); // Out of range.
    assert_eq!(w.resource::<Fleets>().selected, Some(ids[2]));
    press(&mut w, KeyCode::BracketRight);
    press(&mut w, KeyCode::BracketRight);
    assert_eq!(w.resource::<Fleets>().selected, Some(ids[0]));
    press(&mut w, KeyCode::BracketLeft);
    assert_eq!(w.resource::<Fleets>().selected, Some(ids[3]));
    assert_eq!(w.resource::<Time<Virtual>>().relative_speed(), 1.0);
    let row = w.resource::<bindings::Bindings>().rows.iter().position(|r| r.0 == bindings::Act::Primary).unwrap();
    w.resource_mut::<bindings::Bindings>().rows[row].2 = KeyCode::Quote;
    press(&mut w, KeyCode::Quote);
    assert_eq!(w.resource::<Fleets>().selected, Some(ids[0]));
    w.resource_mut::<Fleets>().selected = Some(ids[4]);
    press(&mut w, KeyCode::BracketLeft);
    assert_eq!(w.resource::<Fleets>().selected, Some(ids[4]));
}

#[test]
fn fresh_scenario_resumes_paused_time_without_changing_speed() {
    let mut world = World::new();
    let mut time = Time::<Virtual>::default();
    set_time_scale(&mut time, 4.0);
    time.pause();
    world.insert_resource(time);
    let mut queue = bevy::ecs::world::CommandQueue::default();
    Commands::new(&mut queue, &world).queue(resume_scenario);
    queue.apply(&mut world);
    let time = world.resource::<Time<Virtual>>();
    assert!(!time.is_paused());
    assert_eq!(time.relative_speed(), 4.0);
}
#[test]
#[ignore = "needs imported assets"]
fn open_map_ships_get_distinct_names_from_their_type_pool() {
    let lib = crate::testutil::lib();
    let mut namer = Namer::new();
    let pool = &lib.names["Ship_Pirate_Sloop"];
    let picked: std::collections::HashSet<String> = (0..pool.len()).map(|_| namer.pick(&lib, "Ship_Pirate_Sloop").unwrap()).collect();
    assert_eq!(picked.len(), pool.len(), "no repeats until the pool is used up");
    assert!(namer.pick(&lib, "Ship_Pirate_Sloop").is_some(), "a used-up pool repeats");
    assert!(namer.pick(&lib, "Ship_Nonexistent").is_none());
}
#[test]
fn time_scale_preserves_virtual_catch_up_caps() {
    use bevy::time::{TimePlugin, TimeUpdateStrategy};
    use std::time::Duration;
    // Virtual delta after one real frame of `raw` seconds at `speed`.
    let step = |speed: f32, raw: f32| {
        let mut app = App::new();
        app.add_plugins(TimePlugin).insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(raw)));
        set_time_scale(&mut app.world_mut().resource_mut::<Time<Virtual>>(), speed);
        app.update();
        app.update();
        app.world().resource::<Time<Virtual>>().delta_secs()
    };
    assert!((step(4.0, 0.01) - 0.04).abs() < 1e-4); // fast frame: full 4x
    assert!((step(16.0, 1.0 / 60.0) - 0.1).abs() < 1e-4); // vsync frame at 16x: capped at 0.1 s
    assert!((step(16.0, 1.0) - 0.1).abs() < 1e-4); // above 5x a slow frame is capped at 0.1 s (about 3 ticks)
    // docs/re/tick.md: at most 10 frames of `speed` ticks of 0.033 s per call; a longer stall drops the rest.
    assert!((step(1.0, 1.0) - 0.33).abs() < 1e-4);
    assert!((step(2.0, 1.0) - 0.66).abs() < 1e-4);
    assert!((step(5.0, 10.0) - 1.65).abs() < 1e-4);
}

#[test]
#[ignore = "needs imported assets"]
fn ship_keeps_speed_and_heading_past_last_waypoint() {
    let lib = crate::testutil::lib();
    // Not class 0: the sorted ship folders now start with the (immobile) armed bases.
    let ci = lib.index("Ship_Navy_Cutter");
    let class = &lib.classes[ci];
    let mut ship = Ship { heading: 0.0, order: EngineOrder::Emergency, waypoints: vec![Vec3::new(-30.0, 0.0, -80.0)], ..Ship::new(ci, Side::Player) };
    let mut tf = Transform::default();
    let h = Health { sections: vec![1.0; class.sections.len()], core: 1.0, destroyed: false };
    for _ in 0..3000 {
        step(&mut ship, &mut tf, class, &h, 0.033);
    }
    assert!(ship.waypoints.is_empty());
    assert!(ship.order == EngineOrder::Emergency && ship.speed > 1.0, "speed {}", ship.speed);
}
#[test]
fn exploring_heads_for_quadrant_centres() {
    // retail Carrack, Diablo Strait (half 1625), from (322, -1127): its course (0.27, 0.96) points at quadrant (+x, +y)
    let c = Explore::centre(3, 1625.0);
    assert_eq!((c.x, -c.z), (812.5, 812.5));
    let d = (Vec2::new(c.x, -c.z) - Vec2::new(322.0, -1127.0)).normalize();
    assert!((d.x - 0.245).abs() < 0.01 && (d.y - 0.97).abs() < 0.01, "{d}");
    assert_eq!(Explore::centre(0, 1000.0), Vec3::new(-500.0, 0.0, 500.0));
}

#[test]
fn captain_keeps_its_target_six_seconds_then_takes_the_nearest() {
    let (a, b) = (Entity::from_raw_u32(1).unwrap(), Entity::from_raw_u32(2).unwrap());
    let alive = |_: Entity| true;
    let pick = |held, valid: &dyn Fn(Entity) -> bool| choose_target(held, None, Some(b), valid);
    assert_eq!(pick(None, &alive), Some((b, false, true)), "no target: its own pick, which can be overridden");
    assert_eq!(pick(Some((a, 5.9, true)), &alive), Some((a, true, true)), "kept inside 6 s though farther");
    assert_eq!(choose_target(Some((a, 5.9, true)), None, Some(b), alive), Some((a, true, true)), "a held target is followed out of sight");
    assert_eq!(pick(Some((a, 6.0, true)), &alive), Some((b, false, true)), "re-chosen after 6 s");
    assert_eq!(pick(Some((a, 1.0, true)), &|t| t != a), Some((b, false, true)), "a sunk target is replaced at once");
    assert_eq!(choose_target(Some((a, 1.0, true)), None, None, |t| t != a), None);
}

/// "Cannot override" targets: every explicit target is taken at once and kept past 6 s until it is lost (friendly, sunk) or replaced.
#[test]
fn an_ordered_target_cannot_be_overridden_and_is_kept_until_lost() {
    let (a, b, c) = (Entity::from_raw_u32(1).unwrap(), Entity::from_raw_u32(2).unwrap(), Entity::from_raw_u32(3).unwrap());
    let near = Some(b);
    assert_eq!(choose_target(Some((b, 1.0, true)), Some(a), near, |_| true), Some((a, false, false)), "an order replaces the captain's pick");
    assert_eq!(choose_target(Some((a, 60.0, false)), None, near, |_| true), Some((a, true, false)), "kept past 6 s once the order is spent");
    assert_eq!(choose_target(Some((a, 60.0, false)), None, near, |t| t != a), Some((b, false, true)), "lost: the captain picks again");
    assert_eq!(choose_target(Some((a, 1.0, false)), Some(c), near, |_| true), Some((c, false, false)), "a new order replaces it");
    assert_eq!(choose_target(None, Some(c), near, |t| t != c), Some((b, false, true)), "an order on a friend is dropped");
}

#[test]
fn a_campaign_limits_targets_and_docks_to_scan_range_a_skirmish_does_not() {
    assert!(in_reach(false, || false), "skirmish: any range");
    assert!(!in_reach(true, || false) && in_reach(true, || true), "campaign: within scan range only");
}

/// An asset-free world for the captain (`ai`): one synthetic class (sight 100), sensors refreshed before each think. With `campaign`
/// a mission is loaded.
pub(crate) fn captain_world(campaign: bool) -> World {
    let mut w = World::new();
    w.insert_resource(Library { classes: vec![crate::fixed_tests::synthetic_class()], guns: default(), bullets: default(), names: default(), clips: default() });
    w.insert_resource(crate::Config { speed: 1.0, ..default() });
    w.init_resource::<crate::environment::Sensors>();
    w.init_resource::<CaptainTargets>();
    w.init_resource::<Fleets>();
    w.init_resource::<Time>();
    if campaign {
        let data = serde_json::json!({"rules": []});
        w.insert_resource(campaign::Mission { id: "synthetic".into(), script: crate::rules::Script::new(&data, serde_json::Value::Null), data, opening: None });
    }
    w
}
/// A ship of `side` at `at`, afloat.
pub(crate) fn captain_ship(w: &mut World, side: Side, at: Vec3) -> Entity {
    w.spawn((Ship { heading: 0.0, ..Ship::new(0, side) }, Health { sections: vec![], core: 100.0, destroyed: false }, Transform::from_translation(at))).id()
}
#[derive(Resource)]
struct Captains(bevy::ecs::system::SystemId, bevy::ecs::system::SystemId);
/// Runs the sensors and the captains three times a second apart (each captain thinks at least once, every captain after the others'
/// first picks); the systems keep their state between calls.
pub(crate) fn think(w: &mut World) {
    if !w.contains_resource::<Captains>() {
        let ids = Captains(w.register_system(crate::environment::refresh_sensors), w.register_system(ai));
        w.insert_resource(ids);
    }
    let (sense, captains) = (w.resource::<Captains>().0, w.resource::<Captains>().1);
    for _ in 0..3 {
        w.run_system(sense).unwrap();
        w.run_system(captains).unwrap();
        w.resource_mut::<Time>().advance_by(std::time::Duration::from_secs(1));
    }
}

/// The one captain in a mission: a computer mission ship picks a hostile base only inside its scan range (here none is in sight), the
/// same ship in a skirmish at any range. Asset-free.
#[test]
fn a_mission_captain_takes_a_base_only_in_scan_range_a_skirmish_one_at_any() {
    let target_of = |campaign: bool| {
        let mut w = captain_world(campaign);
        let ship = captain_ship(&mut w, Side::Enemy, Vec3::ZERO);
        let base = w
            .spawn((
                Transform::from_xyz(0.0, 0.0, -5000.0),
                combat::BaseSide(Side::Player),
                Health { sections: vec![], core: 100.0, destroyed: false },
                combat::ArmedBase { class: 0 },
            ))
            .id();
        think(&mut w);
        (w.resource::<CaptainTargets>().of(ship), base)
    };
    let (skirmish, base) = target_of(false);
    assert_eq!(skirmish, Some(base), "skirmish: the base at any range");
    assert_eq!(target_of(true).0, None, "campaign: out of scan range");
}

/// Every order source sets "cannot override"; the captain's own pick sets "can" (here a repair dock the captain chose, and a hostile in
/// sight).
#[test]
fn a_repair_dock_and_an_order_are_cannot_targets_an_own_pick_can() {
    let mut w = captain_world(false);
    let (ship, foe, dock) = (captain_ship(&mut w, Side::Enemy, Vec3::ZERO), captain_ship(&mut w, Side::Player, Vec3::X * 50.0), captain_ship(&mut w, Side::Enemy, Vec3::Z * 50.0));
    let docker = captain_ship(&mut w, Side::Enemy, Vec3::NEG_X * 50.0);
    w.entity_mut(docker).insert(crate::docking::Docker::repair(dock));
    think(&mut w);
    let t = |e: Entity| w.resource::<CaptainTargets>().0.get(&e).map(|t| (t.0, t.3));
    assert_eq!(t(ship), Some((foe, true)), "its own pick: can override");
    assert_eq!(t(docker), Some((dock, false)), "a chosen repair dock: cannot");
}

#[test]
fn captains_turn_straight_away_from_a_black_hole() {
    assert!(flee(&[(Vec3::new(0.0, 0.0, 300.0), 145.0)], Vec3::new(0.0, 0.0, 150.0)).is_some_and(|h| angdiff(h, 0.0).abs() < 1e-5), "bow toward -z");
    assert_eq!(flee(&[(Vec3::new(0.0, 0.0, 300.0), 145.0)], Vec3::new(0.0, 0.0, -100.0)), None);
}

#[test]
#[ignore = "needs imported assets"]
fn a_broadside_attacker_circles_an_idle_ship_on_its_turning_circle() {
    // The original's broadside course (attack.rs) with the real movement step: crewed computer ships run in from 400 units and end
    // circling with the target abeam. Retail: Carrack 45-50 units around an idle Cutter, Barque 51-58 (docs/re/movement.md).
    let lib = crate::testutil::lib();
    let foe = Vec3::ZERO;
    // Minimum throttle: the measured Carrack's save holds 0.745; the Barque takes the range's mean (each ship draws 0.65-0.75).
    // With the computer helm's torque boost and the crew factor applied once, the circles run 43-48 (Carrack) and 53-58 (Barque).
    for (name, min_throttle, lo_ok, hi_ok) in [("Ship_Pirate_Carrack", 0.745, 41.0, 51.0), ("Ship_Pirate_Barque", 0.7, 50.0, 61.0)] {
        let ci = lib.index(name);
        let class = &lib.classes[ci];
        let h = Health { sections: class.sections.iter().map(|s| s.hp).collect(), core: class.core_hp, destroyed: false };
        let (routine, range) = attack::plan(&lib, class, &h, None).unwrap();
        let mut ship = Ship { heading: 0.0, crew_speed: COMPUTER_CREW, crew_turn: COMPUTER_CREW, ..Ship::new(ci, Side::Enemy) };
        let mut tf = Transform::from_xyz(30.0, 0.0, 400.0);
        let cutter = &lib.classes[lib.index("Ship_Navy_Cutter")].sections;
        let cutter_hp: Vec<f32> = cutter.iter().map(|s| s.hp).collect();
        let dt = 1.0 / 30.0;
        let (mut dists, mut beams) = (vec![], vec![]);
        for i in 0..(200.0 / dt) as usize {
            let gap = tf.translation.length() - class.radius;
            let at = attack::aim(cutter, &cutter_hp, &Transform::IDENTITY, tf.translation, false);
            let course = attack::course(routine, tf.translation, ship.heading, at, gap, range[0], i as f32 * dt, &mut None);
            let throttle = attack::throttle(routine, crate::scenario::SpawnIndex(3), tf.translation, ship.heading, course[0]).map(|_| 2.0 * min_throttle);
            (ship.throttle, ship.waypoints, ship.goal, ship.order) = (throttle, course, None, EngineOrder::Emergency);
            helm_step(&mut ship, &mut tf, class, &h, dt, true);
            if i as f32 * dt > 120.0 {
                let d = foe - tf.translation;
                dists.push(Vec2::new(d.x, d.z).length());
                beams.push(angdiff(crate::geom::heading_of(d), ship.heading));
            }
        }
        let (lo, hi) = dists.iter().fold((f32::MAX, 0.0f32), |(a, b), d| (a.min(*d), b.max(*d)));
        assert!(lo > lo_ok && hi < hi_ok, "{name}: circle {lo}..{hi}");
        let side = if let attack::Routine::Broadside(side) = routine { side } else { 0.0 };
        assert!(beams.iter().all(|b| (b - side * PI / 2.0).abs() < 0.6), "{name}: target kept on the chosen beam");
    }
}

/// Retail Navy Cutter without crew, sampled from the running original (docs/re/movement.md, "Measured").
#[test]
#[ignore = "needs imported assets"]
fn cutter_matches_measured_retail_movement() {
    let lib = crate::testutil::lib();
    let ci = lib.index("Ship_Navy_Cutter");
    let class = &lib.classes[ci];
    let h = Health { sections: vec![1.0; class.sections.len()], core: 1.0, destroyed: false };
    let dt = 0.0344; // retail update interval
    let run = |ship: &mut Ship, tf: &mut Transform, secs: f32| {
        for _ in 0..(secs / dt) as usize {
            step(ship, tf, class, &h, dt);
        }
    };
    let mut ship = Ship { heading: 0.0, order: EngineOrder::Slow, ..Ship::new(ci, Side::Player) };
    let mut tf = Transform::default();
    run(&mut ship, &mut tf, 1.0);
    assert!((ship.speed - 1.95).abs() < 0.1, "Slow from rest after 1 s: {}", ship.speed); // 0.53 s time constant
    for (order, measured) in [(EngineOrder::Slow, 2.2655), (EngineOrder::Full, 4.531), (EngineOrder::Emergency, 9.0623)] {
        ship.order = order;
        run(&mut ship, &mut tf, 8.0);
        assert!((ship.speed - measured).abs() < 0.01, "order {order:?}: {} vs {measured}", ship.speed);
    }
    ship.order = EngineOrder::Full;
    run(&mut ship, &mut tf, 8.0);
    ship.order = EngineOrder::Stop;
    run(&mut ship, &mut tf, 0.75); // retail: 23 ticks of decay to 1.05, then rest on the 24th (0.83 s)
    assert!(ship.speed > 1.0, "still slowing after 0.75 s: {}", ship.speed);
    run(&mut ship, &mut tf, 0.15);
    assert_eq!(ship.speed, 0.0, "All Stop from Full is at rest after about 0.83 s");
    for order in [EngineOrder::Slow, EngineOrder::Full] {
        ship.order = order;
        run(&mut ship, &mut tf, 8.0);
        ship.goal = Some(ship.heading + 3.0); // hard to port
        run(&mut ship, &mut tf, 3.0);
        let roll = tf.rotation.to_euler(EulerRot::YXZ).2;
        assert!(roll > 0.1 && roll < MAX_ROLL, "leaning into the turn, not yet at the limit: {roll}");
        assert!((ship.ang_vel.to_degrees() - 13.6).abs() < 0.4, "order {order:?} turn rate {}", ship.ang_vel.to_degrees());
        ship.goal = Some(ship.heading + 3.0);
        run(&mut ship, &mut tf, 2.0);
        assert!((tf.rotation.to_euler(EulerRot::YXZ).2 - MAX_ROLL).abs() < 1e-3, "full lean after about 4 s");
        assert!((tf.rotation * Vec3::NEG_Z).y.abs() < 1e-4, "roll never pitches the bow");
        ship.goal = None;
        ship.ang_vel = 0.0;
    }
}

/// Retail Navy Cutters with engineer and helmsman both at skill 5 and 9 (docs/re/crew.md): Ahead Full and full-rudder turn.
#[test]
#[ignore = "needs imported assets"]
fn crewed_cutter_matches_measured_retail_speed_and_turn() {
    let lib = crate::testutil::lib();
    let ci = lib.index("Ship_Navy_Cutter");
    let class = &lib.classes[ci];
    let h = Health { sections: vec![1.0; class.sections.len()], core: 1.0, destroyed: false };
    assert_eq!(crate::crew_arms::skill_factor(0.1), 1.0, "default hands change nothing");
    for (skill, full, turn) in [(0.5, 5.336, 15.61), (0.9, 6.041, 9.99)] {
        let f = crate::crew_arms::skill_factor(skill);
        let mut ship = Ship { heading: 0.0, order: EngineOrder::Full, crew_speed: f, crew_turn: f, ..Ship::new(ci, Side::Player) };
        let mut tf = Transform::default();
        for _ in 0..300 {
            step(&mut ship, &mut tf, class, &h, 0.0344);
        }
        assert!((ship.speed - full).abs() < 0.01, "skill {skill}: Full {} vs {full}", ship.speed);
        ship.goal = Some(ship.heading + 3.0);
        for _ in 0..90 {
            step(&mut ship, &mut tf, class, &h, 0.0344);
        }
        assert!((ship.ang_vel.to_degrees() - turn).abs() < 0.4, "skill {skill}: turn {} vs {turn}", ship.ang_vel.to_degrees());
    }
}

/// Retail full-rudder turns without crew on two classes (docs/re/movement.md table): speed and steady turn rate.
#[test]
#[ignore = "needs imported assets"]
fn uncrewed_turns_match_measured_retail() {
    let lib = crate::testutil::lib();
    for (name, order, speed, turn) in [
        ("Ship_Navy_Cutter", EngineOrder::Emergency, 9.056, 9.71),
        ("Ship_Navy_ManOWar", EngineOrder::Full, 2.341, 4.16),
        ("Ship_Navy_ManOWar", EngineOrder::Emergency, 4.684, 3.11),
    ] {
        let ci = lib.index(name);
        let class = &lib.classes[ci];
        let h = Health { sections: vec![1.0; class.sections.len()], core: 1.0, destroyed: false };
        let mut ship = Ship { heading: 0.0, order, ..Ship::new(ci, Side::Player) };
        let mut tf = Transform::default();
        for _ in 0..1500 {
            step(&mut ship, &mut tf, class, &h, 0.0344);
        }
        assert!((ship.speed - speed).abs() < 0.01, "{name} order {order:?}: speed {} vs {speed}", ship.speed);
        ship.goal = Some(ship.heading + 3.0);
        for _ in 0..300 {
            step(&mut ship, &mut tf, class, &h, 0.0344);
        }
        assert!((ship.ang_vel.to_degrees() - turn).abs() < 0.05, "{name} order {order:?}: turn {} vs {turn}", ship.ang_vel.to_degrees());
    }
}

#[test]
fn stop_is_frame_rate_independent_and_reaches_rest() {
    let rate = 18000.0 / 2000.0 / 11.111111; // retail Comet
    let mut speed = 11.111111;
    for _ in 0..50 {
        speed = approach_speed(speed, 0.0, rate, 0.1);
    }
    assert!(speed < 0.2);
    assert!((speed - approach_speed(11.111111, 0.0, rate, 5.0)).abs() < 0.0001);
    for _ in 0..50 {
        speed = approach_speed(speed, 0.0, rate, 0.1);
    }
    assert_eq!(speed, 0.0);
    assert!(approach_speed(0.0, 11.0, rate, 100.0) <= 11.0);
}

#[test]
#[ignore = "needs imported assets"]
fn ships_lean_into_turns_except_at_emergency_speed() {
    let lib = crate::testutil::lib();
    let ci = lib.index("Ship_Navy_Cutter");
    let class = &lib.classes[ci];
    let h = Health { sections: vec![1.0; class.sections.len()], core: 1.0, destroyed: false };
    let roll = |order| {
        let mut ship = Ship { heading: 0.0, order, waypoints: vec![Vec3::new(-200.0, 0.0, -10.0)], ..Ship::new(ci, Side::Player) };
        let mut tf = Transform::default();
        for _ in 0..60 {
            step(&mut ship, &mut tf, class, &h, 0.033);
        }
        tf.rotation.to_euler(EulerRot::YXZ).2.abs()
    };
    assert!(roll(EngineOrder::Full) > 0.05, "Ahead Full leans: {}", roll(EngineOrder::Full));
    assert!(roll(EngineOrder::Emergency) < 1e-4, "Emergency Speed stays level: {}", roll(EngineOrder::Emergency));
    assert!(roll(EngineOrder::Stop) < 1e-4, "a stopped ship stays level though its rudder is over: {}", roll(EngineOrder::Stop));
}

/// Border Dispute with every slot filled: allied computer fleets share the human's side but must still go for the enemy base,
/// while the human's own ships wait for orders.
#[test]
#[ignore = "needs imported assets"]
fn allied_computer_ships_hunt_the_enemy_base_the_humans_fleet_waits() {
    let (mut w, ids) = fleet_world();
    w.despawn(ids[5]);
    let class = w.resource::<Library>().index("Base_SmugglersCamp");
    let health = Health::new(&w.resource::<Library>().classes[class]);
    w.spawn((Transform::from_xyz(0.0, 0.0, -2000.0), combat::BaseSide(Side::Enemy), health, combat::ArmedBase { class }));
    w.insert_resource(crate::Config { speed: 1.0, ..default() });
    w.init_resource::<crate::environment::Sensors>();
    w.init_resource::<CaptainTargets>();
    w.init_resource::<Time>();
    // A captain first thinks one period (plus its start delay) after it appears, so run the system at 0 s and again at 1 s.
    let captains = w.register_system(ai);
    w.run_system(captains).unwrap();
    assert_eq!(w.get::<Ship>(ids[4]).unwrap().order, EngineOrder::Stop, "no captain has thought yet");
    w.resource_mut::<Time>().advance_by(std::time::Duration::from_secs(1));
    w.run_system(captains).unwrap();
    assert!(w.get::<Ship>(ids[4]).unwrap().order > EngineOrder::Stop, "the ally sets off for the enemy base");
    assert_eq!(w.get::<Ship>(ids[1]).unwrap().order, EngineOrder::Stop, "the human's own ship waits for orders");
}

#[test]
fn a_formation_swap_pass_gives_ships_the_slot_nearest_them_and_leaves_fitting_ones() {
    let slots = formation_slots(Formation::Diamond, &[10.0, 10.0, 10.0]);
    let on = |p: Vec3| (p, 0.0);
    // Each ship on its own slot, all heading with the leader: nothing moves.
    let mut s = slots.clone();
    swap_slots(&mut s, &[on(Vec3::ZERO), on(slots[1]), on(slots[2])]);
    assert_eq!(s, slots);
    // The second and third ships are on each other's slots: they trade.
    let mut s = slots.clone();
    swap_slots(&mut s, &[on(Vec3::ZERO), on(slots[2]), on(slots[1])]);
    assert_eq!((s[1], s[2]), (slots[2], slots[1]));
    // Equal fits never swap: both ships at the same spot keep their own slots.
    let mut s = slots.clone();
    let mid = (slots[1] + slots[2]) / 2.0;
    swap_slots(&mut s, &[on(Vec3::ZERO), on(mid), on(mid)]);
    assert_eq!(s, slots);
}

fn plain_ship() -> Ship {
    Ship {
        heading: 0.0,
        speed: 5.0,
        order: EngineOrder::Emergency,
        waypoints: vec![Vec3::new(10.0, 0.0, -50.0)],
        goal: Some(1.0),
        throttle: Some(0.5),
        ..Ship::new(0, Side::Player)
    }
}

#[test]
fn hold_and_stop_zero_the_controls_and_end_the_course() {
    let mut s = plain_ship();
    s.stand_still();
    assert_eq!((s.order, s.ordered()), (EngineOrder::Stop, 0.0));
    assert!(s.waypoints.is_empty() && s.goal.is_none(), "no leftover course for the helm to steer to");
}

#[test]
fn hold_ends_a_captains_destination_but_keeps_its_target_and_formation_hold() {
    let target = Entity::from_raw_u32(7).unwrap();
    let mut p = FleetPilot { hold: true, target: Some(target), destination: Some(Vec3::X), hold_fire: true, ..default() };
    p.hold_position();
    assert_eq!((p.destination, p.target, p.hold, p.hold_fire), (None, Some(target), true, true));
}

#[test]
fn stop_leaves_formation_hold_and_hold_fire_alone() {
    let target = Entity::from_raw_u32(7).unwrap();
    let mut p = FleetPilot { hold: true, target: Some(target), destination: Some(Vec3::X), hold_fire: true, ..default() };
    p.cancel_actions();
    assert_eq!((p.destination, p.target, p.hold, p.hold_fire), (None, None, true, true));
}

#[test]
fn a_held_ship_is_released_by_move_dock_tow_and_board_orders_only() {
    let mut w = World::new();
    let e = w.spawn(HoldPosition).id();
    // An attack order (or Stop) never calls `release_hold`; the order clicks that clear a hold all do.
    assert!(w.get::<HoldPosition>(e).is_some());
    let mut queue = bevy::ecs::world::CommandQueue::default();
    release_hold(&mut Commands::new(&mut queue, &w), e);
    queue.apply(&mut w);
    assert!(w.get::<HoldPosition>(e).is_none());
}

/// A dying Navy Cutter's drive (docs/re/effects.md "Death drive"): with every section at 0 HP and the computer's full throttle it
/// settles at 0.1 x MaxSpeed x crew factor (retail: 0.90 against 0.1 x 8.06 x 1.125 = 0.906), and a fast hull slows to that speed
/// through the ordinary drag law instead of stopping.
#[test]
#[ignore = "needs imported assets"]
fn dying_cutter_drifts_at_a_tenth_of_max_speed_and_a_fast_one_decays_to_it() {
    let lib = crate::testutil::lib();
    let ci = lib.index("Ship_Navy_Cutter");
    let class = &lib.classes[ci];
    let wreck = Health { sections: vec![0.0; class.sections.len()], core: 0.0, destroyed: true };
    let dt = 0.0344;
    let ship = |speed| Ship {
        speed,
        order: EngineOrder::Emergency,
        throttle: Some(1.0 / FULL_SPEED), // the computer's throttle 1.0
        crew_speed: 1.125,
        crew_turn: 1.125,
        ..Ship::new(ci, Side::Player)
    };

    let expected = 0.1 * class.phys.max_speed * 1.125;
    let (mut slow, mut fast) = (ship(0.0), ship(9.0));
    let (mut tf_slow, mut tf_fast) = (Transform::default(), Transform::default());
    let mut previous = fast.speed;
    for i in 0..(20.0 / dt) as usize {
        helm_step(&mut slow, &mut tf_slow, class, &wreck, dt, true);
        helm_step(&mut fast, &mut tf_fast, class, &wreck, dt, true);
        assert!(fast.speed <= previous + 1e-6, "a fast hull only slows (tick {i})");
        assert!(fast.speed >= expected - 1e-3, "it never undershoots the drift speed");
        previous = fast.speed;
    }
    assert!((slow.speed - expected).abs() < 0.01, "from rest {} vs {expected}", slow.speed);
    assert!((fast.speed - expected).abs() < 0.01, "from 9 u/s {} vs {expected}", fast.speed);
    assert!((expected - 0.906).abs() < 0.01 && (slow.speed - 0.90).abs() < 0.02, "retail measured 0.90: {}", slow.speed);
}

/// Steering for a clicked point, the rudder law runs once per think and the rudder is held between runs, so the heading
/// hunts around the line to the point instead of settling (retail Navy Cutter, no crew: +-2.2 deg, 3.8 s period).
#[test]
#[ignore = "needs imported assets"]
fn a_cutter_steering_for_a_point_ahead_weaves_instead_of_settling() {
    let lib = crate::testutil::lib();
    let ci = lib.index("Ship_Navy_Cutter");
    let class = &lib.classes[ci];
    let health = Health::new(class);
    let mut ship = Ship { heading: 1f32.to_radians(), speed: 4.525, order: EngineOrder::Full, waypoints: vec![Vec3::new(0.0, 0.0, -113.0)], ..Ship::new(ci, Side::Player) };
    let mut tf = Transform::default();
    let (mut lo, mut hi, mut crossings) = (f32::MAX, f32::MIN, vec![]);
    let mut last = ship.heading;
    for k in 0..2000 {
        helm_step(&mut ship, &mut tf, class, &health, 0.033, false);
        if ship.waypoints.is_empty() {
            break;
        }
        let t = k as f32 * 0.033;
        if t > 8.0 {
            (lo, hi) = (lo.min(ship.heading), hi.max(ship.heading));
        }
        if last * ship.heading < 0.0 {
            crossings.push(t);
        }
        last = ship.heading;
    }
    let swing = (hi - lo).to_degrees();
    assert!((3.0..7.0).contains(&swing), "heading swing {swing} deg");
    let period = 2.0 * (crossings[crossings.len() - 1] - crossings[1]) / (crossings.len() - 2) as f32;
    assert!((3.2..4.4).contains(&period), "period {period} s");
}

/// The tower's thrust works on both hulls: its top speed under tow is its own times mass / (mass + tow mass).
#[test]
#[ignore = "needs imported assets"]
fn a_tower_is_slowed_by_the_mass_it_tows() {
    let lib = crate::testutil::lib();
    let ci = lib.index("Ship_Navy_Cutter");
    let class = &lib.classes[ci];
    let health = Health::new(class);
    let run = |tow_mass: f32| {
        let mut ship = Ship { order: EngineOrder::Full, tow_mass, ..Ship::new(ci, Side::Player) };
        let mut tf = Transform::default();
        for _ in 0..600 {
            helm_step(&mut ship, &mut tf, class, &health, 0.033, true);
        }
        ship.speed
    };
    let ratio = run(class.phys.mass) / run(0.0);
    assert!((ratio - 0.5).abs() < 0.01, "{ratio}");
}

#[test]
fn an_attack_order_ends_hold_fire_on_that_ship() {
    let mut p = FleetPilot { hold_fire: true, ..default() };
    p.attack(Entity::from_bits(7 | 1 << 32));
    assert!(!p.hold_fire);
    assert_eq!(p.target, Some(Entity::from_bits(7 | 1 << 32)));
}

/// The Fleet Commands buttons and P as commands (`commands::Kind`) do what the direct calls did.
#[test]
fn fleet_orders_as_commands_do_what_the_buttons_did() {
    use crate::commands::{Kind, apply};
    let pilots = |w: &World, ids: &[Entity; 6]| ids.iter().map(|e| w.get::<FleetPilot>(*e).cloned()).collect::<Vec<_>>();
    // `fleet_world` without imported assets: one synthetic hull for all six ships.
    let world = || {
        let mut w = World::new();
        let class = crate::fixed_tests::synthetic_class();
        let ids: [Entity; 6] = std::array::from_fn(|i| {
            let side = if i == 5 { Side::Enemy } else { Side::Player };
            let owner = ["Player", "Player", "Player", "Player", "Ally", "Enemy"][i];
            w.spawn((Ship { heading: 0.0, ..Ship::new(0, side) }, Health::new(&class), Transform::from_xyz(i as f32 * 50.0, 0.0, 0.0), FleetMember(owner.into(), i))).id()
        });
        w.entity_mut(ids[0]).insert(PlayerShip);
        w.insert_resource(Library { classes: vec![class], guns: default(), bullets: default(), names: default(), clips: default() });
        w.insert_resource(Target::default());
        w.insert_resource(Orbit { yaw: 0.0, pitch: 0.0, dist: 60.0, focus: None });
        w.insert_resource(Fleets { local: "Player".into(), ..default() });
        reconcile_fleets(&mut w);
        (w, ids)
    };
    let (mut direct, ids) = world();
    let (mut sent, again) = world();
    assert_eq!(ids, again);
    for w in [&mut direct, &mut sent] {
        w.resource_mut::<Target>().0 = Some(ids[5]);
    }
    set_formation(&mut direct, "Player", Some(Formation::Column), true);
    apply(&mut sent, crate::commands::PlayerId(0), &Kind::Formation { formation: Formation::Column, hold: true });
    assert!(pilots(&sent, &ids) == pilots(&direct, &ids));
    assert_eq!(sent.resource::<Fleets>().groups["Player"].formation, Formation::Column);
    fleet_help(&mut direct, "Player", true);
    apply(&mut sent, crate::commands::PlayerId(0), &Kind::FleetAttack);
    assert!(pilots(&sent, &ids) == pilots(&direct, &ids));
    assert!(ids[1..4].iter().all(|e| sent.get::<FleetPilot>(*e).unwrap().target == Some(ids[5])));
    fleet_help(&mut direct, "Player", false);
    apply(&mut sent, crate::commands::PlayerId(0), &Kind::CeaseFire);
    assert!(pilots(&sent, &ids) == pilots(&direct, &ids));
    assert!(sent.resource::<Fleets>().groups["Player"].cease_fire);
    assert!(promote(&mut direct, ids[2], false));
    apply(&mut sent, crate::commands::PlayerId(0), &Kind::Promote { ship: ids[2] });
    assert!(pilots(&sent, &ids) == pilots(&direct, &ids));
    assert_eq!(sent.resource::<Fleets>().primary(), Some(ids[2]));
    assert!(sent.get::<PlayerShip>(ids[2]).is_some() && sent.get::<PlayerShip>(ids[0]).is_none());
}

/// Each stance's own pick (PROVISIONAL): Aggressive the nearest ship or dragon in scan range, else a base (any range in a skirmish,
/// scan range in a campaign); Persistent, Defensive and Neutral only a hostile in scan range whose target is this ship; Dummy nothing.
#[test]
fn each_stance_picks_its_own_kind_of_target() {
    use crate::rules::Stance::*;
    let e = |i| Entity::from_raw_u32(i).unwrap();
    let seen = |i, dist, base, in_scan, targets_me| Seen { e: e(i), dist, base, in_scan, targets_me };
    let world = [seen(1, 50.0, false, true, false), seen(2, 80.0, false, true, true), seen(3, 20.0, false, false, true), seen(4, 10.0, true, false, false)];
    assert_eq!(stance_pick(Aggressive, false, &world), Some(e(1)), "the nearest ship in scan range, before a nearer base");
    for st in [Persistent, Defensive, Neutral] {
        assert_eq!(stance_pick(st, false, &world), Some(e(2)), "{st:?}: only one that targets it, in scan range");
    }
    assert_eq!(stance_pick(Dummy, false, &world), None);
    let bases = [seen(4, 10.0, true, false, false)];
    assert_eq!(stance_pick(Aggressive, false, &bases), Some(e(4)), "skirmish: a base at any range");
    assert_eq!(stance_pick(Aggressive, true, &bases), None, "campaign: only in scan range");
    assert_eq!(stance_pick(Neutral, false, &bases), None, "a base never retaliates here (it keeps no target)");
}

/// With no target an Aggressive ship explores, a Defensive or Neutral one stops dead unless it has waypoints (a formation is the
/// fleet's own, and the captain leaves it alone).
#[test]
fn without_a_target_aggressive_explores_and_the_others_stop_unless_on_a_course_or_in_formation() {
    use crate::rules::Stance::*;
    let mut w = captain_world(false);
    let ship = |w: &mut World, stance, waypoints: Vec<Vec3>| {
        let e = captain_ship(w, Side::Enemy, Vec3::ZERO);
        w.entity_mut(e).insert(campaign::MissionShip { stance, ..campaign::MissionShip::placed(1, "s".into(), "o".into()) });
        let mut s = w.get_mut::<Ship>(e).unwrap();
        (s.waypoints, s.order) = (waypoints, EngineOrder::Full);
        e
    };
    let hunter = ship(&mut w, Aggressive, vec![]);
    let (still, sailing) = (ship(&mut w, Defensive, vec![]), ship(&mut w, Neutral, vec![Vec3::X * 300.0]));
    let held = ship(&mut w, Neutral, vec![]);
    w.entity_mut(held).insert(FleetPilot { hold: true, ..default() });
    think(&mut w);
    let at = |e| (w.get::<Ship>(e).unwrap().order, w.get::<Ship>(e).unwrap().waypoints.len());
    assert_eq!(at(hunter), (EngineOrder::Emergency, 1), "explores");
    assert_eq!(at(still), (EngineOrder::Stop, 0), "stops dead");
    assert_eq!(at(sailing), (EngineOrder::Full, 1), "keeps its course");
    assert_eq!(at(held), (EngineOrder::Full, 0), "in formation: the fleet's");
}

/// An Aggressive ship with no working gun and nothing to attack in reach rams the nearest hostile ship at any range; a friend's target
/// comes before that.
#[test]
fn a_gunless_aggressive_ship_rams_after_taking_a_friends_target() {
    let mut w = captain_world(false);
    let ship = captain_ship(&mut w, Side::Enemy, Vec3::ZERO);
    let far = captain_ship(&mut w, Side::Player, Vec3::X * 5000.0);
    think(&mut w);
    assert_eq!(w.resource::<CaptainTargets>().of(ship), Some(far), "rams the nearest hostile, out of sight");
    let mut w = captain_world(false);
    let (ship, mate) = (captain_ship(&mut w, Side::Enemy, Vec3::ZERO), captain_ship(&mut w, Side::Enemy, Vec3::X * 10.0));
    let (_far, other) = (captain_ship(&mut w, Side::Player, Vec3::X * 5000.0), captain_ship(&mut w, Side::Player, Vec3::Z * 7000.0));
    for e in [ship, mate] {
        w.entity_mut(e).insert(FleetMember("computer".into(), 0));
    }
    w.resource_mut::<CaptainTargets>().0.insert(mate, (other, 0.0, Vec3::ZERO, false));
    w.entity_mut(mate).insert(crate::docking::Docker::repair(other)); // keeps the mate's target as it is
    think(&mut w);
    assert_eq!(w.resource::<CaptainTargets>().of(ship), Some(other), "a fleet mate's target first");
}

/// Persistent ships never dock for repair; the other stances may.
#[test]
fn a_persistent_or_dummy_ship_never_docks_for_repair() {
    use crate::rules::Stance::*;
    assert!([None, Some(Aggressive), Some(Defensive), Some(Neutral)].into_iter().all(crate::docking::seeks_repair));
    assert!(!crate::docking::seeks_repair(Some(Persistent)) && !crate::docking::seeks_repair(Some(Dummy)));
}

/// A Persistent ship on a path keeps to it while it fires at the ship that targets it; once latched it holds position when the path
/// ends, as long as the target lasts. Without a path it attacks like any ship; a formation comes first.
#[test]
fn a_persistent_ship_fires_from_its_path_then_holds_and_attacks_without_one() {
    let mut w = captain_world(false);
    let foe = captain_ship(&mut w, Side::Player, Vec3::ZERO);
    let persistent = |w: &mut World, at: Vec3, path: Vec<Vec3>| {
        let e = captain_ship(w, Side::Enemy, at);
        w.entity_mut(e).insert(campaign::MissionShip { stance: crate::rules::Stance::Persistent, ..campaign::MissionShip::placed(1, "p".into(), "o".into()) });
        let mut s = w.get_mut::<Ship>(e).unwrap();
        (s.waypoints, s.order) = (path, EngineOrder::Full);
        e
    };
    let route = vec![Vec3::X * 900.0];
    let walker = persistent(&mut w, Vec3::X * 30.0, route.clone());
    think(&mut w);
    assert_eq!(w.resource::<CaptainTargets>().of(walker), Some(foe), "it answers the ship that targets it");
    assert_eq!((w.get::<Ship>(walker).unwrap().waypoints.clone(), w.get::<Ship>(walker).unwrap().order), (route, EngineOrder::Full), "on its path");
    w.get_mut::<Ship>(walker).unwrap().waypoints.clear(); // the path ends
    think(&mut w);
    assert_eq!(w.get::<Ship>(walker).unwrap().order, EngineOrder::Stop, "latched: holds position, target kept");
    assert_eq!(w.resource::<CaptainTargets>().of(walker), Some(foe));

    let mut w = captain_world(false);
    let _foe = captain_ship(&mut w, Side::Player, Vec3::ZERO);
    let free = persistent(&mut w, Vec3::X * 30.0, vec![]);
    let formed = persistent(&mut w, Vec3::Z * 60.0, vec![]);
    w.entity_mut(formed).insert(FleetPilot { hold: true, ..default() });
    think(&mut w);
    assert_eq!(w.get::<Ship>(free).unwrap().order, EngineOrder::Emergency, "no path: it attacks");
    assert_eq!(w.get::<Ship>(formed).unwrap().order, EngineOrder::Full, "a formation comes first");
}
