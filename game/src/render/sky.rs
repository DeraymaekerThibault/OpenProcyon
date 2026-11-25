//! Not retail: the sky dome of the render profile (`sky` block, drawn with the mod's `shaders/sky.wesl`). The baked sky cube
//! (`view::hd_sky`) stays the base picture and the image-based light; this adds a camera-centred dome just inside it
//! (an inverted unit sphere, child of the `view::Sky` entity, so it follows the camera and never veils a ship: the depth
//! test keeps it behind the world, and its sort bias draws it before every other blended effect).
//!
//! The near layers (up to three) are the near ether: a sparse, soft copy of the sky's own gas, sampled as if it floated
//! `distance` units out, so it slides against the sky as the camera moves (parallax), and carried along a map-wide current
//! (`drift`, world units a second). Each layer is broken up by slowly churning 3-D noise so it reads as wisps, not as a ghost
//! of the backdrop. A rigid turn of the whole sky reads as the camera rolling; relative motion reads as depth.
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

/// The `sky` block.
#[derive(Clone, Debug, PartialEq)]
pub struct SkyDome {
    pub layers: Vec<NearLayer>,
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
    /// Whether the layer shows the painted clouds cube (`source: "clouds"`) instead of the sky's own gas. Then `threshold` and
    /// `softness` act on the cloud's alpha (both 0 by default: alpha passes unchanged).
    pub clouds: bool,
    /// Aerial perspective, 0..1: the share the layer's colour sinks into the backdrop behind it, and loses contrast.
    pub haze: f32,
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
    rgb(v).map(|d| crate::data::conv(d.x, d.y, d.z))
}

// What a profile's sky keys fall back to when it leaves them out. All PROVISIONAL.
/// A near layer's distance from the camera.
const LAYER_DISTANCE: f32 = 4000.0;
/// A near layer's opacity.
const LAYER_OPACITY: f32 = 0.5;
/// Where a picture layer cuts its picture's brightness off, and how softly.
const LAYER_THRESHOLD: f32 = 0.3;
const LAYER_SOFTNESS: f32 = 0.35;
/// The same for a cloud layer, which has no cut-off.
const CLOUD_THRESHOLD: f32 = 0.0;
const CLOUD_SOFTNESS: f32 = 0.0;
/// How many times the picture repeats over a near layer.
const LAYER_SCALE: f32 = 2.5;
/// Star brightness, twinkle depth, twinkle speed and gain.
const STARS_BRIGHTNESS: f32 = 1.0;
const STARS_TWINKLE: f32 = 0.4;
const STARS_SPEED: f32 = 2.5;
const STARS_GAIN: f32 = 16.0;
/// The base picture's brightness, drift rate, drift cycle (seconds), breathing depth and breathing period (seconds).
const BASE_BRIGHTNESS: f32 = 1.0;
const BASE_FLOW: f32 = 0.02;
const BASE_CYCLE: f32 = 30.0;
const BASE_BREATH: f32 = 0.05;
const BASE_PERIOD: f32 = 30.0;
/// The sky's flash on a hit: colour (linear rgb, pale blue), strength, length (seconds) and reach (game units).
const FLASH_COLOR: Vec3 = Vec3::new(0.65, 0.78, 1.0);
const FLASH_STRENGTH: f32 = 1.5;
const FLASH_SECONDS: f32 = 0.4;
const FLASH_REACH: f32 = 1200.0;
/// A sky event's colour (linear rgb, warm orange), strength and length (seconds).
const EVENT_COLOR: Vec3 = Vec3::new(1.0, 0.6, 0.3);
const EVENT_STRENGTH: f32 = 1.0;
const EVENT_SECONDS: f32 = 1.0;
/// A shooting star every this many seconds, and how bright it is.
const SHOOTING_EVERY: f32 = 180.0;
const SHOOTING_BRIGHTNESS: f32 = 6.0;
/// Sheet lightning every this many seconds, its strength and colour (linear rgb).
const SHEET_EVERY: f32 = 40.0;
const SHEET_STRENGTH: f32 = 1.5;
const SHEET_COLOR: Vec3 = Vec3::new(0.7, 0.8, 1.0);

fn layer(near: &Value) -> NearLayer {
    let clouds = near["source"].as_str().is_some_and(|s| s.eq_ignore_ascii_case("clouds"));
    NearLayer {
        distance: num(&near["distance"], LAYER_DISTANCE).max(1.0),
        opacity: num(&near["opacity"], LAYER_OPACITY).clamp(0.0, 1.0),
        threshold: num(&near["threshold"], if clouds { CLOUD_THRESHOLD } else { LAYER_THRESHOLD }).clamp(0.0, 1.0),
        softness: if clouds { num(&near["softness"], CLOUD_SOFTNESS).max(0.0) } else { num(&near["softness"], LAYER_SOFTNESS).max(0.01) },
        drift: game_vec(&near["drift"]).unwrap_or(Vec3::ZERO),
        scale: num(&near["scale"], LAYER_SCALE).max(0.0),
        tint: rgb(&near["tint"]).unwrap_or(Vec3::ONE).max(Vec3::ZERO),
        clouds,
        haze: num(&near["haze"], 0.0).clamp(0.0, 1.0),
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
    Some(SkyDome {
        layers,
        stars: v["stars"].is_object().then(|| {
            let t = &v["stars"];
            Stars {
                brightness: num(&t["brightness"], STARS_BRIGHTNESS).max(0.0),
                twinkle: num(&t["twinkle"], STARS_TWINKLE).clamp(0.0, 1.0),
                speed: num(&t["speed"], STARS_SPEED).max(0.0),
                gain: num(&t["gain"], STARS_GAIN).max(0.0),
            }
        }),
        base: v["base"].is_object().then(|| {
            let b = &v["base"];
            Base {
                brightness: num(&b["brightness"], BASE_BRIGHTNESS).max(0.0),
                flow: num(&b["flow"], BASE_FLOW).clamp(0.0, 0.2),
                cycle: num(&b["cycle"], BASE_CYCLE).max(1.0),
                breath: num(&b["breath"], BASE_BREATH).clamp(0.0, 1.0),
                period: num(&b["period"], BASE_PERIOD).max(1.0),
                tint: rgb(&b["tint"]).unwrap_or(Vec3::ONE).max(Vec3::ZERO),
            }
        }),
        flash: Flash {
            color: rgb(&v["flash"]["color"]).unwrap_or(FLASH_COLOR).max(Vec3::ZERO),
            strength: num(&v["flash"]["strength"], FLASH_STRENGTH).max(0.0),
            seconds: num(&v["flash"]["seconds"], FLASH_SECONDS).max(0.05),
            reach: num(&v["flash"]["reach"], FLASH_REACH).max(0.0),
        },
        events: v["events"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|e| {
                        Some(SkyEvent {
                            effect: e["match"].as_str()?.to_string(),
                            color: rgb(&e["color"]).unwrap_or(EVENT_COLOR).max(Vec3::ZERO),
                            strength: num(&e["strength"], EVENT_STRENGTH).max(0.0),
                            seconds: num(&e["seconds"], EVENT_SECONDS).max(0.05),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        shooting_every: if v["shooting_star"].is_object() { num(&v["shooting_star"]["every"], SHOOTING_EVERY).max(10.0) } else { 0.0 },
        shooting_brightness: num(&v["shooting_star"]["brightness"], SHOOTING_BRIGHTNESS).max(0.0),
        sheet: v["sheet_lightning"].is_object().then(|| Sheet {
            every: num(&v["sheet_lightning"]["every"], SHEET_EVERY).max(5.0),
            strength: num(&v["sheet_lightning"]["strength"], SHEET_STRENGTH).max(0.0),
            color: rgb(&v["sheet_lightning"]["color"]).unwrap_or(SHEET_COLOR).max(Vec3::ZERO),
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
    /// x: 1 for a painted cloud layer, y: haze
    pub d: Vec4,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, ShaderType)]
pub struct SkyUniform {
    /// x: brightness (0 = no stars), y: twinkle depth, z: twinkles a second, w: gain
    pub stars: Vec4,
    /// x: number of near layers, y: 1 when the far sky is redrawn
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
}

impl SkyDome {
    fn uniform(&self) -> SkyUniform {
        let mut layers = [LayerUniform::default(); MAX_LAYERS];
        for (u, l) in layers.iter_mut().zip(&self.layers) {
            *u = LayerUniform {
                a: Vec4::new(l.opacity, l.threshold, l.softness, l.distance),
                b: Vec4::new(l.scale, l.tint.x, l.tint.y, l.tint.z),
                c: l.drift.extend(0.0),
                d: Vec4::new(if l.clouds { 1.0 } else { 0.0 }, l.haze, 0.0, 0.0),
            };
        }
        let (base, base_tint) = self.base.map_or((Vec4::ZERO, Vec4::ZERO), |b| (Vec4::new(b.flow, b.cycle, b.breath, b.period), b.tint.extend(b.brightness - 1.0)));
        SkyUniform {
            stars: self.stars.map_or(Vec4::ZERO, |t| Vec4::new(t.brightness, t.twinkle, t.speed, t.gain)),
            info: Vec4::new(self.layers.len().min(MAX_LAYERS) as f32, if self.base.is_some() { 1.0 } else { 0.0 }, 0.0, 0.0),
            base,
            base_tint,
            flash: (self.flash.color * self.flash.strength).extend(self.flash.seconds),
            flash_reach: Vec4::new(self.flash.reach, 0.0, 0.0, 0.0),
            shoot: Vec4::new(self.shooting_every, self.shooting_brightness, 0.0, 0.0),
            sheet: self.sheet.map_or(Vec4::ZERO, |s| Vec4::new(s.every, s.strength, 0.0, 0.0)),
            sheet_color: self.sheet.map_or(Vec4::ZERO, |s| s.color.extend(0.0)),
            layers,
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
    /// The painted clouds (`<sky>_clouds.png`, straight-alpha rgba); none when the sky has none, and then its cloud layers draw nothing.
    #[texture(107, dimension = "cube")]
    #[sampler(108)]
    pub clouds: Option<Handle<Image>>,
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
        app.add_systems(Update, loading.run_if(resource_exists::<crate::loading::Loading>));
    }
}

/// The sky's star and cloud pictures hold a loading battle (`loading.rs`).
fn loading(mut loading: ResMut<crate::loading::Loading>, server: Res<AssetServer>, stars: Query<&StarsImage>, clouds: Query<&CloudsImage>) {
    let pictures = stars.iter().map(|s| &s.0).chain(clouds.iter().map(|c| &c.0));
    if pictures.into_iter().any(|h| crate::loading::pending(&server, h.id().untyped())) {
        loading.busy("the sky");
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

/// The clouds cube that goes with a baked sky (`<sky>_clouds.png`), when the mod has one.
fn clouds_file(root: &std::path::Path, cube: &str) -> Option<String> {
    let rel = cube.strip_suffix("_cube.png")?.to_string() + "_clouds.png";
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

/// Whether a loaded clouds image is (now) a cube map. Before it becomes one, the colour of fully transparent texels is filled in
/// from their neighbours (`bleed_colour`), so bilinear filtering at a cloud's edge never drags in black.
fn ready_clouds(h: &Handle<Image>, images: &mut Assets<Image>) -> bool {
    match images.get(h) {
        None => false,
        Some(i) if i.texture_descriptor.size.depth_or_array_layers == 6 => true,
        Some(_) => images.get_mut(h).is_some_and(|mut i| {
            let (w, ok) = (
                i.width() as usize,
                i.height() == i.width() * 6
                    && matches!(
                        i.texture_descriptor.format,
                        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb | bevy::render::render_resource::TextureFormat::Rgba8Unorm
                    ),
            );
            if ok && let Some(d) = i.data.as_mut() {
                bleed_colour(d, w);
            }
            crate::view::as_cube(&mut i)
        }),
    }
}

/// Gives every fully transparent texel of the stacked square faces (RGBA8, `w` wide) the average colour of its opaque-ish
/// neighbours, two texels deep, within its own face. The colour of an invisible texel is whatever the baker left there.
fn bleed_colour(data: &mut [u8], w: usize) {
    if w == 0 || data.len() < w * w * 6 * 4 {
        return;
    }
    for face in data.chunks_exact_mut(w * w * 4).take(6) {
        let mut solid: Vec<bool> = face.as_chunks::<4>().0.iter().map(|p| p[3] > 0).collect();
        for _ in 0..2 {
            let prev = face.to_vec();
            let was = solid.clone();
            for y in 0..w {
                for x in 0..w {
                    if was[y * w + x] {
                        continue;
                    }
                    let (mut sum, mut n) = ([0u32; 3], 0u32);
                    for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
                        if nx < w && ny < w && was[ny * w + nx] {
                            let i = (ny * w + nx) * 4;
                            (0..3).for_each(|c| sum[c] += prev[i + c] as u32);
                            n += 1;
                        }
                    }
                    if n > 0 {
                        let i = (y * w + x) * 4;
                        (0..3).for_each(|c| face[i + c] = (sum[c] / n) as u8);
                        solid[y * w + x] = true;
                    }
                }
            }
        }
    }
}

/// The clouds image a sky is waiting for.
#[derive(Component)]
pub(super) struct CloudsImage(pub(super) Handle<Image>);

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
    skies: Query<(Entity, &HdSky, Option<&StarsImage>, Option<&CloudsImage>), With<Sky>>,
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
    for (e, hd, stars, clouds) in &skies {
        if domes.iter().any(|(p, ..)| p.parent() == e) {
            continue;
        }
        let cube = hd.0.path().map(|p| p.path().to_string_lossy().replace('\\', "/"));
        let mut sky = cube.as_deref().map_or_else(|| block.clone(), |c| block.for_sky(sky_name(c)));
        let want_stars = sky.stars.is_some_and(|t| t.brightness > 0.0);
        // cloud layers need their cube: without one they stay in place (the layers' turns go by index) but draw nothing
        let want_clouds = sky.layers.iter().any(|l| l.clouds);
        let clouds_handle = match clouds {
            Some(c) => Some(c.0.clone()),
            None if want_clouds => {
                let handle = cube.as_deref().and_then(|c| clouds_file(crate::assets_dir(), c)).map(|f| assets.load::<Image>(f));
                if let Some(h) = &handle {
                    commands.entity(e).insert(CloudsImage(h.clone()));
                }
                handle
            }
            None => None,
        };
        let clouds_handle = clouds_handle.filter(|h| !assets.load_state(h).is_failed());
        if clouds_handle.as_ref().is_some_and(|h| !ready_clouds(h, &mut images)) {
            continue;
        }
        if clouds_handle.is_none() {
            sky.layers.iter_mut().filter(|l| l.clouds).for_each(|l| l.opacity = 0.0);
        }
        if sky.layers.iter().all(|l| l.opacity <= 0.0) && sky.base.is_none() && !want_stars && sky.sheet.is_none() && sky.shooting_every <= 0.0 {
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
        let material = materials.add(SkyMaterial {
            base,
            extension: SkyGlow { u: uniform, cube: Some(hd.0.clone()), stars: stars_handle, noise: Some(noise.0.clone()), clouds: clouds_handle, frame: default() },
        });
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

    #[test]
    fn a_layers_source_and_haze_parse_with_defaults() {
        let d = parse(&json!({"near": [{}, {"source": "clouds", "haze": 0.6}, {"source": "Clouds", "haze": 4, "threshold": 0.2, "softness": 0.1}, {"source": "bogus"}]})).unwrap();
        assert_eq!(d.layers.len(), 3);
        let (sky, clouds, strict) = (d.layers[0], d.layers[1], d.layers[2]);
        assert!(!sky.clouds && sky.haze == 0.0 && (sky.threshold, sky.softness) == (0.3, 0.35), "today's defaults");
        assert!(clouds.clouds && clouds.haze == 0.6);
        assert_eq!((clouds.threshold, clouds.softness), (0.0, 0.0), "alpha passes unchanged unless asked");
        assert!(strict.clouds && strict.haze == 1.0, "haze is clamped, the source is not case-sensitive");
        assert_eq!((strict.threshold, strict.softness), (0.2, 0.1));
        assert!(!parse(&json!({"near": {"source": "bogus"}})).unwrap().layers[0].clouds, "an unknown source is the sky");
    }

    #[test]
    fn the_new_values_are_packed_into_the_layer_uniform() {
        let u = parse(&json!({"near": [{"distance": 900}, {"source": "clouds", "haze": 0.3}]})).unwrap().uniform();
        assert_eq!(u.layers[0].d, Vec4::ZERO, "a plain layer packs as before");
        assert_eq!(u.layers[1].d, Vec4::new(1.0, 0.3, 0.0, 0.0));
    }

    #[test]
    fn source_and_haze_merge_per_sky() {
        let d = parse(&json!({"near": {"distance": 3000}, "skies": {"shakedown_sky": {"near": {"source": "clouds", "haze": 0.5}, "stars": {}}}})).unwrap();
        assert!(!d.for_sky("other").layers[0].clouds);
        let s = d.for_sky("Shakedown_Sky");
        assert!(s.layers[0].clouds && s.layers[0].haze == 0.5 && s.layers[0].distance == 3000.0, "objects merge key by key");
    }

    #[test]
    fn the_clouds_cube_sits_beside_the_sky_cube() {
        let dir = std::env::temp_dir().join(format!("op-sky-clouds-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("models/sky")).unwrap();
        assert_eq!(clouds_file(&dir, "models/sky/s_cube.png"), None);
        std::fs::write(dir.join("models/sky/s_clouds.png"), b"").unwrap();
        assert_eq!(clouds_file(&dir, "models/sky/s_cube.png").as_deref(), Some("models/sky/s_clouds.png"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn transparent_texels_take_the_colour_of_their_neighbours() {
        let w = 4;
        let mut data = vec![0u8; w * w * 6 * 4];
        let at = |x: usize, y: usize| (y * w + x) * 4;
        data[at(1, 1)..at(1, 1) + 4].copy_from_slice(&[200, 100, 50, 255]);
        bleed_colour(&mut data, w);
        assert_eq!(data[at(2, 1)..at(2, 1) + 4], [200, 100, 50, 0], "colour spreads, alpha stays 0");
        assert_eq!(data[at(3, 1)..at(3, 1) + 4], [200, 100, 50, 0], "two texels deep");
        assert_eq!(data[at(0, 0)..at(0, 0) + 3], [200, 100, 50], "diagonal reached in two steps");
        assert_eq!(data[w * w * 4..w * w * 4 + 4], [0, 0, 0, 0], "other faces are untouched");
        assert_eq!(data[at(1, 1) + 3], 255);
    }
}
