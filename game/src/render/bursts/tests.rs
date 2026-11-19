use super::*;

#[test]
fn rules_parse_their_parts_with_defaults_per_kind() {
    let b = parse(&serde_json::json!({
        "effects": [{"match": "_Impact", "sparks": {"count": 20}, "smoke": {}, "shake": 0.5}, {"match": "FireOnShip", "embers": {}, "every": 0.2}],
        "scorch": {"seconds": 4},
    }))
    .unwrap();
    let hit = &b.rules[b.rule("CannonBall_Heavy_Impact").unwrap()];
    assert_eq!(hit.parts.iter().map(|(k, _)| *k).collect::<Vec<_>>(), [Kind::Spark, Kind::Smoke]);
    assert_eq!((hit.parts[0].1.count, hit.parts[0].1.drag), (20, 3.0), "unset keys come from the kind's defaults");
    assert_eq!(hit.shake, 0.5);
    assert_eq!(b.rules[b.rule("FireOnShip").unwrap()].every, 0.2);
    assert_eq!(b.scorch.map(|s| s.seconds), Some(4.0));
    assert!(b.motes.is_none() && b.rule("Engine_Navy").is_none());
    assert!(parse(&Value::Null).is_none());
}

#[test]
fn a_flame_replaces_the_sprite_stays_at_the_source_and_leaves_smoke_when_extinguished() {
    use bevy::ecs::system::RunSystemOnce;
    use serde_json::json;
    let mut world = World::new();
    world.init_resource::<Messages<EffectStarted>>();
    world.init_resource::<Particles>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(FxLib::with_names(&["FireOnShip"]));
    world.insert_resource(Look::parse(&json!({"bursts": {"effects": [
        {"match": "FireOnShip", "flame": {"width": 4, "height": 5}, "smoke": {"count": 1}, "every": 0.25}
    ]}})));
    let fire = world
        .run_system_once(|mut c: Commands, lib: Res<FxLib>| {
            let host = c.spawn(Transform::default()).id();
            let at = Transform::from_xyz(1.0, 2.0, 3.0);
            crate::fx::spawn(&mut c, &lib, "FireOnShip", Some(host), at, at, false).unwrap()
        })
        .unwrap();
    world.run_system_once(start).unwrap();
    assert!(world.get::<HiddenFx>(fire).is_some());
    let f = *world.get::<FlameSource>(fire).unwrap();
    let parts = &world.resource::<Particles>().list;
    assert_eq!(parts[0].from, Vec3::new(1.0, 6.0, 3.0), "smoke starts above the visible flame base");
    assert!(parts[0].host.is_none(), "released smoke drifts free");

    let mut b = QuadBatch::with_normals();
    let cam = GlobalTransform::IDENTITY;
    let moved = Vec3::new(9.0, 3.0, 4.0);
    flame_quad(&mut b, &f, FlameSource::strength(world.get::<Fx>(fire).unwrap()), moved, &cam, 2);
    assert_eq!(b.pos[0], [7.0, 3.0, 4.25]);
    assert_eq!(b.pos[2], [7.0, 8.0, 4.25]);
    assert_eq!(b.uv[0], [32.0, 4.0]);
    world.get_mut::<Fx>(fire).unwrap().emitting = false;
    world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.5));
    world.run_system_once(emitters).unwrap();
    b.clear();
    flame_quad(&mut b, &f, FlameSource::strength(world.get::<Fx>(fire).unwrap()), moved, &cam, 2);
    assert!(b.is_empty());
    assert_eq!(world.resource::<Particles>().list.len(), 1, "no new smoke, but the detached puff remains");
    assert!(FlameSource::parse(&json!({"height": 0})).is_none(), "a disabled flame leaves the original drawing alone");
}

#[test]
fn particles_slow_with_drag_rise_and_stay_in_their_cone() {
    let mut ps = Particles::default();
    let p = Part { count: 200, speed: 10.0, spread: 30f32.to_radians(), drag: 2.0, rise: 1.0, ..defaults(Kind::Spark) };
    ps.throw(Kind::Spark, &p, Vec3::ZERO, Vec3::X, 0.0, None);
    assert_eq!(ps.list.len(), 200);
    for q in &ps.list {
        assert!(q.vel.normalize().dot(Vec3::X) >= 30f32.to_radians().cos() - 1e-3, "inside the cone");
    }
    let q = Particle { vel: Vec3::X * 10.0, drag: 2.0, rise: 1.0, ..ps.list[0] };
    let (far, slow) = q.motion(10.0);
    assert!((far.x - 5.0).abs() < 1e-3, "drag stops it after speed / drag: {far}");
    assert!((far.y - 10.0).abs() < 1e-3 && slow.x.abs() < 1e-3, "while it keeps rising");
    let still = Particle { drag: 0.0, ..q };
    assert_eq!(still.motion(2.0).0, Vec3::new(20.0, 2.0, 0.0));
}

/// Spawns effects through `fx::spawn_with`, so `EffectStarted` carries what the real callers send, then runs `start`.
fn thrown(spawns: Vec<(&'static str, Transform, crate::fx::Origin)>) -> Vec<Particle> {
    use bevy::ecs::system::RunSystemOnce;
    let mut world = World::new();
    world.init_resource::<Messages<EffectStarted>>();
    world.init_resource::<Particles>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(FxLib::with_names(&["Gun_Muzzle_Flash", "Gun_Impact", "Boarding_Fight", "Special"]));
    world.insert_resource(Look::parse(&serde_json::json!({"bursts": {"effects": [
        {"match": "muzzle", "sparks": {"count": 60, "spread": 20}},
        {"match": "_impact", "surface_debris": true, "sparks": {"count": 60, "spread": 30}},
        {"match": "special", "sparks": {"count": 1}},
    ], "impact_materials": [
        {"match": ["Wood", "Reinforced"], "splinters": {"count": 2}, "smoke": {"count": 1}},
        {"match": ["Iron"], "sparks": {"count": 3, "spread": 27}},
        {"match": ["Cloth"], "fibers": {"count": 4}}
    ]}})));
    world
        .run_system_once(move |mut commands: Commands, lib: Res<FxLib>| {
            for (name, tf, origin) in &spawns {
                crate::fx::spawn_with(&mut commands, &lib, name, None, *tf, *tf, true, *origin);
            }
        })
        .unwrap();
    world.run_system_once(start).unwrap();
    world.resource::<Particles>().list.clone()
}

fn inside(list: &[Particle], axis: Vec3, degrees: f32) -> bool {
    list.iter().all(|p| p.vel.normalize().dot(axis.normalize()) >= degrees.to_radians().cos() - 1e-3)
}

#[test]
fn a_muzzle_flash_throws_along_the_shot_not_its_mounts_axes() {
    let mount = Transform::from_xyz(5.0, 0.0, 0.0).looking_to(Vec3::NEG_Z, Vec3::Y);
    let shot = Vec3::new(1.0, 0.2, 0.0).normalize();
    let origin = crate::fx::Origin { forward: Some(shot), shell: Some(Entity::from_raw_u32(7).unwrap()), ..default() };
    let list = thrown(vec![("Gun_Muzzle_Flash", mount, origin)]);
    assert_eq!(list.len(), 60);
    assert!(inside(&list, shot, 20.0), "inside the cone about the shot's velocity");
    assert!(!inside(&list, Vec3::NEG_Z, 20.0), "and not about the mount");
    assert!(list.iter().all(|p| p.from == mount.translation));
}

#[test]
fn an_impact_burst_spreads_about_the_surface_normal() {
    let normal = Vec3::new(0.0, 1.0, 0.5).normalize();
    // the effect node faces back along the flight, 90 degrees from the surface normal here
    let node = Transform::from_xyz(0.0, 3.0, 0.0).looking_to(Vec3::X, Vec3::Y);
    let origin = crate::fx::Origin { forward: Some(normal), host: Some(Entity::from_raw_u32(9).unwrap()), ..default() };
    let list = thrown(vec![("Gun_Impact", node, origin)]);
    assert_eq!(list.len(), 60);
    assert!(inside(&list, normal, 30.0));
    assert!(!inside(&list, Vec3::X, 30.0));
}

#[test]
fn material_debris_replaces_generic_particles_only_for_opted_in_hits() {
    let tf = Transform::from_xyz(1.0, 0.0, 1.0);
    let launch = |name, material| thrown(vec![(name, tf, crate::fx::Origin { material, forward: Some(Vec3::X), ..default() })]);
    for (material, kind, count) in [("Wood", Kind::Splinter, 2), ("Reinforced", Kind::Splinter, 2), ("Iron", Kind::Spark, 3), ("Cloth", Kind::Fiber, 4)] {
        let list = launch("Gun_Impact", Some(material));
        assert_eq!(list.len(), count + usize::from(kind == Kind::Splinter));
        assert_eq!(list.iter().filter(|p| p.kind == kind).count(), count);
        assert!(list.iter().all(|p| p.from == tf.translation));
        if kind == Kind::Spark {
            assert!(inside(&list, Vec3::X, 27.0));
        }
    }
    assert_eq!(launch("Gun_Impact", Some("Stone")).len(), 60, "unconfigured materials keep the generic burst");
    assert_eq!(launch("Gun_Muzzle_Flash", None).len(), 60, "muzzle flashes are unaffected");
    assert_eq!(launch("Special", Some("Wood"))[0].kind, Kind::Spark, "a weapon can retain its own burst");
}

#[test]
fn an_effect_without_shell_or_normal_keeps_its_nodes_forward() {
    let node = Transform::from_xyz(1.0, 2.0, 3.0).looking_to(Vec3::new(-1.0, 0.0, 1.0), Vec3::Y);
    let list = thrown(vec![("Gun_Impact", node, crate::fx::Origin::default()), ("Boarding_Fight", node, crate::fx::Origin::default())]);
    assert_eq!(list.len(), 60, "only the effect with a rule throws");
    assert!(inside(&list, node.forward().as_vec3(), 30.0));
}
#[test]
fn quads_carry_kind_seed_and_age_and_motes_fill_the_box() {
    let cam = GlobalTransform::from(Transform::from_xyz(0.0, 50.0, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z));
    let mut ps = Particles::default();
    ps.throw(Kind::Ember, &defaults(Kind::Ember), Vec3::ZERO, Vec3::Y, 0.0, None);
    let mut b = QuadBatch::with_normals();
    let p = ps.list[0];
    place(&mut b, &p, Vec3::ZERO, p.life * 0.5, &cam);
    assert_eq!((b.pos.len(), b.idx.len()), (4, 6));
    assert!(b.uv.iter().all(|uv| (uv[0] / 4.0).floor() == Kind::Ember as u32 as f32 && (uv[1] / 2.0).floor() == p.seed as f32));
    assert!((b.col[0][3] - 0.5).abs() < 1e-4, "alpha is the age share");
    let mut m = QuadBatch::with_normals();
    motes(&mut m, &defaults(Kind::Mote), Vec3::new(1000.0, 5.0, -300.0), 12.0, &cam);
    assert_eq!(m.pos.len(), 4 * 300);
    assert!(m.pos.iter().all(|p| (Vec3::from(*p) - Vec3::new(1000.0, 5.0, -300.0)).abs().max_element() <= 121.0), "around the camera");
}
