//! Guns, shells, damage sections and the ship-class library. Data comes from the WOT/MDB/MSB files via
//! tools/export_combat.py. PROVISIONAL items are my interpretations, not measured original behaviour.

use bevy::prelude::*;

use crate::audio::Sfx;
pub use crate::data::*;
use crate::fx::{self, FxLib};
use crate::{PlayerShip, Ship, Side, assets_dir};

mod asteroid;
mod damage;
mod lob;
mod status;

pub use asteroid::*;
pub use damage::*;
pub use lob::{Detonate, Shoot, Strike};
pub use status::*;

/// Order matters: "Reinforced Wood" must match Reinforced before Wood.
pub const MATERIALS: [&str; 6] = ["Reinforced", "Wood", "Iron", "Cloth", "Stone", "Dragon"];

#[derive(Component)]
pub struct Health {
    pub sections: Vec<f32>,
    pub core: f32,
    pub destroyed: bool,
}
/// A base with gun towers (island): `class` is its `Base_*` entry in the library (mounts and sections); `Health` holds the section HP.
#[derive(Component)]
pub struct ArmedBase {
    pub class: usize,
}
/// What the rules' 'Vital Section has X damage' reads for a base (PROVISIONAL: mean damage of its vitalToMission sections).
pub fn base_mission_damage(class: &Class, hp: &Health) -> f32 {
    let v: Vec<f32> = class.sections.iter().zip(&hp.sections).filter(|(c, _)| c.mission).map(|(c, h)| 1.0 - h / c.hp.max(1.0)).collect();
    if v.is_empty() { 0.0 } else { (v.iter().sum::<f32>() / v.len() as f32).clamp(0.0, 1.0) }
}
#[derive(Clone)]
struct GunState {
    gun: String,
    root: Entity,
    cooldown: f32,
    burst_left: u32,
    burst_timer: f32,
    base: Entity,
    barrel: Entity,
    muzzles: Vec<Entity>,
    next_muzzle: usize,
    yaw: f32,
    pitch: f32,
    aligned: bool,
    in_range: bool, // a target is within range and the mount's arc (weapon bar: green outline)
    dev: f32,       // this burst's deviation angle, drawn once when the burst starts (exe: once per burst)
}
/// Marks the entities of a gun's visual hierarchy (root, base, barrel, muzzle points).
#[derive(Component)]
struct GunPart;
#[derive(Component)]
pub struct GunStates(Vec<GunState>);
impl GunStates {
    pub fn names(&self) -> Vec<String> {
        self.0.iter().map(|s| s.gun.clone()).collect()
    }

    pub fn gun(&self, index: usize) -> &str {
        &self.0[index].gun
    }

    pub fn replace(&mut self, commands: &mut Commands, assets: &AssetServer, lib: &Library, ship: Entity, mount: &crate::data::Mount, index: usize, gun: &str) {
        commands.entity(self.0[index].root).try_despawn();
        self.0[index] = spawn_gun(commands, assets, lib, ship, mount, gun);
    }

    /// A refit re-creates every mount: loaded, no burst in progress.
    pub fn rearm(&mut self) {
        for g in &mut self.0 {
            (g.cooldown, g.burst_left, g.burst_timer) = (0.0, 0, 0.0);
        }
    }
}
#[derive(Component)]
pub struct Shell {
    vel: Vec3,
    age: f32,
    lifetime: f32,
    /// Downward acceleration (units/s^2): the firing gun's `GunDef::shell_gravity`, 0 for straight-flying shells.
    gravity: f32,
    bullet: String,
    side: Side,
    shooter: Entity,
    target: Option<Entity>,
    angular_velocity: f32,
    payload: Payload,
    /// Lob and mine shells: the point they were fired at (a lob bursts descending through its height; a mine settles there).
    aim: Option<Vec3>,
    /// Fired by a point-defence gun at a torpedo or mine.
    pd: bool,
    /// Remaining projectile hitpoints (`BulletDef::hitpoints`); point-defence bolts wear torpedoes and mines down.
    hp: f32,
    /// A warning shot ('Set Group/Unit Warning Shot Mode'): no damage.
    harmless: bool,
}
/// A shell struck `victim` (counted even when the victim is invulnerable); fed to the mission rules.
#[derive(Message)]
pub struct Hit {
    pub shooter: Entity,
    pub victim: Entity,
}
/// An asteroid crossed the death threshold; breakup and rules consume independent readers.
#[derive(Message)]
pub struct Destroyed {
    pub by: Entity,
    pub victim: Entity,
}
/// Segment a->b against a sphere; entry fraction in [0, 1].
pub fn seg_sphere(a: Vec3, b: Vec3, c: Vec3, r: f32) -> Option<f32> {
    let (d, f) = (b - a, a - c);
    let (qa, qb, qc) = (d.dot(d), 2.0 * f.dot(d), f.dot(f) - r * r);
    if qc <= 0.0 {
        return Some(0.0);
    }
    let disc = qb * qb - 4.0 * qa * qc;
    if qa < 1e-9 || disc < 0.0 {
        return None;
    }
    let t = (-qb - disc.sqrt()) / (2.0 * qa);
    (0.0..=1.0).contains(&t).then_some(t)
}
/// A gun burst started at `target` (None never happens for ships; kept for asteroids/scenery later).
#[derive(Message)]
pub struct Fired {
    pub shooter: Entity,
    pub target: Option<Entity>,
    /// The burst's bullet type (crew callouts: torpedoes away, mortar fired).
    pub bullet: String,
}
#[derive(Component)]
struct Flash(f32);
/// Exhaust effect on a ship; its rate follows the ship's speed (PROVISIONAL: 30% idle up to 100% at top speed).
#[derive(Component)]
pub struct Engine;
/// Marks entities that a restart must despawn.
#[derive(Component)]
pub struct Transient;
/// The player's target and where on it the player clicked (ship-local); the guns aim at that spot, as in the original.
#[derive(Resource, Default)]
pub struct Target(pub Option<Entity>, pub Option<Vec3>, pub GroundFire);
#[derive(Resource)]
pub struct Rng(u64);
#[derive(Resource)]
struct Look {
    mesh: Handle<Mesh>,
    shell: Handle<StandardMaterial>,
    enemy_shell: Handle<StandardMaterial>,
    flash: Handle<StandardMaterial>,
}

impl Rng {
    #[cfg(test)]
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn direction(&mut self) -> Vec3 {
        Vec3::new(self.f() * 2.0 - 1.0, self.f() * 2.0 - 1.0, self.f() * 2.0 - 1.0).try_normalize().unwrap_or(Vec3::X)
    }
    pub fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

pub fn plugin(app: &mut App) {
    app.insert_resource(Library::load(&assets_dir()))
        .init_resource::<Target>()
        .add_message::<Hit>()
        .add_message::<Fired>()
        .add_message::<Destroyed>()
        .add_message::<Detonate>()
        .add_message::<Shoot>()
        .add_message::<Strike>()
        .add_message::<AsteroidSetup>()
        .init_resource::<crate::data::AsteroidDefs>()
        .insert_resource(Rng(0x9E3779B97F4A7C15))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                (fire, base_fire, lob::shoot, shells, lob::detonate, lob::blasts, lob::grav_wells, lob::ram_damage, lob::strikes).chain(),
                (asteroid_deaths, fires, crew_repair, deaths, flashes, draw, engine_rate, damage_fx, swap_meshes).chain(),
            )
                .chain()
                .after(crate::sail),
        )
        .add_systems(
            Update,
            (asteroid_motion, crate::collision::update_asteroids, asteroid_demo).chain().after(crate::input).before(crate::sail).run_if(in_state(crate::menu::Screen::Battle)),
        );
}

impl Health {
    pub fn new(c: &Class) -> Self {
        Health { sections: c.sections.iter().map(|s| s.hp).collect(), core: c.core_hp, destroyed: false }
    }
    /// Multiplies every section and the core by `f` (1 keeps them, 0.5 halves them).
    pub fn scale(&mut self, f: f32) {
        self.sections.iter_mut().for_each(|x| *x *= f);
        self.core *= f;
    }
}
/// Spawns the visual hierarchy of every mount as children of `ship` and returns the gun state (PROVISIONAL: guns start loaded).
pub fn spawn_guns(commands: &mut Commands, assets: &AssetServer, lib: &Library, ship: Entity, class: usize) -> GunStates {
    GunStates(lib.classes[class].mounts.iter().map(|m| spawn_gun(commands, assets, lib, ship, m, &m.gun)).collect())
}

fn spawn_gun(commands: &mut Commands, assets: &AssetServer, lib: &Library, ship: Entity, m: &crate::data::Mount, gun: &str) -> GunState {
    let scene = |glb: &str| WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(crate::data::model(glb))));
    let g = &lib.guns[gun];
    let root = commands.spawn((GunPart, Transform { translation: m.pos, rotation: m.rot, ..default() }, Visibility::default(), ChildOf(ship))).id();
    let base = commands.spawn((GunPart, Transform::default(), Visibility::default(), ChildOf(root))).id();
    if let Some(b) = &g.base {
        commands.entity(base).insert(scene(b));
    }
    let barrel = commands.spawn((GunPart, Transform::from_translation(g.pivot), Visibility::default(), ChildOf(base))).id();
    if let Some(b) = &g.barrel {
        commands.entity(barrel).insert(scene(b));
    }
    let muzzles = g.muzzles.iter().map(|p| commands.spawn((GunPart, Transform::from_translation(*p), Visibility::default(), ChildOf(barrel))).id()).collect();
    GunState {
        gun: gun.into(),
        root,
        cooldown: 0.0,
        burst_left: 0,
        burst_timer: 0.0,
        base,
        barrel,
        muzzles,
        next_muzzle: 0,
        yaw: 0.0,
        pitch: 0.0,
        aligned: false,
        in_range: false,
        dev: 0.0,
    }
}

pub fn spawn_engines(commands: &mut Commands, fxl: &FxLib, lib: &Library, ship: Entity, class: usize, world: Transform) {
    for (pos, rot, name) in &lib.classes[class].engines {
        let local = Transform { translation: *pos, rotation: *rot, ..default() };
        if let Some(e) = fx::spawn(commands, fxl, name, Some(ship), local, world * local, false) {
            commands.entity(e).insert(Engine);
        }
    }
}

fn engine_rate(ships: Query<(&Ship, &Health)>, mut q: Query<(&mut fx::Fx, &ChildOf), With<Engine>>) {
    for (mut f, parent) in &mut q {
        if let Ok((ship, h)) = ships.get(parent.parent()) {
            // Thrust follows the engine order: dark at All Stop, brighter with each step up (PROVISIONAL curve).
            let throttle = crate::ORDERS[ship.order].1;
            f.emitting = !h.destroyed && throttle > 0.0;
            f.size_scale = 2.6; // PROVISIONAL: data size 1.6 gives a thread; the reference jet is about hull-wide
            f.rate_scale = 0.75 * (0.25 + 0.75 * throttle.min(1.25));
        }
    }
}

/// Indirect weapons (mortars, mines, gravity charges) never auto-target: the player arms the bank in the weapon panel and
/// clicks a ground point for each shot.
#[derive(Default)]
pub struct GroundFire {
    pub armed: Option<usize>,
    pub shot: Option<(usize, Vec3)>,
    /// Panic button: every gun picks its own nearest enemy it can engage (exe: PanicFireCommand) until the player retargets.
    pub panic: bool,
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>) {
    let lit = |c: Color, mats: &mut Assets<StandardMaterial>| mats.add(StandardMaterial { base_color: c, unlit: true, ..default() });
    commands.insert_resource(Look {
        mesh: meshes.add(Sphere::new(0.35)),
        shell: lit(Color::srgb(1.0, 0.8, 0.3), &mut mats),
        enemy_shell: lit(Color::srgb(1.0, 0.25, 0.25), &mut mats),
        flash: lit(Color::srgb(1.0, 0.45, 0.1), &mut mats),
    });
}

/// Nearest ray hit against a class's triangles in the ship's local frame; used for click-targeting.
pub fn ray_hit(c: &Class, tf: &Transform, ray: Ray3d) -> Option<f32> {
    let inv = tf.to_matrix().inverse();
    let (o, d) = (inv.transform_point3(ray.origin), inv.transform_vector3(*ray.direction));
    c.sections.iter().filter_map(|s| seg_mesh(o, o + d * 1000.0, s)).map(|t| t * 1000.0).reduce(f32::min)
}

/// Only live mission sections (gun towers) are attackable; the island terrain is not a target.
pub fn base_ray_hit(c: &Class, hp: &Health, tf: &Transform, ray: Ray3d) -> Option<f32> {
    let inv = tf.to_matrix().inverse();
    let (o, d) = (inv.transform_point3(ray.origin), inv.transform_vector3(*ray.direction));
    c.sections.iter().zip(&hp.sections).filter(|(s, h)| s.mission && **h > 0.0).filter_map(|(s, _)| seg_mesh(o, o + d * 1000.0, s)).map(|t| t * 1000.0).reduce(f32::min)
}

fn base_aim_point(c: &Class, hp: &Health, clicked: Option<Vec3>) -> Option<Vec3> {
    c.sections
        .iter()
        .zip(&hp.sections)
        .filter(|(s, h)| s.mission && **h > 0.0)
        .map(|(s, _)| (s.min + s.max) * 0.5)
        .min_by(|a, b| a.distance_squared(clicked.unwrap_or(c.centre)).total_cmp(&b.distance_squared(clicked.unwrap_or(c.centre))))
}

// PROVISIONAL: omit destroyed non-vital geometry until damaged-mesh collision is exported.
// Destroyed vital hulls must remain hittable so subsequent hits can damage the core.
pub fn ship_segment_hit(c: &Class, hp: &Health, a: Vec3, b: Vec3) -> Option<(f32, usize)> {
    c.sections.iter().enumerate().filter(|(k, s)| s.vital || hp.sections[*k] > 0.0).filter_map(|(k, s)| seg_mesh(a, b, s).map(|t| (t, k))).min_by(|x, y| x.0.total_cmp(&y.0))
}

fn ship_aim_point(c: &Class, hp: &Health, clicked: Option<Vec3>) -> Vec3 {
    let Some(point) = clicked else { return c.centre };
    let contains = |s: &Section| point.cmpge(s.min - Vec3::splat(0.01)).all() && point.cmple(s.max + Vec3::splat(0.01)).all();
    // PROVISIONAL: use bounds to recognize a lost sail aim point; preserve overlapping hull targets.
    if !c.sections.iter().any(|s| s.vital && contains(s)) && c.sections.iter().zip(&hp.sections).any(|(s, h)| !s.vital && *h <= 0.0 && contains(s)) { c.centre } else { point }
}

/// Slab test of segment a->b against a box; returns the entry fraction in [0, 1].
fn seg_aabb(a: Vec3, b: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let (d, mut t0, mut t1) = (b - a, 0.0f32, 1.0f32);
    for i in 0..3 {
        if d[i].abs() < 1e-9 {
            if a[i] < min[i] || a[i] > max[i] {
                return None;
            }
        } else {
            let (mut u, mut v) = ((min[i] - a[i]) / d[i], (max[i] - a[i]) / d[i]);
            if u > v {
                std::mem::swap(&mut u, &mut v);
            }
            t0 = t0.max(u);
            t1 = t1.min(v);
            if t0 > t1 {
                return None;
            }
        }
    }
    Some(t0)
}

/// Segment vs the section's triangles (box first); earliest hit fraction in [0, 1]. Triangles are double-sided.
fn seg_mesh(a: Vec3, b: Vec3, s: &Section) -> Option<f32> {
    seg_aabb(a, b, s.min, s.max)?;
    let d = b - a;
    s.tris
        .iter()
        .filter_map(|[p, q, r]| {
            let (e1, e2) = (*q - *p, *r - *p);
            let h = d.cross(e2);
            let det = e1.dot(h);
            if det.abs() < 1e-9 {
                return None;
            }
            let (f, sv) = (1.0 / det, a - *p);
            let u = f * sv.dot(h);
            if !(0.0..=1.0).contains(&u) {
                return None;
            }
            let qv = sv.cross(e1);
            let v = f * d.dot(qv);
            if v < 0.0 || u + v > 1.0 {
                return None;
            }
            let t = f * e2.dot(qv);
            (0.0..=1.0).contains(&t).then_some(t)
        })
        .reduce(f32::min)
}

// Straight projectile intercept; a slow torpedo needs more than the generic one-step lead.
fn intercept_time(d: Vec3, relative_velocity: Vec3, speed: f32) -> Option<f32> {
    let a = relative_velocity.length_squared() - speed * speed;
    let b = 2.0 * d.dot(relative_velocity);
    let c = d.length_squared();
    if c < 0.000001 {
        return Some(0.0);
    }
    if a.abs() < 0.000001 {
        let t = -c / b;
        return (t.is_finite() && t >= 0.0).then_some(t);
    }
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return None;
    }
    [(-b - discriminant.sqrt()) / (2.0 * a), (-b + discriminant.sqrt()) / (2.0 * a)].into_iter().filter(|t| t.is_finite() && *t >= 0.0).reduce(f32::min)
}

/// Aim point with lead. Shells and torpedoes leave the gun at their own speed in the world frame (retail: launch speed equals
/// MuzzleSpeed whatever the firing ship's motion), so the lead uses the target's velocity only.
fn aim_offset(d: Vec3, target_velocity: Vec3, speed: f32, torpedo: bool) -> Vec3 {
    if torpedo {
        let d = d.with_y(0.0);
        let velocity = target_velocity.with_y(0.0);
        // No reachable straight intercept: aim at the target, not an arbitrarily distant extrapolation.
        return intercept_time(d, velocity, speed).map_or(d, |t| d + velocity * t);
    }
    d + target_velocity * (d.length() / speed)
}

/// Launch velocity: MuzzleSpeed along `dir` in the world frame; the firing ship's velocity is not added (measured on retail
/// cannon, carronade and gattling shells). Torpedoes launch level.
fn launch_velocity(dir: Vec3, speed: f32, torpedo: bool) -> Vec3 {
    if torpedo { dir.with_y(0.0).normalize_or_zero() * speed } else { dir * speed }
}

/// Launch direction for speed `s` to reach offset `d` under gravity `g`: the low arc, or straight when `g` is 0.
fn aim(d: Vec3, s: f32, g: f32) -> Vec3 {
    if g <= 0.0 {
        return d.normalize();
    }
    let (r, h) = (Vec2::new(d.x, d.z).length(), d.y);
    let disc = s.powi(4) - g * (g * r * r + 2.0 * h * s * s);
    let theta = if disc < 0.0 { std::f32::consts::FRAC_PI_4 } else { ((s * s - disc.sqrt()) / (g * r)).atan() };
    let flat = Vec3::new(d.x, 0.0, d.z).normalize_or_zero();
    flat * theta.cos() + Vec3::Y * theta.sin()
}

/// Launch velocity of a lobbing gun (fixed `angle` above the horizon) that lands on offset `d`: the speed is solved for the
/// angle, as measured on retail star mortar shells (speed ~ sqrt(g * range) at ~45 degrees). None when the point is above
/// the arc. PROVISIONAL: retail elevations scattered 44-50 degrees; whether MuzzleSpeed caps the speed is not observed.
fn lob_velocity(d: Vec3, angle: f32, g: f32) -> Option<Vec3> {
    let r = Vec2::new(d.x, d.z).length();
    let rise = r * angle.tan() - d.y;
    if r < 0.5 || rise <= 0.1 {
        return None;
    }
    let speed = (g * r * r / (2.0 * angle.cos().powi(2) * rise)).sqrt();
    let flat = Vec3::new(d.x, 0.0, d.z) / r;
    Some((flat * angle.cos() + Vec3::Y * angle.sin()) * speed)
}

/// Panic fire: the nearest visible enemy ship within this gun's maximum range and mount arc.
/// Exe: each gun finds its own target and fires when possible. PROVISIONAL: nearest-first choice, no gun-class
/// category filter (exe skips guns by three class flags, not decoded), ships only.
fn panic_target(snap: &[(Entity, Side, Vec3, bool, bool, Vec3)], tf: &Transform, m: &crate::data::Mount, g: &crate::data::GunDef) -> Option<(Vec3, Vec3, Entity)> {
    let origin = tf.transform_point(m.pos);
    let inv = (tf.rotation * m.rot).inverse();
    snap.iter()
        .filter(|s| s.1 == Side::Enemy && s.3)
        .filter(|s| {
            let d = s.2 - origin;
            let l = inv * d;
            Vec2::new(d.x, d.z).length() <= g.range[2] && (-l.x).atan2(-l.z).abs() <= m.arc / 2.0
        })
        .min_by(|a, b| a.2.distance(origin).total_cmp(&b.2.distance(origin)))
        .map(|s| (s.2, s.5, s.0))
}

/// Aims every mount at the chosen target (base yaw, barrel pitch at the gun's rotation speeds) and fires aligned, loaded guns
/// from the muzzle points.
fn fire(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    fxl: Res<FxLib>,
    mut target: ResMut<Target>,
    look: Res<Look>,
    mut rng: ResMut<Rng>,
    mut sfx: MessageWriter<Sfx>,
    mut fired: MessageWriter<Fired>,
    objects: Query<
        (Entity, &Transform, Option<&crate::environment::SpaceAnimal>, Option<(&ArmedBase, &Health, &crate::campaign::MissionBase)>),
        (Or<(With<Asteroid>, With<crate::environment::SpaceAnimal>, With<ArmedBase>)>, Without<GunPart>),
    >,
    campaign: Option<Res<crate::campaign::Mission>>,
    mut shooters: Query<(
        Entity,
        &Transform,
        &Ship,
        &mut GunStates,
        &Health,
        Option<&crate::campaign::MissionShip>,
        Has<PlayerShip>,
        Option<&crate::ship::FleetPilot>,
        Option<&crate::cloak::Cloak>,
        Option<&crate::crew_arms::CrewPosts>,
    )>,
    others: Query<(Entity, &Transform, &Ship, &Health, Has<PlayerShip>, Option<&crate::campaign::MissionShip>, Option<&crate::cloak::Cloak>)>,
    mut parts: Query<&mut Transform, (With<GunPart>, Without<Ship>)>,
    (globals, threats, points, sensors): (
        Query<&GlobalTransform, With<GunPart>>,
        Query<(Entity, &Transform, &Shell), (Without<GunPart>, Without<Ship>)>,
        Option<Res<crate::campaign::CampaignPoints>>,
        Res<crate::environment::Sensors>,
    ),
) {
    // The human fleet's gunners in a campaign: the roster's best gunner (crew_arms.rs); everyone else the plain-AI factor.
    let crewed = crate::crew_arms::accuracy(points.as_deref());
    let dt = time.delta_secs();
    // Point-defence targets: torpedoes and mines in flight, with their side.
    let incoming: Vec<(Entity, Vec3, Vec3, Side)> = threats
        .iter()
        .filter(|(_, _, s)| s.bullet.starts_with(TORPEDO_BULLET) || matches!(s.payload, Payload::Mine { .. }))
        .map(|(e, t, s)| (e, t.translation, s.vel, s.side))
        .collect();
    // 'NIS Toggle NIS mode Gun Accuracy': scripted broadsides fly true.
    let nis_aim = campaign.as_deref().is_some_and(|m| m.script.nis_accuracy);
    // Script visibility stays separate from observer-dependent detection.
    let snap: Vec<(Entity, Side, Vec3, bool, bool, Vec3)> = others
        .iter()
        .map(|(e, t, s, h, p, m, _)| {
            (
                e,
                s.side,
                t.transform_point(ship_aim_point(&lib.classes[s.class], h, if Some(e) == target.0 { target.1 } else { None })),
                !h.destroyed && m.is_none_or(|m| m.visible),
                p,
                t.rotation * Vec3::NEG_Z * s.speed,
            )
        })
        .collect();
    let rocks: Vec<(Entity, Vec3, Vec3)> = objects
        .iter()
        .filter(|(_, _, a, base)| {
            a.is_none_or(|a| a.visible && campaign.as_deref().is_some_and(|m| m.script.hostile(m.script.player(), &a.owner)))
                && base.is_none_or(|(_, _, b)| campaign.as_deref().is_some_and(|m| m.script.hostile(m.script.player(), &b.owner)))
        })
        .filter_map(|(e, t, a, base)| {
            let position = match base {
                Some((b, h, _)) => t.transform_point(base_aim_point(&lib.classes[b.class], h, (target.0 == Some(e)).then_some(target.1).flatten())?),
                None => t.translation,
            };
            Some((e, position, a.map_or(Vec3::ZERO, |a| a.velocity)))
        })
        .collect();
    for (me, tf, ship, mut states, health, mission, primary, pilot, cloak, posts) in &mut shooters {
        if health.destroyed {
            continue;
        }
        let snap: Vec<_> = snap.iter().map(|s| (s.0, s.1, s.2, s.3 && sensors.in_sight(me, s.0, campaign.as_deref()), s.4, s.5)).collect();
        let snap = &snap[..];
        let cloaked_now = cloak.as_ref().is_some_and(|c| c.level > 0.0);
        let class = &lib.classes[ship.class];
        // Player: the clicked target if still alive, else the nearest living enemy. Enemies: the player ship.
        // Mission ships other than the player's fire at the target the rules' AI chose (rules::mission_ai).
        let tp = if pilot.is_some_and(|p| p.hold_fire) || mission.is_some_and(|m| m.hold_fire) {
            None
        } else if let Some(t) = pilot.filter(|_| !primary).and_then(|p| p.target) {
            snap.iter().find(|s| s.0 == t && s.1 != ship.side && s.3).map(|s| (s.2, s.5, s.0)).or_else(|| rocks.iter().find(|r| r.0 == t).map(|r| (r.1, r.2, r.0)))
        } else if let (Some(m), false) = (mission, primary) {
            m.target.and_then(|t| snap.iter().find(|s| s.0 == t && s.3).map(|s| (s.2, s.5, s.0)).or_else(|| rocks.iter().find(|r| r.0 == t).map(|r| (r.1, r.2, r.0))))
        } else if let (Side::Player, Some(r)) = (ship.side, target.0.filter(|_| primary).and_then(|t| rocks.iter().find(|r| r.0 == t))) {
            Some((r.1, r.2, r.0)) // selected space object (asteroid or whale)
        } else {
            match ship.side {
                Side::Player => snap
                    .iter()
                    .find(|s| primary && Some(s.0) == target.0 && s.1 == Side::Enemy && s.3)
                    .or_else(|| snap.iter().filter(|s| s.1 == Side::Enemy && s.3).min_by(|a, b| a.2.distance(tf.translation).total_cmp(&b.2.distance(tf.translation)))),
                Side::Enemy => snap.iter().find(|s| s.4 && s.3),
            }
            .map(|s| (s.2, s.5, s.0))
        };
        let tp = tp.filter(|(_, _, e)| sensors.in_sight(me, *e, campaign.as_deref()));
        let pending = if primary { target.2.shot } else { None };
        let (mut shot_fired, mut shot_engaged) = (false, false);
        for (m, st) in class.mounts.iter().zip(states.0.iter_mut()) {
            // The player's indirect guns only fire at a clicked ground point.
            let on_shot = pending.is_some_and(|s| s.0 == m.bank);
            let g = &lib.guns[&st.gun];
            let tp = match (primary && is_indirect(&st.gun), pending) {
                // Point defence: the nearest hostile torpedo or mine within its maximum range (exe: PD guns never fire at ships).
                _ if g.point_defense => incoming
                    .iter()
                    .filter(|t| t.3 != ship.side && t.1.distance(tf.transform_point(m.pos)) <= g.range[2])
                    .min_by(|a, b| a.1.distance(tf.translation).total_cmp(&b.1.distance(tf.translation)))
                    .map(|t| (t.1, t.2, t.0)),
                (false, _) if primary && target.2.panic => panic_target(snap, tf, m, g).or(tp),
                (false, _) => tp,
                (true, Some((b, p))) if b == m.bank => Some((p, Vec3::ZERO, Entity::PLACEHOLDER)),
                _ => None,
            };
            // PROVISIONAL (owner retail recollection): torpedoes cannot target base towers.
            let tp = tp.filter(|t| g.bullet != TORPEDO_BULLET || !objects.get(t.2).is_ok_and(|o| o.3.is_some()));
            st.cooldown -= dt;
            st.burst_timer -= dt;
            let mount_rot = tf.rotation * m.rot;
            let origin = tf.transform_point(m.pos);
            let dead = m.parent.is_some_and(|p| health.sections[p] <= 0.0);
            // Turret AI after the exe (docs/re/gunnery.md): the base always turns toward
            // the target at HorizontalRotationSpeed, clamped to the mount's half-arc, whatever the range; the barrel only elevates
            // once the base is within 1 degree and the target is within range; it may fire when the target is in range and arc.
            // PROVISIONAL: with no target (or a dead mount section) the gun returns to rest; the exe's idle behaviour is not decoded.
            const TOL: f32 = std::f32::consts::PI / 180.0;
            let half = m.arc / 2.0; // already converted to radians when loading the mount
            let mut want_yaw = 0.0f32;
            let mut want_pitch = Some(0.0f32);
            let mut engaged = None;
            // Point-defense guns only engage incoming torpedoes/mines (not in the sim yet), so they stay idle against ships; harpoons are
            // grapples, fired by the Grapple order (boarding.rs) and never at will. PROVISIONAL: the exe's grapple-gun exclusion is not decoded.
            if let (false, false, Some((tp, tv, _))) = (dead, st.gun.contains("Harpoon"), tp) {
                // Lead like the exe: aim point += target velocity * (distance / muzzle speed), one step, no gravity term.
                // Torpedo lead remains straight interception, not the native homing controller.
                let d = aim_offset(tp - origin, tv, g.muzzle, g.bullet == TORPEDO_BULLET);
                let dir = match g.lob.and_then(|a| lob_velocity(d, a, g.shell_gravity())) {
                    Some(v) => v.normalize(),
                    None => aim(d, g.muzzle, g.shell_gravity()),
                };
                let l = mount_rot.inverse() * dir;
                let yaw = (-l.x).atan2(-l.z);
                want_yaw = yaw.clamp(-half, half);
                let in_range = Vec2::new(d.x, d.z).length() <= g.range[2];
                let in_arc = yaw.abs() <= half;
                want_pitch = None; // hold elevation until the base is on target
                if (st.yaw - want_yaw).abs() < TOL && in_range {
                    want_pitch = Some(l.y.clamp(-1.0, 1.0).asin().clamp(g.vlim[0], g.vlim[1]));
                }
                if in_range && in_arc {
                    engaged = Some((dir, d.length()));
                }
            }
            let dy = (want_yaw - st.yaw).clamp(-g.hspeed * dt, g.hspeed * dt);
            st.yaw = (st.yaw + dy).clamp(-half.max(0.0), half);
            if let Some(p) = want_pitch {
                st.pitch += (p - st.pitch).clamp(-g.vspeed * dt, g.vspeed * dt);
            }
            st.in_range = engaged.is_some();
            shot_engaged |= on_shot && engaged.is_some();
            st.aligned = engaged.is_some() && (st.yaw - want_yaw).abs() < TOL && want_pitch.is_some_and(|p| (st.pitch - p).abs() < TOL);
            if let Ok(mut t) = parts.get_mut(st.base) {
                t.rotation = Quat::from_rotation_y(st.yaw);
            }
            if let Ok(mut t) = parts.get_mut(st.barrel) {
                t.rotation = Quat::from_rotation_x(st.pitch);
            }
            let Some((dir0, dist)) = engaged else {
                st.burst_left = 0;
                continue;
            };
            if dead {
                st.burst_left = 0;
                continue;
            }
            // Guns that cannot fire while cloaked hold, unless the rules let this AI drop its cloak to shoot.
            if cloaked_now && !g.cloak_fire {
                if cloak.is_some_and(|c| c.ai_override && c.on && !primary) && st.cooldown <= 0.0 {
                    commands.queue(move |w: &mut World| {
                        if let Some(mut c) = w.get_mut::<crate::cloak::Cloak>(me) {
                            c.on = false;
                        }
                    });
                }
                st.burst_left = 0;
                continue;
            }
            if st.burst_left == 0 && st.cooldown <= 0.0 && st.aligned {
                st.burst_left = g.burst;
                shot_fired |= on_shot;
                if !g.point_defense {
                    let t = tp.map(|t| t.2).filter(|t| *t != Entity::PLACEHOLDER);
                    fired.write(Fired { shooter: me, target: t, bullet: g.bullet.clone() });
                }
                st.cooldown = g.burst_cycle();
                st.burst_timer = 0.0;
                // exe: band deviation * 0.5, uniform in +-that, times an accuracy factor. 0.5 is the plain-AI factor;
                // crewed ships use 1 - 0.8 * gunnery skill (the campaign roster's best gunner for the human fleet).
                let band = if dist <= g.range[0] {
                    g.dev[0]
                } else if dist <= g.range[1] {
                    g.dev[1]
                } else {
                    g.dev[2]
                };
                let human = campaign.as_deref().is_some_and(|c| mission.is_some_and(|m| m.owner == c.script.player()));
                let factor = if human { posts.map_or(crewed, |p| p.skill(0, "gunnery").map_or(0.5, |g| 1.0 - 0.8 * g)) } else { 0.5 };
                st.dev = if nis_aim { 0.0 } else { (rng.f() * 2.0 - 1.0) * band * 0.5 * factor };
            }
            if st.burst_left > 0 && st.burst_timer <= 0.0 {
                st.burst_left -= 1;
                st.burst_timer = g.burst_reload;
                // exe: the burst's angle applied about two muzzle axes, each with a random sign.
                let sign = |r: f32| if r < 0.5 { -1.0 } else { 1.0 };
                let right = dir0.cross(Vec3::Y).normalize_or(Vec3::X);
                let turn = Quat::from_axis_angle(Vec3::Y, sign(rng.f()) * st.dev) * Quat::from_axis_angle(right, sign(rng.f()) * st.dev);
                let dir = turn * dir0;
                let mz = st.muzzles[st.next_muzzle % st.muzzles.len()];
                st.next_muzzle += 1;
                let side = ship.side;
                let goal = tp.map(|t| t.0);
                let shot = Launch {
                    vel: g.lob.and_then(|a| lob_velocity(goal? - origin, a, g.shell_gravity())).map(|v| turn * v),
                    aim: goal.filter(|_| is_indirect(&st.gun)),
                    pd: g.point_defense,
                    harmless: mission.is_some_and(|m| m.warning_shots),
                };
                launch_shell(&mut commands, &assets, &lib, &fxl, &look, &mut sfx, g, mz, globals.get(mz).ok().copied(), origin, dir, side, me, tp.map(|t| t.2), shot);
            }
        }
        // One click, one shot: drop the point once fired, or when the bank cannot reach it.
        if pending.is_some() && (shot_fired || !shot_engaged) {
            target.2.shot = None;
        }
    }
}

/// Per-shot extras of `launch_shell`: a solved lob velocity, the indirect aim point, point-defence and warning-shot flags.
#[derive(Default)]
struct Launch {
    vel: Option<Vec3>,
    aim: Option<Vec3>,
    pd: bool,
    harmless: bool,
}

/// Spawns one shell of gun `g` from muzzle `mz` (its fire sound, muzzle flash and bullet effect included).
fn launch_shell(
    commands: &mut Commands,
    assets: &AssetServer,
    lib: &Library,
    fxl: &FxLib,
    look: &Look,
    sfx: &mut MessageWriter<Sfx>,
    g: &GunDef,
    mz: Entity,
    mgt: Option<GlobalTransform>,
    origin: Vec3,
    dir: Vec3,
    side: Side,
    shooter: Entity,
    target: Option<Entity>,
    shot: Launch,
) {
    let from = mgt.map(|t| t.translation()).unwrap_or(origin);
    let b = &lib.bullets[&g.bullet];
    // PROVISIONAL: one fire sound per shell of a burst.
    if let Some((n, vol, max)) = &g.snd {
        sfx.write(Sfx { name: n.clone(), at: Some(from), vol: *vol, max: *max });
    }
    let mat = if side == Side::Player { look.shell.clone() } else { look.enemy_shell.clone() };
    let vel = shot.vel.unwrap_or_else(|| launch_velocity(dir, g.muzzle, g.bullet == TORPEDO_BULLET));
    let stf = Transform::from_translation(from).looking_to(vel, Vec3::Y);
    let shell = commands
        .spawn((
            Shell {
                vel,
                age: 0.0,
                lifetime: b.lifetime,
                // Mines drift.
                gravity: if matches!(b.payload, Payload::Mine { .. }) { 0.0 } else { g.shell_gravity() },
                bullet: g.bullet.clone(),
                side,
                shooter,
                target,
                angular_velocity: 0.0,
                payload: b.payload,
                aim: shot.aim,
                pd: shot.pd,
                hp: b.hitpoints,
                harmless: shot.harmless,
            },
            Transient,
            stf,
        ))
        .id();
    if let Some((name, vol, max)) = &b.travel_snd {
        commands.entity(shell).insert(crate::audio::Travel { name: name.clone(), vol: *vol, max: *max });
    }
    if let Some(glb) = &b.glb {
        commands.entity(shell).insert(WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(crate::data::model(glb)))));
    } else if b.bullet_fx.is_none() {
        // fx-only shells (Lancer sprite) get no placeholder mesh
        commands.entity(shell).insert((Mesh3d(look.mesh.clone()), MeshMaterial3d(mat)));
    }
    if let Some(n) = &b.bullet_fx {
        fx::spawn(commands, fxl, n, Some(shell), Transform::default(), stf, false);
    }
    if let (Some(n), Some(mg)) = (&g.muzzle_fx, mgt) {
        fx::spawn(commands, fxl, n, Some(mz), Transform::default(), mg.compute_transform(), false);
    }
}

/// Island gun towers: every mount of an armed base shoots the nearest visible ship its owner is hostile to, within the gun's range
/// (turret model as in `fire`). PROVISIONAL: the base's AI stance and 'attack' orders are not consulted; hostility alone arms it.
fn base_fire(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    fxl: Res<FxLib>,
    look: Res<Look>,
    mut rng: ResMut<Rng>,
    mut sfx: MessageWriter<Sfx>,
    mut fired: MessageWriter<Fired>,
    campaign: Option<Res<crate::campaign::Mission>>,
    mut bases: Query<(Entity, &Transform, &crate::campaign::MissionBase, &ArmedBase, &Health, &mut GunStates), Without<GunPart>>,
    ships: Query<(&Transform, &Ship, &Health, Option<&crate::campaign::MissionShip>, Entity)>,
    mut parts: Query<&mut Transform, (With<GunPart>, Without<Ship>)>,
    (globals, sensors): (Query<&GlobalTransform, With<GunPart>>, Res<crate::environment::Sensors>),
) {
    let Some(mission) = campaign else { return };
    let dt = time.delta_secs();
    for (me, tf, base, armed, health, mut states) in &mut bases {
        let class = &lib.classes[armed.class];
        let tp = ships
            .iter()
            .filter(|(_, _, h, m, _)| !h.destroyed && m.is_some_and(|m| m.visible && mission.script.hostile(&base.owner, &m.owner)))
            .map(|(t, s, _, _, e)| (t.transform_point(lib.classes[s.class].centre), t.rotation * Vec3::NEG_Z * s.speed, e, s.side))
            .filter(|(_, _, e, _)| sensors.in_sight(me, *e, Some(&mission)))
            .min_by(|a, b| a.0.distance(tf.translation).total_cmp(&b.0.distance(tf.translation)));
        for (m, st) in class.mounts.iter().zip(states.0.iter_mut()) {
            st.cooldown -= dt;
            st.burst_timer -= dt;
            let g = &lib.guns[&m.gun];
            let (mount_rot, origin) = (tf.rotation * m.rot, tf.transform_point(m.pos));
            let dead = m.parent.is_some_and(|p| health.sections[p] <= 0.0);
            const TOL: f32 = std::f32::consts::PI / 180.0;
            let half = m.arc / 2.0;
            let (mut want_yaw, mut want_pitch, mut engaged) = (0.0f32, Some(0.0f32), None);
            // Mortar towers lob at the ship (PROVISIONAL: mines/grav charges on bases are not seen in the data and stay idle).
            let usable = !is_indirect(&m.gun) || g.lob.is_some();
            if let (false, false, true, Some((p, v, _, _))) = (dead, g.point_defense || m.gun.contains("Harpoon"), usable, tp) {
                let d = aim_offset(p - origin, v, g.muzzle, g.bullet == TORPEDO_BULLET);
                let dir = match g.lob.and_then(|a| lob_velocity(d, a, g.shell_gravity())) {
                    Some(v) => v.normalize(),
                    None => aim(d, g.muzzle, g.shell_gravity()),
                };
                let l = mount_rot.inverse() * dir;
                let yaw = (-l.x).atan2(-l.z);
                want_yaw = yaw.clamp(-half, half);
                let in_range = Vec2::new(d.x, d.z).length() <= g.range[2];
                want_pitch = None;
                if (st.yaw - want_yaw).abs() < TOL && in_range {
                    want_pitch = Some(l.y.clamp(-1.0, 1.0).asin().clamp(g.vlim[0], g.vlim[1]));
                }
                if in_range && yaw.abs() <= half {
                    engaged = Some((dir, d.length()));
                }
            }
            st.yaw = (st.yaw + (want_yaw - st.yaw).clamp(-g.hspeed * dt, g.hspeed * dt)).clamp(-half.max(0.0), half);
            if let Some(p) = want_pitch {
                st.pitch += (p - st.pitch).clamp(-g.vspeed * dt, g.vspeed * dt);
            }
            st.in_range = engaged.is_some();
            st.aligned = engaged.is_some() && (st.yaw - want_yaw).abs() < TOL && want_pitch.is_some_and(|p| (st.pitch - p).abs() < TOL);
            if let Ok(mut t) = parts.get_mut(st.base) {
                t.rotation = Quat::from_rotation_y(st.yaw);
            }
            if let Ok(mut t) = parts.get_mut(st.barrel) {
                t.rotation = Quat::from_rotation_x(st.pitch);
            }
            let (Some((dir0, dist)), Some((_, _, target, tside))) = (engaged, tp) else {
                st.burst_left = 0;
                continue;
            };
            if st.burst_left == 0 && st.cooldown <= 0.0 && st.aligned {
                st.burst_left = g.burst;
                fired.write(Fired { shooter: me, target: Some(target), bullet: g.bullet.clone() });
                st.cooldown = g.burst_cycle();
                st.burst_timer = 0.0;
                let band = if dist <= g.range[0] {
                    g.dev[0]
                } else if dist <= g.range[1] {
                    g.dev[1]
                } else {
                    g.dev[2]
                };
                st.dev = (rng.f() * 2.0 - 1.0) * band * 0.5 * 0.5;
            }
            if st.burst_left > 0 && st.burst_timer <= 0.0 {
                st.burst_left -= 1;
                st.burst_timer = g.burst_reload;
                let sign = |r: f32| if r < 0.5 { -1.0 } else { 1.0 };
                let right = dir0.cross(Vec3::Y).normalize_or(Vec3::X);
                let turn = Quat::from_axis_angle(Vec3::Y, sign(rng.f()) * st.dev) * Quat::from_axis_angle(right, sign(rng.f()) * st.dev);
                let dir = turn * dir0;
                let mz = st.muzzles[st.next_muzzle % st.muzzles.len()];
                st.next_muzzle += 1;
                // The shell must hit ships of the target's side; the shooter's own side is the other one.
                let side = if tside == Side::Player { Side::Enemy } else { Side::Player };
                let goal = tp.map(|t| t.0);
                let shot = Launch {
                    vel: g.lob.and_then(|a| lob_velocity(goal? - origin, a, g.shell_gravity())).map(|v| turn * v),
                    aim: goal.filter(|_| is_indirect(&m.gun)),
                    ..default()
                };
                launch_shell(&mut commands, &assets, &lib, &fxl, &look, &mut sfx, g, mz, globals.get(mz).ok().copied(), origin, dir, side, me, Some(target), shot);
            }
        }
    }
}

/// Core damage of one hit on a destroyed vital section: `roll` x (sum of core% over destroyed vital sections) / 100.
fn core_bleed(class: &Class, hp: &Health, roll: f32) -> f32 {
    let pct: f32 = class.sections.iter().zip(&hp.sections).filter(|(s, h)| s.vital && **h <= 0.0).map(|(s, _)| s.core_pct).sum();
    (roll * pct / 100.0).round()
}

/// Native launch coast (0.5 s), 20-degree rudder saturation and lead inside 100 units; native WOT physics.
/// PROVISIONAL: uses the remake's planar integrator; the native invalidation timer and altitude tick scheduling remain approximate.
/// One point-defence bolt against a torpedo or mine: it loses the bolt's hitpoints; true once it is destroyed.
fn wear_down(threat_hp: &mut f32, bolt_hp: f32) -> bool {
    *threat_hp -= bolt_hp;
    *threat_hp <= 0.0
}

fn guide_torpedo(mut position: Vec3, velocity: &mut Vec3, spin: &mut f32, age: f32, target: Option<(Vec3, Vec3)>, physics: &crate::data::Physics, dt: f32) -> Vec3 {
    let steps = (dt * 60.0).ceil().max(1.0) as usize;
    let h = dt / steps as f32;
    for i in 0..steps {
        let speed = velocity.with_y(0.0).length();
        let mut heading = (-velocity.x).atan2(-velocity.z);
        let mut rudder = 0.0;
        if age + i as f32 * h >= 0.5
            && let Some((point, target_velocity)) = target
        {
            let distance = point.distance(position);
            let aim = point + if distance < 100.0 { target_velocity * (distance / speed.max(0.01)) } else { Vec3::ZERO };
            let delta = aim - position;
            let want = (-delta.x).atan2(-delta.z);
            rudder = (crate::angdiff(want, heading) / 20.0f32.to_radians()).clamp(-1.0, 1.0);
            // Native height adjustment is 0.025 every ~0.05 s, bounded to +/-3 units.
            let height = (point.y - position.y).clamp(-0.5 * h, 0.5 * h);
            position.y = (position.y + height).clamp(-3.0, 3.0);
        }
        let mut acceleration = rudder * physics.max_ang_acc * (1.0 - 1.0 / (0.1 * speed + 1.0));
        if speed > 0.67 * physics.max_speed {
            acceleration /= 3.0;
        } else if speed < 0.33 * physics.max_speed {
            acceleration *= 5.0 / 3.0;
        }
        acceleration -= spin.signum() * *spin * *spin * physics.rot_friction;
        *spin += acceleration.clamp(-60.0, 60.0) * h;
        heading += *spin * h;
        let speed = crate::approach_speed(speed, physics.max_speed, physics.thrust / physics.mass / physics.max_speed, h);
        *velocity = Vec3::new(-heading.sin(), 0.0, -heading.cos()) * speed;
        position += *velocity * h;
    }
    position
}

/// Distance from `p` to the segment `a`-`b`.
fn segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + ab * t)
}

/// What one shell hit on a ship section did beyond the lost hit points.
struct ShipHit {
    /// Initial strength of a fire that started.
    fire: Option<f32>,
    /// The section went from alive to destroyed with this hit.
    lost: bool,
}

/// Damage rules of one shell hit on section `k` (`damage` = [lower, upper, crit, fire chance, fire strength]).
/// Criticals are not modelled: in the exe they only feed a tally and crew speech (docs/re/movement.md).
/// Measured on retail (headless 1-vs-1 skirmishes per difficulty): the bullet table's damage is scaled by side and difficulty.
/// Player shells on computer ships: Easy x4, Normal x2, Hard x1; computer shells on the player's: x0.25, x0.5, x1.
/// PROVISIONAL: Easy x4 rests on two hits; computer-vs-computer and allied hits are not measured (1.0 here).
fn side_damage_scale(shooter: Side, victim: Side, difficulty: u8) -> f32 {
    let k = [4.0, 2.0, 1.0][difficulty.min(2) as usize];
    match (shooter, victim) {
        (Side::Player, Side::Enemy) => k,
        (Side::Enemy, Side::Player) => 1.0 / k,
        _ => 1.0,
    }
}

fn apply_ship_hit(class: &Class, hp: &mut Health, k: usize, damage: [f32; 5], invuln: f32, fire_free: bool, rng: &mut Rng) -> ShipHit {
    let [lo, hi, _crit, fire_p, fire_str] = damage;
    let protected = invuln >= 1.0;
    let was = hp.sections[k];
    hp.sections[k] = (was - (1.0 - invuln) * (lo + rng.f() * (hi - lo))).max(0.0);
    // Exe ignition roll per hit: chance > 0 and roll <= chance starts a fire of the bullet's initial strength.
    // PROVISIONAL: one fire per section (the exe's extra gate is not decoded).
    let fire = (!protected && fire_p > 0.0 && rng.f() <= fire_p && hp.sections[k] > 0.0 && fire_free).then_some(fire_str);
    // Exe: every hit that leaves a vital section at 0 HP (the killing hit and later ones) takes
    // a fresh damage roll x the summed core% of all destroyed vital sections / 100 off the core (rounding provisional).
    if !protected && class.sections[k].vital && hp.sections[k] == 0.0 {
        hp.core -= core_bleed(class, hp, (1.0 - invuln) * (lo + rng.f() * (hi - lo)));
    }
    ShipHit { fire, lost: was > 0.0 && hp.sections[k] == 0.0 }
}

fn shells(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<Library>,
    fxl: Res<FxLib>,
    mut rng: ResMut<Rng>,
    mut sfx: MessageWriter<Sfx>,
    mut hits: MessageWriter<Hit>,
    mut destroyed: MessageWriter<Destroyed>,
    mut rocks: Query<(Entity, &Transform, &mut Asteroid, &crate::collision::Drift), Without<Shell>>,
    animals: Query<(Entity, &Transform, &crate::environment::SpaceAnimal), Without<Shell>>,
    mut bases: Query<(Entity, &Transform, &ArmedBase, &mut Health, &crate::campaign::MissionBase), (Without<Shell>, Without<Ship>)>,
    campaign: Option<Res<crate::campaign::Mission>>,
    mut q: Query<(Entity, &mut Shell, &mut Transform), Without<Ship>>,
    // Shells pass through lifeboats (no weapon-vs-lifeboat response is registered in the exe; PROVISIONAL, docs/re/lifeboats.md §2.4).
    mut ships: Query<
        (Entity, &Transform, &Ship, &mut Health, &mut Fires, Option<&crate::campaign::MissionShip>, Option<&crate::collision::Drift>),
        Without<crate::lifeboats::Lifeboat>,
    >,
    mut detonate: MessageWriter<Detonate>,
    (mut dragons, mut scars): (Query<&mut crate::dragons::Dragon>, MessageWriter<crate::decals::Scar>),
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    // Torpedoes and mines in flight, for point-defence intercepts; each is shot down at most once.
    let threats: Vec<(Entity, Vec3, Side)> =
        q.iter().filter(|(_, s, _)| s.bullet.starts_with(TORPEDO_BULLET) || matches!(s.payload, Payload::Mine { .. })).map(|(e, s, t)| (e, t.translation, s.side)).collect();
    let mut threat_hp: std::collections::HashMap<Entity, f32> = q.iter().filter(|(e, ..)| threats.iter().any(|t| t.0 == *e)).map(|(e, s, _)| (e, s.hp)).collect();
    let mut downed: Vec<Entity> = vec![];
    for (e, mut s, mut tf) in &mut q {
        if downed.contains(&e) {
            continue;
        }
        let p0 = tf.translation;
        let bd = &lib.bullets[&s.bullet];
        s.vel.y -= s.gravity * dt;
        // Mines (MineAI sight, MinePhysics max speed): home on the nearest hostile ship in sight, else coast to rest at the drop point.
        // PROVISIONAL: the exe's mine steering/drag is not decoded.
        if let Payload::Mine { sight, speed } = s.payload {
            let prey = ships
                .iter()
                .filter(|(_, t, sh, h, _, m, _)| {
                    !h.destroyed && sh.side != s.side && m.is_none_or(|m| m.visible) && t.translation.distance(p0) < sight + lib.classes[sh.class].radius
                })
                .map(|(_, t, ..)| t.translation)
                .min_by(|a, b| a.distance(p0).total_cmp(&b.distance(p0)));
            s.vel = match prey {
                Some(p) => s.vel.lerp((p - p0).normalize_or_zero() * speed, (3.0 * dt).min(1.0)),
                None if s.aim.is_none_or(|a| a.distance(p0) < 2.0) || s.age > 3.0 => s.vel * (-1.5 * dt).exp(),
                None => s.vel,
            };
        }
        let p1 = if let Some(physics) = &bd.physics {
            let target = s
                .target
                .and_then(|target| ships.get(target).ok())
                .filter(|(_, _, ship, health, _, mission, _)| !health.destroyed && ship.side != s.side && mission.is_none_or(|m| m.visible))
                .map(|(_, tf, ship, _, _, _, drift)| {
                    let point = tf.transform_point(lib.classes[ship.class].homing_point);
                    let velocity = Vec3::new(-ship.heading.sin(), 0.0, -ship.heading.cos()) * ship.speed + drift.map_or(Vec3::ZERO, |d| d.0);
                    (point, velocity)
                })
                .or_else(|| {
                    s.target.and_then(|target| rocks.get(target).ok()).filter(|(_, _, rock, _)| rock.hp > 0.0).map(|(_, tf, rock, drift)| (tf.translation, rock.velocity + drift.0))
                })
                .or_else(|| s.target.and_then(|target| animals.get(target).ok()).filter(|(_, _, animal)| animal.visible).map(|(_, tf, animal)| (tf.translation, animal.velocity)));
            if target.is_none() {
                s.target = None;
            }
            let mut velocity = s.vel;
            let mut spin = s.angular_velocity;
            let position = guide_torpedo(p0, &mut velocity, &mut spin, s.age, target, physics, dt);
            s.vel = velocity;
            s.angular_velocity = spin;
            position
        } else {
            p0 + s.vel * dt
        };
        s.age += dt;
        // Point defence: the bolt meets a hostile torpedo or mine on its way and is spent; the threat loses the bolt's hitpoints and
        // is destroyed at zero. PROVISIONAL: hitpoint-for-hitpoint exchange inferred from the WOT values, not verified in the original.
        if s.pd
            && let Some(&(te, tp, _)) = threats.iter().find(|t| t.2 != s.side && !downed.contains(&t.0) && segment_distance(t.1, p0, p1) < 2.5)
        {
            commands.entity(e).despawn();
            let at = Transform::from_translation(tp);
            if wear_down(threat_hp.entry(te).or_insert(1.0), s.hp) {
                downed.push(te);
                commands.entity(te).despawn();
                fx::spawn(&mut commands, &fxl, "Explosion_Light", None, at, at, true);
                sfx.write(Sfx { name: format!("Explosion_Med0{}", 1 + (rng.f() * 8.0) as u32 % 8), at: Some(tp), vol: 0.8, max: 600.0 });
            } else if let Some(fx_name) = &bd.hit_fx {
                fx::spawn(&mut commands, &fxl, fx_name, None, at, at, true);
            }
            continue;
        }
        // Lob shells burst where they come down through the height they were aimed at.
        if let Some(a) = s.aim.filter(|a| !matches!(s.payload, Payload::Mine { .. }) && s.vel.y < 0.0 && p1.y <= a.y) {
            let t = if (p0.y - p1.y).abs() > 1e-4 { ((p0.y - a.y) / (p0.y - p1.y)).clamp(0.0, 1.0) } else { 1.0 };
            detonate.write(Detonate { bullet: s.bullet.clone(), at: p0.lerp(p1, t), shooter: Some(s.shooter) });
            commands.entity(e).despawn();
            continue;
        }
        let mut best: Option<(f32, usize, Vec3)> = None; // (t, section, class)
        let mut best_ship: Option<Entity> = None;
        for (i, stf, ship, hp, _, m, _) in ships.iter() {
            if hp.destroyed || ship.side == s.side || m.is_some_and(|m| !m.visible) {
                continue;
            }
            let class = &lib.classes[ship.class];
            if segment_distance(stf.translation, p0, p1) > class.hull_bound * stf.scale.max_element() {
                continue;
            }
            let inv = stf.to_matrix().inverse();
            let (a, b) = (inv.transform_point3(p0), inv.transform_point3(p1));
            let hit = ship_segment_hit(class, hp, a, b);
            if let Some((t, k)) = hit
                && best.as_ref().is_none_or(|x| t < x.0)
            {
                best = Some((t, k, p0 + (p1 - p0) * t));
                best_ship = Some(i);
            }
        }
        let rock = rocks
            .iter()
            .filter(|(_, _, rk, _)| rk.hp > 0.0)
            .filter_map(|(re, rtf, rk, _)| seg_sphere(p0, p1, rtf.translation, rk.radius).map(|t| (t, re)))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let animal = animals
            .iter()
            .filter(|(ae, _, a)| a.visible && !a.dead && *ae != s.shooter)
            .filter_map(|(ae, atf, a)| seg_sphere(p0, p1, atf.translation, a.radius).map(|t| (t, ae)))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((t, ae)) = animal.filter(|a| best.as_ref().is_none_or(|b| a.0 < b.0) && rock.is_none_or(|r| a.0 < r.0)) {
            hits.write(Hit { shooter: s.shooter, victim: ae });
            // Dragons have one 'Dragon Scale' section (500 HP). PROVISIONAL: whales have no exported strength/damage model; impacts
            // consume shells but do not invent ship-style HP.
            if let Ok(mut d) = dragons.get_mut(ae)
                && !s.harmless
            {
                let [lo, hi, ..] = bd.damage["Dragon"];
                d.hurt(lo + rng.f() * (hi - lo), s.shooter, now);
            }
            if let Some(n) = &bd.hit_fx {
                let tf = Transform::from_translation(p0 + (p1 - p0) * t).looking_to(-s.vel, Vec3::Y);
                fx::spawn(&mut commands, &fxl, n, None, tf, tf, true);
            }
            commands.entity(e).despawn();
            continue;
        }
        if let Some((t, re)) = rock.filter(|r| best.as_ref().is_none_or(|b| r.0 < b.0)) {
            let at = p0 + (p1 - p0) * t;
            let [lo, hi, ..] = bd.damage["Stone"];
            hits.write(Hit { shooter: s.shooter, victim: re });
            if let Ok((_, _, mut rk, _)) = rocks.get_mut(re) {
                if rk.damage(lo + rng.f() * (hi - lo)) {
                    destroyed.write(Destroyed { by: s.shooter, victim: re });
                } else {
                    sfx.write(Sfx { name: format!("Damage_Stone0{}", 1 + (rng.f() * 5.0) as u32 % 5), at: Some(at), vol: 1.0, max: 600.0 });
                }
            }
            if let Some(n) = &bd.hit_fx {
                let tf = Transform::from_translation(at).looking_to(-s.vel, Vec3::Y);
                fx::spawn(&mut commands, &fxl, n, None, tf, tf, true);
            }
            commands.entity(e).despawn();
            continue;
        }
        // Island gun towers: hit only by the side the base's owner is hostile to (neutral islands let shells pass).
        let base_hit = bases
            .iter()
            .filter(|(.., hp, mb)| {
                !hp.destroyed
                    && campaign.as_deref().is_some_and(|c| {
                        let enemy = c.script.hostile(c.script.player(), &mb.owner);
                        (if enemy { Side::Enemy } else { Side::Player }) != s.side
                    })
            })
            .filter_map(|(be, btf, ab, ..)| {
                let class = &lib.classes[ab.class];
                let inv = btf.to_matrix().inverse();
                let (a, b) = (inv.transform_point3(p0), inv.transform_point3(p1));
                class.sections.iter().enumerate().filter_map(|(k, sec)| seg_mesh(a, b, sec).map(|t| (t, k, be))).min_by(|x, y| x.0.total_cmp(&y.0))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0));
        if let Some((t, k, be)) = base_hit.filter(|b| best.as_ref().is_none_or(|x| b.0 < x.0))
            && let Ok((_, _, ab, mut hp, _)) = bases.get_mut(be)
        {
            let sec = &lib.classes[ab.class].sections[k];
            let key = MATERIALS.iter().find(|m| sec.material.contains(**m)).copied().unwrap_or("Wood");
            let [lo, hi, ..] = bd.damage[key];
            let at = p0 + (p1 - p0) * t;
            hits.write(Hit { shooter: s.shooter, victim: be });
            hp.sections[k] = (hp.sections[k] - (lo + rng.f() * (hi - lo))).max(0.0);
            if let Some(pool) = bd.impact_snd.get(key).filter(|p| !p.is_empty()) {
                sfx.write(Sfx { name: pool[(rng.f() * pool.len() as f32) as usize % pool.len()].clone(), at: Some(at), vol: bd.impact_vol, max: bd.impact_max });
            }
            if let Some(n) = &bd.hit_fx {
                let tf = Transform::from_translation(at).looking_to(-s.vel, Vec3::Y);
                fx::spawn(&mut commands, &fxl, n, None, tf, tf, true);
            }
            commands.entity(e).despawn();
            continue;
        }
        // Blast, Nova and gravity shells burst on the hull they strike; warning shots splash harmlessly.
        if let (Some((_, k, at)), true) = (best, s.harmless || matches!(s.payload, Payload::Blast { .. } | Payload::Nova | Payload::Grav { .. })) {
            if s.harmless {
                // A warning shot glances off: the bullet's ricochet sound for the struck material.
                let material = best_ship.and_then(|si| ships.get(si).ok()).map(|x| lib.classes[x.2.class].sections[k].material.clone()).unwrap_or_default();
                let key = MATERIALS.iter().find(|m| material.contains(**m)).copied().unwrap_or("Wood");
                if let Some(name) = bd.ricochet_snd.get(key) {
                    sfx.write(Sfx { name: name.clone(), at: Some(at), vol: 1.0, max: 600.0 });
                }
                if let Some(n) = &bd.hit_fx {
                    let t = Transform::from_translation(at).looking_to(-s.vel, Vec3::Y);
                    fx::spawn(&mut commands, &fxl, n, None, t, t, true);
                }
            } else {
                detonate.write(Detonate { bullet: s.bullet.clone(), at, shooter: Some(s.shooter) });
            }
            commands.entity(e).despawn();
            continue;
        }
        if let (Some((_, k, at)), Some(si)) = (best, best_ship) {
            let (se, stf, ship, mut hp, mut fires, mission, _) = ships.get_mut(si).unwrap();
            let class = &lib.classes[ship.class];
            let sec = &class.sections[k];
            let key = MATERIALS.iter().find(|m| sec.material.contains(**m)).copied().unwrap_or("Wood");
            // Criticals are not modelled: in the exe they only feed a tally and crew speech (docs/re/movement.md).
            let mut damage = bd.damage[key];
            let scale = side_damage_scale(s.side, ship.side, crate::menu::difficulty());
            damage[0] *= scale;
            damage[1] *= scale;
            hits.write(Hit { shooter: s.shooter, victim: se });
            // Script vulnerability: 'Invulnerable Percent' 1.0 takes no damage and no fire; partial values scale the damage.
            let invuln = mission.map_or(0.0, |m| m.invuln);
            if s.bullet == TORPEDO_BULLET {
                info!("Torpedo hit {} ({}){}", class.name, sec.label, if invuln >= 1.0 { " [training: invulnerable]" } else { "" });
            }
            let fire_free = !fires.list.iter().any(|f| f.sec == k && f.state == 0);
            let before = hp.sections[k];
            let outcome = apply_ship_hit(class, &mut hp, k, damage, invuln, fire_free, &mut rng);
            if let (Some(size), true) = (bd.decal, hp.sections[k] < before) {
                let inv = stf.to_matrix().inverse();
                let material = if sec.material.contains("Cloth") { "Cloth" } else { key };
                scars.write(crate::decals::Scar { ship: se, local: inv.transform_point3(at), dir: inv.transform_vector3(s.vel).normalize_or_zero(), material, size });
            }
            if let Some(fire_str) = outcome.fire {
                let local = Transform::from_translation(stf.to_matrix().inverse().transform_point3(at));
                let fx = fx::spawn(&mut commands, &fxl, "FireOnShip", Some(se), local, Transform::from_translation(at), false);
                if let Some(e) = fx {
                    let sc = fire_scale(fire_str);
                    commands.entity(e).queue(move |mut w: EntityWorldMut| {
                        if let Some(mut f) = w.get_mut::<fx::Fx>() {
                            f.rate_scale = sc;
                        }
                    });
                }
                info!("fire on {} of {}", sec.label, class.name);
                fires.list.push(Fire { sec: k, intensity: fire_str, state: 0, t: 0.0, fx });
            }
            if let Some(pool) = bd.impact_snd.get(key).filter(|p| !p.is_empty()) {
                sfx.write(Sfx { name: pool[(rng.f() * pool.len() as f32) as usize % pool.len()].clone(), at: Some(at), vol: bd.impact_vol, max: bd.impact_max });
            }
            if outcome.lost {
                // Exe pool per section material (Damage_<Wood|Metal|Sail|Stone>01-05, 600 units). PROVISIONAL: played on section loss.
                let pool = match key {
                    "Wood" | "Reinforced" => Some("Wood"),
                    "Iron" => Some("Metal"),
                    "Cloth" => Some("Sail"),
                    "Stone" => Some("Stone"),
                    _ => None,
                };
                if let Some(p) = pool {
                    sfx.write(Sfx { name: format!("Damage_{p}0{}", 1 + (rng.f() * 5.0) as u32 % 5), at: Some(at), vol: 1.0, max: 600.0 });
                }
                info!("{} of {} destroyed", sec.label, class.name);
            }
            if let Some(n) = &bd.hit_fx {
                let t = Transform::from_translation(at).looking_to(-s.vel, Vec3::Y);
                fx::spawn(&mut commands, &fxl, n, None, t, t, true);
            }
            commands.entity(e).despawn();
        } else if s.age > s.lifetime || p1.y < -300.0 {
            commands.entity(e).despawn();
        } else {
            tf.translation = p1;
            tf.look_to(s.vel, Vec3::Y);
        }
    }
    // Torpedoes and mines keep the wear from point-defence bolts that did not finish them this frame.
    for (e, mut s, _) in &mut q {
        if let Some(&hp) = threat_hp.get(&e) {
            s.hp = hp;
        }
    }
}

/// PROVISIONAL: owner-observed nose-first sinking, not yet calibrated against retail timing.
#[derive(Component)]
pub struct Sinking {
    start: Transform,
    elapsed: f32,
}

const SINK_TIME: f32 = 4.0;

impl Sinking {
    fn transform(&self, radius: f32) -> Transform {
        let t = (self.elapsed / SINK_TIME).clamp(0.0, 1.0);
        let mut tf = self.start;
        // The bow is local -Z; negative X pitch points it down.
        tf.rotation *= Quat::from_rotation_x(-65.0_f32.to_radians() * t);
        tf.translation.y -= radius * 0.8 * t * t;
        tf
    }
}

/// Gameplay death is immediate; the hull sinks before its final explosion.
fn deaths(
    mut commands: Commands,
    lib: Res<Library>,
    fxl: Res<FxLib>,
    look: Res<Look>,
    mut rng: ResMut<Rng>,
    mut sfx: MessageWriter<Sfx>,
    mut lost: MessageWriter<crate::lifeboats::ShipLost>,
    time: Res<Time>,
    children: Query<&Children>,
    mut ships: Query<(Entity, &mut Transform, &mut Ship, &mut Health, &mut Visibility, Option<&mut Sinking>)>,
) {
    for (e, mut tf, mut ship, mut hp, mut vis, sinking) in &mut ships {
        let class = &lib.classes[ship.class];
        if !hp.destroyed {
            if hp.core > 0.0 {
                continue;
            }
            hp.destroyed = true;
            ship.speed = 0.0;
            ship.ang_vel = 0.0;
            lost.write(crate::lifeboats::ShipLost(e));
            commands.entity(e).insert(Sinking { start: *tf, elapsed: 0.0 });
            info!("{} destroyed", class.name);
            continue;
        }
        // Scripted Destroy remains an immediate, silent removal without lifeboats.
        let Some(mut sinking) = sinking else { continue };
        sinking.elapsed += time.delta_secs();
        *tf = sinking.transform(class.radius);
        if sinking.elapsed < SINK_TIME {
            continue;
        }
        let at = tf.translation;
        *vis = Visibility::Hidden;
        // Keep the logical ship for mission rules, but remove its entire presentation (including looping smoke).
        if let Ok(children) = children.get(e) {
            for child in children.iter() {
                commands.entity(child).try_despawn();
            }
        }
        *tf = sinking.start;
        commands.entity(e).remove::<Sinking>();
        // Exe pool Explosion_Heavy01-08 (8000 units). PROVISIONAL: played on ship destruction.
        sfx.write(Sfx { name: format!("Explosion_Heavy0{}", 1 + (rng.f() * 8.0) as u32 % 8), at: Some(at), vol: 1.0, max: 8000.0 });
        if let Some(n) = &class.explosion_fx {
            let t = Transform::from_translation(at);
            fx::spawn(&mut commands, &fxl, n, None, t, t, true);
        }
        commands.spawn((Flash(1.5), Transient, Mesh3d(look.mesh.clone()), MeshMaterial3d(look.flash.clone()), Transform::from_translation(at).with_scale(Vec3::splat(10.0))));
    }
}

fn flashes(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Flash)>) {
    for (e, mut f) in &mut q {
        f.0 -= time.delta_secs();
        if f.0 <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

fn ring(g: &mut Gizmos, c: Vec3, r: f32, col: Color) {
    let n = 32;
    for k in 0..n {
        let (a, b) = (k as f32 / n as f32 * std::f32::consts::TAU, (k + 1) as f32 / n as f32 * std::f32::consts::TAU);
        g.line(c + Vec3::new(a.cos(), 0.0, a.sin()) * r, c + Vec3::new(b.cos(), 0.0, b.sin()) * r, col);
    }
}

/// A ring on the target (the HUD draws the HP; gun readiness is in the weapon panel).
fn draw(mut g: Gizmos, lib: Res<Library>, target: Res<Target>, ships: Query<(Entity, &Transform, &Ship, &Health)>, mission: Option<Res<crate::campaign::Mission>>) {
    if crate::briefing::active(mission.as_deref()) {
        return;
    }
    for (e, tf, ship, h) in &ships {
        if h.destroyed {
            continue;
        }
        let class = &lib.classes[ship.class];
        if target.0 == Some(e) {
            ring(&mut g, tf.translation, class.radius, Color::srgb(1.0, 0.2, 0.2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_scales_with_side_and_difficulty() {
        assert_eq!(side_damage_scale(Side::Player, Side::Enemy, 1), 2.0);
        assert_eq!(side_damage_scale(Side::Enemy, Side::Player, 1), 0.5);
        assert_eq!(side_damage_scale(Side::Enemy, Side::Player, 0), 0.25);
        assert_eq!(side_damage_scale(Side::Player, Side::Enemy, 2), 1.0);
        assert_eq!(side_damage_scale(Side::Enemy, Side::Enemy, 0), 1.0);
    }

    #[test]
    fn health_scale_applies_to_core_and_every_section() {
        let mut h = Health { sections: vec![10.0, 4.0], core: 8.0, destroyed: false };
        h.scale(0.5);
        assert_eq!((h.sections.as_slice(), h.core), (&[5.0, 2.0][..], 4.0));
    }

    #[test]
    fn destroyed_sail_does_not_absorb_shots_but_destroyed_vital_sections_do() {
        let (guns, _) = load_weapons(&assets_dir());
        let mut class = load_class(&assets_dir(), "Ship_Navy_Torpedo", &guns);
        class.sections.retain(|s| !s.vital);
        assert_eq!(class.sections.len(), 1);
        let [a, b, c] = class.sections[0].tris[0];
        let centre = (a + b + c) / 3.0;
        let normal = (b - a).cross(c - a).normalize();
        let (start, end) = (centre - normal, centre + normal);
        let mut hp = Health::new(&class);
        assert!(ship_segment_hit(&class, &hp, start, end).is_some());
        hp.sections[0] = 0.0;
        assert!(ship_segment_hit(&class, &hp, start, end).is_none());
        class.sections[0].vital = true;
        assert!(ship_segment_hit(&class, &hp, start, end).is_some());
    }

    #[test]
    fn destroyed_torpedo_boat_sail_redirects_aim_to_hull() {
        let lib = Library::load(&assets_dir());
        let class = &lib.classes[lib.index("Ship_Navy_Torpedo")];
        let k = class.sections.iter().position(|s| !s.vital).unwrap();
        let sail = &class.sections[k];
        let point = Vec3::new((sail.min.x + sail.max.x) / 2.0, sail.max.y - 0.01, (sail.min.z + sail.max.z) / 2.0);
        let mut hp = Health::new(class);
        assert_eq!(ship_aim_point(class, &hp, Some(point)), point);
        hp.sections[k] = 0.0;
        assert_eq!(ship_aim_point(class, &hp, Some(point)), class.centre);
        let hull = class.sections.iter().find(|s| s.vital && s.label == "Bow_wood").unwrap();
        let hull_point = (hull.min + hull.max) / 2.0;
        assert_eq!(ship_aim_point(class, &hp, Some(hull_point)), hull_point);
    }

    #[test]
    fn sinking_lowers_the_bow_and_preserves_heading_and_scale() {
        let start = Transform::from_xyz(10.0, 20.0, 30.0).with_rotation(Quat::from_rotation_y(1.2)).with_scale(Vec3::splat(2.0));
        let mut sinking = Sinking { start, elapsed: 0.0 };
        assert_eq!(sinking.transform(10.0), start);
        sinking.elapsed = SINK_TIME / 2.0;
        let midway = sinking.transform(10.0);
        assert!(midway.rotation.mul_vec3(Vec3::NEG_Z).y < 0.0, "bow must pitch down");
        assert_eq!(midway.translation, Vec3::new(10.0, 18.0, 30.0));
        assert_eq!(midway.scale, start.scale);
        sinking.elapsed = SINK_TIME;
        let end = sinking.transform(10.0);
        assert_eq!(end.translation, Vec3::new(10.0, 12.0, 30.0));
        assert!(end.rotation.mul_vec3(Vec3::NEG_Z).y < midway.rotation.mul_vec3(Vec3::NEG_Z).y);
        sinking.elapsed *= 2.0;
        assert_eq!(sinking.transform(10.0), end);
    }

    #[test]
    fn asteroid_tumble_obeys_group_ranges_without_drift() {
        let mut app = App::new();
        app.init_resource::<Time>().init_resource::<AsteroidDefs>().insert_resource(Rng(123)).add_message::<AsteroidSetup>().add_systems(Update, asteroid_motion);
        let defs = app.world().resource::<AsteroidDefs>();
        let a = Asteroid::new("Asteroid_Large", "Asteroid Belt", defs).unwrap();
        let b = Asteroid::new("Asteroid_Med", "Montressor Asteroids", defs).unwrap();
        let a = app.world_mut().spawn((a, Transform::from_xyz(20.0, 0.0, 0.0))).id();
        let b = app.world_mut().spawn((b, Transform::default())).id();
        for (entity, upper) in [(a, 0.9), (b, 0.75)] {
            app.world_mut().write_message(AsteroidSetup { units: vec![entity], speed: [0.0, 0.0], tumble: [0.5, upper], path: vec![], closest: false, mode: PathMode::ToEnd });
        }
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.1));
        app.update();
        for (entity, upper) in [(a, 0.9), (b, 0.75)] {
            let rock = app.world().get::<Asteroid>(entity).unwrap();
            assert!((0.5..=upper).contains(&rock.tumble));
            assert!((rock.axis.length() - 1.0).abs() < 1e-5);
            let tf = app.world().get::<Transform>(entity).unwrap();
            assert!((tf.rotation.angle_between(Quat::IDENTITY) - rock.tumble * 0.1).abs() < 1e-4);
            assert_eq!(tf.translation, if entity == a { Vec3::new(20.0, 0.0, 0.0) } else { Vec3::ZERO });
        }
    }

    #[test]
    fn asteroid_path_tracks_velocity_and_stops_at_end() {
        let mut app = App::new();
        app.init_resource::<Time>().init_resource::<AsteroidDefs>().insert_resource(Rng(123)).add_message::<AsteroidSetup>().add_systems(Update, asteroid_motion);
        let rock = Asteroid::new("Asteroid_Med", "rocks", app.world().resource::<AsteroidDefs>()).unwrap();
        let e = app.world_mut().spawn((rock, Transform::default())).id();
        app.world_mut().write_message(AsteroidSetup { units: vec![e], speed: [2.0, 2.0], tumble: [0.0, 0.0], path: vec![Vec3::X], closest: true, mode: PathMode::ToEnd });
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.1));
        app.update();
        assert_eq!(app.world().get::<Asteroid>(e).unwrap().velocity, Vec3::X * 2.0);
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(app.world().get::<Transform>(e).unwrap().translation, Vec3::X);
        assert_eq!(app.world().get::<Asteroid>(e).unwrap().velocity, Vec3::ZERO);
    }

    #[test]
    fn asteroid_path_modes_teleport_loop_and_ping_pong() {
        let run = |mode| {
            let mut app = App::new();
            app.init_resource::<Time>().init_resource::<AsteroidDefs>().insert_resource(Rng(1)).add_message::<AsteroidSetup>().add_systems(Update, asteroid_motion);
            let rock = Asteroid::new("Asteroid_Med", "rocks", app.world().resource::<AsteroidDefs>()).unwrap();
            let e = app.world_mut().spawn((rock, Transform::default())).id();
            let path = vec![Vec3::ZERO, Vec3::X * 10.0, Vec3::X * 20.0];
            app.world_mut().write_message(AsteroidSetup { units: vec![e], speed: [10.0, 10.0], tumble: [0.0, 0.0], path, closest: true, mode });
            let mut xs = vec![];
            for _ in 0..40 {
                app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.1));
                app.update();
                xs.push(app.world().get::<Transform>(e).unwrap().translation.x);
            }
            xs
        };
        let tele = run(PathMode::Teleport);
        assert!(tele.windows(2).any(|w| w[1] < w[0] - 15.0), "jumps back to the first point");
        assert!(tele.iter().all(|x| *x <= 20.001));
        let pong = run(PathMode::EndToEnd);
        assert!(pong.iter().any(|x| *x > 19.0) && pong.windows(2).any(|w| w[1] < w[0]) && pong.windows(2).all(|w| w[0] - w[1] < 1.5), "reverses smoothly");
        assert!(run(PathMode::ToEnd).iter().all(|x| *x <= 20.001));
    }

    #[test]
    fn asteroid_setup_replaces_contact_drift_without_erasing_it_each_tick() {
        let mut app = App::new();
        app.init_resource::<Time>().init_resource::<AsteroidDefs>().insert_resource(Rng(123)).add_message::<AsteroidSetup>().add_systems(Update, asteroid_motion);
        let rock = Asteroid::new("Asteroid_Small", "rocks", app.world().resource::<AsteroidDefs>()).unwrap();
        let e = app.world_mut().spawn((rock, Transform::default())).id();
        app.world_mut().get_mut::<crate::collision::Drift>(e).unwrap().0 = Vec3::X * 6.0;
        app.update();
        assert_eq!(app.world().get::<crate::collision::Drift>(e).unwrap().0, Vec3::X * 6.0);
        app.world_mut().write_message(AsteroidSetup { units: vec![e], speed: [0.0, 0.0], tumble: [0.0, 0.0], path: vec![], closest: false, mode: PathMode::ToEnd });
        app.update();
        assert_eq!(app.world().get::<crate::collision::Drift>(e).unwrap().0, Vec3::ZERO);
    }

    #[test]
    fn asteroid_breakup_is_once_only_recursive_and_cleans_target() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<WorldAsset>()
            .init_resource::<AsteroidDefs>()
            .init_resource::<crate::collision::Shapes>()
            .init_resource::<FxLib>()
            .init_resource::<Target>()
            .insert_resource(Rng(123))
            .add_message::<Destroyed>()
            .add_message::<Detonate>()
            .add_message::<Shoot>()
            .add_message::<Strike>()
            .add_message::<Sfx>()
            .add_systems(Update, asteroid_deaths);
        let shooter = app.world_mut().spawn_empty().id();
        let rock = Asteroid::new("Asteroid_Large", "Asteroid Belt", app.world().resource::<AsteroidDefs>()).unwrap();
        let parent = app.world_mut().spawn((rock, Transform::default())).id();
        app.world_mut().get_mut::<crate::collision::Drift>(parent).unwrap().0 = Vec3::X * 6.0;
        app.world_mut().resource_mut::<Target>().0 = Some(parent);
        let mut rock = app.world_mut().get_mut::<Asteroid>(parent).unwrap();
        assert!(!rock.damage(49.0));
        assert!(rock.damage(1.0));
        assert!(!rock.damage(1000.0), "multiple shells must not repeat death");
        app.world_mut().write_message(Destroyed { by: shooter, victim: parent });
        app.update();
        assert!(app.world().get_entity(parent).is_err());
        assert_eq!(app.world().resource::<Target>().0, None);
        let children: Vec<_> = app
            .world_mut()
            .query::<(Entity, &Asteroid, &Transform)>()
            .iter(app.world())
            .map(|(e, a, t)| {
                assert_eq!((a.kind.as_str(), a.group.as_str(), a.hp), ("Asteroid_Med", "Asteroid Belt", 30.0));
                assert!((0.8..=0.8 + 2.2 / 3.0).contains(&(a.velocity - Vec3::X * 6.0).length()), "fragments inherit the parent's contact velocity");
                assert!((0.0..=std::f32::consts::FRAC_PI_2).contains(&a.tumble));
                assert!((t.translation.length() - 6.6).abs() < 1e-4);
                assert!(app.world().get::<crate::collision::Obstacle>(e).is_some());
                e
            })
            .collect();
        assert_eq!(children.len(), 2);
        assert_eq!(app.world_mut().query::<&fx::Fx>().iter(app.world()).count(), 1);
        app.update();
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 2);
        for child in children {
            assert!(app.world_mut().get_mut::<Asteroid>(child).unwrap().damage(30.0));
            app.world_mut().write_message(Destroyed { by: shooter, victim: child });
        }
        app.update();
        let small: Vec<_> = app
            .world_mut()
            .query::<(Entity, &Asteroid)>()
            .iter(app.world())
            .map(|(e, a)| {
                assert_eq!(a.hp, 10.0);
                e
            })
            .collect();
        assert_eq!(small.len(), 6);
        for child in small {
            assert!(app.world_mut().get_mut::<Asteroid>(child).unwrap().damage(10.0));
            app.world_mut().write_message(Destroyed { by: shooter, victim: child });
        }
        app.update();
        assert_eq!(app.world_mut().query::<&Asteroid>().iter(app.world()).count(), 0);
        assert_eq!(app.world_mut().query::<&crate::collision::Obstacle>().iter(app.world()).count(), 0);
    }

    #[test]
    fn island_gun_towers_load_and_mission_damage_follows_their_vital_sections() {
        let lib = Library::load(&assets_dir());
        let class = &lib.classes[lib.index("Base_SmugglersCamp")];
        assert_eq!(class.mounts.len(), 2);
        for mount in &class.mounts {
            let section = &class.sections[mount.parent.expect("gun belongs to a tower")];
            assert!(mount.pos.x >= section.min.x && mount.pos.x <= section.max.x);
            assert!(mount.pos.z >= section.min.z && mount.pos.z <= section.max.z);
            assert!((mount.pos.y - section.max.y).abs() < 1.0, "gun sits on its own tower roof");
        }
        assert!(!class.skirmish);
        let mut hp = Health::new(class);
        assert_eq!(base_mission_damage(class, &hp), 0.0);
        let point = base_aim_point(class, &hp, Some(class.mounts[0].pos)).unwrap();
        let ray = Ray3d::new(point + Vec3::Y * 20.0, Dir3::NEG_Y);
        assert!(base_ray_hit(class, &hp, &Transform::default(), ray).is_some());
        for (s, c) in hp.sections.iter_mut().zip(&class.sections) {
            if c.mission {
                *s = 0.0;
            }
        }
        assert_eq!(base_mission_damage(class, &hp), 1.0);
        assert!(base_aim_point(class, &hp, Some(point)).is_none());
        assert!(base_ray_hit(class, &hp, &Transform::default(), ray).is_none(), "destroyed towers are no longer attack targets");
    }

    #[test]
    fn hostile_island_towers_shoot_the_player_and_neutral_ones_do_not() {
        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default(), TransformPlugin))
            .init_asset::<bevy::world_serialization::WorldAsset>()
            .init_asset::<bevy::prelude::Image>()
            .add_message::<Sfx>()
            .add_message::<Fired>()
            .add_message::<Hit>()
            .add_message::<Destroyed>()
            .add_message::<Detonate>()
            .add_message::<crate::decals::Scar>()
            .init_resource::<Time>()
            .init_resource::<FxLib>()
            .insert_resource(Library::load(&assets_dir()))
            .insert_resource(Rng::new(7))
            .insert_resource(Look { mesh: default(), shell: default(), enemy_shell: default(), flash: default() })
            .init_resource::<crate::environment::Sensors>()
            .add_systems(PreUpdate, crate::environment::refresh_sensors)
            .add_systems(Update, base_fire);
        let mission = crate::campaign::load_id("mission02");
        let player = mission.script.player().to_string();
        assert!(mission.script.hostile(&player, "SmugglerCampEnemy"), "the camp's enemy owner is hostile to the player");
        assert!(!mission.script.hostile(&player, "NO PLAYER"));
        app.insert_resource(mission);
        let assets = app.world().resource::<AssetServer>().clone();
        let lib = Library::load(&assets_dir());
        let class = lib.index("Base_SmugglersCamp");
        let spawn_base = |app: &mut App, owner: &str, x: f32| {
            let id = app.world_mut().spawn((Transform::from_xyz(x, -27.0, 0.0), crate::campaign::MissionBase { id: 1, owner: owner.into() })).id();
            let states = spawn_guns(&mut app.world_mut().commands(), &assets, &lib, id, class);
            app.world_mut().entity_mut(id).insert((ArmedBase { class }, Health::new(&lib.classes[class]), states));
            app.world_mut().flush();
            id
        };
        let hostile = spawn_base(&mut app, "SmugglerCampEnemy", 0.0);
        spawn_base(&mut app, "NO PLAYER", 40.0);
        let c = lib.index("Ship_Navy_Torpedo");
        let ship = Ship {
            class: c,
            side: Side::Player,
            heading: 0.0,
            speed: 0.0,
            ang_vel: 0.0,
            order: 0,
            waypoints: vec![],
            goal: None,
            cap: 1.0,
            throttle: None,
            sail_energy: 1.0,
            crew_speed: 1.0,
            crew_turn: 1.0,
        };
        let ms = crate::campaign::MissionShip {
            id: 1,
            name: "p".into(),
            owner: player,
            hold_fire: false,
            invuln: 1.0,
            visible: true,
            stance: crate::rules::Stance::Neutral,
            target: None,
            attack: vec![],
            flag: String::new(),
            banner: String::new(),
            looping: None,
            stop_at_end: false,
            create_lifeboats: false,
            ..Default::default()
        };
        let player_ship = app.world_mut().spawn((Transform::from_xyz(0.0, 0.0, -120.0), ship.clone(), Health::new(&lib.classes[c]), Fires::default(), ms, PlayerShip)).id();
        for _ in 0..300 {
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.05));
            app.update();
        }
        let shooters: Vec<Entity> = app.world_mut().query::<&Shell>().iter(app.world()).map(|s| s.shooter).collect();
        assert!(!shooters.is_empty() && shooters.iter().all(|s| *s == hostile), "only the hostile camp opened fire");
        let states = spawn_guns(&mut app.world_mut().commands(), &assets, &lib, player_ship, c);
        app.world_mut().entity_mut(player_ship).insert(states);
        app.world_mut().flush();
        let old_shells: Vec<Entity> = app.world_mut().query_filtered::<Entity, With<Shell>>().iter(app.world()).collect();
        for e in old_shells {
            app.world_mut().despawn(e);
        }
        app.insert_resource(Target::default()).add_systems(Update, (fire.after(base_fire), shells.after(fire)));
        let mut lancer_fired = false;
        for mount in &lib.classes[class].mounts {
            let tower = mount.parent.unwrap();
            let aim = mount.pos;
            let world_aim = app.world().get::<Transform>(hostile).unwrap().transform_point(aim);
            app.world_mut().get_mut::<Transform>(player_ship).unwrap().translation = (world_aim + Vec3::Z * 100.0).with_y(0.0);
            *app.world_mut().resource_mut::<Target>() = Target(Some(hostile), Some(aim), default());
            for _ in 0..1200 {
                app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.05));
                app.update();
                for shell in app.world_mut().query::<&Shell>().iter(app.world()).filter(|s| s.shooter == player_ship) {
                    assert_eq!(shell.bullet, "Bullet_Lancer_Light", "only the front Lancer should fire at base towers");
                    lancer_fired = true;
                }
                if app.world().get::<Health>(hostile).unwrap().sections[tower] == 0.0 {
                    break;
                }
            }
            assert_eq!(
                app.world().get::<Health>(hostile).unwrap().sections[tower],
                0.0,
                "Mission 2's torpedo boat must actually destroy {}",
                lib.classes[class].sections[tower].label
            );
        }
        assert!(lancer_fired, "the front Lancer must engage the selected towers");
        assert_eq!(base_mission_damage(&lib.classes[class], app.world().get::<Health>(hostile).unwrap()), 1.0);
    }

    #[test]
    fn ship_hit_rules() {
        let lib = Library::load(&assets_dir());
        let class = &lib.classes[lib.index("Ship_Navy_Cutter")];
        let stern = class.sections.iter().position(|s| s.vital && s.core_pct == 45.0).unwrap();
        let sure_fire = [10.0, 10.0, 0.0, 1.0, 3.0];
        let mut rng = Rng::new(7);
        // Invulnerable: no damage, no fire, no core loss.
        let mut hp = Health::new(class);
        let out = apply_ship_hit(class, &mut hp, stern, sure_fire, 1.0, true, &mut rng);
        assert!(out.fire.is_none() && !out.lost && hp.sections[stern] == class.sections[stern].hp && hp.core == class.core_hp);
        // A live section takes damage and starts one fire, but not a second while one burns there.
        let out = apply_ship_hit(class, &mut hp, stern, sure_fire, 0.0, true, &mut rng);
        assert_eq!((out.fire, out.lost, hp.sections[stern]), (Some(3.0), false, class.sections[stern].hp - 10.0));
        assert!(apply_ship_hit(class, &mut hp, stern, sure_fire, 0.0, false, &mut rng).fire.is_none());
        // The killing hit reports the loss, and a hit on the destroyed vital section bleeds the core (10 x 45% = 4.5, rounded).
        hp.sections[stern] = 5.0;
        let out = apply_ship_hit(class, &mut hp, stern, sure_fire, 0.0, true, &mut rng);
        assert!(out.lost && out.fire.is_none() && hp.sections[stern] == 0.0);
        let core = hp.core;
        let out = apply_ship_hit(class, &mut hp, stern, sure_fire, 0.0, true, &mut rng);
        assert!(!out.lost && hp.core < core);
    }

    #[test]
    fn segment_distance_clamps_to_the_ends() {
        let (a, b) = (Vec3::ZERO, Vec3::X * 10.0);
        assert_eq!(segment_distance(Vec3::new(5.0, 3.0, 0.0), a, b), 3.0);
        assert_eq!(segment_distance(Vec3::new(-4.0, 3.0, 0.0), a, b), 5.0);
        assert_eq!(segment_distance(Vec3::Y, a, a), 1.0);
    }

    #[test]
    fn core_bleed_scales_with_destroyed_vital_sections() {
        let lib = Library::load(&assets_dir());
        let class = &lib.classes[lib.index("Ship_Navy_Cutter")];
        let mut hp = Health::new(class);
        assert_eq!(core_bleed(class, &hp, 40.0), 0.0);
        let stern = class.sections.iter().position(|s| s.vital && s.core_pct == 45.0).unwrap();
        hp.sections[stern] = 0.0;
        assert_eq!(core_bleed(class, &hp, 40.0), 18.0); // 40 x 45%
    }

    #[test]
    fn bank_recharge_follows_the_slowest_live_gun() {
        let lib = Library::load(&assets_dir());
        let ci = lib.index("Ship_Navy_Cutter");
        let class = &lib.classes[ci];
        let bank = class.mounts[0].bank;
        let gun = |name: &str, cooldown| GunState {
            gun: name.into(),
            root: Entity::PLACEHOLDER,
            cooldown,
            burst_left: 0,
            burst_timer: 0.0,
            base: Entity::PLACEHOLDER,
            barrel: Entity::PLACEHOLDER,
            muzzles: vec![],
            next_muzzle: 0,
            yaw: 0.0,
            pitch: 0.0,
            aligned: false,
            in_range: false,
            dev: 0.0,
        };
        let health = Health::new(class);
        let mut g = GunStates(class.mounts.iter().map(|m| gun(&m.gun, 0.0)).collect());
        assert_eq!(bank_recharge(&lib, &g, class, &health, bank), 1.0);
        let reload = lib.guns[&class.mounts[0].gun].reload;
        g.0[0].cooldown = reload / 4.0;
        assert!((bank_recharge(&lib, &g, class, &health, bank) - 0.75).abs() < 1e-4);
        assert_eq!(bank_recharge(&lib, &g, class, &health, 99), 0.0, "no live gun in the bank");
    }
    #[test]
    fn every_gun_has_a_panel_icon_and_pd_is_hidden() {
        let hud: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(assets_dir().join("hud/hud.json")).unwrap()).unwrap();
        let (g, _) = load_weapons(&assets_dir());
        for name in g.keys() {
            assert!(hud.get(gun_icon(name)).is_some(), "{name} -> {} missing", gun_icon(name));
        }
        assert!(is_indirect("Gun_NovaMortar") && !is_indirect("Gun_CannonBall_Med"));
        assert!(g["Gun_Lancer_PD_Light"].point_defense);
    }
    #[test]
    fn port_and_starboard_sections_sit_on_their_own_side() {
        let (g, _) = load_weapons(&assets_dir());
        for name in ["Ship_Pirate_Schooner", "Ship_Navy_Frigate", "Ship_Navy_Cutter"] {
            let c = load_class(&assets_dir(), name, &g);
            for s in &c.sections {
                let cx = (s.min.x + s.max.x) / 2.0;
                println!("{name} {} x {:.1}..{:.1}", s.label, s.min.x, s.max.x);
                if s.label.to_lowercase().contains("port") && !s.label.to_lowercase().contains("starboard") {
                    assert!(cx < 0.0, "{name} {} on the wrong side", s.label);
                }
                if s.label.to_lowercase().contains("starboard") {
                    assert!(cx > 0.0, "{name} {} on the wrong side", s.label);
                }
            }
        }
    }
    #[test]
    fn slow_projectile_intercepts_crossing_and_receding_targets() {
        let d = Vec3::new(0.0, 0.0, -100.0);
        for velocity in [Vec3::ZERO, Vec3::X * 10.0, Vec3::NEG_Z * 10.0, Vec3::Z * 22.0] {
            let t = intercept_time(d, velocity, 22.0).unwrap();
            assert!(((d + velocity * t).length() - 22.0 * t).abs() < 0.001);
        }
        assert_eq!(intercept_time(d, Vec3::NEG_Z * 30.0, 22.0), None);
        assert_eq!(intercept_time(Vec3::ZERO, Vec3::ZERO, 22.0), Some(0.0));
    }
    #[test]
    fn point_defence_wears_a_torpedo_down_instead_of_one_shot_kills() {
        let lib = Library::load(&crate::assets_dir());
        let torpedo = lib.bullets["Bullet_Torpedo"].hitpoints;
        let bolts = |gun: &str| {
            let bolt = lib.bullets[&lib.guns[gun].bullet].hitpoints;
            let mut hp = torpedo;
            (1..).find(|_| wear_down(&mut hp, bolt)).unwrap()
        };
        assert_eq!(torpedo, 16.0);
        assert_eq!(bolts("Gun_Lancer_PD_Light"), 16, "light PD bolts (1 HP) need sixteen hits");
        assert_eq!(bolts("Gun_Lancer_PD_Med"), 8, "medium PD bolts (2 HP) need eight hits");
    }

    #[test]
    fn torpedo_guidance_launch_coast_and_moving_target_intercept() {
        let lib = Library::load(&assets_dir());
        let physics = lib.bullets["Bullet_Torpedo"].physics.as_ref().unwrap();
        assert!((physics.max_speed - 20.833334).abs() < 0.001);
        let (mut p, mut v, mut spin) = (Vec3::ZERO, Vec3::NEG_Z * 22.0, 0.0);
        let target_class = &lib.classes[lib.index("Ship_Pirate_Schooner")];
        let target_rotation = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2);
        let mut target = Vec3::new(15.0, target_class.homing_point.y, -80.0);
        let target_velocity = Vec3::X * 4.0;
        p = guide_torpedo(p, &mut v, &mut spin, 0.0, Some((target, target_velocity)), physics, 0.5);
        assert_eq!(p.x, 0.0, "no steering during launch coast");
        assert_eq!(p.y, 0.0);
        let mut closest = f32::INFINITY;
        let mut hit = false;
        for i in 0..900 {
            let dt = 1.0 / 60.0;
            target += target_velocity * dt;
            let previous = p;
            p = guide_torpedo(p, &mut v, &mut spin, 0.5 + i as f32 * dt, Some((target, target_velocity)), physics, dt);
            let inverse = Transform::from_translation(target - target_rotation * target_class.homing_point).with_rotation(target_rotation).to_matrix().inverse();
            hit |= target_class.sections.iter().any(|s| seg_mesh(inverse.transform_point3(previous), inverse.transform_point3(p), s).is_some());
            closest = closest.min(p.distance(target));
            assert!(p.is_finite() && v.is_finite() && spin.is_finite());
        }
        assert!(hit, "torpedo must intersect the moving target's authored hull; closest homing-point distance: {closest}");
        let old_y = p.y;
        let next = guide_torpedo(p, &mut v, &mut spin, 16.0, None, physics, 0.1);
        assert_eq!(next.y, old_y, "lost target must stop altitude tracking");
    }

    #[test]
    fn model_hardpoints_are_loaded_in_bevy_axes() {
        let lib = Library::load(&assets_dir());
        let class = &lib.classes[lib.index("Ship_Navy_Torpedo")];
        assert!((class.homing_point.y - 0.2275).abs() < 0.001);
        assert!(class.tow_points[0].z > 0.0 && class.tow_points[1].z < 0.0);
    }

    #[test]
    fn torpedo_aim_and_launch_do_not_inherit_shooter_motion() {
        let offset = Vec3::new(15.0, 3.0, -80.0);
        let target_velocity = Vec3::X * 5.0;
        let aimed = aim_offset(offset, target_velocity, 22.0, true);
        let velocity = launch_velocity(aimed.normalize(), 22.0, true);
        assert!((velocity.length() - 22.0).abs() < 1e-5);
        assert_eq!(velocity.y, 0.0);
        let time = intercept_time(offset.with_y(0.0), target_velocity, 22.0).unwrap();
        assert!((velocity * time - (offset.with_y(0.0) + target_velocity * time)).length() < 1e-4);
        let unreachable = aim_offset(offset, Vec3::NEG_Z * 30.0, 22.0, true);
        assert_eq!(unreachable, offset.with_y(0.0), "unreachable targets do not produce runaway lead");
    }

    /// Retail damage control (refit-resupply.md#crew-damage-control): riggers mend the most damaged sail +3 per successful
    /// roll; engineers mend the most damaged hull by floor(10 x mean engineering x material factor).
    #[test]
    fn crew_damage_control_follows_riggers_and_engineers() {
        let lib = Library::load(&assets_dir());
        let class = &lib.classes[lib.index("Ship_Navy_ManOWar")];
        let full: Vec<f32> = class.sections.iter().map(|s| s.hp).collect();
        let sails: Vec<usize> = (0..full.len()).filter(|&i| class.sections[i].material.contains("Cloth")).collect();
        let hulls: Vec<usize> = (0..full.len()).filter(|i| !sails.contains(i)).collect();
        let mut hp = full.clone();
        hp[sails[0]] -= 30.0;
        hp[sails[1]] *= 0.5; // most damaged sail
        hp[sails[2]] = 0.0; // destroyed sails stay down
        hp[hulls[1]] -= 50.0;
        hp[hulls[2]] *= 0.5; // most damaged hull
        let mut always = || 0.0;
        // Run 10: eight riggers averaging 8.25 of 10, seven successes -> +21 on one sail.
        damage_control(class, &mut hp, &[], &[0.9, 0.9, 0.8, 0.8, 0.8, 0.8, 0.8, 0.0], &mut always);
        assert_eq!(hp[sails[1]], full[sails[1]] * 0.5 + 21.0);
        assert_eq!((hp[sails[0]], hp[sails[2]]), (full[sails[0]] - 30.0, 0.0));
        // Run 9: engineers 9, 7, 7 -> c = 0.767 -> +7 on the most damaged wood section.
        damage_control(class, &mut hp, &[0.9, 0.7, 0.7], &[], &mut always);
        assert_eq!(hp[hulls[2]], full[hulls[2]] * 0.5 + 7.0);
        assert_eq!(hp[hulls[1]], full[hulls[1]] - 50.0);
        // A failed roll mends nothing; repairs stop at the maximum.
        let before = hp.clone();
        damage_control(class, &mut hp, &[0.5], &[0.5], &mut || 0.99);
        assert_eq!(hp, before);
        let mut nearly = full.clone();
        nearly[sails[3]] -= 1.0;
        damage_control(class, &mut nearly, &[], &[1.0; 8], &mut always);
        assert_eq!(nearly, full);
    }

    /// Retail shells (docs/re/gunnery.md#shell-gravity): gravity MuzzleSpeed^2 / (sqrt(2) * MaximumRange) per gun, launch speed
    /// MuzzleSpeed in the world frame whatever the shooter's motion, lead from the target's velocity alone.
    #[test]
    fn shells_match_measured_retail_ballistics() {
        let (guns, _) = load_weapons(&assets_dir());
        // gun, measured gravity (median of repeated retail shells)
        for (gun, measured) in [
            ("Gun_StarMortar", 1.263),
            ("Gun_CannonBall_Heavy", 3.231),
            ("Gun_CannonBall_Med", 4.184),
            ("Gun_Carronade_Med", 4.908),
            ("Gun_CannonBall_Light", 6.269),
            ("Gun_Carronade_Light", 6.355),
            ("Gun_Gattling_Light", 28.5),
        ] {
            let g = guns[gun].shell_gravity();
            assert!((g / measured - 1.0).abs() < 0.015, "{gun}: {g} vs measured {measured}");
        }
        assert_eq!(guns["Gun_Lancer_PD_Light"].shell_gravity(), 0.0, "lancers fly straight");
        // Burst cycle (start to start): gattling 3 s + 7 x 0.15 s, measured 3.98-4.17 s; medium cannon 9.0 s, measured 8.96-8.99 s.
        assert!((guns["Gun_Gattling_Light"].burst_cycle() - 4.05).abs() < 1e-4);
        assert_eq!(guns["Gun_CannonBall_Med"].burst_cycle(), 9.0);
        let v = launch_velocity(Vec3::NEG_Z, 40.0, false);
        assert_eq!(v, Vec3::NEG_Z * 40.0, "no shooter velocity added");
        let d = Vec3::new(0.0, 0.0, -100.0);
        assert_eq!(aim_offset(d, Vec3::X * 5.0, 40.0, false), d + Vec3::X * 12.5);
        // A medium cannon aimed by `aim` lands on a point 200 units off at the measured gravity.
        let g = guns["Gun_CannonBall_Med"].shell_gravity();
        let target = Vec3::new(120.0, -3.0, -160.0);
        let (mut p, mut vel, dt) = (Vec3::ZERO, aim(target, 40.0, g) * 40.0, 0.0005);
        while Vec2::new(p.x, p.z).length() < Vec2::new(target.x, target.z).length() {
            vel.y -= g * dt;
            p += vel * dt;
        }
        assert!(p.distance(target) < 0.3, "landed {p} for {target}");
    }

    #[test]
    fn retail_torpedo_arc_body_and_hull_collision() {
        let (guns, bullets) = load_weapons(&assets_dir());
        let torpedo = load_class(&assets_dir(), "Ship_Navy_Torpedo", &guns);
        let launcher = torpedo.mounts.iter().find(|m| m.gun == "Gun_Torpedo").unwrap();
        assert!((launcher.arc / 2.0 - 22.5f32.to_radians()).abs() < 0.00001);
        assert!(10.0f32.to_radians() < launcher.arc / 2.0);
        let glb = bullets["Bullet_Torpedo"].glb.as_ref().unwrap();
        assert!(assets_dir().join(crate::data::model(glb)).is_file());
        let target = load_class(&assets_dir(), "Ship_Pirate_Schooner", &guns);
        let start = target.centre + Vec3::Z * 100.0;
        assert!(target.sections.iter().any(|s| seg_mesh(start, target.centre, s).is_some()));
    }
}
