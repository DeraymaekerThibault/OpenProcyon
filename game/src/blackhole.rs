//! Black holes (Terrain_BlackHole_*): the funnel mesh, the BlackHoleRibbons effect's particle systems (via fx/) and its
//! `Vortex` primitive, which fx/ cannot run. Behaviour as specified in docs/re/effects.md "Vortex"
//! (point spawn and update, trail segment init and update).
//! Everything ticks at the engine's 30 Hz. PROVISIONAL: ribbon texture mapping/blend and the point-effect orientation's
//! normalisation (unknown). Pull (`Magnitude`) and the core kill: see `pull` below, from retail measurements (docs/re/effects.md).
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::docking::Docker;
use crate::{Health, Scenario, Ship, assets_dir, fx};

use crate::TICK_SECS as TICK;
const SEG_LIFE: i32 = 300; // trail segment lifetime in ticks
const SEG_EVERY: i32 = 15; // ticks between trail segments
/// Full-depth pull of a Magnitude 9000 hole = CLASS_GAIN x hull length / Mass (units/s²). Fits the Small hole's measured Cutter
/// 4.50, Frigate 1.21-1.26 and Man-O-War 0.99-1.04 within ~4 %. PROVISIONAL: a candidate law from three classes.
pub const CLASS_GAIN: f32 = 1330.0;
/// Hull mass the pull law sees at least: the lightest measured class (the Cutter). A retail mission 11 save shows the pinned SS
/// Harvester (`_Feather` galleon, Mass 2000) with no stored pull speed 26 s in, so its full-depth pull is under 9.35, not the law's 13.6;
/// the law is not carried below its fitted range. PROVISIONAL: an upper bound only (a smaller hull term would fit too).
pub const MIN_PULL_MASS: f32 = 4000.0;
/// The pull is turned this far off the line to the centre (clockwise seen from above): sideways / inward = 0.27, the same in
/// every measured creep and fall, whatever the ship's heading.
const DRIFT_SWIRL: f32 = 0.27;
/// A ship breaks free of the snap and is carried off once one tick of pull (at 30 Hz) outruns the integrator's snap of speeds
/// under 0.1 to zero: pull >= 0.1 x 30 = 3.0 units/s² (decoded; both retail Cutter runs broke free at 2.9-3.0, a Frigate and a
/// Man-O-War never reach it on the Small hole).
const BREAK_FREE: f32 = 3.0;
/// Seconds a ship survives inside the hole's core (measured ~1.1 s for a Cutter and a Frigate). The original's
/// code kills at once inside the core; the delay is probably its destruction sequence. PROVISIONAL.
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
/// core.
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
        CLASS_GAIN * length / mass.max(MIN_PULL_MASS) * self.magnitude / 9000.0
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

#[derive(Component, Default)]
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

/// Black-hole pull (docs/re/effects.md "Black holes"). It acts on every ship, under way or not: the pull feeds a velocity of its own
/// beside the engine's. Speeds under 0.1 of that velocity are snapped to zero every engine tick, so a weak pull only creeps the ship
/// in at one tick's worth of acceleration; once the pull reaches `BREAK_FREE` the ship is carried off, that velocity following
/// v' = a - k v with the hull's own rate k = MaxThrust / (MaxSpeed x Mass). In the core the pull stops and the ship is destroyed
/// after `KILL_DELAY`.
/// Docked and destroyed ships are left alone, and so are ships the rules pinned in place ('Set Group/Unit Movable' FALSE): the ships
/// stranded in mission 4's Vanov hole and mission 11's SS Harvester wait there to be towed out (their light hulls feel the pull of
/// `MIN_PULL_MASS`). The rules unpin them once the player comes for them.
pub fn pull(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<crate::combat::Library>,
    holes: Query<(&Transform, &Pull), Without<Ship>>,
    wells: Query<(&Transform, &crate::combat::lob::GravWell), (Without<Ship>, Without<crate::Joining>)>,
    mut ships: Query<(Entity, &Ship, &mut Transform, &mut Health, Option<&Docker>, Option<&mut Caught>, Option<&mut crate::cloak::Cloak>, Has<crate::collision::Pinned>)>,
) {
    let dt = time.delta_secs();
    for (e, ship, mut tf, mut health, docker, caught, mut cloak, pinned) in &mut ships {
        if health.destroyed || pinned || docker.is_some_and(Docker::is_docked) {
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
        // A gravity charge's well goes through the same force helper (decoded), straight at its centre, with no core.
        for (well, w) in &wells {
            let to_centre = horizontal(well.translation, tf.translation);
            in_radius |= to_centre.length() <= w.radius;
            // A cloaked ship inside a gravity charge's well is ordered to decloak every tick (the only projectile that does this).
            if to_centre.length() <= w.radius
                && let Some(c) = cloak.as_deref_mut()
            {
                c.stop();
            }
            accel += to_centre.normalize_or_zero() * CLASS_GAIN * class.length / class.phys.mass.max(MIN_PULL_MASS) * w.force(to_centre.length()) / 9000.0;
        }
        let Some(mut caught) = caught else {
            if in_radius {
                commands.entity(e).insert(Caught::default());
            }
            continue;
        };
        let rate = class.phys.thrust / (class.phys.max_speed * class.phys.mass);
        let (step, killed) = caught.step(accel, in_core, rate, dt);
        tf.translation += step;
        if killed {
            health.core = 0.0; // combat::deaths sinks it next
            commands.entity(e).insert(Swallowed);
        }
    }
}

impl Caught {
    /// One frame for a ship pulled by `accel` (units/s²) with hull rate `rate` (1/s): the displacement the hole adds to whatever the
    /// engine does, and whether the hole destroys it now.
    pub fn step(&mut self, accel: Vec3, in_core: bool, rate: f32, dt: f32) -> (Vec3, bool) {
        self.inside = if in_core { self.inside + dt } else { 0.0 };
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
mod tests;
