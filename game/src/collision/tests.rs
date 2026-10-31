use super::*;
fn rect(x: f32, z: f32, yaw: f32) -> BoxBody {
    box_body([Vec3::new(-1.0, -1.0, -3.0), Vec3::new(1.0, 1.0, 3.0)], &Transform::from_xyz(x, 0.0, z).with_rotation(Quat::from_rotation_y(yaw)))
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
fn docking_route_goes_around_scenery_and_rejects_unreachable_points() {
    let bounds = [Vec3::new(-1.0, -1.0, -3.0), Vec3::new(1.0, 1.0, 3.0)];
    let island = Obstacle { body: box_body([Vec3::new(-30.0, -5.0, -20.0), Vec3::new(30.0, 5.0, 20.0)], &Transform::default()), triangles: vec![] };
    let start = Vec3::new(0.0, 0.0, 80.0);
    let goal = Vec3::new(0.0, 0.0, -80.0);
    let route = route_clear(bounds, start, goal, &[&island]).unwrap();
    assert!(route.len() >= 3, "cannot take a straight leg through the island");
    let mut from = start;
    for to in route {
        let d = to - from;
        let yaw = (-d.x).atan2(-d.z);
        for i in 0..=100 {
            let tf = Transform::from_translation(from.lerp(to, i as f32 / 100.0)).with_rotation(Quat::from_rotation_y(yaw));
            assert!(!touches(bounds, &tf, &[&island]));
        }
        from = to;
    }
    assert_eq!(from, goal);
    assert!(route_clear(bounds, start, Vec3::ZERO, &[&island]).is_none());
    assert_eq!(route_clear(bounds, start, goal, &[]).unwrap(), vec![goal]);
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
    let ship = crate::Ship {
        class,
        side: crate::Side::Player,
        heading: 0.0,
        speed: 20.0,
        ang_vel: 0.2,
        order: 2,
        waypoints: vec![Vec3::NEG_Z * 500.0],
        goal: None,
        cap: 1.0,
        throttle: None,
        sail_energy: 1.0,
        crew_speed: 1.0,
        crew_turn: 1.0,
        avoid: 1.0,
    };
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
        assert_eq!((ship.speed, ship.ang_vel, ship.order), (0.0, 0.0, 2));
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
    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs_f32(0.1));
    app.insert_resource(time).insert_resource(lib).add_systems(Update, sail);
    let ship = crate::Ship {
        class,
        side: crate::Side::Player,
        heading: 0.0,
        speed: 20.0,
        ang_vel: 0.0,
        order: 2,
        waypoints: vec![],
        goal: None,
        cap: 1.0,
        throttle: None,
        sail_energy: 1.0,
        crew_speed: 1.0,
        crew_turn: 1.0,
        avoid: 1.0,
    };
    let health = || crate::Health::new(&app.world().resource::<crate::Library>().classes[class]);
    let h1 = health();
    let h2 = health();
    let a = app.world_mut().spawn((ship.clone(), h1, Transform::from_xyz(0.0, 0.0, 4.0), Drift::default())).id();
    let b = app.world_mut().spawn((crate::Ship { heading: std::f32::consts::PI, ..ship }, h2, Transform::from_xyz(0.0, 0.0, -4.0), Drift::default())).id();
    for _ in 0..20 {
        app.update();
    }
    assert!(app.world().get::<Transform>(a).unwrap().translation.z > app.world().get::<Transform>(b).unwrap().translation.z, "ships cannot swap sides");
    app.world_mut().despawn(b);
    let wall = [Vec3::new(-20.0, -2.0, -5.0), Vec3::new(20.0, -2.0, -5.0), Vec3::new(0.0, 2.0, -5.0)];
    app.world_mut()
        .spawn(Obstacle { body: BoxBody { points: wall.to_vec(), bounds: Bounds::points(&wall) }, triangles: vec![Triangle { points: wall, bounds: Bounds::points(&wall) }] });
    for _ in 0..100 {
        app.update();
    }
    assert!(app.world().get::<Transform>(a).unwrap().translation.z >= -2.001, "bow must remain on this side of the wall");
}
#[test]
#[ignore = "needs imported assets"]
fn ai_ship_on_a_path_detours_round_a_wall_instead_of_grinding_into_it() {
    let mut lib = crate::Library::load(crate::assets_dir());
    let class = lib.index("Ship_Navy_Cutter");
    lib.classes[class].collision_bounds = [Vec3::new(-1.0, -1.0, -3.0), Vec3::new(1.0, 1.0, 3.0)];
    let health = crate::Health::new(&lib.classes[class]);
    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs_f32(0.1));
    app.insert_resource(time).insert_resource(lib).add_systems(Update, sail);
    // A 40-wide wall between the ship and its only path marker straight ahead.
    let wall = [Vec3::new(-20.0, -2.0, -30.0), Vec3::new(20.0, -2.0, -30.0), Vec3::new(0.0, 2.0, -30.0)];
    app.world_mut()
        .spawn(Obstacle { body: BoxBody { points: wall.to_vec(), bounds: Bounds::points(&wall) }, triangles: vec![Triangle { points: wall, bounds: Bounds::points(&wall) }] });
    let goal = Vec3::new(0.0, 0.0, -120.0);
    let ship = crate::Ship {
        class,
        side: crate::Side::Player,
        heading: 0.0,
        speed: 8.0,
        ang_vel: 0.0,
        order: 2,
        waypoints: vec![goal],
        goal: None,
        cap: 1.0,
        throttle: None,
        sail_energy: 1.0,
        crew_speed: 1.0,
        crew_turn: 1.0,
        avoid: 1.0,
    };
    let e = app.world_mut().spawn((ship, health, Transform::default(), Drift::default())).id();
    let mut best = f32::INFINITY;
    for _ in 0..1500 {
        app.update();
        best = best.min(app.world().get::<Transform>(e).unwrap().translation.distance(goal));
    }
    assert!(best < 10.0, "the ship reaches the marker behind the wall (closest {best})");
}
#[test]
#[ignore = "needs imported assets"]
fn ai_ship_slows_for_an_island_in_its_way_creeps_round_it_and_picks_up_speed_again() {
    // Retail Frigate on Border Dispute: 6.6 u/s straight at the map-centre island, down to 1.6 u/s at 5 units, never stopped, back to cruise.
    // A wall stands in for the island; the captain keeps asking for the straight line, as `ship::ai` does every tick.
    let lib = crate::Library::load(crate::assets_dir());
    let class = lib.index("Ship_Procyon_Frigate");
    let health = crate::Health::new(&lib.classes[class]);
    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs_f32(0.1));
    app.insert_resource(time).insert_resource(lib).add_systems(Update, sail);
    let wall = [Vec3::new(-45.0, -2.0, -200.0), Vec3::new(45.0, -2.0, -200.0), Vec3::new(0.0, 2.0, -200.0)];
    app.world_mut()
        .spawn(Obstacle { body: BoxBody { points: wall.to_vec(), bounds: Bounds::points(&wall) }, triangles: vec![Triangle { points: wall, bounds: Bounds::points(&wall) }] });
    let ship = crate::Ship {
        class,
        side: crate::Side::Enemy,
        heading: 0.0,
        speed: 6.6,
        ang_vel: 0.0,
        order: 3,
        waypoints: vec![],
        goal: Some(0.0),
        cap: 1.0,
        throttle: None,
        sail_energy: 1.0,
        crew_speed: 1.0,
        crew_turn: 1.0,
        avoid: 1.0,
    };
    let e = app.world_mut().spawn((ship, health, Transform::default(), Drift::default())).id();
    let (mut cruise, mut slowest, mut passed_at) = (0.0f32, f32::INFINITY, None);
    for frame in 0..1200 {
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
    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs_f32(0.1));
    app.insert_resource(time).insert_resource(lib).add_systems(Update, sail);
    app.world_mut().spawn(island);
    let ship = crate::Ship {
        class,
        side: crate::Side::Player,
        heading: 0.0,
        speed: 20.0,
        ang_vel: 0.0,
        order: 2,
        waypoints: vec![],
        goal: None,
        cap: 1.0,
        throttle: None,
        sail_energy: 1.0,
        crew_speed: 1.0,
        crew_turn: 1.0,
        avoid: 1.0,
    };
    let e = app.world_mut().spawn((ship, health, Transform::from_xyz(0.0, 0.0, 80.0), Drift::default())).id();
    for _ in 0..100 {
        app.update();
        assert!(app.world().get::<Transform>(e).unwrap().translation.length() > 10.0, "must not traverse the solid centre of Island01");
    }
}
#[test]
fn course_prefix_stops_before_a_wall() {
    let wall = [Vec3::new(-20.0, -2.0, -30.0), Vec3::new(20.0, -2.0, -30.0), Vec3::new(0.0, 2.0, -30.0)];
    let obstacle = Obstacle { body: BoxBody { points: wall.to_vec(), bounds: Bounds::points(&wall) }, triangles: vec![Triangle { points: wall, bounds: Bounds::points(&wall) }] };
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
#[test]
fn steer_away_keeps_clear_of_a_head_on_island() {
    let block = [Vec3::new(-10.0, -2.0, -30.0), Vec3::new(10.0, -2.0, -30.0), Vec3::new(0.0, 2.0, -30.0)];
    let obstacle =
        Obstacle { body: BoxBody { points: block.to_vec(), bounds: Bounds::points(&block) }, triangles: vec![Triangle { points: block, bounds: Bounds::points(&block) }] };
    let scenery = [&obstacle];
    let bounds = [Vec3::new(-1.0, -1.0, -3.0), Vec3::new(1.0, 1.0, 3.0)];
    let tf = Transform::default();
    // Heading straight for the obstacle (-Z is heading 0): must be nudged, not kept.
    let want = 0.0;
    let h = steer_clear(bounds, &tf, 10.0, want, &scenery);
    assert!(h.abs() > 0.1, "head-on order is nudged, got {h}");
    assert!(h.abs() <= 2.2 + 1e-4, "nudge stays within the offset ceiling");
    // A clear heading is returned unchanged.
    assert_eq!(steer_clear(bounds, &tf, 10.0, 0.0, &[]), 0.0);
    let far = Transform::from_xyz(0.0, 0.0, 200.0);
    assert_eq!(steer_clear(bounds, &far, 10.0, 0.0, &scenery), 0.0);
}
#[test]
fn land_ahead_slows_a_ship_along_the_retail_curve_and_a_clear_pass_does_not() {
    // Retail Procyon Frigate, 6.6 u/s, land on its line: 5.7 u/s at 47 units from the coast, 3.6 at 21, 1.6 at 5 (0.86 / 0.55 / 0.24 of cruise).
    let bounds = [Vec3::new(-1.0, -1.0, -3.0), Vec3::new(1.0, 1.0, 3.0)];
    let wall_at = |z: f32| {
        let t = [Vec3::new(-40.0, -2.0, z), Vec3::new(40.0, -2.0, z), Vec3::new(0.0, 2.0, z)];
        Obstacle { body: BoxBody { points: t.to_vec(), bounds: Bounds::points(&t) }, triangles: vec![Triangle { points: t, bounds: Bounds::points(&t) }] }
    };
    let slow = |coast: f32| contact_slow(bounds, Vec3::ZERO, 0.0, 6.6, &[&wall_at(-coast)]);
    for (coast, retail) in [(47.0, 0.86), (21.0, 0.55), (5.0, 0.24)] {
        let f = slow(coast);
        assert!((f - retail).abs() < 0.12, "{coast} units from land: {f} against retail {retail}");
    }
    assert_eq!(slow(200.0), 1.0, "land beyond the look-ahead does not slow it");
    assert!(slow(10.0) < slow(20.0) && slow(20.0) < slow(40.0), "slower the closer the land");
    // An island it would clear by 27 units is passed at full speed (the retail Frigate did exactly that at Island08).
    let beside = {
        let t = [Vec3::new(27.0, -2.0, -60.0), Vec3::new(27.0, -2.0, 20.0), Vec3::new(80.0, 2.0, -20.0)];
        Obstacle { body: BoxBody { points: t.to_vec(), bounds: Bounds::points(&t) }, triangles: vec![Triangle { points: t, bounds: Bounds::points(&t) }] }
    };
    assert_eq!(contact_slow(bounds, Vec3::ZERO, 0.0, 6.6, &[&beside]), 1.0);
}
fn ship_rock_contact(mass: f32, rock_velocity: Vec3) -> (Vec3, Vec3, Vec3) {
    let mut lib = crate::Library::load(crate::assets_dir());
    let class = lib.index("Ship_Navy_Cutter");
    lib.classes[class].phys.thrust = 0.0; // Isolate collision from engine acceleration/braking.
    lib.classes[class].collision_bounds = [Vec3::splat(-1.0), Vec3::splat(1.0)];
    let health = crate::Health::new(&lib.classes[class]);
    let mut defs = crate::data::AsteroidDefs::from_world(&mut World::new());
    defs.0.get_mut("Asteroid_Small").unwrap().mass = mass;
    let mut rock = crate::combat::Asteroid::new("Asteroid_Small", "rocks", &defs).unwrap();
    rock.velocity = rock_velocity;
    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs_f32(0.1));
    app.insert_resource(time).insert_resource(lib).add_systems(Update, sail);
    let ship = crate::Ship {
        class,
        side: crate::Side::Player,
        heading: 0.0,
        speed: 10.0,
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
    };
    let a = app.world_mut().spawn((ship, health, Transform::from_xyz(0.0, 0.0, 1.5), Drift::default())).id();
    let body = box_body([Vec3::splat(-1.0), Vec3::splat(1.0)], &Transform::default());
    let b = app.world_mut().spawn((rock, Transform::default(), Drift::default(), Obstacle { body, triangles: vec![] })).id();
    app.update();
    let ship = app.world().get::<crate::Ship>(a).unwrap();
    let va = forward(ship) * ship.speed + app.world().get::<Drift>(a).unwrap().0;
    let vb = app.world().get::<crate::combat::Asteroid>(b).unwrap().velocity + app.world().get::<Drift>(b).unwrap().0;
    let position = app.world().get::<Transform>(b).unwrap().translation;
    let obstacle = app.world().get::<Obstacle>(b).unwrap();
    assert!((obstacle.body.bounds.min - (position - Vec3::ONE)).length() < 1e-5, "rock collider follows contact movement");
    if rock_velocity == Vec3::ZERO {
        app.update();
        let next = app.world().get::<Transform>(b).unwrap().translation;
        assert!((next - position - vb * 0.1).length() < 1e-4, "contact velocity must persist into the next frame");
    }
    (va, vb, position)
}

#[test]
#[ignore = "needs imported assets"]
fn ship_asteroid_contact_transfers_momentum_by_authored_mass() {
    for mass in [1000.0, 40000.0] {
        let (ship, rock, position) = ship_rock_contact(mass, Vec3::ZERO);
        let expected = -4000.0 * 10.0 / (4000.0 + mass);
        assert!((ship.z - expected).abs() < 1e-4, "ship response for rock mass {mass}: {ship:?}");
        assert!((rock.z - expected).abs() < 1e-4, "finite-mass rock must receive momentum: {rock:?}");
        assert!(position.z < 0.0, "rock is displaced rather than treated as an immovable wall");
        assert!((ship * 4000.0 + rock * mass - Vec3::NEG_Z * 40000.0).length() < 0.05);
    }
}

#[test]
#[ignore = "needs imported assets"]
fn ecs_ship_contact_preserves_light_and_heavy_ship_mass_ratios() {
    for other in ["Ship_Navy_Torpedo", "Ship_Navy_Tug"] {
        let mut lib = crate::Library::load(crate::assets_dir());
        let a = lib.index("Ship_Navy_Cutter");
        let b = lib.index(other);
        let expected = 10.0 * lib.classes[a].phys.mass / (lib.classes[a].phys.mass + lib.classes[b].phys.mass);
        for index in [a, b] {
            lib.classes[index].phys.thrust = 0.0;
            lib.classes[index].collision_bounds = [Vec3::splat(-1.0), Vec3::splat(1.0)];
        }
        let ha = crate::Health::new(&lib.classes[a]);
        let hb = crate::Health::new(&lib.classes[b]);
        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs_f32(0.1));
        app.insert_resource(time).insert_resource(lib).add_systems(Update, sail);
        let ship = crate::Ship {
            class: a,
            side: crate::Side::Player,
            heading: 0.0,
            speed: 10.0,
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
        };
        let ea = app.world_mut().spawn((ship.clone(), ha, Transform::from_xyz(0.0, 0.0, 1.5), Drift::default())).id();
        let eb = app.world_mut().spawn((crate::Ship { class: b, speed: 0.0, ..ship }, hb, Transform::default(), Drift::default())).id();
        app.update();
        for e in [ea, eb] {
            assert!((app.world().get::<crate::Ship>(e).unwrap().speed - expected).abs() < 1e-4, "incorrect momentum transfer for {other}");
        }
    }
}

#[test]
#[ignore = "needs imported assets"]
fn moving_asteroid_contact_uses_relative_velocity() {
    let (ship, rock, _) = ship_rock_contact(10000.0, Vec3::Z * 4.0);
    assert!(ship.length() < 1e-4 && rock.length() < 1e-4, "equal opposing momenta stop in an inelastic contact: {ship:?}, {rock:?}");
}

#[test]
fn off_centre_impulse_transfers_spin_without_creating_energy() {
    let a = ContactBody { velocity: Vec3::NEG_Z * 10.0, inverse_mass: 0.5, inverse_inertia: 0.2, arm: Vec3::X * 2.0, ..fixed_body() };
    let b = ContactBody { inverse_mass: 0.25, inverse_inertia: 0.1, arm: Vec3::NEG_X * 2.0, ..fixed_body() };
    let j = impulse(&a, &b, Vec3::Z);
    let va = a.velocity + j * a.inverse_mass;
    let vb = b.velocity - j * b.inverse_mass;
    let wa = a.arm.cross(j).y * a.inverse_inertia;
    let wb = -b.arm.cross(j).y * b.inverse_inertia;
    assert!(wa < 0.0 && wb < 0.0);
    let after = va.length_squared() / a.inverse_mass + vb.length_squared() / b.inverse_mass + wa * wa / a.inverse_inertia + wb * wb / b.inverse_inertia;
    assert!(after <= a.velocity.length_squared() / a.inverse_mass);
    let point_velocity = va + Vec3::Y.cross(a.arm) * wa - vb - Vec3::Y.cross(b.arm) * wb;
    assert!(point_velocity.z.abs() < 1e-4);
    assert!((va / a.inverse_mass + vb / b.inverse_mass - a.velocity / a.inverse_mass).length() < 1e-4);
}

#[test]
#[ignore = "needs imported assets"]
fn tow_uses_mass_and_independent_hardpoints_and_breaks_without_teleporting() {
    let lib = crate::Library::load(crate::assets_dir());
    let class = lib.index("Ship_Navy_Cutter");
    let health = || crate::Health::new(&lib.classes[class]);
    let hull = crate::Ship {
        class,
        side: crate::Side::Player,
        heading: 0.0,
        speed: 10.0,
        ang_vel: 0.0,
        order: 2,
        waypoints: vec![],
        goal: None,
        cap: 1.0,
        throttle: None,
        sail_energy: 1.0,
        crew_speed: 1.0,
        crew_turn: 1.0,
        avoid: 1.0,
    };
    let mut app = App::new();
    let a = app.world_mut().spawn((hull.clone(), health(), Transform::default(), Drift::default())).id();
    let b = app.world_mut().spawn((crate::Ship { speed: 0.0, order: 0, ..hull }, health(), Transform::from_xyz(3.0, 0.0, 30.0), Drift::default())).id();
    let mut mission = crate::campaign::load();
    mission.script.set_tow(a, b, true);
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs_f32(0.02));
    app.insert_resource(time).insert_resource(lib).insert_resource(mission).add_systems(Update, sail);
    for _ in 0..100 {
        app.update();
    }
    let tower = app.world().get::<crate::Ship>(a).unwrap();
    let cargo = app.world().get::<crate::Ship>(b).unwrap();
    assert!(cargo.speed > 0.0 && cargo.ang_vel.abs() > 0.001, "off-axis bow attachment must pull and turn the cargo");
    assert!((cargo.heading - tower.heading).abs() > 0.001, "heading must not be copied");
    let before = app.world().get::<Transform>(b).unwrap().translation;
    assert!(before.z < 29.0);
    app.world_mut().resource_mut::<crate::campaign::Mission>().script.break_tows(a);
    app.world_mut().get_mut::<Transform>(a).unwrap().translation.x += 500.0;
    app.update();
    assert!(app.world().get::<Transform>(b).unwrap().translation.distance(before) < 1.0);
}

#[test]
fn impulse_conserves_momentum_and_leaves_separating_contacts_alone() {
    let va = Vec3::X * -10.0;
    let vb = Vec3::ZERO;
    let a_body = ContactBody { velocity: va, inverse_mass: 0.5, ..fixed_body() };
    let b_body = ContactBody { velocity: vb, inverse_mass: 0.25, ..fixed_body() };
    let j = impulse(&a_body, &b_body, Vec3::X);
    let a = va + j * 0.5;
    let b = vb - j * 0.25;
    assert!((a.x - b.x).abs() < 1e-5);
    assert!((a * 2.0 + b * 4.0 - va * 2.0).length() < 1e-4);
    assert_eq!(impulse(&ContactBody { velocity: Vec3::X, inverse_mass: 1.0, ..fixed_body() }, &b_body, Vec3::X), Vec3::ZERO);
}
