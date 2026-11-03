//! Ship flags: a waving cloth strip hung between the hull's Flag_Top / Flag_Bottom attachment points (streaming astern).
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::combat::Library;
use crate::{Scenario, Ship, campaign::MissionShip};

const SEG: usize = 10;
/// Cloth rows from Flag_Top to Flag_Bottom; classic moves every row alike, so more rows only matter with `Cloth::flutter`.
const ROWS: usize = 4;

/// How the cloth moves and is drawn. Default: classic (one sideways wave at a constant pace, unlit). A look such as
/// the render profile (render/mod.rs) adds `flutter` (ripples that also run across the flag's height, a share of the flag length), `lit` (shaded by
/// the scene) and `wind` (the flag follows its ship's speed: slack and drooping at rest, streaming and snapping at full speed).
#[derive(Resource, Clone, Copy, Default, PartialEq, Debug)]
pub struct Cloth {
    pub flutter: f32,
    pub lit: bool,
    pub wind: bool,
}
#[derive(Component)]
struct Flag {
    top: Vec3,
    bottom: Vec3,
    phase: f32,
    /// With `Cloth::wind`: the wave's own clock (it runs faster with speed) and the smoothed share of full speed.
    clock: f32,
    wind: f32,
}
#[derive(Component)]
struct Flagged;

pub fn plugin(app: &mut App) {
    app.init_resource::<Cloth>().add_systems(Update, (spawn, wave, retexture));
}

fn spawn(
    mut commands: Commands,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    ships: Query<(Entity, &Ship), Without<Flagged>>,
    cloth: Res<Cloth>,
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
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; (SEG + 1) * (ROWS + 1)]);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[1.0f32, 0.0, 0.0]; (SEG + 1) * (ROWS + 1)]);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, cloth_uvs());
        m.insert_indices(Indices::U32(cloth_indices()));
        let mut material = cloth_material(assets.load(format!("flags/{tex}.png")));
        material.unlit = !cloth.lit;
        let mat = mats.add(material);
        commands.spawn((
            Scenario,
            Flag { top, bottom, phase: e.index().index() as f32, clock: 0.0, wind: 0.0 },
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

/// Vertex `row` (0 at Flag_Top) of cloth column `col` (0 at the hoist).
const fn vertex(col: usize, row: usize) -> usize {
    col * (ROWS + 1) + row
}

/// Per cloth column (hoist to free end), the vertex UVs from Flag_Top to Flag_Bottom. The retail flag images lie on their side: the
/// image top is the hoist (pole side), its bottom the free end (tattered edge, pennant point), its left edge the flag's top edge.
fn cloth_uvs() -> Vec<[f32; 2]> {
    (0..=SEG).flat_map(|i| (0..=ROWS).map(move |j| [j as f32 / ROWS as f32, i as f32 / SEG as f32])).collect()
}

fn cloth_indices() -> Vec<u32> {
    (0..SEG)
        .flat_map(|i| {
            (0..ROWS).flat_map(move |j| {
                let (a, c) = (vertex(i, j) as u32, vertex(i + 1, j) as u32);
                [a, a + 1, c, a + 1, c + 1, c]
            })
        })
        .collect()
}

fn wave(
    time: Res<Time>,
    settings: Option<Res<crate::menu::Settings>>,
    cloth: Res<Cloth>,
    lib: Res<Library>,
    ships: Query<&Ship>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut q: Query<(&mut Flag, &Mesh3d, &ChildOf)>,
) {
    // Display Options 'Flag Waving' off: the cloth holds its last shape.
    if settings.is_some_and(|s| !s.flags) {
        return;
    }
    let dt = time.delta_secs();
    for (mut f, mesh, parent) in &mut q {
        if cloth.wind {
            let share = ships.get(parent.parent()).map_or(0.0, |s| (s.speed.abs() / crate::ship::full_speed(&lib.classes[s.class]).max(1e-3)).clamp(0.0, 1.0));
            f.wind += (share - f.wind) * (dt * 1.5).min(1.0);
            f.clock += dt * (0.35 + 0.65 * f.wind);
        } else {
            (f.wind, f.clock) = (1.0, time.elapsed_secs());
        }
        let Some(mut m) = meshes.get_mut(&mesh.0) else { continue };
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, cloth_shape(&f, cloth.flutter));
        if cloth.lit {
            m.compute_smooth_normals();
        }
    }
}

/// Cloth vertex positions at the flag's clock and wind (1 = classic: streaming straight astern at the classic pace).
fn cloth_shape(f: &Flag, flutter_share: f32) -> Vec<[f32; 3]> {
    let (t, w) = (f.clock, f.wind);
    let len = 1.6 * f.top.distance(f.bottom);
    (0..=SEG)
        .flat_map(|i| {
            let u = i as f32 / SEG as f32;
            // Streams toward the stern (+Z in the Bevy ship frame, bow is -Z), rippling sideways more toward the free end; with
            // less wind it streams less far, droops, and its waves are smaller.
            let side = Vec3::X * (0.25 * len * u * (4.0 * t - 7.0 * u + f.phase).sin()) * (0.25 + 0.75 * w);
            let drop = Vec3::NEG_Y * (0.08 * len * u * u + 0.7 * (1.0 - w) * len * u);
            let along = Vec3::Z * len * u * (0.3 + 0.7 * w);
            (0..=ROWS).map(move |j| {
                let v = j as f32 / ROWS as f32;
                (f.top.lerp(f.bottom, v) + along + side + drop + Vec3::X * flutter_share * len * flutter(u, v, t + f.phase) * (0.2 + 0.8 * w)).to_array()
            })
        })
        .collect()
}

/// Sideways ripple (a share of the flag length) at cloth point (u along, v across): quick waves running to the free end at an
/// angle, so they also cross the flag's height, growing toward the free end, plus a slow belly that breathes in the middle.
fn flutter(u: f32, v: f32, t: f32) -> f32 {
    let ripple = 0.07 * (14.0 * u - 5.0 * v - 9.0 * t).sin() + 0.035 * (27.0 * u + 9.0 * v - 15.0 * t + 1.3).sin();
    let belly = 0.05 * (std::f32::consts::PI * v).sin() * (2.3 * t).sin();
    u * (ripple * (0.3 + 0.7 * u) + belly)
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
        assert_eq!((uv[vertex(0, 0)][1], uv[vertex(0, ROWS)][1]), (0.0, 0.0), "hoist column samples the image top");
        assert_eq!((uv[vertex(SEG, 0)][1], uv[vertex(SEG, ROWS)][1]), (1.0, 1.0), "free end samples the image bottom");
        assert_eq!((uv[vertex(0, 0)][0], uv[vertex(0, ROWS)][0]), (0.0, 1.0), "Flag_Top edge samples the image's left side, Flag_Bottom its right");
        assert_eq!(cloth_indices().iter().max(), Some(&(vertex(SEG, ROWS) as u32)));
    }

    #[test]
    fn classic_cloth_moves_every_row_alike_and_flutter_ripples_across_it() {
        assert_eq!(flutter(0.0, 0.5, 1.0), 0.0, "the hoist stays on the pole");
        let across: Vec<f32> = (0..=ROWS).map(|j| flutter(1.0, j as f32 / ROWS as f32, 0.7)).collect();
        assert!(across.iter().any(|&x| (x - across[0]).abs() > 0.01), "ripples differ from Flag_Top to Flag_Bottom: {across:?}");
        assert_eq!(Cloth::default(), Cloth { flutter: 0.0, lit: false, wind: false });
    }

    #[test]
    fn full_wind_is_the_classic_shape_and_no_wind_hangs_the_flag() {
        let (top, bottom, t) = (Vec3::new(0.0, 4.0, 0.0), Vec3::ZERO, 1.3);
        let flag = |wind| Flag { top, bottom, phase: 0.5, clock: t, wind };
        let len = 1.6 * top.distance(bottom);
        let classic = |i: usize, j: usize| {
            let u = i as f32 / SEG as f32;
            let side = Vec3::X * (0.25 * len * u * (4.0 * t - 7.0 * u + 0.5).sin());
            top.lerp(bottom, j as f32 / ROWS as f32) + Vec3::Z * len * u + side + Vec3::NEG_Y * 0.08 * len * u * u
        };
        let full = cloth_shape(&flag(1.0), 0.0);
        for (i, j) in [(0, 0), (5, 2), (SEG, ROWS)] {
            assert!((Vec3::from(full[vertex(i, j)]) - classic(i, j)).length() < 1e-4, "({i}, {j})");
        }
        let calm = Vec3::from(cloth_shape(&flag(0.0), 1.0)[vertex(SEG, 0)]);
        assert!(calm.y < top.y - 0.5 * len && calm.z < 0.5 * len, "at rest the free end droops: {calm}");
    }
}
