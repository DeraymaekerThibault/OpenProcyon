//! Nebula clouds and solar storms: the Nebula / SolarStorm effects are one mesh particle each (OvalNebula / GeoSolar) whose
//! vertices shift and breathe every 30 Hz tick. The shifting rules follow the original's mesh-particle update, its
//! parameters are the effect's MeshChunk (scaled by 0.001 on load, as the original does). Meteor showers reuse the MeteorStorm particle
//! effect. PROVISIONAL: the cloud radius (= point magnitude), FADE_EDGES (alpha by view angle), the texture-spin axis (U),
//! lightning, wind and meteor hazards (partly provisional; docs/re/nebula-landmark.md). Each meteor emitter belongs to the group named after its
//! nebula and follows that group's `Group to follow path` / `Set Group Space Objects velocity` rules along the shared waypoint
//! follower (`combat::follow_path`, as asteroid belts do). PROVISIONAL: emitters, not meteor objects, travel the path.
use bevy::asset::RenderAssetUsages;
use bevy::audio::Volume;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use serde_json::Value;

use crate::combat::{PathMode, follow_path};
use crate::{Scenario, assets_dir, fx, game_to_bevy};
use std::collections::HashMap;

const TICK: f32 = 1.0 / 30.0;
const SIZE: f32 = 1.0; // mesh units -> world units per unit of magnitude
const MAX_NEIGHBOURS: usize = 5;

/// MeshChunk parameters after the load-time scaling (x 0.001).
#[derive(Clone, Default, Debug)]
struct Shift {
    noise: f32,
    noise_normal: f32,
    attract: f32,
    pull: f32,
    constrain: f32,
    lower: f32,
    upper: f32,
    come_back_dist: f32,
    stop_dist: f32,
    come_back: f32,
    come_back_delta: f32,
    neighbour_dist: f32,
    spin: f32,
}

struct Vertex {
    base: Vec3,
    off: Vec3,
    vel: Vec3,
    scale: f32,
    scale_vel: f32,
    returning: bool,
    master: Option<usize>, // an exact copy of an earlier vertex (UV seam): follows it
    near: Vec<usize>,
}

struct Sim {
    v: Vec<Vertex>,
    p: Shift,
    rng: u32,
    spin: f32,
}

impl Sim {
    fn new(base: &[Vec3], p: Shift, seed: u32) -> Sim {
        let mut v: Vec<Vertex> =
            base.iter().map(|&b| Vertex { base: b, off: Vec3::ZERO, vel: Vec3::ZERO, scale: 1.0, scale_vel: 0.0, returning: false, master: None, near: vec![] }).collect();
        for i in 0..v.len() {
            v[i].master = (0..i).find(|&j| v[j].master.is_none() && v[j].base == v[i].base);
        }
        for i in 0..v.len() {
            if v[i].master.is_some() {
                continue;
            }
            let mut near: Vec<(f32, usize)> = vec![];
            for j in (0..v.len()).filter(|&j| j != i && v[j].master.is_none()) {
                let d = v[i].base.distance_squared(v[j].base);
                if d < p.neighbour_dist {
                    if near.len() < MAX_NEIGHBOURS {
                        near.push((d, j));
                    } else if let Some(far) = near.iter_mut().max_by(|a, b| a.0.total_cmp(&b.0)).filter(|f| d < f.0) {
                        *far = (d, j);
                    }
                }
            }
            v[i].near = near.into_iter().map(|n| n.1).collect();
        }
        Sim { v, p, rng: seed, spin: 0.0 }
    }

    fn f(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(0x19660d).wrapping_add(0x3c6ef35f);
        f32::from_bits(self.rng & 0x7fffff | 0x3f800000) - 1.0
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }

    fn tick(&mut self) {
        let p = self.p.clone();
        for i in 0..self.v.len() {
            if self.v[i].master.is_some() || self.v[i].near.is_empty() {
                continue;
            }
            let k = self.rng.wrapping_mul(0x19660d).wrapping_add(0x3c6ef35f);
            self.rng = k;
            let u = self.v[i].near[(k as usize) % self.v[i].near.len()];
            let diff = self.v[i].scale - self.v[u].scale;
            // Pull the scale toward the neighbour's; near-equal scales jitter freely.
            let (lo, hi) = if diff <= p.pull { if diff >= -p.pull { (-3.0 * p.noise_normal, 3.0 * p.noise_normal) } else { (0.0, p.attract) } } else { (-p.attract, 0.0) };
            let (a, b) = (self.range(lo, hi), self.range(-p.noise_normal, p.noise_normal));
            let vx = &mut self.v[i];
            vx.scale_vel = (vx.scale_vel + a + b).clamp(-p.constrain, p.constrain);
            vx.scale = (vx.scale + vx.scale_vel).clamp(p.lower, p.upper);
            if !vx.returning {
                let j = Vec3::new(self.range(-p.noise, p.noise), self.range(-p.noise, p.noise), self.range(-p.noise, p.noise));
                let vx = &mut self.v[i];
                vx.vel += j;
                vx.off += vx.vel;
                if vx.off.length_squared() > p.come_back_dist {
                    vx.returning = true;
                }
            } else {
                vx.off *= p.come_back;
                vx.vel *= p.come_back_delta;
                if vx.vel.length_squared() < p.stop_dist {
                    vx.returning = false;
                }
            }
        }
        let j = self.f();
        self.spin += j * p.spin * 0.01 + p.spin;
    }

    /// Current vertex position in mesh units.
    fn pos(&self, i: usize) -> Vec3 {
        let v = &self.v[self.v[i].master.unwrap_or(i)];
        v.base * v.scale + v.off
    }
}

fn shift_params(chunk: &Value) -> Shift {
    let g = |k: &str| chunk[k].as_f64().unwrap_or(0.0) as f32;
    let m = 0.001; // the original scales these on load
    Shift {
        noise: g("CHUNK_DATA_SHIFT_NOISE") * m,
        noise_normal: g("CHUNK_DATA_SHIFT_NOISE_NORMAL") * m,
        attract: g("CHUNK_DATA_SHIFT_NOISE_NORMAL_ATTRACT") * m,
        pull: g("CHUNK_DATA_SHIFT_DIFFERENCE_PULL") * m,
        constrain: g("CHUNK_DATA_SHIFT_NOISE_NORMAL_CONSTRAIN") * m,
        lower: g("CHUNK_DATA_SHIFT_NORMAL_MULTIPLY_CONSTRAIN_LOWER"),
        upper: g("CHUNK_DATA_SHIFT_NORMAL_MULTIPLY_CONSTRAIN_UPPER"),
        come_back_dist: g("CHUNK_DATA_COME_BACK_DIST_SQUARED") * m,
        stop_dist: g("CHUNK_DATA_STOP_COME_BACK_DIST_SQUARED") * m,
        come_back: g("CHUNK_DATA_COME_BACK_MULTIPLY"),
        come_back_delta: g("CHUNK_DATA_COME_BACK_MULTIPLY_DELTA") * m,
        neighbour_dist: g("CHUNK_DATA_NEIBOURS_MIN_DISTANCE_SQUARED"),
        spin: g("CHUNK_DATA_TEXTURE_SPIN_RATE") * m,
    }
}

/// A rule's route for the nebula group of this name (rules.rs writes these once per mission setup).
#[derive(Message)]
pub enum MeteorRoute {
    Path { group: String, path: Vec<Vec3>, closest: bool, mode: PathMode },
    Speed { group: String, speed: f32 },
}

/// The route a group of meteor emitters follows: waypoints from the rule's path plus its velocity.
#[derive(Clone, Default, Debug)]
pub struct MeteorPlan {
    pub path: Vec<Vec3>,
    pub closest: bool,
    pub mode: PathMode,
    pub speed: f32,
}

#[derive(Resource, Default)]
struct MeteorPlans(HashMap<String, MeteorPlan>);

/// One meteor emitter (a MeteorStorm effect at a point of its nebula's meteor point set).
#[derive(Component)]
pub struct MeteorEmitter {
    pub group: String,
    next: usize,
    forward: bool,
    started: bool,
}

/// One tick of a meteor emitter's motion. The first move with a path picks the start point (`Find Closest Point`).
fn step_meteor(m: &mut MeteorEmitter, pos: &mut Vec3, plan: &MeteorPlan, dt: f32) {
    if plan.path.is_empty() || plan.speed <= 0.0 {
        return;
    }
    if !m.started {
        m.started = true;
        m.forward = true;
        m.next = if plan.closest { plan.path.iter().enumerate().min_by(|a, b| a.1.distance_squared(*pos).total_cmp(&b.1.distance_squared(*pos))).map_or(0, |p| p.0) } else { 0 };
    }
    m.next = m.next.min(plan.path.len() - 1);
    follow_path(pos, &plan.path, &mut m.next, &mut m.forward, plan.mode, plan.speed, dt);
}

fn route_meteors(mut routes: MessageReader<MeteorRoute>, mut plans: ResMut<MeteorPlans>) {
    for r in routes.read() {
        match r {
            MeteorRoute::Path { group, path, closest, mode } => {
                let p = plans.0.entry(group.clone()).or_default();
                (p.path, p.closest, p.mode) = (path.clone(), *closest, *mode);
            }
            MeteorRoute::Speed { group, speed } => plans.0.entry(group.clone()).or_default().speed = *speed,
        }
    }
}

/// The map's nebula/storm setups (`map["nebulae"]`), for the storm hazards.
#[derive(Resource)]
pub struct Storms(pub Value);

/// Damaging nebulae (a solar storm or meteors) as moving circles (bevy centre, mean vertex distance) for captains to keep out of
/// (retail captains avoid nebulae, `ship::captain_steer`). PROVISIONAL: plain clouds are not avoided; the circle is a stand-in.
pub fn storm_circles(storms: &Storms, shift: &StormShift) -> Vec<(Vec3, f32)> {
    storms
        .0
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| n["storm"]["effect"].as_str().is_some_and(|e| e != "EFFECT") || n["meteors"]["on"] == true)
        .filter_map(|n| {
            let pts: Vec<Vec2> = n["polygon"].as_array()?.iter().filter_map(|p| Some(Vec2::new(p[0].as_f64()? as f32, p[1].as_f64()? as f32))).collect();
            if pts.is_empty() {
                return None;
            }
            let c = pts.iter().sum::<Vec2>() / pts.len() as f32;
            let r = pts.iter().map(|p| p.distance(c)).sum::<f32>() / pts.len() as f32;
            let offset = shift.0.get(n["name"].as_str().unwrap_or_default()).map_or(Vec3::ZERO, |s| s.0);
            Some((Vec3::new(c.x, 0.0, -c.y) + offset, r))
        })
        .collect()
}

/// Retail Terrain_Nebula damage potentials, independent of weapon bullets.
#[derive(Resource)]
struct WeatherDamage(HashMap<String, HashMap<String, [f32; 5]>>);

/// How far each nebula group has travelled from where it was placed, and this frame's step (its volume moves with it).
#[derive(Resource, Default)]
pub struct StormShift(pub HashMap<String, (Vec3, Vec3)>);

fn move_meteors(
    time: Res<Time>,
    plans: Res<MeteorPlans>,
    mut emitters: Query<(&mut Transform, &mut MeteorEmitter), Without<Cloud>>,
    mut clouds: Query<(&mut Transform, &mut Cloud)>,
    mut shift: ResMut<StormShift>,
) {
    let dt = time.delta_secs().min(crate::MAX_SIM_STEP);
    for (mut tf, mut m) in &mut emitters {
        if let Some(plan) = plans.0.get(&m.group) {
            step_meteor(&mut m, &mut tf.translation, plan, dt);
        }
    }
    for v in shift.0.values_mut() {
        v.1 = Vec3::ZERO;
    }
    for (mut tf, mut c) in &mut clouds {
        let Some(plan) = plans.0.get(&c.route.group) else { continue };
        let before = tf.translation;
        step_meteor(&mut c.route, &mut tf.translation, plan, dt);
        shift.0.insert(c.route.group.clone(), (tf.translation - c.origin, tf.translation - before));
    }
}

// Terrain_Nebula.wot constants; hit geometry/selection from the original game's behaviour.
const LIGHTNING_MIN_SHIP_DISTANCE: f32 = 50.0;
const LIGHTNING_MIN_LENGTH: f32 = 100.0;
const LIGHTNING_HEIGHT: f32 = 200.0;
const METEOR_HIT_MODIFIER: f32 = 80.0;

fn storm_due(timers: &mut HashMap<(String, u8), f32>, name: &str, kind: u8, every: f32, dt: f32) -> bool {
    let t = timers.entry((name.into(), kind)).or_insert(every);
    *t -= dt;
    if *t <= 0.0 {
        *t = every;
        true
    } else {
        false
    }
}

fn meteor_hit_chance(radius: f32) -> f32 {
    (2.0 * radius / METEOR_HIT_MODIFIER).clamp(0.0, 1.0)
}

/// PROVISIONAL sampler: uniform rejection in the polygon's bounds; retail samples weighted polygon vertices.
fn lightning_point(poly: &Value, offset: Vec3, ships: &[Vec3], rnd: &mut impl FnMut() -> f32) -> Option<Vec3> {
    let points = poly.as_array().filter(|p| p.len() >= 3)?;
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for p in points {
        let p = Vec2::new(p[0].as_f64()? as f32, p[1].as_f64()? as f32);
        min = min.min(p);
        max = max.max(p);
    }
    for _ in 0..50 {
        let p = min + (max - min) * Vec2::new(rnd(), rnd());
        let at = Vec3::new(p.x, (2.0 * rnd() - 1.0) * LIGHTNING_HEIGHT, -p.y) + offset;
        if crate::rules::in_game_volume(poly, p.x, p.y) && ships.iter().all(|s| s.distance(at) >= LIGHTNING_MIN_SHIP_DISTANCE) {
            return Some(at);
        }
    }
    None
}

/// Lightning is one storm-wide random segment, not a guaranteed strike per ship. Meteors roll diameter / 80 and pick a section.
/// PROVISIONAL: retain conservative 3 s lightning / 2 s meteor recharge floors until full retail timing is validated.
/// Wind force remains a tangential push of 3% of its magnitude per second; damage excludes the inner 5% of the storm radius.
fn storms(
    time: Res<Time>,
    lib: Res<crate::combat::Library>,
    mission: Option<Res<crate::campaign::Mission>>,
    setups: Option<Res<Storms>>,
    damage: Option<Res<WeatherDamage>>,
    shift: Res<StormShift>,
    fxl: Option<Res<fx::FxLib>>,
    mut commands: Commands,
    mut ships: Query<(Entity, &mut Transform, &crate::Ship, &crate::combat::Health, &mut crate::collision::Drift, Option<&crate::campaign::MissionShip>)>,
    mut strike: MessageWriter<crate::combat::Strike>,
    mut sfx: MessageWriter<crate::audio::Sfx>,
    mut timers: Local<HashMap<(String, u8), f32>>,
    mut seed: Local<u32>,
) {
    let (Some(map), Some(damage)) = (setups, damage) else { return };
    if map.is_changed() {
        timers.clear();
    }
    let trap = mission.as_ref().is_some_and(|m| m.script.nebula_trap);
    let dt = time.delta_secs().min(crate::MAX_SIM_STEP);
    let mut rnd = || {
        *seed = seed.wrapping_mul(0x19660d).wrapping_add(0x3c6ef35f);
        (*seed >> 8) as f32 / (1u32 << 24) as f32
    };
    for n in map.0.as_array().into_iter().flatten() {
        let name = n["name"].as_str().unwrap_or_default();
        let (offset, step) = shift.0.get(name).copied().unwrap_or_default();
        let poly = &n["polygon"];
        let storm = &n["storm"];
        let meteors = &n["meteors"];
        let centre = storm["points"].get(0).map(|p| game_to_bevy(&p["pos"]) + offset);
        let f = |v: &Value| v.as_f64().unwrap_or(0.0) as f32;
        let lightning = storm["lightning"] == true && storm_due(&mut timers, name, 0, f(&storm["lightning_recharge"]).max(3.0), dt) && rnd() < 0.5;
        let wind = storm["winds"] == true && f(&storm["wind_damage_frequency"]) > 0.0 && storm_due(&mut timers, name, 1, f(&storm["wind_damage_frequency"]), dt);
        let meteor = meteors["on"] == true && storm_due(&mut timers, name, 2, f(&meteors["recharge"]).max(2.0), dt);
        for (e, mut tf, ship, h, mut drift, ms) in &mut ships {
            if h.destroyed || ms.is_some_and(|m| !m.visible) || !crate::rules::in_game_volume(poly, tf.translation.x - offset.x, -(tf.translation.z - offset.z)) {
                continue;
            }
            if trap {
                tf.translation += step;
            }
            let at = tf.translation;
            let class = &lib.classes[ship.class];
            if storm["winds"] == true
                && let Some(c) = centre
            {
                let radial = (at - c).with_y(0.0);
                let tangent = Vec3::new(-radial.z, 0.0, radial.x).normalize_or_zero();
                drift.0 += tangent * f(&storm["wind_magnitude"]) * 0.03 * dt;
                if wind && at.distance(c) > 0.05 * f(&storm["points"][0]["magnitude"]) {
                    for (section, _) in class.sections.iter().enumerate().filter(|(_, s)| s.material.contains("Cloth")) {
                        strike.write(crate::combat::Strike { victim: e, damage: damage.0["wind"].clone(), section });
                    }
                }
            }
            if meteor && !class.sections.is_empty() && rnd() < meteor_hit_chance(class.radius) {
                let section = (rnd() * class.sections.len() as f32) as usize;
                let s = &class.sections[section];
                let hit = tf.transform_point((s.min + s.max) / 2.0);
                strike.write(crate::combat::Strike { victim: e, damage: damage.0["meteor"].clone(), section });
                let r = rnd();
                sfx.write(crate::audio::Sfx { name: format!("Damage_Stone0{}", 1 + (r * 5.0) as u32 % 5), at: Some(hit), vol: 1.0, max: 600.0 });
                if let Some(fxl) = fxl.as_deref() {
                    let t = Transform::from_translation(hit);
                    fx::spawn(&mut commands, fxl, "Explosion_Light", None, t, t, true);
                }
            }
        }
        if lightning {
            let positions: Vec<_> = ships.iter().filter(|(_, _, _, h, _, ms)| !h.destroyed && ms.is_none_or(|m| m.visible)).map(|(_, tf, ..)| tf.translation).collect();
            if let Some(a) = lightning_point(poly, offset, &positions, &mut rnd) {
                let b = (0..50).filter_map(|_| lightning_point(poly, offset, &positions, &mut rnd)).find(|b| a.distance(*b) >= LIGHTNING_MIN_LENGTH);
                if let Some(b) = b {
                    let hit = ships
                        .iter()
                        .filter_map(|(e, tf, ship, h, _, ms)| {
                            if h.destroyed || ms.is_some_and(|m| !m.visible) || !crate::rules::in_game_volume(poly, tf.translation.x - offset.x, -(tf.translation.z - offset.z)) {
                                return None;
                            }
                            let inv = tf.to_matrix().inverse();
                            crate::combat::ship_segment_hit(&lib.classes[ship.class], h, inv.transform_point3(a), inv.transform_point3(b)).map(|(t, section)| (t, e, section))
                        })
                        .min_by(|x, y| x.0.total_cmp(&y.0));
                    if let Some((t, victim, section)) = hit {
                        let at = a.lerp(b, t);
                        strike.write(crate::combat::Strike { victim, damage: damage.0["lightning"].clone(), section });
                        sfx.write(crate::audio::Sfx { name: format!("Solar_Storm_Lightning_ShipImpact0{}", 1 + (rnd() * 6.0) as u32), at: Some(at), vol: 1.0, max: 800.0 });
                        if let Some(fxl) = fxl.as_deref() {
                            let tf = Transform::from_translation(at);
                            fx::spawn(&mut commands, fxl, "Lightning", None, tf, tf, true);
                        }
                    }
                }
            }
        }
        // Sheet lightning inside the storm, heard when near.
        if storm["lightning"] == true
            && let Some(c) = centre
        {
            let r = rnd();
            if r < dt * 0.4 {
                sfx.write(crate::audio::Sfx { name: format!("Solar_Storm_Lightning0{}", 1 + (rnd() * 8.0) as u32 % 8), at: Some(c), vol: 0.8, max: 1500.0 });
            }
        }
    }
}

#[derive(Component)]
pub struct Cloud {
    sim: Sim,
    radius: f32,
    age: f32,
    acc: f32,
    colours: [Vec<(f32, f32)>; 3],
    uv: Vec<[f32; 2]>,
    idx: Vec<u32>,
    sound_distance: f32,
    mesh: Option<Handle<Mesh>>,
    model: String,
    /// The nebula's group (its setup name): clouds travel that group's route like the meteor emitters ('nebula ride').
    route: MeteorEmitter,
    origin: Vec3,
}

/// Linear over the keys, repeating with the last key's time (the effect's colour loops start 0, end = last key).
fn looped(c: &[(f32, f32)], t: f32) -> f32 {
    let Some(&(end, _)) = c.last() else { return 1.0 };
    let t = if end > 0.0 { t % end } else { 0.0 };
    c.windows(2).find(|w| t <= w[1].0).map_or(c[0].1, |w| w[0].1 + (w[1].1 - w[0].1) * (t - w[0].0) / (w[1].0 - w[0].0).max(1e-6))
}

fn spawn_one(commands: &mut Commands, assets: &AssetServer, effect: &str, mesh_name: &str, pos: Vec3, radius: f32, sound: &str, sound_distance: f32, seed: u32, group: &str) {
    let Some(j) = crate::data::try_json(&assets_dir().join(format!("fx/{effect}.json"))) else { return };
    let chunk = &j["Primitive"][0]["CommonPropertiesChunk"];
    let Some((base, uv, idx)) = crate::data::mesh_geometry(assets_dir(), mesh_name) else { return };
    let rgb = &chunk["ColorFaderChunk"]["ColorInterpolator3Chunk"];
    commands.spawn((
        Scenario,
        Cloud {
            sim: Sim::new(&base, shift_params(&chunk["MeshChunk"]), seed),
            radius,
            age: 0.0,
            acc: 0.0,
            colours: ["Value1Interpolator", "Value2Interpolator", "Value3Interpolator"].map(|k| crate::fx::curve_keys(&rgb[k])),
            uv,
            idx,
            sound_distance,
            mesh: None,
            model: crate::data::model(&format!("{mesh_name}.glb")),
            route: MeteorEmitter { group: group.into(), next: 0, forward: true, started: false },
            origin: pos,
        },
        Transform::from_translation(pos),
        Visibility::default(),
        AudioPlayer::new(assets.load(crate::audio::sound(sound))),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)),
    ));
}

/// Nebula volumes of a mission (game x, y polygons): each 'Setup Nebula' names its polygon in the mission's volumes.
/// PROVISIONAL: every setup counts for 'Unit is Within any Nebula', whatever its cloud or storm effect.
pub fn polygons(mission: &Value, shift: &StormShift) -> Vec<crate::rules::Region> {
    mission["environment"]["nebulae"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| mission["volumes"][n["Polygon Name"].as_str().unwrap_or("")].is_array())
        .map(|n| {
            let mut polygon = crate::rules::Region::from_json(&mission["volumes"][n["Polygon Name"].as_str().unwrap_or("")]);
            let offset = shift.0.get(n["New Nebula Name"].as_str().unwrap_or("")).map_or(Vec3::ZERO, |s| s.0);
            polygon.0.iter_mut().for_each(|p| *p += Vec2::new(offset.x, -offset.z));
            polygon
        })
        .collect()
}

/// Spawns every nebula setup of a map (`map["nebulae"]`, tools/export_nebulae.py).
pub fn spawn(commands: &mut Commands, assets: &AssetServer, fxl: &fx::FxLib, map: &Value) {
    commands.insert_resource(Storms(map["nebulae"].clone()));
    let damage = crate::data::read_json(&assets_dir().join("data/nebula.json"));
    commands.insert_resource(WeatherDamage(serde_json::from_value(damage).expect("nebula weather damage tables")));
    commands.insert_resource(StormShift::default());
    let mut seed = 0x9e37_79b9u32;
    for n in map["nebulae"].as_array().into_iter().flatten() {
        let dist = n["sound_distance"].as_f64().unwrap_or(300.0) as f32;
        let points = |k: &str| n[k]["points"].as_array().cloned().unwrap_or_default();
        let radius = |p: &Value| p["magnitude"].as_f64().unwrap_or(0.0) as f32;
        if n["cloud"]["effect"] == "Nebula" {
            for p in points("cloud") {
                seed = seed.wrapping_mul(0x19660d).wrapping_add(0x3c6ef35f);
                spawn_one(commands, assets, "Nebula", "OvalNebula", game_to_bevy(&p["pos"]), radius(&p), "Ambience_Nebula", dist, seed, n["name"].as_str().unwrap_or_default());
            }
        }
        if n["storm"]["effect"] == "SolarStorm" {
            for p in points("storm") {
                seed = seed.wrapping_mul(0x19660d).wrapping_add(0x3c6ef35f);
                spawn_one(
                    commands,
                    assets,
                    "SolarStorm",
                    "GeoSolar",
                    game_to_bevy(&p["pos"]),
                    radius(&p),
                    "Ambience_SolarStorm_Loop",
                    dist,
                    seed,
                    n["name"].as_str().unwrap_or_default(),
                );
            }
        }
        if n["meteors"]["on"] == true {
            for p in points("meteors") {
                let pos = game_to_bevy(&p["pos"]);
                if let Some(e) = fx::spawn(commands, fxl, "MeteorStorm", None, Transform::from_translation(pos), Transform::from_translation(pos), false) {
                    let group = n["name"].as_str().unwrap_or_default().to_string();
                    commands.entity(e).insert(MeteorEmitter { group, next: 0, forward: true, started: false });
                }
            }
        }
    }
}

pub fn plugin(app: &mut App) {
    app.add_message::<MeteorRoute>()
        .init_resource::<MeteorPlans>()
        .init_resource::<StormShift>()
        .add_systems(Update, (update, (route_meteors, move_meteors, storms.before(crate::sail).after(crate::input)).chain()).run_if(in_state(crate::menu::Screen::Battle)));
}

fn update(
    mission: Option<Res<crate::campaign::Mission>>,
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<fx::EffectMaterial>>,
    cam: Single<&GlobalTransform, With<Camera3d>>,
    mut clouds: Query<(Entity, &mut Cloud, &GlobalTransform, Option<&mut AudioSink>)>,
) {
    let cam = cam.translation();
    for (e, mut c, gt, sink) in &mut clouds {
        if c.mesh.is_none() {
            let m = meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
            let mat = mats.add(fx::additive_material(assets.load(GltfAssetLabel::Texture(0).from_asset(c.model.clone()))));
            commands.entity(e).insert((Mesh3d(m.clone()), MeshMaterial3d(mat), NoFrustumCulling));
            c.mesh = Some(m);
        }
        c.acc += time.delta_secs().min(0.25);
        while c.acc >= TICK {
            c.acc -= TICK;
            c.age += TICK;
            c.sim.tick();
        }
        let centre = gt.translation();
        // 'Set Nebula Fadeout Distance': clouds fade out over that distance beyond it (PROVISIONAL linear ramp).
        let fade = mission.as_ref().and_then(|m| m.script.nebula_fade).map_or(1.0, |d| (1.0 - ((cam.distance(centre) - c.radius) - d) / d.max(1.0)).clamp(0.0, 1.0));
        let rgb = [0, 1, 2].map(|i| looped(&c.colours[i], c.age) * fade);
        let (mut pos, mut uv, mut col) = (vec![], vec![], vec![]);
        for i in 0..c.sim.v.len() {
            let p = c.sim.pos(i);
            let world = p * c.radius * SIZE; // GLB positions are already Bevy axes
            // FADE_EDGES (PROVISIONAL): silhouettes fade, faces toward the camera stay solid.
            let facing = (centre + world - cam).normalize_or_zero().dot(world.normalize_or_zero()).abs();
            pos.push(world.to_array());
            uv.push([c.uv[i][0] + c.sim.spin, c.uv[i][1]]);
            // ONE/ONE ignores texture/colour alpha; edge fading modulates RGB instead.
            col.push([rgb[0] * facing, rgb[1] * facing, rgb[2] * facing, 1.0]);
        }
        if let Some(mut m) = c.mesh.as_ref().and_then(|m| meshes.get_mut(m)) {
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
            m.insert_indices(Indices::U32(c.idx.clone()));
        }
        if let Some(mut sink) = sink {
            let distance = (cam.distance(centre) - c.radius).max(0.0);
            sink.set_volume(Volume::Linear(0.25 * (1.0 - distance / c.sound_distance).clamp(0.0, 1.0) * crate::menu::volume(crate::menu::Setting::Effects)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weather_selection_uses_global_pulses_and_ship_size_hit_rolls() {
        let mut timers = HashMap::new();
        assert!(!storm_due(&mut timers, "A", 0, 3.0, 1.0));
        assert!(!storm_due(&mut timers, "A", 0, 3.0, 1.0));
        assert!(storm_due(&mut timers, "A", 0, 3.0, 1.0));
        assert!(!storm_due(&mut timers, "B", 0, 3.0, 1.0));
        assert!(!storm_due(&mut timers, "A", 0, 3.0, 0.0));
        assert_eq!(meteor_hit_chance(8.75), 17.5 / 80.0);
        assert_eq!(meteor_hit_chance(0.0), 0.0);
        assert_eq!(meteor_hit_chance(100.0), 1.0);
        let poly = serde_json::json!([[-100.0, -100.0], [100.0, -100.0], [100.0, 100.0], [-100.0, 100.0]]);
        assert!(lightning_point(&poly, Vec3::ZERO, &[Vec3::ZERO], &mut || 0.5).is_none(), "bolts must not spawn beside a ship");
        let at = lightning_point(&poly, Vec3::X * 500.0, &[], &mut || 0.5).unwrap();
        assert_eq!(at, Vec3::X * 500.0);
        assert!(lightning_point(&serde_json::json!([]), Vec3::ZERO, &[], &mut || 0.5).is_none());
    }

    #[test]
    fn captains_avoid_storms_but_not_plain_clouds() {
        let square = serde_json::json!([[100.0, 100.0], [300.0, 100.0], [300.0, 300.0], [100.0, 300.0]]);
        let storms = Storms(serde_json::json!([
            {"name": "s", "polygon": square, "storm": {"effect": "SolarStorm"}, "meteors": {"on": false}},
            {"name": "c", "polygon": square, "storm": {"effect": "EFFECT"}, "meteors": {"on": false}},
        ]));
        let mut shift = StormShift::default();
        shift.0.insert("s".into(), (Vec3::new(10.0, 0.0, 0.0), Vec3::ZERO));
        let circles = storm_circles(&storms, &shift);
        assert_eq!(circles.len(), 1, "the plain cloud is not a hazard");
        let (c, r) = circles[0];
        assert_eq!(c, Vec3::new(210.0, 0.0, -200.0), "centroid in bevy axes, moved with the storm");
        assert!((r - 100.0 * 2f32.sqrt()).abs() < 1e-3);
    }

    #[test]
    fn thirty_seconds_in_a_storm_does_not_guarantee_lightning_damage() {
        let lib = crate::combat::Library::load(assets_dir());
        let class = lib.index("Ship_Navy_Sloop");
        let hp = crate::combat::Health::new(&lib.classes[class]);
        let ship = crate::Ship {
            class,
            side: crate::Side::Player,
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
        fn no_hits(mut hits: MessageReader<crate::combat::Strike>) {
            assert_eq!(hits.read().count(), 0, "a random bolt must actually intersect the ship before emitting damage");
        }
        let mut app = App::new();
        app.insert_resource(lib)
            .init_resource::<Time>()
            .init_resource::<StormShift>()
            .insert_resource(WeatherDamage(serde_json::from_value(crate::data::read_json(&assets_dir().join("data/nebula.json"))).unwrap()))
            .insert_resource(Storms(serde_json::json!([{
                "name":"test storm", "polygon":[[-1000.0,-1000.0],[1000.0,-1000.0],[1000.0,1000.0],[-1000.0,1000.0]],
                "storm":{"lightning":true,"lightning_recharge":3.0,"winds":false,"points":[{"pos":[0.0,0.0,0.0],"magnitude":1000.0}]},
                "meteors":{"on":false}
            }])))
            .add_message::<crate::combat::Strike>()
            .add_message::<crate::audio::Sfx>()
            .add_systems(Update, (storms, no_hits).chain());
        app.world_mut().spawn((ship, hp, Transform::from_xyz(300.0, 0.0, 0.0), crate::collision::Drift::default()));
        for _ in 0..300 {
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.1));
            app.update();
        }
    }

    fn sim() -> Sim {
        let (base, ..) = crate::data::mesh_geometry(assets_dir(), "OvalNebula").expect("mesh");
        let j: Value = crate::data::read_json(&assets_dir().join("fx/Nebula.json"));
        Sim::new(&base, shift_params(&j["Primitive"][0]["CommonPropertiesChunk"]["MeshChunk"]), 5)
    }

    #[test]
    fn shifting_stays_within_the_exe_limits() {
        let mut s = sim();
        assert!(s.v.iter().any(|v| v.near.len() == MAX_NEIGHBOURS) && s.v.iter().all(|v| v.near.len() <= MAX_NEIGHBOURS));
        for _ in 0..3000 {
            s.tick();
        }
        assert!(s.v.iter().all(|v| (1.0..=1.15).contains(&v.scale) && v.scale_vel.abs() <= s.p.constrain + 1e-6));
        assert!(s.v.iter().any(|v| v.scale > 1.001), "scales breathe");
        assert!(s.v.iter().all(|v| v.off.length() < 0.2));
    }

    fn emitter(group: &str) -> MeteorEmitter {
        MeteorEmitter { group: group.into(), next: 0, forward: true, started: false }
    }

    #[test]
    fn meteor_emitter_teleport_loops_along_its_path_at_its_speed() {
        let plan = MeteorPlan { path: vec![Vec3::ZERO, Vec3::X * 100.0, Vec3::X * 200.0], closest: false, mode: PathMode::Teleport, speed: 100.0 };
        let (mut m, mut pos) = (emitter("Meteor Storm 1"), Vec3::ZERO);
        let seen: Vec<f32> = (0..5)
            .map(|_| {
                step_meteor(&mut m, &mut pos, &plan, 1.0);
                pos.x
            })
            .collect();
        // starts at 0, reaches 100, then 200: arriving at the last point teleports back to 0 in the same tick, and it never stops
        assert_eq!(seen, vec![0.0, 100.0, 0.0, 100.0, 0.0]);
    }

    #[test]
    fn meteor_emitter_starts_at_the_closest_point_and_idles_without_speed() {
        let mut plan = MeteorPlan { path: vec![Vec3::ZERO, Vec3::X * 100.0, Vec3::X * 200.0], closest: true, mode: PathMode::Teleport, speed: 0.0 };
        let (mut m, mut pos) = (emitter("Meteor Storm 2"), Vec3::X * 190.0);
        step_meteor(&mut m, &mut pos, &plan, 1.0);
        assert_eq!(pos, Vec3::X * 190.0, "zero speed does not move");
        plan.speed = 5.0;
        step_meteor(&mut m, &mut pos, &plan, 1.0);
        assert_eq!(m.next, 2, "closest start is the last point");
        assert!((pos.x - 195.0).abs() < 1e-4, "moves 5 units toward it, got {pos}");
    }

    #[test]
    fn colour_curve_loops_with_the_last_key() {
        let c = [(0.0, 0.8), (1.0, 0.9), (2.5, 1.0), (3.7, 0.8)];
        assert!((looped(&c, 0.0) - 0.8).abs() < 1e-6 && (looped(&c, 3.7 + 2.5) - 1.0).abs() < 1e-5);
    }
}
