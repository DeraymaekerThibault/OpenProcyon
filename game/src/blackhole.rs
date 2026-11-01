//! Black holes (Terrain_BlackHole_*): the funnel mesh, the BlackHoleRibbons effect's particle systems (via fx.rs) and its
//! `Vortex` primitive, which fx.rs cannot run. Behaviour as specified in docs/re/effects.md "Vortex"
//! (point spawn and update, trail segment init and update).
//! Everything ticks at the engine's 30 Hz. PROVISIONAL: ribbon texture mapping/blend and the point-effect orientation's
//! normalisation (unknown). Pull (`Magnitude`) and the core kill: see `pull` below, from retail measurements (docs/re/effects.md).
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::docking::Docker;
use crate::{Health, Scenario, Ship, assets_dir, fx};

const TICK: f32 = 1.0 / 30.0;
const SEG_LIFE: i32 = 300; // trail segment lifetime in ticks
const SEG_EVERY: i32 = 15; // ticks between trail segments
/// Full-depth pull of a Magnitude 9000 hole = CLASS_GAIN x hull length / Mass (units/s²). Fits the Small hole's measured Cutter
/// 4.50, Frigate 1.21-1.26 and Man-O-War 0.99-1.04 within ~4 %. PROVISIONAL: a candidate law from three classes.
pub const CLASS_GAIN: f32 = 1330.0;
/// The pull is turned this far off the line to the centre (clockwise seen from above): sideways / inward = 0.27, the same in
/// every measured creep and fall, whatever the ship's heading.
const DRIFT_SWIRL: f32 = 0.27;
/// A resting ship breaks free of its rest snap and falls once the pull reaches this (units/s²). PROVISIONAL: both retail Cutter
/// runs broke free at 2.9-3.0 while a Frigate (1.26 at most) and a Man-O-War (1.04) never did; time spent inside is not ruled out.
const BREAK_FREE: f32 = 2.9;
/// Seconds a ship that is not under way survives inside the hole's core (measured ~1.1 s for a Cutter and a Frigate).
pub const KILL_DELAY: f32 = 1.1;

/// A hole's CUSTOMINFO `Magnitude` and `Radius` (Terrain_BlackHole_*.wot; retail values 9000/145, 15000/260, 25000/625).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Pull {
    pub magnitude: f32,
    pub radius: f32,
}

/// Set on a ship the hole destroyed: the Game Summary credits nobody with it (retail: Ships Sunk stayed 0).
#[derive(Component)]
pub struct Swallowed;

/// A ship's state inside a hole's radius: its own velocity once it broke free of the rest snap, and how long it has been in the
/// core while not under way.
#[derive(Component, Default, Debug, Clone, Copy)]
pub struct Caught {
    pub free: bool,
    pub velocity: Vec3,
    pub inside: f32,
}

impl Pull {
    /// Retail values for a WOT type, or None for other terrain.
    pub fn for_terrain(terrain: &str) -> Option<Pull> {
        let (magnitude, radius) = match terrain.rsplit('_').next()? {
            "Small" => (9000.0, 145.0),
            "Med" => (15000.0, 260.0),
            "Big" => (25000.0, 625.0),
            _ => return None,
        };
        Some(Pull { magnitude, radius })
    }

    /// The core: inside this distance the pull stops and a ship not under way is destroyed. A quarter of the radius (measured
    /// 36.5 ± 0.5 on the Small hole; PROVISIONAL for the larger holes).
    pub fn core(&self) -> f32 {
        self.radius / 4.0
    }

    /// Full-depth pull (units/s²) on a hull of `length` (collision box, bow to stern) and `mass`. PROVISIONAL: the class law
    /// (`CLASS_GAIN`) and the linear Magnitude scaling (only the Small hole was measured).
    pub fn strength(&self, length: f32, mass: f32) -> f32 {
        CLASS_GAIN * length / mass.max(1.0) * self.magnitude / 9000.0
    }

    /// Pull (units/s²) at `distance`: zero outside the radius and inside the core; in between it rises linearly with the depth
    /// inside the edge over one core width (measured ramp 36 units on the Small hole), then stays at `strength`.
    pub fn accel(&self, distance: f32, length: f32, mass: f32) -> f32 {
        if distance > self.radius || distance <= self.core() {
            return 0.0;
        }
        self.strength(length, mass) * ((self.radius - distance) / self.core()).min(1.0)
    }
}

/// Numbers of the effect's Vortex primitive plus the Wake_Hole trail it attaches to every point.
#[derive(Clone, Default)]
struct Params {
    spawn: f32,
    start: f32,
    kill: f32,
    rdelta: f32,
    ang0: f32,
    ang_add: f32,
    z_off: f32,
    z_add: f32,
    up: f32,
    high: f32,
    back: f32,
    out: f32,
    width: f32,
}

struct Seg {
    a: Vec3,
    b: Vec3,
    out: Vec3,
    back: Vec3,
    life: i32,
    alpha: f32,
}

struct Point {
    pos: Vec3,
    ang: f32,
    rad: f32,
    dang: f32,
    z7: f32,
    z8: f32,
    cd: i32,
    dead: bool,
    segs: Vec<Seg>,
}

#[derive(Component)]
pub struct BlackHole {
    p: Params,
    pts: Vec<Point>,
    acc: f32,
    rng: u32,
    mesh: Option<Handle<Mesh>>,
}

impl BlackHole {
    fn f(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(0x19660d).wrapping_add(0x3c6ef35f);
        f32::from_bits(self.rng & 0x7fffff | 0x3f800000) - 1.0
    }
    fn jitter(&mut self) -> Vec3 {
        Vec3::new((self.f() * 2.0 - 1.0) * 0.07, (self.f() * 2.0 - 1.0) * 0.07, (self.f() * 2.0 - 1.0) * 0.02)
    }

    /// One engine tick, in hole-relative game axes (x starboard, y forward, z up).
    fn tick(&mut self) {
        let p = self.p.clone();
        if self.f() < p.spawn {
            let ang = (self.f() * 2.0 - 1.0) * std::f32::consts::PI;
            self.pts.push(Point {
                pos: Vec3::new(ang.cos(), ang.sin(), 0.0) * p.start - Vec3::Z * p.z_off,
                ang,
                rad: p.start,
                dang: p.ang0,
                z7: 0.0,
                z8: 0.0,
                cd: 0,
                dead: false,
                segs: vec![],
            });
        }
        let mut pts = std::mem::take(&mut self.pts);
        for pt in &mut pts {
            if !pt.dead {
                if pt.rad < p.kill {
                    pt.dead = true;
                }
                let old = pt.pos;
                pt.dang += p.ang_add;
                pt.rad -= p.rdelta;
                pt.ang += pt.dang;
                pt.z8 += p.z_add;
                pt.z7 += pt.z8;
                pt.pos = Vec3::new(pt.ang.cos(), pt.ang.sin(), 0.0) * pt.rad + Vec3::Z * (old.z - pt.z7);
                pt.cd -= 1;
                if pt.cd < 1 {
                    pt.cd = SEG_EVERY;
                    // The point's frame: forward = back toward where it was, up leans toward the centre and rises with height above the funnel.
                    let fwd = (old - pt.pos).try_normalize().unwrap_or(Vec3::Y);
                    let f = (1.0 - (-pt.pos.z).min(p.up) / p.up).clamp(0.0, 1.0);
                    let up = Vec3::new(-pt.pos.x, -pt.pos.y, f * p.high + p.up);
                    let right = fwd.cross(up).try_normalize().unwrap_or(Vec3::X);
                    let c = right * p.out;
                    pt.segs.push(Seg { a: pt.pos + c * p.width, b: pt.pos - c * p.width, out: c, back: fwd * p.back, life: SEG_LIFE, alpha: 0.0 });
                }
            }
            for i in 0..pt.segs.len() {
                let (ja, jb) = (self.jitter(), self.jitter());
                let s = &mut pt.segs[i];
                s.life -= 1;
                s.a += s.out + s.back + ja;
                s.b += -s.out + s.back + jb;
                s.alpha = if SEG_LIFE - s.life < 60 { (s.alpha + 1.0 / 60.0).min(1.0) } else { (s.alpha - 1.0 / 240.0).max(0.0) };
            }
            pt.segs.retain(|s| s.life > 0);
        }
        // A dead point takes its trail with it (the original frees the point's effect).
        pts.retain(|pt| !pt.dead);
        self.pts = pts;
    }
}

fn load(kind: &str) -> Option<Params> {
    let read = |n: &str| crate::data::try_json(&assets_dir().join(format!("fx/{n}.json")));
    let eft = read(&format!("BlackHoleRibbons_{kind}"))?;
    let v = eft["Primitive"].as_array()?.iter().find(|p| p["VORTEX_POINT_EFFECT_NAME"].is_string())?.clone();
    let wake = read(v["VORTEX_POINT_EFFECT_NAME"].as_str()?)?;
    let t = |k: &str| v[k].as_f64().unwrap_or(0.0) as f32;
    let w = |k: &str| wake["Primitive"][0][k].as_f64().unwrap_or(0.0) as f32;
    Some(Params {
        spawn: t("SPAWN_PROBABILITY"),
        start: t("START_RADIUS"),
        kill: t("KILL_RADIUS"),
        rdelta: t("RADIUS_DELTA"),
        ang0: t("ANGLE_DELTA_START"),
        ang_add: t("ANGLE_DELTA_CHANGE"),
        z_off: t("CENTER_Z_OFFSET"),
        z_add: t("Z_DELTA_DELTA_ADD") * 0.001, // the original scales this value by 0.001 on load
        up: t("UP_CHANGE"),
        high: t("HIGHT_POINT"),
        back: w("BackDrift"),
        out: w("OutDrift"),
        width: w("InitialWidth"),
    })
}

/// `terrain` is the WOT type ("Terrain_BlackHole_Small" | "_Med" | "_Big").
pub fn spawn(commands: &mut Commands, lib: &fx::FxLib, terrain: &str, pos: Vec3) {
    let kind = match terrain.rsplit('_').next() {
        Some("Med") => "Medium",
        Some("Big") => "Large",
        _ => "Small",
    };
    let Some(p) = load(kind) else { return };
    let mut hole =
        commands.spawn((Scenario, BlackHole { p, pts: vec![], acc: 0.0, rng: 0x1234_5678 ^ pos.x.to_bits(), mesh: None }, Transform::from_translation(pos), Visibility::default()));
    if let Some(pull) = Pull::for_terrain(terrain) {
        hole.insert(pull);
    }
    let e = hole.id();
    fx::spawn(commands, lib, &format!("BlackHoleRibbons_{kind}"), Some(e), Transform::IDENTITY, Transform::from_translation(pos), false);
}

/// Black-hole pull (docs/re/effects.md "Black holes"). It acts only on ships that are not under way (engine speed 0); a ship under
/// orders feels nothing. A resting ship's rest snap cancels the pull every engine tick, so it only creeps in at one tick's worth of
/// acceleration; once the pull reaches `BREAK_FREE` it falls freely, its velocity following v' = a - k v with the hull's own rate
/// k = MaxThrust / (MaxSpeed x Mass). In the core the pull stops and a ship not under way is destroyed after `KILL_DELAY`.
/// Docked and destroyed ships are left alone.
pub fn pull(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<crate::combat::Library>,
    holes: Query<(&Transform, &Pull), Without<Ship>>,
    mut ships: Query<(Entity, &Ship, &mut Transform, &mut Health, Option<&Docker>, Option<&mut Caught>)>,
) {
    let dt = time.delta_secs();
    for (e, ship, mut tf, mut health, docker, caught) in &mut ships {
        if health.destroyed || docker.is_some_and(Docker::is_docked) {
            continue;
        }
        let class = &lib.classes[ship.class];
        let (mut accel, mut in_radius, mut in_core) = (Vec3::ZERO, false, false);
        for (hole, pull) in &holes {
            let distance = horizontal(hole.translation, tf.translation).length();
            in_radius |= distance <= pull.radius;
            in_core |= distance <= pull.core();
            accel += pull_dir(hole.translation, tf.translation) * pull.accel(distance, class.length, class.phys.mass);
        }
        let Some(mut caught) = caught else {
            if in_radius {
                commands.entity(e).insert(Caught::default());
            }
            continue;
        };
        let rate = class.phys.thrust / (class.phys.max_speed * class.phys.mass);
        let (step, killed) = caught.step(ship.speed == 0.0, accel, in_core, rate, dt);
        tf.translation += step;
        if killed {
            health.core = 0.0; // combat::deaths sinks it next
            commands.entity(e).insert(Swallowed);
        }
    }
}

impl Caught {
    /// One frame for a ship that is `resting` (engine speed 0) or under way, pulled by `accel` (units/s²) and with hull rate `rate`
    /// (1/s): its displacement, and whether the hole destroys it now.
    pub fn step(&mut self, resting: bool, accel: Vec3, in_core: bool, rate: f32, dt: f32) -> (Vec3, bool) {
        self.inside = if in_core && resting { self.inside + dt } else { 0.0 };
        if !resting {
            // Under way the engine owns the motion: no pull, and a fall ends.
            (self.free, self.velocity) = (false, Vec3::ZERO);
            return (Vec3::ZERO, false);
        }
        if !self.free && accel.length() >= BREAK_FREE {
            self.free = true;
            self.velocity = accel * TICK;
        }
        let step = if self.free {
            let terminal = accel / rate.max(1e-6);
            self.velocity = terminal + (self.velocity - terminal) * (-rate * dt).exp();
            self.velocity * dt
        } else {
            accel * TICK * dt // the rest snap leaves one tick's worth of the pull
        };
        (step, self.inside >= KILL_DELAY)
    }
}

fn horizontal(hole: Vec3, ship: Vec3) -> Vec3 {
    Vec3::new(hole.x - ship.x, 0.0, hole.z - ship.z)
}

/// Unit direction of a hole's pull on a ship at `ship`: toward the centre, turned DRIFT_SWIRL clockwise seen from above.
fn pull_dir(hole: Vec3, ship: Vec3) -> Vec3 {
    let Some(inward) = horizontal(hole, ship).try_normalize() else { return Vec3::ZERO };
    // Bevy is y-up with game forward = -z, so clockwise seen from above is a negative turn about +y.
    Quat::from_rotation_y(-DRIFT_SWIRL.atan()) * inward
}

pub fn update(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut holes: Query<(Entity, &mut BlackHole)>,
) {
    for (e, mut h) in &mut holes {
        if h.mesh.is_none() {
            let m = meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
            let mat = mats.add(StandardMaterial {
                base_color_texture: Some(assets.load("fx/tex/fireblackhole.png")),
                unlit: true,
                cull_mode: None,
                double_sided: true,
                alpha_mode: AlphaMode::Add,
                ..default()
            });
            commands.entity(e).insert((Mesh3d(m.clone()), MeshMaterial3d(mat), NoFrustumCulling));
            h.mesh = Some(m);
        }
        h.acc += time.delta_secs().min(0.25);
        while h.acc >= TICK {
            h.acc -= TICK;
            h.tick();
        }
        let (mut pos, mut uv, mut col, mut idx) = (vec![], vec![], vec![], vec![]);
        for pt in &h.pts {
            if pt.segs.len() < 3 {
                continue; // the original draws a trail only once it has more than two segments
            }
            let base = pos.len() as u32;
            for (i, s) in pt.segs.iter().enumerate() {
                let v = i as f32 * 0.5;
                pos.extend([s.a, s.b].map(|g| [g.x, g.z, -g.y]));
                uv.extend([[0.0, v], [1.0, v]]);
                col.extend([[1.0, 1.0, 1.0, s.alpha]; 2]);
                if i > 0 {
                    let b = base + (i * 2) as u32;
                    idx.extend([b - 2, b - 1, b, b - 1, b + 1, b]);
                }
            }
        }
        if let Some(mut mesh) = h.mesh.as_ref().and_then(|m| meshes.get_mut(m)) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
            mesh.insert_indices(Indices::U32(idx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> Pull {
        Pull::for_terrain("Terrain_BlackHole_Small").unwrap()
    }
    // Hull length (collision box) and Mass of the measured classes, and the hull rate MaxThrust / (MaxSpeed x Mass) of the Cutter.
    const CUTTER: (f32, f32) = (13.5, 4000.0);
    const FRIGATE: (f32, f32) = (18.0, 20000.0);
    const MAN_O_WAR: (f32, f32) = (31.0, 40000.0);
    const CUTTER_RATE: f32 = 1.862;

    /// Runs a ship standing still at `distance` from a Small hole for `secs`: (inward distance covered, whether it broke free).
    fn rest(hull: (f32, f32), distance: f32, secs: f32) -> (f32, Caught) {
        let pull = small();
        let mut c = Caught::default();
        let mut pos = Vec3::new(distance, 0.0, 0.0);
        for _ in 0..(secs * 30.0).round() as usize {
            let d = pos.length();
            let a = pull_dir(Vec3::ZERO, pos) * pull.accel(d, hull.0, hull.1);
            pos += c.step(true, a, d <= pull.core(), CUTTER_RATE, TICK).0;
        }
        (distance - pos.length(), c)
    }

    #[test]
    fn pull_law_matches_the_retail_small_hole() {
        let pull = small();
        assert_eq!(pull, Pull { magnitude: 9000.0, radius: 145.0 });
        assert_eq!(Pull::for_terrain("Terrain_Island"), None);
        // measured full-depth pull per class: Cutter 4.50, Frigate 1.21-1.26, Man-O-War 0.99-1.04
        for (hull, measured) in [(CUTTER, 4.50), (FRIGATE, 1.235), (MAN_O_WAR, 1.015)] {
            let a = pull.strength(hull.0, hull.1);
            assert!((a / measured - 1.0).abs() < 0.05, "{hull:?}: {a} vs {measured}");
        }
        // nothing outside the radius (retail: Man-O-War at 200-203 for 6 min, Cutter at 372) nor inside the core (36.5 measured)
        assert_eq!(pull.accel(200.0, MAN_O_WAR.0, MAN_O_WAR.1), 0.0);
        assert_eq!(pull.accel(372.0, CUTTER.0, CUTTER.1), 0.0);
        assert_eq!(pull.accel(36.0, CUTTER.0, CUTTER.1), 0.0);
        assert!((pull.core() - 36.5).abs() <= 0.5);
        // linear ramp over the first ~36 units, then constant (Man-O-War: the same creep at 105 and 48 units)
        let full = pull.strength(CUTTER.0, CUTTER.1);
        assert!((pull.accel(145.0 - 18.125, CUTTER.0, CUTTER.1) - full / 2.0).abs() < 1e-4);
        assert_eq!(pull.accel(105.0, MAN_O_WAR.0, MAN_O_WAR.1), pull.accel(48.0, MAN_O_WAR.0, MAN_O_WAR.1));
        // Magnitude scales the pull (PROVISIONAL)
        let big = Pull::for_terrain("Terrain_BlackHole_Big").unwrap();
        assert!((big.strength(CUTTER.0, CUTTER.1) / full - 25.0 / 9.0).abs() < 1e-4);
    }

    #[test]
    fn a_resting_ship_creeps_one_tick_of_pull_per_second() {
        // retail: Cutter 0.0908 units/s at depth 22.9 inside the edge, Frigate 0.0398 and Man-O-War 0.0328 at full depth
        for (hull, distance, measured) in [(CUTTER, 145.0 - 22.9, 0.0908), (FRIGATE, 54.0, 0.0398), (MAN_O_WAR, 105.0, 0.0328)] {
            let (moved, c) = rest(hull, distance, 1.0);
            assert!(!c.free, "{hull:?} stays snapped");
            assert!((moved / measured - 1.0).abs() < 0.06, "{hull:?}: {moved} vs {measured} units/s");
        }
        // a Man-O-War resting 200 s never broke free and stays alive
        let (_, c) = rest(MAN_O_WAR, 105.0, 200.0);
        assert!(!c.free && c.inside == 0.0);
    }

    #[test]
    fn a_ship_under_way_feels_no_pull_and_survives_the_core() {
        let mut c = Caught { free: true, velocity: Vec3::X, inside: 0.0 };
        for _ in 0..17 * 30 {
            let (step, killed) = c.step(false, Vec3::new(-4.5, 0.0, 0.0), true, CUTTER_RATE, TICK);
            assert_eq!((step, killed), (Vec3::ZERO, false));
        }
        assert!(!c.free && c.velocity == Vec3::ZERO, "getting under way ends a fall");
    }

    #[test]
    fn a_deep_cutter_breaks_free_and_falls_at_the_hull_rate() {
        // PROVISIONAL trigger: 2.9 units/s². Below it the Cutter stays snapped; past it, it falls to A / k = 2.42 units/s.
        assert!(!rest(CUTTER, 123.0, 5.0).1.free);
        let pull = small();
        let a = pull_dir(Vec3::ZERO, Vec3::new(90.0, 0.0, 0.0)) * pull.accel(90.0, CUTTER.0, CUTTER.1);
        let mut c = Caught::default();
        for _ in 0..300 {
            c.step(true, a, false, CUTTER_RATE, TICK);
        }
        assert!(c.free);
        assert!((c.velocity.length() - 2.42).abs() < 0.03, "terminal speed {}", c.velocity.length());
        // inside the core the pull stops and the speed decays at the hull rate
        let v0 = c.velocity.length();
        c.step(true, Vec3::ZERO, true, CUTTER_RATE, 0.5);
        assert!((c.velocity.length() - v0 * (-CUTTER_RATE * 0.5).exp()).abs() < 1e-4);
    }

    #[test]
    fn the_core_destroys_a_resting_ship_after_1_1_s() {
        let mut c = Caught::default();
        let mut killed_at = None;
        for i in 1..=60 {
            if c.step(true, Vec3::ZERO, true, CUTTER_RATE, TICK).1 {
                killed_at = Some(i as f32 * TICK);
                break;
            }
        }
        assert!(killed_at.is_some_and(|t| (t - KILL_DELAY).abs() < 0.05), "{killed_at:?}");
        // leaving the core or getting under way resets the clock
        let mut c = Caught { inside: 1.0, ..default() };
        c.step(false, Vec3::ZERO, true, CUTTER_RATE, TICK);
        assert_eq!(c.inside, 0.0);
    }

    #[test]
    fn pull_spirals_clockwise_toward_the_centre_and_is_horizontal() {
        // ship 100 units east of the hole (game +x = bevy +x); the centre is west
        let v = pull_dir(Vec3::ZERO, Vec3::new(100.0, 40.0, 0.0));
        assert!(v.x < 0.0 && v.y == 0.0 && (v.length() - 1.0).abs() < 1e-6, "toward the centre and horizontal, got {v}");
        // clockwise from above while heading west turns north (game +y = bevy -z): 15° off, sideways / inward 0.27
        assert!(v.z < 0.0 && (v.z / v.x - DRIFT_SWIRL).abs() < 1e-3, "sideways part, got {v}");
        assert_eq!(pull_dir(Vec3::ZERO, Vec3::ZERO), Vec3::ZERO, "no direction at the centre");
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn class_data_gives_the_measured_pulls() {
        let lib = crate::combat::Library::load(assets_dir());
        for (class, measured) in [("Ship_Navy_Cutter", 4.50), ("Ship_Navy_Frigate", 1.235), ("Ship_Navy_ManOWar", 1.015)] {
            let c = &lib.classes[lib.index(class)];
            let a = small().strength(c.length, c.phys.mass);
            assert!((a / measured - 1.0).abs() < 0.05, "{class}: {a} vs {measured}");
        }
        let c = &lib.classes[lib.index("Ship_Navy_Cutter")];
        assert!((c.phys.thrust / (c.phys.max_speed * c.phys.mass) - CUTTER_RATE).abs() < 0.01);
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn the_centre_destroys_a_ship_and_credits_nobody() {
        use crate::combat::{Hit, Library};
        use crate::{Side, collision::Drift};
        let mut app = App::new();
        app.add_message::<Hit>()
            .init_resource::<Time>()
            .insert_resource(Library::load(assets_dir()))
            .init_resource::<crate::stats::GameStats>()
            .add_systems(Update, (pull, crate::stats::collect).chain());
        app.world_mut().spawn((Transform::default(), small()));
        let ship = |app: &mut App, class: &str, side: Side, at: Vec3| {
            let lib = app.world().resource::<Library>();
            let ci = lib.index(class);
            let c = &lib.classes[ci];
            let health = Health::new(c);
            let ship = Ship {
                class: ci,
                side,
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
                net: 1.0,
                avoid: 1.0,
            };
            app.world_mut().spawn((Transform::from_translation(at), ship, health, Drift::default())).id()
        };
        let gunner = ship(&mut app, "Ship_Pirate_Carrack", Side::Enemy, Vec3::new(300.0, 0.0, 0.0));
        let victim = ship(&mut app, "Ship_Navy_ManOWar", Side::Player, Vec3::new(100.0, 0.0, 0.0));
        app.update(); // stats registers both hulls; the victim is caught
        app.world_mut().write_message(Hit { shooter: gunner, victim });
        app.world_mut().get_mut::<Health>(victim).unwrap().sections[0] -= 50.0; // the Carrack's last hit
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.05));
        app.update();
        let before = app.world().get::<Transform>(victim).unwrap().translation;
        assert!(before.x < 100.0 && app.world().get::<Health>(victim).unwrap().core > 0.0, "creeping in, still alive: {before}");
        app.world_mut().get_mut::<Transform>(victim).unwrap().translation = Vec3::new(30.0, 0.0, 0.0);
        for _ in 0..20 {
            app.update(); // 1.0 s in the core
        }
        assert!(app.world().get::<Health>(victim).unwrap().core > 0.0, "the core takes ~1.1 s");
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(app.world().get::<Health>(victim).unwrap().core, 0.0, "after 1.1 s in the core the hull goes");
        assert!(app.world().get::<Swallowed>(victim).is_some());
        let far = app.world().get::<Transform>(gunner).unwrap().translation;
        assert_eq!(far, Vec3::new(300.0, 0.0, 0.0), "outside the radius nothing moves");
        // combat::deaths would set `destroyed`; the summary then shows the loss but no kill and no extra damage
        app.world_mut().get_mut::<Health>(victim).unwrap().destroyed = true;
        app.update();
        let stats = app.world().resource::<crate::stats::GameStats>();
        let (enemy, player) = (&stats.players["Enemy fleet"], &stats.players["Your fleet"]);
        assert_eq!((player.end, enemy.sunk), (0, 0));
        assert_eq!((enemy.inflicted, player.taken), (50.0, 50.0), "only the gunfire counts");
    }

    fn hole() -> BlackHole {
        BlackHole { p: load("Small").expect("effect JSON"), pts: vec![], acc: 0.0, rng: 7, mesh: None }
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn small_hole_params_match_the_effect_file() {
        let p = hole().p;
        assert_eq!((p.start, p.kill, p.rdelta, p.out, p.width), (150.0, 10.0, 0.45, 0.2, 50.0));
        assert!((p.z_add - 1e-5).abs() < 1e-9);
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn points_spiral_in_die_and_trails_fade() {
        let mut h = hole();
        for _ in 0..3000 {
            h.tick();
            assert!(h.pts.iter().all(|p| p.rad >= h.p.kill - h.p.rdelta && p.segs.iter().all(|s| (0.0..=1.0).contains(&s.alpha))));
        }
        assert!(!h.pts.is_empty() && h.pts.len() < 40, "steady stream of points, got {}", h.pts.len());
        // every point is older than one segment interval only if it has laid segments
        assert!(h.pts.iter().any(|p| p.segs.len() >= 3));
    }
}
