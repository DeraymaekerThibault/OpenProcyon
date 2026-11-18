//! Not retail: black holes of the render profile (`black_holes` block, drawn with the mod's `shaders/blackhole.wesl`). The retail
//! look is only its additive ribbons (`blackhole.rs`); this puts a flat disc of the pull's radius in the hole's plane, just under
//! the ships: a black horizon exactly as wide as the kill core (`Pull::core`), a thin bright ring on its edge that marks the kill
//! zone at any zoom, a hot accretion disc that turns the way the pull swirls (clockwise seen from above), the faint outline of
//! the pull radius, and a flash when a ship is swallowed. Without `replace` the retail ribbons stay on top as the infalling streams; with
//! it the retail glow and beam layers are hidden (`replace.rs`) and the hole's own vortex strips are restyled as thin, hot infall
//! threads (`shaders/ribbon.wesl`). The disc is thin and crisp (hard inner edge, fine arms, sharp photon ring). All sizes are
//! shares of the pull radius, so the three hole sizes scale together.
//!
//! The bending of the sky (`lensing`) and the hole's round shadow are drawn by the sky dome (`sky.rs` reads the holes), so they
//! bend the clouds and stars too and always stay behind the ships; with no dome there is no bending.
//!
//! Only reads `blackhole::BlackHole`, `blackhole::Pull` and `blackhole::Swallowed`; nothing here changes what the hole does.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use serde_json::Value;

use super::{Look, num, rgb};
use crate::blackhole::{BlackHole, Pull, Swallowed};

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
    /// How hard the hole bends the sky behind it (1 = a radian at the horizon's edge); drawn by the sky dome.
    pub lensing: f32,
    pub disc: Disc,
    /// Warm halo just outside the horizon, strength.
    pub glow: f32,
    /// How far under the hole's plane the disc lies, a share of the radius, so hulls in the plane draw over it.
    pub drop: f32,
    /// The retail glow, beam and ribbon layers are hidden (the hole keeps pulling and its trail keeps running) and the profile
    /// draws the hole (`replace.rs`).
    pub replace: bool,
    /// With `replace`: brightness of the infall ribbons (the thin streaking strips), times `fx_gain`.
    pub ribbons: f32,
    /// With `replace`: the infall ribbons' cooler colour (linear); they run hotter toward the core.
    pub ribbon_color: Vec3,
    /// The whirlpool (`funnel.depth`): how deep under the hole's plane the funnel sinks at its throat, a share of the radius
    /// (0 = a flat disc in the plane). The bent sky and the hole's shadow are centred down there too, so the plane the ships
    /// sail stays unbent and the hole reads as a maelstrom to keep out of, not a sphere in the battle.
    pub depth: f32,
    /// The funnel's throat radius, a share of the radius.
    pub throat: f32,
    /// Colour of the whirlpool's outer water, faint at the pull radius (linear).
    pub rim: Vec3,
}

/// How far down (0..1 of the depth) the whirlpool's funnel is at `share` of the pull radius: a gravity well, nearly flat at the
/// rim and plunging toward the throat (1 / r), all the way down inside the throat. `shaders/blackhole.wesl` and `ribbon.wesl`
/// repeat it.
pub fn funnel_drop(share: f32, throat: f32) -> f32 {
    if share <= throat {
        return 1.0;
    }
    ((1.0 / share - 1.0) / (1.0 / throat - 1.0)).clamp(0.0, 1.0)
}

/// The whirlpool's funnel around the hole's centre, in the hole's frame (y up): rings from the pull radius in to the throat,
/// spaced on a log scale so the steep part near the core is fine, and a cap closing the throat.
fn funnel_mesh(radius: f32, depth: f32, throat: f32) -> Mesh {
    const RINGS: usize = 56;
    const SEGS: usize = 128;
    let height = |share: f32| -depth * radius * funnel_drop(share, throat);
    let (mut pos, mut nor, mut uv, mut idx) = (vec![], vec![], vec![], Vec::<u32>::new());
    for i in 0..RINGS {
        let share = throat.powf(i as f32 / (RINGS - 1) as f32);
        let e = share * 0.01;
        let slope = (height(share + e) - height(share - e)) / (2.0 * e * radius);
        for j in 0..=SEGS {
            let a = j as f32 / SEGS as f32 * std::f32::consts::TAU;
            let (sin, cos) = a.sin_cos();
            pos.push([radius * share * cos, height(share), radius * share * sin]);
            nor.push(Vec3::new(-slope * cos, 1.0, -slope * sin).normalize().to_array());
            uv.push([j as f32 / SEGS as f32, i as f32 / (RINGS - 1) as f32]);
        }
    }
    for i in 0..RINGS - 1 {
        for j in 0..SEGS {
            let (a, b) = ((i * (SEGS + 1) + j) as u32, ((i + 1) * (SEGS + 1) + j) as u32);
            idx.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    let centre = pos.len() as u32;
    pos.push([0.0, height(0.0), 0.0]);
    nor.push([0.0, 1.0, 0.0]);
    uv.push([0.5, 1.0]);
    let last = ((RINGS - 1) * (SEGS + 1)) as u32;
    for j in 0..SEGS as u32 {
        idx.extend([last + j, centre, last + j + 1]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx))
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
            replace: v["replace"].as_bool().unwrap_or(false),
            ribbons: num(&v["ribbons"], 1.0).clamp(0.0, 8.0),
            ribbon_color: rgb(&v["ribbon_color"]).unwrap_or(Vec3::new(1.0, 0.38, 0.12)).max(Vec3::ZERO),
            depth: num(&v["funnel"]["depth"], 0.0).clamp(0.0, 3.0),
            throat: num(&v["funnel"]["throat"], 0.06).clamp(0.01, 0.5),
            rim: rgb(&v["funnel"]["rim"]).unwrap_or(Vec3::new(0.3, 0.18, 0.65)).max(Vec3::ZERO),
        }
    })
}

/// What the black hole shader gets at `@group(MATERIAL_BIND_GROUP) @binding(100)`.
#[derive(Clone, Copy, Default, Debug, PartialEq, ShaderType)]
pub struct HoleUniform {
    /// xyz: the hole's centre (world), w: the pull radius
    pub centre: Vec4,
    /// x: horizon radius (world units), y: the kill core's radius, z: ring thickness (world units)
    pub shape: Vec4,
    /// x: disc inner radius, y: outer radius (world units), z: spin (turns a second), w: brightness
    pub disc: Vec4,
    /// rgb: the disc's cooler colour
    pub color: Vec4,
    /// x: clock (battle seconds), y: swallow flash 0..1, z: glow
    pub params: Vec4,
    /// x: the funnel's depth, y: its throat radius, z: how far under the plane its rim lies (world units; x = 0: flat)
    pub funnel: Vec4,
    /// rgb: the outer water's colour
    pub rim: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct HoleGlow {
    #[uniform(100)]
    pub u: HoleUniform,
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
        // under the ships: no depth write, and the ships' own depth keeps them in front
        super::kit::no_depth_write(descriptor);
        Ok(())
    }
}

pub type HoleMaterial = ExtendedMaterial<StandardMaterial, HoleGlow>;

pub fn plugin(app: &mut App, look: &Look) {
    if look.black_holes.is_some() {
        app.add_plugins((MaterialPlugin::<HoleMaterial>::default(), MaterialPlugin::<RibbonMaterial>::default()))
            .add_systems(Update, (spawn_discs, update_discs, flash_on_swallow, ribbon_gain, restyle_ribbons, update_ribbons).chain());
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
    fn uniform(&self, centre: Vec3, pull: &Pull, clock: f32, flash: f32) -> HoleUniform {
        let r = pull.radius;
        HoleUniform {
            centre: centre.extend(r),
            shape: Vec4::new(self.horizon * r, pull.core(), self.ring * r, 0.0),
            disc: Vec4::new(self.disc.inner * r, self.disc.outer * r, self.disc.spin, self.disc.brightness),
            color: self.disc.color.extend(0.0),
            params: Vec4::new(clock, flash, self.glow, 0.0),
            funnel: Vec4::new(self.depth * r, self.throat * r, self.drop * r, 0.0),
            rim: self.rim.extend(0.0),
        }
    }

    /// Where the hole's bent sky and shadow are centred: down at the bottom of the whirlpool's throat (the hole itself when flat).
    pub fn singularity(&self, centre: Vec3, pull: &Pull) -> Vec3 {
        if self.depth <= 0.0 {
            return centre;
        }
        centre - Vec3::Y * ((self.depth + self.drop) * pull.radius)
    }

    /// The radius of the hole's round shadow and of the bending around it: the kill core for a flat hole, the throat for a
    /// whirlpool (whose throat is already black, so the shadow is only the point everything falls toward).
    pub fn shadow_core(&self, pull: &Pull) -> f32 {
        if self.depth > 0.0 { self.throat * pull.radius } else { pull.core() }
    }
}

pub const RIBBON_SHADER: &str = "shaders/ribbon.wesl";

/// What the ribbon shader gets at `@group(MATERIAL_BIND_GROUP) @binding(100)`.
#[derive(Clone, Copy, Default, Debug, PartialEq, ShaderType)]
pub struct RibbonUniform {
    /// xyz: the hole's centre (world), w: the pull radius
    pub centre: Vec4,
    /// x: the kill core's radius, y: brightness, z: the whirlpool's depth, w: its throat radius (world units)
    pub shape: Vec4,
    /// rgb: the cooler colour
    pub color: Vec4,
    /// x: clock (battle seconds)
    pub params: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct RibbonGlow {
    #[uniform(100)]
    pub u: RibbonUniform,
}

impl MaterialExtension for RibbonGlow {
    fn fragment_shader() -> ShaderRef {
        RIBBON_SHADER.into()
    }
    // the strips sink along the whirlpool's funnel
    fn vertex_shader() -> ShaderRef {
        RIBBON_SHADER.into()
    }
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        super::kit::additive(descriptor);
        Ok(())
    }
}

pub type RibbonMaterial = ExtendedMaterial<StandardMaterial, RibbonGlow>;

/// A hole whose infall ribbons have the profile's material.
#[derive(Component)]
pub struct RibbonStyled;

impl BlackHoles {
    fn ribbon(&self, centre: Vec3, pull: &Pull, gain: f32, clock: f32) -> RibbonUniform {
        RibbonUniform {
            centre: centre.extend(pull.radius),
            shape: Vec4::new(pull.core(), self.ribbons * gain, self.depth * pull.radius, self.throat * pull.radius),
            color: self.ribbon_color.extend(0.0),
            params: Vec4::new(clock, 0.0, 0.0, 0.0),
        }
    }
}

/// With `replace`, the hole's own vortex strips (the retail geometry, blackhole.rs) swap their broad soft additive material for the
/// thin, streaking infall one (`shaders/ribbon.wesl`), once. Without it they only get the `fx_gain` (`ribbon_gain`).
fn restyle_ribbons(
    mut commands: Commands,
    look: Res<Look>,
    time: Res<Time>,
    holes: Query<(Entity, &Pull, &GlobalTransform), (With<BlackHole>, With<MeshMaterial3d<StandardMaterial>>, Without<RibbonStyled>)>,
    mut materials: ResMut<Assets<RibbonMaterial>>,
) {
    let Some(b) = look.black_holes.as_ref().filter(|b| b.replace) else { return };
    for (e, pull, at) in &holes {
        let handle = materials
            .add(RibbonMaterial { base: super::kit::additive_base(None), extension: RibbonGlow { u: b.ribbon(at.translation(), pull, look.fx_gain, time.elapsed_secs()) } });
        super::kit::swap::<StandardMaterial, _>(&mut commands, e, handle).try_insert(RibbonStyled);
    }
}

fn update_ribbons(
    look: Res<Look>,
    time: Res<Time>,
    holes: Query<(&Pull, &GlobalTransform, &MeshMaterial3d<RibbonMaterial>), With<BlackHole>>,
    mut materials: ResMut<Assets<RibbonMaterial>>,
) {
    let Some(b) = &look.black_holes else { return };
    for (pull, at, mat) in &holes {
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.extension.u = b.ribbon(at.translation(), pull, look.fx_gain, time.elapsed_secs());
        }
    }
}

/// Gives every hole its disc, once.
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
        let material = materials.add(HoleMaterial { base, extension: HoleGlow { u: b.uniform(at.translation(), pull, time.elapsed_secs(), 0.0) } });
        let (mesh, tf) = if b.depth > 0.0 {
            (funnel_mesh(pull.radius, b.depth, b.throat), Transform::from_xyz(0.0, -b.drop * pull.radius, 0.0))
        } else {
            let flat = Transform::from_xyz(0.0, -b.drop * pull.radius, 0.0).with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
            (Circle::new(pull.radius).mesh().resolution(64).build(), flat)
        };
        commands.entity(e).insert(HoleLook).with_child((HoleDisc { flash: 0.0 }, Mesh3d(meshes.add(mesh)), MeshMaterial3d(material), tf, bevy::light::NotShadowCaster));
    }
}

/// The hole's own infall ribbons (`blackhole.rs` draws them with a plain additive StandardMaterial, which `fx_gain` does not
/// reach) get the profile's `fx_gain` too, so they pass the bloom threshold like every other additive effect. Once per material.
fn ribbon_gain(
    look: Res<Look>,
    holes: Query<&MeshMaterial3d<StandardMaterial>, (With<BlackHole>, Added<MeshMaterial3d<StandardMaterial>>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if look.black_holes.as_ref().is_some_and(|b| b.replace) {
        return;
    }
    for mat in &holes {
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.base_color = Color::linear_rgb(look.fx_gain, look.fx_gain, look.fx_gain);
        }
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
        m.extension.u = b.uniform(at.translation(), pull, time.elapsed_secs(), disc.flash * disc.flash);
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
        assert!(!b.replace && parse(&json!({"replace": true})).unwrap().replace);
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
        let small = b.uniform(Vec3::ZERO, &Pull::for_terrain("Terrain_BlackHole_Small").unwrap(), 0.0, 0.0);
        let big = b.uniform(Vec3::ZERO, &Pull::for_terrain("Terrain_BlackHole_Big").unwrap(), 0.0, 0.0);
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
    fn the_holes_own_ribbons_get_the_fx_gain() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<StandardMaterial>()
            .insert_resource(Look::parse(&json!({"black_holes": {}, "fx_gain": 1.6})))
            .add_systems(Update, ribbon_gain);
        let mat = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { alpha_mode: AlphaMode::Add, unlit: true, ..default() });
        let other = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial::default());
        app.world_mut().spawn((BlackHole::default(), MeshMaterial3d(mat.clone())));
        app.world_mut().spawn(MeshMaterial3d(other.clone()));
        app.update();
        let mats = app.world().resource::<Assets<StandardMaterial>>();
        assert_eq!(mats.get(&mat).unwrap().base_color.to_linear().red, 1.6);
        assert_eq!(mats.get(&other).unwrap().base_color, Color::WHITE, "only the hole's ribbons");
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

    #[test]
    fn the_whirlpool_is_flat_at_the_rim_and_all_the_way_down_in_the_throat() {
        assert_eq!(funnel_drop(1.0, 0.06), 0.0);
        assert_eq!(funnel_drop(0.06, 0.06), 1.0);
        assert_eq!(funnel_drop(0.01, 0.06), 1.0);
        let (outer, core) = (funnel_drop(0.6, 0.06), funnel_drop(0.25, 0.06));
        assert!(outer < 0.05 && core > 3.0 * outer && core < 0.5, "nearly flat out where ships sail, plunging only near the core: {outer} {core}");
        let m = funnel_mesh(100.0, 1.2, 0.06);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        assert!(p.iter().all(|q| q[1] <= 0.0 && q[1] >= -120.0 - 1e-3), "never above the plane, never below the throat");
        assert!(p.iter().map(|q| q[0].hypot(q[2])).fold(0.0, f32::max) <= 100.0 + 1e-3);
    }

    #[test]
    fn the_sky_bends_around_the_bottom_of_the_throat_and_flat_holes_stay_in_the_plane() {
        let pull = Pull::for_terrain("Terrain_BlackHole_Med").unwrap();
        let at = Vec3::new(10.0, 0.0, 20.0);
        assert_eq!(parse(&json!({"drop": 0.0})).unwrap().singularity(at, &pull), at);
        let deep = parse(&json!({"drop": 0.0, "funnel": {"depth": 1.0}})).unwrap().singularity(at, &pull);
        assert_eq!(deep, at - Vec3::Y * pull.radius);
        assert_eq!(parse(&json!({})).unwrap().shadow_core(&pull), pull.core());
        assert!(parse(&json!({"funnel": {"depth": 1.0}})).unwrap().shadow_core(&pull) < pull.core() * 0.5);
    }

    #[test]
    fn the_ribbons_colour_comes_from_the_profile() {
        let pull = Pull::for_terrain("Terrain_BlackHole_Med").unwrap();
        let default = parse(&json!({})).unwrap().ribbon(Vec3::ZERO, &pull, 1.0, 0.0);
        assert_eq!(default.color, Vec4::new(1.0, 0.38, 0.12, 0.0));
        let own = parse(&json!({"ribbon_color": [0.2, 0.5, 1.0]})).unwrap().ribbon(Vec3::ZERO, &pull, 1.0, 0.0);
        assert_eq!(own.color, Vec4::new(0.2, 0.5, 1.0, 0.0));
    }

    #[test]
    fn ribbons_default_to_full_brightness_and_are_clamped() {
        assert_eq!(parse(&json!({})).unwrap().ribbons, 1.0);
        assert_eq!(parse(&json!({"ribbons": 99})).unwrap().ribbons, 8.0);
    }

    fn ribbon_app(look: serde_json::Value) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Shader>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .init_asset::<RibbonMaterial>()
            .insert_resource(Look::parse(&look))
            .add_systems(Update, (ribbon_gain, restyle_ribbons, update_ribbons).chain());
        let mat = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { alpha_mode: AlphaMode::Add, unlit: true, ..default() });
        let pull = Pull::for_terrain("Terrain_BlackHole_Small").unwrap();
        let hole = app.world_mut().spawn((BlackHole::default(), pull, GlobalTransform::from_xyz(7.0, 0.0, 0.0), MeshMaterial3d(mat))).id();
        (app, hole)
    }

    #[test]
    fn with_replace_the_vortex_strips_swap_to_the_thin_material_once_and_keep_their_mesh() {
        let (mut app, hole) = ribbon_app(json!({"black_holes": {"replace": true, "ribbons": 2}, "fx_gain": 1.5}));
        for _ in 0..3 {
            app.update();
        }
        let e = app.world().entity(hole);
        assert!(e.contains::<RibbonStyled>() && e.contains::<MeshMaterial3d<RibbonMaterial>>() && !e.contains::<MeshMaterial3d<StandardMaterial>>());
        assert!(e.contains::<BlackHole>(), "the hole and its trail sim are untouched");
        let mats = app.world().resource::<Assets<RibbonMaterial>>();
        assert_eq!(mats.len(), 1, "once");
        let u = mats.iter().next().unwrap().1.extension.u;
        assert_eq!((u.centre, u.shape.x, u.shape.y), (Vec4::new(7.0, 0.0, 0.0, 145.0), 145.0 / 4.0, 3.0), "the kill core and brightness times fx_gain");
    }

    #[test]
    fn without_replace_the_strips_keep_the_retail_material_with_only_the_gain() {
        let (mut app, hole) = ribbon_app(json!({"black_holes": {}, "fx_gain": 1.5}));
        for _ in 0..3 {
            app.update();
        }
        let e = app.world().entity(hole);
        assert!(!e.contains::<RibbonStyled>() && e.contains::<MeshMaterial3d<StandardMaterial>>());
        let mat = e.get::<MeshMaterial3d<StandardMaterial>>().unwrap();
        assert_eq!(app.world().resource::<Assets<StandardMaterial>>().get(&mat.0).unwrap().base_color.to_linear().red, 1.5);
    }
}
