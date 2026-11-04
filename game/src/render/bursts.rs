//! Not retail: burst particles of the render profile (part of render/mod.rs's look), drawn on top of the retail effects.
//! When a retail effect starts whose name contains a rule's `match` (`bursts.effects`: muzzle flashes, impacts,
//! explosions), the rule's parts are thrown from the effect's pose: a flash, sparks, embers, a fireball, a shockwave ring,
//! smoke; a rule with `every` keeps throwing them while the effect lives (fires on a burning ship), and `shake` jolts the
//! camera. A hull struck by a shell glows where it was scarred and cools (`bursts.scorch`), and specks of ether dust
//! drift around the camera (`bursts.motes`). The jolt also flares the colour fringing (`camera.aberration_pulse`). Particles move on rails (launched, slowed by drag, smoke rising), so the
//! CPU only places camera-facing quads each frame: every glowing kind in one additive mesh (`shaders/burst.wesl`), the
//! smoke in one blended mesh sorted far to near (`shaders/smoke.wesl`). See `quad` for the vertex contract.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::post_process::effect_stack::ChromaticAberration;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use serde_json::Value;

use super::{Look, num, rgb};
use crate::fx::{Fx, FxLib};

pub const SHADER: &str = "shaders/burst.wesl";
pub const SMOKE_SHADER: &str = "shaders/smoke.wesl";

/// Most particles alive at once; the oldest give way.
const CAP: usize = 12000;

/// What a particle is drawn as; the shader gets it as `uv.x / 4`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Flash = 0,
    Spark = 1,
    Ember = 2,
    Fireball = 3,
    Ring = 4,
    Scorch = 5,
    Mote = 6,
    Smoke = 7,
}

/// One part of a rule: how many particles, their colour (linear), size (world units), life (seconds), launch speed,
/// spread (half-angle of the cone about the effect's forward, degrees; 180 all round), drag (1/s), rise (units/s up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    pub count: u32,
    pub color: Vec3,
    pub size: f32,
    pub seconds: f32,
    pub speed: f32,
    pub spread: f32,
    pub drag: f32,
    pub rise: f32,
}

fn part(v: &Value, d: Part) -> Option<Part> {
    v.is_object().then(|| Part {
        count: num(&v["count"], d.count as f32).max(0.0) as u32,
        color: rgb(&v["color"]).unwrap_or(d.color),
        size: num(&v["size"], d.size),
        seconds: num(&v["seconds"], d.seconds).max(0.01),
        speed: num(&v["speed"], d.speed),
        spread: num(&v["spread"], d.spread).to_radians(),
        drag: num(&v["drag"], d.drag).max(0.0),
        rise: num(&v["rise"], d.rise),
    })
}

/// The parts a rule throws, by kind.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Rule {
    pub matching: String,
    pub parts: Vec<(Kind, Part)>,
    /// Seconds between throws while the effect lives; 0 = once, when it starts.
    pub every: f32,
    /// Camera jolt, fading with distance.
    pub shake: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bursts {
    pub rules: Vec<Rule>,
    /// Glow left where a shell scars a hull.
    pub scorch: Option<Part>,
    /// Ether dust around the camera: `count` specks within `speed` units (the box's half size), drifting.
    pub motes: Option<Part>,
}

const KINDS: [(&str, Kind); 7] = [
    ("flash", Kind::Flash),
    ("sparks", Kind::Spark),
    ("embers", Kind::Ember),
    ("fireball", Kind::Fireball),
    ("ring", Kind::Ring),
    ("smoke", Kind::Smoke),
    ("scorch", Kind::Scorch),
];

fn defaults(kind: Kind) -> Part {
    let p = Part { count: 1, color: Vec3::new(1.0, 0.7, 0.4), size: 1.0, seconds: 0.5, speed: 0.0, spread: 180.0, drag: 0.0, rise: 0.0 };
    match kind {
        Kind::Flash => Part { seconds: 0.12, size: 3.0, ..p },
        Kind::Spark => Part { count: 16, size: 0.08, seconds: 0.5, speed: 25.0, spread: 50.0, drag: 3.0, ..p },
        Kind::Ember => Part { count: 8, size: 0.25, seconds: 1.6, speed: 6.0, drag: 1.5, rise: 0.8, ..p },
        Kind::Fireball => Part { count: 6, size: 3.0, seconds: 0.9, speed: 4.0, drag: 3.0, ..p },
        Kind::Ring => Part { size: 0.5, seconds: 0.7, speed: 30.0, drag: 2.0, ..p },
        Kind::Smoke => Part { count: 6, color: Vec3::splat(0.45), size: 1.5, seconds: 2.5, speed: 3.0, spread: 40.0, drag: 1.5, rise: 0.6 },
        Kind::Scorch => Part { color: Vec3::new(1.0, 0.35, 0.08), size: 1.0, seconds: 6.0, ..p },
        Kind::Mote => Part { count: 300, color: Vec3::new(0.6, 0.7, 1.0), size: 0.12, seconds: 1.0, speed: 120.0, ..p },
    }
}

/// A smoke part with the smoke defaults (tracers' smoke trail).
pub fn smoke_part(v: &Value) -> Option<Part> {
    part(v, defaults(Kind::Smoke))
}

pub fn parse(v: &Value) -> Option<Bursts> {
    v.is_object().then(|| Bursts {
        rules: v["effects"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                Some(Rule {
                    matching: r["match"].as_str()?.to_lowercase(),
                    parts: KINDS.iter().filter_map(|&(key, kind)| Some((kind, part(&r[key], defaults(kind))?))).collect(),
                    every: num(&r["every"], 0.0),
                    shake: num(&r["shake"], 0.0),
                })
            })
            .collect(),
        scorch: part(&v["scorch"], defaults(Kind::Scorch)),
        motes: part(&v["motes"], defaults(Kind::Mote)),
    })
}

impl Bursts {
    pub fn rule(&self, effect: &str) -> Option<usize> {
        let n = effect.to_lowercase();
        self.rules.iter().position(|r| n.contains(r.matching.as_str()))
    }
}

/// A particle on rails: launched from `from` at `vel`, slowed by `drag`, drifting up at `rise`; on a ship (`host`) its
/// launch point rides along with the hull.
#[derive(Clone, Copy, Debug)]
pub struct Particle {
    pub kind: Kind,
    pub born: f32,
    pub life: f32,
    pub from: Vec3,
    pub vel: Vec3,
    pub drag: f32,
    pub rise: f32,
    pub size: f32,
    pub color: Vec3,
    pub seed: u32,
    pub host: Option<Entity>,
}

impl Particle {
    /// Offset from the launch point after `t` seconds, and the velocity then.
    pub fn motion(&self, t: f32) -> (Vec3, Vec3) {
        let (travel, slow) = if self.drag > 1e-4 { ((1.0 - (-self.drag * t).exp()) / self.drag, (-self.drag * t).exp()) } else { (t, 1.0) };
        (self.vel * travel + Vec3::Y * self.rise * t, self.vel * slow + Vec3::Y * self.rise)
    }
}

/// Every live particle; tracers add their smoke here too.
#[derive(Resource, Default)]
pub struct Particles {
    pub list: Vec<Particle>,
    seed: u32,
    /// Camera jolt left, decaying.
    pub shake: f32,
}

impl Particles {
    fn next(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }

    /// A direction within `spread` of `forward`.
    fn direction(&mut self, forward: Vec3, spread: f32) -> Vec3 {
        let cos = 1.0 - (1.0 - spread.min(std::f32::consts::PI).cos()) * self.next();
        let sin = (1.0 - cos * cos).max(0.0).sqrt();
        let a = self.next() * std::f32::consts::TAU;
        let (x, y) = forward.any_orthonormal_pair();
        forward * cos + (x * a.cos() + y * a.sin()) * sin
    }

    /// Throws one part at `at` (world, or ship-local when `host`), facing `forward`.
    pub fn throw(&mut self, kind: Kind, p: &Part, at: Vec3, forward: Vec3, now: f32, host: Option<Entity>) {
        for _ in 0..p.count {
            let dir = self.direction(forward.normalize_or(Vec3::NEG_Z), p.spread);
            let jitter = 0.6 + 0.8 * self.next();
            let seed = (self.next() * 63.0) as u32;
            let (life, size) = (p.seconds * (0.7 + 0.6 * self.next()), p.size * (0.75 + 0.5 * self.next()));
            self.list.push(Particle { kind, born: now, life, from: at, vel: dir * p.speed * jitter, drag: p.drag, rise: p.rise, size, color: p.color, seed, host });
        }
        if self.list.len() > CAP {
            let extra = self.list.len() - CAP;
            self.list.drain(..extra);
        }
    }
}

#[derive(Clone, Copy, Default, Debug, ShaderType)]
pub struct BurstUniform {
    /// x: clock
    pub params: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct BurstGlow {
    #[uniform(100)]
    pub u: BurstUniform,
}

impl MaterialExtension for BurstGlow {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        super::trails::additive(descriptor);
        Ok(())
    }
}

pub type BurstMaterial = ExtendedMaterial<StandardMaterial, BurstGlow>;

#[derive(Clone, Copy, Default, Debug, ShaderType)]
pub struct SmokeUniform {
    /// xyz: direction the sunlight travels (world), w: clock
    pub sun: Vec4,
    /// rgb: sunlight colour on the smoke
    pub light: Vec4,
    /// rgb: shadowed side's colour (the sky's)
    pub shade: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct SmokeShade {
    #[uniform(100)]
    pub u: SmokeUniform,
}

impl MaterialExtension for SmokeShade {
    fn fragment_shader() -> ShaderRef {
        SMOKE_SHADER.into()
    }
}

pub type SmokeMaterial = ExtendedMaterial<StandardMaterial, SmokeShade>;

pub fn plugin(app: &mut App, look: &Look) {
    if look.bursts.is_some() {
        app.init_resource::<Particles>()
            .add_plugins((MaterialPlugin::<BurstMaterial>::default(), MaterialPlugin::<SmokeMaterial>::default()))
            .add_systems(Update, (start, emitters, scorch, draw).chain())
            .add_systems(PreUpdate, unshake)
            .add_systems(PostUpdate, shake.before(bevy::transform::TransformSystems::Propagate));
    }
    if look.aberration > 0.0 || look.aberration_pulse > 0.0 {
        app.add_systems(Update, aberration);
    }
}

/// A retail effect that keeps throwing its rule's parts while it lives.
#[derive(Component)]
struct Emitter {
    rule: usize,
    next: f32,
}

/// Retail effects starting now throw their rule's parts.
#[allow(clippy::too_many_arguments)]
fn start(
    mut commands: Commands,
    time: Res<Time>,
    look: Res<Look>,
    lib: Option<Res<FxLib>>,
    mut parts: ResMut<Particles>,
    new: Query<(Entity, &Fx, &GlobalTransform), Added<Fx>>,
    fired: Query<&Transform, Added<crate::combat::Shell>>,
    cams: Query<&GlobalTransform, With<Camera3d>>,
    mut names: Local<Vec<String>>,
) {
    let (Some(b), Some(lib)) = (&look.bursts, lib) else { return };
    if names.is_empty() {
        let mut all: Vec<(&str, usize)> = lib.names().collect();
        all.sort_by_key(|(_, i)| *i);
        *names = vec![String::new(); all.last().map_or(0, |(_, i)| i + 1)];
        for (n, i) in all {
            names[i] = n.to_string();
        }
    }
    let now = time.elapsed_secs();
    let eye = cams.iter().next().map(|c| c.translation());
    for (e, fx, gt) in &new {
        let Some(name) = names.get(fx.effect()) else { continue };
        let Some(r) = b.rule(name) else { continue };
        let rule = &b.rules[r];
        // a muzzle flash blows along the shot fired from it (a shell just launched beside it), not its mount's axes
        let at = gt.translation();
        let shot =
            fired.iter().filter(|t| t.translation.distance_squared(at) < 16.0).min_by(|a, b| a.translation.distance_squared(at).total_cmp(&b.translation.distance_squared(at)));
        let forward = shot.map_or(gt.forward().as_vec3(), |t| t.forward().as_vec3());
        for (kind, p) in &rule.parts {
            parts.throw(*kind, p, at, forward, now, None);
        }
        if rule.shake > 0.0 {
            let d = eye.map_or(0.0, |c| c.distance(at));
            parts.shake = parts.shake.max(rule.shake / (1.0 + d / 150.0));
        }
        if rule.every > 0.0 {
            commands.entity(e).try_insert(Emitter { rule: r, next: now + rule.every });
        }
    }
}

/// Effects with `every` keep throwing while they emit.
fn emitters(time: Res<Time>, look: Res<Look>, mut parts: ResMut<Particles>, mut fx: Query<(&mut Emitter, &Fx, &GlobalTransform)>) {
    let Some(b) = &look.bursts else { return };
    let now = time.elapsed_secs();
    for (mut em, f, gt) in &mut fx {
        if now < em.next {
            continue;
        }
        let rule = &b.rules[em.rule];
        em.next = now + rule.every;
        if !f.emitting {
            continue;
        }
        for (kind, p) in &rule.parts {
            parts.throw(*kind, p, gt.translation(), Vec3::Y, now, None);
        }
    }
}

/// Where a shell scars a hull, the hull glows and cools.
fn scorch(time: Res<Time>, look: Res<Look>, mut parts: ResMut<Particles>, mut scars: MessageReader<crate::decals::Scar>) {
    let Some(p) = look.bursts.as_ref().and_then(|b| b.scorch) else {
        scars.clear();
        return;
    };
    for s in scars.read() {
        let p = Part { size: p.size * (0.6 + 0.4 * s.size as f32), speed: 0.0, ..p };
        parts.throw(Kind::Scorch, &p, s.local - s.dir * 0.05, -s.dir, time.elapsed_secs(), Some(s.ship));
    }
}

/// The two meshes the particles are drawn into.
#[derive(Component)]
struct Glows;
#[derive(Component)]
struct Smokes;

#[derive(Default)]
struct Built {
    pos: Vec<[f32; 3]>,
    nor: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl Built {
    /// One quad: `centre` with half extents `a` and `b`. Vertex contract: uv.x = kind * 4 + u (0..1 across the quad),
    /// uv.y = seed * 2 + v (seed 0..63, v 0..1), colour rgb the particle's, a its age as a share of its life.
    fn quad(&mut self, kind: Kind, centre: Vec3, a: Vec3, b: Vec3, normal: Vec3, color: Vec3, age: f32, seed: u32) {
        let first = self.pos.len() as u32;
        for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            self.pos.push((centre + a * x + b * y).to_array());
            self.nor.push(normal.to_array());
            self.uv.push([kind as u32 as f32 * 4.0 + 0.5 + 0.5 * x, seed as f32 * 2.0 + 0.5 + 0.5 * y]);
            self.col.push(color.extend(age).to_array());
        }
        self.idx.extend([first, first + 1, first + 2, first + 1, first + 3, first + 2]);
    }

    fn write(self, mesh: &mut Mesh) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.nor);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uv);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.col);
        mesh.insert_indices(Indices::U32(self.idx));
    }
}

/// The quad a live particle is drawn as, `t` seconds old: flashes, embers, fireballs, scorches, motes and smoke face
/// the camera and grow as the kind does; sparks stretch along their flight; rings lie flat and spread.
fn place(b: &mut Built, p: &Particle, at: Vec3, t: f32, cam: &GlobalTransform) {
    let age = (t / p.life).clamp(0.0, 1.0);
    let (offset, vel) = p.motion(t);
    let centre = at + offset;
    let (right, up, back) = (cam.right().as_vec3(), cam.up().as_vec3(), cam.back().as_vec3());
    let grow = match p.kind {
        Kind::Flash => 0.6 + 0.8 * age,
        Kind::Fireball => 0.4 + 1.1 * (1.0 - (-4.0 * age).exp()),
        Kind::Smoke => 0.5 + 1.8 * age.sqrt(),
        Kind::Scorch => 1.0 - 0.3 * age,
        _ => 1.0,
    };
    let s = p.size * grow;
    match p.kind {
        Kind::Spark => {
            let to_cam = (cam.translation() - centre).normalize_or(back);
            let along = vel * 0.04 + vel.normalize_or(Vec3::Y) * p.size;
            let side = along.cross(to_cam).normalize_or(right) * p.size;
            b.quad(p.kind, centre - along * 0.5, along * 0.5, side, to_cam, p.color, age, p.seed);
        }
        Kind::Ring => {
            let r = p.size + offset.length();
            b.quad(p.kind, at, Vec3::X * r, Vec3::Z * r, Vec3::Y, p.color, age, p.seed);
        }
        _ => b.quad(p.kind, centre + back * s * 0.3, right * s, up * s, back, p.color, age, p.seed),
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    mut commands: Commands,
    time: Res<Time>,
    look: Res<Look>,
    mut parts: ResMut<Particles>,
    cams: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    hosts: Query<&GlobalTransform>,
    sun: Query<(&DirectionalLight, &GlobalTransform)>,
    ambient: Res<GlobalAmbientLight>,
    mut glows: Query<(&Mesh3d, &MeshMaterial3d<BurstMaterial>, &mut Visibility), (With<Glows>, Without<Smokes>)>,
    mut smokes: Query<(&Mesh3d, &MeshMaterial3d<SmokeMaterial>, &mut Visibility), (With<Smokes>, Without<Glows>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    (mut glow_mats, mut smoke_mats): (ResMut<Assets<BurstMaterial>>, ResMut<Assets<SmokeMaterial>>),
) {
    let Some(b) = &look.bursts else { return };
    let now = time.elapsed_secs();
    parts.list.retain(|p| now - p.born < p.life && p.host.is_none_or(|h| hosts.contains(h)));
    let Some((_, cam)) = cams.iter().filter(|(c, _)| c.is_active).min_by_key(|(c, _)| c.order) else { return };
    if glows.is_empty() || smokes.is_empty() {
        // the meshes go with each battle (Scenario); new ones for the next
        let mesh = |meshes: &mut Assets<Mesh>| Mesh3d(meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())));
        let common = || (crate::Scenario, Transform::default(), Visibility::Hidden, NoFrustumCulling, NotShadowCaster);
        if glows.is_empty() {
            let m = glow_mats.add(BurstMaterial { base: super::trails::additive_base(None), extension: BurstGlow::default() });
            commands.spawn((Glows, mesh(&mut meshes), MeshMaterial3d(m), common()));
        }
        if smokes.is_empty() {
            let base = StandardMaterial { unlit: true, cull_mode: None, double_sided: true, alpha_mode: AlphaMode::Blend, ..default() };
            let m = smoke_mats.add(SmokeMaterial { base, extension: SmokeShade::default() });
            commands.spawn((Smokes, mesh(&mut meshes), MeshMaterial3d(m), common()));
        }
        return;
    }
    let (mut glow, mut smoke) = (Built::default(), Built::default());
    let eye = cam.translation();
    let mut smoky: Vec<(f32, &Particle, Vec3)> = vec![];
    for p in &parts.list {
        let at = p.host.and_then(|h| hosts.get(h).ok()).map_or(p.from, |h| h.transform_point(p.from));
        if p.kind == Kind::Smoke {
            smoky.push((eye.distance_squared(at + p.motion(now - p.born).0), p, at));
        } else {
            place(&mut glow, p, at, now - p.born, cam);
        }
    }
    smoky.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (_, p, at) in smoky {
        place(&mut smoke, p, at, now - p.born, cam);
    }
    if let Some(m) = b.motes {
        motes(&mut glow, &m, eye, now, cam);
    }
    let (gm, gmat, mut gvis) = glows.single_mut().expect("one glow mesh");
    gvis.set_if_neq(if glow.idx.is_empty() { Visibility::Hidden } else { Visibility::Inherited });
    if let Some(mut mesh) = meshes.get_mut(&gm.0) {
        glow.write(&mut mesh);
    }
    if let Some(mut m) = glow_mats.get_mut(&gmat.0) {
        m.extension.u.params.x = now;
    }
    let (sm, smat, mut svis) = smokes.single_mut().expect("one smoke mesh");
    svis.set_if_neq(if smoke.idx.is_empty() { Visibility::Hidden } else { Visibility::Inherited });
    if let Some(mut mesh) = meshes.get_mut(&sm.0) {
        smoke.write(&mut mesh);
    }
    if let Some(mut m) = smoke_mats.get_mut(&smat.0) {
        let (dir, light) = sun.iter().next().map_or((Vec3::NEG_Y, Vec3::ONE), |(l, t)| (t.forward().as_vec3(), l.color.to_linear().to_vec3()));
        m.extension.u.sun = dir.extend(now);
        m.extension.u.light = light.extend(1.0);
        m.extension.u.shade = (ambient.color.to_linear().to_vec3() * 0.35).extend(1.0);
    }
    parts.shake *= (1.0 - time.delta_secs() * 4.0).max(0.0);
}

/// Specks of dust in a box around the camera, each drifting slowly on its own; the box wraps, so they never run out.
fn motes(b: &mut Built, m: &Part, eye: Vec3, now: f32, cam: &GlobalTransform) {
    let half = m.speed.max(1.0);
    let span = 2.0 * half;
    let (right, up, back) = (cam.right().as_vec3(), cam.up().as_vec3(), cam.back().as_vec3());
    for i in 0..m.count {
        let h = |k: u32| ((i.wrapping_mul(2654435761).wrapping_add(k.wrapping_mul(40503))) % 10007) as f32 / 10007.0;
        let home = Vec3::new(h(1), h(2), h(3)) * span;
        let drift = Vec3::new(h(4) - 0.5, (h(5) - 0.5) * 0.3, h(6) - 0.5) * 2.0 * now;
        let rel = (home + drift - eye).rem_euclid(Vec3::splat(span)) - Vec3::splat(half);
        let p = eye + rel;
        // fade in from the box's edge and very near the lens; `age` carries that fade (1 = gone)
        let edge = (rel.abs().max_element() / half).clamp(0.0, 1.0);
        let near = (rel.length() / (half * 0.08)).clamp(0.0, 1.0);
        let fade = 1.0 - (1.0 - edge * edge) * near;
        let s = m.size * (0.5 + h(7));
        b.quad(Kind::Mote, p, right * s, up * s, back, m.color, fade, (h(8) * 63.0) as u32);
    }
}

/// The camera jolt applied this frame, taken back before the next one moves the camera.
#[derive(Resource, Default)]
struct Shaken(Vec3);

fn shake(mut commands: Commands, time: Res<Time>, parts: Res<Particles>, mut cams: Query<&mut Transform, With<Camera3d>>) {
    if parts.shake < 0.01 {
        return;
    }
    let t = time.elapsed_secs();
    let j = Vec3::new((t * 37.0).sin() + (t * 23.0).sin() * 0.5, (t * 31.0).sin(), (t * 29.0).sin()) * parts.shake * 0.35;
    for mut tf in &mut cams {
        tf.translation += j;
    }
    commands.insert_resource(Shaken(j));
}

fn unshake(mut commands: Commands, shaken: Option<Res<Shaken>>, mut cams: Query<&mut Transform, With<Camera3d>>) {
    let Some(s) = shaken else { return };
    for mut tf in &mut cams {
        tf.translation -= s.0;
    }
    commands.remove_resource::<Shaken>();
}

/// Colour fringing flaring with the camera's jolt.
fn aberration(look: Res<Look>, parts: Option<Res<Particles>>, mut cams: Query<(Entity, Option<&mut ChromaticAberration>), With<Camera3d>>, mut commands: Commands) {
    let jolt = parts.map_or(0.0, |p| p.shake.min(2.0));
    let want = look.aberration + look.aberration_pulse * jolt;
    for (cam, ca) in &mut cams {
        match ca {
            Some(mut ca) => {
                if (ca.intensity - want).abs() > 1e-4 {
                    ca.intensity = want;
                }
            }
            None => {
                commands.entity(cam).try_insert(ChromaticAberration { intensity: want, ..default() });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_parse_their_parts_with_defaults_per_kind() {
        let b = parse(&serde_json::json!({
            "effects": [{"match": "_Impact", "sparks": {"count": 20}, "smoke": {}, "shake": 0.5}, {"match": "FireOnShip", "embers": {}, "every": 0.2}],
            "scorch": {"seconds": 4},
        }))
        .unwrap();
        let hit = &b.rules[b.rule("CannonBall_Heavy_Impact").unwrap()];
        assert_eq!(hit.parts.iter().map(|(k, _)| *k).collect::<Vec<_>>(), [Kind::Spark, Kind::Smoke]);
        assert_eq!((hit.parts[0].1.count, hit.parts[0].1.drag), (20, 3.0), "unset keys come from the kind's defaults");
        assert_eq!(hit.shake, 0.5);
        assert_eq!(b.rules[b.rule("FireOnShip").unwrap()].every, 0.2);
        assert_eq!(b.scorch.map(|s| s.seconds), Some(4.0));
        assert!(b.motes.is_none() && b.rule("Engine_Navy").is_none());
        assert!(parse(&Value::Null).is_none());
    }

    #[test]
    fn particles_slow_with_drag_rise_and_stay_in_their_cone() {
        let mut ps = Particles::default();
        let p = Part { count: 200, speed: 10.0, spread: 30f32.to_radians(), drag: 2.0, rise: 1.0, ..defaults(Kind::Spark) };
        ps.throw(Kind::Spark, &p, Vec3::ZERO, Vec3::X, 0.0, None);
        assert_eq!(ps.list.len(), 200);
        for q in &ps.list {
            assert!(q.vel.normalize().dot(Vec3::X) >= 30f32.to_radians().cos() - 1e-3, "inside the cone");
        }
        let q = Particle { vel: Vec3::X * 10.0, drag: 2.0, rise: 1.0, ..ps.list[0] };
        let (far, slow) = q.motion(10.0);
        assert!((far.x - 5.0).abs() < 1e-3, "drag stops it after speed / drag: {far}");
        assert!((far.y - 10.0).abs() < 1e-3 && slow.x.abs() < 1e-3, "while it keeps rising");
        let still = Particle { drag: 0.0, ..q };
        assert_eq!(still.motion(2.0).0, Vec3::new(20.0, 2.0, 0.0));
    }

    #[test]
    fn quads_carry_kind_seed_and_age_and_motes_fill_the_box() {
        let cam = GlobalTransform::from(Transform::from_xyz(0.0, 50.0, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z));
        let mut ps = Particles::default();
        ps.throw(Kind::Ember, &defaults(Kind::Ember), Vec3::ZERO, Vec3::Y, 0.0, None);
        let mut b = Built::default();
        let p = ps.list[0];
        place(&mut b, &p, Vec3::ZERO, p.life * 0.5, &cam);
        assert_eq!((b.pos.len(), b.idx.len()), (4, 6));
        assert!(b.uv.iter().all(|uv| (uv[0] / 4.0).floor() == Kind::Ember as u32 as f32 && (uv[1] / 2.0).floor() == p.seed as f32));
        assert!((b.col[0][3] - 0.5).abs() < 1e-4, "alpha is the age share");
        let mut m = Built::default();
        motes(&mut m, &defaults(Kind::Mote), Vec3::new(1000.0, 5.0, -300.0), 12.0, &cam);
        assert_eq!(m.pos.len(), 4 * 300);
        assert!(m.pos.iter().all(|p| (Vec3::from(*p) - Vec3::new(1000.0, 5.0, -300.0)).abs().max_element() <= 121.0), "around the camera");
    }
}
