//! Not retail: black holes of the render profile (`black_holes` block, drawn with the mod's `shaders/blackhole.wesl`). The retail
//! look is only its additive ribbons (`blackhole.rs`); this puts a flat disc of the pull's radius in the hole's plane, blended
//! over everything behind it and just under the ships: a black horizon exactly as wide as the kill core (`Pull::core`), a thin
//! bright ring on its edge that marks the kill zone at any zoom, the baked sky bent around it (lensing of the sky cube, and of
//! the stars cube when the sky has one), a hot accretion disc that turns the way the pull swirls (clockwise seen from above),
//! and a flash when a ship is swallowed. The retail ribbons stay on top as the infalling streams. All sizes are shares of the
//! pull radius, so the three hole sizes scale together.
//!
//! Only reads `blackhole::BlackHole`, `blackhole::Pull` and `blackhole::Swallowed`; nothing here changes what the hole does.
//! The disc does not bend ships behind the hole: the camera looks down, so what lies behind a hole is almost always sky.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use serde_json::Value;

use super::{Look, num, rgb};
use crate::blackhole::{BlackHole, Pull, Swallowed};
use crate::view::{HdSky, Sky};

pub const SHADER: &str = "shaders/blackhole.wesl";

/// Seconds the ring and disc flash when a hole swallows a ship.
const FLASH_SECONDS: f32 = 1.0;

/// The `black_holes` block; every size is a share of the hole's pull radius.
#[derive(Clone, Debug, PartialEq)]
pub struct BlackHoles {
    /// Radius of the black horizon; the default 0.25 is `Pull::core()`'s share, so the black area is the kill zone.
    pub horizon: f32,
    /// Thickness of the bright ring on the horizon's edge (never thinner than a pixel and a half on screen).
    pub ring: f32,
    /// How hard the hole bends the sky behind it (1 = a radian at the horizon's edge).
    pub lensing: f32,
    pub disc: Disc,
    /// Warm halo just outside the horizon, strength.
    pub glow: f32,
    /// How far under the hole's plane the disc lies, a share of the radius, so hulls in the plane draw over it.
    pub drop: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Disc {
    /// Colour of the cooler outer part (linear); the inner part is hotter, towards white.
    pub color: Vec3,
    pub inner: f32,
    pub outer: f32,
    /// Turns a second (clockwise seen from above).
    pub spin: f32,
    /// Brightness; above 1 so the bloom catches it.
    pub brightness: f32,
}

pub fn parse(v: &Value) -> Option<BlackHoles> {
    v.is_object().then(|| {
        let d = &v["disc"];
        let inner = num(&d["inner"], 0.3).clamp(0.0, 0.95);
        BlackHoles {
            horizon: num(&v["horizon"], 0.25).clamp(0.0, 0.95),
            ring: num(&v["ring"], 0.012).clamp(0.0, 0.2),
            lensing: num(&v["lensing"], 1.0).clamp(0.0, 4.0),
            disc: Disc {
                color: rgb(&d["color"]).unwrap_or(Vec3::new(1.0, 0.45, 0.12)).max(Vec3::ZERO),
                inner,
                outer: num(&d["outer"], 0.62).clamp(inner + 0.01, 1.0),
                spin: num(&d["spin"], 0.08),
                brightness: num(&d["brightness"], 2.0).max(0.0),
            },
            glow: num(&v["glow"], 0.4).max(0.0),
            drop: num(&v["drop"], 0.02).clamp(0.0, 0.5),
        }
    })
}

/// What the black hole shader gets at `@group(MATERIAL_BIND_GROUP) @binding(100)`.
#[derive(Clone, Copy, Default, Debug, PartialEq, ShaderType)]
pub struct HoleUniform {
    /// xyz: the hole's centre (world), w: the pull radius
    pub centre: Vec4,
    /// x: horizon radius (world units), y: the kill core's radius, z: ring thickness (world units), w: lensing strength (0 = none)
    pub shape: Vec4,
    /// x: disc inner radius, y: outer radius (world units), z: spin (turns a second), w: brightness
    pub disc: Vec4,
    /// rgb: the disc's cooler colour
    pub color: Vec4,
    /// x: clock (battle seconds), y: swallow flash 0..1, z: glow, w: the stars cube's decode gain (0 = no stars cube)
    pub params: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct HoleGlow {
    #[uniform(100)]
    pub u: HoleUniform,
    #[texture(101, dimension = "cube")]
    #[sampler(102)]
    pub cube: Option<Handle<Image>>,
    #[texture(103, dimension = "cube")]
    #[sampler(104)]
    pub stars: Option<Handle<Image>>,
}

impl MaterialExtension for HoleGlow {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // a veil over the sky, never over the ships: no depth write, and the ships' own depth keeps them in front
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

pub type HoleMaterial = ExtendedMaterial<StandardMaterial, HoleGlow>;

pub fn plugin(app: &mut App, look: &Look) {
    if look.black_holes.is_some() {
        app.add_plugins(MaterialPlugin::<HoleMaterial>::default()).add_systems(Update, (spawn_discs, update_discs, flash_on_swallow, sky_cube).chain());
    }
}

/// Marks a hole that has its disc.
#[derive(Component)]
pub struct HoleLook;

/// The disc, the child of its hole.
#[derive(Component)]
pub struct HoleDisc {
    flash: f32,
}

impl BlackHoles {
    fn uniform(&self, centre: Vec3, pull: &Pull, clock: f32, flash: f32, gain: f32, lens: bool) -> HoleUniform {
        let r = pull.radius;
        HoleUniform {
            centre: centre.extend(r),
            shape: Vec4::new(self.horizon * r, pull.core(), self.ring * r, if lens { self.lensing } else { 0.0 }),
            disc: Vec4::new(self.disc.inner * r, self.disc.outer * r, self.disc.spin, self.disc.brightness),
            color: self.disc.color.extend(0.0),
            params: Vec4::new(clock, flash, self.glow, gain),
        }
    }
}

/// Gives every hole its disc, once. The sky cubes are filled in by `sky_cube` when the baked sky is ready (a hole drawn before
/// that bends nothing).
fn spawn_discs(
    mut commands: Commands,
    look: Res<Look>,
    time: Res<Time>,
    holes: Query<(Entity, &Pull, &GlobalTransform), (With<BlackHole>, Without<HoleLook>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<HoleMaterial>>,
) {
    let Some(b) = &look.black_holes else { return };
    for (e, pull, at) in &holes {
        let base = StandardMaterial { unlit: true, cull_mode: None, double_sided: true, alpha_mode: AlphaMode::Premultiplied, ..default() };
        let material =
            materials.add(HoleMaterial { base, extension: HoleGlow { u: b.uniform(at.translation(), pull, time.elapsed_secs(), 0.0, 0.0, false), cube: None, stars: None } });
        let flat = Transform::from_xyz(0.0, -b.drop * pull.radius, 0.0).with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
        commands.entity(e).insert(HoleLook).with_child((
            HoleDisc { flash: 0.0 },
            Mesh3d(meshes.add(Circle::new(pull.radius).mesh().resolution(64))),
            MeshMaterial3d(material),
            flat,
            bevy::light::NotShadowCaster,
        ));
    }
}

/// Moves the clock and the flash on and follows the hole.
fn update_discs(
    look: Res<Look>,
    time: Res<Time>,
    holes: Query<(&Pull, &GlobalTransform)>,
    mut discs: Query<(&ChildOf, &mut HoleDisc, &MeshMaterial3d<HoleMaterial>)>,
    mut materials: ResMut<Assets<HoleMaterial>>,
) {
    let Some(b) = &look.black_holes else { return };
    for (parent, mut disc, mat) in &mut discs {
        let Ok((pull, at)) = holes.get(parent.parent()) else { continue };
        disc.flash = (disc.flash - time.delta_secs() / FLASH_SECONDS).max(0.0);
        let Some(mut m) = materials.get_mut(&mat.0) else { continue };
        let (gain, lens) = (m.extension.u.params.w, m.extension.cube.is_some());
        m.extension.u = b.uniform(at.translation(), pull, time.elapsed_secs(), disc.flash * disc.flash, gain, lens);
    }
}

/// A ship the hole swallows inside its radius flashes the ring and the disc.
fn flash_on_swallow(new: Query<&GlobalTransform, Added<Swallowed>>, holes: Query<(&Pull, &GlobalTransform)>, mut discs: Query<(&ChildOf, &mut HoleDisc)>) {
    for ship in &new {
        for (parent, mut disc) in &mut discs {
            let Ok((pull, at)) = holes.get(parent.parent()) else { continue };
            let d = ship.translation() - at.translation();
            if Vec2::new(d.x, d.z).length() <= pull.radius {
                disc.flash = 1.0;
            }
        }
    }
}

/// Binds the baked sky (and its stars) to discs that have none yet, once the images are cubes.
fn sky_cube(
    look: Res<Look>,
    skies: Query<(&HdSky, Option<&super::sky::StarsImage>), With<Sky>>,
    discs: Query<&MeshMaterial3d<HoleMaterial>, With<HoleDisc>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<HoleMaterial>>,
) {
    let Some((hd, stars)) = skies.iter().next() else { return };
    let gain = look.sky.as_ref().and_then(|s| s.stars).map_or(16.0, |t| t.gain);
    let ready = |h: &Handle<Image>, images: &mut Assets<Image>| images.get_mut(h).is_some_and(|mut i| crate::view::as_cube(&mut i));
    for mat in &discs {
        let Some(m) = materials.get(&mat.0) else { continue };
        if m.extension.cube.as_ref() == Some(&hd.0) {
            continue;
        }
        let stars_handle = stars.map(|s| s.0.clone());
        if !ready(&hd.0, &mut images) || stars_handle.as_ref().is_some_and(|h| !ready(h, &mut images)) {
            continue;
        }
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.extension.cube = Some(hd.0.clone());
            m.extension.u.params.w = if stars_handle.is_some() { gain } else { 0.0 };
            m.extension.stars = stars_handle;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn absent_without_the_key_and_the_horizon_is_the_kill_core_by_default() {
        assert!(parse(&Value::Null).is_none());
        let b = parse(&json!({})).unwrap();
        let pull = Pull::for_terrain("Terrain_BlackHole_Med").unwrap();
        assert_eq!(b.horizon, pull.core() / pull.radius, "the black area is the kill zone");
        assert!(b.disc.inner > b.horizon && b.disc.outer > b.disc.inner && b.disc.outer <= 1.0);
    }

    #[test]
    fn values_are_clamped_and_disc_edges_keep_their_order() {
        let b = parse(&json!({"horizon": 3, "lensing": 99, "ring": -1, "disc": {"inner": 0.8, "outer": 0.1, "brightness": -2}})).unwrap();
        assert_eq!((b.horizon, b.lensing, b.ring, b.disc.brightness), (0.95, 4.0, 0.0, 0.0));
        assert!(b.disc.outer > b.disc.inner);
    }

    #[test]
    fn sizes_scale_with_the_pull_radius() {
        let b = parse(&json!({})).unwrap();
        let small = b.uniform(Vec3::ZERO, &Pull::for_terrain("Terrain_BlackHole_Small").unwrap(), 0.0, 0.0, 0.0, true);
        let big = b.uniform(Vec3::ZERO, &Pull::for_terrain("Terrain_BlackHole_Big").unwrap(), 0.0, 0.0, 0.0, false);
        assert_eq!(big.shape.w, 0.0, "no sky to bend, no lensing");
        assert_eq!(small.shape.x, small.shape.y, "the horizon is the core of a small hole");
        assert_eq!(big.shape.x, big.shape.y, "and of a big one");
        assert!(big.disc.y / small.disc.y > 4.0 && big.centre.w == 625.0);
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Shader>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Mesh>()
            .init_asset::<HoleMaterial>()
            .insert_resource(Look::parse(&json!({"black_holes": {}})))
            .add_systems(Update, spawn_discs);
        app
    }

    #[test]
    fn each_hole_gets_one_disc_that_follows_it_and_none_without_the_block() {
        let mut app = app();
        let pull = Pull::for_terrain("Terrain_BlackHole_Small").unwrap();
        let hole = app.world_mut().spawn((Transform::from_xyz(50.0, 0.0, 0.0), GlobalTransform::default(), pull)).id();
        app.update();
        assert!(!app.world().entity(hole).contains::<HoleLook>(), "a hole is only a BlackHole with the retail component");
        app.world_mut().entity_mut(hole).insert(BlackHole::default());
        app.update();
        app.update();
        let discs = app.world_mut().query::<(&ChildOf, &HoleDisc)>().iter(app.world()).filter(|(p, _)| p.parent() == hole).count();
        assert_eq!(discs, 1, "once, not every frame");
        assert_eq!(app.world().resource::<Assets<HoleMaterial>>().len(), 1);
        let mut none = App::new();
        none.add_plugins(MinimalPlugins).insert_resource(Look::parse(&json!({})));
        super::plugin(&mut none, &Look::parse(&json!({})));
        assert!(none.world().get_resource::<Assets<HoleMaterial>>().is_none(), "classic never sees it");
    }

    #[test]
    fn a_swallowed_ship_inside_the_radius_flashes_the_disc() {
        let mut app = app();
        app.add_systems(Update, (update_discs, flash_on_swallow).chain().after(spawn_discs));
        let pull = Pull::for_terrain("Terrain_BlackHole_Small").unwrap();
        let hole = app.world_mut().spawn((Transform::default(), GlobalTransform::default(), pull, BlackHole::default())).id();
        app.update();
        app.world_mut().spawn((GlobalTransform::from_xyz(pull.radius * 0.5, 0.0, 0.0), Swallowed));
        app.world_mut().spawn((GlobalTransform::from_xyz(pull.radius * 3.0, 0.0, 0.0), Swallowed));
        app.update();
        let flash = app.world_mut().query::<(&ChildOf, &HoleDisc)>().iter(app.world()).find(|(p, _)| p.parent() == hole).map(|(_, d)| d.flash).unwrap();
        assert!(flash > 0.9, "{flash}");
    }
}
