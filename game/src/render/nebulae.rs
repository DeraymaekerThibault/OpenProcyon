//! Not retail: nebula clouds and solar storms of the render profile (`nebulae` block, drawn with the mod's
//! `shaders/nebula.wesl`). The retail cloud is one additive textured shell whose vertices breathe (`nebula.rs`: the shifting
//! sim, the colour loop and the edge fade, written into the vertex colours every tick). That stays as the envelope; this swaps
//! the shell's material for one whose shader also marches a short ray through the cloud's ellipsoid and adds glowing, churning
//! gas (domain-warped 3-D noise) to it. Added light only (ONE/ONE), so it can never hide a ship. A bolt of storm lightning
//! (`nebula::StormFlash`) lights the gas around it for a moment from inside. With the camera inside the cloud the far wall
//! of the shell is marched from the camera, so the gas fills the screen gently (`inside` caps it).
//!
//!
//! With `replace` (`replace.rs` hides the retail shell) the cloud is a volume instead (`shaders/volume.wesl`): a proxy box child of the
//! `Cloud`, drawn on its back faces, ray-marches the cloud's ellipsoid with emission and absorption, composited premultiplied, ending
//! at the depth prepass, lit by the sun with a cheap self-shadow, tinted by the retail colour loop (`Cloud::colour`) and sized by the
//! retail breath (`Cloud::breath`), against a baked tiling noise (`volnoise.rs`). A bolt lights it along its whole segment.
//!
//! Only reads `nebula::Cloud`; nothing here changes what a cloud does.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, CompareFunction, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use serde_json::Value;

use super::frame::Frame;
use super::volnoise::NoiseVolume;
use super::{Look, num, rgb};
use crate::fx::EffectMaterial;
use crate::nebula::Cloud;

pub const SHADER: &str = "shaders/nebula.wesl";

/// Most steps a ray takes through a cloud.
pub const MAX_STEPS: f32 = 24.0;

/// Half the cloud's size along each axis, shares of its radius (`docs/re/nebula-landmark.md`; the same box for a solar storm's
/// mesh, PROVISIONAL).
const AXES: Vec3 = Vec3::new(1.0, 0.7, 0.99);

/// Seconds a bolt's light lasts inside the cloud.
const FLASH_SECONDS: f32 = 0.3;

/// The `nebulae` block.
#[derive(Clone, Debug, PartialEq)]
pub struct Nebulae {
    pub rules: Vec<Rule>,
}

/// How the clouds whose effect name contains `matching` are drawn (first rule that matches wins).
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    /// A fragment of the cloud's effect name (`Nebula`, `SolarStorm`), lower case.
    pub matching: String,
    /// Strength of the gas; above 1 the cores bloom.
    pub density: f32,
    /// Ray steps through the cloud (4..=24).
    pub steps: f32,
    /// Noise patches across the cloud.
    pub scale: f32,
    /// How fast the gas churns (noise units a second).
    pub churn: f32,
    pub color_low: Vec3,
    pub color_high: Vec3,
    /// How much of the retail shell (its texture and edge fade) stays under the gas.
    pub shell: f32,
    /// Share of the gas drawn when the camera is inside the cloud (keeps the battle readable).
    pub inside: f32,
    /// How bright a bolt's light is in the gas, and how far it reaches (a share of the radius).
    pub flash: f32,
    pub flash_reach: f32,
    /// The retail shell is hidden (the cloud keeps running) and the profile draws the cloud itself (`replace.rs`).
    pub replace: bool,
    /// Replacing rules only: how opaque dense gas is (optical depth across a radius at full density), 0 = glow only.
    pub absorption: f32,
    /// Replacing rules only: strength of the dark dust lanes (extinction without light).
    pub dust: f32,
    /// Replacing rules only: most opacity one cloud reaches (a ship is never fully hidden).
    pub max_opacity: f32,
    /// Replacing rules only: steps toward the sun for the self-shadow (0..=3).
    pub light_steps: f32,
    /// Replacing rules only: how strongly the sun lights the gas.
    pub sun: f32,
    /// Replacing rules only: HDR emission from dense pockets, separate from the veil's opacity.
    pub core_glow: f32,
    /// Replacing rules only: a second hue the gas drifts into across the cloud (large patches), and how far (0..1), so a cloud
    /// is not one flat colour.
    pub color_alt: Vec3,
    pub variation: f32,
    /// Replacing rules only, for storms: how fast the gas wheels round the centre like a cyclone (turns a second near the
    /// centre, slower outward), how bright the hot plasma filaments are, and how bright the flickers of lightning inside it.
    pub swirl: f32,
    pub filaments: f32,
    pub flicker: f32,
}

pub fn parse(v: &Value) -> Option<Nebulae> {
    let all = v["replace"].as_bool().unwrap_or(false);
    v.is_object().then(|| Nebulae {
        rules: v["rules"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|r| {
                        Some(Rule {
                            matching: r["match"].as_str()?.to_lowercase(),
                            density: num(&r["density"], 1.0).clamp(0.0, 8.0),
                            steps: num(&r["steps"], 16.0).clamp(4.0, MAX_STEPS).round(),
                            scale: num(&r["scale"], 2.2).clamp(0.2, 12.0),
                            churn: num(&r["churn"], 0.02).clamp(0.0, 1.0),
                            color_low: rgb(&r["color_low"]).unwrap_or(Vec3::new(0.35, 0.15, 0.6)).max(Vec3::ZERO),
                            color_high: rgb(&r["color_high"]).unwrap_or(Vec3::new(1.0, 0.85, 1.0)).max(Vec3::ZERO),
                            shell: num(&r["shell"], 0.5).clamp(0.0, 1.0),
                            inside: num(&r["inside"], 0.6).clamp(0.0, 1.0),
                            flash: num(&r["flash"], 3.0).max(0.0),
                            flash_reach: num(&r["flash_reach"], 0.5).clamp(0.05, 2.0),
                            replace: r["replace"].as_bool().unwrap_or(all),
                            absorption: num(&r["absorption"], 3.0).clamp(0.0, 20.0),
                            dust: num(&r["dust"], 0.6).clamp(0.0, 4.0),
                            max_opacity: num(&r["max_opacity"], 0.6).clamp(0.05, 1.0),
                            light_steps: num(&r["light_steps"], 2.0).clamp(0.0, 3.0).round(),
                            sun: num(&r["sun"], 1.0).clamp(0.0, 4.0),
                            core_glow: num(&r["core_glow"], 0.0).clamp(0.0, 12.0),
                            color_alt: rgb(&r["color_alt"]).unwrap_or(Vec3::ONE).max(Vec3::ZERO),
                            variation: num(&r["variation"], 0.0).clamp(0.0, 1.0),
                            swirl: num(&r["swirl"], 0.0).clamp(-1.0, 1.0),
                            filaments: num(&r["filaments"], 0.0).clamp(0.0, 8.0),
                            flicker: num(&r["flicker"], 0.0).clamp(0.0, 8.0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

impl Nebulae {
    /// Whether a rule for a solar storm replaces the retail look (its bolts are then drawn by the profile).
    pub fn replaces_storms(&self) -> bool {
        self.rules.iter().any(|r| r.replace && "solarstorm".contains(&r.matching))
    }

    /// The rule for a cloud made by the effect `effect`, if any.
    pub fn find(&self, effect: &str) -> Option<&Rule> {
        let e = effect.to_lowercase();
        self.rules.iter().find(|r| e.contains(&r.matching))
    }
}

/// What the nebula shader gets at `@group(MATERIAL_BIND_GROUP) @binding(100)`, besides the StandardMaterial bindings.
#[derive(Clone, Copy, Default, Debug, PartialEq, ShaderType)]
pub struct NebulaUniform {
    /// xyz: the cloud's centre (world), w: its radius
    pub centre: Vec4,
    /// xyz: half-size per axis as shares of the radius, w: ray steps
    pub axes: Vec4,
    /// x: density, y: noise scale, z: churn, w: share of the retail shell kept
    pub look: Vec4,
    /// rgb: colour of thin gas, w: share of the gas drawn with the camera inside
    pub low: Vec4,
    /// rgb: colour of dense gas, w: the bolt's brightness in the gas
    pub high: Vec4,
    /// xyz: where the last bolt struck (world), w: how much of its light is left (0 = none)
    pub flash: Vec4,
    /// x: clock (battle seconds), y: how far a bolt reaches (world units)
    pub params: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct NebulaGlow {
    #[uniform(100)]
    pub u: NebulaUniform,
    /// The frame data (`frame.rs`) at `@binding(99)`.
    #[storage(99, read_only)]
    pub frame: super::frame::FrameBinding,
}

impl MaterialExtension for NebulaGlow {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
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

pub type NebulaMaterial = ExtendedMaterial<StandardMaterial, NebulaGlow>;

pub const VOLUME_SHADER: &str = "shaders/volume.wesl";

/// How far the proxy box reaches past the ellipsoid (the shell breathes up to 1.15 times its size; the box bounds the march).
const BOX_MARGIN: f32 = 1.25;

/// What the volume shader gets at `@group(MATERIAL_BIND_GROUP) @binding(100)`.
#[derive(Clone, Copy, Default, Debug, PartialEq, ShaderType)]
pub struct VolumeUniform {
    /// xyz: the cloud's centre (world), w: its radius
    pub centre: Vec4,
    /// xyz: half-size per axis as shares of the radius (the retail breath applied), w: most ray steps
    pub axes: Vec4,
    /// x: emission, y: noise scale, z: churn, w: absorption
    pub look: Vec4,
    /// rgb: thin gas, w: most opacity
    pub low: Vec4,
    /// rgb: dense gas, w: dust
    pub high: Vec4,
    /// rgb: the retail colour loop, w: steps toward the sun
    pub tint: Vec4,
    /// xyz: toward the sun, w: its strength
    pub sun: Vec4,
    /// rgb: the sun's colour, w: share of the gas kept with the camera inside
    pub sun_color: Vec4,
    /// xyz: one end of the last bolt, w: how much of its light is left
    pub flash: Vec4,
    /// xyz: its other end
    pub flash_to: Vec4,
    /// x: clock, y: a bolt's reach (world units), z: a bolt's brightness, w: the mission's fadeout factor
    pub params: Vec4,
    /// x: swirl, y: filaments, z: flicker, w: hue variation
    pub storm: Vec4,
    /// rgb: the second hue; w: HDR core glow
    pub alt: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct VolumeGas {
    #[uniform(100)]
    pub u: VolumeUniform,
    /// The frame data (`frame.rs`) at `@binding(99)`.
    #[storage(99, read_only)]
    pub frame: super::frame::FrameBinding,
    /// The baked noise (`volnoise.rs`), one for every cloud.
    #[texture(101, dimension = "3d")]
    #[sampler(102)]
    pub noise: Handle<Image>,
}

impl MaterialExtension for VolumeGas {
    fn fragment_shader() -> ShaderRef {
        VOLUME_SHADER.into()
    }
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // the shader ends its own rays at the scene's depth, so the proxy's back faces take no depth test and write none
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
            depth.depth_compare = Some(CompareFunction::Always);
        }
        Ok(())
    }
}

pub type VolumeMaterial = ExtendedMaterial<StandardMaterial, VolumeGas>;

/// The proxy box of a cloud the profile draws as a volume: the child of the `Cloud`, which keeps its retail shell hidden.
#[derive(Component)]
pub struct VolumeProxy;

/// The sun the clouds are lit by: the direction toward it, and its colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunLight {
    pub toward: Vec3,
    pub color: Vec3,
}

impl Default for SunLight {
    fn default() -> Self {
        SunLight { toward: Vec3::Y, color: Vec3::ONE }
    }
}

impl Rule {
    fn volume(&self, cloud: &Cloud, centre: Vec3, clock: f32, bolt: &Bolt, sun: SunLight) -> VolumeUniform {
        let radius = cloud.radius();
        let colour = sun.color / sun.color.max_element().max(1e-3);
        let reach = self.flash_reach * radius;
        VolumeUniform {
            centre: centre.extend(radius),
            axes: (AXES * cloud.breath().clamp(0.8, 1.2)).extend(self.steps),
            look: Vec4::new(self.density, self.scale, self.churn, self.absorption),
            low: self.color_low.extend(self.max_opacity),
            high: self.color_high.extend(self.dust),
            tint: cloud.colour().extend(self.light_steps),
            sun: sun.toward.normalize_or(Vec3::Y).extend(self.sun),
            sun_color: colour.extend(self.inside),
            flash: bolt.from.extend(bolt.light(reach)),
            flash_to: bolt.to.extend(0.0),
            params: Vec4::new(clock, reach, self.flash, cloud.fade()),
            storm: Vec4::new(self.swirl, self.filaments, self.flicker, self.variation),
            alt: self.color_alt.extend(self.core_glow),
        }
    }
}

/// Gives each cloud a replacing rule matches its proxy box, once.
fn spawn_volumes(
    mut commands: Commands,
    look: Res<Look>,
    time: Res<Time>,
    frame: Res<Frame>,
    noise: Option<Res<NoiseVolume>>,
    clouds: Query<(Entity, &Cloud, &GlobalTransform), Without<VolumeProxyOf>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<VolumeMaterial>>,
    mut cube: Local<Option<Handle<Mesh>>>,
) {
    let (Some(n), Some(noise)) = (&look.nebulae, noise) else { return };
    let bolt = Bolt::of(&frame);
    for (e, cloud, at) in &clouds {
        let Some(rule) = n.find(cloud.effect()).filter(|r| r.replace) else { continue };
        let mesh = cube.get_or_insert_with(|| meshes.add(Cuboid::new(1.0, 1.0, 1.0))).clone();
        // back faces only: the camera may be inside the box, and the shader marches the ray from there
        let base =
            StandardMaterial { unlit: true, cull_mode: Some(bevy::render::render_resource::Face::Front), double_sided: false, alpha_mode: AlphaMode::Premultiplied, ..default() };
        let material = materials.add(VolumeMaterial {
            base,
            extension: VolumeGas { u: rule.volume(cloud, at.translation(), time.elapsed_secs(), &bolt, SunLight::default()), noise: noise.0.clone(), frame: default() },
        });
        let size = AXES * 2.0 * cloud.radius() * BOX_MARGIN;
        commands.entity(e).insert(VolumeProxyOf).with_child((
            VolumeProxy,
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_scale(size),
            // the shell is hidden, so the proxy says it is shown; the ray march needs the whole box (the camera may be inside it)
            Visibility::Visible,
            bevy::camera::visibility::NoFrustumCulling,
            bevy::light::NotShadowCaster,
            bevy::light::NotShadowReceiver,
        ));
    }
}

/// Marks a cloud that has its proxy.
#[derive(Component)]
pub struct VolumeProxyOf;

/// The sun: the first directional light's direction toward it, and its colour.
fn sun_of(frame: &Frame) -> SunLight {
    frame.sun().map_or_else(SunLight::default, |(travel, color)| SunLight { toward: -travel, color })
}

fn update_volumes(
    look: Res<Look>,
    time: Res<Time>,
    frame: Res<Frame>,
    clouds: Query<(&Cloud, &GlobalTransform, &Children)>,
    proxies: Query<&MeshMaterial3d<VolumeMaterial>, With<VolumeProxy>>,
    mut materials: ResMut<Assets<VolumeMaterial>>,
) {
    let Some(n) = &look.nebulae else { return };
    let (sun, bolt) = (sun_of(&frame), Bolt::of(&frame));
    for (cloud, at, children) in &clouds {
        let Some(rule) = n.find(cloud.effect()).filter(|r| r.replace) else { continue };
        for mat in children.iter().filter_map(|c| proxies.get(c).ok()) {
            if let Some(mut m) = materials.get_mut(&mat.0) {
                m.extension.u = rule.volume(cloud, at.translation(), time.elapsed_secs(), &bolt, sun);
            }
        }
    }
}

pub fn plugin(app: &mut App, look: &Look) {
    if look.nebulae.is_some() {
        app.add_plugins((MaterialPlugin::<NebulaMaterial>::default(), MaterialPlugin::<VolumeMaterial>::default()))
            .add_systems(Update, (hide_shells, spawn_volumes, update_volumes, restyle, update).chain());
    }
}

/// A cloud whose retail shell is hidden because the profile replaces it.
#[derive(Component)]
pub struct ShellHidden;

/// A cloud that has the volume material.
#[derive(Component)]
pub struct Volumetric;

/// The last bolt of storm lightning (from the frame data, `frame.rs`): where, and how much of its light is left (1 at the strike,
/// 0 when out).
#[derive(Default)]
struct Bolt {
    at: Vec3,
    /// The bolt's segment (both ends equal for sheet lightning, a point of light).
    from: Vec3,
    to: Vec3,
    eye: Vec3,
    left: f32,
}

impl Bolt {
    fn of(frame: &Frame) -> Bolt {
        let (at, from, to, since) = frame.bolt();
        Bolt { at, from, to, eye: frame.camera(), left: (1.0 - since / FLASH_SECONDS).max(0.0) }
    }

    /// Full light close to either a bolt or a sheet flash; none at three times its reach through the gas.
    fn light(&self, reach: f32) -> f32 {
        if self.left <= 0.0 {
            return 0.0;
        }
        let span = self.to - self.from;
        let along = ((self.eye - self.from).dot(span) / span.length_squared().max(1e-3)).clamp(0.0, 1.0);
        let distance = self.eye.distance(self.from + span * along);
        let t = ((distance / reach.max(1.0) - 1.0) * 0.5).clamp(0.0, 1.0);
        self.left * self.left * (1.0 - t * t * (3.0 - 2.0 * t))
    }
}

impl Rule {
    fn uniform(&self, centre: Vec3, radius: f32, clock: f32, bolt: &Bolt) -> NebulaUniform {
        NebulaUniform {
            centre: centre.extend(radius),
            axes: AXES.extend(self.steps),
            look: Vec4::new(self.density, self.scale, self.churn, self.shell),
            low: self.color_low.extend(self.inside),
            high: self.color_high.extend(self.flash),
            flash: bolt.at.extend(bolt.light(self.flash_reach * radius)),
            params: Vec4::new(clock, self.flash_reach * radius, 0.0, 0.0),
        }
    }
}

/// Hides the retail shell of every cloud a replacing rule matches (the cloud keeps running: nothing but its visibility changes).
fn hide_shells(mut commands: Commands, look: Res<Look>, clouds: Query<(Entity, &Cloud), Without<ShellHidden>>) {
    let Some(n) = &look.nebulae else { return };
    for (e, cloud) in &clouds {
        if n.find(cloud.effect()).is_some_and(|r| r.replace) {
            commands.entity(e).try_insert((Visibility::Hidden, ShellHidden));
        }
    }
}

/// Swaps the retail material of every cloud a rule matches, once, keeping its mesh (which still carries the retail colour loop
/// and the edge fade in its vertex colours) and its texture.
fn restyle(
    mut commands: Commands,
    look: Res<Look>,
    time: Res<Time>,
    frame: Res<Frame>,
    clouds: Query<(Entity, &Cloud, &GlobalTransform, &MeshMaterial3d<EffectMaterial>), Without<Volumetric>>,
    retail: Res<Assets<EffectMaterial>>,
    mut materials: ResMut<Assets<NebulaMaterial>>,
) {
    let Some(n) = &look.nebulae else { return };
    let bolt = Bolt::of(&frame);
    for (e, cloud, at, mat) in &clouds {
        let Some(rule) = n.find(cloud.effect()).filter(|r| !r.replace) else { continue };
        let Some(old) = retail.get(&mat.0) else { continue };
        let base = StandardMaterial { base_color: Color::WHITE, base_color_texture: old.base.base_color_texture.clone(), ..old.base.clone() };
        let handle =
            materials.add(NebulaMaterial { base, extension: NebulaGlow { u: rule.uniform(at.translation(), cloud.radius(), time.elapsed_secs(), &bolt), frame: default() } });
        super::kit::swap::<EffectMaterial, _>(&mut commands, e, handle).try_insert(Volumetric);
    }
}

fn update(
    look: Res<Look>,
    time: Res<Time>,
    frame: Res<Frame>,
    clouds: Query<(&Cloud, &GlobalTransform, &MeshMaterial3d<NebulaMaterial>)>,
    mut materials: ResMut<Assets<NebulaMaterial>>,
) {
    let Some(n) = &look.nebulae else { return };
    let bolt = Bolt::of(&frame);
    for (cloud, at, mat) in &clouds {
        let (Some(rule), Some(mut m)) = (n.find(cloud.effect()), materials.get_mut(&mat.0)) else { continue };
        m.extension.u = rule.uniform(at.translation(), cloud.radius(), time.elapsed_secs(), &bolt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flash_light_fades_smoothly_with_distance_to_the_visible_bolt() {
        let mut bolt = Bolt { from: Vec3::X * -100.0, to: Vec3::X * 100.0, eye: Vec3::X * 150.0, left: 0.5, ..default() };
        assert_eq!(bolt.light(50.0), 0.25, "near the end of the bolt, not its midpoint");
        bolt.eye.x = 200.0;
        assert_eq!(bolt.light(50.0), 0.125, "halfway through the distance fade");
        bolt.eye.x = 250.0;
        assert_eq!(bolt.light(50.0), 0.0, "a distant flash contributes no light");
        let sheet = Bolt { eye: Vec3::X * 150.0, left: 1.0, ..default() };
        assert_eq!(sheet.light(50.0), 0.0, "a sheet flash is a point, with the same falloff");
    }

    #[test]
    fn a_bolts_light_fades_over_its_seconds_from_the_frame_data() {
        let mut frame = Frame::default();
        assert_eq!(Bolt::of(&frame).left, 0.0, "none yet");
        frame.rows[super::super::frame::BOLT_AT] = Vec3::X.extend(0.0);
        assert_eq!(Bolt::of(&frame).left, 1.0, "struck just now");
        frame.rows[super::super::frame::BOLT_AT].w = FLASH_SECONDS * 0.5;
        assert_eq!((Bolt::of(&frame).at, Bolt::of(&frame).left), (Vec3::X, 0.5));
        frame.rows[super::super::frame::BOLT_AT].w = FLASH_SECONDS * 2.0;
        assert_eq!(Bolt::of(&frame).left, 0.0, "out");
    }

    #[test]
    fn absent_without_the_key_and_a_rule_needs_its_match() {
        assert!(parse(&Value::Null).is_none());
        assert!(parse(&json!({})).unwrap().rules.is_empty());
        assert!(parse(&json!({"rules": [{"density": 2}]})).unwrap().rules.is_empty(), "a rule without `match` is dropped");
    }

    #[test]
    fn defaults_and_clamping() {
        let n = parse(&json!({"rules": [{"match": "Nebula"}, {"match": "SolarStorm", "steps": 999, "density": -3, "inside": 5}]})).unwrap();
        assert_eq!((n.rules[0].steps, n.rules[0].density, n.rules[0].matching.as_str()), (16.0, 1.0, "nebula"));
        assert_eq!((n.rules[1].steps, n.rules[1].density, n.rules[1].inside), (MAX_STEPS, 0.0, 1.0));
    }

    #[test]
    fn rules_match_by_effect_name_first_wins() {
        let n = parse(&json!({"rules": [{"match": "SolarStorm", "density": 4}, {"match": "Nebula", "density": 1}, {"match": "s", "density": 9}]})).unwrap();
        assert_eq!(n.find("SolarStorm").map(|r| r.density), Some(4.0));
        assert_eq!(n.find("Nebula").map(|r| r.density), Some(1.0));
        assert!(n.find("Meteor").is_none(), "no rule, the retail material stays");
    }

    #[test]
    fn the_uniform_carries_the_boxes_axes_and_a_fading_bolt() {
        let rule = &parse(&json!({"rules": [{"match": "n", "steps": 20}]})).unwrap().rules[0];
        let bolt = Bolt { at: Vec3::new(1.0, 2.0, 3.0), left: 0.5, ..default() };
        let u = rule.uniform(Vec3::new(10.0, 0.0, 0.0), 300.0, 4.0, &bolt);
        assert_eq!((u.centre, u.axes), (Vec4::new(10.0, 0.0, 0.0, 300.0), Vec4::new(1.0, 0.7, 0.99, 20.0)));
        assert_eq!(u.flash, Vec4::new(1.0, 2.0, 3.0, 0.25));
        assert_eq!(rule.uniform(Vec3::ZERO, 1.0, 0.0, &Bolt::default()).flash.w, 0.0);
    }

    #[test]
    fn a_cloud_swaps_its_material_once_keeping_its_mesh_and_unmatched_clouds_keep_theirs() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Shader>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Mesh>()
            .init_asset::<EffectMaterial>()
            .init_asset::<NebulaMaterial>()
            .init_resource::<Frame>()
            .insert_resource(Look::parse(&json!({"nebulae": {"rules": [{"match": "Nebula"}]}})))
            .add_systems(Update, restyle);
        let (mesh, retail) = {
            let w = app.world_mut();
            (w.resource_mut::<Assets<Mesh>>().add(Mesh::from(Cuboid::default())), w.resource_mut::<Assets<EffectMaterial>>().add(crate::fx::additive_material(Handle::default())))
        };
        let (cloud, storm) = (Cloud::for_tests("Nebula", 100.0), Cloud::for_tests("MeteorStorm", 50.0));
        let a = app.world_mut().spawn((cloud, GlobalTransform::default(), Mesh3d(mesh.clone()), MeshMaterial3d(retail.clone()))).id();
        let b = app.world_mut().spawn((storm, GlobalTransform::default(), Mesh3d(mesh.clone()), MeshMaterial3d(retail.clone()))).id();
        app.update();
        app.update();
        let w = app.world();
        assert!(w.entity(a).contains::<MeshMaterial3d<NebulaMaterial>>() && w.entity(a).contains::<Volumetric>() && !w.entity(a).contains::<MeshMaterial3d<EffectMaterial>>());
        assert_eq!(w.entity(a).get::<Mesh3d>().map(|m| m.0.id()), Some(mesh.id()), "the mesh, with its vertex colours, is kept");
        assert!(w.entity(b).contains::<MeshMaterial3d<EffectMaterial>>() && !w.entity(b).contains::<Volumetric>());
        assert_eq!(w.resource::<Assets<NebulaMaterial>>().len(), 1, "once");
    }

    #[test]
    fn replace_is_per_rule_or_for_the_block_and_off_by_default() {
        assert!(!parse(&json!({"rules": [{"match": "n"}]})).unwrap().rules[0].replace);
        let n = parse(&json!({"replace": true, "rules": [{"match": "Nebula"}, {"match": "SolarStorm", "replace": false}]})).unwrap();
        assert_eq!((n.rules[0].replace, n.rules[1].replace), (true, false));
        assert!(!n.replaces_storms(), "the storm rule opted out");
        assert!(parse(&json!({"rules": [{"match": "SolarStorm", "replace": true}]})).unwrap().replaces_storms());
    }

    #[test]
    fn a_replacing_rule_hides_the_shell_and_keeps_the_cloud_and_its_material() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Shader>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Mesh>()
            .init_asset::<EffectMaterial>()
            .init_asset::<NebulaMaterial>()
            .init_resource::<Frame>()
            .insert_resource(Look::parse(&json!({"nebulae": {"rules": [{"match": "Nebula", "replace": true}, {"match": "Meteor"}]}})))
            .add_systems(Update, (hide_shells, restyle).chain());
        let retail = app.world_mut().resource_mut::<Assets<EffectMaterial>>().add(crate::fx::additive_material(Handle::default()));
        let mut spawn =
            |effect: &str| app.world_mut().spawn((Cloud::for_tests(effect, 100.0), GlobalTransform::default(), Visibility::default(), MeshMaterial3d(retail.clone()))).id();
        let (replaced, plain) = (spawn("Nebula"), spawn("MeteorStorm"));
        app.update();
        let w = app.world();
        assert_eq!(w.entity(replaced).get::<Visibility>(), Some(&Visibility::Hidden));
        assert!(w.entity(replaced).contains::<Cloud>() && w.entity(replaced).contains::<MeshMaterial3d<EffectMaterial>>(), "nothing is removed from it");
        assert_eq!(w.entity(plain).get::<Visibility>(), Some(&Visibility::Inherited));
        assert!(w.entity(plain).contains::<Volumetric>(), "a rule without replace still restyles the shell");
    }

    fn volume_app(look: serde_json::Value) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Shader>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<VolumeMaterial>()
            .init_resource::<Frame>()
            .insert_resource(Look::parse(&look))
            .add_systems(Startup, crate::render::volnoise::bake)
            .add_systems(Update, (spawn_volumes, update_volumes).chain());
        app
    }

    #[test]
    fn volume_keys_have_defaults_and_clamp() {
        let r = &parse(&json!({"rules": [{"match": "n"}, {"match": "s", "absorption": 99, "dust": -1, "max_opacity": 5, "light_steps": 9, "sun": 50, "core_glow": 300}]}))
            .unwrap()
            .rules;
        assert_eq!((r[0].absorption, r[0].dust, r[0].max_opacity, r[0].light_steps, r[0].sun), (3.0, 0.6, 0.6, 2.0, 1.0));
        assert_eq!((r[1].absorption, r[1].dust, r[1].max_opacity, r[1].light_steps, r[1].sun), (20.0, 0.0, 1.0, 3.0, 4.0));
        assert_eq!((r[0].core_glow, r[1].core_glow), (0.0, 12.0), "older profiles get no extra core emission");
    }

    #[test]
    fn a_replaced_cloud_gets_one_back_face_proxy_that_follows_it_and_others_get_none() {
        let mut app = volume_app(json!({"nebulae": {"rules": [{"match": "Nebula", "replace": true, "steps": 20}, {"match": "MeteorStorm"}]}}));
        let spawn = |app: &mut App, effect: &str, x: f32| app.world_mut().spawn((Cloud::for_tests(effect, 100.0), GlobalTransform::from_xyz(x, 0.0, 0.0), Visibility::Hidden)).id();
        let (cloud, plain) = (spawn(&mut app, "Nebula", 10.0), spawn(&mut app, "MeteorStorm", 0.0));
        app.update();
        app.update();
        app.update();
        let proxies = app
            .world_mut()
            .query::<(&ChildOf, &VolumeProxy, &MeshMaterial3d<VolumeMaterial>, &Visibility, &Transform)>()
            .iter(app.world())
            .map(|(p, _, m, v, t)| (p.parent(), m.0.clone(), *v, *t))
            .collect::<Vec<_>>();
        assert_eq!(proxies.len(), 1, "one proxy, once");
        let (parent, mat, vis, tf) = &proxies[0];
        assert_eq!(*parent, cloud);
        assert_eq!(*vis, Visibility::Visible, "drawn although the shell is hidden");
        assert_eq!(tf.scale, AXES * 2.0 * 100.0 * BOX_MARGIN);
        assert!(!app.world().entity(plain).contains::<VolumeProxyOf>());
        let m = app.world().resource::<Assets<VolumeMaterial>>().get(mat).unwrap();
        assert_eq!(m.base.cull_mode, Some(bevy::render::render_resource::Face::Front), "back faces only, so it draws from inside");
        assert_eq!(m.base.alpha_mode, AlphaMode::Premultiplied);
        assert_eq!((m.extension.u.centre, m.extension.u.axes.w), (Vec4::new(10.0, 0.0, 0.0, 100.0), 20.0));
        assert!(m.extension.noise != Handle::default(), "the baked noise is bound");
    }

    #[test]
    fn the_volume_follows_the_retail_breath_the_sun_and_the_bolt() {
        let rule = &parse(&json!({"rules": [{"match": "n", "steps": 12, "max_opacity": 0.5, "sun": 2, "core_glow": 6}]})).unwrap().rules[0];
        let cloud = Cloud::for_tests("Nebula", 200.0);
        let sun = SunLight { toward: Vec3::new(0.0, 3.0, 0.0), color: Vec3::new(2.0, 1.0, 1.0) };
        let u = rule.volume(&cloud, Vec3::new(1.0, 2.0, 3.0), 5.0, &Bolt { at: Vec3::X, from: Vec3::X, to: Vec3::X * 3.0, left: 0.5, ..default() }, sun);
        assert_eq!(u.axes, (AXES * cloud.breath()).extend(12.0));
        assert_eq!((u.sun, u.sun_color.truncate()), (Vec4::new(0.0, 1.0, 0.0, 2.0), Vec3::new(1.0, 0.5, 0.5)), "a unit direction and a colour normalised to its brightest channel");
        assert_eq!((u.low.w, u.flash.w, u.params.x, u.params.w), (0.5, 0.25, 5.0, 1.0));
        assert_eq!(u.alt, Vec3::ONE.extend(6.0), "core emission reaches the shader independently of opacity, flash and hue");
        assert_eq!((u.flash.truncate(), u.flash_to.truncate()), (Vec3::X, Vec3::X * 3.0), "the volume is lit along the whole bolt");
    }
}
