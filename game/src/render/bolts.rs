//! Not retail: the bolts of storm lightning of the render profile (`nebulae.bolt`, drawn with the mod's `shaders/bolt.wesl`). The
//! retail game shows a bolt as a sprite where it strikes; this draws the bolt itself: the storm-wide random segment
//! `nebula::StormFlash` carries (`from`, `to`), as a jagged, branching ribbon facing the camera, a white-hot thread in a coloured
//! glow, past 1 so it blooms, flickering and fading over `seconds`. When a `SolarStorm` rule replaces the retail look (`replace`), the
//! retail `Lightning` sprite is hidden (`replace.rs`) so the two do not stack. The bolt's light in the storm volume and the sky is the
//! nebula's and the sky's business (they read the same message). All bolts live in one mesh, rebuilt each frame.
//!
//! Only reads `StormFlash`; nothing here changes what a storm does.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use serde_json::Value;

use super::batch::QuadBatch;
use super::{Look, num, rgb};
use crate::nebula::StormFlash;

pub const SHADER: &str = "shaders/bolt.wesl";

/// The `nebulae.bolt` block.
#[derive(Clone, Debug, PartialEq)]
pub struct Bolts {
    /// The glow's colour (linear) and the white-hot thread's.
    pub color: Vec3,
    pub core: Vec3,
    /// The thread's width (world units); the ribbon is `glow` times as wide, the glow fading out to its edge.
    pub width: f32,
    pub glow: f32,
    /// Past 1 so the bloom catches it.
    pub brightness: f32,
    /// How long a bolt lasts.
    pub seconds: f32,
    /// Segments along the main path (rounded up to a power of two, 2..=64).
    pub segments: usize,
    /// Sideways wander of the path, a share of its length.
    pub jitter: f32,
    /// Side branches off the main path.
    pub branches: usize,
}

pub fn parse(v: &Value) -> Option<Bolts> {
    v.is_object().then(|| Bolts {
        color: rgb(&v["color"]).unwrap_or(Vec3::new(0.45, 0.6, 1.0)).max(Vec3::ZERO),
        core: rgb(&v["core"]).unwrap_or(Vec3::new(1.0, 0.97, 0.9)).max(Vec3::ZERO),
        width: num(&v["width"], 2.5).clamp(0.05, 50.0),
        glow: num(&v["glow"], 6.0).clamp(1.0, 30.0),
        brightness: num(&v["brightness"], 3.0).max(0.0),
        seconds: num(&v["seconds"], 0.45).clamp(0.05, 5.0),
        segments: (num(&v["segments"], 16.0).clamp(2.0, 64.0) as usize).next_power_of_two(),
        jitter: num(&v["jitter"], 0.12).clamp(0.0, 0.5),
        branches: num(&v["branches"], 3.0).clamp(0.0, 8.0) as usize,
    })
}

#[derive(Clone, Copy, Default, Debug, ShaderType)]
pub struct BoltUniform {
    /// rgb: the thread's colour, a: brightness
    pub core: Vec4,
    /// x: the thread's share of the ribbon's half width
    pub params: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct BoltGlow {
    #[uniform(100)]
    pub u: BoltUniform,
    /// The frame data (`frame.rs`) at `@binding(99)`.
    #[storage(99, read_only)]
    pub frame: super::frame::FrameBinding,
}

impl MaterialExtension for BoltGlow {
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

pub type BoltMaterial = ExtendedMaterial<StandardMaterial, BoltGlow>;

pub fn plugin(app: &mut App, look: &Look) {
    if look.bolts.is_some() {
        app.init_resource::<Live>().add_plugins(MaterialPlugin::<BoltMaterial>::default()).add_systems(Update, (strike, draw).chain());
    }
}

/// A small xorshift, for the bolts' shapes only (presentation: not the game's random numbers).
struct Rng(u32);

impl Rng {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
    /// A random unit vector at right angles to `dir`.
    fn across(&mut self, dir: Vec3) -> Vec3 {
        let r = Vec3::new(self.f() - 0.5, self.f() - 0.5, self.f() - 0.5);
        let side = r - dir * r.dot(dir);
        if side.length_squared() > 1e-6 { side.normalize() } else { dir.any_orthonormal_vector() }
    }
}

/// A jagged path from `from` to `to` in `segments` (a power of two) steps: midpoint displacement, the sideways wander `jitter` times the
/// length at first and a little over half of it at each finer level.
fn jagged(from: Vec3, to: Vec3, segments: usize, jitter: f32, rng: &mut Rng) -> Vec<Vec3> {
    let dir = (to - from).normalize_or(Vec3::Y);
    let mut amp = jitter * from.distance(to);
    let mut pts = vec![from, to];
    let mut n = 1;
    while n < segments {
        let mut next = Vec::with_capacity(pts.len() * 2 - 1);
        for w in pts.windows(2) {
            next.push(w[0]);
            next.push((w[0] + w[1]) * 0.5 + rng.across(dir) * amp * rng.f());
        }
        next.push(*pts.last().unwrap());
        pts = next;
        amp *= 0.55;
        n *= 2;
    }
    pts
}

/// One bolt: its main path and branches, and how old it is.
struct Bolt {
    main: Vec<Vec3>,
    branches: Vec<Vec<Vec3>>,
    age: f32,
    /// The bolt struck a ship: a bolt that found nothing is dimmer.
    hit: bool,
}

impl Bolt {
    fn new(f: &StormFlash, b: &Bolts, seed: u32) -> Bolt {
        let mut rng = Rng(seed | 1);
        let main = jagged(f.from, f.to, b.segments, b.jitter, &mut rng);
        let len = f.from.distance(f.to);
        let dir = (f.to - f.from).normalize_or(Vec3::Y);
        let mut branches = vec![];
        for _ in 0..b.branches {
            // off the middle of the path, leaning away from it, about a quarter as long
            let i = 2 + (rng.f() * (main.len().saturating_sub(4)) as f32) as usize;
            let at = main[i.min(main.len() - 1)];
            let lean = (dir * 0.6 + rng.across(dir) * 0.8).normalize();
            branches.push(jagged(at, at + lean * len * (0.15 + 0.15 * rng.f()), 4, b.jitter, &mut rng));
        }
        Bolt { main, branches, age: 0.0, hit: f.hit }
    }

    /// How bright the bolt is at its age: a flicker on a fast fade, with a second stroke a third of the way through.
    fn glow(&self, seconds: f32) -> f32 {
        let x = self.age / seconds;
        if !(0.0..1.0).contains(&x) {
            return 0.0;
        }
        let stroke = (1.0 - x).powi(2) * (0.65 + 0.35 * (self.age * 90.0).sin().powi(2)) + 0.6 * (-((x - 0.33) / 0.05).powi(2)).exp();
        stroke * if self.hit { 1.0 } else { 0.7 }
    }
}

#[derive(Resource, Default)]
struct Live {
    bolts: Vec<Bolt>,
    made: u32,
}

/// The one mesh the bolts are drawn into.
#[derive(Component)]
struct BoltMesh;

/// Starts a bolt for every flash that carries a segment.
fn strike(look: Res<Look>, mut flashes: MessageReader<StormFlash>, mut live: ResMut<Live>) {
    let Some(b) = &look.bolts else { return };
    for f in flashes.read() {
        if f.from.distance(f.to) > 1.0 {
            live.made = live.made.wrapping_add(1);
            let seed = f.at.x.to_bits() ^ f.at.z.to_bits().rotate_left(13) ^ live.made.wrapping_mul(0x9e37_79b1);
            live.bolts.push(Bolt::new(f, b, seed));
        }
    }
}

/// A camera-facing strip along `pts` of half width `half`: u 0..1 across, v 0..1 along it.
fn ribbon(b: &mut QuadBatch, pts: &[Vec3], half: f32, eye: Vec3, colour: [f32; 4]) {
    if pts.len() < 2 {
        return;
    }
    let first = b.pos.len() as u32;
    for (i, &p) in pts.iter().enumerate() {
        let along = (pts[(i + 1).min(pts.len() - 1)] - pts[i.saturating_sub(1)]).normalize_or(Vec3::Y);
        let to_cam = (eye - p).normalize_or(Vec3::Y);
        let side = along.cross(to_cam).normalize_or(Vec3::X) * half;
        let v = i as f32 / (pts.len() - 1) as f32;
        for (end, u) in [(1.0, 0.0), (-1.0, 1.0)] {
            b.vertex(p + side * end, to_cam, Vec2::new(u, v), colour);
        }
    }
    for i in 0..(pts.len() - 1) as u32 {
        let a = first + 2 * i;
        b.triangle(a, a + 1, a + 2);
        b.triangle(a + 1, a + 3, a + 2);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    look: Res<Look>,
    time: Res<Time<Real>>,
    mut live: ResMut<Live>,
    cams: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    target: Query<(&Mesh3d, &mut Visibility), With<BoltMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<BoltMaterial>>,
    mut batch: Local<Option<QuadBatch>>,
) {
    let Some(b) = &look.bolts else { return };
    let dt = time.delta_secs();
    for bolt in &mut live.bolts {
        bolt.age += dt;
    }
    live.bolts.retain(|bolt| bolt.age < b.seconds);
    let Some((_, cam)) = cams.iter().filter(|(c, _)| c.is_active).min_by_key(|(c, _)| c.order) else { return };
    let mut target = target;
    let Some((mesh, mut vis)) = target.iter_mut().next() else {
        // the mesh goes with each battle (Scenario); a new one for the next
        let material = materials.add(BoltMaterial {
            base: super::kit::additive_base(None),
            extension: BoltGlow { u: BoltUniform { core: b.core.extend(b.brightness), params: Vec4::new(1.0 / b.glow, 0.0, 0.0, 0.0) }, frame: default() },
        });
        commands.spawn((
            crate::Scenario,
            BoltMesh,
            Mesh3d(meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()))),
            MeshMaterial3d(material),
            Transform::default(),
            Visibility::Hidden,
            NoFrustumCulling,
            NotShadowCaster,
        ));
        return;
    };
    let batch = batch.get_or_insert_with(QuadBatch::with_normals);
    batch.clear();
    let eye = cam.translation();
    for bolt in &live.bolts {
        let colour = b.color.extend(bolt.glow(b.seconds)).to_array();
        ribbon(batch, &bolt.main, b.width * b.glow * 0.5, eye, colour);
        for branch in &bolt.branches {
            ribbon(batch, branch, b.width * b.glow * 0.25, eye, colour);
        }
    }
    vis.set_if_neq(batch.upload(&mut meshes, &mesh.0));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn absent_without_the_key_with_defaults_and_clamps() {
        assert!(parse(&Value::Null).is_none());
        let b = parse(&json!({})).unwrap();
        assert_eq!((b.segments, b.branches, b.seconds), (16, 3, 0.45));
        let b = parse(&json!({"segments": 20, "branches": 99, "width": 0, "seconds": 0})).unwrap();
        assert_eq!((b.segments, b.branches, b.width, b.seconds), (32, 8, 0.05, 0.05), "segments round up to a power of two");
    }

    #[test]
    fn a_path_keeps_its_ends_and_wanders_sideways_by_less_than_its_budget() {
        let (a, z) = (Vec3::new(0.0, 100.0, 0.0), Vec3::new(300.0, -100.0, 50.0));
        let p = jagged(a, z, 16, 0.12, &mut Rng(7));
        assert_eq!((p.len(), p[0], *p.last().unwrap()), (17, a, z));
        let len = a.distance(z);
        let dir = (z - a) / len;
        let wander = p.iter().map(|&q| (q - a - dir * (q - a).dot(dir)).length()).fold(0.0, f32::max);
        assert!(wander > 1.0 && wander < 0.12 * len * 2.0, "jagged but bounded ({wander})");
        assert_eq!(p, jagged(a, z, 16, 0.12, &mut Rng(7)), "the same seed is the same bolt");
        let straight = jagged(a, z, 16, 0.0, &mut Rng(7));
        assert!(straight.iter().all(|q| (q - a).cross(dir).length() < 1e-3), "no jitter, a straight line");
    }

    #[test]
    fn a_bolt_has_branches_that_leave_its_path_and_flickers_out() {
        let b = parse(&json!({"branches": 3})).unwrap();
        let flash = StormFlash { at: Vec3::ZERO, from: Vec3::ZERO, to: Vec3::new(200.0, 0.0, 0.0), hit: true };
        let bolt = Bolt::new(&flash, &b, 42);
        assert_eq!(bolt.branches.len(), 3);
        for br in &bolt.branches {
            assert!(bolt.main.contains(&br[0]) && br.len() == 5, "a branch starts on the main path");
        }
        let at = |age: f32, hit| Bolt { age, hit, ..Bolt::new(&flash, &b, 42) }.glow(b.seconds);
        assert!(at(0.0, true) > 0.6 && at(b.seconds * 0.9, true) < at(0.0, true) && at(b.seconds * 1.1, true) == 0.0, "it fades and ends");
        assert!(at(0.0, false) < at(0.0, true), "a bolt that found no ship is dimmer");
    }

    #[test]
    fn only_a_flash_with_a_segment_starts_a_bolt() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<StormFlash>()
            .init_resource::<Live>()
            .insert_resource(Look::parse(&json!({"nebulae": {"bolt": {"branches": 1}}})))
            .add_systems(Update, strike);
        let at = Vec3::new(5.0, 0.0, 5.0);
        app.world_mut().resource_mut::<Messages<StormFlash>>().write(StormFlash { at, from: at, to: at, hit: false });
        app.world_mut().resource_mut::<Messages<StormFlash>>().write(StormFlash { at, from: at, to: at + Vec3::new(120.0, 0.0, 0.0), hit: true });
        app.update();
        assert_eq!(app.world().resource::<Live>().bolts.len(), 1, "sheet lightning has no bolt to draw");
    }

    #[test]
    fn the_ribbon_has_two_vertices_per_point_across_the_view() {
        let mut b = QuadBatch::with_normals();
        let pts = [Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(20.0, 5.0, 0.0)];
        ribbon(&mut b, &pts, 2.0, Vec3::new(10.0, 100.0, 0.0), [1.0; 4]);
        assert_eq!((b.pos.len(), b.idx.len()), (6, 12));
        for (i, p) in pts.iter().enumerate() {
            let width = (Vec3::from(b.pos[2 * i]) - Vec3::from(b.pos[2 * i + 1])).length();
            assert!((width - 4.0).abs() < 1e-4, "{width}");
            assert_eq!(b.pos[2 * i][1], p.y, "seen from above the strip lies flat, across the path");
        }
    }
}
