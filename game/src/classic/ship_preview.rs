//! The Ships screen's 3D preview ('Ship ViewPort Button'): the selected hull rendered by an off-screen camera into an image that the
//! menu draws over the globe grid. Everything lives on its own render layer, so the battle camera never sees it.
//! PROVISIONAL: the original's camera angle is matched by eye on one screenshot, and the slow turn is not confirmed.
use bevy::app::{HierarchyPropagatePlugin, Propagate};
use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use crate::combat::Library;
use crate::menu::{Screen, Skirmish};

/// The image the preview renders into; the menu draws it in the viewport.
#[derive(Resource)]
pub struct ShipPreview {
    pub image: Handle<Image>,
}

#[derive(Component)]
struct PreviewPart;
/// The loaded hull and the class it shows.
#[derive(Component)]
struct PreviewHull(usize);

const LAYER: usize = 7;
/// Render size: the 393x285 viewport at twice its reference size.
const SIZE: UVec2 = UVec2::new(786, 570);
/// Turn rate (rad/s) and camera elevation / bearing, matched by eye on the original screenshot.
const TURN: f32 = 0.25;

pub fn plugin(app: &mut App) {
    app.add_plugins(HierarchyPropagatePlugin::<RenderLayers>::new(PostUpdate))
        .add_systems(OnEnter(Screen::Fleet), spawn_rig)
        .add_systems(OnExit(Screen::Fleet), despawn)
        .add_systems(Update, (show_hull, frame, turn).run_if(in_state(Screen::Fleet)));
}

/// The rig (camera and light) for the Fleet screen; the target image is created on first use (the initial screen's enter hook can
/// run before startup systems).
fn spawn_rig(mut commands: Commands, preview: Option<Res<ShipPreview>>, mut images: ResMut<Assets<Image>>) {
    let image = match preview {
        Some(p) => p.image.clone(),
        None => {
            let image = images.add(Image::new_target_texture(SIZE.x, SIZE.y, TextureFormat::Rgba8UnormSrgb, None));
            commands.insert_resource(ShipPreview { image: image.clone() });
            image
        }
    };
    commands.spawn((
        PreviewPart,
        Camera3d::default(),
        Camera { order: -10, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
        RenderTarget::Image(image.into()),
        RenderLayers::layer(LAYER),
        Transform::default(),
    ));
    commands.spawn((
        PreviewPart,
        DirectionalLight { illuminance: 9000.0, shadow_maps_enabled: false, ..default() },
        RenderLayers::layer(LAYER),
        Transform::from_xyz(-30.0, 60.0, 40.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn despawn(mut commands: Commands, parts: Query<Entity, Or<(With<PreviewPart>, With<PreviewHull>)>>) {
    for e in &parts {
        commands.entity(e).despawn();
    }
}

/// The selected class (the human player's offered list at `ship_pick`), reloaded when the selection changes; the camera is placed
/// to frame its hull from the bow quarter, slightly above.
fn show_hull(mut commands: Commands, assets: Res<AssetServer>, sk: Res<Skirmish>, lib: Res<Library>, hulls: Query<(Entity, &PreviewHull)>) {
    let want = sk.shown_class(&lib);
    if hulls.iter().any(|(_, h)| Some(h.0) == want) {
        return;
    }
    for (e, _) in &hulls {
        commands.entity(e).despawn();
    }
    let Some(ci) = want else { return };
    let class = &lib.classes[ci];
    commands.spawn((
        PreviewHull(ci),
        WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(class.glb.clone()))),
        Propagate(RenderLayers::layer(LAYER)),
        Transform::from_rotation(Quat::from_rotation_y(0.9)),
    ));
}

/// Frames the shown hull: the original fills most of the viewport, seen from slightly above the deck.
fn frame(lib: Res<Library>, hulls: Query<&PreviewHull>, mut camera: Query<&mut Transform, (With<Camera3d>, With<PreviewPart>)>) {
    let (Ok(hull), Ok(mut cam)) = (hulls.single(), camera.single_mut()) else { return };
    let r = lib.classes[hull.0].hull_bound.max(5.0);
    let d = r * 1.75;
    *cam = Transform::from_xyz(d * 0.6, d * 0.18, d * 0.78).looking_at(Vec3::Y * r * 0.2, Vec3::Y);
}

fn turn(time: Res<Time<Real>>, mut hulls: Query<&mut Transform, With<PreviewHull>>) {
    for mut t in &mut hulls {
        t.rotate_y(TURN * time.delta_secs());
    }
}
