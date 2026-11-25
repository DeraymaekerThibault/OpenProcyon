use super::*;

fn small() -> Pull {
    Pull::for_terrain("Terrain_BlackHole_Small").unwrap()
}
// Hull length (collision box) and Mass of the measured classes, and the hull rate MaxThrust / (MaxSpeed x Mass) of the Cutter.
const CUTTER: (f32, f32) = (13.5, 4000.0);
const FRIGATE: (f32, f32) = (18.0, 20000.0);
const MAN_O_WAR: (f32, f32) = (31.0, 40000.0);
const CUTTER_RATE: f32 = 1.862;

/// Runs a ship standing still at `distance` from a Small hole for `secs`: (inward distance covered, whether it broke free).
fn rest(hull: (f32, f32), distance: f32, secs: f32) -> (f32, Caught) {
    let pull = small();
    let mut c = Caught::default();
    let mut pos = Vec3::new(distance, 0.0, 0.0);
    for _ in 0..(secs * 30.0).round() as usize {
        let d = pos.length();
        let a = pull_dir(Vec3::ZERO, pos) * pull.accel(d, hull.0, hull.1);
        pos += c.step(a, d <= pull.core(), CUTTER_RATE, TICK).0;
    }
    (distance - pos.length(), c)
}

#[test]
fn pull_law_matches_the_retail_small_hole() {
    let pull = small();
    assert_eq!(pull, Pull { magnitude: 9000.0, radius: 145.0 });
    assert_eq!(Pull::for_terrain("Terrain_Island"), None);
    // measured full-depth pull per class: Cutter 4.50, Frigate 1.21-1.26, Man-O-War 0.99-1.04
    for (hull, measured) in [(CUTTER, 4.50), (FRIGATE, 1.235), (MAN_O_WAR, 1.015)] {
        let a = pull.strength(hull.0, hull.1);
        assert!((a / measured - 1.0).abs() < 0.05, "{hull:?}: {a} vs {measured}");
    }
    // nothing outside the radius (retail: Man-O-War at 200-203 for 6 min, Cutter at 372) nor inside the core (36.5 measured)
    assert_eq!(pull.accel(200.0, MAN_O_WAR.0, MAN_O_WAR.1), 0.0);
    assert_eq!(pull.accel(372.0, CUTTER.0, CUTTER.1), 0.0);
    assert_eq!(pull.accel(36.0, CUTTER.0, CUTTER.1), 0.0);
    assert!((pull.core() - 36.5).abs() <= 0.5);
    // linear ramp over the first ~36 units, then constant (Man-O-War: the same creep at 105 and 48 units)
    let full = pull.strength(CUTTER.0, CUTTER.1);
    assert!((pull.accel(145.0 - 18.125, CUTTER.0, CUTTER.1) - full / 2.0).abs() < 1e-4);
    assert_eq!(pull.accel(105.0, MAN_O_WAR.0, MAN_O_WAR.1), pull.accel(48.0, MAN_O_WAR.0, MAN_O_WAR.1));
    // Magnitude scales the pull (PROVISIONAL)
    let big = Pull::for_terrain("Terrain_BlackHole_Big").unwrap();
    assert!((big.strength(CUTTER.0, CUTTER.1) / full - 25.0 / 9.0).abs() < 1e-4);
}

#[test]
fn a_resting_ship_creeps_one_tick_of_pull_per_second() {
    // retail: Cutter 0.0908 units/s at depth 22.9 inside the edge, Frigate 0.0398 and Man-O-War 0.0328 at full depth
    for (hull, distance, measured) in [(CUTTER, 145.0 - 22.9, 0.0908), (FRIGATE, 54.0, 0.0398), (MAN_O_WAR, 105.0, 0.0328)] {
        let (moved, c) = rest(hull, distance, 1.0);
        assert!(!c.free, "{hull:?} stays snapped");
        assert!((moved / measured - 1.0).abs() < 0.06, "{hull:?}: {moved} vs {measured} units/s");
    }
    // a Man-O-War resting 200 s never broke free and stays alive
    let (_, c) = rest(MAN_O_WAR, 105.0, 200.0);
    assert!(!c.free && c.inside == 0.0);
}

#[test]
fn a_resting_ship_moves_one_tick_of_pull_per_tick_of_0_033_s() {
    // the snap leaves a x 0.033 per tick as a velocity, moved for 0.033 s: a x 0.033 x 0.033 units per tick
    let mut c = Caught::default();
    let (step, _) = c.step(Vec3::new(1.0, 0.0, 0.0), false, CUTTER_RATE, crate::TICK_SECS);
    assert!((step.x - 0.033 * 0.033).abs() < 1e-9, "{}", step.x);
}

#[test]
fn a_ship_under_way_is_carried_off_like_a_resting_one() {
    // retail: a Cutter at Ahead Full was pushed toward the centre at 2.1-2.4 units/s once inside ~117 units (terminal A / k
    // = 2.42), and crept at one tick of pull (0.05 units/s at 132 units) before that; it was destroyed at R/4.
    let pull = small();
    let at = |d: f32| pull_dir(Vec3::ZERO, Vec3::new(d, 0.0, 0.0)) * pull.accel(d, CUTTER.0, CUTTER.1);
    let mut c = Caught::default();
    let (creep, _) = c.step(at(132.0), false, CUTTER_RATE, TICK);
    assert!(!c.free && (creep.length() / TICK - 0.05).abs() < 0.01, "creep {} units/s", creep.length() / TICK);
    for _ in 0..90 {
        c.step(at(90.0), false, CUTTER_RATE, TICK);
    }
    assert!(c.free && (c.velocity.length() - 2.42).abs() < 0.05, "drift {}", c.velocity.length());
}

#[test]
fn a_deep_cutter_breaks_free_and_falls_at_the_hull_rate() {
    // PROVISIONAL trigger: 2.9 units/s². Below it the Cutter stays snapped; past it, it falls to A / k = 2.42 units/s.
    assert!(!rest(CUTTER, 123.0, 5.0).1.free);
    let pull = small();
    let a = pull_dir(Vec3::ZERO, Vec3::new(90.0, 0.0, 0.0)) * pull.accel(90.0, CUTTER.0, CUTTER.1);
    let mut c = Caught::default();
    for _ in 0..300 {
        c.step(a, false, CUTTER_RATE, TICK);
    }
    assert!(c.free);
    assert!((c.velocity.length() - 2.42).abs() < 0.03, "terminal speed {}", c.velocity.length());
    // inside the core the pull stops and the speed decays at the hull rate
    let v0 = c.velocity.length();
    c.step(Vec3::ZERO, true, CUTTER_RATE, 0.5);
    assert!((c.velocity.length() - v0 * (-CUTTER_RATE * 0.5).exp()).abs() < 1e-4);
}

#[test]
fn the_core_destroys_a_ship_after_1_1_s() {
    let mut c = Caught::default();
    let mut killed_at = None;
    for i in 1..=60 {
        if c.step(Vec3::ZERO, true, CUTTER_RATE, TICK).1 {
            killed_at = Some(i as f32 * TICK);
            break;
        }
    }
    assert!(killed_at.is_some_and(|t| (t - KILL_DELAY).abs() < 0.05), "{killed_at:?}");
    // leaving the core resets the clock
    let mut c = Caught { inside: 1.0, ..default() };
    c.step(Vec3::ZERO, false, CUTTER_RATE, TICK);
    assert_eq!(c.inside, 0.0);
}

#[test]
fn pull_spirals_clockwise_toward_the_centre_and_is_horizontal() {
    // ship 100 units east of the hole (game +x = bevy +x); the centre is west
    let v = pull_dir(Vec3::ZERO, Vec3::new(100.0, 40.0, 0.0));
    assert!(v.x < 0.0 && v.y == 0.0 && (v.length() - 1.0).abs() < 1e-6, "toward the centre and horizontal, got {v}");
    // clockwise from above while heading west turns north (game +y = bevy -z): 15° off, sideways / inward 0.27
    assert!(v.z < 0.0 && (v.z / v.x - DRIFT_SWIRL).abs() < 1e-3, "sideways part, got {v}");
    assert_eq!(pull_dir(Vec3::ZERO, Vec3::ZERO), Vec3::ZERO, "no direction at the centre");
}

#[test]
#[ignore = "needs imported assets"]
fn class_data_gives_the_measured_pulls() {
    let lib = crate::testutil::lib();
    for (class, measured) in [("Ship_Navy_Cutter", 4.50), ("Ship_Navy_Frigate", 1.235), ("Ship_Navy_ManOWar", 1.015)] {
        let c = &lib.classes[lib.index(class)];
        let a = small().strength(c.length, c.phys.mass);
        assert!((a / measured - 1.0).abs() < 0.05, "{class}: {a} vs {measured}");
    }
    // Mission 11's pinned SS Harvester (Mass 2000, 11.6 units inside a Small hole's rim) builds no pull speed in retail: one tick
    // of its pull stays under the 0.1 snap there (a retail save).
    let h = &lib.classes[lib.index("Ship_Civilian_Galleon_Feather")];
    assert!(h.phys.mass < MIN_PULL_MASS && small().accel(145.0 - 11.6, h.length, h.phys.mass) < BREAK_FREE);
    let c = &lib.classes[lib.index("Ship_Navy_Cutter")];
    assert!((c.phys.thrust / (c.phys.max_speed * c.phys.mass) - CUTTER_RATE).abs() < 0.01);
}

#[test]
#[ignore = "needs imported assets"]
fn the_centre_destroys_a_ship_and_credits_nobody() {
    use crate::combat::{Hit, Library};
    use crate::{Side, collision::Drift};
    let mut app = App::new();
    app.add_message::<Hit>()
        .init_resource::<Time>()
        .insert_resource(crate::testutil::lib())
        .init_resource::<crate::stats::GameStats>()
        .add_systems(Update, (pull, crate::stats::collect).chain());
    app.world_mut().spawn((Transform::default(), small()));
    let ship = |app: &mut App, class: &str, side: Side, at: Vec3| {
        let lib = app.world().resource::<Library>();
        let ci = lib.index(class);
        let c = &lib.classes[ci];
        let health = Health::new(c);
        let ship = Ship { heading: 0.0, ..Ship::new(ci, side) };
        app.world_mut().spawn((Transform::from_translation(at), ship, health, Drift::default())).id()
    };
    let gunner = ship(&mut app, "Ship_Pirate_Carrack", Side::Enemy, Vec3::new(300.0, 0.0, 0.0));
    let victim = ship(&mut app, "Ship_Navy_ManOWar", Side::Player, Vec3::new(100.0, 0.0, 0.0));
    // A ship the rules pinned (mission 4's stranded St. Christopher) waits in the core to be towed out.
    let stranded = ship(&mut app, "Ship_Navy_Tender", Side::Enemy, Vec3::new(0.0, 0.0, 20.0));
    app.world_mut().entity_mut(stranded).insert(crate::collision::Pinned);
    app.update(); // stats registers both hulls; the victim is caught
    app.world_mut().write_message(Hit { shooter: gunner, victim });
    app.world_mut().get_mut::<Health>(victim).unwrap().sections[0] -= 50.0; // the Carrack's last hit
    app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.05));
    app.update();
    let before = app.world().get::<Transform>(victim).unwrap().translation;
    assert!(before.x < 100.0 && app.world().get::<Health>(victim).unwrap().core > 0.0, "creeping in, still alive: {before}");
    app.world_mut().get_mut::<Transform>(victim).unwrap().translation = Vec3::new(30.0, 0.0, 0.0);
    for _ in 0..20 {
        app.update(); // 1.0 s in the core
    }
    assert!(app.world().get::<Health>(victim).unwrap().core > 0.0, "the core takes ~1.1 s");
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(app.world().get::<Health>(victim).unwrap().core, 0.0, "after 1.1 s in the core the hull goes");
    assert!(app.world().get::<Swallowed>(victim).is_some());
    assert!(app.world().get::<Health>(stranded).unwrap().core > 0.0, "a pinned ship is not swallowed");
    assert_eq!(app.world().get::<Transform>(stranded).unwrap().translation, Vec3::new(0.0, 0.0, 20.0), "nor pulled");
    let far = app.world().get::<Transform>(gunner).unwrap().translation;
    assert_eq!(far, Vec3::new(300.0, 0.0, 0.0), "outside the radius nothing moves");
    // combat::deaths would set `destroyed`; the summary then shows the loss but no kill and no extra damage
    app.world_mut().get_mut::<Health>(victim).unwrap().destroyed = true;
    app.update();
    let stats = app.world().resource::<crate::stats::GameStats>();
    let (enemy, player) = (&stats.players["Enemy fleet"], &stats.players["Your fleet"]);
    assert_eq!((player.end, enemy.sunk), (0, 0));
    assert_eq!((enemy.inflicted, player.taken), (50.0, 50.0), "only the gunfire counts");
}

fn hole() -> BlackHole {
    BlackHole { p: load("Small").expect("effect JSON"), pts: vec![], acc: 0.0, rng: 7, mesh: None }
}

#[test]
#[ignore = "needs imported assets"]
fn small_hole_params_match_the_effect_file() {
    let p = hole().p;
    assert_eq!((p.start, p.kill, p.rdelta, p.out, p.width), (150.0, 10.0, 0.45, 0.2, 50.0));
    assert!((p.z_add - 1e-5).abs() < 1e-9);
}

#[test]
#[ignore = "needs imported assets"]
fn points_spiral_in_die_and_trails_fade() {
    let mut h = hole();
    for _ in 0..3000 {
        h.tick();
        assert!(h.pts.iter().all(|p| p.rad >= h.p.kill - h.p.rdelta && p.segs.iter().all(|s| (0.0..=1.0).contains(&s.alpha))));
    }
    assert!(!h.pts.is_empty() && h.pts.len() < 40, "steady stream of points, got {}", h.pts.len());
    // every point is older than one segment interval only if it has laid segments
    assert!(h.pts.iter().any(|p| p.segs.len() >= 3));
}
