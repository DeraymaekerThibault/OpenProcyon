//! Grapple and tow lines (`boarding::Ropes`) drawn as the original's Rope effect: a camera-facing ribbon textured with rope.tga,
//! segment width and texture repeat from the effect file. A slack line sags into a parabola as long as the line.
//! PROVISIONAL: the original simulates the line as a 40-segment mass chain under a constant force; the sag direction (down) and the
//! minimum on-screen width (so the 0.1-unit line still reads at battle zoom) are ours.
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::Scenario;
use crate::boarding::Ropes;

const POINTS: usize = 24;
/// Never thinner than this fraction of the camera distance (about two pixels at 1080p).
const MIN_WIDTH: f32 = 0.002;

pub fn plugin(app: &mut App) {
    app.add_systems(Update, draw.run_if(in_state(crate::menu::Screen::Battle)));
}

#[derive(Component)]
struct RopeMesh;

/// (segment width, segment length) from fx/Rope.json.
fn shape() -> (f32, f32) {
    let j = crate::data::try_json(&crate::assets_dir().join("fx/Rope.json")).unwrap_or_default();
    let f = |k: &str, d: f32| j["Primitive"][0][k].as_f64().map_or(d, |v| v as f32);
    (f("SegmentWidth", 0.1), f("SegmentLength", 2.0))
}

/// The line from `a` to `b`, `slack` longer than the gap: a parabola sagging by the depth that gives it that length.
fn sag_points(a: Vec3, b: Vec3, slack: f32) -> Vec<Vec3> {
    let gap = a.distance(b).max(1e-3);
    let depth = (3.0 * gap * slack / 8.0).sqrt();
    (0..=POINTS).map(|i| i as f32 / POINTS as f32).map(|t| a.lerp(b, t) - Vec3::Y * depth * 4.0 * t * (1.0 - t)).collect()
}

fn draw(
    mut commands: Commands,
    assets: Res<AssetServer>,
    ropes: Res<Ropes>,
    cam: Single<&GlobalTransform, With<Camera3d>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mesh: Query<(&Mesh3d, &mut Visibility), With<RopeMesh>>,
    mut look: Local<Option<(f32, f32)>>,
) {
    let existing = mesh.single_inner().ok();
    if ropes.0.is_empty() {
        if let Some((_, mut vis)) = existing {
            vis.set_if_neq(Visibility::Hidden);
        }
        return;
    }
    let (width, segment) = *look.get_or_insert_with(shape);
    let eye = cam.translation();
    let (mut pos, mut uv, mut idx) = (vec![], vec![], vec![]);
    for &(a, b, slack) in &ropes.0 {
        let pts = sag_points(a, b, slack);
        let mut u = 0.0;
        for (i, &p) in pts.iter().enumerate() {
            let dir = pts[(i + 1).min(POINTS)] - pts[i.saturating_sub(1)];
            let side = dir.cross(eye - p).try_normalize().unwrap_or(Vec3::Y);
            let half = 0.5 * width.max(MIN_WIDTH * eye.distance(p));
            if i > 0 {
                u += p.distance(pts[i - 1]) / segment;
                let k = pos.len() as u32;
                idx.extend([k - 2, k - 1, k, k - 1, k + 1, k]);
            }
            pos.extend([p - side * half, p + side * half]);
            uv.extend([[u, 0.0], [u, 1.0]]);
        }
    }
    let mut new_mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    new_mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    new_mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    new_mesh.insert_indices(Indices::U32(idx));
    if let Some((handle, mut vis)) = existing {
        vis.set_if_neq(Visibility::Inherited);
        if let Some(mut m) = meshes.get_mut(&handle.0) {
            *m = new_mesh;
        }
    } else {
        let texture = assets
            .load_builder()
            .with_settings(|s: &mut ImageLoaderSettings| {
                s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor { address_mode_u: ImageAddressMode::Repeat, ..ImageSamplerDescriptor::linear() });
            })
            .load("fx/tex/rope.png");
        let mat = mats.add(StandardMaterial { base_color_texture: Some(texture), unlit: true, double_sided: true, cull_mode: None, alpha_mode: AlphaMode::Blend, ..default() });
        commands.spawn((Scenario, RopeMesh, Mesh3d(meshes.add(new_mesh)), MeshMaterial3d(mat), Transform::default(), Visibility::Inherited, NoFrustumCulling));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rope_mesh_is_created_only_for_a_rope_and_hidden_when_idle() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<StandardMaterial>()
            .init_resource::<Ropes>()
            .add_systems(Update, draw);
        app.world_mut().spawn((Camera3d::default(), GlobalTransform::default()));
        app.update();
        assert_eq!(app.world_mut().query::<&RopeMesh>().iter(app.world()).count(), 0);
        app.world_mut().resource_mut::<Ropes>().0.push((Vec3::ZERO, Vec3::X, 0.0));
        app.update();
        let world = app.world_mut();
        let (handle, vis) = world.query_filtered::<(&Mesh3d, &Visibility), With<RopeMesh>>().single(world).unwrap();
        assert_eq!(*vis, Visibility::Inherited);
        assert!(world.resource::<Assets<Mesh>>().get(&handle.0).unwrap().count_vertices() > 0);
        world.resource_mut::<Ropes>().0.clear();
        app.update();
        let world = app.world_mut();
        let (handle, vis) = world.query_filtered::<(&Mesh3d, &Visibility), With<RopeMesh>>().single(world).unwrap();
        assert_eq!(*vis, Visibility::Hidden);
        assert!(world.resource::<Assets<Mesh>>().get(&handle.0).unwrap().count_vertices() > 0);
        world.resource_mut::<Ropes>().0.push((Vec3::ZERO, Vec3::Z, 0.0));
        app.update();
        assert_eq!(*app.world_mut().query_filtered::<&Visibility, With<RopeMesh>>().single(app.world()).unwrap(), Visibility::Inherited);
    }

    #[test]
    fn a_slack_line_sags_to_its_length_and_a_taut_one_runs_straight() {
        let length = |p: &[Vec3]| p.windows(2).map(|w| w[0].distance(w[1])).sum::<f32>();
        let (a, b) = (Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        assert!((length(&sag_points(a, b, 0.0)) - 30.0).abs() < 1e-3);
        let slack = sag_points(a, b, 4.0);
        assert!((length(&slack) - 34.0).abs() < 1.0, "a parabola about as long as the line: {}", length(&slack));
        assert!(slack[POINTS / 2].y < -5.0 && slack[0] == a && slack[POINTS] == b);
    }
}
