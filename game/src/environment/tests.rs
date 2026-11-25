use super::*;

fn sensor_object(position: Vec3, radius: f32, group: &str, sight: f32) -> SensorObject {
    SensorObject { at: position, sphere: (position, radius), sight: Some(sight), group: group.into(), shown: true, cloak: None, side: crate::Side(u8::MAX), scenery: false }
}

#[test]
fn sight_uses_strict_surface_distance_and_three_dimensions() {
    let (a, b) = (Entity::from_bits(1), Entity::from_bits(2));
    let mut sensors = Sensors { primary: Some(a), ..default() };
    sensors.objects.insert(a, sensor_object(Vec3::ZERO, 10.0, "A", 100.0));
    sensors.objects.insert(b, sensor_object(Vec3::X * 130.0, 20.0, "B", 100.0));
    assert!(!sensors.in_sight(a, b, None), "equal to the range is outside");
    sensors.objects.get_mut(&b).unwrap().sphere.0.x -= 0.01;
    assert!(sensors.sees_now(b, None), "surface, not centre distance");
    sensors.objects.get_mut(&b).unwrap().sphere.0 = Vec3::Y * 140.0;
    assert!(!sensors.sees_now(b, None), "altitude counts");
}

#[test]
fn group_sharing_excludes_unrelated_hidden_or_dead_observers() {
    let (a, b, c) = (Entity::from_bits(1), Entity::from_bits(2), Entity::from_bits(3));
    let mut sensors = Sensors { primary: Some(a), ..default() };
    sensors.objects.insert(a, sensor_object(Vec3::ZERO, 0.0, "A", 100.0));
    sensors.objects.insert(b, sensor_object(Vec3::X * 200.0, 0.0, "B", 100.0));
    sensors.objects.insert(c, sensor_object(Vec3::X * 150.0, 0.0, "Other ally", 100.0));
    assert!(!sensors.sees_now(b, None));
    sensors.objects.get_mut(&c).unwrap().group = "A".into();
    assert!(sensors.sees_now(b, None));
    sensors.objects.get_mut(&c).unwrap().shown = false;
    assert!(!sensors.sees_now(b, None));
    sensors.objects.get_mut(&c).unwrap().shown = true;
    sensors.objects.get_mut(&b).unwrap().shown = false;
    assert!(!sensors.sees_now(b, None));
}

#[test]
fn cloak_script_visibility_no_primary_and_scenery_are_separate() {
    let (a, b) = (Entity::from_bits(1), Entity::from_bits(2));
    let mut sensors = Sensors { primary: Some(a), ..default() };
    sensors.objects.insert(a, sensor_object(Vec3::ZERO, 0.0, "A", 100.0));
    sensors.objects.insert(b, sensor_object(Vec3::X * 80.0, 0.0, "B", 100.0));
    sensors.objects.get_mut(&b).unwrap().cloak = Some(vec![]);
    assert!(!sensors.sees_now(b, None));
    sensors.objects.get_mut(&b).unwrap().sphere.0.x = 10.0;
    sensors.objects.get_mut(&b).unwrap().at.x = 10.0;
    assert!(!sensors.sees_now(b, None), "no distance reveals a cloaked ship");
    sensors.objects.get_mut(&b).unwrap().cloak = Some(vec![crate::Side(u8::MAX)]);
    assert!(sensors.sees_now(b, None), "a team that knows it sees it");
    sensors.objects.get_mut(&b).unwrap().cloak = None;
    sensors.objects.get_mut(&b).unwrap().shown = false;
    sensors.primary = None;
    assert!(!sensors.sees_now(b, None), "no primary never reveals a script-hidden object");
    sensors.objects.get_mut(&b).unwrap().shown = true;
    sensors.objects.get_mut(&b).unwrap().cloak = None;
    assert!(sensors.sees_now(b, None), "native no-primary exception");
    sensors.primary = Some(a);
    sensors.objects.get_mut(&b).unwrap().sphere.0.x = 1000.0;
    sensors.objects.get_mut(&b).unwrap().scenery = true;
    assert!(sensors.sees_now(b, None), "do not apply ship contact policy to islands");
    assert!(!sensors.in_sight(a, b, None), "docking still needs group sight");
}

#[test]
#[ignore = "needs imported assets"]
fn sensor_snapshot_tracks_ship_spheres_script_changes_and_cleanup() {
    use crate::combat::Health;
    let mut app = App::new();
    let lib = crate::testutil::lib();
    let class = lib.index("Ship_Navy_Cutter");
    let health = Health::new(&lib.classes[class]);
    let sphere = lib.classes[class].sphere;
    app.insert_resource(lib).init_resource::<Sensors>().add_systems(Update, refresh_sensors);
    let tf = Transform::from_xyz(10.0, 20.0, 30.0).with_rotation(Quat::from_rotation_y(0.8));
    let ship = Ship { heading: 0.8, ..Ship::new(class, crate::Side::Player) };
    let e = app
        .world_mut()
        .spawn((ship, tf, health, crate::PlayerShip, crate::ship::FleetMember("Player".into(), 0), crate::campaign::MissionShip { visible: true, ..default() }))
        .id();
    app.update();
    let sensors = app.world().resource::<Sensors>();
    assert_eq!(sensors.primary, Some(e));
    assert_eq!(sensors.objects[&e].sphere, (tf.transform_point(sphere.0), sphere.1));
    let dock = app
        .world_mut()
        .spawn((Transform::from_xyz(100.0, 0.0, 0.0), crate::docking::Dockable::new(vec![crate::docking::DockPoint { pos: Vec3::Z * 3.0, fwd: Vec3::NEG_Z }], true)))
        .id();
    app.update();
    assert_eq!(app.world().resource::<Sensors>().objects[&dock].sphere, (Vec3::X * 100.0, 33.0));
    assert!(app.world().resource::<Sensors>().in_sight(e, dock, None), "collider-free mod docks remain reachable");
    app.world_mut().get_mut::<crate::campaign::MissionShip>(e).unwrap().visible = false;
    app.update();
    assert!(!app.world().resource::<Sensors>().player_sees(e, None));
    app.world_mut().despawn(e);
    app.world_mut().despawn(dock);
    app.update();
    assert!(app.world().resource::<Sensors>().objects.is_empty());
    assert!(app.world().resource::<Sensors>().primary.is_none());
}

#[test]
fn current_ribbon_follows_the_path_and_matches_the_push_width() {
    use bevy::mesh::VertexAttributeValues;
    let path = [Vec3::ZERO, Vec3::new(0.0, 8.0, -240.0), Vec3::new(240.0, 8.0, -480.0)];
    let current = Current::new(path.to_vec());
    let mesh = current_mesh(&current.path);
    let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
        panic!("ribbon vertices");
    };
    assert_eq!(positions.len(), path.len() * 2);
    assert_eq!(mesh.indices().unwrap().len(), (path.len() - 1) * 6);
    for (p, vertices) in current.path.iter().zip(positions.as_chunks::<2>().0) {
        let (a, b) = (Vec3::from_array(vertices[0]), Vec3::from_array(vertices[1]));
        assert!(((a + b) / 2.0).distance(*p) < 0.001);
        assert!((a.distance(b) - 2.0 * CURRENT_RADIUS).abs() < 0.001);
    }
    assert_eq!(current.force(Vec3::new(0.0, 0.0, -100.0)), Vec3::NEG_Z * CURRENT_FORCE);
    assert_eq!(current.force(Vec3::new(CURRENT_RADIUS + 1.0, 0.0, -100.0)), Vec3::ZERO);
    assert_eq!(Current::new(vec![]).force(Vec3::ZERO), Vec3::ZERO);
    assert_eq!(current_mesh(&[]).count_vertices(), 0);
}
#[test]
#[ignore = "needs imported assets"]
fn mission4_current_pushes_the_way_nelsons_fleet_sails_out() {
    use bevy::mesh::VertexAttributeValues;
    let mission = crate::campaign::load_id("mission04");
    let path = |name: &str| -> Vec<Vec3> { mission.data["paths"][name].as_array().unwrap().iter().map(game_to_bevy).collect() };
    let current = Current::new(path("Mucullough path"));
    let route = path("Nelson exit route");
    let inside: Vec<_> = route.windows(2).filter(|w| current.force(w[0]) != Vec3::ZERO).collect();
    assert!(inside.len() >= 3, "the exit route runs inside the current");
    for w in inside {
        assert!(current.force(w[0]).dot(w[1] - w[0]) > 0.0, "the current carries the fleet toward the frontier");
    }
    let mesh = current_mesh(&current.path);
    let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else { panic!("ribbon UVs") };
    assert!(uv.last().unwrap()[1] > uv[0][1], "negative UV scrolling moves the texture downstream, toward the last point");
}

#[test]
fn cloud_blocks_crossing_and_interior_not_exterior_segments() {
    let polygon = serde_json::json!([[0, 0], [10, 0], [10, 10], [0, 10]]);
    assert!(crosses(&polygon, Vec2::new(-5.0, 5.0), Vec2::new(15.0, 5.0)));
    assert!(crosses(&polygon, Vec2::new(5.0, 5.0), Vec2::new(15.0, 5.0)));
    assert!(!crosses(&polygon, Vec2::new(3.0, 5.0), Vec2::new(7.0, 5.0)), "same cloud does not obscure its interior");
    assert!(!crosses(&polygon, Vec2::new(-5.0, 15.0), Vec2::new(15.0, 15.0)));
    let concave = serde_json::json!([[0, 0], [10, 0], [10, 10], [6, 10], [6, 4], [4, 4], [4, 10], [0, 10]]);
    assert!(crosses(&concave, Vec2::new(2.0, 8.0), Vec2::new(8.0, 8.0)), "same cloud endpoints can cross its boundary");
}
#[test]
#[ignore = "needs imported assets"]
fn enabled_nebula_gates_short_sight_and_group_members_can_see_around_it() {
    let mut mission = crate::campaign::load_id("mission02");
    mission.opening = None;
    mission.script.nis = false;
    mission.data["volumes"]["Sensor test"] = serde_json::json!([[0, 0], [10, 0], [10, 10], [0, 10]]);
    mission.data["environment"]["nebulae"] = serde_json::json!([{"Polygon Name": "Sensor test", "Nebula Occlusion On/Off": "TRUE"}]);
    assert!(!can_see(Some(&mission), Vec3::new(-1.0, 0.0, -5.0), Vec3::new(1.0, 0.0, -5.0)), "no invented 40-unit bypass");
    assert!(can_see(Some(&mission), Vec3::new(2.0, 0.0, -5.0), Vec3::new(8.0, 0.0, -5.0)));
    let (a, b, c) = (Entity::from_bits(1), Entity::from_bits(2), Entity::from_bits(3));
    let mut sensors = Sensors { primary: Some(a), ..default() };
    sensors.objects.insert(a, sensor_object(Vec3::new(-5.0, 0.0, -5.0), 0.0, "A", 100.0));
    sensors.objects.insert(b, sensor_object(Vec3::new(15.0, 0.0, -5.0), 0.0, "B", 100.0));
    assert!(!sensors.sees_now(b, Some(&mission)));
    sensors.objects.insert(c, sensor_object(Vec3::new(15.0, 0.0, -15.0), 0.0, "A", 100.0));
    assert!(sensors.sees_now(b, Some(&mission)));
    sensors.objects.remove(&c);
    mission.data["environment"]["nebulae"][0]["Nebula Occlusion On/Off"] = Value::from("FALSE");
    assert!(sensors.sees_now(b, Some(&mission)));
}

#[test]
fn whale_paths_loop_and_duplicate_points_do_not_hang() {
    let mut animal = SpaceAnimal { speed: 12.0, call_in: 3.0, calls: 0, ..SpaceAnimal::new("Whales", 6.0, Vec3::ZERO, 0, true) };
    let mut tf = Transform::default();
    animal.follow(&[Vec3::ZERO, Vec3::X * 12.0], tf.translation, true, true);
    animal.advance(&mut tf, 0.0);
    assert_eq!(tf.translation, Vec3::ZERO);
    assert_eq!(animal.next, 0);
    animal.advance(&mut tf, 1.0);
    assert_eq!(tf.translation, Vec3::X * 12.0);
    animal.advance(&mut tf, 1.0);
    assert_eq!(tf.translation, Vec3::ZERO);
    animal.follow(&[Vec3::ZERO, Vec3::ZERO], tf.translation, false, true);
    animal.advance(&mut tf, 1.0);
    assert_eq!(tf.translation, Vec3::ZERO);
}

#[test]
fn an_unseen_ships_running_effects_stop_drawing_and_its_hull_does_not() {
    use bevy::ecs::system::RunSystemOnce;
    let mut world = World::new();
    world.insert_resource(crate::fx::FxLib::with_names(&["Engine", "Hit"]));
    let health = || crate::combat::Health { sections: vec![], core: 1.0, destroyed: false };
    let own = world.spawn((Ship::new(0, crate::Side::Player), health(), Visibility::default())).id();
    let other = world.spawn((Ship::new(0, crate::Side(1)), health(), Visibility::default())).id();
    let (engine, hit) = world
        .run_system_once(move |mut commands: Commands, lib: Res<crate::fx::FxLib>| {
            let at = Transform::IDENTITY;
            let engine = crate::fx::spawn(&mut commands, &lib, "Engine", Some(other), at, at, false).unwrap();
            let hit = crate::fx::spawn(&mut commands, &lib, "Hit", Some(other), at, at, true).unwrap();
            commands.entity(hit).insert(crate::combat::Transient);
            (engine, hit)
        })
        .unwrap();
    // The player's ship sees 500 (a Man-O-War): the other ship 520 away (surface) is out of sight, 480 away in it.
    let mut sensors = Sensors { primary: Some(own), ..default() };
    sensors.objects.insert(own, sensor_object(Vec3::ZERO, 0.0, "A", 500.0));
    sensors.objects.insert(other, sensor_object(Vec3::X * 520.0, 0.0, "B", 500.0));
    sensors.advance_sightings(None, 0.0, 0.0);
    world.insert_resource(sensors);
    world.run_system_once(mark_unseen_ship_effects).unwrap();
    world.flush();
    assert!(world.get::<Unseen>(engine).is_some() && world.get::<Visibility>(engine) == Some(&Visibility::Hidden), "out of sight: the exhaust stops drawing");
    assert!(world.get::<Unseen>(hit).is_none(), "one-shot effects are not touched");
    assert_eq!(world.get::<Visibility>(other), Some(&Visibility::Inherited), "the hull is still drawn");
    world.resource_mut::<Sensors>().objects.get_mut(&other).unwrap().sphere.0.x = 480.0;
    world.resource_mut::<Sensors>().advance_sightings(None, 0.5, 0.5);
    world.run_system_once(mark_unseen_ship_effects).unwrap();
    world.flush();
    assert!(world.get::<Unseen>(engine).is_some(), "in sight, but the re-test is once a second");
    world.resource_mut::<Sensors>().advance_sightings(None, 1.1, 0.6);
    world.run_system_once(mark_unseen_ship_effects).unwrap();
    world.flush();
    assert!(world.get::<Unseen>(engine).is_none() && world.get::<Visibility>(engine) == Some(&Visibility::Inherited), "in sight again: drawn");
    world.resource_mut::<Sensors>().objects.get_mut(&other).unwrap().sphere.0.x = 520.0;
    world.get_mut::<crate::combat::Health>(other).unwrap().destroyed = true;
    world.run_system_once(mark_unseen_ship_effects).unwrap();
    world.flush();
    assert!(world.get::<Unseen>(engine).is_none(), "a sunk ship keeps its effects");
}

/// Two ships 480 apart with sight 500 (in sight) or 520 apart (out of it); `b` is the other ship.
fn view(apart: f32) -> (Sensors, Entity, Entity) {
    let (a, b) = (Entity::from_bits(1), Entity::from_bits(2));
    let mut sensors = Sensors { primary: Some(a), ..default() };
    sensors.objects.insert(a, sensor_object(Vec3::ZERO, 0.0, "A", 500.0));
    sensors.objects.insert(b, sensor_object(Vec3::X * apart, 0.0, "B", 500.0));
    (sensors, a, b)
}
fn fade(sensors: &Sensors, e: Entity) -> f32 {
    sensors.sightings[&e].fade
}
/// Frames of `dt` real seconds from `from` to `to` (exclusive of `from`).
fn frames(sensors: &mut Sensors, from: f64, to: f64, dt: f64) -> f64 {
    let mut t = from;
    while t + dt <= to + 1e-9 {
        t += dt;
        sensors.advance_sightings(None, t, dt as f32);
    }
    t
}

#[test]
fn an_object_in_sight_when_it_appears_is_shown_at_once_at_full_fade() {
    let (mut sensors, a, b) = view(480.0);
    sensors.advance_sightings(None, 0.0, 0.0);
    assert!(sensors.player_sees(b, None) && sensors.player_sees(a, None));
    assert_eq!(fade(&sensors, b), 1.0);
    let (mut far, _, b) = view(520.0);
    far.advance_sightings(None, 0.0, 0.0);
    assert!(!far.player_sees(b, None));
    assert_eq!(fade(&far, b), 0.0);
}

#[test]
fn sight_is_re_tested_only_after_more_than_one_real_second() {
    let (mut sensors, _, b) = view(520.0);
    sensors.advance_sightings(None, 0.0, 0.0);
    sensors.objects.get_mut(&b).unwrap().sphere.0.x = 480.0;
    frames(&mut sensors, 0.0, 1.0, 0.0625);
    assert!(!sensors.player_sees(b, None), "exactly 1 s later is not yet a re-test");
    sensors.advance_sightings(None, 1.0625, 0.0625);
    assert!(sensors.player_sees(b, None), "seen from the re-test on");
    assert_eq!(fade(&sensors, b), 0.0, "the fade starts from 0 and takes no step on the re-test frame");
    sensors.retest_now = true;
    sensors.objects.get_mut(&b).unwrap().sphere.0.x = 520.0;
    sensors.advance_sightings(None, 1.125, 0.0625);
    assert!(!sensors.sightings[&b].seen, "a forced re-test does not wait");
}

#[test]
fn the_fade_rises_at_0_8_per_second_after_a_sighting_and_falls_as_fast_after_a_loss() {
    // Frames of 1/16 s; re-tests on the first frame more than 1 s after the last one.
    let (mut sensors, _, b) = view(520.0);
    sensors.advance_sightings(None, 0.0, 0.0);
    sensors.objects.get_mut(&b).unwrap().sphere.0.x = 480.0;
    frames(&mut sensors, 0.0, 1.0625, 0.0625); // re-test at 1.0625: seen, fade 0
    assert!(sensors.sightings[&b].seen && fade(&sensors, b) == 0.0);
    frames(&mut sensors, 1.0625, 1.5625, 0.0625);
    assert!((fade(&sensors, b) - 0.4).abs() < 1e-4, "0.5 s seen: 0.8 x 0.5 = 0.4, not {}", fade(&sensors, b));
    frames(&mut sensors, 1.5625, 2.0625, 0.0625);
    assert!((fade(&sensors, b) - 0.8).abs() < 1e-4, "1.0 s seen: 0.8, not {}", fade(&sensors, b));
    frames(&mut sensors, 2.0625, 2.5, 0.0625); // re-test at 2.125 (no step), then capped
    assert_eq!(fade(&sensors, b), 1.0, "capped at 1");
    // Lost at 2.5; the re-test at 3.1875 drops it, then the fade falls 1 -> 0 in 1.25 s (one more skipped frame at the 4.25 re-test).
    sensors.objects.get_mut(&b).unwrap().sphere.0.x = 520.0;
    frames(&mut sensors, 2.5, 3.1875, 0.0625);
    assert!(!sensors.sightings[&b].seen && sensors.player_sees(b, None), "lost but fading: still shown");
    assert_eq!(fade(&sensors, b), 1.0);
    frames(&mut sensors, 3.1875, 4.4375, 0.0625);
    assert!(sensors.player_sees(b, None), "19 steps after the loss the fade is {} > 0", fade(&sensors, b));
    frames(&mut sensors, 4.4375, 4.5, 0.0625);
    assert!(!sensors.player_sees(b, None), "20 steps (1.25 s) after the loss it is gone");
    assert_eq!(fade(&sensors, b), 0.0);
}

#[test]
fn a_line_between_two_objects_is_shown_only_while_both_ends_are() {
    let (mut sensors, a, b) = view(480.0);
    sensors.advance_sightings(None, 0.0, 0.0);
    assert!(sensors.player_sees_both(a, b, None));
    sensors.objects.get_mut(&b).unwrap().sphere.0.x = 520.0;
    sensors.retest_now = true;
    sensors.advance_sightings(None, 0.1, 0.1);
    assert!(sensors.player_sees_both(a, b, None), "the far end is fading out");
    frames(&mut sensors, 0.1, 1.5, 0.05);
    assert!(!sensors.player_sees_both(a, b, None) && sensors.player_sees(a, None), "one end unseen: no line");
}

/// The `advance_sightings` system's view of `b` after each of 120 updates, with `sleep` of wall time between updates and the
/// `--fixed-step` time strategy; `b` leaves sight at update 40.
fn fixed_step_sightings(sleep: std::time::Duration) -> Vec<(bool, f32)> {
    let (sensors, _, b) = view(480.0);
    let mut app = App::new();
    app.add_plugins(bevy::time::TimePlugin)
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(crate::BATTLE_TICK))
        .insert_resource(sensors)
        .add_systems(Update, advance_sightings);
    (0..120)
        .map(|i| {
            if i == 40 {
                app.world_mut().resource_mut::<Sensors>().objects.insert(b, sensor_object(Vec3::X * 520.0, 0.0, "B", 500.0));
            }
            std::thread::sleep(sleep);
            app.update();
            let s = &app.world().resource::<Sensors>().sightings[&b];
            (s.seen, s.fade)
        })
        .collect()
}

#[test]
fn under_fixed_step_the_view_advances_one_tick_per_frame_whatever_the_frame_rate() {
    let fast = fixed_step_sightings(std::time::Duration::ZERO);
    let slow = fixed_step_sightings(std::time::Duration::from_millis(5));
    assert_eq!(fast, slow);
    // It did play out: lost at the first re-test after the move, then faded out.
    assert!(fast[39].0 && !fast[119].0 && fast[119].1 < fast[60].1);
}

/// Not retail: `OP Etherium Current` sets a current's strength and lane and switches it off; a current nobody changed pushes exactly as before.
#[test]
fn a_current_can_be_made_stronger_wider_and_switched_off() {
    let path = vec![Vec3::ZERO, Vec3::NEG_Z * 500.0];
    let mut current = Current::new(path.clone()).named("lane");
    assert_eq!(current.name(), "lane");
    let (inside, edge) = (Vec3::new(0.0, 0.0, -100.0), Vec3::new(CURRENT_RADIUS + 20.0, 0.0, -100.0));
    assert_eq!((current.force(inside), current.force(edge)), (Vec3::NEG_Z * CURRENT_FORCE, Vec3::ZERO), "the retail strength and lane");
    current.set(Some(2.0), None, None);
    assert_eq!(current.force(inside), Vec3::NEG_Z * CURRENT_FORCE * 2.0);
    current.set(None, Some(4.0 * CURRENT_RADIUS), None);
    assert_eq!(current.force(edge), Vec3::NEG_Z * CURRENT_FORCE * 2.0, "a lane twice as wide reaches the ship at the edge");
    let mesh = current_mesh_of(&path, 2.0 * CURRENT_RADIUS);
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!("ribbon vertices") };
    assert!((Vec3::from_array(positions[0]).distance(Vec3::from_array(positions[1])) - 4.0 * CURRENT_RADIUS).abs() < 0.001, "the ribbon is as wide as the lane");
    current.set(None, None, Some(false));
    assert!(!current.is_on() && current.force(inside) == Vec3::ZERO, "off: no push");
    current.set(None, None, Some(true));
    assert_eq!(current.force(inside), Vec3::NEG_Z * CURRENT_FORCE * 2.0, "back on with the settings it had");
    let retail = Current::new(path);
    assert_eq!((retail.name(), retail.is_on()), ("", true));
}

fn ship_hull(x: f32, z: f32) -> avoid::Body {
    avoid::Body::hull([Vec3::new(-2.0, -1.0, -6.0), Vec3::new(2.0, 2.0, 6.0)], &Transform::from_xyz(x, 0.0, z))
}

/// Swims a whale (radius 5) at 12 u/s along +X from the origin at `height` past `bodies`, in 0.1 s steps, and returns every position.
fn swim(bodies: &[avoid::Body], height: f32) -> Vec<Vec3> {
    let mut animal = SpaceAnimal { speed: 12.0, ..SpaceAnimal::new("Whales", 5.0, Vec3::ZERO, 0, true) };
    let mut tf = Transform::from_xyz(0.0, height, 0.0);
    animal.follow(&[Vec3::new(0.0, height, 0.0), Vec3::new(600.0, height, 0.0)], tf.translation, false, false);
    (0..400)
        .map(|_| {
            animal.advance_around(&mut tf, 0.1, bodies);
            tf.translation
        })
        .collect()
}

fn hull_distance(b: &avoid::Body, p: Vec3) -> f32 {
    b.nearest(p.xz()).1
}

#[test]
fn a_whale_steers_round_a_hull_on_its_path_and_rejoins_the_path() {
    // A ship lying across the way, 100 ahead.
    let hull = ship_hull(100.0, 0.0);
    let plain = swim(&[], 0.0);
    assert!(plain.iter().any(|p| hull_distance(&hull, *p) == 0.0), "without avoidance it passes through the hull");
    let track = swim(&[hull], 0.0);
    let nearest = track.iter().map(|p| hull_distance(&hull, *p)).fold(f32::MAX, f32::min);
    assert!(nearest > 2.5, "never inside the body: {nearest}");
    let widest = track.iter().map(|p| p.z.abs()).fold(0.0, f32::max);
    assert!(widest < 25.0, "a small sidestep, not a detour: {widest}");
    let past = track.iter().find(|p| p.x > 150.0).unwrap();
    assert!(past.z.abs() < 8.0, "back near the path past the hull: {past}");
    assert!(track.last().unwrap().x > 300.0 && track.last().unwrap().z.abs() < 1.0, "and on along it");
}

#[test]
fn a_whale_does_not_swerve_for_a_hull_that_clears_it_or_is_at_another_height() {
    let plain = swim(&[], 0.0);
    // The hull's near side is 14 - 2 = 12 off the path, past radius 5 + margin 1.5.
    assert_eq!(swim(&[ship_hull(100.0, 14.0)], 0.0), plain, "a gap that leaves room is left alone");
    // Mission 2 flies its whales at 36 above ships at 0.
    assert!(swim(&[ship_hull(100.0, 0.0)], 36.0).iter().all(|p| p.z == 0.0), "flown over");
}

#[test]
fn a_whale_goes_between_two_hulls_with_room_and_a_hull_on_top_of_it_pushes_it_out() {
    // Gap 20 between the near sides; the whale needs 2 x 6.5 = 13.
    let (a, b) = (ship_hull(100.0, 12.0), ship_hull(100.0, -12.0));
    let track = swim(&[a, b], 0.0);
    assert!(track.iter().all(|p| hull_distance(&a, *p) > 2.5 && hull_distance(&b, *p) > 2.5));
    assert!(track.last().unwrap().x > 300.0, "through and on");
    let hull = ship_hull(100.0, 0.0);
    let out = avoid::push_out(Vec3::new(100.0, 0.0, 0.5), 5.0, &[hull]);
    assert!(hull_distance(&hull, out) >= 2.5 - 1e-3, "{out}");
}
