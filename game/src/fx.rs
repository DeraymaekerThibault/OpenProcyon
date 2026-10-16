//! Particle effects from the original .eft files (exported to JSON by tools/export_effects.py).
//! CPU particles, drawn as camera-facing quads in one dynamic mesh per (texture, blend). The meaning of the emitter
//! parameters is inferred from the data, not measured from the original renderer (see AGENTS.md, rule 5).

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor, SpecializedMeshPipelineError};
use serde_json::Value;
use std::collections::HashMap;

use crate::assets_dir;
use crate::combat::Transient;

/// Retail ONE/ONE blending must not premultiply texture alpha a second time (Bevy AlphaMode::Add does).
#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct RetailAdd {}
impl MaterialExtension for RetailAdd {
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let component = BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::One, operation: BlendOperation::Add };
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState { color: component, alpha: component });
            }
        }
        // `depth_bias` is only a sort key here (see `additive_material`); keep the GPU depth test unbiased.
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.bias.constant = 0;
        }
        Ok(())
    }
}
pub type EffectMaterial = ExtendedMaterial<StandardMaterial, RetailAdd>;
/// Transparent sort bias for additive effects. Their pooled meshes span the whole map, so Bevy's centre-distance sort could draw
/// them before a nearer sail, which then painted over them (PT-05). PROVISIONAL: drawn after every alpha-blended mesh, so a trail
/// behind a translucent sail shows at full strength instead of dimmed; per-emitter sorting would fix that.
pub const ADDITIVE_SORT_BIAS: f32 = 10_000.0;
pub fn additive_material(texture: Handle<Image>) -> EffectMaterial {
    EffectMaterial {
        base: StandardMaterial {
            base_color_texture: Some(texture),
            unlit: true,
            cull_mode: None,
            double_sided: true,
            alpha_mode: AlphaMode::Blend,
            depth_bias: ADDITIVE_SORT_BIAS,
            ..default()
        },
        extension: RetailAdd {},
    }
}

/// The `curve` key list (time, value) of an exported interpolator; missing numbers read as 0.
pub fn curve_keys(v: &Value) -> Vec<(f32, f32)> {
    v["curve"].as_array().map(|a| a.iter().map(|k| (k[0].as_f64().unwrap_or(0.0) as f32, k[1].as_f64().unwrap_or(0.0) as f32)).collect()).unwrap_or_default()
}

#[derive(Clone, Default)]
struct Curve(Vec<(f32, f32)>, Option<(f32, f32, u32)>);
impl Curve {
    fn from(v: Option<&Value>) -> Curve {
        let keys = v.map(curve_keys);
        let looping = v.and_then(|v| v.get("loop")).and_then(|l| {
            let (start, end, count) = (l["StartLoopingT"].as_f64()? as f32, l["EndLoopingT"].as_f64()? as f32, l["NumberLoops"].as_u64()?);
            (start.is_finite() && end.is_finite() && end > start && count > 0).then_some((start, end, count.min(u32::MAX as u64) as u32))
        });
        Curve(keys.unwrap_or_default(), looping)
    }
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    fn last_t(&self) -> f32 {
        self.0.last().map_or(0.0, |k| k.0)
    }
    /// Linear between keys, with authored interval repeats; held outside the keys, `def` when empty.
    fn at(&self, mut t: f32, def: f32) -> f32 {
        if let Some((start, end, count)) = self.1 {
            let period = end - start;
            t -= ((t - start) / period).floor().clamp(0.0, count as f32) * period;
        }
        let k = &self.0;
        match k.len() {
            0 => def,
            1 => k[0].1,
            _ => {
                if t <= k[0].0 {
                    return k[0].1;
                }
                for w in k.windows(2) {
                    if t <= w[1].0 {
                        let f = (t - w[0].0) / (w[1].0 - w[0].0).max(1e-6);
                        return w[0].1 + (w[1].1 - w[0].1) * f;
                    }
                }
                k[k.len() - 1].1
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Blend {
    Add,
    Alpha,
    Multiply,
}

struct MorphMesh {
    source: Vec<Vec3>,
    destination: Vec<Vec3>,
    uv: Vec<[f32; 2]>,
    indices: Vec<u32>,
    model: String,
    weight: Curve,
}

struct Prim {
    mesh: Option<MorphMesh>,
    duration: f32,
    delay: f32,
    ever: bool,
    world: bool,
    rate: Curve,
    start: u32,
    pos_t: [Curve; 3],
    pos_r: Curve,
    pos_l: Curve,
    pos_le: Curve,
    vel_t: [Curve; 3],
    vel_r: Curve,
    vel_l: Curve,
    vel_le: Curve,
    target_on: bool,
    target_pos_velocity: bool,
    angle_on: bool,
    angle_pos_velocity: bool,
    vel_ang_dir: [Curve; 3],
    vel_ang_min: Curve,
    vel_ang_max: Curve,
    vel_ang_len: Curve,
    vel_ang_len_err: Curve,
    ang_dir: [Curve; 3],
    ang_min: Curve,
    ang_max: Curve,
    ang_len: Curve,
    ang_len_err: Curve,
    tex: Option<usize>,
    blend: Blend,
    size: Curve,
    rgb: [Curve; 3],
    alpha: Curve,
    rect: Option<[Curve; 4]>,
    flat: bool,
    along_velocity: bool,
    pseudo_dir: Option<Vec3>, // fixed streak axis in effect space (PseudoBillboardChunk)

    life: Option<Curve>,
    attract: Option<([Curve; 3], [Curve; 3])>,
    fallback_life: f32,
    low_rate: f32,      // LowDetailParticleRateMultiply
    src: Option<usize>, // spawn at the positions of this primitive's particles (invisible "source" emitters)
}
struct Effect {
    prims: Vec<Prim>,
}

#[derive(Resource)]
pub struct FxLib {
    effects: HashMap<String, usize>,
    list: Vec<Effect>,
    tex_names: Vec<String>,
}
#[derive(Resource, Default)]
struct FxTextures(Vec<Handle<Image>>);
#[derive(Resource)]
struct FxRng(u64);
impl FxRng {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    fn sphere(&mut self) -> Vec3 {
        loop {
            let v = Vec3::new(self.f() * 2.0 - 1.0, self.f() * 2.0 - 1.0, self.f() * 2.0 - 1.0);
            if v.length_squared() <= 1.0 {
                return v;
            }
        }
    }
}

struct P {
    pos: Vec3,
    vel: Vec3,
    age: f32,
    life: f32,
}
struct PrimState {
    started: bool,
    parts: Vec<P>,
}

/// A running effect. Everlasting effects (engines) never finish; others despawn when done and empty.
#[derive(Component)]
pub struct Fx {
    eff: usize,
    age: f32,
    tick_acc: f32,
    move_acc: f32,
    previous_world: Transform,
    prims: Vec<PrimState>,
    pub rate_scale: f32,
    pub size_scale: f32,
    pub emitting: bool,
    pub src_half: Vec3,
    meshes: Vec<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
}

impl Fx {
    pub fn particle_count(&self) -> usize {
        self.prims.iter().map(|p| p.parts.len()).sum()
    }
}

/// Game frame (x right, y bow, z up) -> Bevy (x, z, -y).
fn conv(v: Vec3) -> Vec3 {
    crate::data::conv(v.x, v.y, v.z)
}

fn first(v: Option<&Value>) -> Option<&Value> {
    match v {
        Some(Value::Array(a)) => a.first(),
        other => other,
    }
}
fn get<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cur = v;
    for k in path {
        cur = first(Some(cur.get(*k)?))?;
    }
    Some(cur)
}
fn c3(v: Option<&Value>) -> [Curve; 3] {
    ["Value1Interpolator", "Value2Interpolator", "Value3Interpolator"].map(|k| Curve::from(v.and_then(|v| v.get(k))))
}

fn parse_prim(p: &Value, tex_names: &mut Vec<String>) -> Prim {
    let flag = |k: &str| p[k].as_bool().unwrap_or(false);
    let num = |k: &str| p[k].as_f64().unwrap_or(0.0) as f32;
    let pe = p.get("PositionEmitter");
    let ve = p.get("VelocityEmitter");
    let pa = p.get("CHUNKDATA_POSITION_ANGLE_EMITTER");
    let va = p.get("CHUNKDATA_VELOCITY_ANGLE_EMITTER");
    // Per-spawn chunks (colour fader, size, rectangle, rotation) override the common ones; the original creates them per spawned
    // batch, so their curves run over the particle's own age (e.g. the engine's orange fade lives only here).
    let mut merged = p["CommonPropertiesChunk"].clone();
    if let (Some(m), Some(sp)) = (merged.as_object_mut(), p["SpawnedEachTimePropertiesChunk"].as_object()) {
        for (k, v) in sp {
            if k != "RenderNumInfos" && k != "MovementNumInfos" {
                m.insert(k.clone(), v.clone());
            }
        }
    }
    let cp = &merged;
    let tex = get(cp, &["SimpleTextureChunk", "TextureName"]).and_then(|t| t.as_str()).map(|t| {
        let n = std::path::Path::new(t).file_stem().unwrap().to_string_lossy().to_lowercase();
        tex_names.iter().position(|x| *x == n).unwrap_or_else(|| {
            tex_names.push(n);
            tex_names.len() - 1
        })
    });
    let src = get(cp, &["BlendChunk", "BlendSource"]).and_then(|v| v.as_str()).unwrap_or("SRCALPHA");
    let dst = get(cp, &["BlendChunk", "BlendDestination"]).and_then(|v| v.as_str()).unwrap_or("INVSRCALPHA");
    let blend = if dst == "ONE" {
        Blend::Add
    } else if dst == "SRCCOLOR" || dst == "ZERO" {
        Blend::Multiply
    } else {
        let _ = src;
        Blend::Alpha
    };
    let cc = get(cp, &["ColorFaderChunk", "ColorInterpolator3Chunk"]);
    let rgb = c3(cc);
    let alpha = Curve::from(get(cp, &["ColorFaderChunk", "AlphaComponentChunk"]));
    let size = Curve::from(get(cp, &["SizeChunk", "SizeInterpolatorChunk"]));
    let rect = get(cp, &["Rectangle2dChunk"])
        .map(|r| ["TopLeftXInterpolatorChunk", "TopLeftYInterpolatorChunk", "BottomRightXInterpolatorChunk", "BottomRightYInterpolatorChunk"].map(|k| Curve::from(r.get(k))));
    let life = get(cp, &["ExpirationChunk", "ExpirationInterpolateChunk"]).map(|c| Curve::from(Some(c))).filter(|c| !c.is_empty());
    let attract = get(cp, &["AttractionChunk"]).map(|a| {
        (
            c3(a.get("PositionInterpolator3Chunk")),
            ["InterpolatorComponentAttractionConst", "InterpolatorComponentAttractionWithDist", "InterpolatorComponentAttractionWithDistSquared"].map(|k| Curve::from(a.get(k))),
        )
    });
    let rate = Curve::from(p.get("ParticleRateInterpolator"));
    let fl = [rgb[0].last_t(), rgb[1].last_t(), rgb[2].last_t(), alpha.last_t(), size.last_t()].into_iter().fold(0.0, f32::max);
    // PROVISIONAL: native no-expiration lifetime is unresolved.
    // shortcut: bound continuous, non-fading particles by authored duration; replace after recovering native expiry.
    // A start-only looping glow is a fixed pool, not a new emission every animation cycle.
    let start_only_loop = !rate.0.iter().any(|(_, r)| *r > 0.0) && [&rgb[0], &rgb[1], &rgb[2], &alpha, &size].iter().any(|c| c.1.is_some());
    let fallback_life = if start_only_loop {
        f32::INFINITY
    } else if fl > 0.0 {
        fl.max(0.05)
    } else if rate.0.iter().any(|(_, r)| *r > 0.0) {
        num("Duration").max(0.05)
    } else {
        f32::INFINITY
    };
    let src = p
        .get("CHUNKDATA_SOURCE_EMITTER")
        .filter(|s| s["CHUNKDATA_PARTICLE_SYSTEM_SOURCE_EMITTER_ENABLED"].as_bool() == Some(true))
        .and_then(|s| s["CHUNKDATA_PARTICLE_SYSTEM_SOURCE_EMITTER_INDEX"].as_u64())
        .map(|i| i as usize);
    let mesh = get(p, &["CommonPropertiesChunk", "MeshChunk"]).filter(|m| m["UseMorph"] == true).map(|m| {
        let src = m["SourceMeshName"].as_str().expect("Morph source mesh");
        let dst = m["DestinationMeshName"].as_str().expect("Morph destination mesh");
        let (source, uv, indices) = crate::data::mesh_geometry(&assets_dir(), src).unwrap_or_else(|| panic!("Missing/invalid effect mesh {src}; regenerate effects"));
        let (destination, _, other_indices) = crate::data::mesh_geometry(&assets_dir(), dst).unwrap_or_else(|| panic!("Missing/invalid effect mesh {dst}; regenerate effects"));
        assert!(source.len() == destination.len() && indices == other_indices, "Morph topology mismatch: {src} / {dst}");
        MorphMesh { source, destination, uv, indices, model: crate::data::model(&format!("{src}.glb")), weight: Curve::from(m.get("MeshWeightInterpolator")) }
    });
    Prim {
        mesh,
        duration: num("Duration"),
        delay: num("StartDelay"),
        ever: flag("Everlasting"),
        world: flag("UseWorldCoordInternaly"),
        rate,
        start: p["NumberParticlesAtStart"].as_u64().unwrap_or(0) as u32,
        pos_t: c3(pe.and_then(|e| e.get("EmitterTargetPositionInterpolator3"))),
        pos_r: Curve::from(pe.and_then(|e| e.get("RadiusInterpolator"))),
        pos_l: Curve::from(pe.and_then(|e| e.get("LengthInterpolator"))),
        pos_le: Curve::from(pe.and_then(|e| e.get("LengthErrorInterpolator"))),
        vel_t: c3(ve.and_then(|e| e.get("EmitterTargetPositionInterpolator3"))),
        vel_r: Curve::from(ve.and_then(|e| e.get("RadiusInterpolator"))),
        vel_l: Curve::from(ve.and_then(|e| e.get("LengthInterpolator"))),
        vel_le: Curve::from(ve.and_then(|e| e.get("LengthErrorInterpolator"))),
        target_on: flag("CHUNKDATA_ENABLE_TARGET_EMITTER"),
        target_pos_velocity: flag("CHUNKDATA_TARGET_EMITTER_USE_POSITION_FOR_VELOCITY"),
        angle_on: flag("CHUNKDATA_ENABLE_ANGLE_EMITTER"),
        angle_pos_velocity: flag("CHUNKDATA_ANGLE_EMITTER_USE_POSITION_FOR_VELOCITY"),
        vel_ang_dir: c3(va.and_then(|e| e.get("CHUNK_ANGLEEMITTER_TARGET_POSITION_INTERPOLATOR3"))),
        vel_ang_min: Curve::from(va.and_then(|e| e.get("CHUNK_ANGLEEMITTER_MIN_ANGLE"))),
        vel_ang_max: Curve::from(va.and_then(|e| e.get("CHUNK_ANGLEEMITTER_MAX_ANGLE"))),
        vel_ang_len: Curve::from(va.and_then(|e| e.get("CHUNK_ANGLEEMITTER_LENGTHINTERPOLATOR"))),
        vel_ang_len_err: Curve::from(va.and_then(|e| e.get("CHUNK_ANGLEEMITTER_LENGTHERRORINTERPOLATOR"))),
        ang_dir: c3(pa.and_then(|e| e.get("CHUNK_ANGLEEMITTER_TARGET_POSITION_INTERPOLATOR3"))),
        ang_min: Curve::from(pa.and_then(|e| e.get("CHUNK_ANGLEEMITTER_MIN_ANGLE"))),
        ang_max: Curve::from(pa.and_then(|e| e.get("CHUNK_ANGLEEMITTER_MAX_ANGLE"))),
        ang_len: Curve::from(pa.and_then(|e| e.get("CHUNK_ANGLEEMITTER_LENGTHINTERPOLATOR"))),
        ang_len_err: Curve::from(pa.and_then(|e| e.get("CHUNK_ANGLEEMITTER_LENGTHERRORINTERPOLATOR"))),
        tex,
        blend,
        size,
        rgb,
        alpha,
        rect,
        flat: cp.get("Flat").is_some(),
        along_velocity: get(cp, &["PseudoBillboardChunk", "PseudoDirectionUseVelocity"]).and_then(|v| v.as_bool()).unwrap_or(false),
        pseudo_dir: get(cp, &["PseudoBillboardChunk", "DirectionInterpolator3Chunk"]).map(|d| {
            let c = |k: &str| d.get(k).and_then(|c| c["curve"].get(0)).and_then(|p| p[1].as_f64()).unwrap_or(0.0) as f32;
            conv(Vec3::new(c("Value1Interpolator"), c("Value2Interpolator"), c("Value3Interpolator")))
        }),
        life,
        attract,
        fallback_life,
        low_rate: p["LowDetailParticleRateMultiply"].as_f64().unwrap_or(1.0) as f32,
        src,
    }
}

impl FromWorld for FxLib {
    fn from_world(_: &mut World) -> Self {
        let dir = assets_dir().join("fx");
        let names: Vec<String> = std::fs::read_to_string(dir.join("index.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let (mut effects, mut list, mut tex_names) = (HashMap::new(), vec![], vec![]);
        for n in names {
            let j: Value = serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{n}.json"))).unwrap()).unwrap();
            let prims = match &j["Primitive"] {
                Value::Array(a) => a.iter().map(|p| parse_prim(p, &mut tex_names)).collect(),
                _ => vec![],
            };
            effects.insert(n, list.len());
            list.push(Effect { prims });
        }
        Self { effects, list, tex_names }
    }
}

pub fn plugin(app: &mut App) {
    app.add_plugins(MaterialPlugin::<EffectMaterial>::default())
        .init_resource::<FxLib>()
        .insert_resource(FxRng(0xD1B54A32D192ED03))
        .init_resource::<FxTextures>()
        .init_resource::<Groups>()
        .add_systems(Startup, load_textures)
        .add_systems(PostUpdate, (update_fx, render_mesh_fx, render_fx).chain().after(bevy::transform::TransformSystems::Propagate));
}

fn load_textures(lib: Res<FxLib>, assets: Res<AssetServer>, mut tex: ResMut<FxTextures>) {
    tex.0 = lib.tex_names.iter().map(|n| assets.load(format!("fx/tex/{n}.png"))).collect();
}

/// Starts effect `name` at world transform `world` (so the first frame is right), optionally as a child of `parent`
/// (then `local` is relative to it). Unknown names are ignored. Returns the effect entity.
pub fn spawn(commands: &mut Commands, lib: &FxLib, name: &str, parent: Option<Entity>, local: Transform, world: Transform, transient: bool) -> Option<Entity> {
    let &eff = lib.effects.get(name)?;
    let fx = Fx {
        eff,
        age: 0.0,
        tick_acc: 0.0,
        move_acc: 0.0,
        previous_world: world,
        prims: lib.list[eff].prims.iter().map(|_| PrimState { started: false, parts: vec![] }).collect(),
        rate_scale: 1.0,
        size_scale: 1.0,
        emitting: true,
        src_half: Vec3::ZERO,
        meshes: lib.list[eff].prims.iter().map(|_| None).collect(),
    };
    let mut e = commands.spawn((fx, local, GlobalTransform::from(world), Visibility::default()));
    if let Some(p) = parent {
        e.insert(ChildOf(p));
    }
    if transient {
        e.insert(Transient);
    }
    Some(e.id())
}

/// Engine tick (retail constant 0.033): particles spawn and advance once per tick; velocities are stored per second.
const TICK: f32 = 1.0 / 30.0;
/// Movement infos (attractor etc.) only run once at least this much time has passed (retail constant 0.1).
const MOVE_STEP: f32 = 0.1;

/// Retail: effect detail D falls linearly from 1 at the camera to 0 at EffectsLowestDetailDistance (350, retail
/// default profile; max detail 1.0); spawn count = rate x (D - D*L + L), L = the primitive's LowDetailParticleRateMultiply.
/// shortcut: the original's extra off-screen test (D = 0) is not modelled; no camera = full detail.
fn detail_factor(low: f32, dist: f32) -> f32 {
    let d = (1.0 - dist / LOWEST_DETAIL_DISTANCE).clamp(0.0, 1.0);
    d - d * low + low
}
const LOWEST_DETAIL_DISTANCE: f32 = 350.0;

fn update_fx(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<FxLib>,
    mut rng: ResMut<FxRng>,
    mut q: Query<(Entity, &mut Fx, &GlobalTransform)>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
) {
    let frame = time.delta_secs().min(0.25);
    let cam_pos = cam.single().ok().map(|c| c.translation());
    for (e, mut fx, gt) in &mut q {
        fx.tick_acc += frame;
        let world = gt.compute_transform();
        let previous_world = fx.previous_world;
        fx.previous_world = world;
        let eff = &lib.list[fx.eff];
        let meteor = lib.effects.get("MeteorStorm") == Some(&fx.eff);
        let mut busy = true; // no tick this frame: nothing to decide
        while fx.tick_acc >= TICK {
            fx.tick_acc -= TICK;
            // Catch-up ticks emit along the travelled path, rather than in a clump at the frame's endpoint.
            let tick_world = GlobalTransform::from(emitter_transform(previous_world, world, fx.tick_acc, frame));
            fx.age += TICK;
            fx.move_acc += TICK;
            let move_dt = if fx.move_acc > MOVE_STEP { std::mem::take(&mut fx.move_acc) } else { 0.0 };
            let (age, scale, emitting) = (fx.age, fx.rate_scale * crate::menu::detail_scale(), fx.emitting);
            busy = false;
            // A source emitter places particles at the positions of its source primitive's particles.
            let sources: HashMap<usize, Vec<Vec3>> =
                eff.prims.iter().filter_map(|p| p.src).map(|i| (i, fx.prims[i.min(fx.prims.len() - 1)].parts.iter().map(|p| p.pos).collect())).collect();
            let half = fx.src_half;
            for (pi, (prim, st)) in eff.prims.iter().zip(fx.prims.iter_mut()).enumerate() {
                let is_source = eff.prims.iter().any(|q| q.src == Some(pi));
                let t = age - prim.delay;
                for p in &mut st.parts {
                    p.age += TICK;
                    p.pos += p.vel * TICK;
                    if move_dt > 0.0
                        && let Some((tg, k)) = &prim.attract
                    {
                        let tt = t.max(0.0);
                        let target = conv(Vec3::new(tg[0].at(tt, 0.0), tg[1].at(tt, 0.0), tg[2].at(tt, 0.0)));
                        let to = target - p.pos;
                        let d = to.length();
                        let (k0, k1, k2) = (k[0].at(tt, 0.0), k[1].at(tt, 0.0), k[2].at(tt, 0.0));
                        let strength = if d > 0.0 { k0 + (k1 + k2 / d) / d } else { k0 };
                        p.vel += to * strength * move_dt;
                    }
                }
                st.parts.retain(|p| p.age < p.life);
                if !prim.ever && t > prim.duration {
                    st.parts.clear();
                }
                if t >= 0.0 && emitting {
                    let live = prim.ever || t <= prim.duration;
                    let mut n = 0u32;
                    if !st.started {
                        st.started = true;
                        n += prim.start;
                    }
                    if live {
                        // Retail: rate is particles per tick; a fractional rate spawns one with that probability.
                        let r = prim.rate.at(t, 0.0) * scale * detail_factor(prim.low_rate, cam_pos.map_or(0.0, |c| c.distance(gt.translation())));
                        n += if r >= 1.0 {
                            r as u32
                        } else if r > 0.0 && rng.f() < r {
                            1
                        } else {
                            0
                        };
                    }
                    for _ in 0..n {
                        let mut np = spawn_particle(prim, t, &tick_world, &mut rng);
                        if meteor {
                            align_meteor_particle(&mut np, tick_world.translation().y);
                        }
                        // Data flag ASSIGN_RANDOM_POSITIONS_FROM_MESH_VERTEXES; shortcut: random point in the section's box, not its real vertices.
                        if is_source && half != Vec3::ZERO {
                            np.pos = half * Vec3::new(rng.f() * 2.0 - 1.0, rng.f() * 2.0 - 1.0, rng.f() * 2.0 - 1.0);
                        }
                        if let Some(si) = prim.src {
                            let pool = &sources[&si];
                            if !pool.is_empty() {
                                np.pos += pool[(rng.f() * pool.len() as f32) as usize % pool.len()];
                            }
                        }
                        st.parts.push(np);
                    }
                    busy |= live;
                }
                busy |= !st.parts.is_empty() || t < 0.0;
            }
        }
        if !busy && !eff.prims.iter().any(|p| p.ever) {
            commands.entity(e).despawn();
        }
    }
}

// PROVISIONAL: match each meteor's authored fall speed/lifetime to the storm's flight-plane splash surface.
fn align_meteor_particle(p: &mut P, surface: f32) {
    p.pos.y = surface - p.vel.y.min(0.0) * p.life;
}

fn emitter_transform(previous: Transform, current: Transform, remainder: f32, frame: f32) -> Transform {
    if frame <= 0.0 {
        return current;
    }
    let t = (1.0 - remainder / frame).clamp(0.0, 1.0);
    Transform {
        translation: previous.translation.lerp(current.translation, t),
        rotation: previous.rotation.slerp(current.rotation, t),
        scale: previous.scale.lerp(current.scale, t),
    }
}

fn particle_sample(p: &P, remainder: f32) -> (Vec3, f32) {
    (p.pos + p.vel * remainder, p.age + remainder)
}

fn streak_axes(direction: Vec3, right: Vec3, up: Vec3, view: Vec3) -> (Vec3, Vec3) {
    let Some(axis) = direction.try_normalize() else { return (right, up) };
    // Keep length along the world-space axis: flattening it into the camera plane lifts horizontal flames.
    (view.cross(axis).try_normalize().unwrap_or(right), axis)
}

fn particle_axes(prim: &Prim, gt: &GlobalTransform, vel: Vec3, camera_axes: [Vec3; 3]) -> (Vec3, Vec3) {
    let [right, up, view] = camera_axes;
    if prim.flat {
        // Retail Flat quads lie in the effect's game XY plane, not the camera plane.
        (gt.right().as_vec3(), gt.forward().as_vec3())
    } else if prim.along_velocity && vel.length_squared() > 1e-6 {
        streak_axes(vel, right, up, view)
    } else if let Some(d) = prim.pseudo_dir.map(|d| gt.affine().transform_vector3(d)) {
        streak_axes(d, right, up, view)
    } else {
        (right, up)
    }
}

fn spawn_particle(p: &Prim, t: f32, gt: &GlobalTransform, rng: &mut FxRng) -> P {
    let at = |c: &[Curve; 3]| Vec3::new(c[0].at(t, 0.0), c[1].at(t, 0.0), c[2].at(t, 0.0));
    // Retail: the error is one-sided, uniform between 0 and err.
    let jitter = |len: f32, err: f32, rng: &mut FxRng| len + err * rng.f();
    // Retail: enabled target and angle emitters add independent position/velocity contributions.
    let emit = |tg: Vec3, r: f32, l: f32, le: f32, rng: &mut FxRng| (tg - rng.sphere().normalize_or_zero() * r).normalize_or_zero() * jitter(l, le, rng);
    let angle = |dir: &[Curve; 3], min: &Curve, max: &Curve, len: &Curve, err: &Curve, rng: &mut FxRng| {
        let dir = at(dir).normalize_or_zero();
        let dir = if dir == Vec3::ZERO { Vec3::Y } else { dir };
        let (lo, hi) = (min.at(t, 0.0).to_radians(), max.at(t, 0.0).to_radians());
        let a = lo + (hi - lo) * rng.f();
        let az = rng.f() * std::f32::consts::TAU;
        let o1 = dir.any_orthonormal_vector();
        let o2 = dir.cross(o1);
        let d = dir * a.cos() + (o1 * az.cos() + o2 * az.sin()) * a.sin();
        d * jitter(len.at(t, 0.0), err.at(t, 0.0), rng)
    };
    let (mut pos, mut vel) = (Vec3::ZERO, Vec3::ZERO);
    if p.target_on {
        let offset = emit(at(&p.pos_t), p.pos_r.at(t, 0.0), p.pos_l.at(t, 0.0), p.pos_le.at(t, 0.0), rng);
        pos += offset;
        vel += if p.target_pos_velocity { offset } else { emit(at(&p.vel_t), p.vel_r.at(t, 0.0), p.vel_l.at(t, 0.0), p.vel_le.at(t, 0.0), rng) };
    }
    if p.angle_on {
        let offset = angle(&p.ang_dir, &p.ang_min, &p.ang_max, &p.ang_len, &p.ang_len_err, rng);
        pos += offset;
        vel += if p.angle_pos_velocity { offset } else { angle(&p.vel_ang_dir, &p.vel_ang_min, &p.vel_ang_max, &p.vel_ang_len, &p.vel_ang_len_err, rng) };
    }
    let (mut pos, mut vel) = (conv(pos), conv(vel));
    if p.world {
        pos = gt.transform_point(pos);
        vel = gt.affine().transform_vector3(vel);
    }
    let life = p.life.as_ref().map_or(p.fallback_life, |c| c.at(t, p.fallback_life)).max(0.05);
    P { pos, vel, age: 0.0, life }
}

/// Repair scaffolds use the authored source/destination topology and full deploy/hold/retract curve.
fn render_mesh_fx(
    mut commands: Commands,
    lib: Res<FxLib>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut effects: Query<(Entity, &mut Fx, &GlobalTransform)>,
) {
    for (e, mut fx, gt) in &mut effects {
        for (i, prim) in lib.list[fx.eff].prims.iter().enumerate() {
            let Some(def) = &prim.mesh else { continue };
            let age = fx.age + fx.tick_acc - prim.delay;
            if age < 0.0 {
                continue;
            }
            let scale = fx.size_scale;
            let entry = fx.meshes[i].get_or_insert_with(|| {
                let mesh = meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
                let material = materials.add(StandardMaterial {
                    base_color_texture: Some(assets.load(GltfAssetLabel::Texture(0).from_asset(def.model.clone()))),
                    unlit: true,
                    cull_mode: None,
                    double_sided: true,
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                });
                commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default(), *gt, ChildOf(e), NoFrustumCulling));
                (mesh, material)
            });
            if let Some(mut mesh) = meshes.get_mut(&entry.0) {
                let weight = def.weight.at(age, 0.0).clamp(0.0, 1.0);
                let positions: Vec<_> = def.source.iter().zip(&def.destination).map(|(a, b)| (a.lerp(*b, weight) * scale).to_array()).collect();
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
                mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, def.uv.clone());
                mesh.insert_attribute(
                    Mesh::ATTRIBUTE_COLOR,
                    vec![[prim.rgb[0].at(age, 1.0), prim.rgb[1].at(age, 1.0), prim.rgb[2].at(age, 1.0), prim.alpha.at(age, 1.0)]; def.source.len()],
                );
                mesh.insert_indices(Indices::U32(def.indices.clone()));
            }
        }
    }
}

#[derive(Resource, Default)]
struct Groups(HashMap<(usize, Blend), (Entity, Handle<Mesh>)>);

fn render_fx(
    mut commands: Commands,
    lib: Res<FxLib>,
    tex: Res<FxTextures>,
    mut groups: ResMut<Groups>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut additive: ResMut<Assets<EffectMaterial>>,
    cam: Single<&GlobalTransform, With<Camera3d>>,
    fx: Query<(&Fx, &GlobalTransform)>,
    mut vis: Query<&mut Visibility>,
) {
    let (right, up, view) = (cam.right().as_vec3(), cam.up().as_vec3(), cam.forward().as_vec3());
    #[derive(Default)]
    struct Buf {
        pos: Vec<[f32; 3]>,
        uv: Vec<[f32; 2]>,
        col: Vec<[f32; 4]>,
        idx: Vec<u32>,
    }
    let mut bufs: HashMap<(usize, Blend), Buf> = HashMap::new();
    for (f, gt) in &fx {
        let eff = &lib.list[f.eff];
        for (prim, st) in eff.prims.iter().zip(&f.prims) {
            let Some(ti) = prim.tex else { continue };
            for p in &st.parts {
                // Keep retail 30 Hz simulation, but draw motion and fades at the display frame rate.
                let (pos, age) = particle_sample(p, f.tick_acc);
                if age >= p.life {
                    continue;
                }
                let (pos, vel) = if prim.world { (pos, p.vel) } else { (gt.transform_point(pos), gt.affine().transform_vector3(p.vel)) };
                let size = prim.size.at(age, 1.0) * f.size_scale;
                let col = [prim.rgb[0].at(age, 1.0), prim.rgb[1].at(age, 1.0), prim.rgb[2].at(age, 1.0), prim.alpha.at(age, 1.0)];
                let (x0, y0, x1, y1) = match &prim.rect {
                    Some(r) => (r[0].at(age, -0.5), r[1].at(age, 0.5), r[2].at(age, 0.5), r[3].at(age, -0.5)),
                    None => (-0.5, 0.5, 0.5, -0.5),
                };
                let (ax, ay) = particle_axes(prim, gt, vel, [right, up, view]);
                let b = bufs.entry((ti, prim.blend)).or_default();
                let i = b.pos.len() as u32;
                for (cx, cy, u, v) in [(x0, y0, 0.0, 0.0), (x1, y0, 1.0, 0.0), (x1, y1, 1.0, 1.0), (x0, y1, 0.0, 1.0)] {
                    b.pos.push((pos + ax * cx * size + ay * cy * size).to_array());
                    b.uv.push([u, v]);
                    b.col.push(col);
                }
                b.idx.extend([i, i + 1, i + 2, i, i + 2, i + 3]);
            }
        }
    }
    let used: std::collections::HashSet<(usize, Blend)> = bufs.keys().copied().collect();
    for (key, b) in bufs {
        let entry = groups.0.entry(key).or_insert_with(|| {
            let mesh = meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
            let mut entity = commands.spawn((Mesh3d(mesh.clone()), Transform::default(), NoFrustumCulling, Visibility::default()));
            // Every ONE/ONE texture, flares included: several (flare, cannon-ball pseudo) have an all-zero alpha channel, which
            // Bevy's premultiplying AlphaMode::Add turned black, hiding dock lights, repair sparkles and the SOS beacon (PT-11).
            if key.1 == Blend::Add {
                entity.insert(MeshMaterial3d(additive.add(additive_material(tex.0[key.0].clone()))));
            } else {
                entity.insert(MeshMaterial3d(mats.add(StandardMaterial {
                    base_color_texture: Some(tex.0[key.0].clone()),
                    unlit: true,
                    cull_mode: None,
                    double_sided: true,
                    alpha_mode: if key.1 == Blend::Multiply { AlphaMode::Multiply } else { AlphaMode::Blend },
                    ..default()
                })));
            }
            let e = entity.id();
            (e, mesh)
        });
        if let Some(mut m) = meshes.get_mut(&entry.1) {
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, b.pos);
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, b.uv);
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, b.col);
            m.insert_indices(Indices::U32(b.idx));
        }
        if let Ok(mut v) = vis.get_mut(entry.0) {
            *v = Visibility::Inherited;
        }
    }
    for (key, (e, _)) in &groups.0 {
        if !used.contains(key)
            && let Ok(mut v) = vis.get_mut(*e)
        {
            *v = Visibility::Hidden;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meteor_shower_spreads_above_the_emitter_and_falls_vertically() {
        let doc: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("fx/MeteorStorm.json")).unwrap()).unwrap();
        let gt = GlobalTransform::from(Transform::from_xyz(30.0, 20.0, -40.0));
        let mut rng = FxRng(12345);
        for (i, data) in doc["Primitive"].as_array().unwrap().iter().enumerate() {
            let prim = parse_prim(data, &mut vec![]);
            for _ in 0..64 {
                let particle = spawn_particle(&prim, 0.0, &gt, &mut rng);
                if i < 2 {
                    assert!(particle.pos.y > gt.translation().y + 99.0);
                    assert!(particle.vel.y <= -50.0);
                    assert!(Vec2::new(particle.vel.x, particle.vel.z).length() < 1e-5);
                    assert!(particle_sample(&particle, TICK).0.y < particle.pos.y);
                    assert!(prim.along_velocity);
                } else {
                    assert_eq!(particle.vel, Vec3::ZERO, "splash motion must not inherit the position cone");
                }
            }
        }
    }

    #[test]
    fn meteor_streaks_end_at_the_splash_plane_at_their_authored_lifetime() {
        let doc: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("fx/MeteorStorm.json")).unwrap()).unwrap();
        let mut rng = FxRng(13);
        for surface in [0.0, 25.0] {
            let gt = GlobalTransform::from(Transform::from_xyz(10.0, surface, 20.0));
            for (i, data) in doc["Primitive"].as_array().unwrap().iter().enumerate() {
                let prim = parse_prim(data, &mut vec![]);
                for _ in 0..100 {
                    let mut p = spawn_particle(&prim, 0.0, &gt, &mut rng);
                    let horizontal = p.pos.with_y(0.0);
                    align_meteor_particle(&mut p, surface);
                    assert_eq!(p.pos.with_y(0.0), horizontal);
                    assert!((p.pos.y + p.vel.y.min(0.0) * p.life - surface).abs() < 0.001);
                    if i < 2 {
                        assert!(p.pos.y > surface);
                        assert!(p.pos.y + p.vel.y * (p.life - 0.01) > surface);
                    } else {
                        assert_eq!(p.pos.y, surface, "splashes begin at flight height");
                    }
                }
            }
        }
    }

    #[test]
    fn meteor_splash_lies_flat_independently_of_camera_orientation() {
        let doc: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("fx/MeteorStorm.json")).unwrap()).unwrap();
        let prim = parse_prim(&doc["Primitive"][2], &mut vec![]);
        assert!(prim.flat);
        let gt = GlobalTransform::from(Transform::from_rotation(Quat::from_rotation_y(0.7)));
        for pitch in [0.0, 0.4, 1.2] {
            let camera = Quat::from_rotation_x(pitch);
            let (ax, ay) = particle_axes(&prim, &gt, Vec3::ZERO, [camera * Vec3::X, camera * Vec3::Y, camera * Vec3::NEG_Z]);
            assert!(ax.y.abs() < 1e-6 && ay.y.abs() < 1e-6);
            assert!(ax.cross(ay).distance(Vec3::Y) < 1e-6);
        }
        let billboard = parse_prim(&doc["Primitive"][3], &mut vec![]);
        assert!(!billboard.flat, "splash sparks retain their authored billboard mode");
        assert_eq!(particle_axes(&billboard, &gt, Vec3::ZERO, [Vec3::X, Vec3::Y, Vec3::NEG_Z]), (Vec3::X, Vec3::Y));
    }

    #[test]
    fn angle_velocity_uses_its_own_emitter_or_the_position_when_requested() {
        let doc: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("fx/CannonBall_Light_Muzzle.json")).unwrap()).unwrap();
        let mut prim = parse_prim(&doc["Primitive"][0], &mut vec![]);
        let mut rng = FxRng(12345);
        let particle = spawn_particle(&prim, 0.0, &GlobalTransform::IDENTITY, &mut rng);
        assert!(particle.pos.length() > 0.0);
        assert_eq!(particle.vel, particle.pos);
        prim.angle_pos_velocity = false;
        prim.vel_ang_dir = [Curve::default(), Curve::default(), Curve(vec![(0.0, -1.0)], None)];
        prim.vel_ang_len = Curve(vec![(0.0, 12.0)], None);
        prim.vel_ang_len_err = Curve::default();
        prim.vel_ang_min = Curve::default();
        prim.vel_ang_max = Curve::default();
        let particle = spawn_particle(&prim, 0.0, &GlobalTransform::IDENTITY, &mut rng);
        assert_eq!(particle.vel, Vec3::NEG_Y * 12.0);
    }

    #[test]
    fn repair_meshes_have_matching_topology_and_deploy_retract_curves() {
        for name in ["Repair_Small", "Repair_Medium", "Repair_Large", "Repair_IC_Medium", "Repair_IC_Large"] {
            let doc: Value = serde_json::from_slice(&std::fs::read(assets_dir().join(format!("fx/{name}.json"))).unwrap()).unwrap();
            let prim = doc["Primitive"].as_array().unwrap().iter().map(|p| parse_prim(p, &mut vec![])).find(|p| p.mesh.is_some()).unwrap();
            let mesh = prim.mesh.unwrap();
            assert_eq!(mesh.source.len(), mesh.destination.len());
            assert!(mesh.source.iter().zip(&mesh.destination).any(|(a, b)| a.distance(*b) > 1.0));
            assert!(mesh.weight.at(prim.duration * 0.4, 0.0) >= 0.5, "{name} must deploy");
            assert!(mesh.weight.at(prim.duration, 0.0) < 0.01, "{name} must retract");
        }
        let material = additive_material(Handle::default());
        assert_eq!(material.base.alpha_mode, AlphaMode::Blend, "shader must leave source RGB unpremultiplied for ONE/ONE");
        assert!(material.base.depth_bias >= ADDITIVE_SORT_BIAS, "additive effects sort after alpha-blended sails (PT-05)");
    }

    #[test]
    fn malformed_mesh_buffers_fail_without_panicking() {
        let root = std::env::temp_dir().join(format!("procyon-mesh-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("badmesh.glb");
        std::fs::write(&path, []).unwrap();
        assert!(crate::data::mesh_geometry(&root, "badmesh").is_none());
        let json = serde_json::json!({"meshes":[{"primitives":[{"attributes":{"POSITION":0,"TEXCOORD_0":0},"indices":0}]}],"accessors":[{"bufferView":0,"count":1024,"componentType":5126}],"bufferViews":[{"byteOffset":200,"byteLength":12288}]});
        let encoded = serde_json::to_vec(&json).unwrap();
        let mut bytes = vec![0u8; 20];
        bytes[0..4].copy_from_slice(b"glTF");
        bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
        bytes[16..20].copy_from_slice(b"JSON");
        bytes[12..16].copy_from_slice(&(encoded.len() as u32).to_le_bytes());
        bytes.extend(encoded);
        bytes.extend([0u8; 4]);
        bytes.extend(b"BIN\0");
        std::fs::write(&path, bytes).unwrap();
        assert!(crate::data::mesh_geometry(&root, "badmesh").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn horizontal_streaks_stay_level_at_oblique_camera_angles() {
        for pitch in [0.0_f32, 0.2, 0.8, 1.5] {
            let rotation = Quat::from_rotation_x(-pitch);
            let view = rotation * Vec3::NEG_Z;
            let up = rotation * Vec3::Y;
            let (width, length) = streak_axes(Vec3::NEG_Z, Vec3::X, up, view);
            assert_eq!(length, Vec3::NEG_Z, "flame length follows the projectile, not camera-up");
            assert!(width.dot(length).abs() < 1e-6);
            assert!(width.dot(view).abs() < 1e-6);
            assert!((width.length() - 1.0).abs() < 1e-6);
        }
        assert_eq!(streak_axes(Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::NEG_Z), (Vec3::X, Vec3::Y));
    }

    #[test]
    fn particle_motion_and_fade_are_sampled_between_ticks() {
        let p = P { pos: Vec3::X, vel: Vec3::X * 30.0, age: 0.2, life: 1.0 };
        let (pos, age) = particle_sample(&p, TICK / 2.0);
        assert!((pos.x - 1.5).abs() < 1e-6);
        assert!((age - (0.2 + TICK / 2.0)).abs() < 1e-6);
        assert_eq!(p.pos, Vec3::X, "render sampling does not alter simulation");
    }

    #[test]
    fn catch_up_emission_is_spread_along_the_frames_motion() {
        let previous = Transform::default();
        let current = Transform::from_xyz(3.0, 0.0, 0.0).with_rotation(Quat::from_rotation_y(0.3));
        for (remainder, x) in [(0.2, 1.0), (0.1, 2.0), (0.0, 3.0)] {
            let at = emitter_transform(previous, current, remainder, 0.3);
            assert!((at.translation.x - x).abs() < 1e-5);
            assert!((at.rotation.angle_between(previous.rotation) - x * 0.1).abs() < 1e-4);
        }
        assert_eq!(emitter_transform(previous, current, 0.0, 0.0), current);
    }

    #[test]
    fn lifetime_fallback_preserves_start_only_source_pools_and_authored_expiration() {
        let lib = FxLib::from_world(&mut World::new());
        let marker = &lib.list[lib.effects["Docking_Point"]].prims[8];
        assert_eq!(marker.fallback_life, marker.duration);
        assert!(marker.life.is_none());
        let source = &lib.list[lib.effects["Damaged Section_Heavy"]].prims[0];
        assert!(source.fallback_life.is_infinite(), "start-only source particles remain available");
        let engine = &lib.list[lib.effects["NavyEngine_Light"]].prims[0];
        assert_eq!(engine.life.as_ref().unwrap().at(0.0, 0.0), 1.0, "explicit expiration is unchanged");
    }

    #[test]
    fn curve_repeats_only_the_authored_interval_and_resumes_after_finite_loops() {
        let c = Curve::from(Some(&serde_json::json!({"curve": [[0, 0], [1, 10], [3, 30], [4, 40]],
            "loop": {"StartLoopingT": 1, "EndLoopingT": 3, "NumberLoops": 2}})));
        assert_eq!(c.at(0.5, 0.0), 5.0);
        assert_eq!(c.at(2.0, 0.0), 20.0);
        assert_eq!(c.at(3.0, 0.0), 10.0);
        assert_eq!(c.at(6.0, 0.0), 20.0);
        assert_eq!(c.at(7.5, 0.0), 35.0);
        let invalid = Curve::from(Some(&serde_json::json!({"curve": [[0, 1]], "loop": {"StartLoopingT": 1, "EndLoopingT": 1, "NumberLoops": 2}})));
        assert!(invalid.1.is_none());
    }

    #[test]
    fn sos_beacon_keeps_its_fixed_particles_and_pulses_until_released() {
        let mut app = App::new();
        app.init_resource::<Time>().init_resource::<FxLib>().insert_resource(FxRng(123)).add_systems(Update, update_fx);
        let beacon = app.world_mut().resource_scope(|world, lib: Mut<FxLib>| {
            for p in &lib.list[lib.effects["BottleRocket_Light_Impact"]].prims {
                assert!(p.fallback_life.is_infinite());
                assert!(assets_dir().join(format!("fx/tex/{}.png", lib.tex_names[p.tex.unwrap()])).is_file());
                assert!((p.size.at(0.6, 0.0) - p.size.at(50.6, 0.0)).abs() < 0.001);
            }
            spawn(&mut world.commands(), &lib, "BottleRocket_Light_Impact", None, Transform::default(), Transform::default(), false).unwrap()
        });
        app.world_mut().flush();
        for _ in 0..1800 {
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(TICK));
            app.update();
            assert_eq!(app.world().get::<Fx>(beacon).unwrap().particle_count(), 9, "SOS beacon must remain visible without accumulating particles");
        }
        app.world_mut().despawn(beacon);
        app.update();
        assert!(app.world().get::<Fx>(beacon).is_none());
    }

    #[test]
    fn docking_marker_particles_are_bounded_over_a_long_mission() {
        let mut app = App::new();
        app.init_resource::<Time>().init_resource::<FxLib>().insert_resource(FxRng(123)).add_systems(Update, update_fx);
        let marker = app
            .world_mut()
            .resource_scope(|world, lib: Mut<FxLib>| spawn(&mut world.commands(), &lib, "Docking_Point", None, Transform::default(), Transform::default(), false).unwrap());
        app.world_mut().flush();
        for i in 0..9000 {
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(TICK));
            app.update();
            if i >= 899 && i % 900 == 899 {
                let fx = app.world().get::<Fx>(marker).unwrap();
                assert!(fx.particle_count() <= 4000, "docking marker grew to {} particles after {} ticks", fx.particle_count(), i + 1);
            }
        }
    }

    #[test]
    fn dock_light_flares_draw_with_the_unpremultiplied_additive_material() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<FxLib>()
            .insert_resource(FxRng(123))
            .init_resource::<Groups>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<EffectMaterial>>()
            .add_systems(Update, (update_fx, render_fx).chain());
        let n = app.world().resource::<FxLib>().tex_names.len();
        app.insert_resource(FxTextures(vec![Handle::default(); n]));
        app.world_mut().spawn((Camera3d::default(), GlobalTransform::default()));
        app.world_mut()
            .resource_scope(|world, lib: Mut<FxLib>| spawn(&mut world.commands(), &lib, "Docking_Point", None, Transform::default(), Transform::default(), false).unwrap());
        app.world_mut().flush();
        for _ in 0..10 {
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(TICK));
            app.update();
        }
        let flare = app.world().resource::<FxLib>().tex_names.iter().position(|t| t == "flare").unwrap();
        let &(group, _) = app.world().resource::<Groups>().0.get(&(flare, Blend::Add)).expect("dock-light flares drawn");
        // The flare's alpha channel is all zero: a premultiplying material would draw it black (PT-11).
        assert!(app.world().get::<MeshMaterial3d<EffectMaterial>>(group).is_some());
        assert!(app.world().get::<MeshMaterial3d<StandardMaterial>>(group).is_none());
    }

    #[test]
    fn mission_effect_particle_counts_do_not_grow_with_elapsed_time() {
        let mut app = App::new();
        app.init_resource::<Time>().init_resource::<FxLib>().insert_resource(FxRng(123)).add_systems(Update, update_fx);
        app.world_mut().resource_scope(|world, lib: Mut<FxLib>| {
            let mut commands = world.commands();
            for name in ["NavyEngine_Light", "Torpedo_Trail", "Damaged Section_Heavy", "Docking_Point"] {
                spawn(&mut commands, &lib, name, None, Transform::default(), Transform::default(), false).unwrap();
            }
        });
        app.world_mut().flush();
        let mut counts = vec![];
        for i in 0..1800 {
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(TICK));
            app.update();
            if i == 899 || i == 1799 {
                let total: usize = app.world_mut().query::<&Fx>().iter(app.world()).flat_map(|f| &f.prims).map(|p| p.parts.len()).sum();
                counts.push(total);
            }
        }
        assert!(counts[0] > 0 && counts[1] <= counts[0] * 11 / 10, "particle counts at 30/60 seconds: {counts:?}");
    }

    #[test]
    fn detail_scales_spawn_rate_by_camera_distance() {
        assert_eq!(detail_factor(0.1, 0.0), 1.0);
        assert!((detail_factor(0.1, 175.0) - 0.55).abs() < 1e-6);
        assert!((detail_factor(0.1, 1000.0) - 0.1).abs() < 1e-6);
    }
}
