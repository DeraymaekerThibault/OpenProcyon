//! Black holes (Terrain_BlackHole_*): the funnel mesh, the BlackHoleRibbons effect's particle systems (via fx.rs) and its
//! `Vortex` primitive, which fx.rs cannot run. Behaviour as specified in docs/re/effects.md "Vortex"
//! (point spawn and update, trail segment init and update).
//! Everything ticks at the engine's 30 Hz. PROVISIONAL: ribbon texture mapping/blend and the point-effect orientation's
//! normalisation (unknown). Pull (`Magnitude`): see `pull` below, from retail measurements (docs/re/effects.md).
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::docking::Docker;
use crate::{Health, Scenario, Ship, assets_dir, fx};

const TICK: f32 = 1.0 / 30.0;
const SEG_LIFE: i32 = 300; // trail segment lifetime in ticks
const SEG_EVERY: i32 = 15; // ticks between trail segments
/// Inward drift inside the radius = DRIFT_GAIN * (Magnitude / 9000) / sqrt(Mass) units/s. Fits the Small hole's measured drifts
/// (Man-O-War 0.033, Cutter 0.093) within 6 %. PROVISIONAL: the mass law and the Magnitude scaling are guesses from two classes.
const DRIFT_GAIN: f32 = 6.2;
/// The drift turns this far clockwise (seen from above) off the line to the centre: sideways ~0.26 x radial, measured.
const DRIFT_SWIRL: f32 = 0.256;

/// A hole's CUSTOMINFO `Magnitude` and `Radius` (Terrain_BlackHole_*.wot; retail values 9000/145, 15000/260, 25000/625), and the
/// distance at which it destroys a ship: its vortex's KILL_RADIUS (PROVISIONAL: retail destroyed a Man-O-War inside 17 units of
/// the Small hole, whose vortex kills at 10; a plunging Cutter went at 33.6).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Pull {
    pub magnitude: f32,
    pub radius: f32,
    pub kill: f32,
}

/// Set on a ship the hole destroyed: the Game Summary credits nobody with it (retail: Ships Sunk stayed 0).
#[derive(Component)]
pub struct Swallowed;

impl Pull {
    /// Retail values for a WOT type, or None for other terrain.
    pub fn for_terrain(terrain: &str) -> Option<Pull> {
        let (magnitude, radius) = match terrain.rsplit('_').next()? {
            "Small" => (9000.0, 145.0),
            "Med" => (15000.0, 260.0),
            "Big" => (25000.0, 625.0),
            _ => return None,
        };
        Some(Pull { magnitude, radius, kill: 0.0 })
    }

    /// Drift speed (units/s) of a ship of `mass` at `distance`: zero outside the radius, constant inside (retail: nothing moved at
    /// 1.4 and 2.6 radii; the same speed at 0.73 and 0.33 radii).
    pub fn drift(&self, distance: f32, mass: f32) -> f32 {
        if distance > self.radius { 0.0 } else { DRIFT_GAIN * self.magnitude / 9000.0 / mass.max(1.0).sqrt() }
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
    let kill = p.kill;
    let mut hole =
        commands.spawn((Scenario, BlackHole { p, pts: vec![], acc: 0.0, rng: 0x1234_5678 ^ pos.x.to_bits(), mesh: None }, Transform::from_translation(pos), Visibility::default()));
    if let Some(pull) = Pull::for_terrain(terrain) {
        hole.insert(Pull { kill, ..pull });
    }
    let e = hole.id();
    fx::spawn(commands, lib, &format!("BlackHoleRibbons_{kind}"), Some(e), Transform::IDENTITY, Transform::from_translation(pos), false);
}

/// Moves every free ship inside a black hole's radius toward its centre, and destroys it there. Retail moves the position directly
/// (a resting ship keeps zero velocity while it creeps in), so this shifts the transform and leaves speed and drift alone.
/// Docked and destroyed ships are left alone. PROVISIONAL / not modelled: a resting retail Cutter, after ~21 s inside, went straight
/// in at a constant 2.32 units/s (a Man-O-War resting 200 s never did); what triggers it is unknown.
pub fn pull(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<crate::combat::Library>,
    holes: Query<(&Transform, &Pull), Without<Ship>>,
    mut ships: Query<(Entity, &Ship, &mut Transform, &mut Health, Option<&Docker>)>,
) {
    let dt = time.delta_secs();
    for (e, ship, mut tf, mut health, docker) in &mut ships {
        if health.destroyed || docker.is_some_and(Docker::is_docked) {
            continue;
        }
        let mass = lib.classes[ship.class].phys.mass;
        for (hole, pull) in &holes {
            if horizontal(hole.translation, tf.translation).length() < pull.kill {
                health.core = 0.0; // combat::deaths sinks it next
                commands.entity(e).insert(Swallowed);
            } else {
                let step = pull_step(hole.translation, *pull, tf.translation, mass, dt);
                tf.translation += step;
            }
        }
    }
}

fn horizontal(hole: Vec3, ship: Vec3) -> Vec3 {
    Vec3::new(hole.x - ship.x, 0.0, hole.z - ship.z)
}

/// Displacement of a ship of `mass` at `ship` by one hole at `hole` over `dt`: toward the centre, turned DRIFT_SWIRL clockwise.
fn pull_step(hole: Vec3, pull: Pull, ship: Vec3, mass: f32, dt: f32) -> Vec3 {
    let to = horizontal(hole, ship);
    let distance = to.length();
    if distance <= 0.0 {
        return Vec3::ZERO;
    }
    let inward = to / distance;
    // Bevy is y-up with game forward = -z, so clockwise seen from above is a negative turn about +y.
    let dir = Quat::from_rotation_y(-DRIFT_SWIRL.atan()) * inward;
    dir * pull.drift(distance, mass) * dt
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
    fn drift_matches_the_retail_small_hole() {
        let small = Pull::for_terrain("Terrain_BlackHole_Small").unwrap();
        assert_eq!(small, Pull { magnitude: 9000.0, radius: 145.0, kill: 0.0 });
        assert_eq!(Pull::for_terrain("Terrain_Island"), None);
        // nothing outside the radius (retail: Man-O-War at 200-203 for 6 min, Cutter at 372)
        assert_eq!(small.drift(200.0, 40000.0), 0.0);
        assert_eq!(small.drift(372.0, 4000.0), 0.0);
        // constant inside: Man-O-War 0.033 at 105 and 48 units, Cutter 0.093 at 122
        assert_eq!(small.drift(105.0, 40000.0), small.drift(48.0, 40000.0));
        assert!((small.drift(105.0, 40000.0) / 0.0328 - 1.0).abs() < 0.07);
        assert!((small.drift(122.0, 4000.0) / 0.0925 - 1.0).abs() < 0.07);
    }

    #[test]
    fn drift_spirals_clockwise_toward_the_centre_and_is_horizontal() {
        let pull = Pull::for_terrain("Terrain_BlackHole_Small").unwrap();
        // ship 100 units east of the hole (game +x = bevy +x); the centre is west
        let v = pull_step(Vec3::ZERO, pull, Vec3::new(100.0, 40.0, 0.0), 40000.0, 1.0);
        assert!(v.x < 0.0 && v.y == 0.0, "toward the centre and horizontal, got {v}");
        assert!((v.length() - pull.drift(100.0, 40000.0)).abs() < 1e-6);
        // clockwise from above while heading west turns north (game +y = bevy -z)
        assert!(v.z < 0.0 && (v.z / v.x - DRIFT_SWIRL).abs() < 1e-3, "sideways part, got {v}");
        assert_eq!(pull_step(Vec3::ZERO, pull, Vec3::ZERO, 40000.0, 1.0), Vec3::ZERO, "no direction at the centre");
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
        let small = Pull { kill: 10.0, ..Pull::for_terrain("Terrain_BlackHole_Small").unwrap() };
        app.world_mut().spawn((Transform::default(), small));
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
            };
            app.world_mut().spawn((Transform::from_translation(at), ship, health, Drift::default())).id()
        };
        let gunner = ship(&mut app, "Ship_Pirate_Carrack", Side::Enemy, Vec3::new(300.0, 0.0, 0.0));
        let victim = ship(&mut app, "Ship_Navy_ManOWar", Side::Player, Vec3::new(30.0, 0.0, 0.0));
        app.update(); // stats registers both hulls
        app.world_mut().write_message(Hit { shooter: gunner, victim });
        app.world_mut().get_mut::<Health>(victim).unwrap().sections[0] -= 50.0; // the Carrack's last hit
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.05));
        app.update();
        let before = app.world().get::<Transform>(victim).unwrap().translation;
        assert!(before.x < 30.0 && app.world().get::<Health>(victim).unwrap().core > 0.0, "drifting in, still alive: {before}");
        app.world_mut().get_mut::<Transform>(victim).unwrap().translation = Vec3::new(5.0, 0.0, 0.0);
        app.update();
        assert_eq!(app.world().get::<Health>(victim).unwrap().core, 0.0, "inside the kill radius the core goes");
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
