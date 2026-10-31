use super::*;
pub fn fleet_world() -> (World, [Entity; 6]) {
    let mut w = World::new();
    let lib = Library::load(assets_dir());
    let ci = lib.index("Ship_Navy_Cutter");
    let class = &lib.classes[ci];
    let ids = std::array::from_fn(|i| {
        w.spawn((
            Ship {
                class: ci,
                side: if i == 5 { Side::Enemy } else { Side::Player },
                heading: 0.0,
                speed: 0.0,
                ang_vel: 0.0,
                order: 0,
                waypoints: vec![],
                goal: None,
                cap: 1.0,
                throttle: None,
                sail_energy: 1.0,
                crew_speed: 1.0,
                crew_turn: 1.0,
                avoid: 1.0,
            },
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
    let ms = |id, owner: &str, name: &str| campaign::MissionShip {
        id,
        owner: owner.into(),
        name: name.into(),
        hold_fire: false,
        invuln: 0.0,
        visible: true,
        stance: rules::Stance::Neutral,
        target: None,
        attack: vec![],
        flag: String::new(),
        banner: String::new(),
        looping: None,
        stop_at_end: false,
        create_lifeboats: false,
        ..Default::default()
    };
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
    assert!((0.0..=1.0).contains(&w.get::<Ship>(ids[1]).unwrap().throttle.unwrap()));
    w.get_mut::<Transform>(ids[1]).unwrap().translation.y = 25.0;
    w.get_mut::<Ship>(ids[1]).unwrap().cap = 0.2;
    w.get_mut::<Health>(ids[1]).unwrap().sections.iter_mut().for_each(|h| *h *= 0.25);
    fleet_follow(&mut w);
    assert_eq!(w.get::<Ship>(ids[1]).unwrap().waypoints[0].y, 25.0);
    assert_eq!(w.get::<Ship>(ids[1]).unwrap().cap, 0.2);
    assert!((0.0..=1.0).contains(&w.get::<Ship>(ids[1]).unwrap().throttle.unwrap()));
    set_formation(&mut w, "Player", Some(Formation::None), false);
    assert!(ids[1..4].iter().all(|e| !w.get::<FleetPilot>(*e).unwrap().hold));
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
    press(&mut w, KeyCode::Digit2);
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
    let lib = combat::Library::load(crate::assets_dir());
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
    assert!(step(16.0, 1.0 / 60.0) <= 0.1 + 1e-4); // vsync frame: capped
    assert!((step(1.0, 0.2) - 0.2).abs() < 1e-4); // 1x keeps Bevy's normal 0.25 s hitch clamp
}

#[test]
#[ignore = "needs imported assets"]
fn ship_keeps_speed_and_heading_past_last_waypoint() {
    let lib = combat::Library::load(crate::assets_dir());
    // Not class 0: the sorted ship folders now start with the (immobile) armed bases.
    let ci = lib.index("Ship_Navy_Cutter");
    let class = &lib.classes[ci];
    let mut ship = Ship {
        class: ci,
        side: Side::Player,
        heading: 0.0,
        speed: 0.0,
        ang_vel: 0.0,
        order: 3,
        waypoints: vec![Vec3::new(-30.0, 0.0, -80.0)],
        goal: None,
        cap: 1.0,
        throttle: None,
        sail_energy: 1.0,
        crew_speed: 1.0,
        crew_turn: 1.0,
        avoid: 1.0,
    };
    let mut tf = Transform::default();
    let h = Health { sections: vec![1.0; class.sections.len()], core: 1.0, destroyed: false };
    for _ in 0..3000 {
        step(&mut ship, &mut tf, class, &h, 0.033);
    }
    assert!(ship.waypoints.is_empty());
    assert!(ship.order == 3 && ship.speed > 1.0, "speed {}", ship.speed);
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
    assert_eq!(choose_target(None, &[(a, 90.0), (b, 40.0)], alive), Some((b, false)), "no target: the nearest");
    assert_eq!(choose_target(Some((a, 5.9)), &[(a, 90.0), (b, 40.0)], alive), Some((a, true)), "kept inside 6 s though farther");
    assert_eq!(choose_target(Some((a, 5.9)), &[(b, 40.0)], alive), Some((a, true)), "a held target is followed out of sight");
    assert_eq!(choose_target(Some((a, 6.0)), &[(a, 90.0), (b, 40.0)], alive), Some((b, false)), "re-chosen after 6 s");
    assert_eq!(choose_target(Some((a, 1.0)), &[(b, 40.0)], |t| t != a), Some((b, false)), "a sunk target is replaced at once");
    assert_eq!(choose_target(Some((a, 1.0)), &[], |t| t != a), None);
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
    let lib = combat::Library::load(crate::assets_dir());
    let foe = Vec3::ZERO;
    // Minimum throttle: the measured Carrack's save holds 0.745; the Barque takes the range's mean (each ship draws 0.65-0.75).
    // ponytail: aiming at the Cutter's nearest section (as the original), the circles run 42-49 (Carrack) and 43-73 (Barque): the right
    // size, but the Barque breathes more than retail. Not the aim (centre-aimed it was 43-72); left: our turn model (movement.md).
    for (name, min_throttle, lo_ok, hi_ok) in [("Ship_Pirate_Carrack", 0.745, 38.0, 53.0), ("Ship_Pirate_Barque", 0.7, 40.0, 76.0)] {
        let ci = lib.index(name);
        let class = &lib.classes[ci];
        let h = Health { sections: class.sections.iter().map(|s| s.hp).collect(), core: class.core_hp, destroyed: false };
        let (routine, range) = attack::plan(&lib, class, &h, None).unwrap();
        let mut ship = Ship {
            class: ci,
            side: Side::Enemy,
            heading: 0.0,
            speed: 0.0,
            ang_vel: 0.0,
            order: 0,
            waypoints: vec![],
            goal: None,
            cap: 1.0,
            throttle: None,
            sail_energy: 1.0,
            crew_speed: COMPUTER_CREW,
            crew_turn: COMPUTER_TURN,
            avoid: 1.0,
        };
        let mut tf = Transform::from_xyz(30.0, 0.0, 400.0);
        let cutter = &lib.classes[lib.index("Ship_Navy_Cutter")].sections;
        let cutter_hp: Vec<f32> = cutter.iter().map(|s| s.hp).collect();
        let dt = 1.0 / 30.0;
        let e = Entity::from_raw_u32(3).unwrap();
        let (mut dists, mut beams) = (vec![], vec![]);
        for i in 0..(200.0 / dt) as usize {
            let gap = tf.translation.length() - class.radius;
            let at = attack::aim(cutter, &cutter_hp, &Transform::IDENTITY, tf.translation, false);
            let course = attack::course(routine, tf.translation, ship.heading, at, gap, range[0], i as f32 * dt, &mut None);
            let throttle = attack::throttle(e, tf.translation, ship.heading, course[0]).map(|_| 2.0 * min_throttle);
            (ship.throttle, ship.waypoints, ship.goal, ship.order) = (throttle, course, None, 3);
            step(&mut ship, &mut tf, class, &h, dt);
            if i as f32 * dt > 120.0 {
                let d = foe - tf.translation;
                dists.push(Vec2::new(d.x, d.z).length());
                beams.push(angdiff((-d.x).atan2(-d.z), ship.heading));
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
    let lib = combat::Library::load(crate::assets_dir());
    let ci = lib.index("Ship_Navy_Cutter");
    let class = &lib.classes[ci];
    let h = Health { sections: vec![1.0; class.sections.len()], core: 1.0, destroyed: false };
    let dt = 0.0344; // retail update interval
    let run = |ship: &mut Ship, tf: &mut Transform, secs: f32| {
        for _ in 0..(secs / dt) as usize {
            step(ship, tf, class, &h, dt);
        }
    };
    let mut ship = Ship {
        class: ci,
        side: Side::Player,
        heading: 0.0,
        speed: 0.0,
        ang_vel: 0.0,
        order: 1,
        waypoints: vec![],
        goal: None,
        cap: 1.0,
        throttle: None,
        sail_energy: 1.0,
        crew_speed: 1.0,
        crew_turn: 1.0,
        avoid: 1.0,
    };
    let mut tf = Transform::default();
    run(&mut ship, &mut tf, 1.0);
    assert!((ship.speed - 1.95).abs() < 0.1, "Slow from rest after 1 s: {}", ship.speed); // 0.53 s time constant
    for (order, measured) in [(1, 2.2655), (2, 4.531), (3, 9.0623)] {
        ship.order = order;
        run(&mut ship, &mut tf, 8.0);
        assert!((ship.speed - measured).abs() < 0.01, "order {order}: {} vs {measured}", ship.speed);
    }
    ship.order = 2;
    run(&mut ship, &mut tf, 8.0);
    ship.order = 0;
    run(&mut ship, &mut tf, 0.75); // retail: 23 ticks of decay to 1.05, then rest on the 24th (0.83 s)
    assert!(ship.speed > 1.0, "still slowing after 0.75 s: {}", ship.speed);
    run(&mut ship, &mut tf, 0.15);
    assert_eq!(ship.speed, 0.0, "All Stop from Full is at rest after about 0.83 s");
    for order in [1, 2] {
        ship.order = order;
        run(&mut ship, &mut tf, 8.0);
        ship.goal = Some(ship.heading + 3.0); // hard to port
        run(&mut ship, &mut tf, 3.0);
        let roll = tf.rotation.to_euler(EulerRot::YXZ).2;
        assert!(roll > 0.1 && roll < MAX_ROLL, "leaning into the turn, not yet at the limit: {roll}");
        assert!((ship.ang_vel.to_degrees() - 13.6).abs() < 0.4, "order {order} turn rate {}", ship.ang_vel.to_degrees());
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
    let lib = combat::Library::load(crate::assets_dir());
    let ci = lib.index("Ship_Navy_Cutter");
    let class = &lib.classes[ci];
    let h = Health { sections: vec![1.0; class.sections.len()], core: 1.0, destroyed: false };
    assert_eq!(crate::crew_arms::skill_factor(0.1), 1.0, "default hands change nothing");
    for (skill, full, turn) in [(0.5, 5.336, 15.61), (0.9, 6.041, 9.99)] {
        let f = crate::crew_arms::skill_factor(skill);
        let mut ship = Ship {
            class: ci,
            side: Side::Player,
            heading: 0.0,
            speed: 0.0,
            ang_vel: 0.0,
            order: 2,
            waypoints: vec![],
            goal: None,
            cap: 1.0,
            throttle: None,
            sail_energy: 1.0,
            crew_speed: f,
            crew_turn: f,
            avoid: 1.0,
        };
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
    let lib = combat::Library::load(crate::assets_dir());
    for (name, order, speed, turn) in [("Ship_Navy_Cutter", 3, 9.056, 9.71), ("Ship_Navy_ManOWar", 2, 2.341, 4.16), ("Ship_Navy_ManOWar", 3, 4.684, 3.11)] {
        let ci = lib.index(name);
        let class = &lib.classes[ci];
        let h = Health { sections: vec![1.0; class.sections.len()], core: 1.0, destroyed: false };
        let mut ship = Ship {
            class: ci,
            side: Side::Player,
            heading: 0.0,
            speed: 0.0,
            ang_vel: 0.0,
            order,
            waypoints: vec![],
            goal: None,
            cap: 1.0,
            throttle: None,
            sail_energy: 1.0,
            crew_speed: 1.0,
            crew_turn: 1.0,
            avoid: 1.0,
        };
        let mut tf = Transform::default();
        for _ in 0..1500 {
            step(&mut ship, &mut tf, class, &h, 0.0344);
        }
        assert!((ship.speed - speed).abs() < 0.01, "{name} order {order}: speed {} vs {speed}", ship.speed);
        ship.goal = Some(ship.heading + 3.0);
        for _ in 0..300 {
            step(&mut ship, &mut tf, class, &h, 0.0344);
        }
        assert!((ship.ang_vel.to_degrees() - turn).abs() < 0.05, "{name} order {order}: turn {} vs {turn}", ship.ang_vel.to_degrees());
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
    let lib = combat::Library::load(crate::assets_dir());
    let ci = lib.index("Ship_Navy_Cutter");
    let class = &lib.classes[ci];
    let h = Health { sections: vec![1.0; class.sections.len()], core: 1.0, destroyed: false };
    let roll = |order| {
        let mut ship = Ship {
            class: ci,
            side: Side::Player,
            heading: 0.0,
            speed: 0.0,
            ang_vel: 0.0,
            order,
            waypoints: vec![Vec3::new(-200.0, 0.0, -10.0)],
            goal: None,
            cap: 1.0,
            throttle: None,
            sail_energy: 1.0,
            crew_speed: 1.0,
            crew_turn: 1.0,
            avoid: 1.0,
        };
        let mut tf = Transform::default();
        for _ in 0..60 {
            step(&mut ship, &mut tf, class, &h, 0.033);
        }
        tf.rotation.to_euler(EulerRot::YXZ).2.abs()
    };
    assert!(roll(2) > 0.05, "Ahead Full leans: {}", roll(2));
    assert!(roll(3) < 1e-4, "Emergency Speed stays level: {}", roll(3));
}

/// Border Dispute with every slot filled: allied computer fleets share the human's side but must still go for the enemy base,
/// while the human's own ships wait for orders.
#[test]
#[ignore = "needs imported assets"]
fn allied_computer_ships_hunt_the_enemy_base_the_humans_fleet_waits() {
    use bevy::ecs::system::RunSystemOnce;
    let (mut w, ids) = fleet_world();
    w.despawn(ids[5]);
    let class = w.resource::<Library>().index("Base_SmugglersCamp");
    let health = Health::new(&w.resource::<Library>().classes[class]);
    w.spawn((Transform::from_xyz(0.0, 0.0, -2000.0), combat::BaseSide(Side::Enemy), health, combat::ArmedBase { class }));
    w.insert_resource(crate::Config {
        groups: vec![],
        player: String::new(),
        allies: vec![],
        enemies: vec![],
        map: None,
        shot: None,
        frame: 0,
        speed: 1.0,
        ended: None,
        script: None,
        autolaunch: false,
        shot_frame: 0,
        late_frame: 0,
    });
    w.init_resource::<crate::environment::Sensors>();
    w.init_resource::<CaptainTargets>();
    w.init_resource::<Time>();
    w.run_system_once(ai).unwrap();
    assert!(w.get::<Ship>(ids[4]).unwrap().order > 0, "the ally sets off for the enemy base");
    assert_eq!(w.get::<Ship>(ids[1]).unwrap().order, 0, "the human's own ship waits for orders");
}
