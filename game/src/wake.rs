//! Ship wakes: Wake_Medium is a Trail primitive (Wake.tga, additive ribbon behind the stern) rather than a particle effect.
//! Widths/lifetime are tuned by eye against the reference video; the retail 40-unit InitialWidth is not applied directly.
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::{Scenario, Ship};

const LIFE: f32 = 14.0;
const STERN: f32 = 3.5; // distance behind the ship origin (engine is at y=-5.89)
const STEP: f32 = 1.2;

#[derive(Component)]
pub struct Wake {
    ship: Entity,
    pts: Vec<(Vec3, f32, f32)>,
} // (position, age, strength 0..1 from speed when laid)

/// Multiplier of the wake texture per fleet type, from the optional `data/fx/wake_tint.json` (`{"Navy": [r, g, b]}`, values may exceed 1).
/// Absent: the retail colour for everyone. A mod ships the file, so wakes can tell the factions apart without new textures.
fn wake_tint(race: &str) -> Option<[f32; 3]> {
    static TINTS: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let tints = TINTS.get_or_init(|| crate::data::try_json(&crate::assets_dir().join("data/fx/wake_tint.json")).unwrap_or_default());
    let v = tints.get(race)?.as_array()?;
    Some([v.first()?.as_f64()? as f32, v.get(1)?.as_f64()? as f32, v.get(2)?.as_f64()? as f32])
}

fn sh_class(ships: &Query<(Entity, &Transform, &Ship, Option<&crate::combat::Health>), With<Ship>>, e: Entity) -> usize {
    ships.get(e).map_or(0, |(_, _, sh, _)| sh.class)
}

pub fn update(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<crate::fx::EffectMaterial>>,
    ships: Query<(Entity, &Transform, &Ship, Option<&crate::combat::Health>), With<Ship>>,
    lib: Res<crate::combat::Library>,
    mut wakes: Query<(Entity, &mut Wake, &Mesh3d)>,
) {
    let dt = time.delta_secs();
    for (s, _, _, health) in &ships {
        if health.is_some_and(|h| h.destroyed) {
            continue;
        }
        if !wakes.iter().any(|(_, w, _)| w.ship == s) {
            let mesh = meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
            let mut material = crate::fx::additive_material(assets.load("fx/tex/wake.png"));
            if let Some([r, g, b]) = wake_tint(&lib.classes[sh_class(&ships, s)].race) {
                material.base.base_color = Color::LinearRgba(LinearRgba::new(r, g, b, 1.0));
            }
            let mat = mats.add(material);
            commands.spawn((Scenario, Wake { ship: s, pts: vec![] }, Mesh3d(mesh), MeshMaterial3d(mat), Transform::default(), Visibility::default(), NoFrustumCulling));
        }
    }
    for (e, mut w, mesh) in &mut wakes {
        let Ok((_, tf, sh, health)) = ships.get(w.ship) else {
            commands.entity(e).despawn();
            continue;
        };
        if health.is_some_and(|h| h.destroyed) {
            commands.entity(e).despawn();
            continue;
        }
        for p in &mut w.pts {
            p.1 += dt;
        }
        w.pts.retain(|p| p.1 < LIFE);
        let stern = tf.translation + tf.rotation * Vec3::Z * STERN;
        if w.pts.last().is_none_or(|p| p.0.distance(stern) > STEP) {
            w.pts.push((stern, 0.0, (sh.speed / crate::ship::full_speed(&lib.classes[sh.class])).clamp(0.0, 1.0)));
        }
        let (mut pos, mut uv, mut col, mut idx) = (vec![], vec![], vec![], vec![]);
        let n = w.pts.len() + 1; // Draw a live head between distance-sampled points, so the trail does not jump every STEP.
        let strength = (sh.speed / crate::ship::full_speed(&lib.classes[sh.class])).clamp(0.0, 1.0);
        let mut v = 0.0;
        for i in 0..n {
            let (p, age, k) = if i + 1 < n { w.pts[i] } else { (stern, 0.0, strength) };
            let dir = if i + 2 < n {
                w.pts[i + 1].0 - p
            } else if i + 1 < n {
                stern - p
            } else {
                stern - w.pts[i - 1].0
            };
            let side = dir.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
            let t = age / LIFE;
            let half = 1.5 * (2.0 + 7.0 * t.sqrt()) * (0.4 + 0.6 * k);
            let a = (1.0 - t) * (1.0 - t) * k.sqrt(); // No extra guessed 0.7 brightness multiplier.
            if i > 0 {
                v += p.distance(w.pts[i - 1].0) / 8.0;
            }
            let base = p + Vec3::Y * 0.3;
            pos.extend([(base - side * half).to_array(), (base + side * half).to_array()]);
            uv.extend([[0.0, v], [1.0, v]]);
            col.extend([[a, a, a, 1.0]; 2]); // Preserve lifetime/speed fade with ONE/ONE blending.
            if i > 0 {
                let b = (i * 2) as u32;
                idx.extend([b - 2, b - 1, b, b - 1, b + 1, b]);
            }
        }
        if let Some(mut m) = meshes.get_mut(&mesh.0) {
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
            m.insert_indices(Indices::U32(idx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "needs imported assets"]
    fn live_head_moves_between_distance_samples() {
        let lib = crate::combat::Library::load(crate::assets_dir());
        let class = lib.index("Ship_Navy_Cutter");
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<crate::fx::EffectMaterial>()
            .insert_resource(lib)
            .add_systems(Update, update);
        let ship = app
            .world_mut()
            .spawn((
                crate::Ship {
                    class,
                    side: crate::Side::Player,
                    heading: 0.0,
                    speed: 5.0,
                    ang_vel: 0.0,
                    order: 2,
                    waypoints: vec![],
                    goal: None,
                    cap: 1.0,
                    throttle: None,
                    sail_energy: 1.0,
                    crew_speed: 1.0,
                    crew_turn: 1.0,
                    net: 1.0,
                    avoid: 1.0,
                },
                Transform::default(),
            ))
            .id();
        app.update();
        app.world_mut().get_mut::<Transform>(ship).unwrap().translation.x = 0.1;
        app.update();
        let world = app.world_mut();
        let (wake, handle) = world.query::<(&Wake, &Mesh3d)>().single(world).unwrap();
        assert_eq!(wake.pts.len(), 1, "movement smaller than STEP must not create a stored sample");
        let mesh = world.resource::<Assets<Mesh>>().get(&handle.0).unwrap();
        let bevy::mesh::VertexAttributeValues::Float32x3(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() else { panic!("positions") };
        let centre = (Vec3::from_array(positions[positions.len() - 2]) + Vec3::from_array(positions[positions.len() - 1])) * 0.5;
        assert!((centre - Vec3::new(0.1, 0.3, STERN)).length() < 1e-5, "head must follow current stern, not the old sample");
        world.entity_mut(ship).insert(crate::combat::Health { sections: vec![], core: 0.0, destroyed: true });
        app.update();
        app.update();
        let world = app.world_mut();
        assert_eq!(world.query::<&Wake>().iter(world).count(), 0, "dead ships must neither retain nor recreate wakes");
    }
}
