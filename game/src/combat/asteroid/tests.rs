use super::*;

#[test]
fn each_rock_follows_the_belt_path_shifted_by_its_own_offset_and_height() {
    // A path along +x (Bevy: -z is forward, x is starboard); a rock 6 units to one side of it and 3 up.
    let path = [Vec3::ZERO, Vec3::X * 50.0, Vec3::X * 100.0];
    let own = rock_path(&path, Vec3::new(40.0, 3.0, 6.0));
    for (p, q) in path.iter().zip(&own) {
        assert!((*q - *p - Vec3::new(0.0, 3.0, 6.0)).length() < 1e-4, "{q} vs {p}");
    }
    // The other side shifts the other way: two rocks never share their points.
    assert!((rock_path(&path, Vec3::new(40.0, 0.0, -6.0))[1] - Vec3::new(50.0, 0.0, -6.0)).length() < 1e-4);
}

#[test]
fn find_closest_point_skips_points_behind_the_rock() {
    let path = [Vec3::ZERO, Vec3::X * 10.0, Vec3::X * 20.0, Vec3::X * 30.0];
    // Nearest is point 1 (behind), but only points ahead toward the end count.
    assert_eq!(closest_point(&path, Vec3::new(13.0, 0.0, 0.0)), 2);
    assert_eq!(closest_point(&path, Vec3::new(-5.0, 0.0, 0.0)), 0);
}

#[test]
#[ignore = "needs imported assets"]
fn asteroid_tumble_obeys_group_ranges_without_drift() {
    let mut app = App::new();
    app.init_resource::<Time>().init_resource::<AsteroidDefs>().insert_resource(Rng(123)).add_message::<AsteroidSetup>().add_systems(Update, asteroid_motion);
    let defs = app.world().resource::<AsteroidDefs>();
    let a = Asteroid::new("Asteroid_Large", "Asteroid Belt", defs).unwrap();
    let b = Asteroid::new("Asteroid_Med", "Montressor Asteroids", defs).unwrap();
    let a = app.world_mut().spawn((a, Transform::from_xyz(20.0, 0.0, 0.0))).id();
    let b = app.world_mut().spawn((b, Transform::default())).id();
    for (entity, upper) in [(a, 0.9), (b, 0.75)] {
        app.world_mut().write_message(AsteroidSetup { units: vec![entity], speed: [0.0, 0.0], tumble: [0.5, upper], path: vec![], closest: false, mode: PathMode::ToEnd });
    }
    app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.1));
    app.update();
    for (entity, upper) in [(a, 0.9), (b, 0.75)] {
        let rock = app.world().get::<Asteroid>(entity).unwrap();
        assert!((0.5..=upper).contains(&rock.tumble));
        assert!((rock.axis.length() - 1.0).abs() < 1e-5);
        let tf = app.world().get::<Transform>(entity).unwrap();
        assert!((tf.rotation.angle_between(Quat::IDENTITY) - rock.tumble * 0.1).abs() < 1e-4);
        assert_eq!(tf.translation, if entity == a { Vec3::new(20.0, 0.0, 0.0) } else { Vec3::ZERO });
    }
}

const DT: f32 = 1.0 / 30.0;

fn rock(acceleration: f32) -> Asteroid {
    Asteroid {
        kind: "Asteroid_Med".into(),
        group: "rocks".into(),
        id: -1,
        radius: 1.0,
        mass: 20000.0,
        hp: 30.0,
        velocity: Vec3::ZERO,
        speed: 0.0,
        acceleration,
        thrust: Vec3::ZERO,
        axis: Vec3::X,
        tumble: 0.0,
        path: vec![],
        next: 0,
        mode: PathMode::ToEnd,
        forward: true,
    }
}

/// One tick of the rock's controller then physics, as `asteroid_motion` runs it.
fn tick(r: &mut Asteroid, pos: &mut Vec3) {
    control(pos, &mut r.thrust, &r.path, &mut r.next, &mut r.forward, r.mode);
    physics(pos, &mut r.velocity, r.thrust, r.acceleration, r.speed, DT);
}

#[test]
fn a_rock_accelerates_to_its_speed_cap_and_does_not_slow_on_approach() {
    // Medium rock: acceleration 100 per second per second is 100/30 per tick; cap 10 is reached on the third tick.
    let mut r = rock(100.0);
    (r.speed, r.path) = (10.0, vec![Vec3::X * 1000.0, Vec3::X * 2000.0]);
    let mut pos = Vec3::ZERO;
    let mut speeds = vec![];
    for _ in 0..4 {
        tick(&mut r, &mut pos);
        speeds.push(r.velocity.length());
    }
    for (got, want) in speeds.iter().zip([100.0 / 30.0, 200.0 / 30.0, 10.0, 10.0]) {
        assert!((got - want).abs() < 1e-4, "{speeds:?}");
    }
    // The position moves by the velocity the rock had at the start of the tick: 0, then 1/9 of a unit.
    let mut r = rock(100.0);
    (r.speed, r.path) = (10.0, vec![Vec3::X * 1000.0, Vec3::X * 2000.0]);
    let mut pos = Vec3::ZERO;
    tick(&mut r, &mut pos);
    assert_eq!(pos, Vec3::ZERO);
    tick(&mut r, &mut pos);
    assert!((pos.x - 100.0 / 30.0 / 30.0).abs() < 1e-5);
    // Close to the point it is still at full speed (no braking).
    let mut r = rock(500.0);
    (r.speed, r.path, r.velocity) = (10.0, vec![Vec3::X * 100.0, Vec3::X * 200.0], Vec3::X * 10.0);
    let mut pos = Vec3::X * 99.0;
    tick(&mut r, &mut pos);
    assert!((r.velocity.length() - 10.0).abs() < 1e-5);
}

#[test]
fn a_small_rock_turns_in_a_few_ticks_not_at_once() {
    // Small: 500/30 = 16.7 per tick against a cap of 5..20: a rock flying +x told to go +z first keeps part of its old velocity.
    let mut r = rock(100.0);
    (r.speed, r.velocity, r.path) = (10.0, Vec3::X * 10.0, vec![Vec3::Z * 1000.0, Vec3::Z * 2000.0]);
    let mut pos = Vec3::ZERO;
    tick(&mut r, &mut pos);
    assert!(r.velocity.x > 5.0 && r.velocity.z > 3.0 && r.velocity.length() <= 10.0 + 1e-4, "{:?}", r.velocity);
}

#[test]
fn a_point_is_reached_when_the_rock_has_crossed_the_line_through_it() {
    // Path along +x, then a corner at (100,0,0) turning to +z.
    let path = [Vec3::ZERO, Vec3::X * 100.0, Vec3::new(100.0, 0.0, 100.0)];
    // Corner point: the line through it runs along the bisector, direction (1,0,-1) in this frame.
    assert!(!crossed(&path, 1, Vec3::new(99.0, 0.0, 0.0), true), "short of the line");
    assert!(crossed(&path, 1, Vec3::new(100.5, 0.0, 0.0), true), "past the line");
    assert!(crossed(&path, 1, Vec3::new(60.0, 0.0, 60.0), true), "far away on the far side counts");
    assert!(!crossed(&path, 1, Vec3::new(140.0, 0.0, -60.0), true), "far away on the near side does not");
    assert!(!crossed(&path, 1, path[1], true), "exactly on the point is not across it");
    // End points use the line perpendicular to the segment.
    assert!(!crossed(&path, 2, Vec3::new(100.0, 0.0, 99.0), true));
    assert!(crossed(&path, 2, Vec3::new(30.0, 0.0, 101.0), true));
    assert!(crossed(&path, 0, Vec3::new(0.5, 0.0, 40.0), true));
    // Height is ignored.
    assert!(crossed(&path, 2, Vec3::new(100.0, 50.0, 101.0), true));
    // A single point or a missing one is never reached.
    assert!(!crossed(&path[..1], 0, Vec3::splat(5.0), true) && !crossed(&path, 7, Vec3::splat(5.0), true));
}

#[test]
fn crossing_a_point_moves_on_and_a_rock_does_not_need_to_get_within_a_step() {
    let mut r = rock(100.0);
    (r.speed, r.path) = (10.0, vec![Vec3::ZERO, Vec3::X * 100.0, Vec3::X * 200.0]);
    let mut pos = Vec3::new(130.0, 0.0, 40.0); // 40 off the line, past point 1's line
    r.next = 1;
    tick(&mut r, &mut pos);
    assert_eq!(r.next, 2);
    assert!(r.thrust.x > 0.0 && r.thrust.z < 0.0, "now aimed at the next point");
}

#[test]
fn the_follow_modes_at_the_last_point() {
    let path = vec![Vec3::ZERO, Vec3::X * 100.0, Vec3::X * 200.0];
    let past_end = Vec3::new(201.0, 0.0, 0.0);
    // Loop: back to the first point, flying straight there, no jump.
    let mut r = rock(100.0);
    (r.speed, r.path, r.next, r.mode) = (10.0, path.clone(), 2, PathMode::Loop);
    let mut pos = past_end;
    control(&mut pos, &mut r.thrust, &r.path, &mut r.next, &mut r.forward, r.mode);
    assert_eq!((r.next, pos), (0, past_end));
    assert!(r.thrust.x < -0.99);
    // Teleport loop: the rock jumps to the first point, its velocity is untouched.
    let mut r = rock(100.0);
    (r.speed, r.velocity, r.path, r.next, r.mode) = (10.0, Vec3::X * 7.0, path.clone(), 2, PathMode::Teleport);
    let mut pos = past_end;
    tick(&mut r, &mut pos);
    assert_eq!((r.next, r.velocity.x > 6.9), (0, true));
    assert!(pos.distance(Vec3::X * 7.0 * DT) < 1e-4, "{pos}");
    // To end: the last point is never reached; the rock keeps pushing at it, so it ends up hovering around it.
    let mut r = rock(100.0);
    (r.speed, r.path, r.next, r.mode) = (10.0, path, 2, PathMode::ToEnd);
    let mut pos = Vec3::new(150.0, 0.0, 0.0);
    let mut far = 0.0f32;
    for i in 0..600 {
        tick(&mut r, &mut pos);
        assert_eq!(r.next, 2);
        if i > 300 {
            far = far.max((pos - Vec3::X * 200.0).length());
        }
    }
    assert!(far < 1.0, "hovers within a few ticks of travel of the last point: {far}");
}

#[test]
fn a_rock_a_ship_touched_leaves_its_belt_and_keeps_a_quarter_of_the_centre_distance_per_second() {
    let mut r = rock(100.0);
    (r.speed, r.velocity, r.path, r.next) = (12.0, Vec3::X * 12.0, vec![Vec3::ZERO, Vec3::X * 500.0], 1);
    let mut pos = Vec3::new(20.0, 0.0, 0.0);
    knock_off(&mut r, pos, Vec3::ZERO);
    assert!(r.path.is_empty());
    assert!((r.velocity - Vec3::X * 5.0).length() < 1e-5 && (r.speed - 5.0).abs() < 1e-5);
    for _ in 0..90 {
        tick(&mut r, &mut pos);
        assert!((r.velocity - Vec3::X * 5.0).length() < 1e-4, "not slowed: {:?}", r.velocity);
    }
    assert!((pos.x - (20.0 + 5.0 * 3.0)).abs() < 0.2, "{pos}");
    // Centre distance 8 on a diagonal: a quarter of the offset vector.
    knock_off(&mut r, Vec3::new(0.0, 6.0, 8.0), Vec3::ZERO);
    assert!((r.velocity - Vec3::new(0.0, 1.5, 2.0)).length() < 1e-5);
}

#[test]
fn the_belt_system_runs_controller_then_physics() {
    let mut app = App::new();
    app.init_resource::<Time>().insert_resource(Rng(1)).add_message::<AsteroidSetup>().add_systems(Update, asteroid_motion);
    let mut r = rock(100.0);
    (r.speed, r.path) = (10.0, vec![Vec3::X * 1000.0, Vec3::X * 2000.0]);
    let e = app.world_mut().spawn((r, Transform::default())).id();
    for _ in 0..4 {
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(DT));
        app.update();
    }
    assert!((app.world().get::<Asteroid>(e).unwrap().velocity - Vec3::X * 10.0).length() < 1e-3);
}

#[test]
#[ignore = "needs imported assets"]
fn asteroid_setup_replaces_contact_drift_without_erasing_it_each_tick() {
    let mut app = App::new();
    app.init_resource::<Time>().init_resource::<AsteroidDefs>().insert_resource(Rng(123)).add_message::<AsteroidSetup>().add_systems(Update, asteroid_motion);
    let rock = Asteroid::new("Asteroid_Small", "rocks", app.world().resource::<AsteroidDefs>()).unwrap();
    let e = app.world_mut().spawn((rock, Transform::default())).id();
    app.world_mut().get_mut::<crate::collision::Drift>(e).unwrap().0 = Vec3::X * 6.0;
    app.update();
    assert_eq!(app.world().get::<crate::collision::Drift>(e).unwrap().0, Vec3::X * 6.0);
    app.world_mut().write_message(AsteroidSetup { units: vec![e], speed: [0.0, 0.0], tumble: [0.0, 0.0], path: vec![], closest: false, mode: PathMode::ToEnd });
    app.update();
    assert_eq!(app.world().get::<crate::collision::Drift>(e).unwrap().0, Vec3::ZERO);
}

#[test]
#[ignore = "needs imported assets"]
fn asteroid_breakup_is_once_only_recursive_and_cleans_target() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<WorldAsset>()
        .init_resource::<AsteroidDefs>()
        .init_resource::<crate::collision::Shapes>()
        .init_resource::<FxLib>()
        .init_resource::<Target>()
        .insert_resource(Rng(123))
        .add_message::<Destroyed>()
        .add_message::<Detonate>()
        .add_message::<Shoot>()
        .add_message::<Strike>()
        .add_message::<Sfx>()
        .add_systems(Update, (crate::tick_start, asteroid_deaths).chain());
    let shooter = app.world_mut().spawn_empty().id();
    let rock = Asteroid::new("Asteroid_Large", "Asteroid Belt", app.world().resource::<AsteroidDefs>()).unwrap();
    let parent = app.world_mut().spawn((rock, Transform::default())).id();
    app.world_mut().get_mut::<crate::collision::Drift>(parent).unwrap().0 = Vec3::X * 6.0;
    app.world_mut().resource_mut::<Target>().0 = Some(parent);
    let mut rock = app.world_mut().get_mut::<Asteroid>(parent).unwrap();
    assert!(!rock.damage(49.0));
    assert!(rock.damage(1.0));
    assert!(!rock.damage(1000.0), "multiple shells must not repeat death");
    app.world_mut().write_message(Destroyed { by: shooter, victim: parent });
    app.update();
    app.update(); // removal is applied at the start of the next tick
    assert!(app.world().get_entity(parent).is_err());
    assert_eq!(app.world().resource::<Target>().0, None);
    let children: Vec<_> = app
        .world_mut()
        .query::<(Entity, &Asteroid, &Transform)>()
        .iter(app.world())
        .map(|(e, a, t)| {
            assert_eq!((a.kind.as_str(), a.group.as_str(), a.hp), ("Asteroid_Med", "Asteroid Belt", 30.0));
            assert!((0.8..=0.8 + 2.2 / 3.0).contains(&(a.velocity - Vec3::X * 6.0).length()), "fragments inherit the parent's contact velocity");
            assert!((0.0..=std::f32::consts::FRAC_PI_2).contains(&a.tumble));
            assert!((t.translation.length() - 6.6).abs() < 1e-4);
            assert!(app.world().get::<crate::collision::Obstacle>(e).is_some());
            e
        })
        .collect();
    assert_eq!(children.len(), 2);
    assert_eq!(app.world_mut().query::<&fx::Fx>().iter(app.world()).count(), 1);
    app.update();
    assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 2);
    for child in children {
        assert!(app.world_mut().get_mut::<Asteroid>(child).unwrap().damage(30.0));
        app.world_mut().write_message(Destroyed { by: shooter, victim: child });
    }
    app.update();
    app.update(); // the dead mediums are destroyed at the start of this tick
    let small: Vec<_> = app
        .world_mut()
        .query::<(Entity, &Asteroid)>()
        .iter(app.world())
        .map(|(e, a)| {
            assert_eq!(a.hp, 10.0);
            e
        })
        .collect();
    assert_eq!(small.len(), 6);
    for child in small {
        assert!(app.world_mut().get_mut::<Asteroid>(child).unwrap().damage(10.0));
        app.world_mut().write_message(Destroyed { by: shooter, victim: child });
    }
    app.update();
    app.update();
    assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 0);
    assert_eq!(app.world_mut().query::<&crate::collision::Obstacle>().iter(app.world()).count(), 0);
}
