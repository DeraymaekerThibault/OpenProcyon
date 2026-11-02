//! Ship flags: a waving cloth strip hung between the hull's Flag_Top / Flag_Bottom attachment points (streaming astern).
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::combat::Library;
use crate::{Scenario, Ship, campaign::MissionShip};

const SEG: usize = 10;
#[derive(Component)]
struct Flag {
    top: Vec3,
    bottom: Vec3,
    phase: f32,
}
#[derive(Component)]
struct Flagged;

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (spawn, wave, retexture));
}

fn spawn(
    mut commands: Commands,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    ships: Query<(Entity, &Ship), Without<Flagged>>,
) {
    for (e, ship) in &ships {
        commands.entity(e).insert(Flagged);
        let class = &lib.classes[ship.class];
        let Some((top, bottom)) = class.flag else { continue };
        // shortcut: first flag of the faction, not the per-ship or per-mission flag choice.
        let tex = if class.name.contains("Pirate") {
            "flag_pirate00"
        } else if class.name.contains("Procyon") {
            "flag_procyon_00"
        } else {
            "flag_navy00"
        };
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; (SEG + 1) * 2]);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, cloth_uvs());
        m.insert_indices(Indices::U32(
            (0..SEG as u32)
                .flat_map(|i| {
                    let a = i * 2;
                    [a, a + 1, a + 2, a + 1, a + 3, a + 2]
                })
                .collect(),
        ));
        let mat = mats.add(cloth_material(assets.load(format!("flags/{tex}.png"))));
        commands.spawn((
            Scenario,
            Flag { top, bottom, phase: e.index().index() as f32 },
            Mesh3d(meshes.add(m)),
            MeshMaterial3d(mat),
            Transform::default(),
            Visibility::default(),
            NoFrustumCulling,
            ChildOf(e),
        ));
    }
}

/// Unlit, double-sided cloth. The retail flag art cuts its outline with alpha (the pirate flag's tattered edge), so transparent
/// texels are masked out; a cut-out avoids the sorting problems of blending a double-sided waving mesh.
fn cloth_material(texture: Handle<Image>) -> StandardMaterial {
    StandardMaterial { base_color_texture: Some(texture), unlit: true, cull_mode: None, double_sided: true, alpha_mode: AlphaMode::Mask(0.5), ..default() }
}

/// Per cloth row (hoist to free end), the (Flag_Top, Flag_Bottom) vertex UVs. The retail flag images lie on their side: the image
/// top is the hoist (pole side), its bottom the free end (tattered edge, pennant point), its left edge the flag's top edge.
fn cloth_uvs() -> Vec<[f32; 2]> {
    (0..=SEG)
        .flat_map(|i| {
            let u = i as f32 / SEG as f32;
            [[0.0, u], [1.0, u]]
        })
        .collect()
}

fn wave(time: Res<Time>, settings: Option<Res<crate::menu::Settings>>, mut meshes: ResMut<Assets<Mesh>>, q: Query<(&Flag, &Mesh3d)>) {
    // Display Options 'Flag Waving' off: the cloth holds its last shape.
    if settings.is_some_and(|s| !s.flags) {
        return;
    }
    let t = time.elapsed_secs();
    for (f, mesh) in &q {
        let Some(mut m) = meshes.get_mut(&mesh.0) else { continue };
        let len = 1.6 * f.top.distance(f.bottom);
        let pos: Vec<[f32; 3]> = (0..=SEG)
            .flat_map(|i| {
                let u = i as f32 / SEG as f32;
                // Streams toward the stern (+Z in the Bevy ship frame, bow is -Z), rippling sideways more toward the free end.
                let side = Vec3::X * (0.25 * len * u * (4.0 * t - 7.0 * u + f.phase).sin());
                let drop = Vec3::NEG_Y * 0.08 * len * u * u;
                [f.top, f.bottom].map(|p| (p + Vec3::Z * len * u + side + drop).to_array())
            })
            .collect();
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    }
}

/// Mission rules 'Set Ships Flag Texture': swap the cloth texture when the unit's flag name changes.
fn retexture(
    assets: Res<AssetServer>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    flags: Query<(&ChildOf, &MeshMaterial3d<StandardMaterial>), With<Flag>>,
    ships: Query<&MissionShip, Changed<MissionShip>>,
) {
    for (parent, mat) in &flags {
        let Ok(ms) = ships.get(parent.parent()) else { continue };
        if ms.flag.is_empty() {
            continue;
        }
        let tex = assets.load(format!("flags/{}.png", ms.flag.trim_end_matches(".tga").to_lowercase()));
        if let Some(mut m) = mats.get_mut(&mat.0)
            && m.base_color_texture.as_ref() != Some(&tex)
        {
            m.base_color_texture = Some(tex);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_cloth_masks_the_transparent_parts_of_the_art() {
        assert_eq!(cloth_material(Handle::default()).alpha_mode, AlphaMode::Mask(0.5));
    }

    #[test]
    fn image_top_is_the_hoist_and_image_left_the_top_edge() {
        let uv = cloth_uvs();
        assert_eq!((uv[0][1], uv[1][1]), (0.0, 0.0), "hoist row samples the image top");
        assert_eq!((uv[2 * SEG][1], uv[2 * SEG + 1][1]), (1.0, 1.0), "free end samples the image bottom");
        assert_eq!((uv[0][0], uv[1][0]), (0.0, 1.0), "Flag_Top edge samples the image's left side, Flag_Bottom its right");
    }
}
