//! Black holes (Terrain_BlackHole_*): the funnel mesh, the BlackHoleRibbons effect's particle systems (via fx.rs) and its
//! `Vortex` primitive, which fx.rs cannot run. Behaviour as specified in docs/re/effects.md "Vortex"
//! (point spawn and update, trail segment init and update).
//! Everything ticks at the engine's 30 Hz. PROVISIONAL: ribbon texture mapping/blend and the point-effect orientation's
//! normalisation (unknown). Pull (`Magnitude`): see `pull` below; the force law is PROVISIONAL.
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use serde_json::Value;

use crate::collision::Drift;
use crate::docking::Docker;
use crate::{Health, Scenario, Ship, assets_dir, fx};

const TICK: f32 = 1.0 / 30.0;
const SEG_LIFE: i32 = 300; // trail segment lifetime in ticks
const SEG_EVERY: i32 = 15; // ticks between trail segments
/// PROVISIONAL pull gain: acceleration at the hole's radius = PULL_GAIN * Magnitude / radius^2 (the original's law is unknown).
const PULL_GAIN: f32 = 25.0;

/// A hole's CUSTOMINFO `Magnitude` and `Radius` (Terrain_BlackHole_*.wot; retail values 9000/145, 15000/260, 25000/625).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Pull {
    pub magnitude: f32,
    pub radius: f32,
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

    /// PROVISIONAL inverse-square pull toward the centre, softened inside the hole's radius; returns the acceleration magnitude.
    pub fn accel(&self, distance: f32) -> f32 {
        PULL_GAIN * self.magnitude / distance.max(self.radius).powi(2)
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
    let read = |n: &str| serde_json::from_str::<Value>(&std::fs::read_to_string(assets_dir().join(format!("fx/{n}.json"))).ok()?).ok();
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

/// Pulls every free ship toward each black hole: the acceleration is added to the ship's lateral drift (the same velocity that
/// contact uses), so the sail step's drag still damps it. Docked and destroyed ships are not pulled. Runs before `collision::sail`.
pub fn pull(time: Res<Time>, holes: Query<(&Transform, &Pull)>, mut ships: Query<(&Transform, &mut Drift, &Health, Option<&Docker>), With<Ship>>) {
    let dt = time.delta_secs().min(crate::MAX_SIM_STEP);
    for (tf, mut drift, health, docker) in &mut ships {
        if health.destroyed || docker.is_some_and(Docker::is_docked) {
            continue;
        }
        for (hole, pull) in &holes {
            drift.0 += pull_step(hole.translation, *pull, tf.translation, dt);
        }
    }
}

/// Lateral velocity added to a ship at `ship` by one hole at `hole` over `dt`.
fn pull_step(hole: Vec3, pull: Pull, ship: Vec3, dt: f32) -> Vec3 {
    let to = Vec3::new(hole.x - ship.x, 0.0, hole.z - ship.z);
    let distance = to.length();
    if distance > 0.0 { to / distance * pull.accel(distance) * dt } else { Vec3::ZERO }
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

    #[test]
    fn pull_uses_retail_magnitudes_and_falls_off_outside_the_radius() {
        let small = Pull::for_terrain("Terrain_BlackHole_Small").unwrap();
        assert_eq!(small, Pull { magnitude: 9000.0, radius: 145.0 });
        assert_eq!(Pull::for_terrain("Terrain_Island"), None);
        // softened: constant inside the radius, inverse square outside
        assert_eq!(small.accel(10.0), small.accel(145.0));
        assert!((small.accel(290.0) * 4.0 - small.accel(145.0)).abs() < 1e-3);
        // the bigger hole pulls harder at its own radius (its softening radius is larger, so 300 is inside it)
        let big = Pull::for_terrain("Terrain_BlackHole_Big").unwrap();
        assert!(big.accel(625.0) > small.accel(625.0));
    }

    #[test]
    fn pull_points_at_the_centre_and_is_horizontal() {
        let pull = Pull::for_terrain("Terrain_BlackHole_Small").unwrap();
        let v = pull_step(Vec3::ZERO, pull, Vec3::new(300.0, 40.0, 0.0), 0.1);
        assert!(v.x < 0.0 && v.y == 0.0 && v.z.abs() < 1e-6, "pulled toward the centre, got {v}");
        assert!((v.length() - pull.accel(300.0) * 0.1).abs() < 1e-4);
        assert_eq!(pull_step(Vec3::ZERO, pull, Vec3::ZERO, 0.1), Vec3::ZERO, "no direction at the centre");
    }

    fn hole() -> BlackHole {
        BlackHole { p: load("Small").expect("effect JSON"), pts: vec![], acc: 0.0, rng: 7, mesh: None }
    }

    #[test]
    fn small_hole_params_match_the_effect_file() {
        let p = hole().p;
        assert_eq!((p.start, p.kill, p.rdelta, p.out, p.width), (150.0, 10.0, 0.45, 0.2, 50.0));
        assert!((p.z_add - 1e-5).abs() < 1e-9);
    }

    #[test]
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
