//! The Ships screen's 3D preview ('Ship ViewPort Button'): the selected hull rendered by an off-screen camera into an image that the
//! menu draws over the globe grid. Everything lives on its own render layer, so the battle camera never sees it.
//! The hull stands still at a fixed bearing; dragging with the mouse over the viewport turns it.
//! This file only builds the render rig and applies the decisions of `menu/preview.rs` (hull choice, camera framing, drag turn).
use bevy::app::{HierarchyPropagatePlugin, Propagate};
use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use crate::combat::Library;
use crate::menu::{HullChange, PREVIEW_YAW, Screen, Skirmish, drag_turn, hull_change, preview_camera};

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

/// Marks the menu node that shows the preview; pressing and dragging on it turns the hull.
#[derive(Component)]
pub struct PreviewDrag;

pub fn plugin(app: &mut App) {
    app.add_plugins(HierarchyPropagatePlugin::<RenderLayers>::new(PostUpdate))
        .add_systems(OnEnter(Screen::Fleet), spawn_rig)
        .add_systems(OnExit(Screen::Fleet), despawn)
        .add_systems(Update, (show_hull, frame, drag).run_if(in_state(Screen::Fleet)));
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
        // Plain camera and light: the profile's HDR, bloom and shadows would turn the transparent background opaque.
        crate::render::Styled,
        Camera3d::default(),
        Camera { order: -10, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
        RenderTarget::Image(image.into()),
        RenderLayers::layer(LAYER),
        Transform::default(),
    ));
    commands.spawn((
        PreviewPart,
        crate::render::Styled,
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
    let HullChange::Replace(want) = hull_change(hulls.iter().map(|(_, h)| h.0), sk.shown_class(&lib)) else { return };
    for (e, _) in &hulls {
        commands.entity(e).despawn();
    }
    let Some(ci) = want else { return };
    let class = &lib.classes[ci];
    commands.spawn((
        PreviewHull(ci),
        WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(class.glb.clone()))),
        Propagate(RenderLayers::layer(LAYER)),
        Transform::from_rotation(Quat::from_rotation_y(PREVIEW_YAW)),
    ));
}

/// Frames the shown hull: the original fills most of the viewport, seen from slightly above the deck.
fn frame(lib: Res<Library>, hulls: Query<&PreviewHull>, mut camera: Query<&mut Transform, (With<Camera3d>, With<PreviewPart>)>) {
    let (Ok(hull), Ok(mut cam)) = (hulls.single(), camera.single_mut()) else { return };
    *cam = preview_camera(lib.classes[hull.0].hull_bound);
}

#[allow(deprecated)] // Interaction, see pointer.rs
fn drag(viewport: Query<&Interaction, With<PreviewDrag>>, motion: Res<bevy::input::mouse::AccumulatedMouseMotion>, mut hulls: Query<&mut Transform, With<PreviewHull>>) {
    if !viewport.iter().any(crate::pointer::down) {
        return;
    }
    for mut t in &mut hulls {
        t.rotate_y(drag_turn(motion.delta.x));
    }
}
