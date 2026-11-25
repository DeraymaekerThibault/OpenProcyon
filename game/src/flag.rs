//! Ship flags: a waving cloth strip hung between the hull's Flag_Top / Flag_Bottom attachment points (streaming astern).
//! Not retail: some hulls' points miss their staff (the Legacy's by about 2.2 units, the Heavy Escorts' and the Navy Assault
//! Cutter's by about 0.5), so every flag is hung on the nearest staff within `SNAP` units, at the points' heights (`snap`).
use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
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

/// A flag's mesh is rewritten (and so re-uploaded to the GPU) at most this often, in cloth-clock seconds: 60 shapes a second is as
/// smooth as the 15 rad/s fastest ripple can show, and a display running at 150 Hz does not need 150.
const REDRAW: f32 = 1.0 / 60.0;

/// How far (units) from its retail points a flag looks for the staff it hangs on.
const SNAP: f32 = 3.0;
#[derive(Component)]
struct Flag {
    top: Vec3,
    bottom: Vec3,
    phase: f32,
    /// With `Cloth::wind`: the wave's own clock (it runs faster with speed) and the smoothed share of full speed.
    clock: f32,
    wind: f32,
    /// The clock the mesh was last written at (`REDRAW`).
    drawn: f32,
}
#[derive(Component)]
struct Flagged;
/// A flag not yet moved onto its staff (it waits for the ship's meshes to load).
#[derive(Component)]
struct Unsnapped;

pub fn plugin(app: &mut App) {
    app.init_resource::<Cloth>().add_systems(Update, (spawn, snap, wave, retexture));
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
            Flag { top, bottom, phase: e.index().index() as f32, clock: 0.0, wind: 0.0, drawn: f32::NEG_INFINITY },
            Unsnapped,
            Mesh3d(meshes.add(m)),
            MeshMaterial3d(mat),
            Transform::default(),
            Visibility::default(),
            cloth_bounds(top, bottom),
            ChildOf(e),
        ));
    }
}

/// The box the cloth can reach, in the ship's frame (the flag sits at the ship's origin): the mesh is rewritten every frame, so a
/// computed box would lag, and a flag out of view should cost nothing (`wave` skips it by `ViewVisibility`). Generous: the cloth
/// streams about 1.6 flag heights astern and droops up to as far, and the staff snap moves the points by up to `SNAP`.
fn cloth_bounds(top: Vec3, bottom: Vec3) -> Aabb {
    let reach = 2.0 * top.distance(bottom) + SNAP;
    Aabb { center: ((top + bottom) / 2.0).into(), half_extents: Vec3::splat(reach).into() }
}

/// Once the ship's meshes are loaded, moves the flag onto the nearest staff (`onto_staff`). Damage layers are left
/// out (their staff may be broken).
fn snap(
    mut commands: Commands,
    meshes: Res<Assets<Mesh>>,
    children: Query<&Children>,
    parts: Query<(&Mesh3d, &GlobalTransform, &ChildOf)>,
    names: Query<&Name>,
    frames: Query<&GlobalTransform>,
    mut flags: Query<(Entity, &mut Flag, &ChildOf), With<Unsnapped>>,
) {
    'flags: for (e, mut f, parent) in &mut flags {
        let ship = parent.parent();
        let Ok(frame) = frames.get(ship) else { continue };
        let to_ship = frame.affine().inverse();
        let mut pieces = vec![];
        for d in children.iter_descendants(ship) {
            let Ok((mesh, at, node)) = parts.get(d) else { continue };
            if [d, node.parent()].iter().any(|n| names.get(*n).is_ok_and(|n| crate::data::derive::is_damage_layer(n.as_str()))) {
                continue;
            }
            let Some(m) = meshes.get(&mesh.0) else { continue 'flags };
            // a mesh that lives on the GPU only (the remaster's deck crews) has no positions here: reading them would panic
            let Ok(Some(bevy::mesh::VertexAttributeValues::Float32x3(pos))) = m.try_attribute_option(Mesh::ATTRIBUTE_POSITION) else { continue };
            let local = to_ship * at.affine();
            let pos: Vec<[f32; 3]> = pos.iter().map(|p| local.transform_point3(Vec3::from(*p)).to_array()).collect();
            let tris = match m.try_indices_option() {
                Ok(Some(i)) => i.iter().collect::<Vec<_>>().as_chunks::<3>().0.to_vec(),
                _ => (0..pos.len() / 3).map(|t| [3 * t, 3 * t + 1, 3 * t + 2]).collect(),
            };
            pieces.push((pos, tris));
        }
        if pieces.is_empty() {
            continue; // the ship's scene has not spawned yet
        }
        if let Some((top, bottom)) = onto_staff(&pieces, f.top, f.bottom, SNAP) {
            (f.top, f.bottom) = (top, bottom);
        }
        commands.entity(e).remove::<Unsnapped>();
    }
}

/// The flag's top and bottom moved onto the nearest staff, keeping their heights: a connected piece of mesh (ship frame) no more
/// than 0.4 thick about the line from its lowest to its highest point and at least 0.6 of the flag's height tall, whose line passes
/// within `radius` of the flag's middle. None when there is none.
fn onto_staff(pieces: &[(Vec<[f32; 3]>, Vec<[usize; 3]>)], top: Vec3, bottom: Vec3, radius: f32) -> Option<(Vec3, Vec3)> {
    let mid = (top + bottom) / 2.0;
    let tall = 0.6 * (top.y - bottom.y).abs();
    let off_line = |p: Vec3, a: Vec3, dir: Vec3| (p - a).reject_from_normalized(dir).length();
    let mut best: Option<(f32, Vec3, Vec3)> = None;
    for (pos, tris) in pieces {
        let piece = crate::render::sails::sail_of(pos, tris);
        let mut groups: std::collections::HashMap<usize, Vec<Vec3>> = std::collections::HashMap::new();
        for &i in tris.iter().flatten() {
            groups.entry(piece[i]).or_default().push(Vec3::from(pos[i]));
        }
        for v in groups.values() {
            let lo = v.iter().copied().min_by(|a, b| a.y.total_cmp(&b.y)).unwrap_or_default();
            let hi = v.iter().copied().max_by(|a, b| a.y.total_cmp(&b.y)).unwrap_or_default();
            if hi.y - lo.y < tall.max(1e-3) {
                continue;
            }
            let dir = (hi - lo).normalize();
            if v.iter().any(|&p| off_line(p, lo, dir) > 0.4) {
                continue;
            }
            let d = off_line(mid, lo, dir);
            if d <= radius && best.is_none_or(|b| d < b.0) {
                best = Some((d, lo, hi - lo));
            }
        }
    }
    let (_, lo, axis) = best?;
    let at = |y: f32| lo + axis * ((y - lo.y) / axis.y);
    Some((at(top.y), at(bottom.y)))
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
    lib: Option<Res<Library>>,
    ships: Query<&Ship>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut q: Query<(&mut Flag, &Mesh3d, &ChildOf, &ViewVisibility)>,
) {
    // Display Options 'Flag Waving' off: the cloth holds its last shape.
    if settings.is_some_and(|s| !s.flags) {
        return;
    }
    let dt = time.delta_secs();
    for (mut f, mesh, parent, seen) in &mut q {
        if cloth.wind {
            let share = lib
                .as_ref()
                .zip(ships.get(parent.parent()).ok())
                .map_or(0.0, |(lib, s)| (s.speed.abs() / crate::ship::full_speed(&lib.classes[s.class]).max(1e-3)).clamp(0.0, 1.0));
            f.wind += (share - f.wind) * (dt * 1.5).min(1.0);
            f.clock += dt * (0.35 + 0.65 * f.wind);
        } else {
            (f.wind, f.clock) = (1.0, time.elapsed_secs());
        }
        // The clock runs on, but a flag nobody sees is not drawn into its mesh, nor one that has not moved a 60th of a second since
        // (a mesh asset that is touched is re-uploaded whole). A change of look redraws at once.
        if !seen.get() || ((f.clock - f.drawn).abs() < REDRAW && !cloth.is_changed()) {
            continue;
        }
        f.drawn = f.clock;
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
    use bevy::camera::visibility::SetViewVisibility;

    #[test]
    fn a_flag_that_misses_its_staff_is_hung_on_the_nearest_one() {
        // a staff: a thin quad from (0, 0, 2) up to (0, 4, 2.4), leaning aft; a wide deck slab nearer the flag must not count
        let staff: Vec<[f32; 3]> = [[-0.1, 0.0, 2.0], [0.1, 0.0, 2.0], [-0.1, 4.0, 2.4], [0.1, 4.0, 2.4]].to_vec();
        let deck: Vec<[f32; 3]> = [[-3.0, 0.5, 0.0], [3.0, 0.5, 0.0], [-3.0, 3.5, 0.2], [3.0, 3.5, 0.2]].to_vec();
        let quad = vec![[0, 1, 2], [1, 3, 2]];
        let pieces = vec![(staff, quad.clone()), (deck, quad)];
        let (top, bottom) = onto_staff(&pieces, Vec3::new(0.0, 3.0, 0.0), Vec3::new(0.0, 1.0, 0.0), 3.0).unwrap();
        assert!(top.distance(Vec3::new(0.0, 3.0, 2.3)) < 0.11 && bottom.distance(Vec3::new(0.0, 1.0, 2.1)) < 0.11, "{top} {bottom}");
        assert!(onto_staff(&pieces, Vec3::new(0.0, 3.0, 0.0), Vec3::new(0.0, 1.0, 0.0), 1.0).is_none(), "out of reach: retail points kept");
    }

    /// A mesh already handed to the GPU (the remaster's deck crews ride the ship) is skipped, not read: reading it panicked at a
    /// mission start. The flag still snaps by the ship's other meshes.
    #[test]
    fn a_gpu_only_mesh_on_the_ship_is_skipped_when_snapping() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default())).init_asset::<Mesh>().add_systems(Update, snap);
        let mut gone = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        gone.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3]);
        let _ = gone.take_gpu_data();
        let staff = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-0.1f32, 0.0, 2.0], [0.1, 0.0, 2.0], [-0.1, 4.0, 2.4], [0.1, 4.0, 2.4]])
            .with_inserted_indices(Indices::U32(vec![0, 1, 2, 1, 3, 2]));
        let (gone, staff) = {
            let mut meshes = app.world_mut().resource_mut::<Assets<Mesh>>();
            (meshes.add(gone), meshes.add(staff))
        };
        let ship = app.world_mut().spawn(GlobalTransform::IDENTITY).id();
        for m in [gone, staff] {
            app.world_mut().spawn((Mesh3d(m), GlobalTransform::IDENTITY, ChildOf(ship)));
        }
        let flag = Flag { top: Vec3::new(0.0, 3.0, 0.0), bottom: Vec3::new(0.0, 1.0, 0.0), phase: 0.0, clock: 0.0, wind: 0.0, drawn: 0.0 };
        let f = app.world_mut().spawn((flag, Unsnapped, ChildOf(ship))).id();
        app.update();
        assert!(!app.world().entity(f).contains::<Unsnapped>(), "snapped");
        assert!(app.world().get::<Flag>(f).unwrap().top.z > 2.0, "onto the staff");
    }

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
        let flag = |wind| Flag { top, bottom, phase: 0.5, clock: t, wind, drawn: 0.0 };
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

    /// A one-flag world: `wave` on a flag whose view visibility the test sets.
    fn flag_world() -> (App, Entity, Handle<Mesh>) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default())).init_asset::<Mesh>().init_resource::<Cloth>().add_systems(Update, wave);
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        m.insert_indices(Indices::U32(cloth_indices()));
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(m);
        let ship = app.world_mut().spawn_empty().id();
        let flag = app
            .world_mut()
            .spawn((
                Flag { top: Vec3::Y * 4.0, bottom: Vec3::ZERO, phase: 0.0, clock: 0.0, wind: 1.0, drawn: f32::NEG_INFINITY },
                Mesh3d(mesh.clone()),
                ChildOf(ship),
                ViewVisibility::HIDDEN,
            ))
            .id();
        (app, flag, mesh)
    }

    /// Runs one frame `dt` seconds long and tells whether the flag's mesh asset was modified in it.
    fn frame(app: &mut App, dt: f32, mesh: &Handle<Mesh>) -> bool {
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f32(dt)));
        app.update();
        let mut msgs = app.world_mut().resource_mut::<bevy::ecs::message::Messages<AssetEvent<Mesh>>>();
        msgs.drain().any(|e| e.is_modified(mesh.id()))
    }

    #[test]
    fn a_flag_out_of_view_is_not_redrawn_and_one_in_view_is_redrawn_at_most_sixty_times_a_second() {
        let (mut app, flag, mesh) = flag_world();
        assert!(!(0..5).any(|_| frame(&mut app, 0.01, &mesh)), "out of view: the mesh asset is never touched");
        app.world_mut().get_mut::<ViewVisibility>(flag).unwrap().set_visible();
        assert!(frame(&mut app, 0.01, &mesh), "in view: drawn at once");
        assert!(!frame(&mut app, 0.01, &mesh), "10 ms later: still within the same 60th of a second");
        assert!(frame(&mut app, 0.01, &mesh), "20 ms after the last draw: due");
        assert!(app.world().get::<Flag>(flag).unwrap().clock > 0.03, "the clock ran on while it was not drawn");
        app.world_mut().resource_mut::<Cloth>().lit = true;
        assert!(frame(&mut app, 0.001, &mesh), "a change of look redraws at once");
    }
}
