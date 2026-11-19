use super::super::draw::particle_sample;
use super::super::lib::{Curve, parse_prim};
use super::super::spawn;
use super::*;
use crate::assets_dir;
use serde_json::Value;

#[test]
#[ignore = "needs imported assets"]
fn meteor_shower_spreads_above_the_emitter_and_falls_vertically() {
    let doc: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("fx/MeteorStorm.json")).unwrap()).unwrap();
    let gt = GlobalTransform::from(Transform::from_xyz(30.0, 20.0, -40.0));
    let mut rng = FxRng(12345);
    for (i, data) in doc["Primitive"].as_array().unwrap().iter().enumerate() {
        let prim = parse_prim(data, &mut vec![]);
        for _ in 0..64 {
            let particle = spawn_particle(&prim, 0.0, &gt, &mut rng, 1.0);
            if i < 2 {
                assert!(particle.pos.y > gt.translation().y + 99.0);
                assert!(particle.vel.y <= -50.0);
                assert!(Vec2::new(particle.vel.x, particle.vel.z).length() < 1e-5);
                assert!(particle_sample(&particle, TICK).0.y < particle.pos.y);
                assert!(prim.along_velocity);
            } else {
                assert_eq!(particle.vel, Vec3::ZERO, "splash motion must not inherit the position cone");
            }
        }
    }
}

#[test]
#[ignore = "needs imported assets"]
fn meteor_streaks_end_at_the_splash_plane_at_their_authored_lifetime() {
    let doc: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("fx/MeteorStorm.json")).unwrap()).unwrap();
    let mut rng = FxRng(13);
    for surface in [0.0, 25.0] {
        let gt = GlobalTransform::from(Transform::from_xyz(10.0, surface, 20.0));
        for (i, data) in doc["Primitive"].as_array().unwrap().iter().enumerate() {
            let prim = parse_prim(data, &mut vec![]);
            for _ in 0..100 {
                let mut p = spawn_particle(&prim, 0.0, &gt, &mut rng, 1.0);
                let horizontal = p.pos.with_y(0.0);
                align_meteor_particle(&mut p, surface);
                assert_eq!(p.pos.with_y(0.0), horizontal);
                assert!((p.pos.y + p.vel.y.min(0.0) * p.life - surface).abs() < 0.001);
                if i < 2 {
                    assert!(p.pos.y > surface);
                    assert!(p.pos.y + p.vel.y * (p.life - 0.01) > surface);
                } else {
                    assert_eq!(p.pos.y, surface, "splashes begin at flight height");
                }
            }
        }
    }
}

#[test]
#[ignore = "needs imported assets"]
fn angle_velocity_uses_its_own_emitter_or_the_position_when_requested() {
    let doc: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("fx/CannonBall_Light_Muzzle.json")).unwrap()).unwrap();
    let mut prim = parse_prim(&doc["Primitive"][0], &mut vec![]);
    let mut rng = FxRng(12345);
    let particle = spawn_particle(&prim, 0.0, &GlobalTransform::IDENTITY, &mut rng, 1.0);
    assert!(particle.pos.length() > 0.0);
    assert_eq!(particle.vel, particle.pos);
    prim.angle_pos_velocity = false;
    prim.vel_ang_dir = [Curve::default(), Curve::default(), Curve(vec![(0.0, -1.0)], None, false)];
    prim.vel_ang_len = Curve(vec![(0.0, 12.0)], None, false);
    prim.vel_ang_len_err = Curve::default();
    prim.vel_ang_min = Curve::default();
    prim.vel_ang_max = Curve::default();
    let particle = spawn_particle(&prim, 0.0, &GlobalTransform::IDENTITY, &mut rng, 1.0);
    assert_eq!(particle.vel, Vec3::NEG_Y * 12.0);
}

#[test]
fn catch_up_emission_is_spread_along_the_frames_motion() {
    let previous = Transform::default();
    let current = Transform::from_xyz(3.0, 0.0, 0.0).with_rotation(Quat::from_rotation_y(0.3));
    for (remainder, x) in [(0.2, 1.0), (0.1, 2.0), (0.0, 3.0)] {
        let at = emitter_transform(previous, current, remainder, 0.3);
        assert!((at.translation.x - x).abs() < 1e-5);
        assert!((at.rotation.angle_between(previous.rotation) - x * 0.1).abs() < 1e-4);
    }
    assert_eq!(emitter_transform(previous, current, 0.0, 0.0), current);
}

#[test]
#[ignore = "needs imported assets"]
fn sos_beacon_keeps_its_fixed_particles_and_pulses_until_released() {
    let mut app = App::new();
    app.init_resource::<Time>().init_resource::<FxLib>().insert_resource(FxRng(123)).add_systems(Update, update_fx);
    let beacon = app.world_mut().resource_scope(|world, lib: Mut<FxLib>| {
        for p in &lib.list[lib.effects["BottleRocket_Light_Impact"]].prims {
            assert!(p.fallback_life.is_infinite());
            assert!(assets_dir().join(format!("fx/tex/{}.png", lib.tex_names[p.tex.unwrap()])).is_file());
            assert!((p.size.at(0.6, 0.0) - p.size.at(50.6, 0.0)).abs() < 0.001);
        }
        spawn(&mut world.commands(), &lib, "BottleRocket_Light_Impact", None, Transform::default(), Transform::default(), false).unwrap()
    });
    app.world_mut().flush();
    for _ in 0..1800 {
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(TICK));
        app.update();
        assert_eq!(app.world().get::<Fx>(beacon).unwrap().particle_count(), 9, "SOS beacon must remain visible without accumulating particles");
    }
    app.world_mut().despawn(beacon);
    app.update();
    assert!(app.world().get::<Fx>(beacon).is_none());
}

#[test]
#[ignore = "needs imported assets"]
fn docking_marker_particles_are_bounded_over_a_long_mission() {
    let mut app = App::new();
    app.init_resource::<Time>().init_resource::<FxLib>().insert_resource(FxRng(123)).add_systems(Update, update_fx);
    let marker = app
        .world_mut()
        .resource_scope(|world, lib: Mut<FxLib>| spawn(&mut world.commands(), &lib, "Docking_Point", None, Transform::default(), Transform::default(), false).unwrap());
    app.world_mut().flush();
    for i in 0..9000 {
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(TICK));
        app.update();
        if i >= 899 && i % 900 == 899 {
            let fx = app.world().get::<Fx>(marker).unwrap();
            assert!(fx.particle_count() <= 4000, "docking marker grew to {} particles after {} ticks", fx.particle_count(), i + 1);
        }
    }
}

#[test]
#[ignore = "needs imported assets"]
fn mission_effect_particle_counts_do_not_grow_with_elapsed_time() {
    let mut app = App::new();
    app.init_resource::<Time>().init_resource::<FxLib>().insert_resource(FxRng(123)).add_systems(Update, update_fx);
    app.world_mut().resource_scope(|world, lib: Mut<FxLib>| {
        let mut commands = world.commands();
        for name in ["NavyEngine_Light", "Torpedo_Trail", "Damaged Section_Heavy", "Docking_Point"] {
            spawn(&mut commands, &lib, name, None, Transform::default(), Transform::default(), false).unwrap();
        }
    });
    app.world_mut().flush();
    let mut counts = vec![];
    for i in 0..1800 {
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(TICK));
        app.update();
        if i == 899 || i == 1799 {
            let total: usize = app.world_mut().query::<&Fx>().iter(app.world()).flat_map(|f| &f.prims).map(|p| p.parts.len()).sum();
            counts.push(total);
        }
    }
    // 20%: thinned smoke spawns by chance (a fractional rate per tick), so the two counts differ by a few percent either way
    assert!(counts[0] > 0 && counts[1] <= counts[0] * 6 / 5, "particle counts at 30/60 seconds: {counts:?}");
}

#[test]
fn detail_scales_spawn_rate_by_camera_distance() {
    assert_eq!(detail_factor(0.1, 0.0), 1.0);
    assert!((detail_factor(0.1, 175.0) - 0.55).abs() < 1e-6);
    assert!((detail_factor(0.1, 1000.0) - 0.1).abs() < 1e-6);
}

#[test]
fn scene_budget_only_bites_above_the_budget() {
    assert_eq!(scene_budget_factor(0), 1.0);
    assert_eq!(scene_budget_factor(15_000), 1.0);
    assert!((scene_budget_factor(30_000) - 0.5).abs() < 1e-6);
}
