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

/// Seconds the ring and disc flash when a hole swallows a ship. Engine choice, not the original's.
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
    /// x: swallow flash 0..1, y: glow (the clock is the frame data's, `frame.rs`)
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
    /// The frame data (`frame.rs`) at `@binding(99)`.
    #[storage(99, read_only)]
    pub frame: super::frame::FrameBinding,
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
    /// The flash the material was last given, so a quiet disc is never touched.
    shown: f32,
}

impl BlackHoles {
    fn uniform(&self, centre: Vec3, pull: &Pull, flash: f32) -> HoleUniform {
        let r = pull.radius;
        HoleUniform {
            centre: centre.extend(r),
            shape: Vec4::new(self.horizon * r, pull.core(), self.ring * r, 0.0),
            disc: Vec4::new(self.disc.inner * r, self.disc.outer * r, self.disc.spin, self.disc.brightness),
            color: self.disc.color.extend(0.0),
            params: Vec4::new(flash, self.glow, 0.0, 0.0),
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
    /// rgb: the cooler colour (the clock is the frame data's, `frame.rs`)
    pub color: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct RibbonGlow {
    #[uniform(100)]
    pub u: RibbonUniform,
    /// The frame data (`frame.rs`) at `@binding(99)`.
    #[storage(99, read_only)]
    pub frame: super::frame::FrameBinding,
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
    fn ribbon(&self, centre: Vec3, pull: &Pull, gain: f32) -> RibbonUniform {
        RibbonUniform {
            centre: centre.extend(pull.radius),
            shape: Vec4::new(pull.core(), self.ribbons * gain, self.depth * pull.radius, self.throat * pull.radius),
            color: self.ribbon_color.extend(0.0),
        }
    }
}

/// With `replace`, the hole's own vortex strips (the retail geometry, blackhole.rs) swap their broad soft additive material for the
/// thin, streaking infall one (`shaders/ribbon.wesl`), once. Without it they only get the `fx_gain` (`ribbon_gain`).
fn restyle_ribbons(
    mut commands: Commands,
    look: Res<Look>,
    holes: Query<(Entity, &Pull, &GlobalTransform), (With<BlackHole>, With<MeshMaterial3d<StandardMaterial>>, Without<RibbonStyled>)>,
    mut materials: ResMut<Assets<RibbonMaterial>>,
) {
    let Some(b) = look.black_holes.as_ref().filter(|b| b.replace) else { return };
    for (e, pull, at) in &holes {
        let handle =
            materials.add(RibbonMaterial { base: super::kit::additive_base(None), extension: RibbonGlow { u: b.ribbon(at.translation(), pull, look.fx_gain), frame: default() } });
        super::kit::swap::<StandardMaterial, _>(&mut commands, e, handle).try_insert(RibbonStyled);
    }
}

/// Follows a hole that moves (the clock is the frame data's, so a hole that stays put never touches its material).
fn update_ribbons(
    look: Res<Look>,
    holes: Query<(&Pull, &GlobalTransform, &MeshMaterial3d<RibbonMaterial>), (With<BlackHole>, Changed<GlobalTransform>)>,
    mut materials: ResMut<Assets<RibbonMaterial>>,
) {
    let Some(b) = &look.black_holes else { return };
    for (pull, at, mat) in &holes {
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.extension.u = b.ribbon(at.translation(), pull, look.fx_gain);
        }
    }
}

/// Gives every hole its disc, once.
fn spawn_discs(
    mut commands: Commands,
    look: Res<Look>,
    holes: Query<(Entity, &Pull, &GlobalTransform), (With<BlackHole>, Without<HoleLook>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<HoleMaterial>>,
) {
    let Some(b) = &look.black_holes else { return };
    for (e, pull, at) in &holes {
        let base = StandardMaterial { unlit: true, cull_mode: None, double_sided: true, alpha_mode: AlphaMode::Premultiplied, ..default() };
        let material = materials.add(HoleMaterial { base, extension: HoleGlow { u: b.uniform(at.translation(), pull, 0.0), frame: default() } });
        let (mesh, tf) = if b.depth > 0.0 {
            (funnel_mesh(pull.radius, b.depth, b.throat), Transform::from_xyz(0.0, -b.drop * pull.radius, 0.0))
        } else {
            let flat = Transform::from_xyz(0.0, -b.drop * pull.radius, 0.0).with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
            (Circle::new(pull.radius).mesh().resolution(64).build(), flat)
        };
        commands.entity(e).insert(HoleLook).with_child((HoleDisc { flash: 0.0, shown: 0.0 }, Mesh3d(meshes.add(mesh)), MeshMaterial3d(material), tf, bevy::light::NotShadowCaster));
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

/// Moves the flash on and follows the hole; the clock is the frame data's, so the material is only touched while a flash fades
/// (and once more to put it out) or the hole moves.
fn update_discs(
    look: Res<Look>,
    time: Res<Time>,
    holes: Query<(Ref<GlobalTransform>, &Pull)>,
    mut discs: Query<(&ChildOf, &mut HoleDisc, &MeshMaterial3d<HoleMaterial>)>,
    mut materials: ResMut<Assets<HoleMaterial>>,
) {
    let Some(b) = &look.black_holes else { return };
    for (parent, mut disc, mat) in &mut discs {
        let Ok((at, pull)) = holes.get(parent.parent()) else { continue };
        disc.flash = (disc.flash - time.delta_secs() / FLASH_SECONDS).max(0.0);
        let flash = disc.flash * disc.flash;
        if flash == disc.shown && !at.is_changed() {
            continue;
        }
        let Some(mut m) = materials.get_mut(&mat.0) else { continue };
        m.extension.u = b.uniform(at.translation(), pull, flash);
        disc.shown = flash;
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
mod tests;
