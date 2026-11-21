//! Not retail: the sky dome of the render profile (`sky` block, drawn with the mod's `shaders/sky.wesl`). The baked sky cube
//! (`view::hd_sky`) stays the base picture and the image-based light; this adds a camera-centred dome just inside it
//! (an inverted unit sphere, child of the `view::Sky` entity, so it follows the camera and never veils a ship: the depth
//! test keeps it behind the world, and its sort bias draws it before every other blended effect).
//!
//! The near layers (up to three) are the near ether: a sparse, soft copy of the sky's own gas, sampled as if it floated
//! `distance` units out, so it slides against the sky as the camera moves (parallax), and carried along a map-wide current
//! (`drift`, world units a second). Each layer is broken up by slowly churning 3-D noise so it reads as wisps, not as a ghost
//! of the backdrop. A rigid turn of the whole sky reads as the camera rolling; relative motion reads as depth.
//! `clouds` are the real depth: up to three cloud decks, each a slab of 3-D cloud at its own altitude in the world, ray-marched
//! on the dome (a few steps through the slab, one light step toward the sun at each), so a near deck sits in front of a far one
//! and the decks slide against each other as the camera moves, with lit tops, shadowed undersides, a silver rim against the sun
//! and a haze fade toward the horizon; they drift with their own current and churn in place. The baked cube stays the backdrop.
//! `base` also redraws the far sky itself with a slow flow and a breath. The stars are a separate cube of point stars
//! (`<sky>_stars.png`, 16-bit linear light over `gain`, baked with `tools.bake_sky --split-stars`), each twinkling on its own
//! phase, the bright ones past 1 so they bloom. Events light the gas, never the stars: a storm's lightning (`nebula::StormFlash`),
//! big explosions (`events`), sheet lightning and shooting stars (the shader's own clock). `skies` merges per-sky accents.
//! Off without the `sky` key and without a baked sky: classic never sees it.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use serde_json::Value;

use super::{Look, num, rgb};
use crate::view::{HdSky, Sky};

pub const SHADER: &str = "shaders/sky.wesl";

/// Sorts the dome before every other blended thing (Bevy sorts blended meshes by distance plus this bias).
const SORT_BIAS: f32 = -50_000.0;

/// The dome sits a little inside the skybox's sphere.
const DOME_SCALE: f32 = 0.98;

/// Most near layers a sky draws (fixed arrays in the uniform).
pub const MAX_LAYERS: usize = 3;

/// Most cloud decks a sky draws.
pub const MAX_DECKS: usize = 3;

/// Most steps through a deck's slab (the shader's loop bound).
pub const MAX_STEPS: u32 = 12;

/// The `sky` block.
#[derive(Clone, Debug, PartialEq)]
pub struct SkyDome {
    pub layers: Vec<NearLayer>,
    /// The ray-marched cloud decks, over the near layers.
    pub decks: Vec<Deck>,
    pub stars: Option<Stars>,
    /// The far sky redrawn with a flow and a breath.
    pub base: Option<Base>,
    /// The flash a storm's lightning throws on the gas.
    pub flash: Flash,
    /// Explosions that warm the gas on their side (first whose `match` is in the effect's name wins).
    pub events: Vec<SkyEvent>,
    /// A shooting star about every this many seconds (0 = none), and how bright.
    pub shooting_every: f32,
    pub shooting_brightness: f32,
    /// Sheet lightning behind the gas.
    pub sheet: Option<Sheet>,
    /// The block as written (without `skies`), and the per-sky accents merged over it by `for_sky`.
    raw: Value,
    skies: Vec<(String, Value)>,
}

/// One near layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NearLayer {
    /// How far out (world units) the layer floats: smaller slides more as the camera moves.
    pub distance: f32,
    /// Strength, 0..1; 0 draws nothing.
    pub opacity: f32,
    /// Brightness (0..1) below which the gas is left out of the layer, and the width of the fade above it.
    pub threshold: f32,
    pub softness: f32,
    /// Map-wide current: how far the gas travels a second, world units (stored in Bevy axes, written in game axes).
    pub drift: Vec3,
    /// Noise patches across the layer (0 = no breaking up: the plain copy of the sky).
    pub scale: f32,
    pub tint: Vec3,
}

/// One cloud deck: a slab of cloud, `thickness` thick and centred `altitude` above the play plane (negative: below it), made of the
/// baked noise at `scale` world units a cell, so it has a real place in the world and parallax follows from the camera's position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Deck {
    /// World height of the slab's middle (game z; negative lies under the ships).
    pub altitude: f32,
    pub thickness: f32,
    /// How much of the sky the cloud covers, 0..1.
    pub coverage: f32,
    /// Optical depth of a ray straight through the densest cloud (how fast it goes opaque).
    pub density: f32,
    /// World units across one cell of the noise (a bigger scale is bigger clouds).
    pub scale: f32,
    /// The most any ray is covered, 0..1; 0 draws nothing.
    pub opacity: f32,
    /// Distance (world units) over which the deck has faded 63 % of the way into the haze.
    pub haze: f32,
    /// The deck's own current, world units a second (Bevy axes, written in game axes).
    pub drift: Vec3,
    /// Noise cells a second the shapes churn in place.
    pub churn: f32,
    /// Brightness of the rim against the sun, 0..
    pub silver: f32,
    /// Share of the fine noise in the shapes, 0..1.
    pub detail: f32,
    /// Colour where the sun reaches, where it does not, and what the haze fades to.
    pub lit: Vec3,
    pub shadow: Vec3,
    pub haze_color: Vec3,
    /// Steps through the slab.
    pub steps: u32,
    /// Direction toward the sun (Bevy axes; written in game axes). None: the map's sun, else a fixed one.
    pub sun: Option<Vec3>,
}

fn deck(v: &Value) -> Deck {
    Deck {
        altitude: num(&v["altitude"], 600.0),
        thickness: num(&v["thickness"], 300.0).max(1.0),
        coverage: num(&v["coverage"], 0.5).clamp(0.0, 1.0),
        density: num(&v["density"], 4.0).max(0.0),
        scale: num(&v["scale"], 400.0).max(1.0),
        opacity: num(&v["opacity"], 0.9).clamp(0.0, 1.0),
        haze: num(&v["haze"], 6000.0).max(1.0),
        drift: game_vec(&v["drift"]).unwrap_or(Vec3::ZERO),
        churn: num(&v["churn"], 0.02).max(0.0),
        silver: num(&v["silver"], 0.6).max(0.0),
        detail: num(&v["detail"], 0.5).clamp(0.0, 1.0),
        lit: rgb(&v["lit"]).unwrap_or(Vec3::new(1.0, 0.85, 0.65)).max(Vec3::ZERO),
        shadow: rgb(&v["shadow"]).unwrap_or(Vec3::new(0.2, 0.2, 0.35)).max(Vec3::ZERO),
        haze_color: rgb(&v["haze_color"]).unwrap_or(Vec3::new(0.3, 0.3, 0.45)).max(Vec3::ZERO),
        steps: num(&v["steps"], 6.0).clamp(2.0, MAX_STEPS as f32) as u32,
        sun: game_vec(&v["sun"]).and_then(Vec3::try_normalize),
    }
}

/// The `stars` part of the block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stars {
    /// Multiplies the stars' light.
    pub brightness: f32,
    /// How deep the twinkle goes for a bright star, 0 (steady) .. 1 (fully out at the dip).
    pub twinkle: f32,
    /// Twinkles a second.
    pub speed: f32,
    /// Decodes the baked 16-bit values to light (the baker's `STAR_GAIN`).
    pub gain: f32,
}

/// The `base` part: two samples of the sky cross-fade along a slow noise flow, and the bright cores breathe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Base {
    /// Brightness of the far backdrop, independent of the star layer and scene lighting.
    pub brightness: f32,
    /// How far the lookup is pushed along the flow (radians at the end of a cycle).
    pub flow: f32,
    /// Seconds a flow cycle lasts.
    pub cycle: f32,
    /// How much the bright cores brighten and dim (share).
    pub breath: f32,
    /// Seconds a breath lasts.
    pub period: f32,
    /// The breath's colour bias (white = plain brightening).
    pub tint: Vec3,
}

/// How a storm's bolt lights the gas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flash {
    pub color: Vec3,
    pub strength: f32,
    pub seconds: f32,
    /// Distance (world units) from the camera at which a flash lights the gas at half strength; a storm far off barely
    /// flickers (0 = no falloff).
    pub reach: f32,
}

/// An explosion that warms the near gas on its side.
#[derive(Clone, Debug, PartialEq)]
pub struct SkyEvent {
    pub effect: String,
    pub color: Vec3,
    pub strength: f32,
    pub seconds: f32,
}

/// Sheet lightning: about every `every` seconds a flash far behind the gas, in a random direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sheet {
    pub every: f32,
    pub strength: f32,
    pub color: Vec3,
}

/// A game-axes vector (x starboard, y forward, z up) in Bevy's.
fn game_vec(v: &Value) -> Option<Vec3> {
    rgb(v).map(|d| Vec3::new(d.x, d.z, -d.y))
}

fn layer(near: &Value) -> NearLayer {
    NearLayer {
        distance: num(&near["distance"], 4000.0).max(1.0),
        opacity: num(&near["opacity"], 0.5).clamp(0.0, 1.0),
        threshold: num(&near["threshold"], 0.3).clamp(0.0, 1.0),
        softness: num(&near["softness"], 0.35).max(0.01),
        drift: game_vec(&near["drift"]).unwrap_or(Vec3::ZERO),
        scale: num(&near["scale"], 2.5).max(0.0),
        tint: rgb(&near["tint"]).unwrap_or(Vec3::ONE).max(Vec3::ZERO),
    }
}

/// `over` merged into `base`: objects key by key, anything else replaced.
fn merge(base: &Value, over: &Value) -> Value {
    match (base, over) {
        (Value::Object(a), Value::Object(b)) => {
            let mut out = a.clone();
            for (k, v) in b {
                let merged = out.get(k).map_or_else(|| v.clone(), |old| merge(old, v));
                out.insert(k.clone(), merged);
            }
            Value::Object(out)
        }
        _ => over.clone(),
    }
}

pub fn parse(v: &Value) -> Option<SkyDome> {
    let obj = v.as_object()?;
    let skies: Vec<(String, Value)> =
        obj.get("skies").and_then(Value::as_object).map(|m| m.iter().filter(|(_, x)| x.is_object()).map(|(k, x)| (k.to_lowercase(), x.clone())).collect()).unwrap_or_default();
    let mut raw = obj.clone();
    raw.remove("skies");
    let near = &v["near"];
    let layers: Vec<NearLayer> = match near {
        Value::Array(a) => a.iter().filter(|x| x.is_object()).take(MAX_LAYERS).map(layer).collect(),
        Value::Object(_) => vec![layer(near)],
        _ => vec![],
    }
    .into_iter()
    .filter(|l| l.opacity > 0.0)
    .collect();
    let decks: Vec<Deck> = match &v["clouds"] {
        Value::Array(a) => a.iter().filter(|x| x.is_object()).take(MAX_DECKS).map(deck).collect(),
        c @ Value::Object(_) => vec![deck(c)],
        _ => vec![],
    }
    .into_iter()
    .filter(|d| d.opacity > 0.0)
    .collect();
    Some(SkyDome {
        layers,
        decks,
        stars: v["stars"].is_object().then(|| {
            let t = &v["stars"];
            Stars {
                brightness: num(&t["brightness"], 1.0).max(0.0),
                twinkle: num(&t["twinkle"], 0.4).clamp(0.0, 1.0),
                speed: num(&t["speed"], 2.5).max(0.0),
                gain: num(&t["gain"], 16.0).max(0.0),
            }
        }),
        base: v["base"].is_object().then(|| {
            let b = &v["base"];
            Base {
                brightness: num(&b["brightness"], 1.0).max(0.0),
                flow: num(&b["flow"], 0.02).clamp(0.0, 0.2),
                cycle: num(&b["cycle"], 30.0).max(1.0),
                breath: num(&b["breath"], 0.05).clamp(0.0, 1.0),
                period: num(&b["period"], 30.0).max(1.0),
                tint: rgb(&b["tint"]).unwrap_or(Vec3::ONE).max(Vec3::ZERO),
            }
        }),
        flash: Flash {
            color: rgb(&v["flash"]["color"]).unwrap_or(Vec3::new(0.65, 0.78, 1.0)).max(Vec3::ZERO),
            strength: num(&v["flash"]["strength"], 1.5).max(0.0),
            seconds: num(&v["flash"]["seconds"], 0.4).max(0.05),
            reach: num(&v["flash"]["reach"], 1200.0).max(0.0),
        },
        events: v["events"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|e| {
                        Some(SkyEvent {
                            effect: e["match"].as_str()?.to_string(),
                            color: rgb(&e["color"]).unwrap_or(Vec3::new(1.0, 0.6, 0.3)).max(Vec3::ZERO),
                            strength: num(&e["strength"], 1.0).max(0.0),
                            seconds: num(&e["seconds"], 1.0).max(0.05),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        shooting_every: if v["shooting_star"].is_object() { num(&v["shooting_star"]["every"], 180.0).max(10.0) } else { 0.0 },
        shooting_brightness: num(&v["shooting_star"]["brightness"], 6.0).max(0.0),
        sheet: v["sheet_lightning"].is_object().then(|| Sheet {
            every: num(&v["sheet_lightning"]["every"], 40.0).max(5.0),
            strength: num(&v["sheet_lightning"]["strength"], 1.5).max(0.0),
            color: rgb(&v["sheet_lightning"]["color"]).unwrap_or(Vec3::new(0.7, 0.8, 1.0)).max(Vec3::ZERO),
        }),
        raw: Value::Object(raw),
        skies,
    })
}

impl SkyDome {
    /// The block for the sky whose baked cube is named `<name>_cube.png` (`skybox_05`): its `skies` entry merged over the defaults.
    pub fn for_sky(&self, name: &str) -> SkyDome {
        let name = name.to_lowercase();
        let Some((_, over)) = self.skies.iter().find(|(k, _)| *k == name) else { return self.clone() };
        let mut d = parse(&merge(&self.raw, over)).unwrap_or_else(|| self.clone());
        d.skies = self.skies.clone();
        d
    }
}

/// One near layer in the uniform.
#[derive(Clone, Copy, Default, Debug, PartialEq, ShaderType)]
pub struct LayerUniform {
    /// x: opacity, y: threshold, z: softness, w: distance
    pub a: Vec4,
    /// x: noise scale, yzw: tint
    pub b: Vec4,
    /// xyz: the current, units a second
    pub c: Vec4,
}

/// One cloud deck in the uniform.
#[derive(Clone, Copy, Default, Debug, PartialEq, ShaderType)]
pub struct DeckUniform {
    /// x: altitude, y: thickness, z: coverage, w: density
    pub a: Vec4,
    /// x: noise scale, y: opacity, z: haze distance, w: churn
    pub b: Vec4,
    /// xyz: the current, units a second, w: silver
    pub c: Vec4,
    /// rgb: lit colour, w: detail
    pub d: Vec4,
    /// rgb: shadow colour, w: steps
    pub e: Vec4,
    /// rgb: haze colour, w: 1 when `g` is set
    pub f: Vec4,
    /// xyz: direction toward the sun
    pub g: Vec4,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, ShaderType)]
pub struct SkyUniform {
    /// x: brightness (0 = no stars), y: twinkle depth, z: twinkles a second, w: gain
    pub stars: Vec4,
    /// x: number of near layers, y: 1 when the far sky is redrawn, z: number of cloud decks
    pub info: Vec4,
    /// x: flow (radians), y: flow cycle seconds, z: breath depth, w: breath seconds
    pub base: Vec4,
    /// xyz: the breath's colour bias, w: the far backdrop's brightness minus 1 (0 leaves it unchanged)
    pub base_tint: Vec4,
    /// rgb: a storm flash's colour times its strength, w: seconds it lasts (the bolt itself is in the frame data)
    pub flash: Vec4,
    /// x: distance from the camera at which a flash lights the gas at half strength (0 = no falloff)
    pub flash_reach: Vec4,
    /// rgb: an explosion's warmth times what is left of it
    pub warm: Vec4,
    /// xyz: world direction of the explosion
    pub warm_dir: Vec4,
    /// x: seconds a shooting star's slot lasts (0 = none), y: brightness
    pub shoot: Vec4,
    /// x: seconds a sheet lightning slot lasts (0 = none), y: strength
    pub sheet: Vec4,
    pub sheet_color: Vec4,
    pub layers: [LayerUniform; MAX_LAYERS],
    pub decks: [DeckUniform; MAX_DECKS],
}

impl SkyDome {
    fn uniform(&self) -> SkyUniform {
        let mut layers = [LayerUniform::default(); MAX_LAYERS];
        for (u, l) in layers.iter_mut().zip(&self.layers) {
            *u = LayerUniform { a: Vec4::new(l.opacity, l.threshold, l.softness, l.distance), b: Vec4::new(l.scale, l.tint.x, l.tint.y, l.tint.z), c: l.drift.extend(0.0) };
        }
        let mut decks = [DeckUniform::default(); MAX_DECKS];
        for (u, d) in decks.iter_mut().zip(&self.decks) {
            *u = DeckUniform {
                a: Vec4::new(d.altitude, d.thickness, d.coverage, d.density),
                b: Vec4::new(d.scale, d.opacity, d.haze, d.churn),
                c: d.drift.extend(d.silver),
                d: d.lit.extend(d.detail),
                e: d.shadow.extend(d.steps as f32),
                f: d.haze_color.extend(if d.sun.is_some() { 1.0 } else { 0.0 }),
                g: d.sun.unwrap_or(Vec3::ZERO).extend(0.0),
            };
        }
        let (base, base_tint) = self.base.map_or((Vec4::ZERO, Vec4::ZERO), |b| (Vec4::new(b.flow, b.cycle, b.breath, b.period), b.tint.extend(b.brightness - 1.0)));
        SkyUniform {
            stars: self.stars.map_or(Vec4::ZERO, |t| Vec4::new(t.brightness, t.twinkle, t.speed, t.gain)),
            info: Vec4::new(self.layers.len().min(MAX_LAYERS) as f32, if self.base.is_some() { 1.0 } else { 0.0 }, self.decks.len().min(MAX_DECKS) as f32, 0.0),
            base,
            base_tint,
            flash: (self.flash.color * self.flash.strength).extend(self.flash.seconds),
            flash_reach: Vec4::new(self.flash.reach, 0.0, 0.0, 0.0),
            shoot: Vec4::new(self.shooting_every, self.shooting_brightness, 0.0, 0.0),
            sheet: self.sheet.map_or(Vec4::ZERO, |s| Vec4::new(s.every, s.strength, 0.0, 0.0)),
            sheet_color: self.sheet.map_or(Vec4::ZERO, |s| s.color.extend(0.0)),
            layers,
            decks,
            ..default()
        }
    }
}

/// An explosion's warmth, fading out: where, in what colour, how much is left. Render-side only.
#[derive(Default, Clone, Copy, Debug, PartialEq)]
struct Pulse {
    at: Vec3,
    color: Vec3,
    /// 1 at the start, 0 when gone.
    left: f32,
    seconds: f32,
}

impl Pulse {
    fn set(&mut self, at: Vec3, color: Vec3, seconds: f32) {
        *self = Pulse { at, color, left: 1.0, seconds };
    }
    fn step(&mut self, delta: f32) {
        if self.left > 0.0 {
            self.left = (self.left - delta / self.seconds.max(0.01)).max(0.0);
        }
    }
    /// The colour times the quadratic fade, and the direction from `eye`.
    fn uniform(&self, eye: Vec3) -> (Vec4, Vec4) {
        let k = self.left * self.left;
        ((self.color * k).extend(0.0), (self.at - eye).normalize_or_zero().extend(0.0))
    }
}

/// An explosion's warmth on the gas (a storm's flash comes from the frame data, `frame.rs`).
#[derive(Resource, Default)]
struct SkyEvents {
    warm: Pulse,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct SkyGlow {
    #[uniform(100)]
    pub u: SkyUniform,
    /// The frame data (`frame.rs`) at `@binding(99)`.
    #[storage(99, read_only)]
    pub frame: super::frame::FrameBinding,
    #[texture(101, dimension = "cube")]
    #[sampler(102)]
    pub cube: Option<Handle<Image>>,
    #[texture(103, dimension = "cube")]
    #[sampler(104)]
    pub stars: Option<Handle<Image>>,
    /// The baked tiling noise (`volnoise.rs`: r three octaves, g warp), so the wisps cost a texture read, not value noise per pixel.
    #[texture(105, dimension = "3d")]
    #[sampler(106)]
    pub noise: Option<Handle<Image>>,
}

impl MaterialExtension for SkyGlow {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // behind everything: the depth test does that, so the dome writes none
        super::kit::no_depth_write(descriptor);
        Ok(())
    }
}

pub type SkyMaterial = ExtendedMaterial<StandardMaterial, SkyGlow>;

pub fn plugin(app: &mut App, look: &Look) {
    if look.sky.is_some() {
        app.add_plugins(MaterialPlugin::<SkyMaterial>::default()).init_resource::<SkyEvents>().add_systems(Update, (sky_events, dome).chain());
    }
}

/// Marks the dome, the child of the `Sky` entity, with the block as resolved for its sky.
#[derive(Component)]
struct Dome(SkyDome);

/// The stars cube that goes with a baked sky (`models/sky/<sky>_cube.png` -> `<sky>_stars.png`), when the mod has one.
fn stars_file(root: &std::path::Path, cube: &str) -> Option<String> {
    let rel = cube.strip_suffix("_cube.png")?.to_string() + "_stars.png";
    root.join(&rel).is_file().then_some(rel)
}

/// The sky's base name (`skybox_05`) from its cube's path.
fn sky_name(cube: &str) -> &str {
    let file = cube.rsplit('/').next().unwrap_or(cube);
    file.strip_suffix("_cube.png").unwrap_or(file)
}

/// Whether a loaded image is (now) a cube map. Borrows it mutably only while it still is a stack of faces: a mutable borrow
/// marks the image changed, and Bevy then uploads it to the GPU again.
fn ready(h: &Handle<Image>, images: &mut Assets<Image>) -> bool {
    match images.get(h) {
        None => false,
        Some(i) if i.texture_descriptor.size.depth_or_array_layers == 6 => true,
        Some(_) => images.get_mut(h).is_some_and(|mut i| crate::view::as_cube(&mut i)),
    }
}

/// The stars image a sky is waiting for.
#[derive(Component)]
pub(super) struct StarsImage(pub(super) Handle<Image>);

/// Reads what happened in the battle that lights the sky: a storm's bolt, a big explosion.
fn sky_events(
    look: Res<Look>,
    time: Res<Time<Real>>,
    lib: Option<Res<crate::fx::FxLib>>,
    mut started: MessageReader<crate::fx::EffectStarted>,
    mut events: ResMut<SkyEvents>,
    mut table: Local<Option<std::collections::HashMap<usize, SkyEvent>>>,
) {
    let Some(sky) = &look.sky else { return };
    events.warm.step(time.delta_secs());
    let Some(lib) = lib else { return };
    let table = table.get_or_insert_with(|| lib.names().filter_map(|(n, i)| Some((i, sky.events.iter().find(|e| n.contains(&e.effect))?.clone()))).collect());
    for m in started.read() {
        if let Some(e) = table.get(&m.effect) {
            events.warm.set(m.at, e.color * e.strength, e.seconds);
        }
    }
}

/// Gives each baked sky its dome once the sky (and its stars, when it has some) are cubes, and moves the clock on.
#[allow(clippy::too_many_arguments)]
fn dome(
    mut commands: Commands,
    look: Res<Look>,
    assets: Res<AssetServer>,
    events: Res<SkyEvents>,
    frame: Res<super::frame::Frame>,
    noise: Option<Res<super::volnoise::NoiseVolume>>,
    mut warming: Local<bool>,
    skies: Query<(Entity, &HdSky, Option<&StarsImage>), With<Sky>>,
    domes: Query<(&ChildOf, &MeshMaterial3d<SkyMaterial>, &Dome)>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
) {
    let Some(block) = &look.sky else { return };
    // the clock, a storm's flash and the black holes the shader bends the sky round come from the frame data (`frame.rs`); the
    // material is only touched while an explosion's warmth fades on the gas, and once more to put it out
    let warm_now = events.warm.left > 0.0;
    if warm_now || *warming {
        let (warm, warm_dir) = events.warm.uniform(frame.camera());
        for (_, mat, cfg) in &domes {
            if let Some(mut m) = materials.get_mut(&mat.0) {
                m.extension.u = SkyUniform { warm, warm_dir, ..cfg.0.uniform() };
            }
        }
    }
    *warming = warm_now;
    for (e, hd, stars) in &skies {
        if domes.iter().any(|(p, ..)| p.parent() == e) {
            continue;
        }
        let cube = hd.0.path().map(|p| p.path().to_string_lossy().replace('\\', "/"));
        let sky = cube.as_deref().map_or_else(|| block.clone(), |c| block.for_sky(sky_name(c)));
        let want_stars = sky.stars.is_some_and(|t| t.brightness > 0.0);
        if sky.layers.is_empty() && sky.decks.is_empty() && sky.base.is_none() && !want_stars && sky.sheet.is_none() && sky.shooting_every <= 0.0 {
            continue;
        }
        let stars_handle = match stars {
            Some(s) => Some(s.0.clone()),
            None if want_stars => {
                let handle = cube.and_then(|c| stars_file(crate::assets_dir(), &c)).map(|f| assets.load::<Image>(f));
                if let Some(h) = &handle {
                    commands.entity(e).insert(StarsImage(h.clone()));
                }
                handle
            }
            None => None,
        };
        // a stars cube that does not load (a broken file) leaves the dome without stars instead of without a dome
        let stars_handle = stars_handle.filter(|h| !assets.load_state(h).is_failed());
        if !ready(&hd.0, &mut images) || stars_handle.as_ref().is_some_and(|h| !ready(h, &mut images)) {
            continue;
        }
        let Some(noise) = noise.as_ref() else { continue };
        let uniform = sky.uniform();
        let base = StandardMaterial { unlit: true, cull_mode: None, double_sided: true, alpha_mode: AlphaMode::Premultiplied, depth_bias: SORT_BIAS, ..default() };
        let material =
            materials.add(SkyMaterial { base, extension: SkyGlow { u: uniform, cube: Some(hd.0.clone()), stars: stars_handle, noise: Some(noise.0.clone()), frame: default() } });
        commands.entity(e).with_child((
            Dome(sky),
            Mesh3d(meshes.add(Sphere::new(1.0).mesh().uv(48, 24))),
            MeshMaterial3d(material),
            Transform::from_scale(Vec3::splat(DOME_SCALE)),
            bevy::light::NotShadowCaster,
            bevy::camera::visibility::NoFrustumCulling,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn absent_without_the_key_and_defaults_with_it() {
        assert!(parse(&Value::Null).is_none());
        let d = parse(&json!({"near": {}})).unwrap();
        assert_eq!(d.layers.len(), 1);
        assert_eq!(d.layers[0].opacity, 0.5);
        let d = parse(&json!({})).unwrap();
        assert!(d.layers.is_empty() && d.base.is_none() && d.sheet.is_none() && d.events.is_empty());
        assert_eq!(d.shooting_every, 0.0);
    }

    #[test]
    fn the_current_is_given_in_game_axes() {
        let d = parse(&json!({"near": {"drift": [1.0, 2.0, 3.0], "opacity": 4.0}})).unwrap();
        assert_eq!(d.layers[0].drift, Vec3::new(1.0, 3.0, -2.0), "x starboard, y forward, z up become Bevy's x, y up, -z forward");
        assert_eq!(d.layers[0].opacity, 1.0, "clamped");
    }

    #[test]
    fn near_is_one_layer_or_an_array_capped_at_three() {
        let one = parse(&json!({"near": {"distance": 900}})).unwrap();
        assert_eq!((one.layers.len(), one.layers[0].distance), (1, 900.0));
        let many = parse(&json!({"near": [{"distance": 6000, "scale": 1}, {"distance": 1500, "tint": [1, 0.5, 0.2]}, {}, {}, {}]})).unwrap();
        assert_eq!(many.layers.len(), MAX_LAYERS);
        assert_eq!((many.layers[0].distance, many.layers[1].distance, many.layers[1].tint), (6000.0, 1500.0, Vec3::new(1.0, 0.5, 0.2)));
        let u = many.uniform();
        assert_eq!((u.info.x, u.layers[1].a.w, u.layers[0].b.x), (3.0, 1500.0, 1.0));
        assert!(parse(&json!({"near": [{"opacity": 0}]})).unwrap().layers.is_empty(), "a layer of no strength draws nothing");
    }

    #[test]
    fn clouds_are_optional_decks_capped_at_three_with_defaults() {
        assert!(parse(&json!({"near": {}})).unwrap().decks.is_empty(), "no `clouds` key, no decks");
        let one = parse(&json!({"clouds": {"altitude": -200}})).unwrap();
        assert_eq!((one.decks.len(), one.decks[0].altitude, one.decks[0].steps), (1, -200.0, 6));
        let many = parse(&json!({"clouds": [{"altitude": 900}, {"altitude": 300}, {}, {}, {}]})).unwrap();
        assert_eq!(many.decks.len(), MAX_DECKS, "capped");
        assert_eq!((many.decks[0].altitude, many.decks[1].altitude, many.decks[2].altitude), (900.0, 300.0, 600.0));
        assert_eq!(many.uniform().info.z, 3.0);
        assert_eq!(parse(&json!({"clouds": [{"altitude": 900, "opacity": 0}, {"altitude": 5}]})).unwrap().decks.len(), 1, "a deck of no strength draws nothing");
        let d = parse(&json!({"clouds": [{"coverage": 3, "steps": 99, "drift": [1, 2, 3], "sun": [0, 0, 2]}]})).unwrap().decks[0];
        assert_eq!((d.coverage, d.steps, d.drift), (1.0, MAX_STEPS, Vec3::new(1.0, 3.0, -2.0)), "clamped; the current in game axes");
        assert_eq!(d.sun, Some(Vec3::new(0.0, 1.0, 0.0)), "the sun is a unit direction, up in game axes is Bevy's y");
        assert_eq!(parse(&json!({"clouds": [{}]})).unwrap().decks[0].sun, None);
    }

    #[test]
    fn a_decks_uniform_carries_every_field() {
        let u = parse(&json!({"clouds": [{"altitude": 5, "thickness": 6, "coverage": 0.25, "density": 7, "scale": 8, "opacity": 0.5, "haze": 9, "churn": 0.1, "silver": 2,
            "detail": 0.75, "lit": [1, 0.5, 0.25], "shadow": [0, 0.1, 0.2], "haze_color": [0.3, 0.4, 0.5], "steps": 4, "sun": [0, 1, 0]}]}))
        .unwrap()
        .uniform()
        .decks[0];
        assert_eq!((u.a, u.b), (Vec4::new(5.0, 6.0, 0.25, 7.0), Vec4::new(8.0, 0.5, 9.0, 0.1)));
        assert_eq!((u.c.w, u.d, u.e, u.f, u.g.truncate()), (2.0, Vec4::new(1.0, 0.5, 0.25, 0.75), Vec4::new(0.0, 0.1, 0.2, 4.0), Vec4::new(0.3, 0.4, 0.5, 1.0), Vec3::NEG_Z));
    }

    #[test]
    fn clouds_merge_per_sky_as_a_whole_array() {
        let d = parse(&json!({"clouds": [{"altitude": 100}], "skies": {"skybox_05": {"clouds": [{"altitude": 1}, {"altitude": 2}]}}})).unwrap();
        assert_eq!(d.decks.len(), 1);
        let s = d.for_sky("skybox_05");
        assert_eq!(s.decks.iter().map(|x| x.altitude).collect::<Vec<_>>(), [1.0, 2.0], "an array is replaced, like `near`");
    }

    #[test]
    fn a_skys_accents_merge_over_the_defaults() {
        let d = parse(&json!({
            "near": {"distance": 3000, "opacity": 0.4}, "stars": {"twinkle": 0.3},
            "skies": {"skybox_05": {"sheet_lightning": {"every": 30}, "near": {"opacity": 0.8}}, "SKYBOX_16": {"base": {"breath": 0.2}}},
        }))
        .unwrap();
        let plain = d.for_sky("skybox_01");
        assert!(plain.sheet.is_none() && plain.base.is_none());
        let s = d.for_sky("skybox_05");
        assert_eq!(s.sheet.map(|x| x.every), Some(30.0));
        assert_eq!((s.layers[0].opacity, s.layers[0].distance, s.stars.map(|t| t.twinkle)), (0.8, 3000.0, Some(0.3)), "objects merge key by key");
        assert_eq!(d.for_sky("skybox_16").base.map(|b| b.breath), Some(0.2), "keys are not case-sensitive");
    }

    #[test]
    fn a_cube_that_is_ready_is_not_touched_again() {
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default())).init_asset::<Image>();
        let faces = Image::new_fill(
            Extent3d { width: 4, height: 24, depth_or_array_layers: 1 },
            TextureDimension::D2,
            &[0, 0, 0, 255],
            TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::default(),
        );
        let h = app.world_mut().resource_mut::<Assets<Image>>().add(faces);
        app.update();
        let modified = |app: &mut App| {
            let msgs = app.world_mut().resource_mut::<Messages<AssetEvent<Image>>>();
            let mut reader = msgs.get_cursor();
            reader.read(&msgs).filter(|e| matches!(e, AssetEvent::Modified { .. })).count()
        };
        let before = modified(&mut app);
        assert!(ready(&h, &mut app.world_mut().resource_mut::<Assets<Image>>()), "six stacked faces become a cube");
        app.update();
        let converted = modified(&mut app);
        assert!(converted > before, "the conversion is an upload");
        for _ in 0..3 {
            assert!(ready(&h, &mut app.world_mut().resource_mut::<Assets<Image>>()));
            app.update();
        }
        assert_eq!(modified(&mut app), converted, "a ready cube is never borrowed mutably again, so never uploaded again");
        assert!(!ready(&Handle::default(), &mut app.world_mut().resource_mut::<Assets<Image>>()), "not loaded, not ready");
    }

    #[test]
    fn the_sky_is_named_from_its_cube() {
        assert_eq!(sky_name("models/sky/skybox_05_cube.png"), "skybox_05");
    }

    #[test]
    fn a_flash_fades_to_nothing() {
        let mut p = Pulse::default();
        p.set(Vec3::X * 100.0, Vec3::ONE, 0.4);
        assert_eq!(p.uniform(Vec3::ZERO).0, Vec4::new(1.0, 1.0, 1.0, 0.0));
        assert_eq!(p.uniform(Vec3::ZERO).1.truncate(), Vec3::X);
        for _ in 0..10 {
            p.step(0.1);
        }
        assert_eq!(p.left, 0.0);
        assert_eq!(p.uniform(Vec3::ZERO).0, Vec4::ZERO);
    }

    #[test]
    fn backdrop_brightness_defaults_to_neutral_and_is_independent_of_stars() {
        let sky = parse(&json!({"base": {"brightness": 0.7}, "stars": {"brightness": 1.2}, "skies": {"skybox_16": {"base": {"breath": 0.2}}}})).unwrap();
        let u = sky.for_sky("skybox_16").uniform();
        assert_eq!((1.0 + u.base_tint.w, u.stars.x), (0.7, 1.2), "per-sky accents keep the default brightness unless they override it");
        assert_eq!(parse(&json!({"base": {}})).unwrap().uniform().base_tint.w, 0.0);
    }

    #[test]
    fn the_flash_is_configured_here_and_struck_in_the_frame_data() {
        let u = parse(&json!({"flash": {"color": [1, 0.5, 0.25], "strength": 2, "seconds": 0.5, "reach": 900}})).unwrap().uniform();
        assert_eq!((u.flash, u.flash_reach.x), (Vec4::new(2.0, 1.0, 0.5, 0.5), 900.0));
        assert_eq!(u.warm, Vec4::ZERO, "no explosion, no warmth");
    }

    #[test]
    fn stars_are_optional_and_the_near_layer_needs_its_key() {
        let d = parse(&json!({"stars": {"twinkle": 3.0}})).unwrap();
        assert!(d.layers.is_empty(), "no `near` key, no near layer");
        let t = d.stars.unwrap();
        assert_eq!((t.brightness, t.twinkle, t.gain), (1.0, 1.0, 16.0));
        assert!(parse(&json!({"near": {}})).unwrap().stars.is_none());
        assert_eq!(d.uniform().stars, Vec4::new(1.0, 1.0, 2.5, 16.0));
        assert_eq!(parse(&json!({"near": {}})).unwrap().uniform().stars, Vec4::ZERO);
    }

    #[test]
    fn the_stars_cube_sits_beside_the_sky_cube() {
        let dir = std::env::temp_dir().join(format!("op-sky-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("models/sky")).unwrap();
        assert_eq!(stars_file(&dir, "models/sky/skybox_07_cube.png"), None);
        std::fs::write(dir.join("models/sky/skybox_07_stars.png"), b"").unwrap();
        assert_eq!(stars_file(&dir, "models/sky/skybox_07_cube.png").as_deref(), Some("models/sky/skybox_07_stars.png"));
        assert_eq!(stars_file(&dir, "models/sky/skybox_07.glb"), None);
        std::fs::remove_dir_all(dir).ok();
    }
}
