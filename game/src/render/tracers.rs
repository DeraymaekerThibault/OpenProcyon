//! Not retail: projectile tracers of the render profile (part of render/mod.rs's look). Every shell in flight draws a streak
//! of light along the path it has flown (the last `seconds` of it) and a glowing head, both facing the camera, drawn
//! additively with the mod's `shaders/tracer.wesl`. When the shell is gone (a hit, a miss running out) its streak shrinks
//! into the point where it ended. The bullet's rule (`tracers.bullets`, first whose `match` is in the bullet's name, else
//! `tracers.default`) gives its colour and size; width 0 draws none. The retail flight effects stay, thinned by
//! `particles`. All streaks are one mesh, rebuilt each frame. Contract: `TracerUniform`, and the mesh described at
//! `ribbon`.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};

use super::bursts::Part;
use super::{Look, num, rgb};
use crate::combat::Shell;
use crate::fx::{EffectStarted, Fx};

/// The shader's asset path; the mod ships it.
pub const SHADER: &str = "shaders/tracer.wesl";

/// The tracers' look.
#[derive(Clone, Debug, PartialEq)]
pub struct Tracers {
    /// Per bullet type, first match wins; `default` for the rest.
    pub bullets: Vec<(String, Tracer)>,
    pub default: Tracer,
    /// White-hot colour of the head and the streak's middle.
    pub core: Vec3,
    pub brightness: f32,
    /// Share of the retail flight effects' particles kept.
    pub particles: f32,
}

/// One bullet type's tracer: colour (linear), streak width, how many seconds of flight it shows, head size (world units),
/// how much longer than wide the head is drawn along its flight, and smoke left behind (a `bursts` smoke part whose
/// `count` is puffs per second; needs the `bursts` block).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tracer {
    pub color: Vec3,
    pub width: f32,
    pub seconds: f32,
    pub head: f32,
    pub stretch: f32,
    pub smoke: Option<Part>,
}

fn tracer(v: &Value, d: Tracer) -> Tracer {
    Tracer {
        color: rgb(&v["color"]).unwrap_or(d.color),
        width: num(&v["width"], d.width),
        seconds: num(&v["seconds"], d.seconds).max(0.01),
        head: num(&v["head"], d.head),
        stretch: num(&v["stretch"], d.stretch).max(0.0),
        smoke: if v["smoke"].is_null() { d.smoke } else { super::bursts::smoke_part(&v["smoke"]) },
    }
}

pub fn parse(v: &Value) -> Option<Tracers> {
    v.is_object().then(|| {
        let default = tracer(&v["default"], Tracer { color: Vec3::new(1.0, 0.6, 0.3), width: 0.4, seconds: 0.25, head: 1.0, stretch: 1.5, smoke: None });
        Tracers {
            bullets: v["bullets"].as_array().into_iter().flatten().filter_map(|b| Some((b["match"].as_str()?.to_lowercase(), tracer(b, default)))).collect(),
            default,
            core: rgb(&v["core"]).unwrap_or(Vec3::new(1.0, 0.95, 0.85)),
            brightness: num(&v["brightness"], 2.0),
            particles: num(&v["particles"], 1.0),
        }
    })
}

impl Tracers {
    /// The tracer of a bullet type, none when it draws no streak and no head.
    pub fn of(&self, bullet: &str) -> Option<Tracer> {
        let b = bullet.to_lowercase();
        let t = self.bullets.iter().find(|(m, _)| b.contains(m.as_str())).map_or(self.default, |(_, t)| *t);
        (t.width > 0.0 || t.head > 0.0).then_some(t)
    }
}

#[derive(Clone, Copy, Default, Debug, ShaderType)]
pub struct TracerUniform {
    /// rgb: white-hot core colour, a: brightness
    pub core: Vec4,
    /// x: clock
    pub params: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct TracerGlow {
    #[uniform(100)]
    pub u: TracerUniform,
}

impl MaterialExtension for TracerGlow {
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

pub type TracerMaterial = ExtendedMaterial<StandardMaterial, TracerGlow>;

pub fn plugin(app: &mut App, look: &Look) {
    if look.tracers.is_some() {
        app.init_resource::<Streaks>().add_plugins(MaterialPlugin::<TracerMaterial>::default()).add_systems(Update, (thin_flight_fx, tracers));
    }
}

/// The path one shell has flown: (point, time) newest last, when the shell was gone, and smoke owed.
struct Streak {
    tracer: Tracer,
    points: VecDeque<(Vec3, f32)>,
    gone: Option<f32>,
    puffs: f32,
}

#[derive(Resource, Default)]
struct Streaks(HashMap<Entity, Streak>);

/// The one mesh all tracers are drawn into.
#[derive(Component)]
struct TracerMesh;

/// The retail flight effect riding on a new shell (the effect is the shell's child) keeps `particles` of its rate.
fn thin_flight_fx(look: Res<Look>, mut started: MessageReader<EffectStarted>, mut fx: Query<(&mut Fx, &ChildOf)>) {
    let Some(t) = &look.tracers else { return };
    if t.particles >= 1.0 {
        return;
    }
    for m in started.read() {
        // short effects can be gone already
        let Ok((mut f, parent)) = fx.get_mut(m.entity) else { continue };
        if m.shell == Some(parent.parent()) {
            f.rate_scale *= t.particles.max(0.0);
        }
    }
}

/// Records each shell's path, forgets streaks that have shrunk away, and rebuilds the mesh.
#[allow(clippy::too_many_arguments)]
fn tracers(
    mut commands: Commands,
    time: Res<Time>,
    look: Res<Look>,
    mut streaks: ResMut<Streaks>,
    // the shell's own Transform: a shell fired this frame (FixedUpdate) has no GlobalTransform yet until propagation
    shells: Query<(Entity, &Shell, &Transform)>,
    mut smoke: Option<ResMut<super::bursts::Particles>>,
    cams: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    target: Query<(&Mesh3d, &MeshMaterial3d<TracerMaterial>, &mut Visibility), With<TracerMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TracerMaterial>>,
) {
    let Some(t) = &look.tracers else { return };
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    for (e, shell, tf) in &shells {
        let Some(tracer) = t.of(shell.bullet()) else { continue };
        let s = streaks.0.entry(e).or_insert_with(|| Streak { tracer, points: VecDeque::new(), gone: None, puffs: 0.0 });
        record(&mut s.points, tf.translation, now);
        if let (Some(p), Some(smoke)) = (&tracer.smoke, smoke.as_deref_mut()) {
            s.puffs += p.count as f32 * dt;
            let back = s.points.len().checked_sub(2).map_or(Vec3::Y, |i| s.points[i].0 - tf.translation);
            while s.puffs >= 1.0 {
                s.puffs -= 1.0;
                smoke.throw(super::bursts::Kind::Smoke, &super::bursts::Part { count: 1, ..*p }, tf.translation, back, now, None);
            }
        }
    }
    streaks.0.retain(|e, s| {
        if s.gone.is_none() && !shells.contains(*e) {
            s.gone = Some(now);
        }
        expire(&mut s.points, now - s.tracer.seconds, s.gone.is_none());
        !s.points.is_empty()
    });
    let Some((_, cam)) = cams.iter().filter(|(c, _)| c.is_active).min_by_key(|(c, _)| c.order) else { return };
    let mut target = target;
    let Some((mesh, mat, mut vis)) = target.iter_mut().next() else {
        // the mesh goes with each battle (Scenario); a new one for the next
        let material = materials
            .add(TracerMaterial { base: super::trails::additive_base(None), extension: TracerGlow { u: TracerUniform { core: t.core.extend(t.brightness), params: Vec4::ZERO } } });
        commands.spawn((
            crate::Scenario,
            TracerMesh,
            Mesh3d(meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()))),
            MeshMaterial3d(material),
            Transform::default(),
            Visibility::Hidden,
            NoFrustumCulling,
            NotShadowCaster,
        ));
        return;
    };
    let mut b = Built::default();
    for s in streaks.0.values() {
        ribbon(&mut b, s, now, cam);
    }
    vis.set_if_neq(if b.idx.is_empty() { Visibility::Hidden } else { Visibility::Inherited });
    if let Some(mut m) = meshes.get_mut(&mesh.0) {
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, b.pos);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, b.nor);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, b.uv);
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, b.col);
        m.insert_indices(Indices::U32(b.idx));
    }
    if let Some(mut m) = materials.get_mut(&mat.0) {
        m.extension.u.params.x = now;
    }
}

/// Farther than a shell flies between two frames: the path starts over rather than draw a line across the map.
const JUMP: f32 = 60.0;

/// Adds the shell's position; a point closer than a hair to the last one just moves it.
fn record(points: &mut VecDeque<(Vec3, f32)>, at: Vec3, now: f32) {
    if points.back().is_some_and(|last| last.0.distance(at) > JUMP) {
        points.clear();
    }
    match points.back_mut() {
        Some(last) if last.0.distance_squared(at) < 1e-4 => *last = (at, now),
        _ => points.push_back((at, now)),
    }
}

/// Drops points older than `oldest`; a live shell keeps its newest point (its head) whatever its age.
fn expire(points: &mut VecDeque<(Vec3, f32)>, oldest: f32, alive: bool) {
    let keep = usize::from(alive);
    while points.len() > keep && points.front().is_some_and(|p| p.1 < oldest) {
        points.pop_front();
    }
}

#[derive(Default)]
struct Built {
    pos: Vec<[f32; 3]>,
    nor: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

/// One streak into the mesh. The streak: a strip facing the camera, u across (0..1), v its age (0 at the head, 1 the
/// oldest point drawn); colour rgb the tracer's, a how much is left of a gone shell's streak. The head (live shells only):
/// a camera-facing quad stretched along the flight, u 2..3 along it (3 at the front) and v 0..1 across, so the shader
/// tells the two apart by u.
fn ribbon(b: &mut Built, s: &Streak, now: f32, cam: &GlobalTransform) {
    let tr = &s.tracer;
    let eye = cam.translation();
    let left = s.gone.map_or(1.0, |g| (1.0 - (now - g) / tr.seconds).clamp(0.0, 1.0));
    let col = tr.color.extend(left).to_array();
    let pts: Vec<Vec3> = s.points.iter().rev().map(|p| p.0).collect();
    if tr.width > 0.0 && pts.len() >= 2 {
        let first = b.pos.len() as u32;
        for (i, p) in s.points.iter().rev().enumerate() {
            let along = (pts[i.saturating_sub(1)] - pts[(i + 1).min(pts.len() - 1)]).normalize_or(Vec3::Z);
            let to_cam = (eye - p.0).normalize_or(Vec3::Y);
            let side = along.cross(to_cam).normalize_or(Vec3::X) * (tr.width * 0.5);
            let v = ((now - p.1) / tr.seconds).clamp(0.0, 1.0);
            for (end, u) in [(1.0, 0.0), (-1.0, 1.0)] {
                b.pos.push((p.0 + side * end).to_array());
                b.nor.push(to_cam.to_array());
                b.uv.push([u, v]);
                b.col.push(col);
            }
        }
        for i in 0..(pts.len() - 1) as u32 {
            let a = first + 2 * i;
            b.idx.extend([a, a + 1, a + 2, a + 1, a + 3, a + 2]);
        }
    }
    if tr.head > 0.0 && s.gone.is_none() {
        let first = b.pos.len() as u32;
        let to_cam = (eye - pts[0]).normalize_or(Vec3::Y);
        let flight = pts.get(1).map_or(Vec3::ZERO, |&p| pts[0] - p);
        let along = if flight.cross(to_cam).length_squared() > 1e-8 { flight.normalize() } else { cam.right().as_vec3() };
        let side = along.cross(to_cam).normalize_or(cam.up().as_vec3()) * (tr.head * 0.5);
        let length = along * (tr.head * 0.5 * (1.0 + tr.stretch));
        // the front of the head at the shell, the rest trailing
        let centre = pts[0] - along * (tr.head * 0.5 * tr.stretch);
        let normal = to_cam.to_array();
        for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            b.pos.push((centre + length * x + side * y).to_array());
            b.nor.push(normal);
            b.uv.push([2.5 + 0.5 * x, 0.5 + 0.5 * y]);
            b.col.push(col);
        }
        b.idx.extend([first, first + 1, first + 2, first + 1, first + 3, first + 2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::FxLib;

    fn tracers() -> Tracers {
        parse(&serde_json::json!({
            "default": {"width": 0.5},
            "bullets": [{"match": "Lancer", "color": [0.4, 0.7, 1.0], "seconds": 0.1}, {"match": "StarMine", "width": 0, "head": 0}],
        }))
        .unwrap()
    }

    #[test]
    fn bullets_take_the_first_matching_rule_and_width_zero_draws_none() {
        let t = tracers();
        let lancer = t.of("Bullet_Lancer_Heavy").unwrap();
        assert_eq!((lancer.color, lancer.seconds, lancer.width), (Vec3::new(0.4, 0.7, 1.0), 0.1, 0.5), "unset keys come from the default");
        assert_eq!(t.of("Bullet_CannonBall_Med"), Some(t.default));
        assert!(t.of("Bullet_StarMine_Med").is_none());
        assert!(parse(&Value::Null).is_none());
        assert_eq!(parse(&serde_json::json!({})).unwrap().particles, 1.0, "retail flight effects stay by default");
    }

    #[test]
    fn a_streak_keeps_the_last_seconds_of_flight_and_shrinks_away_once_the_shell_is_gone() {
        let mut p = VecDeque::new();
        for i in 0..10 {
            record(&mut p, Vec3::new(i as f32, 0.0, 0.0), i as f32 * 0.1);
        }
        record(&mut p, Vec3::new(9.0, 0.0, 0.0), 1.0);
        assert_eq!(p.len(), 10, "a shell that has not moved does not add a point");
        let mut jump = p.clone();
        record(&mut jump, Vec3::new(500.0, 0.0, 0.0), 1.0);
        assert_eq!(jump.len(), 1, "a jump across the map starts the path over instead of drawing a line");
        expire(&mut p, 1.0 - 0.25, true);
        assert_eq!(p.iter().map(|q| q.0.x).collect::<Vec<_>>(), [8.0, 9.0], "0.25 s of flight");
        expire(&mut p, 5.0, true);
        assert_eq!(p.len(), 1, "a live shell keeps its head");
        expire(&mut p, 5.0, false);
        assert!(p.is_empty());
    }

    #[test]
    fn only_the_effect_riding_on_a_shell_is_thinned() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.init_resource::<Messages<EffectStarted>>();
        world.insert_resource(FxLib::with_names(&["Flight", "Muzzle"]));
        world.insert_resource(Look::parse(&serde_json::json!({"tracers": {"particles": 0.25}})));
        let shell = world.spawn_empty().id();
        let mount = world.spawn_empty().id();
        let started = world
            .run_system_once(move |mut commands: Commands, lib: Res<FxLib>| {
                let flight = crate::fx::spawn_with(
                    &mut commands,
                    &lib,
                    "Flight",
                    Some(shell),
                    Transform::default(),
                    Transform::default(),
                    false,
                    crate::fx::Origin { shell: Some(shell), ..default() },
                );
                let muzzle = crate::fx::spawn_with(
                    &mut commands,
                    &lib,
                    "Muzzle",
                    Some(mount),
                    Transform::default(),
                    Transform::default(),
                    false,
                    crate::fx::Origin { shell: Some(shell), ..default() },
                );
                (flight.unwrap(), muzzle.unwrap())
            })
            .unwrap();
        world.run_system_once(thin_flight_fx).unwrap();
        assert_eq!(world.get::<Fx>(started.0).unwrap().rate_scale, 0.25);
        assert_eq!(world.get::<Fx>(started.1).unwrap().rate_scale, 1.0, "a muzzle flash fires the shell but does not ride on it");
    }

    #[test]
    fn the_ribbon_faces_the_camera_with_its_head_marked_by_u() {
        let tracer = tracers().default;
        let s = Streak { tracer, points: [(Vec3::ZERO, 0.0), (Vec3::new(0.0, 0.0, -5.0), 0.1)].into(), gone: None, puffs: 0.0 };
        let cam = GlobalTransform::from(Transform::from_xyz(0.0, 50.0, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z));
        let mut b = Built::default();
        ribbon(&mut b, &s, 0.1, &cam);
        assert_eq!((b.pos.len(), b.idx.len()), (4 + 4, 6 + 6), "a two-point strip and the head quad");
        assert!(b.idx.iter().all(|&i| (i as usize) < b.pos.len()));
        assert!(b.pos[..4].iter().all(|p| p[1].abs() < 1e-4), "seen from above the strip lies flat");
        assert!((b.pos[0][0] - b.pos[1][0]).abs() > 0.49, "and spans its width across the flight");
        assert_eq!((b.uv[0][1], b.uv[2][1]), (0.0, 0.1 / tracer.seconds), "v: 0 at the head, then the age over `seconds`");
        assert!(b.uv[4..].iter().all(|uv| uv[0] >= 2.0), "head vertices have u >= 2");
        let gone = Streak { gone: Some(0.1), ..s };
        let mut b = Built::default();
        ribbon(&mut b, &gone, 0.2, &cam);
        assert_eq!(b.pos.len(), 4, "a gone shell has no head");
        assert!((b.col[0][3] - (1.0 - 0.1 / tracer.seconds)).abs() < 1e-4, "and its streak fades");
    }
}
