use super::*;

#[test]
fn weather_selection_uses_global_pulses_and_ship_size_hit_rolls() {
    let mut timers = HashMap::new();
    assert!(!storm_due(&mut timers, "A", 0, 3.0, 1.0));
    assert!(!storm_due(&mut timers, "A", 0, 3.0, 1.0));
    assert!(storm_due(&mut timers, "A", 0, 3.0, 1.0));
    assert!(!storm_due(&mut timers, "B", 0, 3.0, 1.0));
    assert!(!storm_due(&mut timers, "A", 0, 3.0, 0.0));
    assert_eq!(meteor_hit_chance(8.75), 17.5 / 80.0);
    assert_eq!(meteor_hit_chance(0.0), 0.0);
    assert_eq!(meteor_hit_chance(100.0), 1.0);
    let poly = serde_json::json!([[-100.0, -100.0], [100.0, -100.0], [100.0, 100.0], [-100.0, 100.0]]);
    assert!(lightning_point(&poly, Vec3::ZERO, &[Vec3::ZERO], &mut || 0.5).is_none(), "bolts must not spawn beside a ship");
    let at = lightning_point(&poly, Vec3::X * 500.0, &[], &mut || 0.5).unwrap();
    assert_eq!(at, Vec3::X * 500.0);
    assert!(lightning_point(&serde_json::json!([]), Vec3::ZERO, &[], &mut || 0.5).is_none());
}

#[test]
fn captains_avoid_storms_but_not_plain_clouds() {
    let square = serde_json::json!([[100.0, 100.0], [300.0, 100.0], [300.0, 300.0], [100.0, 300.0]]);
    let storms = Storms(serde_json::json!([
        {"name": "s", "polygon": square, "storm": {"effect": "SolarStorm"}, "meteors": {"on": false}},
        {"name": "c", "polygon": square, "storm": {"effect": "EFFECT"}, "meteors": {"on": false}},
    ]));
    let mut shift = StormShift::default();
    shift.0.insert("s".into(), (Vec3::new(10.0, 0.0, 0.0), Vec3::ZERO));
    let circles = storm_circles(&storms, &shift);
    assert_eq!(circles.len(), 1, "the plain cloud is not a hazard");
    let (c, r) = circles[0];
    assert_eq!(c, Vec3::new(210.0, 0.0, -200.0), "centroid in bevy axes, moved with the storm");
    assert!((r - 100.0 * 2f32.sqrt()).abs() < 1e-3);
}

#[test]
fn nebula_slowdown_is_distance_over_radius_zero_at_the_centre() {
    assert_eq!(depth_factor(0.0, 500.0), 0.0);
    assert_eq!(depth_factor(250.0, 500.0), 0.5);
    assert_eq!(depth_factor(700.0, 500.0), 1.0);
    assert_eq!(depth_factor(10.0, 0.0), 1.0);
}
#[test]
#[ignore = "needs imported assets"]
fn a_bolt_that_finds_no_ship_is_announced_with_its_segment_and_changes_nothing_else() {
    let lib = crate::combat::Library::load(assets_dir());
    let mut app = App::new();
    app.insert_resource(lib)
        .init_resource::<Time>()
        .init_resource::<StormShift>()
        .insert_resource(WeatherDamage(serde_json::from_value(crate::data::read_json(&assets_dir().join("data/nebula.json"))).unwrap()))
        .insert_resource(Storms(serde_json::json!([{
            "name":"test storm", "polygon":[[-1000.0,-1000.0],[1000.0,-1000.0],[1000.0,1000.0],[-1000.0,1000.0]],
            "storm":{"lightning":true,"lightning_recharge":3.0,"winds":false,"points":[{"pos":[0.0,0.0,0.0],"magnitude":1000.0}]},
            "meteors":{"on":false}
        }])))
        .add_message::<crate::combat::Strike>()
        .add_message::<crate::audio::Sfx>()
        .add_message::<StormFlash>()
        .add_systems(Update, storms);
    let mut bolts = vec![];
    for _ in 0..300 {
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.1));
        app.update();
        bolts.extend(app.world_mut().resource_mut::<Messages<StormFlash>>().drain());
    }
    let bolts: Vec<_> = bolts.into_iter().filter(|f| f.from != f.to).collect();
    assert!(!bolts.is_empty(), "thirty seconds of storm with no ship: bolts still come");
    for f in &bolts {
        assert!(!f.hit && f.at == f.from.lerp(f.to, 0.5) && f.from.distance(f.to) >= LIGHTNING_MIN_LENGTH, "{f:?}");
    }
}

#[test]
#[ignore = "needs imported assets"]
fn thirty_seconds_in_a_storm_does_not_guarantee_lightning_damage() {
    let lib = crate::combat::Library::load(assets_dir());
    let class = lib.index("Ship_Navy_Sloop");
    let hp = crate::combat::Health::new(&lib.classes[class]);
    let ship = crate::Ship { heading: 0.0, ..crate::Ship::new(class, crate::Side::Player) };
    fn no_hits(mut hits: MessageReader<crate::combat::Strike>) {
        assert_eq!(hits.read().count(), 0, "a random bolt must actually intersect the ship before emitting damage");
    }
    let mut app = App::new();
    app.insert_resource(lib)
        .init_resource::<Time>()
        .init_resource::<StormShift>()
        .insert_resource(WeatherDamage(serde_json::from_value(crate::data::read_json(&assets_dir().join("data/nebula.json"))).unwrap()))
        .insert_resource(Storms(serde_json::json!([{
            "name":"test storm", "polygon":[[-1000.0,-1000.0],[1000.0,-1000.0],[1000.0,1000.0],[-1000.0,1000.0]],
            "storm":{"lightning":true,"lightning_recharge":3.0,"winds":false,"points":[{"pos":[0.0,0.0,0.0],"magnitude":1000.0}]},
            "meteors":{"on":false}
        }])))
        .add_message::<crate::combat::Strike>()
        .add_message::<crate::audio::Sfx>()
        .add_message::<StormFlash>()
        .add_systems(Update, (storms, no_hits).chain());
    app.world_mut().spawn((ship, hp, Transform::from_xyz(300.0, 0.0, 0.0), crate::collision::Drift::default()));
    for _ in 0..300 {
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.1));
        app.update();
    }
}

fn sim() -> Sim {
    let (base, ..) = crate::data::mesh_geometry(assets_dir(), "OvalNebula").expect("mesh");
    let j: Value = crate::data::read_json(&assets_dir().join("fx/Nebula.json"));
    Sim::new(&base, shift_params(&j["Primitive"][0]["CommonPropertiesChunk"]["MeshChunk"]), 5)
}

#[test]
#[ignore = "needs imported assets"]
fn shifting_stays_within_the_exe_limits() {
    let mut s = sim();
    assert!(s.v.iter().any(|v| v.near.len() == MAX_NEIGHBOURS) && s.v.iter().all(|v| v.near.len() <= MAX_NEIGHBOURS));
    for _ in 0..3000 {
        s.tick();
    }
    assert!(s.v.iter().all(|v| (1.0..=1.15).contains(&v.scale) && v.scale_vel.abs() <= s.p.constrain + 1e-6));
    assert!(s.v.iter().any(|v| v.scale > 1.001), "scales breathe");
    assert!(s.v.iter().all(|v| v.off.length() < 0.2));
}

fn emitter(group: &str) -> MeteorEmitter {
    MeteorEmitter { group: group.into(), next: 0, forward: true, started: false }
}

#[test]
fn a_rule_moves_the_storm_whose_object_is_in_its_group() {
    let mut app = App::new();
    app.add_message::<MeteorRoute>().init_resource::<MeteorPlans>().add_systems(Update, route_meteors);
    app.insert_resource(Storms(serde_json::json!([{ "name": "storm", "groups": ["stormgroup"] }, { "name": "other", "groups": [] }])));
    app.world_mut().write_message(MeteorRoute::Speed { group: "stormgroup".into(), speed: 10.0 });
    app.update();
    let plans = &app.world().resource::<MeteorPlans>().0;
    assert_eq!(plans["storm"].speed, 10.0);
    assert!(!plans.contains_key("other"));
}

#[test]
fn meteor_emitter_teleport_loops_along_its_path_at_its_speed() {
    let plan = MeteorPlan { path: vec![Vec3::ZERO, Vec3::X * 100.0, Vec3::X * 200.0], closest: false, mode: PathMode::Teleport, speed: 100.0 };
    let (mut m, mut pos) = (emitter("Meteor Storm 1"), Vec3::ZERO);
    let seen: Vec<f32> = (0..5)
        .map(|_| {
            step_meteor(&mut m, &mut pos, &plan, 1.0);
            pos.x
        })
        .collect();
    // starts at 0, reaches 100, then 200: arriving at the last point teleports back to 0 in the same tick, and it never stops
    assert_eq!(seen, vec![0.0, 100.0, 0.0, 100.0, 0.0]);
}

#[test]
fn meteor_emitter_starts_at_the_closest_point_and_idles_without_speed() {
    let mut plan = MeteorPlan { path: vec![Vec3::ZERO, Vec3::X * 100.0, Vec3::X * 200.0], closest: true, mode: PathMode::Teleport, speed: 0.0 };
    let (mut m, mut pos) = (emitter("Meteor Storm 2"), Vec3::X * 190.0);
    step_meteor(&mut m, &mut pos, &plan, 1.0);
    assert_eq!(pos, Vec3::X * 190.0, "zero speed does not move");
    plan.speed = 5.0;
    step_meteor(&mut m, &mut pos, &plan, 1.0);
    assert_eq!(m.next, 2, "closest start is the last point");
    assert!((pos.x - 195.0).abs() < 1e-4, "moves 5 units toward it, got {pos}");
}

#[test]
fn colour_curve_loops_with_the_last_key() {
    let c = [(0.0, 0.8), (1.0, 0.9), (2.5, 1.0), (3.7, 0.8)];
    assert!((looped(&c, 0.0) - 0.8).abs() < 1e-6 && (looped(&c, 3.7 + 2.5) - 1.0).abs() < 1e-5);
}

#[test]
fn a_cloud_whose_shell_the_profile_hides_keeps_running() {
    use std::time::Duration;
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<fx::EffectMaterial>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)))
        .add_systems(Update, update);
    app.world_mut().spawn((Camera3d::default(), GlobalTransform::default()));
    let shown = app.world_mut().spawn((Cloud::for_tests("Nebula", 100.0), GlobalTransform::default(), Visibility::default())).id();
    let hidden = app.world_mut().spawn((Cloud::for_tests("Nebula", 100.0), GlobalTransform::default(), Visibility::Hidden)).id();
    for _ in 0..12 {
        app.update();
    }
    let age = |e: Entity| app.world().entity(e).get::<Cloud>().unwrap().age();
    assert!(age(shown) > 0.5, "the sim runs ({})", age(shown));
    assert_eq!(age(hidden), age(shown), "hidden or not, the same ticks");
}

#[test]
fn a_fuel_burning_hull_is_not_slowed_by_a_draining_cloud() {
    let mut app = App::new();
    let mut fuel = crate::fixed_tests::synthetic_class();
    fuel.fuel = true;
    app.insert_resource(crate::data::Library {
        classes: vec![crate::fixed_tests::synthetic_class(), fuel],
        guns: default(),
        bullets: default(),
        names: default(),
        clips: default(),
    })
    .init_resource::<StormShift>()
    .insert_resource(Storms(serde_json::json!([{ "name": "murk", "polygon": [[-100.0, -100.0], [100.0, -100.0], [100.0, 100.0], [-100.0, 100.0]],
            "cloud": { "drain": true, "points": [{ "pos": [0.0, 0.0, 0.0], "magnitude": 100.0 }] } }])))
    .add_systems(Update, slowdown);
    let sail = app.world_mut().spawn((crate::Ship::new(0, crate::Side::Player), Transform::from_xyz(30.0, 0.0, 0.0))).id();
    let burner = app.world_mut().spawn((crate::Ship::new(1, crate::Side::Player), Transform::from_xyz(30.0, 0.0, 0.0))).id();
    app.update();
    assert!(app.world().get::<crate::Ship>(sail).unwrap().sail_energy < 0.5, "the sail hull loses the wind");
    assert_eq!(app.world().get::<crate::Ship>(burner).unwrap().sail_energy, 1.0, "the fuel hull does not");
}
