//! The retail particle simulation: emission, movement and attraction at the engine's 30 Hz tick, and the morphing effect
//! meshes. Nothing here draws a particle (see `draw.rs`).

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::collections::HashMap;

use super::lib::{Curve, FxLib, Prim, conv};
use super::{Fx, FxRng, P};

/// Engine tick (retail constant 0.033): particles spawn and advance once per tick; velocities are stored per second.
pub(super) const TICK: f32 = 1.0 / 30.0;
/// Movement infos (attractor etc.) only run once at least this much time has passed (retail constant 0.1).
pub(super) const MOVE_STEP: f32 = 0.1;

/// Retail: effect detail D falls linearly from 1 at the camera to 0 at EffectsLowestDetailDistance (350, retail
/// default profile; max detail 1.0); spawn count = rate x (D - D*L + L), L = the primitive's LowDetailParticleRateMultiply.
/// shortcut: the original's extra off-screen test (D = 0) is not modelled; no camera = full detail.
fn detail_factor(low: f32, dist: f32) -> f32 {
    let d = (1.0 - dist / LOWEST_DETAIL_DISTANCE).clamp(0.0, 1.0);
    d - d * low + low
}
const LOWEST_DETAIL_DISTANCE: f32 = 350.0;
/// Whole-scene live-particle budget (not retail): above it every effect emits proportionally fewer particles, so a long fight with many
/// Gattling tracers, damage smoke and impacts cannot pile up thousands of quads per frame. PROVISIONAL: sized for a mid-range GPU; the
/// per-primitive `MAX_LIVE` still applies. The count lags the emission by a frame.
const SCENE_LIVE_BUDGET: f32 = 15_000.0;
fn scene_budget_factor(live: usize) -> f32 {
    (SCENE_LIVE_BUDGET / live.max(1) as f32).min(1.0)
}

pub(super) fn update_fx(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<FxLib>,
    mut rng: ResMut<FxRng>,
    mut q: Query<(Entity, &mut Fx, &GlobalTransform)>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
) {
    let frame = time.delta_secs().min(0.25);
    let scene = scene_budget_factor(q.iter().map(|(_, f, _)| f.particle_count()).sum());
    let cam_pos = cam.single().ok().map(|c| c.translation());
    for (e, mut fx, gt) in &mut q {
        let fx = &mut *fx;
        fx.tick_acc += frame;
        let world = gt.compute_transform();
        let previous_world = fx.previous_world;
        fx.previous_world = world;
        let eff = &lib.list[fx.eff];
        let meteor = lib.effects.get("MeteorStorm") == Some(&fx.eff);
        let mut busy = true; // no tick this frame: nothing to decide
        while fx.tick_acc >= TICK {
            fx.tick_acc -= TICK;
            // Catch-up ticks emit along the travelled path, rather than in a clump at the frame's endpoint.
            let tick_world = GlobalTransform::from(emitter_transform(previous_world, world, fx.tick_acc, frame));
            fx.age += TICK;
            fx.move_acc += TICK;
            let move_dt = if fx.move_acc > MOVE_STEP { std::mem::take(&mut fx.move_acc) } else { 0.0 };
            let (age, scale, emitting) = (fx.age, fx.rate_scale * crate::menu::detail_scale() * scene, fx.emitting);
            busy = false;
            // A source emitter places particles at the positions of its source primitive's particles.
            let sources: HashMap<usize, Vec<Vec3>> =
                eff.prims.iter().filter_map(|p| p.src).map(|i| (i, fx.prims[i.min(fx.prims.len() - 1)].parts.iter().map(|p| p.pos).collect())).collect();
            for (prim, st) in eff.prims.iter().zip(fx.prims.iter_mut()) {
                let t = age - prim.delay;
                for p in &mut st.parts {
                    p.age += TICK;
                    p.pos += p.vel * TICK;
                    if move_dt > 0.0
                        && let Some((tg, k)) = &prim.attract
                    {
                        let tt = t.max(0.0);
                        let target = conv(Vec3::new(tg[0].at(tt, 0.0), tg[1].at(tt, 0.0), tg[2].at(tt, 0.0)));
                        let to = target - p.pos;
                        let d = to.length();
                        let (k0, k1, k2) = (k[0].at(tt, 0.0), k[1].at(tt, 0.0), k[2].at(tt, 0.0));
                        let strength = if d > 0.0 { k0 + (k1 + k2 / d) / d } else { k0 };
                        p.vel += to * strength * move_dt;
                    }
                }
                st.parts.retain(|p| p.age < p.life && eff.bound.is_none_or(|b| p.pos.length() <= b));
                if !prim.ever && t > prim.duration {
                    st.parts.clear();
                }
                if t >= 0.0 && emitting {
                    let live = prim.ever || t <= prim.duration;
                    let mut n = 0u32;
                    if !st.started {
                        st.started = true;
                        n += prim.start;
                    }
                    if live {
                        // Retail: rate is particles per tick; a fractional rate spawns one with that probability.
                        // A keys-only curve gives its value only on the tick that enters a new key interval, else 0.
                        let key = prim.rate.key_index(t);
                        let fire = !prim.rate.2 || st.last_key.replace(key) != Some(key);
                        let r = prim.rate.at(t, 0.0)
                            * f32::from(u8::from(fire))
                            * prim.budget
                            * scale
                            * detail_factor(prim.low_rate, cam_pos.map_or(0.0, |c| c.distance(gt.translation())));
                        n += if r >= 1.0 {
                            r as u32
                        } else if r > 0.0 && rng.f() < r {
                            1
                        } else {
                            0
                        };
                    }
                    for _ in 0..n {
                        let mut np = spawn_particle(prim, t, &tick_world, &mut rng, fx.vel_scale);
                        if meteor {
                            align_meteor_particle(&mut np, tick_world.translation().y);
                        }
                        // Decoded: each spawn gets a uniformly random vertex of the mesh added to its position (not area-weighted).
                        if prim.mesh_vertices && !fx.vertices.is_empty() {
                            np.pos += fx.vertices[(rng.f() * fx.vertices.len() as f32) as usize % fx.vertices.len()];
                        }
                        if let Some(si) = prim.src {
                            let pool = &sources[&si];
                            if !pool.is_empty() {
                                np.pos += pool[(rng.f() * pool.len() as f32) as usize % pool.len()];
                            }
                        }
                        st.parts.push(np);
                    }
                    busy |= live;
                }
                busy |= !st.parts.is_empty() || t < 0.0;
            }
        }
        if !busy && !eff.prims.iter().any(|p| p.ever) {
            commands.entity(e).despawn();
        }
    }
}

// PROVISIONAL: match each meteor's authored fall speed/lifetime to the storm's flight-plane splash surface.
fn align_meteor_particle(p: &mut P, surface: f32) {
    p.pos.y = surface - p.vel.y.min(0.0) * p.life;
}

fn emitter_transform(previous: Transform, current: Transform, remainder: f32, frame: f32) -> Transform {
    if frame <= 0.0 {
        return current;
    }
    let t = (1.0 - remainder / frame).clamp(0.0, 1.0);
    Transform {
        translation: previous.translation.lerp(current.translation, t),
        rotation: previous.rotation.slerp(current.rotation, t),
        scale: previous.scale.lerp(current.scale, t),
    }
}

fn spawn_particle(p: &Prim, t: f32, gt: &GlobalTransform, rng: &mut FxRng, vel_scale: f32) -> P {
    let at = |c: &[Curve; 3]| Vec3::new(c[0].at(t, 0.0), c[1].at(t, 0.0), c[2].at(t, 0.0));
    // Retail: the error is one-sided, uniform between 0 and err.
    let jitter = |len: f32, err: f32, rng: &mut FxRng| len + err * rng.f();
    // Retail: enabled target and angle emitters add independent position/velocity contributions.
    let emit = |tg: Vec3, r: f32, l: f32, le: f32, rng: &mut FxRng| (tg - rng.sphere().normalize_or_zero() * r).normalize_or_zero() * jitter(l, le, rng);
    let angle = |dir: &[Curve; 3], min: &Curve, max: &Curve, len: &Curve, err: &Curve, rng: &mut FxRng| {
        let dir = at(dir).normalize_or_zero();
        let dir = if dir == Vec3::ZERO { Vec3::Y } else { dir };
        let (lo, hi) = (min.at(t, 0.0).to_radians(), max.at(t, 0.0).to_radians());
        let a = lo + (hi - lo) * rng.f();
        let az = rng.f() * std::f32::consts::TAU;
        let o1 = dir.any_orthonormal_vector();
        let o2 = dir.cross(o1);
        let d = dir * a.cos() + (o1 * az.cos() + o2 * az.sin()) * a.sin();
        d * jitter(len.at(t, 0.0), err.at(t, 0.0), rng)
    };
    let (mut pos, mut vel) = (Vec3::ZERO, Vec3::ZERO);
    if p.target_on {
        let offset = emit(at(&p.pos_t), p.pos_r.at(t, 0.0), p.pos_l.at(t, 0.0), p.pos_le.at(t, 0.0), rng);
        pos += offset;
        // SPHERE_VELOCITY_EMITTER_LENGTH_MULTIPLY: the keeper's message scales the velocity emitter's length (engine flames).
        vel += if p.target_pos_velocity { offset } else { emit(at(&p.vel_t), p.vel_r.at(t, 0.0), p.vel_l.at(t, 0.0) * vel_scale, p.vel_le.at(t, 0.0) * vel_scale, rng) };
    }
    if p.angle_on {
        let offset = angle(&p.ang_dir, &p.ang_min, &p.ang_max, &p.ang_len, &p.ang_len_err, rng);
        pos += offset;
        vel += if p.angle_pos_velocity { offset } else { angle(&p.vel_ang_dir, &p.vel_ang_min, &p.vel_ang_max, &p.vel_ang_len, &p.vel_ang_len_err, rng) };
    }
    let (mut pos, mut vel) = (conv(pos), conv(vel));
    if p.world {
        pos = gt.transform_point(pos);
        vel = gt.affine().transform_vector3(vel);
    }
    let life = p.life.as_ref().map_or(p.fallback_life, |c| c.at(t, p.fallback_life)).max(0.05);
    P { pos, vel, age: 0.0, life }
}

/// Repair scaffolds use the authored source/destination topology and full deploy/hold/retract curve.
pub(super) fn render_mesh_fx(
    mut commands: Commands,
    lib: Res<FxLib>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut effects: Query<(Entity, &mut Fx, &GlobalTransform)>,
) {
    for (e, mut fx, gt) in &mut effects {
        for (i, prim) in lib.list[fx.eff].prims.iter().enumerate() {
            let Some(def) = &prim.mesh else { continue };
            let age = fx.age + fx.tick_acc - prim.delay;
            if age < 0.0 {
                continue;
            }
            let scale = fx.size_scale;
            let entry = fx.meshes[i].get_or_insert_with(|| {
                let mesh = meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
                let material = materials.add(StandardMaterial {
                    base_color_texture: Some(assets.load(GltfAssetLabel::Texture(0).from_asset(def.model.clone()))),
                    unlit: true,
                    cull_mode: None,
                    double_sided: true,
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                });
                commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default(), *gt, ChildOf(e), NoFrustumCulling));
                (mesh, material)
            });
            if let Some(mut mesh) = meshes.get_mut(&entry.0) {
                let weight = def.weight.at(age, 0.0).clamp(0.0, 1.0);
                let positions: Vec<_> = def.source.iter().zip(&def.destination).map(|(a, b)| (a.lerp(*b, weight) * scale).to_array()).collect();
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
                mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, def.uv.clone());
                mesh.insert_attribute(
                    Mesh::ATTRIBUTE_COLOR,
                    vec![[prim.rgb[0].at(age, 1.0), prim.rgb[1].at(age, 1.0), prim.rgb[2].at(age, 1.0), prim.alpha.at(age, 1.0)]; def.source.len()],
                );
                mesh.insert_indices(Indices::U32(def.indices.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
}
