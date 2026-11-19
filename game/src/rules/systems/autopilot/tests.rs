use super::*;

#[test]
fn a_wedged_bot_backs_off_sideways_then_resumes_its_goal() {
    let mut cell = None;
    let goal = Some(Vec3::new(1000.0, 0.0, 0.0));
    let at = Vec3::ZERO;
    // Moving: no detour.
    assert_eq!(unstick(0.0, at, goal, &mut cell), None);
    assert_eq!(unstick(10.0, Vec3::new(100.0, 0.0, 0.0), goal, &mut cell), None);
    // Pinned in place past STUCK_SECS: a detour behind and to one side of the course.
    let pinned = Vec3::new(100.0, 0.0, 0.0);
    assert_eq!(unstick(25.0, pinned, goal, &mut cell), None);
    let d = unstick(31.0, pinned, goal, &mut cell).expect("detour");
    assert!(d.x < pinned.x && d.z.abs() > 100.0, "{d:?}");
    // Kept until it runs out, then the goal stands again.
    assert_eq!(unstick(35.0, pinned, goal, &mut cell), Some(d));
    let over = 31.0 + DETOUR_STEP / DETOUR_SPEED + 1.0;
    assert_eq!(unstick(over, pinned, goal, &mut cell), None);
    // Wedged again no nearer the goal: the same side, a longer step; after MAX_TRIES the other side.
    let mut t = over + STUCK_SECS + 1.0;
    let d2 = unstick(t, pinned, goal, &mut cell).expect("second detour");
    assert!(d2.z * d.z > 0.0 && d2.distance(pinned) > d.distance(pinned), "{d:?} {d2:?}");
    let mut last = d2;
    for _ in 2..=MAX_TRIES {
        t += 10_000.0 / DETOUR_SPEED;
        assert_eq!(unstick(t, pinned, goal, &mut cell), None, "the detour ran out");
        t += STUCK_SECS + 1.0;
        last = unstick(t, pinned, goal, &mut cell).expect("another detour");
    }
    assert!(last.z * d.z < 0.0 && (last.distance(pinned) - d.distance(pinned)).abs() < 1.0, "{d:?} {last:?}");
    // A near goal never counts as wedged.
    let mut cell = None;
    assert_eq!(unstick(0.0, at, Some(Vec3::new(100.0, 0.0, 0.0)), &mut cell), None);
    assert_eq!(unstick(60.0, at, Some(Vec3::new(100.0, 0.0, 0.0)), &mut cell), None);
    // A tower hold stalls only when the target's damage stays put for HOLD_STALL_SECS of continuous holding.
    let mut hold = None;
    let mut ticks = |from: f32, to: f32, damage: f32| (0..=((to - from) as i32)).any(|i| hold_stalled(from + i as f32, damage, &mut hold));
    assert!(!ticks(0.0, 20.0, 0.5));
    assert!(!ticks(21.0, 45.0, 0.6), "damage rising restarts the clock");
    assert!(ticks(46.0, 60.0, 0.6));
    assert!(!ticks(100.0, 120.0, 0.6), "leaving the hold starts over");
}

#[test]
fn the_bot_routes_round_an_island_instead_of_into_it() {
    // A wall of land from z = -200 to 200 at x = 50, between the ship at the origin and its goal at x = 150.
    let (a, b, c, d) = (Vec3::new(50.0, -20.0, -200.0), Vec3::new(50.0, 60.0, -200.0), Vec3::new(50.0, 60.0, 200.0), Vec3::new(50.0, -20.0, 200.0));
    let (w1, w2) = (crate::collision::Obstacle::from_triangles(&[[a, b, c]]), crate::collision::Obstacle::from_triangles(&[[a, c, d]]));
    let land = [&w1, &w2];
    let (from, goal) = (Vec3::ZERO, Vec3::new(150.0, 0.0, 0.0));
    let mut st = State::default();
    let next = route(&mut st, &land, 15.0, from, goal);
    assert!(next.z.abs() > 200.0, "steers for the wall's end, not through it: {next}");
    let (_, path) = st.route.clone().unwrap();
    assert_eq!(*path.last().unwrap(), goal);
    assert!(path.windows(2).all(|p| line_clear(&mut st.blocked, &land, 15.0, p[0], p[1])), "every leg is clear");
    // Wedged on the way round: the cells it was steering into are closed and the next plan keeps out of them.
    let at = Vec3::new(30.0, 0.0, next.z.signum() * 180.0);
    let closed: Vec<_> = (1..=WEDGED_CELLS).map(|i| cell_of(at + (next - at).with_y(0.0).normalize() * CELL * i as f32)).collect();
    wedged(&mut st, at, next);
    assert!(st.route.is_none(), "a new plan");
    route(&mut st, &land, 15.0, at, goal);
    let (_, path) = st.route.clone().unwrap();
    assert!(!path.is_empty() && path.iter().all(|q| !closed.contains(&cell_of(*q))), "{closed:?} {path:?}");
    // Open water: straight at the goal, no plan.
    let mut st = State::default();
    assert_eq!(route(&mut st, &[], 15.0, from, goal), goal);
    assert!(st.route.is_none());
    // A goal no route reaches (walled in on every side): steer for it, and keep the failed plan instead of searching every tick.
    let mut st = State::default();
    let gc = cell_of(goal);
    // Arriving within two cells counts, so the wall is the ring three cells out.
    for (dx, dz) in (-3..=3).flat_map(|x| (-3..=3).map(move |z| (x, z))).filter(|(x, z): &(i32, i32)| x.abs().max(z.abs()) == 3) {
        st.blocked.insert((gc.0 + dx, gc.1 + dz), true);
    }
    for _ in 0..2 {
        assert_eq!(route(&mut st, &[], 15.0, from, goal), goal);
        assert!(st.route.as_ref().is_some_and(|(c, p)| *c == gc && p.is_empty()), "the failed plan is remembered");
    }
}

/// The stockade gate and the pirates moored round the island are hidden from most sides; the autopilot picks the nearest
/// open-water spot with a clear shot instead of firing into the island.
#[test]
fn a_target_behind_an_island_is_shot_from_a_spot_that_sees_it() {
    // An island: a tall wall at x = 50 from z = -200 to 200, and a ground slab under x 50..60 (land, not water).
    let (h, l) = (Vec3::new(50.0, -20.0, -200.0), Vec3::new(50.0, 60.0, 200.0));
    let wall = crate::collision::Obstacle::from_triangles(&[
        [h, Vec3::new(50.0, 60.0, -200.0), l],
        [h, l, Vec3::new(50.0, -20.0, 200.0)],
        [Vec3::new(50.0, 0.0, -200.0), Vec3::new(60.0, 0.0, -200.0), Vec3::new(60.0, 0.0, 200.0)],
        [Vec3::new(50.0, 0.0, -200.0), Vec3::new(60.0, 0.0, 200.0), Vec3::new(50.0, 0.0, 200.0)],
    ]);
    let sight = Sight { guns: vec![], land: vec![&wall] };
    let target = Vec3::new(100.0, GUN_HEIGHT, 0.0);
    assert!(!sight.clear(Vec3::new(0.0, GUN_HEIGHT, 0.0), target), "the wall hides it from the west");
    assert!(sight.clear(Vec3::new(150.0, GUN_HEIGHT, 0.0), target));
    assert!(!sight.open(Vec3::new(55.0, 0.0, 0.0)) && sight.open(Vec3::new(0.0, 0.0, 0.0)));
    let spot = sight.firing_spot(target, Vec3::new(0.0, 0.0, 0.0), 40.0).expect("a spot east of the wall");
    assert!(spot.x > 60.0 && (spot.distance(target.with_y(0.0)) - 40.0).abs() < 1e-3, "{spot}");
    assert!(sight.clear(spot + Vec3::Y * GUN_HEIGHT, target));
    // A section just inside the hull still counts as seen (shells strike a hair past the surface).
    assert!(sight.clear(Vec3::new(0.0, GUN_HEIGHT, 0.0), Vec3::new(50.0 + SIGHT_SLACK * 0.5, GUN_HEIGHT, 0.0)));
}

#[test]
fn a_tower_is_faced_with_the_gun_nearest_the_bow() {
    use std::f32::consts::FRAC_PI_2;
    let (port, starboard) = (Quat::from_rotation_y(FRAC_PI_2), Quat::from_rotation_y(-FRAC_PI_2));
    assert!((mount_angle(Quat::IDENTITY)).abs() < 1e-5);
    assert!((mount_angle(port) - FRAC_PI_2).abs() < 1e-5);
    // Mission 2: a forward gun keeps the bow on the tower.
    assert!((tower_face(1.0, &[mount_angle(port), 0.0, mount_angle(starboard)]) - 1.0).abs() < 1e-5);
    // Mission 6: broadside guns only, so the side turns to the gate; the gun's world direction is the bow-on heading.
    let h = tower_face(1.0, &[mount_angle(port), mount_angle(starboard)]);
    let dir = Quat::from_rotation_y(h) * port * Vec3::NEG_Z;
    let want = Quat::from_rotation_y(1.0) * Vec3::NEG_Z;
    assert!(dir.distance(want) < 1e-4, "{dir} points at {want}");
    assert_eq!(tower_face(0.5, &[]), 0.5);
}

#[test]
fn a_tow_task_names_its_ship_by_name_suffix_or_in_its_text() {
    let task = |name: &str, text: &str| super::super::eval::Task { name: name.into(), text: text.into(), active: true, complete: false, failed: false };
    let unit = |n: u32, name: &str| super::super::eval::Unit {
        e: Entity::from_raw_u32(n).unwrap(),
        id: n as i64,
        name: name.into(),
        owner: "Navy".into(),
        owner_is_named_group: false,
        groups: vec![],
        pos: Vec3::ZERO,
        alive: true,
        visible: true,
        primary: n == 1,
        damage: 0.0,
        flag: String::new(),
        attack: vec![],
    };
    let w = [unit(1, "RLS Sea Sparrow"), unit(2, ""), unit(3, "RLS Bachanalia"), unit(4, "Harvester")];
    let me = w[0].e;
    // Mission 4: the task name says "tender", the text names the ship.
    let m4 = task("Tow tender to safety", "Tow the RLS Bachanalia to the Rally Point.");
    assert_eq!(task_towee(&m4, &w, me).map(|u| u.e), Some(w[2].e));
    assert_eq!(task_towee(&task("TowHarvester", ""), &w, me).map(|u| u.e), Some(w[3].e), "a name suffix still wins");
    assert!(task_towee(&task("Tow the wreck", "Tow the wreck home."), &w, me).is_none(), "an unnamed ship never matches the text");
    assert!(task_towee(&task("Protect the RLS Bachanalia", "Protect the RLS Bachanalia."), &w, me).is_none(), "only Tow tasks");
    let w = [unit(1, "RLS Sea Sparrow"), unit(5, "RLS Yorkshire")];
    let sloop = task("MO4 free stuck sloop", "Tow the RLS Yorkshire Clear of the Island it has Run Aground On.");
    assert_eq!(task_towee(&sloop, &w, me).map(|u| u.e), Some(w[1].e), "a task whose text says Tow");
    let w = [unit(1, "RLS Sea Sparrow"), unit(6, "RLS Indomitable")];
    let mow = task("rescue bh man o war", "Tow the RLS Indominable Clear of the Vanov Black Hole.");
    assert_eq!(task_towee(&mow, &w, me).map(|u| u.e), Some(w[1].e), "the misspelt name still matches");
}

#[test]
fn autopilot_flees_on_cover_and_escape_objectives_but_fights_the_rest() {
    assert!(flee_task("Take Cover in the Asteroid Field."));
    assert!(flee_task("Avoid the Pirate Trap. Return to Crescentia Right Away."));
    assert!(!flee_task("Hold off Pirate Attack until Repairs are Complete."));
    assert!(!flee_task("Destroy Both Camp Cannon Towers."));
    assert!(defend_task("Defend Her Majesty Aboard Parliament."));
    assert!(defend_task("Keep the Pirates From Firing on Dorlinor."));
    assert!(!defend_task("Search Nearby Islands for Ships."));
}
