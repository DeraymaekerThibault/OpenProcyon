//! Not retail: beam guns (a mod feature, `combat/beam.rs`) as the render profile draws them (`beams` block, the mod's
//! `shaders/beam.wesl`). Classic draws a beam as two cylinders and a sphere; this draws light, on quads facing the camera:
//! - charging: a flare swelling at the muzzle, rings closing on it down the barrel's line, motes of light spiralling in from all
//!   round and arcs crackling off the muzzle, all quickening as the charge fills;
//! - firing: a ribbon along the beam (the shader puts a white-hot thread in a sheath of the gun's colour, with heat streaming out
//!   along it and two strands winding round it), flaring wide for an instant as it lights, a flare and arcs at the muzzle, and,
//!   where it is on a ship, a flare, arcs and a spray of sparks at the hull. The beam fattens and whitens as its damage ramps up
//!   (`BeamDef::power`), so the player can see it bite harder the longer it holds.
//!
//! All beams live in one additive mesh, rebuilt each frame after the transforms have settled, so the beam leaves the muzzle of a
//! turning ship exactly. With the block present the classic meshes are hidden (`combat::beam::Replaced`); the muzzle and impact
//! lights stay classic's. Only reads the guns' beam phases; nothing here changes what a beam does.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use serde_json::Value;

use super::batch::QuadBatch;
use super::{Look, num, rgb};
use crate::combat::GunStates;
use crate::combat::beam::Phase;
use crate::data::Library;

pub const SHADER: &str = "shaders/beam.wesl";

/// The `beams` block.
#[derive(Clone, Debug, PartialEq)]
pub struct Beams {
    /// The white-hot thread's colour (linear); the sheath takes the gun's `beam.color`.
    pub core: Vec3,
    /// Past 1 so the bloom catches it.
    pub brightness: f32,
    /// The ribbon is `glow` times as wide as the gun's `beam.width` (the thread), the sheath fading out to its edge.
    pub glow: f32,
    /// Motes of light drawn in toward a charging muzzle.
    pub motes: usize,
    /// Arcs crackling round the muzzle (and half as many over the struck hull).
    pub arcs: usize,
    /// Sparks thrown off the hull the beam is on.
    pub sparks: usize,
}

pub fn parse(v: &Value) -> Option<Beams> {
    v.is_object().then(|| Beams {
        core: rgb(&v["core"]).unwrap_or(Vec3::new(1.0, 0.98, 0.95)).max(Vec3::ZERO),
        brightness: num(&v["brightness"], 2.0).max(0.0),
        glow: num(&v["glow"], 6.0).clamp(1.5, 30.0),
        motes: num(&v["motes"], 18.0).clamp(0.0, 64.0) as usize,
        arcs: num(&v["arcs"], 4.0).clamp(0.0, 12.0) as usize,
        sparks: num(&v["sparks"], 14.0).clamp(0.0, 64.0) as usize,
    })
}

#[derive(Clone, Copy, Default, Debug, ShaderType)]
pub struct BeamUniform {
    /// rgb: the thread's colour, a: brightness
    pub core: Vec4,
    /// x: the thread's share of the ribbon's half width
    pub params: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct BeamGlow {
    #[uniform(100)]
    pub u: BeamUniform,
    /// The frame data (`frame.rs`) at `@binding(99)`.
    #[storage(99, read_only)]
    pub frame: super::frame::FrameBinding,
}

impl MaterialExtension for BeamGlow {
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

pub type BeamMaterial = ExtendedMaterial<StandardMaterial, BeamGlow>;

pub fn plugin(app: &mut App, look: &Look) {
    if look.beams.is_some() {
        app.insert_resource(crate::combat::beam::Replaced)
            .add_plugins(MaterialPlugin::<BeamMaterial>::default())
            .add_systems(PostUpdate, draw.after(bevy::transform::TransformSystems::Propagate));
    }
}

/// What a quad is, told to the shader by the whole part of its u: `u = 2 * kind + (0..1)`.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    /// The beam's ribbon: u across, v the distance from the muzzle in beam widths.
    Beam = 0,
    /// A round flare with a turning star in it (muzzle, impact).
    Flare = 1,
    /// A thin ring (the charge closing on the muzzle).
    Ring = 2,
    /// A soft dot (motes, sparks).
    Mote = 3,
    /// A crackling thread (arcs): u across, v along.
    Arc = 4,
}

impl Kind {
    fn u(self, across: f32) -> f32 {
        2.0 * self as u32 as f32 + across
    }
    fn rect(self) -> Rect {
        Rect::new(self.u(0.0), 0.0, self.u(1.0), 1.0)
    }
}

/// One charging or firing beam gun as the drawing needs it.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// The muzzle and the barrel's direction, in the world.
    pub origin: Vec3,
    pub dir: Vec3,
    pub phase: Phase,
    pub width: f32,
    pub color: Vec3,
    pub charge: f32,
    pub duration: f32,
    /// The damage multiplier now (1 until the beam has stayed on a ship) and its cap.
    pub power: f32,
    pub max_power: f32,
    /// Sets this gun's motes, arcs and sparks apart from another's.
    pub seed: u32,
}

/// A repeatable number in 0..1 from two integers (the effects' shapes only: not the game's random numbers).
fn hash(a: u32, b: u32) -> f32 {
    let mut x = a.wrapping_mul(0x9e37_79b1) ^ b.wrapping_mul(0x85eb_ca6b).rotate_left(15);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// A repeatable unit vector.
fn unit(a: u32, b: u32) -> Vec3 {
    let z = hash(a, b) * 2.0 - 1.0;
    let t = hash(a, b ^ 0x5bd1) * std::f32::consts::TAU;
    let r = (1.0 - z * z).max(0.0).sqrt();
    Vec3::new(r * t.cos(), r * t.sin(), z)
}

/// A quad facing the camera at `at`.
fn spot(b: &mut QuadBatch, kind: Kind, at: Vec3, half: f32, eye: Vec3, roll: f32, colour: Vec4) {
    if half <= 0.0 || colour.w <= 0.0 {
        return;
    }
    let to_cam = (eye - at).normalize_or(Vec3::Y);
    let right = to_cam.cross(Vec3::Y).normalize_or(Vec3::X);
    let up = right.cross(to_cam);
    let (s, c) = roll.sin_cos();
    b.quad(at, (right * c + up * s) * half, (up * c - right * s) * half, to_cam, kind.rect(), colour.to_array());
}

/// A camera-facing strip along `pts`: `half`, `v` and `alpha` are given per point.
fn strip(b: &mut QuadBatch, kind: Kind, pts: &[Vec3], eye: Vec3, colour: Vec3, per: impl Fn(usize) -> (f32, f32, f32)) {
    if pts.len() < 2 {
        return;
    }
    let first = b.pos.len() as u32;
    for (i, &p) in pts.iter().enumerate() {
        let along = (pts[(i + 1).min(pts.len() - 1)] - pts[i.saturating_sub(1)]).normalize_or(Vec3::Y);
        let to_cam = (eye - p).normalize_or(Vec3::Y);
        let side = along.cross(to_cam).normalize_or(Vec3::X);
        let (half, v, alpha) = per(i);
        for (end, u) in [(1.0, 0.0), (-1.0, 1.0)] {
            b.vertex(p + side * half * end, to_cam, Vec2::new(kind.u(u), v), colour.extend(alpha).to_array());
        }
    }
    b.strip(first, (pts.len() - 1) as u32);
}

/// A crackling arc from `from` to `to`: a jagged thread, its kinks from `seed`.
fn arc(b: &mut QuadBatch, from: Vec3, to: Vec3, half: f32, eye: Vec3, colour: Vec3, alpha: f32, seed: u32) {
    const N: usize = 8;
    let span = to - from;
    let len = span.length();
    if len < 1e-3 {
        return;
    }
    let dir = span / len;
    let pts: Vec<Vec3> = (0..=N)
        .map(|i| {
            let t = i as f32 / N as f32;
            let r = unit(seed, i as u32);
            let side = (r - dir * r.dot(dir)) * len * 0.22 * (std::f32::consts::PI * t).sin();
            from + span * t + side
        })
        .collect();
    strip(b, Kind::Arc, &pts, eye, colour, |i| (half, i as f32 / N as f32, alpha));
}

/// Size of the fast beam flicker (a fraction of the glow's size; the slower, larger one is 0.1). Engine choice, not the original's.
const BEAM_FLICKER_FINE: f32 = 0.07;
/// The arcs change shape once per this many seconds (about fourteen times a second). Engine choice, not the original's.
const ARC_TICK_SECONDS: f32 = 0.07;
/// A firing beam thins out over its last this many seconds. Engine choice, not the original's.
const FADE_OUT_SECONDS: f32 = 0.3;
/// A firing beam flares wide for this many seconds as it lights. Engine choice, not the original's.
const IGNITE_SECONDS: f32 = 0.18;
/// Hit sparks fly out at least this far (beam widths) ... Engine choice, not the original's.
const SPARK_REACH_MIN: f32 = 3.0;
/// ... and up to this much farther over their life. Engine choice, not the original's.
const SPARK_REACH_SPAN: f32 = 30.0;

/// Draws one beam gun into the batch at time `now` (seconds, for everything that moves).
pub fn emit(b: &mut QuadBatch, cfg: &Beams, v: &View, eye: Vec3, now: f32) {
    let w = v.width;
    let flicker = 1.0 + 0.1 * (now * 47.0).sin() + BEAM_FLICKER_FINE * (now * 113.0).sin();
    let (side_a, side_b) = v.dir.any_orthonormal_pair();
    // the arcs change shape some fourteen times a second
    let tick = (now / ARC_TICK_SECONDS) as u32;
    match v.phase {
        Phase::Idle => {}
        Phase::Charging { t } => {
            let f = (t / v.charge.max(0.01)).clamp(0.0, 1.0);
            spot(b, Kind::Flare, v.origin, w * (1.5 + 5.5 * f * f) * flicker, eye, now * (0.6 + 2.4 * f), v.color.extend(0.2 + 0.8 * f));
            // rings come in along the barrel's line and close on the muzzle, faster as the charge fills
            for k in 0..3 {
                let x = (now * (0.7 + 1.8 * f) + k as f32 / 3.0).fract();
                let r = w * (9.0 - 8.0 * x);
                let at = v.origin + v.dir * w * 5.0 * (1.0 - x);
                let alpha = f.max(0.12) * (x * 4.0).min(1.0) * (1.0 - x * x * 0.4);
                b.quad(at, side_a * r, side_b * r, -v.dir, Kind::Ring.rect(), v.color.extend(alpha).to_array());
            }
            // motes spiral in from all round; more of them, and quicker, as the charge fills
            let n = ((cfg.motes as f32) * (0.25 + 0.75 * f)).ceil() as u32;
            for i in 0..n {
                let x = (now * (0.45 + 1.5 * f) * (0.6 + 0.8 * hash(v.seed, i)) + hash(v.seed ^ 0x51, i)).fract();
                let from = unit(v.seed ^ 0xa7, i);
                let from = (from + v.dir * 0.35).normalize_or(v.dir);
                let swirl = Quat::from_axis_angle(v.dir, (1.0 - x) * 2.6);
                let at = v.origin + swirl * from * w * 24.0 * (1.0 - x).powf(1.4);
                let alpha = (0.25 + 0.75 * f) * (x * 5.0).min(1.0);
                spot(b, Kind::Mote, at, w * (0.45 + 0.7 * hash(v.seed ^ 0x33, i)) * (0.5 + 0.5 * x), eye, 0.0, v.color.extend(alpha));
            }
            for i in 0..((cfg.arcs as f32) * f).round() as u32 {
                let seed = v.seed ^ tick.wrapping_mul(31).wrapping_add(i);
                let out = (unit(seed, 90) + v.dir * 0.5).normalize_or(v.dir);
                arc(b, v.origin, v.origin + out * w * (5.0 + 7.0 * hash(seed, 91)), w * 0.9, eye, v.color, f, seed);
            }
        }
        Phase::Firing { t, length, victim, .. } => {
            // full at once, thinning out over the last 0.3 s; for an instant as it lights it is three times as wide
            let fade = ((v.duration - t) / FADE_OUT_SECONDS).clamp(0.0, 1.0);
            let ignite = (1.0 - t / IGNITE_SECONDS).clamp(0.0, 1.0);
            let heat = if v.max_power > 1.0 { ((v.power - 1.0) / (v.max_power - 1.0)).clamp(0.0, 1.0) } else { 0.0 };
            let colour = v.color.lerp(v.color + Vec3::splat(0.6), heat);
            let half = w * cfg.glow * 0.5 * (1.0 + 0.3 * heat + 2.0 * ignite) * fade * flicker;
            const N: usize = 40;
            let pts: Vec<Vec3> = (0..=N).map(|i| v.origin + v.dir * length * (i as f32 / N as f32)).collect();
            let hit = victim.is_some();
            strip(b, Kind::Beam, &pts, eye, colour, |i| {
                let s = length * i as f32 / N as f32;
                // wider where it leaves the muzzle; a beam that found nothing fades out toward its range
                let flare = 1.0 + 0.9 * (-s / (w * 14.0)).exp();
                let end = if hit { 1.0 } else { 1.0 - ((s / length.max(1e-3) - 0.8) / 0.2).clamp(0.0, 1.0) };
                (half * flare, s / w.max(1e-3), (1.0 + 0.4 * heat) * end)
            });
            let glare = v.color.extend(fade.max(0.2));
            spot(b, Kind::Flare, v.origin, w * (6.0 + 12.0 * ignite) * flicker * fade.max(0.3), eye, now * 3.0, glare);
            for i in 0..(cfg.arcs / 2) as u32 {
                let seed = v.seed ^ tick.wrapping_mul(17).wrapping_add(i);
                let out = (unit(seed, 90) + v.dir * 1.2).normalize_or(v.dir);
                arc(b, v.origin, v.origin + out * w * (5.0 + 6.0 * hash(seed, 91)), w * 0.8, eye, colour, fade, seed);
            }
            if hit {
                let at = v.origin + v.dir * length;
                spot(b, Kind::Flare, at, w * (7.0 + 7.0 * heat) * flicker * fade.max(0.3), eye, -now * 4.0, colour.extend(fade.max(0.2)));
                for i in 0..(cfg.arcs / 2).max(1) as u32 {
                    let seed = v.seed ^ tick.wrapping_mul(13).wrapping_add(i + 40);
                    let out = (unit(seed, 92) - v.dir * 0.4).normalize_or(-v.dir);
                    arc(b, at, at + out * w * (6.0 + 8.0 * hash(seed, 93)), w * 0.8, eye, colour, fade, seed);
                }
                // sparks fly back off the hull, each on its own short life
                for i in 0..cfg.sparks as u32 {
                    let x = (now * (1.8 + 1.6 * hash(v.seed ^ 0x77, i)) + hash(v.seed ^ 0x99, i)).fract();
                    let life = (now * (1.8 + 1.6 * hash(v.seed ^ 0x77, i)) + hash(v.seed ^ 0x99, i)) as u32;
                    let out = (unit(v.seed ^ life.wrapping_mul(7919), i) - v.dir * 0.7).normalize_or(-v.dir);
                    let p = at + out * w * (SPARK_REACH_MIN + SPARK_REACH_SPAN * x) * (0.4 + 0.6 * hash(v.seed ^ 0x13, i)) - Vec3::Y * w * 10.0 * x * x;
                    spot(b, Kind::Mote, p, w * 0.6 * (1.0 - x) * (1.0 + heat), eye, 0.0, colour.extend(fade * (1.0 - x) * (1.0 - x)));
                }
            }
        }
    }
}

/// The one mesh the beams are drawn into.
#[derive(Component)]
struct BeamMesh;

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    look: Res<Look>,
    time: Res<Time>,
    lib: Res<Library>,
    guns: Query<(Entity, &GunStates)>,
    places: Query<&GlobalTransform>,
    cams: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut target: Query<(&Mesh3d, &mut Visibility), With<BeamMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<BeamMaterial>>,
    mut batch: Local<Option<QuadBatch>>,
) {
    let Some(cfg) = &look.beams else { return };
    let Some((_, cam)) = cams.iter().filter(|(c, _)| c.is_active).min_by_key(|(c, _)| c.order) else { return };
    let Some((mesh, mut vis)) = target.iter_mut().next() else {
        // the mesh goes with each battle (Scenario); a new one for the next
        let material = materials.add(BeamMaterial {
            base: super::kit::additive_base(None),
            extension: BeamGlow { u: BeamUniform { core: cfg.core.extend(cfg.brightness), params: Vec4::new(1.0 / cfg.glow, 0.0, 0.0, 0.0) }, frame: default() },
        });
        super::kit::spawn_batch_mesh(&mut commands, &mut meshes, material, BeamMesh);
        return;
    };
    let batch = batch.get_or_insert_with(QuadBatch::with_normals);
    batch.clear();
    let (eye, now) = (cam.translation(), time.elapsed_secs());
    for (ship, states) in &guns {
        for (index, gun, muzzle, phase) in states.beams() {
            let (Some(def), Ok(at)) = (lib.guns.get(gun).and_then(|g| g.beam.as_ref()), places.get(muzzle)) else { continue };
            let power = match phase {
                Phase::Firing { victim: Some(_), on, .. } => def.power(on),
                _ => 1.0,
            };
            let view = View {
                origin: at.translation(),
                dir: (at.rotation() * Vec3::NEG_Z).normalize_or(Vec3::NEG_Z),
                phase,
                width: def.width,
                color: Vec3::from(def.color),
                charge: def.charge,
                duration: def.duration,
                power,
                max_power: def.max_power,
                seed: ship.index().index().wrapping_mul(131).wrapping_add(index as u32),
            };
            emit(batch, cfg, &view, eye, now);
        }
    }
    vis.set_if_neq(batch.upload(&mut meshes, &mesh.0));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn view(phase: Phase) -> View {
        View { origin: Vec3::ZERO, dir: Vec3::NEG_Z, phase, width: 0.4, color: Vec3::new(0.5, 1.5, 2.0), charge: 2.0, duration: 3.0, power: 1.0, max_power: 2.0, seed: 7 }
    }

    fn drawn(cfg: &Beams, v: &View, now: f32) -> QuadBatch {
        let mut b = QuadBatch::with_normals();
        emit(&mut b, cfg, v, Vec3::new(0.0, 60.0, 30.0), now);
        b
    }

    /// The vertices of one kind (by the whole part of u).
    fn of(b: &QuadBatch, kind: Kind) -> Vec<usize> {
        (0..b.uv.len()).filter(|&i| (b.uv[i][0] / 2.0).floor() as u32 == kind as u32 && b.uv[i][0] <= kind.u(1.0)).collect()
    }

    #[test]
    fn absent_without_the_key_with_defaults_and_clamps() {
        assert!(parse(&Value::Null).is_none());
        let b = parse(&json!({})).unwrap();
        assert_eq!((b.glow, b.motes, b.arcs, b.sparks), (6.0, 18, 4, 14));
        let b = parse(&json!({"glow": 0, "motes": 999, "brightness": -1})).unwrap();
        assert_eq!((b.glow, b.motes, b.brightness), (1.5, 64, 0.0));
    }

    #[test]
    fn an_idle_gun_draws_nothing_and_a_charging_one_no_beam() {
        let cfg = parse(&json!({})).unwrap();
        assert!(drawn(&cfg, &view(Phase::Idle), 1.0).is_empty());
        let early = drawn(&cfg, &view(Phase::Charging { t: 0.2 }), 1.0);
        let late = drawn(&cfg, &view(Phase::Charging { t: 1.9 }), 1.0);
        for b in [&early, &late] {
            assert!(of(b, Kind::Beam).is_empty(), "no beam before it fires");
            assert_eq!(of(b, Kind::Ring).len(), 12, "three rings closing on the muzzle");
            assert_eq!(of(b, Kind::Flare).len(), 4);
        }
        assert!(of(&late, Kind::Mote).len() > of(&early, Kind::Mote).len(), "more motes as the charge fills");
        assert!(of(&late, Kind::Arc).len() > of(&early, Kind::Arc).len(), "and the arcs start");
        let size = |b: &QuadBatch| {
            let i = of(b, Kind::Flare);
            (Vec3::from(b.pos[i[0]]) - Vec3::from(b.pos[i[3]])).length()
        };
        assert!(size(&late) > size(&early), "the flare swells");
    }

    #[test]
    fn the_beam_runs_its_length_from_the_muzzle_and_marks_distance_in_widths() {
        let cfg = parse(&json!({"glow": 6})).unwrap();
        let firing = Phase::Firing { t: 1.0, victim: None, on: 0.0, tick: 0.0, length: 80.0 };
        let b = drawn(&cfg, &view(firing), 0.0);
        let beam = of(&b, Kind::Beam);
        let far = beam.iter().map(|&i| -b.pos[i][2]).fold(0.0, f32::max);
        assert!((far - 80.0).abs() < 1e-3, "{far}");
        let v_max = beam.iter().map(|&i| b.uv[i][1]).fold(0.0, f32::max);
        assert!((v_max - 200.0).abs() < 1e-2, "80 units of a 0.4 wide beam: {v_max}");
        assert!(b.col[*beam.last().unwrap()][3] < 1e-5, "a beam that found nothing fades out at its range");
        assert!(of(&b, Kind::Mote).is_empty(), "no sparks without a hull");
    }

    #[test]
    fn a_beam_on_a_ship_ends_bright_with_sparks_and_fattens_as_its_power_ramps() {
        let cfg = parse(&json!({})).unwrap();
        let victim = Some(Entity::from_raw_u32(3).unwrap());
        let firing = Phase::Firing { t: 1.0, victim, on: 0.0, tick: 0.0, length: 50.0 };
        let cold = drawn(&cfg, &view(firing), 0.0);
        let hot = drawn(&cfg, &View { power: 2.0, ..view(firing) }, 0.0);
        let beam = of(&cold, Kind::Beam);
        assert!(cold.col[*beam.last().unwrap()][3] >= 1.0, "it ends on the hull at full strength");
        assert_eq!(of(&cold, Kind::Mote).len(), 14 * 4, "sparks off the hull");
        assert_eq!(of(&cold, Kind::Flare).len(), 8, "a flare at the muzzle and one at the hull");
        let width = |b: &QuadBatch| {
            let i = of(b, Kind::Beam);
            let k = i.len() - 2;
            (Vec3::from(b.pos[i[k]]) - Vec3::from(b.pos[i[k + 1]])).length()
        };
        assert!(width(&hot) > width(&cold) * 1.2, "{} {}", width(&hot), width(&cold));
        assert!(hot.col[of(&hot, Kind::Beam)[0]][0] > cold.col[beam[0]][0], "and whitens");
    }

    #[test]
    fn it_flares_wide_as_it_lights_and_thins_out_as_it_ends() {
        let cfg = parse(&json!({})).unwrap();
        let at = |t: f32| {
            let b = drawn(&cfg, &view(Phase::Firing { t, victim: None, on: 0.0, tick: 0.0, length: 50.0 }), 0.0);
            let i = of(&b, Kind::Beam);
            (Vec3::from(b.pos[i[40]]) - Vec3::from(b.pos[i[41]])).length()
        };
        assert!(at(0.02) > at(1.0) * 2.0, "the ignition flash");
        assert!(at(2.9) < at(1.0) * 0.5 && at(3.0) == 0.0, "thinning to nothing");
    }
}
