use super::*;
use crate::EngineOrder;
fn rect(x: f32, z: f32, yaw: f32) -> BoxBody {
    box_body([Vec3::new(-1.0, -1.0, -3.0), Vec3::new(1.0, 1.0, 3.0)], &Transform::from_xyz(x, 0.0, z).with_rotation(Quat::from_rotation_y(yaw)))
}

/// An app that runs `sail` over a fixed time step with the given library.
fn sail_app(lib: crate::Library, step: std::time::Duration) -> App {
    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(step);
    app.insert_resource(time).insert_resource(lib).add_systems(Update, sail);
    app
}

#[test]
#[ignore = "needs imported assets"]
fn tumbling_asteroid_collision_follows_transform() {
    let mut app = App::new();
    app.init_resource::<Shapes>().init_resource::<crate::data::AsteroidDefs>().add_systems(Update, update_asteroids);
    let old = Transform::default();
    let rock = crate::combat::Asteroid::new("Asteroid_Large", "Asteroid Belt", app.world().resource::<crate::data::AsteroidDefs>()).unwrap();
    let obstacle = app.world().resource::<Shapes>().obstacle("Asteroid_Large", &old).unwrap();
    let entity = app.world_mut().spawn((rock, old, obstacle)).id();
    let moved = Transform::from_xyz(500.0, 30.0, 100.0).with_rotation(Quat::from_rotation_x(0.7) * Quat::from_rotation_z(0.6));
    app.world_mut().entity_mut(entity).insert(moved);
    app.update();
    let obstacle = app.world().get::<Obstacle>(entity).unwrap();
    assert_eq!(obstacle.triangles.len(), 12);
    assert!(obstacle.body.bounds.min.x > 480.0);
    assert!(obstacle.body.bounds.min.y > 15.0);
    for triangle in &obstacle.triangles {
        for p in triangle.points {
            assert!(p.cmpge(obstacle.body.bounds.min - Vec3::splat(0.001)).all());
            assert!(p.cmple(obstacle.body.bounds.max + Vec3::splat(0.001)).all());
        }
    }
    assert!(static_contacts(&rect(0.0, 0.0, 0.0), obstacle).is_empty(), "no stale collider at old position");
}

#[test]
#[ignore = "needs imported assets"]
fn contact_translation_keeps_tumbling_rock_faces_and_bounds_aligned() {
    let shapes = Shapes::from_world(&mut World::new());
    let tf = Transform::default().with_rotation(Quat::from_rotation_x(0.7));
    let mut rock = shapes.obstacle("Asteroid_Large", &tf).unwrap();
    let delta = Vec3::new(7.0, 0.0, -3.0);
    rock.translate(delta);
    let expected = shapes.obstacle("Asteroid_Large", &Transform { translation: delta, ..tf }).unwrap();
    assert!((rock.body.bounds.min - expected.body.bounds.min).length() < 1e-5);
    assert!((rock.body.bounds.max - expected.body.bounds.max).length() < 1e-5);
    for (a, b) in rock.triangles.iter().zip(&expected.triangles) {
        assert!((a.bounds.min - b.bounds.min).length() < 1e-5);
        assert!((a.bounds.max - b.bounds.max).length() < 1e-5);
        for (a, b) in a.points.into_iter().zip(b.points) {
            assert!((a - b).length() < 1e-5);
        }
    }
}

#[test]
fn boxes_separate_and_rotated_contact_resolves() {
    assert!(contact(&rect(0.0, 0.0, 0.0).points, &rect(3.0, 0.0, 0.0).points).is_none());
    let a = rect(0.0, 0.0, 0.4);
    let b = rect(1.0, 0.0, -0.3);
    let push = contact(&a.points, &b.points).unwrap();
    let moved: Vec<_> = a.points.iter().map(|p| *p + push).collect();
    assert!(contact(&moved, &b.points).is_none());
}
#[test]
fn wall_blocks_but_geometry_above_ship_does_not() {
    let body = rect(0.0, 0.0, 0.0);
    let wall = [Vec3::new(0.5, -2.0, -5.0), Vec3::new(0.5, 2.0, -5.0), Vec3::new(0.5, 0.0, 5.0)];
    assert!(contact(&body.points, &slab(&wall, -1.0, 1.0)).is_some());
    assert!(slab(&wall.map(|p| p + Vec3::Y * 10.0), -1.0, 1.0).is_empty());
}
#[test]
#[ignore = "needs imported assets"]
fn nis_stops_helm_and_combat_motion_but_keeps_scripted_routes_and_resumes() {
    let lib = crate::Library::load(crate::assets_dir());
    let class = lib.index("Ship_Navy_Cutter");
    let [primary_health, combat_health, route_health, ally_health] = [0; 4].map(|_| crate::Health::new(&lib.classes[class]));
    let ship =
        crate::Ship { heading: 0.0, speed: 20.0, ang_vel: 0.2, order: EngineOrder::Full, waypoints: vec![Vec3::NEG_Z * 500.0], ..crate::Ship::new(class, crate::Side::Player) };
    let mut mission = crate::campaign::load_id("mission03");
    mission.script.nis = true;
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs_f32(0.1));
    let mut app = App::new();
    app.insert_resource(time).insert_resource(lib).insert_resource(mission).add_systems(Update, sail);
    let primary = app.world_mut().spawn((ship.clone(), primary_health, Transform::default(), Drift(Vec3::X * 10.0), crate::PlayerShip)).id();
    let combat_pos = Vec3::X * 100.0;
    let combat = app.world_mut().spawn((crate::Ship { waypoints: vec![], goal: Some(1.0), ..ship }, combat_health, Transform::from_translation(combat_pos), Drift::default())).id();
    let route_pos = Vec3::X * 200.0;
    let route = app
        .world_mut()
        .spawn((ship.clone(), route_health, Transform::from_translation(route_pos), Drift::default(), crate::campaign::MissionShip { stop_at_end: true, ..default() }))
        .id();
    let ally_pos = Vec3::X * 300.0;
    let ally = app.world_mut().spawn((ship, ally_health, Transform::from_translation(ally_pos), Drift::default(), crate::campaign::MissionShip { human: true, ..default() })).id();
    app.world_mut().spawn(crate::environment::Current::new(vec![Vec3::ZERO, Vec3::NEG_Z * 500.0]));
    for _ in 0..10 {
        app.update();
    }
    for (e, pos) in [(primary, Vec3::ZERO), (combat, combat_pos), (ally, ally_pos)] {
        assert_eq!(app.world().get::<Transform>(e).unwrap().translation, pos);
        let ship = app.world().get::<crate::Ship>(e).unwrap();
        assert_eq!((ship.speed, ship.ang_vel, ship.order), (0.0, 0.0, EngineOrder::Full));
        assert_eq!(app.world().get::<Drift>(e).unwrap().0, Vec3::ZERO);
    }
    assert_ne!(app.world().get::<Transform>(route).unwrap().translation, route_pos, "scripted retreat continues in NIS");
    app.world_mut().resource_mut::<crate::campaign::Mission>().script.nis = false;
    app.update();
    assert_ne!(app.world().get::<Transform>(primary).unwrap().translation, Vec3::ZERO, "helm order resumes after NIS");
    assert!(!app.world().get::<crate::Ship>(primary).unwrap().waypoints.is_empty());
}

#[test]
#[ignore = "needs imported assets"]
fn ecs_ship_contact_and_static_wall_do_not_pass_through() {
    let mut lib = crate::Library::load(crate::assets_dir());
    let class = lib.index("Ship_Navy_Cutter");
    assert_eq!(lib.classes[class].collision_bounds, [Vec3::new(-2.25, -1.5, -6.5), Vec3::new(2.25, 2.0, 7.0)]);
    lib.classes[class].collision_bounds = [Vec3::new(-1.0, -1.0, -3.0), Vec3::new(1.0, 1.0, 3.0)];
    let mut app = sail_app(lib, std::time::Duration::from_millis(33));
    let ship = crate::Ship { heading: 0.0, speed: 20.0, order: EngineOrder::Full, ..crate::Ship::new(class, crate::Side::Player) };
    let health = || crate::Health::new(&app.world().resource::<crate::Library>().classes[class]);
    let h1 = health();
    let h2 = health();
    let a = app.world_mut().spawn((ship.clone(), h1, Transform::from_xyz(0.0, 0.0, 4.0), Drift::default())).id();
    let b = app.world_mut().spawn((crate::Ship { heading: std::f32::consts::PI, ..ship }, h2, Transform::from_xyz(0.0, 0.0, -4.0), Drift::default())).id();
    for _ in 0..60 {
        app.update();
    }
    assert!(app.world().get::<Transform>(a).unwrap().translation.z > app.world().get::<Transform>(b).unwrap().translation.z, "ships cannot swap sides");
    app.world_mut().despawn(b);
    let wall = [Vec3::new(-20.0, -2.0, -5.0), Vec3::new(20.0, -2.0, -5.0), Vec3::new(0.0, 2.0, -5.0)];
    app.world_mut().spawn(Obstacle {
        body: BoxBody { points: wall.to_vec(), bounds: Bounds::points(&wall) },
        triangles: vec![Triangle { points: wall, bounds: Bounds::points(&wall) }],
        tree: vec![],
    });
    for _ in 0..300 {
        app.update();
    }
    assert!(app.world().get::<Transform>(a).unwrap().translation.z >= -2.001, "bow must remain on this side of the wall");
}
/// Closest approach to a marker behind a wall for a cutter on a move order; `player` makes it the human's own ship.
fn wall_run(player: bool) -> f32 {
    let mut lib = crate::Library::load(crate::assets_dir());
    let class = lib.index("Ship_Navy_Cutter");
    lib.classes[class].collision_bounds = [Vec3::new(-1.0, -1.0, -3.0), Vec3::new(1.0, 1.0, 3.0)];
    let health = crate::Health::new(&lib.classes[class]);
    let mut app = sail_app(lib, std::time::Duration::from_secs_f32(0.1));
    // A 40-wide wall between the ship and its only path marker straight ahead.
    let wall = [Vec3::new(-20.0, -2.0, -30.0), Vec3::new(20.0, -2.0, -30.0), Vec3::new(0.0, 2.0, -30.0)];
    app.world_mut().spawn(Obstacle {
        body: BoxBody { points: wall.to_vec(), bounds: Bounds::points(&wall) },
        triangles: vec![Triangle { points: wall, bounds: Bounds::points(&wall) }],
        tree: vec![],
    });
    let goal = Vec3::new(0.0, 0.0, -120.0);
    let ship = crate::Ship {
        heading: 0.0,
        speed: 8.0,
        order: EngineOrder::Full,
        waypoints: vec![goal],
        ..crate::Ship::new(class, if player { crate::Side::Player } else { crate::Side::Enemy })
    };
    let e = app.world_mut().spawn((ship, health, Transform::default(), Drift::default())).id();
    if player {
        app.world_mut().entity_mut(e).insert(crate::PlayerShip);
    }
    let mut best = f32::INFINITY;
    for _ in 0..1500 {
        app.update();
        best = best.min(app.world().get::<Transform>(e).unwrap().translation.distance(goal));
    }
    best
}

#[test]
#[ignore = "needs imported assets"]
fn ai_ship_on_a_path_detours_round_a_wall_instead_of_grinding_into_it() {
    let best = wall_run(false);
    assert!(best < 10.0, "the ship reaches the marker behind the wall (closest {best})");
}

#[test]
#[ignore = "needs imported assets"]
fn the_humans_move_order_runs_straight_into_a_wall() {
    let best = wall_run(true);
    assert!(best > 80.0, "the human's ship stops at the wall, no detour (closest {best})");
}
#[test]
#[ignore = "needs imported assets"]
fn ai_ship_slows_for_an_island_in_its_way_creeps_round_it_and_picks_up_speed_again() {
    // Retail Frigate on Border Dispute: 6.6 u/s straight at the map-centre island, down to 1.6 u/s at 5 units, never stopped, back to cruise.
    // A wall stands in for the island; the captain keeps asking for the straight line, as `ship::ai` does every tick.
    let lib = crate::Library::load(crate::assets_dir());
    let class = lib.index("Ship_Procyon_Frigate");
    let health = crate::Health::new(&lib.classes[class]);
    let mut app = sail_app(lib, std::time::Duration::from_millis(33));
    let wall = [Vec3::new(-45.0, -2.0, -200.0), Vec3::new(45.0, -2.0, -200.0), Vec3::new(0.0, 2.0, -200.0)];
    app.world_mut().spawn(Obstacle {
        body: BoxBody { points: wall.to_vec(), bounds: Bounds::points(&wall) },
        triangles: vec![Triangle { points: wall, bounds: Bounds::points(&wall) }],
        tree: vec![],
    });
    let ship = crate::Ship { heading: 0.0, speed: 6.6, order: EngineOrder::Emergency, goal: Some(0.0), ..crate::Ship::new(class, crate::Side::Enemy) };
    let e = app.world_mut().spawn((ship, health, Transform::default(), Drift::default())).id();
    let (mut cruise, mut slowest, mut passed_at) = (0.0f32, f32::INFINITY, None);
    for frame in 0..3636 {
        app.world_mut().get_mut::<crate::Ship>(e).unwrap().goal = Some(0.0); // the captain keeps asking for the straight line
        app.update();
        let (speed, z) = (app.world().get::<crate::Ship>(e).unwrap().speed, app.world().get::<Transform>(e).unwrap().translation.z);
        if z > -100.0 {
            cruise = cruise.max(speed);
        }
        if z < -150.0 && z > -250.0 {
            slowest = slowest.min(speed);
        }
        if z < -260.0 && passed_at.is_none() {
            passed_at = Some(frame);
        }
    }
    assert!(passed_at.is_some(), "the ship gets round the island");
    assert!(slowest < 0.6 * cruise, "slows near the island: {slowest} against cruise {cruise}");
    // Retail hugged the coast 3-9 units from its centre (the hull scrapes) at 1.3-1.6 u/s; contact takes a little more speed off here.
    assert!(slowest > 0.2, "creeps, does not stop: {slowest}");
    assert!(app.world().get::<crate::Ship>(e).unwrap().speed > 0.8 * cruise, "back to cruise once clear");
}
#[test]
#[ignore = "needs imported assets"]
fn base_hover_hits_the_island_hull_but_not_the_water_near_its_dock_points() {
    let shapes = Shapes::from_world(&mut World::new());
    let base = shapes.obstacle("Base_Safetytown", &Transform::default()).unwrap();
    let down = |x: f32, z: f32| Ray3d::new(Vec3::new(x, 500.0, z), Dir3::NEG_Y);
    let c = (base.body.bounds.min + base.body.bounds.max) / 2.0;
    assert!(base.ray_hit(down(c.x, c.z)).is_some());
    // The dock points are ~208 out; a waypoint click at 160 units used to show the base cursor.
    assert!(base.ray_hit(down(c.x + 160.0, c.z)).is_none());
}

#[test]
#[ignore = "needs imported assets"]
fn retail_island_mesh_blocks_a_ship_driven_through_its_centre() {
    let lib = crate::Library::load(crate::assets_dir());
    let class = lib.index("Ship_Navy_Cutter");
    let health = crate::Health::new(&lib.classes[class]);
    let shapes = Shapes::from_world(&mut World::new());
    let island = shapes.obstacle("Island01", &Transform::default()).unwrap();
    assert!(!island.triangles.is_empty());
    let mut app = sail_app(lib, std::time::Duration::from_secs_f32(0.1));
    app.world_mut().spawn(island);
    let ship = crate::Ship { heading: 0.0, speed: 20.0, order: EngineOrder::Full, ..crate::Ship::new(class, crate::Side::Player) };
    let e = app.world_mut().spawn((ship, health, Transform::from_xyz(0.0, 0.0, 80.0), Drift::default())).id();
    for _ in 0..100 {
        app.update();
        assert!(app.world().get::<Transform>(e).unwrap().translation.length() > 10.0, "must not traverse the solid centre of Island01");
    }
}
#[test]
fn course_prefix_stops_before_a_wall() {
    let wall = [Vec3::new(-20.0, -2.0, -30.0), Vec3::new(20.0, -2.0, -30.0), Vec3::new(0.0, 2.0, -30.0)];
    let obstacle = Obstacle {
        body: BoxBody { points: wall.to_vec(), bounds: Bounds::points(&wall) },
        triangles: vec![Triangle { points: wall, bounds: Bounds::points(&wall) }],
        tree: vec![],
    };
    let scenery = [&obstacle];
    let bounds = [Vec3::new(-1.0, -1.0, -3.0), Vec3::new(1.0, 1.0, 3.0)];
    // Ship heading straight at -Z (bow toward the wall) every 2 units from z=0 to z=-20 (wall at z=-30).
    let path: Vec<Transform> = (0..=10).map(|i| Transform::from_xyz(0.0, 0.0, -2.0 * i as f32)).collect();
    let keep = clear_prefix(bounds, &path, &scenery);
    assert_eq!(keep, path.len(), "nothing blocks before z=-30");
    let path: Vec<Transform> = (0..=20).map(|i| Transform::from_xyz(0.0, 0.0, -2.0 * i as f32)).collect();
    let keep = clear_prefix(bounds, &path, &scenery);
    assert!(keep < path.len() && keep > 0, "path clipped at first contact");
    assert!(path[keep].translation.z < -26.0 && path[keep].translation.z > -31.5, "clip at box-to-wall contact, got {}", path[keep].translation.z);
    assert!(!touches(bounds, &path[keep - 1], &scenery));
}
fn ship_rock_contact(mass: f32) -> (Vec3, Vec3, Vec3, f32) {
    let mut lib = crate::Library::load(crate::assets_dir());
    let class = lib.index("Ship_Navy_Cutter");
    lib.classes[class].phys.thrust = 0.0; // Isolate collision from engine acceleration/braking.
    lib.classes[class].collision_bounds = [Vec3::splat(-1.0), Vec3::splat(1.0)];
    let health = crate::Health::new(&lib.classes[class]);
    let mut defs = crate::data::AsteroidDefs::from_world(&mut World::new());
    defs.0.get_mut("Asteroid_Small").unwrap().mass = mass;
    let rock = crate::combat::Asteroid::new("Asteroid_Small", "rocks", &defs).unwrap();
    let hp = rock.hp;
    let mut app = sail_app(lib, std::time::Duration::from_secs_f32(0.1));
    let ship = crate::Ship { heading: 0.0, speed: 10.0, ..crate::Ship::new(class, crate::Side::Player) };
    let a = app.world_mut().spawn((ship, health, Transform::from_xyz(0.0, 0.0, 1.5), Drift::default())).id();
    let body = box_body([Vec3::splat(-1.0), Vec3::splat(1.0)], &Transform::default());
    let b = app.world_mut().spawn((rock, Transform::default(), Drift::default(), Obstacle { body, triangles: vec![], tree: vec![] })).id();
    app.update();
    let ship = app.world().get::<crate::Ship>(a).unwrap();
    let va = forward(ship) * ship.speed + app.world().get::<Drift>(a).unwrap().0;
    let drift = app.world().get::<crate::combat::Asteroid>(b).unwrap().velocity;
    let position = app.world().get::<Transform>(b).unwrap().translation;
    let obstacle = app.world().get::<Obstacle>(b).unwrap();
    assert!((obstacle.body.bounds.min - (position - Vec3::ONE)).length() < 1e-5, "rock collider follows contact movement");
    (va, drift, position, hp - app.world().get::<crate::combat::Asteroid>(b).unwrap().hp)
}

#[test]
#[ignore = "needs imported assets"]
fn a_rock_contact_pushes_apart_by_mass_keeps_the_ships_speed_and_hurts_the_rock() {
    // The original: no momentum exchange; the rock is shoved away from the ship and both take its mass x the closing speed.
    for mass in [1000.0, 40000.0] {
        let (ship, drift, position, lost) = ship_rock_contact(mass);
        assert!((ship - Vec3::NEG_Z * 10.0).length() < 1e-4, "the ship keeps its velocity: {ship:?}");
        assert!(drift.z < 0.0 && drift.x.abs() < 1e-4, "the rock is shoved away from the ship: {drift:?}");
        assert!(position.z < 0.0, "rock is displaced rather than treated as an immovable wall");
        assert!(lost > 0.0, "the rock loses hit points");
    }
}

#[test]
#[ignore = "needs imported assets"]
fn ship_contact_pushes_the_hulls_apart_and_keeps_both_speeds() {
    for other in ["Ship_Navy_Torpedo", "Ship_Navy_Tug"] {
        let mut lib = crate::Library::load(crate::assets_dir());
        let a = lib.index("Ship_Navy_Cutter");
        let b = lib.index(other);
        for index in [a, b] {
            lib.classes[index].phys.thrust = 0.0;
            lib.classes[index].collision_bounds = [Vec3::splat(-1.0), Vec3::splat(1.0)];
        }
        let ha = crate::Health::new(&lib.classes[a]);
        let hb = crate::Health::new(&lib.classes[b]);
        let mut app = sail_app(lib, std::time::Duration::from_secs_f32(0.1));
        let ship = crate::Ship { heading: 0.0, speed: 10.0, ..crate::Ship::new(a, crate::Side::Player) };
        let ea = app.world_mut().spawn((ship.clone(), ha, Transform::from_xyz(0.0, 0.0, 1.5), Drift::default())).id();
        let eb = app.world_mut().spawn((crate::Ship { class: b, speed: 0.0, ..ship }, hb, Transform::default(), Drift::default())).id();
        app.update();
        let speed = |e| app.world().get::<crate::Ship>(e).unwrap().speed;
        assert!((speed(ea) - 10.0).abs() < 1e-4 && speed(eb).abs() < 1e-4, "no bounce: both keep their speed ({other})");
        let moved_b = app.world().get::<Transform>(eb).unwrap().translation.length();
        assert!(moved_b > 0.0, "the struck ship is pushed ({other})");
    }
}

/// The original's tow keeper: both hulls run their own step, then a taut rope moves the pair back together, 25 % on the tower and
/// 75 % on the towee, without touching speeds. The towee keeps braking itself, so the pair crawls at about three quarters of the
/// tower's speed and the towee trails the tower; once the tow is broken nothing teleports.
#[test]
#[ignore = "needs imported assets"]
fn a_tow_moves_the_pair_back_together_by_a_quarter_and_three_quarters() {
    let lib = crate::Library::load(crate::assets_dir());
    let class = lib.index("Ship_Navy_Cutter");
    let health = || crate::Health::new(&lib.classes[class]);
    let hull = crate::Ship { heading: 0.0, order: EngineOrder::Full, ..crate::Ship::new(class, crate::Side::Player) };
    let mut app = App::new();
    let a = app.world_mut().spawn((hull.clone(), health(), Transform::default(), Drift::default())).id();
    let b = app.world_mut().spawn((crate::Ship { order: EngineOrder::Stop, ..hull }, health(), Transform::from_xyz(0.0, 0.0, 40.0), Drift::default())).id();
    let mut mission = crate::campaign::load();
    mission.script.set_tow(a, b, true);
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs_f32(0.033));
    app.insert_resource(time).insert_resource(lib).insert_resource(mission).add_systems(Update, sail);
    for _ in 0..600 {
        app.update();
    }
    fn at(app: &App, e: Entity) -> Vec3 {
        app.world().get::<Transform>(e).unwrap().translation
    }
    let (tower, cargo) = (app.world().get::<crate::Ship>(a).unwrap(), app.world().get::<crate::Ship>(b).unwrap());
    // Two equal hulls: the tower's thrust works on both masses, so its speed settles at half; the towee brakes itself to 0.
    let full = crate::ship::full_speed(&app.world().resource::<crate::Library>().classes[class]);
    assert!((tower.speed - 0.5 * full).abs() < 0.05 * full && cargo.speed.abs() < 1e-3, "tower {} cargo {}", tower.speed, cargo.speed);
    assert!((at(&app, a) - at(&app, b)).with_y(0.0).length() < 40.0 + 1.0, "the rope holds: {}", (at(&app, a) - at(&app, b)).length());
    assert!(at(&app, b).z < 40.0 - 20.0, "the towee follows: {}", at(&app, b).z);
    let before = at(&app, b);
    app.world_mut().resource_mut::<crate::campaign::Mission>().script.break_tows(a);
    app.world_mut().get_mut::<Transform>(a).unwrap().translation.x += 500.0;
    app.update();
    assert!(at(&app, b).distance(before) < 1.0);
}

/// docs/re/tick.md: contact rates are written per 0.033 s tick, not per 1/30 s: one tick of contact does exactly rate x mass x speed.
#[test]
fn contact_damage_rate_is_per_0_033_s() {
    assert_eq!(TICK, 0.033);
    let (mass, closing) = (500.0, 12.0);
    assert!((rock_hurt(mass, closing, 0.033) - ROCK_FACTOR * mass * closing).abs() < 1e-5, "a 0.033 s tick takes the whole rate");
    assert!((rock_hurt(mass, closing, 0.0165) - ROCK_FACTOR * mass * closing / 2.0).abs() < 1e-5, "a half tick takes half");
    // Over one second the rate adds up to 1 / 0.033 ticks (30.30), not 30.
    assert!((rock_hurt(mass, closing, 1.0) / (ROCK_FACTOR * mass * closing) - 1.0 / 0.033).abs() < 1e-3);
}

/// docs/re/tick.md: everything integrates, then one collision pass; an overlapping pair responds at most once per tick, and a
/// ship touching a rock takes one tick's rate (rate x mass x closing speed per 0.033 s) from the rock, not several fractions.
#[test]
#[ignore = "needs imported assets"]
fn a_ship_and_a_rock_respond_once_per_tick_with_one_ticks_rate() {
    let mut lib = crate::Library::load(crate::assets_dir());
    let class = lib.index("Ship_Navy_Cutter");
    lib.classes[class].phys.thrust = 0.0;
    lib.classes[class].collision_bounds = [Vec3::splat(-1.0), Vec3::splat(1.0)];
    let health = crate::Health::new(&lib.classes[class]);
    let mut defs = crate::data::AsteroidDefs::from_world(&mut World::new());
    let mass = 40000.0;
    defs.0.get_mut("Asteroid_Small").unwrap().mass = mass;
    let mut rock = crate::combat::Asteroid::new("Asteroid_Small", "rocks", &defs).unwrap();
    // hit points well above one tick's damage: the rock must survive to show the whole rate
    rock.hp = 1.0e6;
    let hp = rock.hp;
    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(crate::BATTLE_TICK);
    app.insert_resource(time).insert_resource(lib).add_message::<Ram>().add_systems(Update, sail);
    let ship = crate::Ship { heading: 0.0, speed: 10.0, ..crate::Ship::new(class, crate::Side::Player) };
    let a = app.world_mut().spawn((ship, health, Transform::from_xyz(0.0, 0.0, 1.5), Drift::default())).id();
    let body = box_body([Vec3::splat(-1.0), Vec3::splat(1.0)], &Transform::default());
    let b = app.world_mut().spawn((rock, Transform::default(), Drift::default(), Obstacle { body, triangles: vec![], tree: vec![] })).id();
    app.update();
    let rams: Vec<_> = app.world_mut().resource_mut::<Messages<Ram>>().drain().collect();
    assert_eq!(rams.len(), 1, "one response for the pair in one tick");
    let lost = hp - app.world().get::<crate::combat::Asteroid>(b).unwrap().hp;
    let speed = app.world().get::<crate::Ship>(a).unwrap().speed;
    assert!((lost - ROCK_FACTOR * mass * speed).abs() < 0.02 * lost, "one tick takes rate x mass x closing speed: lost {lost}, speed {speed}");
}

/// A rock the contact damage takes to zero is destroyed by the ship, so the ship's owner gets the credit; one that survives is not.
#[test]
#[ignore = "needs imported assets"]
fn a_rock_broken_by_contact_is_credited_to_the_ship() {
    let mut credits = vec![];
    for hp in [1.0, f32::MAX] {
        let mut lib = crate::Library::load(crate::assets_dir());
        let class = lib.index("Ship_Navy_Cutter");
        lib.classes[class].phys.thrust = 0.0;
        lib.classes[class].collision_bounds = [Vec3::splat(-1.0), Vec3::splat(1.0)];
        let health = crate::Health::new(&lib.classes[class]);
        let mut defs = crate::data::AsteroidDefs::from_world(&mut World::new());
        defs.0.get_mut("Asteroid_Small").unwrap().mass = 40000.0;
        let mut rock = crate::combat::Asteroid::new("Asteroid_Small", "rocks", &defs).unwrap();
        rock.hp = hp;
        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(crate::BATTLE_TICK);
        app.insert_resource(time).insert_resource(lib).add_message::<Ram>().add_message::<crate::combat::Destroyed>().add_systems(Update, sail);
        let ship = crate::Ship { heading: 0.0, speed: 10.0, ..crate::Ship::new(class, crate::Side::Player) };
        let a = app.world_mut().spawn((ship, health, Transform::from_xyz(0.0, 0.0, 1.5), Drift::default())).id();
        let body = box_body([Vec3::splat(-1.0), Vec3::splat(1.0)], &Transform::default());
        let b = app.world_mut().spawn((rock, Transform::default(), Drift::default(), Obstacle { body, triangles: vec![], tree: vec![] })).id();
        app.update();
        let sent: Vec<_> = app.world_mut().resource_mut::<Messages<crate::combat::Destroyed>>().drain().map(|d| (d.by, d.victim)).collect();
        credits.push(sent.clone());
        if hp == 1.0 {
            assert_eq!(sent, [(a, b)], "the ship destroyed the rock");
        }
    }
    assert!(credits[1].is_empty(), "a rock that survives the touch is not destroyed");
}

fn shape_with_tree(offset: Vec3) -> Shapes {
    let node = tree::BoxNode { b: tree::Box3 { centre: Vec3::ZERO, axes: [Vec3::X, Vec3::Y, Vec3::Z], half: Vec3::splat(5.0) }, level: 0, kids: vec![] };
    let shape = Shape { layers: vec![Layer { offset, nodes: vec![node] }], min: [-5.0; 3], max: [5.0; 3], vertices: vec![], indices: vec![] };
    Shapes(HashMap::from([("Island_Test".to_string(), shape)]))
}
#[test]
fn the_layer_offset_places_the_tree_in_the_objects_frame() {
    let shapes = shape_with_tree(Vec3::new(3.0, 7.05, 0.0));
    let at = |tf: Transform| shapes.obstacle("Island_Test", &tf).unwrap().tree[0][0].b;
    near(at(Transform::from_xyz(100.0, 0.0, 0.0)).centre, Vec3::new(103.0, 7.05, 0.0));
    // the offset turns with the object (a quarter turn about up takes +x to -z)
    let turned = at(Transform::from_xyz(100.0, 0.0, 0.0).with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)));
    near(turned.centre, Vec3::new(100.0, 7.05, -3.0));
    near(turned.axes[0], Vec3::NEG_Z);
    // and a ship at water level meets the raised box
    let ship = tree::Box3::placed([Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0)], &Transform::from_xyz(103.0, 0.0, 0.0));
    let flat = shapes.obstacle("Island_Test", &Transform::from_xyz(100.0, 0.0, 0.0)).unwrap();
    // box spans y 2.05..12.05; the ship spans -1..1: they do not touch
    assert!(tree::land_contact(&flat.tree, &ship).is_none());
    let low = shape_with_tree(Vec3::new(3.0, 5.5, 0.0));
    let p = tree::land_push(&low.obstacle("Island_Test", &Transform::from_xyz(100.0, 0.0, 0.0)).unwrap().tree, &ship).unwrap();
    // overlapping by 0.5 in height only: all vertical, nothing sideways
    assert!(p.length() < 1e-4);
}
#[test]
fn a_moved_island_takes_its_tree_along() {
    let mut o = shape_with_tree(Vec3::ZERO).obstacle("Island_Test", &Transform::default()).unwrap();
    o.translate(Vec3::new(1.0, 2.0, 3.0));
    near(o.tree[0][0].b.centre, Vec3::new(1.0, 2.0, 3.0));
}
fn near(a: Vec3, b: Vec3) {
    assert!((a - b).length() < 1e-4, "{a} != {b}");
}

/// Game axes (x east, y north, z up) to the remake's: (x, z, -y). A game box becomes a remake box with min and max z swapped.
fn game_box(min: [f32; 3], max: [f32; 3]) -> [Vec3; 2] {
    [Vec3::new(min[0], min[2], -max[1]), Vec3::new(max[0], max[2], -min[1])]
}
/// The remake transform of a ship at game (x, y) heading along the game unit vector (hx, hy), at the water.
fn game_pose(x: f32, y: f32, hx: f32, hy: f32) -> Transform {
    Transform::from_xyz(x, 0.0, -y).with_rotation(Quat::from_rotation_y((-hx).atan2(hy)))
}
#[test]
fn the_game_to_remake_conversion_keeps_the_bow_where_the_heading_points() {
    let bounds = game_box([-3.0, -17.0, -3.0], [3.0, 14.0, 6.0]);
    let tf = game_pose(10.0, 20.0, 0.6, 0.8);
    // the bow end (game y = 14) is 14 along the heading from the position, the stern 17 behind it
    near(tf.transform_point(Vec3::new(0.0, 0.0, bounds[0].z)), Vec3::new(10.0 + 14.0 * 0.6, 0.0, -(20.0 + 14.0 * 0.8)));
    near(tf.transform_point(Vec3::new(0.0, 0.0, bounds[1].z)), Vec3::new(10.0 - 17.0 * 0.6, 0.0, -(20.0 - 17.0 * 0.8)));
    // game z (up) is remake y
    assert_eq!((bounds[0].y, bounds[1].y), (-3.0, 6.0));
    // a game-axes starboard (+x) side is the remake's too (right of the heading)
    near(tf.transform_point(Vec3::X * 3.0), Vec3::new(10.0 + 3.0 * 0.8, 0.0, -(20.0 - 3.0 * 0.6)));
}

#[test]
#[ignore = "needs imported assets (data/collision.json with box trees)"]
fn island_contact_reproduces_the_original_rest_positions() {
    let shapes = Shapes::from_world(&mut World::new());
    let mow = game_box([-3.0, -17.0, -3.0], [3.0, 14.0, 6.0]);
    let cutter = game_box([-2.25, -7.0, -1.5], [2.25, 6.5, 2.0]);
    let islands = HashMap::from([("8", ("Island_Rocky_08", -499.767_03, -1_311.340_1)), ("6", ("Island_Rocky_06", 338.899_14, -244.701_3))]);
    #[rustfmt::skip]
    let vectors: [(&str, bool, [f32; 2], [f32; 2], [f32; 2]); 12] = [
        ("8", true, [-469.5, -1342.6], [0.299, 0.954], [-465.42, -1368.62]),
        ("8", true, [-526.9, -1270.0], [0.121, -0.993], [-531.41, -1261.59]),
        ("8", true, [-450.4, -1310.9], [0.880, -0.476], [-456.81, -1316.30]),
        ("8", true, [-524.9, -1359.2], [0.982, 0.187], [-532.22, -1367.74]),
        ("8", true, [-467.6, -1262.5], [-0.159, 0.987], [-463.16, -1256.32]),
        ("8", true, [-577.4, -1319.7], [0.581, -0.814], [-585.52, -1321.05]),
        ("8", false, [-537.6, -1278.9], [0.160, -0.987], [-541.17, -1271.78]),
        ("8", false, [-459.7, -1279.8], [0.987, -0.162], [-455.06, -1273.34]),
        ("8", false, [-460.8, -1351.3], [-0.348, 0.937], [-457.55, -1358.45]),
        ("8", false, [-523.4, -1345.9], [-0.983, 0.183], [-529.68, -1357.37]),
        ("6", true, [318.3, -184.4], [-0.447, 0.894], [322.87, -178.26]),
        ("6", true, [321.6, -315.0], [-0.282, 0.959], [320.16, -326.19]),
    ];
    let mut worst: f32 = 0.0;
    for (island, big, start, heading, rest) in vectors {
        let (kind, ix, iy) = islands[island];
        let obstacle = shapes.obstacle(kind, &Transform::from_xyz(ix, 0.0, -iy)).unwrap();
        assert!(!obstacle.tree.is_empty(), "{kind} has no box tree: regenerate collision.json");
        let bounds = if big { mow } else { cutter };
        let mut tf = game_pose(start[0], start[1], heading[0], heading[1]);
        let mut ticks = 0;
        // Once per tick until no contact. The last pushes shrink geometrically (some starts settle in a damped two-point
        // oscillation) and, at these coordinates, below f32 resolution, so a push under 0.001 u counts as rest.
        while let Some(push) = tree::land_push(&obstacle.tree, &tree::Box3::placed(bounds, &tf)) {
            let before = tf.translation;
            tf.translation += push;
            ticks += 1;
            if push.length() < 0.001 || tf.translation == before {
                break;
            }
            assert!(ticks < 400, "no rest from {start:?}");
        }
        let got = Vec2::new(tf.translation.x, -tf.translation.z);
        let error = got.distance(Vec2::from(rest));
        println!(
            "island {island} {} start {start:?}: rest ({:.2}, {:.2}) expected {rest:?}, error {error:.3}, {ticks} pushes",
            if big { "Man-O-War" } else { "Cutter" },
            got.x,
            got.y
        );
        worst = worst.max(error);
    }
    assert!(worst <= 0.05, "worst error {worst}");
}

/// The same starts through the game's own scheduler (`sail`: several substeps per frame, one push each) with an idle ship.
#[test]
#[ignore = "needs imported assets (data/collision.json with box trees)"]
fn island_contact_through_the_game_scheduler() {
    let lib = crate::Library::load(crate::assets_dir());
    let shapes = Shapes::from_world(&mut World::new());
    // (ship, island, start, heading, expected rest), game axes
    let cases = [
        ("Ship_Navy_ManOWar", "Island_Rocky_08", -499.767_03, -1_311.340_1, [-469.5, -1342.6], [0.299, 0.954], [-465.42, -1368.62]),
        ("Ship_Navy_ManOWar", "Island_Rocky_08", -499.767_03, -1_311.340_1, [-467.6, -1262.5], [-0.159, 0.987], [-463.16, -1256.32]),
        ("Ship_Navy_Cutter", "Island_Rocky_08", -499.767_03, -1_311.340_1, [-537.6, -1278.9], [0.160, -0.987], [-541.17, -1271.78]),
        ("Ship_Navy_ManOWar", "Island_Rocky_06", 338.899_14, -244.701_3, [318.3, -184.4], [-0.447, 0.894], [322.87, -178.26]),
    ];
    for (name, kind, ix, iy, start, heading, rest) in cases {
        let class = lib.index(name);
        let health = crate::Health::new(&lib.classes[class]);
        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs_f32(1.0 / 30.0));
        app.insert_resource(time).insert_resource(crate::Library::load(crate::assets_dir())).add_systems(Update, sail);
        app.world_mut().spawn(shapes.obstacle(kind, &Transform::from_xyz(ix, 0.0, -iy)).unwrap());
        let tf = game_pose(start[0], start[1], heading[0], heading[1]);
        let ship = crate::Ship { heading: tf.rotation.to_euler(EulerRot::YXZ).0, order: EngineOrder::Full, throttle: Some(0.0), ..crate::Ship::new(class, crate::Side::Player) };
        let e = app.world_mut().spawn((ship, health, tf, Drift::default())).id();
        for _ in 0..200 {
            app.update();
        }
        let t = app.world().get::<Transform>(e).unwrap().translation;
        let sh = app.world().get::<crate::Ship>(e).unwrap();
        println!("  ship after: heading {:.3} (start {:.3}) speed {:.3} ang_vel {:.3}", sh.heading, tf.rotation.to_euler(EulerRot::YXZ).0, sh.speed, sh.ang_vel);
        let error = Vec2::new(t.x, -t.z).distance(Vec2::from(rest));
        println!("scheduler {name} on {kind} from {start:?}: ({:.2}, {:.2}) expected {rest:?}, error {error:.3}", t.x, -t.z);
        assert!(error <= 0.05, "{name} on {kind}: {error}");
    }
}

/// Two ship boxes whose shallowest axis is steep: B (half extents 0.5, turned 45 degrees about the bow axis) sits up and to the
/// starboard of A (half extents 1). The shallowest axis is the diagonal (0.707, 0.707, 0); depth 1.2 sqrt 2 - (sqrt 2 + 0.5) = 0.2171,
/// so A is pushed 0.2171 x cos 45 = 0.1535 along -x (the horizontal part of the normal), not the whole depth.
#[test]
fn a_ship_pair_is_pushed_apart_by_depth_times_cosine_along_the_horizontal_normal() {
    let a = tree::Box3 { centre: Vec3::ZERO, axes: [Vec3::X, Vec3::Y, Vec3::Z], half: Vec3::ONE };
    let h = std::f32::consts::FRAC_1_SQRT_2;
    let b = tree::Box3 { centre: Vec3::new(1.2, 1.2, 0.0), axes: [Vec3::new(h, h, 0.0), Vec3::new(-h, h, 0.0), Vec3::Z], half: Vec3::splat(0.5) };
    near(hull_push(&a, &b).unwrap(), Vec3::new(-0.1535, 0.0, 0.0));
    near(hull_push(&b, &a).unwrap(), Vec3::new(0.1535, 0.0, 0.0));
    // flat normals lose nothing: side by side along x, overlap 0.5
    let c = tree::Box3 { centre: Vec3::new(1.5, 0.0, 0.0), axes: [Vec3::X, Vec3::Y, Vec3::Z], half: Vec3::ONE };
    near(hull_push(&a, &c).unwrap(), Vec3::new(-0.5, 0.0, 0.0));
    // a separated pair has no push
    assert!(hull_push(&a, &tree::Box3 { centre: Vec3::new(2.5, 0.0, 0.0), ..c }).is_none());
}

/// A destroyed ship that is still breaking up keeps sailing but touches nothing: it passes through a live ship that is in its way,
/// and neither is pushed.
#[test]
#[ignore = "needs imported assets"]
fn dying_ship_sails_on_and_collides_with_nothing() {
    let mut lib = crate::Library::load(crate::assets_dir());
    let class = lib.index("Ship_Navy_Cutter");
    lib.classes[class].collision_bounds = [Vec3::new(-1.0, -1.0, -3.0), Vec3::new(1.0, 1.0, 3.0)];
    let mut app = sail_app(lib, std::time::Duration::from_millis(33));
    let ship = crate::Ship { heading: 0.0, speed: 5.0, order: EngineOrder::Emergency, throttle: Some(1.0), ..crate::Ship::new(class, crate::Side::Player) };
    let health = crate::Health::new(&app.world().resource::<crate::Library>().classes[class]);
    let mut dead = crate::Health::new(&app.world().resource::<crate::Library>().classes[class]);
    (dead.destroyed, dead.core) = (true, 0.0);
    let sinking = crate::combat::Sinking::new();
    let wreck = app.world_mut().spawn((ship.clone(), dead, Transform::from_xyz(0.0, 0.0, 3.0), Drift::default(), sinking)).id();
    let still = crate::Ship { speed: 0.0, order: EngineOrder::Stop, ..ship };
    let other = app.world_mut().spawn((still, health, Transform::from_xyz(0.0, 0.0, 0.0), Drift::default())).id();
    for _ in 0..3 {
        app.update();
    }
    let (w, o) = (app.world().get::<Transform>(wreck).unwrap(), app.world().get::<Transform>(other).unwrap());
    assert!(w.translation.z < 3.0 - 0.3, "the wreck sails on towards the other ship: {}", w.translation.z);
    assert_eq!(o.translation, Vec3::ZERO, "nothing pushes the live ship");
    for _ in 0..40 {
        app.update();
    }
    let w = app.world().get::<Transform>(wreck).unwrap();
    assert!(w.translation.z < -3.0, "it passed through: {}", w.translation.z);
    assert_eq!(app.world().get::<Transform>(other).unwrap().translation, Vec3::ZERO);
}

/// A ship's ram clock starts when it is created: two enemy ships in contact take no ram damage in their first 3.5 s, and do after.
#[test]
#[ignore = "needs imported assets"]
fn a_new_ship_cannot_be_rammed_for_its_first_3_5_s() {
    let mut lib = crate::Library::load(crate::assets_dir());
    let a = lib.index("Ship_Navy_Cutter");
    let b = lib.index("Ship_Navy_Tug");
    for index in [a, b] {
        lib.classes[index].phys.thrust = 0.0;
        lib.classes[index].collision_bounds = [Vec3::splat(-1.0), Vec3::splat(1.0)];
    }
    let ha = crate::Health::new(&lib.classes[a]);
    let hb = crate::Health::new(&lib.classes[b]);
    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs_f32(0.1));
    app.insert_resource(time).insert_resource(lib).add_message::<Ram>().add_systems(Update, sail);
    let ship = crate::Ship { heading: 0.0, speed: 10.0, ..crate::Ship::new(a, crate::Side::Player) };
    let ea = app.world_mut().spawn((ship.clone(), ha, Transform::from_xyz(0.0, 0.0, 1.5), Drift::default())).id();
    let eb = app.world_mut().spawn((crate::Ship { class: b, side: crate::Side::Enemy, speed: 0.0, ..ship }, hb, Transform::default(), Drift::default())).id();
    let rams = |app: &mut App| app.world_mut().resource_mut::<Messages<Ram>>().drain().count();
    let overlap = |app: &mut App| {
        app.world_mut().get_mut::<Transform>(ea).unwrap().translation = Vec3::new(0.0, 0.0, 1.5);
        app.world_mut().get_mut::<Transform>(eb).unwrap().translation = Vec3::ZERO;
        app.world_mut().get_mut::<crate::Ship>(ea).unwrap().speed = 10.0;
    };
    app.update();
    assert_eq!(rams(&mut app), 0, "no ram damage 0.1 s after creation");
    app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(3.0));
    app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.07));
    overlap(&mut app);
    app.update();
    assert_eq!(rams(&mut app), 0, "still none 3.17 s after creation");
    app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.4));
    app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.1));
    overlap(&mut app);
    app.update();
    assert!(rams(&mut app) > 0, "the same contact rams after 3.5 s");
}

#[test]
fn a_teleported_hull_keeps_its_spot_when_free_and_is_nudged_to_the_nearest_free_one_when_not() {
    let bounds = [Vec3::new(-2.0, -1.0, -6.0), Vec3::new(2.0, 2.0, 6.0)];
    let there = Transform::from_xyz(100.0, 0.0, 100.0);
    let others = [spot::hull(bounds, &there)];
    let far = Vec3::new(100.0, 0.0, 200.0);
    assert_eq!(spot::free_spot(bounds, Quat::IDENTITY, far, &others), far, "free: unchanged");
    let stacked = spot::free_spot(bounds, Quat::IDENTITY, there.translation, &others);
    assert_ne!(stacked, there.translation);
    assert!(tree::overlap(&others[0], &spot::hull(bounds, &Transform::from_translation(stacked))).is_none(), "clear of the other hull");
    assert!(stacked.distance(there.translation) < 14.0, "and close: {stacked}");
    assert_eq!(stacked.y, 0.0, "height kept");
    // Wanted just off the other ship's bow: nudged a little, not moved across the whole hull.
    let wanted = there.translation + Vec3::new(0.0, 0.0, -9.0);
    let bow = spot::free_spot(bounds, Quat::IDENTITY, wanted, &others);
    assert!(bow.distance(wanted) < 6.0, "{bow}");
    // A hull heights apart does not touch.
    let above = Vec3::new(100.0, 40.0, 100.0);
    assert_eq!(spot::free_spot(bounds, Quat::IDENTITY, above, &others), above);
}
