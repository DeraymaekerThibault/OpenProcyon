//! Not retail: the frame data every graphics-mod shader can read (`frame.wesl`, `import openprocyon::render::frame::frame`). One
//! small storage buffer under a fixed handle (`HANDLE`), written once a frame and bound at `@binding(99)` of every material a render
//! block draws with. A block whose shader only needs the time, the sun, the camera, the storm's last bolt or the black holes then
//! never touches its material after making it; before, each block wrote its own copy of these into its own uniform every frame.
//!
//! Presentation only: it reads the battle, never writes to it.

use bevy::prelude::*;
use bevy::render::storage::ShaderBuffer;

use super::Look;
use crate::blackhole::{BlackHole, Pull};
use crate::nebula::StormFlash;

/// The frame buffer every render material binds at `@binding(99)`.
pub const HANDLE: Handle<ShaderBuffer> = bevy::asset::uuid_handle!("6f3c2b1e-8a47-4d1c-9f0e-5b2a7c4d9e13");

/// The frame buffer as a material field (`#[storage(99, read_only)] pub frame: FrameBinding`): it defaults to `HANDLE`, so a
/// material made with `..default()` or `frame: default()` is bound to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameBinding(pub Handle<ShaderBuffer>);

impl Default for FrameBinding {
    fn default() -> Self {
        FrameBinding(HANDLE)
    }
}

impl std::ops::Deref for FrameBinding {
    type Target = Handle<ShaderBuffer>;
    fn deref(&self) -> &Handle<ShaderBuffer> {
        &self.0
    }
}

/// Most black holes the buffer carries (retail maps have one).
pub const MAX_HOLES: usize = 4;

// The buffer's layout, one vec4 a row, as `struct Frame` in frame.wesl.
const CLOCK: usize = 0;
const SUN: usize = 1;
const SUN_COLOR: usize = 2;
const CAMERA: usize = 3;
/// The row of the last bolt (where it struck, w real seconds since it).
pub const BOLT_AT: usize = 4;
const BOLT_FROM: usize = 5;
const BOLT_TO: usize = 6;
const HOLES_INFO: usize = 7;
const HOLES: usize = 8;
const HOLE_REACH: usize = HOLES + MAX_HOLES;
/// Rows in the buffer.
pub const ROWS: usize = HOLE_REACH + MAX_HOLES;

/// Seconds since a bolt when there has been none.
const NEVER: f32 = 1.0e6;

/// The frame data as the engine last wrote it (main world), row by row.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Frame {
    pub rows: [Vec4; ROWS],
    /// Real seconds at the last bolt (None: none yet).
    bolt: Option<f32>,
}

impl Default for Frame {
    fn default() -> Self {
        let mut rows = [Vec4::ZERO; ROWS];
        rows[BOLT_AT].w = NEVER;
        Frame { rows, bolt: None }
    }
}

impl Frame {
    /// The battle camera's position as last gathered.
    pub fn camera(&self) -> Vec3 {
        self.rows[CAMERA].truncate()
    }

    /// The sun as last gathered: the direction its light travels and its colour (linear), or None on a map without one.
    pub fn sun(&self) -> Option<(Vec3, Vec3)> {
        (self.rows[SUN].w > 0.0).then(|| (self.rows[SUN].truncate(), self.rows[SUN_COLOR].truncate()))
    }

    /// The storm's last bolt: where it struck, its segment's ends, and real seconds since it (very large: none yet).
    pub fn bolt(&self) -> (Vec3, Vec3, Vec3, f32) {
        (self.rows[BOLT_AT].truncate(), self.rows[BOLT_FROM].truncate(), self.rows[BOLT_TO].truncate(), self.rows[BOLT_AT].w)
    }

    /// The black holes as the sky bends round them: up to `MAX_HOLES` of (where the sky bends, shadow radius, pull radius, kill
    /// core), with the profile's lensing (none without it).
    fn set_holes(&mut self, holes: impl Iterator<Item = (Vec3, f32, f32, f32)>, lensing: f32) {
        let mut n = 0;
        for i in 0..MAX_HOLES {
            self.rows[HOLES + i] = Vec4::ZERO;
            self.rows[HOLE_REACH + i] = Vec4::ZERO;
        }
        if lensing > 0.0 {
            for (at, shadow, radius, core) in holes.take(MAX_HOLES) {
                self.rows[HOLES + n] = at.extend(shadow);
                self.rows[HOLE_REACH + n] = Vec4::new(radius, core, 0.0, 0.0);
                n += 1;
            }
        }
        self.rows[HOLES_INFO] = Vec4::new(n as f32, lensing, 0.0, 0.0);
    }
}

pub fn plugin(app: &mut App) {
    bevy::shader::load_shader_library!(app, "frame.wesl");
    app.init_resource::<Frame>()
        .add_systems(Startup, make_buffer)
        // in `Last`: after the frame's transforms, and out of PostUpdate, whose order Bevy's own systems depend on
        .add_systems(Last, (gather, upload).chain());
}

fn make_buffer(mut buffers: ResMut<Assets<ShaderBuffer>>) {
    let _ = buffers.insert(&HANDLE, ShaderBuffer::new(vec![Vec4::ZERO; ROWS], default()));
}

/// The battle camera: the active 3-D camera drawn first.
fn battle_camera<'a>(cams: impl Iterator<Item = (&'a Camera, &'a GlobalTransform)>) -> Option<Vec3> {
    cams.filter(|(c, _)| c.is_active).min_by_key(|(c, _)| c.order).map(|(_, t)| t.translation())
}

#[allow(clippy::too_many_arguments)]
fn gather(
    look: Res<Look>,
    real: Res<Time<Real>>,
    battle: Res<Time>,
    ambient: Res<GlobalAmbientLight>,
    cams: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    suns: Query<(&DirectionalLight, &GlobalTransform)>,
    holes: Query<(&GlobalTransform, &Pull), With<BlackHole>>,
    mut bolts: MessageReader<StormFlash>,
    mut frame: ResMut<Frame>,
) {
    let now = real.elapsed_secs();
    frame.rows[CLOCK] = Vec4::new(now, battle.elapsed_secs(), real.delta_secs(), battle.delta_secs());
    let (sun, colour) = suns.iter().next().map_or((Vec4::ZERO, Vec3::ZERO), |(l, t)| (t.forward().as_vec3().extend(l.illuminance / 10_000.0), l.color.to_linear().to_vec3()));
    frame.rows[SUN] = sun;
    frame.rows[SUN_COLOR] = colour.extend(ambient.brightness);
    if let Some(eye) = battle_camera(cams.iter()) {
        frame.rows[CAMERA] = eye.extend(0.0);
    }
    for f in bolts.read() {
        frame.bolt = Some(now);
        frame.rows[BOLT_AT] = f.at.extend(0.0);
        frame.rows[BOLT_FROM] = f.from.extend(if f.hit { 1.0 } else { 0.0 });
        frame.rows[BOLT_TO] = f.to.extend(0.0);
    }
    frame.rows[BOLT_AT].w = frame.bolt.map_or(NEVER, |t| now - t);
    let b = look.black_holes.as_ref();
    let lensing = b.map_or(0.0, |b| b.lensing);
    frame.set_holes(
        holes.iter().map(|(g, p)| {
            let (at, shadow) = b.map_or((g.translation(), p.core()), |b| (b.singularity(g.translation(), p), b.shadow_core(p)));
            (at, shadow, p.radius, p.core())
        }),
        lensing,
    );
}

/// The frame data into the buffer: the same size every frame, so Bevy rewrites the GPU buffer in place and no material's
/// bind group changes.
fn upload(frame: Res<Frame>, mut buffers: ResMut<Assets<ShaderBuffer>>) {
    if let Some(mut b) = buffers.get_mut(&HANDLE)
        && let Some(rows) = b.cast_slice_mut::<Vec4>()
    {
        rows.copy_from_slice(&frame.rows);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn app(look: serde_json::Value) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<ShaderBuffer>()
            .add_message::<StormFlash>()
            .init_resource::<GlobalAmbientLight>()
            .insert_resource(Look::parse(&look))
            .init_resource::<Frame>()
            .add_systems(Startup, make_buffer)
            .add_systems(Update, (gather, upload).chain());
        app
    }

    fn buffer(app: &App) -> Vec<Vec4> {
        app.world().resource::<Assets<ShaderBuffer>>().get(&HANDLE).unwrap().cast_slice::<Vec4>().unwrap().to_vec()
    }

    #[test]
    fn the_buffer_matches_the_shader_struct_and_holds_what_was_gathered() {
        let src = include_str!("frame.wesl");
        let body = &src[src.find("struct Frame {").unwrap()..];
        let body = &body[..body.find('}').unwrap()];
        let rows: usize =
            body.lines().map(str::trim).filter(|l| !l.starts_with("//") && l.contains(':')).map(|l| if l.contains("array<vec4<f32>, 4>") { MAX_HOLES } else { 1 }).sum();
        assert_eq!(rows, ROWS, "frame.wesl and frame.rs list the same rows");

        let mut app = app(json!({"black_holes": {"lensing": 1.5}}));
        app.world_mut().spawn((DirectionalLight { illuminance: 20_000.0, ..default() }, GlobalTransform::from(Transform::default().looking_to(Vec3::NEG_Y, Vec3::Z))));
        app.world_mut().spawn((Camera3d::default(), Camera::default(), GlobalTransform::from_xyz(1.0, 2.0, 3.0)));
        let hole = Pull::for_terrain("Terrain_BlackHole_Med").unwrap();
        app.world_mut().spawn((BlackHole::default(), hole, GlobalTransform::from_xyz(100.0, 0.0, 0.0)));
        app.update();
        app.world_mut().resource_mut::<Messages<StormFlash>>().write(StormFlash { at: Vec3::X, from: Vec3::ZERO, to: Vec3::X * 2.0, hit: true });
        app.update();
        let rows = buffer(&app);
        assert_eq!(rows.len(), ROWS);
        assert!((rows[SUN].truncate() - Vec3::NEG_Y).length() < 1e-5 && rows[SUN].w == 2.0, "the way the light travels, in 10000 lux");
        assert_eq!(rows[CAMERA].truncate(), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!((rows[BOLT_AT].truncate(), rows[BOLT_FROM], rows[BOLT_TO].truncate()), (Vec3::X, Vec4::new(0.0, 0.0, 0.0, 1.0), Vec3::X * 2.0));
        assert!(rows[BOLT_AT].w < 1.0, "the bolt was just now");
        assert_eq!(rows[HOLES_INFO], Vec4::new(1.0, 1.5, 0.0, 0.0));
        assert_eq!((rows[HOLES].truncate(), rows[HOLES].w), (Vec3::new(100.0, 0.0, 0.0), hole.core()), "a flat hole bends the sky at itself, round its kill core");
        assert_eq!(rows[HOLE_REACH], Vec4::new(hole.radius, hole.core(), 0.0, 0.0));
    }

    #[test]
    fn no_bolt_yet_reads_as_long_ago_and_no_lensing_means_no_holes() {
        let mut app = app(json!({}));
        app.world_mut().spawn((BlackHole::default(), Pull::for_terrain("Terrain_BlackHole_Med").unwrap(), GlobalTransform::default()));
        app.update();
        let rows = buffer(&app);
        assert!(rows[BOLT_AT].w >= NEVER);
        assert_eq!(rows[HOLES_INFO].x, 0.0, "without the `black_holes` block nothing bends");
    }

    #[test]
    fn the_sun_and_the_bolt_are_read_back_as_gathered() {
        let mut f = Frame::default();
        assert_eq!(f.sun(), None, "a map without a sun");
        assert!(f.bolt().3 >= NEVER, "no bolt yet");
        f.rows[SUN] = Vec3::NEG_Y.extend(2.0);
        f.rows[SUN_COLOR] = Vec3::new(1.0, 0.9, 0.8).extend(80.0);
        assert_eq!(f.sun(), Some((Vec3::NEG_Y, Vec3::new(1.0, 0.9, 0.8))));
    }

    #[test]
    fn many_holes_are_capped() {
        let mut f = Frame::default();
        f.set_holes((0..6).map(|i| (Vec3::splat(i as f32), 36.0, 145.0, 36.0)), 1.0);
        assert_eq!(f.rows[HOLES_INFO].x, MAX_HOLES as f32);
        assert_eq!(f.rows[HOLES + 1], Vec4::new(1.0, 1.0, 1.0, 36.0));
    }
}
