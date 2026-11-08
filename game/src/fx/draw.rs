//! Drawing the live particles: camera-facing quads batched into one dynamic mesh per (texture, blend). The simulation is in
//! `sim.rs`; this only reads it. The quads are built through the shared `QuadBatch`.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use std::collections::HashMap;

use super::lib::{Blend, FxLib, Prim};
use super::{EffectMaterial, Fx, FxTextures, P, additive_material};
use crate::render::batch::QuadBatch;

pub(super) fn particle_sample(p: &P, remainder: f32) -> (Vec3, f32) {
    (p.pos + p.vel * remainder, p.age + remainder)
}

/// Colour weights `(side, front)` of the two textures of a pseudo-billboard particle, from the direction `to_camera` (particle to
/// camera) and the streak `axis`. Retail draws the particle twice: the side texture as a streak scaled by sin(angle) and the
/// front texture as a camera-facing square scaled by |cos(angle)|, `angle` being the one between axis and view line. Both are
/// plain colour factors (ONE/ONE adds the two); there is no cross-fade. Either vector being (near) zero gives angle 0 (retail
/// leaves the streak unscaled then).
fn pseudo_weights(to_camera: Vec3, axis: Vec3) -> (f32, f32) {
    let (Some(v), Some(a)) = (to_camera.try_normalize(), axis.try_normalize()) else { return (1.0, 1.0) };
    let c = v.dot(a).clamp(-1.0, 1.0);
    ((1.0 - c * c).sqrt(), c.abs())
}

fn streak_axes(direction: Vec3, right: Vec3, up: Vec3, view: Vec3) -> (Vec3, Vec3) {
    let Some(axis) = direction.try_normalize() else { return (right, up) };
    // Keep length along the world-space axis: flattening it into the camera plane lifts horizontal flames.
    (view.cross(axis).try_normalize().unwrap_or(right), axis)
}

fn particle_axes(prim: &Prim, gt: &GlobalTransform, vel: Vec3, camera_axes: [Vec3; 3]) -> (Vec3, Vec3) {
    let [right, up, view] = camera_axes;
    if prim.flat {
        // Retail Flat quads lie in the effect's game XY plane, not the camera plane.
        (gt.right().as_vec3(), gt.forward().as_vec3())
    } else if prim.along_velocity && vel.length_squared() > 1e-6 {
        streak_axes(vel, right, up, view)
    } else if let Some(d) = prim.pseudo_dir.map(|d| gt.affine().transform_vector3(d)) {
        streak_axes(d, right, up, view)
    } else {
        (right, up)
    }
}

/// The pooled meshes, one per (texture, blend): the entity and its mesh.
#[derive(Resource, Default)]
pub(super) struct Groups(pub(super) HashMap<(usize, Blend), (Entity, Handle<Mesh>)>);

/// On every pooled particle mesh: whether it blends ONE/ONE (glows) or over the scene (smoke, debris). Classic draws all of
/// them unlit; their quads carry normals facing the camera so a render profile (render/materials.rs) can shade smoke.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub struct ParticleGroup {
    pub additive: bool,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_fx(
    mut commands: Commands,
    lib: Res<FxLib>,
    tex: Res<FxTextures>,
    mut groups: ResMut<Groups>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut additive: ResMut<Assets<EffectMaterial>>,
    cam: Single<&GlobalTransform, With<Camera3d>>,
    fx: Query<(&Fx, &GlobalTransform)>,
    mut vis: Query<&mut Visibility>,
    // the quads of each pooled mesh, kept between frames
    mut batches: Local<HashMap<(usize, Blend), QuadBatch>>,
) {
    let (right, up, view) = (cam.right().as_vec3(), cam.up().as_vec3(), cam.forward().as_vec3());
    let cam_pos = cam.translation();
    // every quad faces the camera
    let normal = -view;
    for b in batches.values_mut() {
        b.clear();
    }
    let unit = Rect::new(0.0, 0.0, 1.0, 1.0);
    for (f, gt) in &fx {
        let eff = &lib.list[f.eff];
        for (prim, st) in eff.prims.iter().zip(&f.prims) {
            let Some(ti) = prim.tex else { continue };
            for p in &st.parts {
                // Keep retail 30 Hz simulation, but draw motion and fades at the display frame rate.
                let (pos, age) = particle_sample(p, f.tick_acc);
                if age >= p.life {
                    continue;
                }
                let (pos, vel) = if prim.world { (pos, p.vel) } else { (gt.transform_point(pos), gt.affine().transform_vector3(p.vel)) };
                let size = prim.size.at(age, 1.0) * f.size_scale;
                let col = [prim.rgb[0].at(age, 1.0), prim.rgb[1].at(age, 1.0), prim.rgb[2].at(age, 1.0), prim.alpha.at(age, 1.0)];
                let (x0, y0, x1, y1) = match &prim.rect {
                    Some(r) => {
                        (r[0].at(age, -0.5) * prim.rect_scale, r[1].at(age, 0.5) * prim.rect_scale, r[2].at(age, 0.5) * prim.rect_scale, r[3].at(age, -0.5) * prim.rect_scale)
                    }
                    None => (-0.5, 0.5, 0.5, -0.5),
                };
                let (ax, ay) = particle_axes(prim, gt, vel, [right, up, view]);
                // Pseudo-billboard with a front texture: retail draws the streak (side texture) scaled by sin(angle) and a
                // camera-facing square (front texture, half-extent = size, the rectangle is ignored) scaled by |cos(angle)|,
                // angle = view line vs streak axis (docs/re/effects.md, "Pseudo-billboard blend").
                let streak = prim.tex2.is_some() && (prim.along_velocity || prim.pseudo_dir.is_some());
                let (side, front) = if streak { pseudo_weights(cam_pos - pos, ay) } else { (1.0, 0.0) };
                if let (true, Some(t2)) = (streak, prim.tex2) {
                    let fcol = col.map(|c| c * front);
                    let half = size;
                    // top-left corner first, as the texture is laid out
                    batches.entry((t2, prim.blend)).or_insert_with(QuadBatch::with_normals).quad(pos, right * half, -up * half, normal, unit, fcol);
                }
                let col = if streak { col.map(|c| c * side) } else { col };
                // the rectangle's corners (x0, y0) top-left to (x1, y1) bottom-right, scaled by the size
                let centre = pos + (ax * (x0 + x1) + ay * (y0 + y1)) * (0.5 * size);
                batches.entry((ti, prim.blend)).or_insert_with(QuadBatch::with_normals).quad(
                    centre,
                    ax * ((x1 - x0) * 0.5 * size),
                    ay * ((y1 - y0) * 0.5 * size),
                    normal,
                    unit,
                    col,
                );
            }
        }
    }
    for (&key, batch) in batches.iter() {
        if batch.is_empty() {
            // nothing to draw this frame: the group hides (it keeps its mesh and material for later)
            if let Some((e, _)) = groups.0.get(&key)
                && let Ok(mut v) = vis.get_mut(*e)
            {
                v.set_if_neq(Visibility::Hidden);
            }
            continue;
        }
        let entry = groups.0.entry(key).or_insert_with(|| {
            // Filled before the entity exists and spawned hidden: an empty mesh (no position attribute) must never be drawn, it
            // fails pipeline validation and Bevy quits (seen when a new effect texture first appeared, a ship's death).
            let mesh = meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
            let shown = batch.upload(&mut meshes, &mesh);
            let mut entity = commands.spawn((Mesh3d(mesh.clone()), Transform::default(), NoFrustumCulling, shown, ParticleGroup { additive: key.1 == Blend::Add }));
            // Every ONE/ONE texture, flares included: several (flare, cannon-ball pseudo) have an all-zero alpha channel, which
            // Bevy's premultiplying AlphaMode::Add turned black, hiding dock lights, repair sparkles and the SOS beacon (PT-11).
            if key.1 == Blend::Add {
                entity.insert(MeshMaterial3d(additive.add(additive_material(tex.0[key.0].clone()))));
            } else {
                entity.insert(MeshMaterial3d(mats.add(StandardMaterial {
                    base_color_texture: Some(tex.0[key.0].clone()),
                    unlit: true,
                    cull_mode: None,
                    double_sided: true,
                    alpha_mode: if key.1 == Blend::Multiply { AlphaMode::Multiply } else { AlphaMode::Blend },
                    ..default()
                })));
            }
            let e = entity.id();
            (e, mesh)
        });
        if let Ok(mut v) = vis.get_mut(entry.0) {
            v.set_if_neq(batch.upload(&mut meshes, &entry.1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::lib::parse_prim;
    use super::super::sim::{TICK, update_fx};
    use super::super::{FxRng, spawn};
    use super::*;
    use crate::assets_dir;
    use serde_json::Value;

    #[test]
    #[ignore = "needs imported assets"]
    fn meteor_streaks_carry_the_front_disc_with_the_retail_weights() {
        let doc: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("fx/MeteorStorm.json")).unwrap()).unwrap();
        for i in 0..2 {
            let prim = parse_prim(&doc["Primitive"][i], &mut vec![]);
            assert!(prim.tex2.is_some() && prim.along_velocity, "streak with a front texture");
            // Falling straight down, seen from the battle camera (pitch 0.9 rad): streak sin, disc |cos|, both partly visible.
            let to_camera = Vec3::new(0.0, 0.9f32.sin(), 0.9f32.cos());
            let (side, front) = pseudo_weights(to_camera, Vec3::NEG_Y);
            assert!((side - 0.9f32.cos()).abs() < 1e-5 && (front - 0.9f32.sin()).abs() < 1e-5);
        }
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn meteor_splash_lies_flat_independently_of_camera_orientation() {
        let doc: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("fx/MeteorStorm.json")).unwrap()).unwrap();
        let prim = parse_prim(&doc["Primitive"][2], &mut vec![]);
        assert!(prim.flat);
        let gt = GlobalTransform::from(Transform::from_rotation(Quat::from_rotation_y(0.7)));
        for pitch in [0.0, 0.4, 1.2] {
            let camera = Quat::from_rotation_x(pitch);
            let (ax, ay) = particle_axes(&prim, &gt, Vec3::ZERO, [camera * Vec3::X, camera * Vec3::Y, camera * Vec3::NEG_Z]);
            assert!(ax.y.abs() < 1e-6 && ay.y.abs() < 1e-6);
            assert!(ax.cross(ay).distance(Vec3::Y) < 1e-6);
        }
        let billboard = parse_prim(&doc["Primitive"][3], &mut vec![]);
        assert!(!billboard.flat, "splash sparks retain their authored billboard mode");
        assert_eq!(particle_axes(&billboard, &gt, Vec3::ZERO, [Vec3::X, Vec3::Y, Vec3::NEG_Z]), (Vec3::X, Vec3::Y));
    }

    #[test]
    fn horizontal_streaks_stay_level_at_oblique_camera_angles() {
        for pitch in [0.0_f32, 0.2, 0.8, 1.5] {
            let rotation = Quat::from_rotation_x(-pitch);
            let view = rotation * Vec3::NEG_Z;
            let up = rotation * Vec3::Y;
            let (width, length) = streak_axes(Vec3::NEG_Z, Vec3::X, up, view);
            assert_eq!(length, Vec3::NEG_Z, "flame length follows the projectile, not camera-up");
            assert!(width.dot(length).abs() < 1e-6);
            assert!(width.dot(view).abs() < 1e-6);
            assert!((width.length() - 1.0).abs() < 1e-6);
        }
        assert_eq!(streak_axes(Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::NEG_Z), (Vec3::X, Vec3::Y));
    }

    #[test]
    fn particle_motion_and_fade_are_sampled_between_ticks() {
        let p = P { pos: Vec3::X, vel: Vec3::X * 30.0, age: 0.2, life: 1.0 };
        let (pos, age) = particle_sample(&p, TICK / 2.0);
        assert!((pos.x - 1.5).abs() < 1e-6);
        assert!((age - (0.2 + TICK / 2.0)).abs() < 1e-6);
        assert_eq!(p.pos, Vec3::X, "render sampling does not alter simulation");
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn dock_light_flares_draw_with_the_unpremultiplied_additive_material() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<FxLib>()
            .insert_resource(FxRng(123))
            .init_resource::<Groups>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<EffectMaterial>>()
            .add_systems(Update, (update_fx, render_fx).chain());
        let n = app.world().resource::<FxLib>().tex_names.len();
        app.insert_resource(FxTextures(vec![Handle::default(); n]));
        app.world_mut().spawn((Camera3d::default(), GlobalTransform::default()));
        app.world_mut()
            .resource_scope(|world, lib: Mut<FxLib>| spawn(&mut world.commands(), &lib, "Docking_Point", None, Transform::default(), Transform::default(), false).unwrap());
        app.world_mut().flush();
        for _ in 0..10 {
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(TICK));
            app.update();
        }
        let flare = app.world().resource::<FxLib>().tex_names.iter().position(|t| t == "flare").unwrap();
        let &(group, _) = app.world().resource::<Groups>().0.get(&(flare, Blend::Add)).expect("dock-light flares drawn");
        // The flare's alpha channel is all zero: a premultiplying material would draw it black (PT-11).
        assert!(app.world().get::<MeshMaterial3d<EffectMaterial>>(group).is_some());
        assert!(app.world().get::<MeshMaterial3d<StandardMaterial>>(group).is_none());
    }

    #[test]
    fn pseudo_billboard_weights_follow_the_view_angle() {
        let (side, front) = pseudo_weights(Vec3::Z, Vec3::X);
        assert!((side - 1.0).abs() < 1e-6 && front.abs() < 1e-6, "side-on: all streak");
        let (side, front) = pseudo_weights(Vec3::Z, Vec3::Z);
        assert!(side.abs() < 1e-6 && (front - 1.0).abs() < 1e-6, "end-on: all front");
        let (side, front) = pseudo_weights(Vec3::new(0.0, 1.0, 1.0), Vec3::Z);
        assert!((side - 0.5f32.sqrt()).abs() < 1e-5 && (front - 0.5f32.sqrt()).abs() < 1e-5, "45 degrees: sin and cos, not complementary");
        assert_eq!(pseudo_weights(Vec3::Z, -Vec3::Z), (0.0, 1.0), "the sign of the axis does not matter");
        assert_eq!(pseudo_weights(Vec3::ZERO, Vec3::Z), (1.0, 1.0), "degenerate vectors count as angle 0");
    }
}
