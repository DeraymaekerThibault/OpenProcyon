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
pub(super) use crate::TICK_SECS as TICK;
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
                        p.vel += attraction(target - p.pos, [k[0].at(tt, 0.0), k[1].at(tt, 0.0), k[2].at(tt, 0.0)]) * move_dt;
                    }
                }
                st.parts.retain(|p| p.age < p.life);
                if !prim.ever && t > prim.duration {
                    st.parts.clear();
                }
                if t >= 0.0 && emitting {
                    let live = prim.ever || t <= prim.duration;
                    // Retail: rate is particles per tick; the start count joins the first tick's rate, and the sum is scaled by
                    // the distance detail (so far effects also start with fewer). A fractional count spawns one with that probability.
                    // A keys-only curve gives its value only on the tick that enters a new key interval, else 0.
                    let mut r = if std::mem::replace(&mut st.started, true) { 0.0 } else { prim.start as f32 };
                    if live {
                        let key = prim.rate.key_index(t);
                        let fire = !prim.rate.2 || st.last_key.replace(key) != Some(key);
                        r += prim.rate.at(t, 0.0) * f32::from(u8::from(fire));
                    }
                    r *= scale * detail_factor(prim.low_rate, cam_pos.map_or(0.0, |c| c.distance(gt.translation())));
                    let n = if r >= 1.0 {
                        r as u32
                    } else if r > 0.0 && rng.f() < r {
                        1
                    } else {
                        0
                    };
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

/// Retail attractor (velocity change per second): k = Const + WithDist / d + WithDistSquared / d^2 along the UNIT direction to the
/// target, d its distance; nothing at d = 0. So a constant k is a constant acceleration of |k| units/s^2, whatever the distance.
pub(super) fn attraction(to: Vec3, [k0, k1, k2]: [f32; 3]) -> Vec3 {
    let d = to.length();
    if d > 0.0 { to / d * (k0 + (k1 + k2 / d) / d) } else { Vec3::ZERO }
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
    crate::interpolate::blend(&previous, &current, t)
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
mod tests;
