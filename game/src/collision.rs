//! Retail WOT boxes for ships/asteroids, mesh triangles for static objects (docs/re/collision.md).
use bevy::prelude::*;
use std::collections::HashMap;

/// One mesh layer's tree in the layer's own frame (`Shapes::obstacle` places it), with the layer's offset from the object's origin.
struct Layer {
    offset: Vec3,
    nodes: Vec<tree::BoxNode>,
}
struct Shape {
    layers: Vec<Layer>,
    min: [f32; 3],
    max: [f32; 3],
    vertices: Vec<[f32; 3]>,
    indices: Vec<[usize; 3]>,
}
fn layer(l: &serde_json::Value, name: &str) -> Layer {
    let vec = |v: &serde_json::Value| Vec3::from_array(serde_json::from_value(v.clone()).expect("Invalid collision tree vector"));
    let nodes = l["nodes"]
        .as_array()
        .expect("Collision tree nodes")
        .iter()
        .map(|n| tree::BoxNode {
            // exported with 4 decimals: restore unit length
            b: tree::Box3 { centre: vec(&n["c"]), axes: [0, 1, 2].map(|i| vec(&n["a"][i]).normalize()), half: vec(&n["h"]) },
            level: n["l"].as_u64().expect("Collision tree level") as u32,
            kids: n["k"].as_array().map_or_else(Vec::new, |k| k.iter().map(|i| i.as_u64().expect("Collision tree child") as usize).collect()),
        })
        .collect::<Vec<_>>();
    assert!(nodes.iter().all(|n| n.kids.iter().all(|k| *k < nodes.len()) && n.b.half.min_element() >= 0.0), "Invalid collision tree: {name}");
    Layer { offset: vec(&l["offset"]), nodes }
}
#[derive(Resource)]
pub struct Shapes(HashMap<String, Shape>);
impl FromWorld for Shapes {
    fn from_world(_: &mut World) -> Self {
        let path = crate::assets_dir().join("data/collision.json");
        let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).expect("Invalid collision data");
        let array = |v: &serde_json::Value| serde_json::from_value(v.clone()).expect("Invalid collision vector");
        Self(
            doc.as_object()
                .expect("Collision type map")
                .iter()
                .map(|(name, s)| {
                    let shape = Shape {
                        layers: s["layers"].as_array().map_or_else(Vec::new, |l| l.iter().map(|l| layer(l, name)).collect()),
                        min: array(&s["min"]),
                        max: array(&s["max"]),
                        vertices: s["vertices"].as_array().map_or_else(Vec::new, |v| v.iter().map(array).collect()),
                        indices: s["indices"]
                            .as_array()
                            .map_or_else(Vec::new, |v| v.iter().map(|t| serde_json::from_value(t.clone()).expect("Invalid collision indices")).collect()),
                    };
                    assert!((0..3).all(|i| shape.min[i].is_finite() && shape.max[i].is_finite() && shape.min[i] <= shape.max[i]), "Invalid collision bounds: {name}");
                    assert!(
                        shape.vertices.iter().flatten().all(|v| v.is_finite()) && shape.indices.iter().flatten().all(|i| *i < shape.vertices.len()),
                        "Invalid collision mesh: {name}"
                    );
                    (name.clone(), shape)
                })
                .collect(),
        )
    }
}

#[derive(Clone, Copy)]
struct Bounds {
    min: Vec3,
    max: Vec3,
}
impl Bounds {
    fn points(p: &[Vec3]) -> Self {
        Self { min: p.iter().copied().fold(Vec3::splat(f32::INFINITY), Vec3::min), max: p.iter().copied().fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max) }
    }
    fn overlaps(self, b: Self) -> bool {
        self.min.cmple(b.max).all() && b.min.cmple(self.max).all()
    }
}
#[derive(Clone)]
struct BoxBody {
    points: Vec<Vec3>,
    bounds: Bounds,
}
fn box_body([min, max]: [Vec3; 2], tf: &Transform) -> BoxBody {
    let points: Vec<_> = [(min.x, min.z), (max.x, min.z), (max.x, max.z), (min.x, max.z)].into_iter().map(|(x, z)| tf.transform_point(Vec3::new(x, min.y, z))).collect();
    let all: Vec<_> = [min.x, max.x]
        .into_iter()
        .flat_map(|x| [min.y, max.y].into_iter().flat_map(move |y| [min.z, max.z].into_iter().map(move |z| tf.transform_point(Vec3::new(x, y, z)))))
        .collect();
    BoxBody { points, bounds: Bounds::points(&all) }
}
struct Triangle {
    points: [Vec3; 3],
    bounds: Bounds,
}
#[derive(Component)]
pub struct Obstacle {
    body: BoxBody,
    triangles: Vec<Triangle>,
    /// Per mesh layer, the tree of boxes a ship touches (world space); empty for scenery without one, which ships touch by triangles.
    tree: Vec<Vec<tree::BoxNode>>,
}
impl Obstacle {
    /// shortcut: scenery without exported collision spheres uses its world bounds; replace when native sphere export is decoded.
    pub fn sphere(&self) -> (Vec3, f32) {
        let b = self.body.bounds;
        ((b.min + b.max) * 0.5, (b.max - b.min).length() * 0.5)
    }
    /// Distance along `ray` to the nearest hull triangle (Möller-Trumbore), for hover picking.
    pub fn ray_hit(&self, ray: Ray3d) -> Option<f32> {
        let (o, d) = (ray.origin, *ray.direction);
        self.triangles
            .iter()
            .filter_map(|t| {
                let [a, b, c] = t.points;
                let (e1, e2) = (b - a, c - a);
                let p = d.cross(e2);
                let det = e1.dot(p);
                if det.abs() < 1e-8 {
                    return None;
                }
                let s = o - a;
                let u = s.dot(p) / det;
                let q = s.cross(e1);
                let v = d.dot(q) / det;
                let t = e2.dot(q) / det;
                (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t > 0.0).then_some(t)
            })
            .min_by(f32::total_cmp)
    }
    /// Scenery made of these world-space triangles.
    #[cfg(test)]
    pub fn from_triangles(tris: &[[Vec3; 3]]) -> Self {
        let all: Vec<Vec3> = tris.iter().flatten().copied().collect();
        let bounds = Bounds::points(&all);
        let triangles = tris.iter().map(|t| Triangle { points: *t, bounds: Bounds::points(t) }).collect();
        Obstacle { body: BoxBody { points: vec![bounds.min, bounds.max], bounds }, triangles, tree: vec![] }
    }
    /// Fraction along the segment `a`-`b` where it first meets a triangle (the original sweeps every projectile's step this way).
    pub fn segment_hit(&self, a: Vec3, b: Vec3) -> Option<f32> {
        let seg = Bounds::points(&[a, b]);
        if !self.body.bounds.overlaps(seg) {
            return None;
        }
        let d = b - a;
        self.triangles
            .iter()
            .filter(|t| t.bounds.overlaps(seg))
            .filter_map(|t| {
                let [p, q, r] = t.points;
                let (e1, e2) = (q - p, r - p);
                let h = d.cross(e2);
                let det = e1.dot(h);
                if det.abs() < 1e-9 {
                    return None;
                }
                let s = a - p;
                let u = s.dot(h) / det;
                let qv = s.cross(e1);
                let v = d.dot(qv) / det;
                let t = e2.dot(qv) / det;
                (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && (0.0..=1.0).contains(&t)).then_some(t)
            })
            .min_by(f32::total_cmp)
    }
    pub fn translate(&mut self, delta: Vec3) {
        for p in &mut self.body.points {
            *p += delta;
        }
        self.body.bounds.min += delta;
        self.body.bounds.max += delta;
        for node in self.tree.iter_mut().flatten() {
            node.shifted(delta);
        }
        for triangle in &mut self.triangles {
            for p in &mut triangle.points {
                *p += delta;
            }
            triangle.bounds.min += delta;
            triangle.bounds.max += delta;
        }
    }
}
impl Shapes {
    pub fn obstacle(&self, name: &str, tf: &Transform) -> Option<Obstacle> {
        let s = self.0.get(name)?;
        // Tumbling box obstacles need their actual 3D faces, not a yaw-only footprint.
        let (vertices, indices) = if name.starts_with("Asteroid") && s.indices.is_empty() {
            let (a, b) = (Vec3::from_array(s.min), Vec3::from_array(s.max));
            let vertices = vec![[a.x, a.y, a.z], [b.x, a.y, a.z], [b.x, b.y, a.z], [a.x, b.y, a.z], [a.x, a.y, b.z], [b.x, a.y, b.z], [b.x, b.y, b.z], [a.x, b.y, b.z]];
            let indices = vec![[0, 1, 2], [0, 2, 3], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [3, 2, 6], [3, 6, 7], [0, 3, 7], [0, 7, 4], [1, 2, 6], [1, 6, 5]];
            (vertices, indices)
        } else {
            (s.vertices.clone(), s.indices.clone())
        };
        let triangles = indices
            .iter()
            .map(|t| {
                let points = t.map(|i| tf.transform_point(Vec3::from_array(vertices[i])));
                Triangle { bounds: Bounds::points(&points), points }
            })
            .collect();
        // Ships touch land by its box tree; a rock moves, so it keeps its triangles (docs/re/collision.md).
        let tree = if name.starts_with("Asteroid") {
            vec![]
        } else {
            s.layers
                .iter()
                .map(|l| {
                    l.nodes
                        .iter()
                        .map(|n| tree::BoxNode {
                            b: tree::Box3 { centre: tf.transform_point(n.b.centre + l.offset), axes: n.b.axes.map(|a| tf.rotation * a), half: n.b.half * tf.scale.abs() },
                            level: n.level,
                            kids: n.kids.clone(),
                        })
                        .collect()
                })
                .collect()
        };
        Some(Obstacle { body: box_body([Vec3::from_array(s.min), Vec3::from_array(s.max)], tf), triangles, tree })
    }
}

pub fn update_asteroids(shapes: Res<Shapes>, mut rocks: Query<(&crate::combat::Asteroid, &Transform, &mut Obstacle), Changed<Transform>>) {
    for (rock, tf, mut obstacle) in &mut rocks {
        if let Some(body) = shapes.obstacle(&rock.kind, tf) {
            *obstacle = body;
        }
    }
}

/// 'Set Group/Unit Movable' FALSE: the ship holds its place and is an immovable body in contacts.
#[derive(Component, Clone)]
pub struct Pinned;
/// 'SetCollidable' FALSE: the ship passes through everything.
#[derive(Component, Clone)]
pub struct Ghost;

/// Extra velocity from contact, separate from the existing forward engine-speed model.
#[derive(Component, Default)]
pub struct Drift(pub Vec3);

fn flat(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}
fn world(v: Vec2) -> Vec3 {
    Vec3::new(v.x, 0.0, v.y)
}

/// Separating-axis contact, including contained intervals and zero-width (vertical wall) projections.
fn contact(a: &[Vec3], b: &[Vec3]) -> Option<Vec3> {
    let mut best = Vec2::ZERO;
    let mut depth = f32::INFINITY;
    for poly in [a, b] {
        for i in 0..poly.len() {
            let edge = flat(poly[(i + 1) % poly.len()] - poly[i]);
            if edge.length_squared() < 1e-10 {
                continue;
            }
            let axis = Vec2::new(-edge.y, edge.x).normalize();
            let range = |p: &[Vec3]| p.iter().map(|v| flat(*v).dot(axis)).fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), v| (min.min(v), max.max(v)));
            let (amin, amax) = range(a);
            let (bmin, bmax) = range(b);
            if amax <= bmin || bmax <= amin {
                return None;
            }
            let left = amax - bmin;
            let right = bmax - amin;
            let (d, n) = if left < right { (left, -axis) } else { (right, axis) };
            if d < depth {
                depth = d;
                best = n;
            }
        }
    }
    depth.is_finite().then(|| world(best) * (depth + 0.001))
}

/// Clip mesh faces to the ship's actual vertical slab before projecting to the sailing plane.
fn slab(points: &[Vec3], min: f32, max: f32) -> Vec<Vec3> {
    let mut out = points.to_vec();
    for (height, sign) in [(min, 1.0), (max, -1.0)] {
        let input = std::mem::take(&mut out);
        for i in 0..input.len() {
            let a = input[i];
            let b = input[(i + 1) % input.len()];
            let da = (a.y - height) * sign;
            let db = (b.y - height) * sign;
            if da >= 0.0 {
                out.push(a);
            }
            if (da >= 0.0) != (db >= 0.0) {
                out.push(a + (b - a) * (da / (da - db)));
            }
        }
    }
    out
}

struct Contact {
    push: Vec3,
    point: Vec3,
}
/// The contact with the shallowest push; coplanar triangles count as one face.
fn closest_contact(contacts: Vec<Contact>) -> Option<Contact> {
    let first = contacts.iter().min_by(|a, b| a.push.length_squared().total_cmp(&b.push.length_squared()))?;
    let normal = first.push.normalize();
    let peers: Vec<_> = contacts.iter().filter(|c| (c.push.length() - first.push.length()).abs() < 0.001 && c.push.normalize().dot(normal) > 0.999).collect();
    // Coplanar triangles are one face; otherwise a head-on hit acquires artificial off-centre torque.
    Some(Contact { push: first.push, point: peers.iter().map(|c| c.point).sum::<Vec3>() / peers.len() as f32 })
}
/// PROVISIONAL single-point manifold: centre of the overlapping support intervals, not the native contact collector.
fn contact_point(a: &[Vec3], b: &[Vec3], normal: Vec3) -> Vec3 {
    let tangent = Vec3::new(normal.z, 0.0, -normal.x);
    let range = |p: &[Vec3], axis: Vec3| p.iter().map(|p| p.dot(axis)).fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
    let (an, _) = range(a, normal);
    let (_, bn) = range(b, normal);
    let (al, ah) = range(a, tangent);
    let (bl, bh) = range(b, tangent);
    normal * ((an + bn) * 0.5) + tangent * ((al.max(bl) + ah.min(bh)) * 0.5)
}
/// Whether `body` overlaps `obstacle` (`static_contacts` without computing the pushes; stops at the first touching triangle).
fn hits(body: &BoxBody, obstacle: &Obstacle) -> bool {
    if !body.bounds.overlaps(obstacle.body.bounds) {
        return false;
    }
    if obstacle.triangles.is_empty() {
        return contact(&body.points, &obstacle.body.points).is_some();
    }
    obstacle.triangles.iter().filter(|t| body.bounds.overlaps(t.bounds)).any(|t| {
        let p = slab(&t.points, body.bounds.min.y, body.bounds.max.y);
        p.len() >= 2 && contact(&body.points, &p).is_some()
    })
}
fn static_contacts(body: &BoxBody, obstacle: &Obstacle) -> Vec<Contact> {
    if !body.bounds.overlaps(obstacle.body.bounds) {
        return vec![];
    }
    if obstacle.triangles.is_empty() {
        return contact(&body.points, &obstacle.body.points)
            .map(|push| Contact { point: contact_point(&body.points, &obstacle.body.points, push.normalize()), push })
            .into_iter()
            .collect();
    }
    obstacle
        .triangles
        .iter()
        .filter(|t| body.bounds.overlaps(t.bounds))
        .filter_map(|t| {
            let p = slab(&t.points, body.bounds.min.y, body.bounds.max.y);
            (p.len() >= 2).then(|| contact(&body.points, &p).map(|push| Contact { point: contact_point(&body.points, &p, push.normalize()), push })).flatten()
        })
        .collect()
}

/// Visible scenery a ship can touch (hidden scenery and docking exceptions are handled by callers).
pub fn scenery<'a>(obstacles: impl Iterator<Item = (&'a Obstacle, Option<&'a Visibility>)>) -> Vec<&'a Obstacle> {
    obstacles.filter(|(_, v)| *v != Some(&Visibility::Hidden)).map(|(o, _)| o).collect()
}

/// Whether a ship box at `tf` overlaps any scenery, with the same test as sailing contact.
pub fn touches(bounds: [Vec3; 2], tf: &Transform, scenery: &[&Obstacle]) -> bool {
    let body = box_body(bounds, tf);
    scenery.iter().any(|o| hits(&body, o))
}

/// Number of leading path poses that stay clear of scenery; the course preview stops drawing at this index.
pub fn clear_prefix(bounds: [Vec3; 2], path: &[Transform], scenery: &[&Obstacle]) -> usize {
    path.iter().position(|tf| touches(bounds, tf, scenery)).unwrap_or(path.len())
}

/// How far ship box `a` moves away from ship box `b` in full (the other moves the opposite way, by mass): depth x |cos| along the
/// horizontal part of the least-penetration normal, none when the boxes do not touch.
fn hull_push(a: &tree::Box3, b: &tree::Box3) -> Option<Vec3> {
    tree::overlap(b, a).map(tree::push)
}

fn forward(ship: &crate::Ship) -> Vec3 {
    Vec3::new(-ship.heading.sin(), 0.0, -ship.heading.cos())
}
pub fn set_velocity(ship: &mut crate::Ship, drift: &mut Drift, velocity: Vec3) {
    let f = forward(ship);
    ship.speed = velocity.dot(f);
    drift.0 = velocity - f * ship.speed;
}
struct ContactBody {
    velocity: Vec3,
    inverse_mass: f32,
    inverse_inertia: f32,
    arm: Vec3,
}
fn ship_body(ship: &crate::Ship, tf: &Transform, drift: &Drift, class: &crate::data::Class, point: Vec3) -> ContactBody {
    let size = (class.collision_bounds[1] - class.collision_bounds[0]) * tf.scale.abs();
    // PROVISIONAL box inertia; native contact torque exists, its exact inertia mapping is not recovered.
    ContactBody {
        velocity: forward(ship) * ship.speed + drift.0,
        inverse_mass: 1.0 / class.phys.mass,
        inverse_inertia: 12.0 / (class.phys.mass * (size.x * size.x + size.z * size.z).max(0.001)),
        arm: (point - tf.transform_point(class.mass_centre)).with_y(0.0),
    }
}

/// The index of `class`'s damage section (placed by `tf`) whose box centre is nearest `at`.
pub fn nearest_section(class: &crate::data::Class, tf: &Transform, at: Vec3) -> Option<usize> {
    let d = |k: &usize| tf.transform_point((class.sections[*k].min + class.sections[*k].max) * 0.5).distance_squared(at);
    (0..class.sections.len()).min_by(|a, b| d(a).total_cmp(&d(b)))
}

/// Contact damage `damage` to ship `a` from ship or rock `b` at `at` (docs/re/collision.md):
/// `combat::lob::ram_damage` puts it on `a`'s section nearest `at`, scaled by difficulty when `b` is a ship.
#[derive(Message)]
pub struct Ram {
    pub a: Entity,
    pub b: Entity,
    pub damage: f32,
    pub at: Vec3,
}
/// Ship-against-ship damage: this x the other ship's mass x the closing speed, at most once per `RAM_COOLDOWN` s per ship.
const RAM_FACTOR: f32 = 0.0004;
const RAM_COOLDOWN: f32 = 3.5;
/// Ship-against-rock damage, to both, every physics tick (0.033 s) in contact: this x the rock's mass x the closing speed.
const ROCK_FACTOR: f32 = 0.0002;
use crate::TICK_SECS as TICK;

/// Damage a rock and a ship do each other over `dt` seconds of contact: the rate is written per tick of `TICK` seconds.
fn rock_hurt(rock_mass: f32, closing_speed: f32, dt: f32) -> f32 {
    ROCK_FACTOR * rock_mass * closing_speed * dt / TICK
}

pub fn sail(
    time: Res<Time>,
    lib: Res<crate::Library>,
    obstacles: Query<(Entity, &Obstacle, Option<&Visibility>), (Without<crate::Ship>, Without<crate::combat::Asteroid>)>,
    mut rocks: Query<(Entity, &mut crate::combat::Asteroid, &mut Transform, &mut Obstacle, &mut Drift, Option<&Visibility>), Without<crate::Ship>>,
    mission: Option<Res<crate::campaign::Mission>>,
    mut cables: Local<HashMap<(Entity, Entity), f32>>,
    human: Query<(), With<crate::PlayerShip>>,
    boats: Query<(), With<crate::lifeboats::Lifeboat>>,
    mut ships: Query<(Entity, &mut crate::Ship, &mut Transform, &crate::Health, &mut Drift, Option<&crate::docking::Docker>)>,
    mut sfx: Option<ResMut<Messages<crate::audio::Sfx>>>,
    mut rng: Option<ResMut<crate::combat::SimRng>>,
    flags: Query<(Has<Pinned>, Has<Ghost>)>,
    currents: Query<&crate::environment::Current>,
    mut rams: Option<ResMut<Messages<Ram>>>,
    captains: Query<&crate::campaign::MissionShip>,
    (fleets, members, mut avoiders, mut rammed, dying, mut broken, spawn_order): (
        Option<Res<crate::ship::Fleets>>,
        Query<&crate::ship::FleetMember>,
        Local<HashMap<Entity, avoid::Avoid>>,
        Local<HashMap<Entity, f32>>,
        Query<&crate::combat::Sinking>,
        Option<ResMut<Messages<crate::combat::Destroyed>>>,
        (Query<&crate::scenario::SpawnIndex>, crate::scenario::BattleStart),
    ),
) {
    let (spawn_order, mut start) = spawn_order;
    if start.is_new() {
        cables.clear();
        avoiders.clear();
        rammed.clear();
    }
    let now = time.elapsed_secs();
    // The ram clock starts at creation: a new ship cannot be rammed for its first `RAM_COOLDOWN` s.
    for (ship, new, ..) in ships.iter_mut() {
        if new.is_added() {
            rammed.insert(ship, now + RAM_COOLDOWN);
        }
    }
    rammed.retain(|_, until| *until > now);
    let cinematic = mission.as_deref().is_some_and(|m| m.script.nis && m.opening.is_none());
    // PROVISIONAL: stop ordinary helm/combat motion in NIS, but retain authored routes (e.g. Exile's retreat).
    let mut stopped = std::collections::HashSet::new();
    if cinematic {
        for (e, mut ship, _, _, mut drift, _) in &mut ships {
            if !captains.get(e).is_ok_and(|m| (m.stop_at_end || m.looping.is_some()) && !ship.waypoints.is_empty()) {
                ship.speed = 0.0;
                ship.ang_vel = 0.0;
                drift.0 = Vec3::ZERO;
                stopped.insert(e);
            }
        }
    }
    let pinned = |e: Entity| stopped.contains(&e) || flags.get(e).is_ok_and(|f| f.0);
    let ghost = |e: Entity| flags.get(e).is_ok_and(|f| f.1);
    let dt = time.delta_secs();
    // The original's collision sound: a 7 % chance per touching pair each 0.033 s tick; a coin picks one of the two hulls, and its
    // section nearest the contact sounds wood or metal (cloth, stone and the rest are silent), heard out to 700 units.
    let mut bump = |classes: [(&crate::data::Class, &Transform); 2], at: Vec3, dt: f32| {
        let (Some(rng), Some(sfx)) = (rng.as_mut(), sfx.as_mut()) else { return };
        if rng.f() >= 0.07 * dt / TICK {
            return;
        }
        let (class, tf) = classes[usize::from(rng.f() < 0.5)];
        let pool = nearest_section(class, tf, at).and_then(|k| match crate::combat::material_key(&class.sections[k].material) {
            "Wood" | "Reinforced" => Some("Wood"),
            "Iron" => Some("Metal"),
            _ => None,
        });
        if let Some(pool) = pool {
            sfx.write(crate::audio::Sfx { name: format!("Ship_Collision_{pool}0{}", rng.variant(5)), at: Some(at), vol: 1.0, max: 700.0 });
        }
    };
    let tows = mission.as_ref().map_or(&[][..], |m| m.script.tows());
    cables.retain(|link, _| tows.contains(link));
    for &(tower, towee) in tows {
        if cables.contains_key(&(tower, towee)) {
            continue;
        }
        if let Ok([(_, _, ta, ha, _, _), (_, _, tb, hb, _, _)]) = ships.get_many_mut([tower, towee])
            && !ha.destroyed
            && !hb.destroyed
        {
            // The original: the length is the distance between the hull centres when the link forms.
            cables.insert((tower, towee), (ta.translation - tb.translation).with_y(0.0).length());
        }
    }
    let tow_masses: HashMap<Entity, f32> = tows.iter().filter_map(|&(tower, towee)| Some((tower, lib.classes[ships.get(towee).ok()?.1.class].phys.mass))).collect();
    let local = fleets.as_ref().map(|f| f.local.as_str());
    // The human's ships (`Captain_Human` in the original): the player ship and the human's fleet; allied computer fleets share the side.
    let humans = |e: Entity, side: crate::Side| side == crate::Side::Player && (human.contains(e) || members.get(e).ok().is_none_or(|m| Some(m.0.as_str()) == local));
    // The original's collision avoider (`avoid`) for every computer-captained ship; the human's ships run it only on their dock routine.
    {
        use avoid::{Avoid, Key, Me, Other, Shape};
        let priority = |e: Entity, s: &crate::Ship| s.speed.abs() + if s.speed.abs() >= 0.001 && humans(e, s.side) { avoid::HUMAN_PRIORITY } else { 0.0 };
        let mut others: Vec<Other> = vec![];
        for (oe, o, v) in &obstacles {
            if v != Some(&Visibility::Hidden) && !ghost(oe) {
                let (centre, radius) = o.sphere();
                others.push(Other { key: Key::Entity(oe), centre, radius, priority: 0.0, lifeboat: false, shape: Shape::Fixed { obstacle: o, island: true } });
            }
        }
        for (i, (_, r, _, o, _, v)) in rocks.iter().enumerate() {
            if r.hp > 0.0 && v != Some(&Visibility::Hidden) {
                let (centre, radius) = o.sphere();
                others.push(Other { key: Key::Rock(i), centre, radius, priority: 0.0, lifeboat: false, shape: Shape::Fixed { obstacle: o, island: false } });
            }
        }
        for (oe, s, tf, h, ..) in &ships {
            if !h.destroyed && !ghost(oe) {
                let class = &lib.classes[s.class];
                let shape = Shape::Ship { bounds: class.collision_bounds, heading: s.heading, velocity: Vec3::new(-s.heading.sin(), 0.0, -s.heading.cos()) * s.speed };
                others.push(Other { key: Key::Entity(oe), centre: tf.translation, radius: class.radius, priority: priority(oe, s), lifeboat: boats.contains(oe), shape });
            }
        }
        let mut seen = 0;
        avoiders.retain(|e, _| ships.contains(*e));
        for (e, mut s, tf, health, _, docker) in &mut ships {
            // A human-captained ship skips the avoider on a move order: the original gates it on "the computer controls this ship",
            // true for the human's ships only on a dock order or a scripted path, so a click course runs straight into ships and rocks.
            if health.destroyed || docker.map_or(humans(e, s.side), |d| !d.avoids()) || pinned(e) {
                s.avoid = 1.0;
                continue;
            }
            seen += 1;
            let a = avoiders.entry(e).or_insert_with(|| Avoid::new(0.05 * (seen % 10) as f32));
            // A detour the avoider set last tick is its own marker, not the ship's course.
            if a.detour.is_some() && s.waypoints.first() == a.detour.as_ref() {
                s.waypoints.remove(0);
            }
            if a.due(dt) {
                let class = &lib.classes[s.class];
                let me = Me {
                    pos: tf.translation,
                    heading: s.heading,
                    speed: s.speed,
                    bounds: class.collision_bounds,
                    radius: class.radius,
                    priority: priority(e, &s),
                    lifeboat: boats.contains(e),
                };
                let ahead = |h: f32| tf.translation + Vec3::new(-h.sin(), 0.0, -h.cos()) * (s.speed.abs() * 2.0 * avoid::LOOK + class.radius + 1.0);
                let dest = s.waypoints.first().copied().or(s.goal.map(ahead));
                match dest {
                    Some(dest) => a.think(&me, dest, &others, Key::Entity(e)),
                    None => *a = Avoid::new(0.0),
                }
            }
            s.avoid = a.cap;
            if let Some(d) = a.detour {
                if (d - tf.translation).with_y(0.0).length() < (s.speed.abs() * 3.0).max(lib.classes[s.class].radius) {
                    a.detour = None;
                } else {
                    s.waypoints.insert(0, d);
                }
            }
        }
    }
    // One tick (docs/re/tick.md): everything integrates one step of `dt`, then cables pull and one collision pass runs over the new
    // positions. Each overlapping pair responds at most once. The earlier small substeps are gone: a runaway drift may now cross a
    // thin hull within a tick, as it does in the original.
    let substep = dt;
    for (_, rock, mut tf, mut obstacle, drift, visible) in &mut rocks {
        if rock.hp <= 0.0 || visible == Some(&Visibility::Hidden) || drift.0 == Vec3::ZERO {
            continue;
        }
        let delta = drift.0 * substep;
        tf.translation += delta;
        obstacle.translate(delta);
    }
    for (e, mut s, mut tf, health, mut drift, docker) in &mut ships {
        // A destroyed hull still sails while it breaks up (docs/re/effects.md "Death drive"): the computer steers it with the
        // wrecked drive; once the break-up is over it no longer moves.
        let sinking = dying.get(e).ok();
        if health.destroyed && sinking.is_none() {
            continue;
        }
        if docker.is_some_and(crate::docking::Docker::gliding) {
            drift.0 = Vec3::ZERO;
            continue;
        }
        if docker.is_some_and(crate::docking::Docker::is_docked) || pinned(e) {
            s.speed = 0.0;
            drift.0 = Vec3::ZERO;
            continue;
        }
        let class = &lib.classes[s.class];
        let towed = tows.iter().any(|link| link.1 == e);
        if s.towed != towed {
            s.towed = towed;
        }
        // The tower's thrust works on both hulls (the original divides it by its own mass plus the towee's).
        let tow_mass = tow_masses.get(&e).copied().unwrap_or(0.0);
        if s.tow_mass != tow_mass {
            s.tow_mass = tow_mass;
        }
        if let Some(dir) = avoiders.get(&e).and_then(|a| a.stuck).filter(|_| sinking.is_none()) {
            // The original's unstick: a fixed turn in place, sqrt(MaxAngularAcceleration / RotationalFriction) / 3 rad/s, whatever
            // the speed, rudder or crew; the ship keeps its speed but does not move until it is free.
            s.heading += dir * (class.phys.max_ang_acc / class.phys.rot_friction.max(1e-3)).sqrt() / 3.0 * substep;
            tf.rotation = Quat::from_rotation_y(s.heading);
        } else if let Some(wreck) = sinking.and_then(|k| k.drive_health(health)) {
            crate::ship::helm_step(&mut s, &mut tf, class, &wreck, substep, true);
            tf.rotation = Quat::from_rotation_y(s.heading); // the roll of a sinking hull is the break-up clip's, not the helm's
        } else {
            let ai = sinking.is_some() || !humans(e, s.side);
            crate::ship::helm_step(&mut s, &mut tf, class, health, substep, ai);
            if sinking.is_some() {
                tf.rotation = Quat::from_rotation_y(s.heading);
            }
        }
        // Etherium currents push along their path (environment.rs); force / mass, as for thrust.
        for c in &currents {
            drift.0 += c.force(tf.translation) / class.phys.mass * substep;
        }
        // PROVISIONAL lateral damping; the original has lateral drag but its full mapping is not recovered.
        drift.0 *= (-2.0 * substep).exp();
        tf.translation += drift.0 * substep;
    }
    // The original's tow keeper (docs/re/collision.md Tow constraint): after both hulls ran their own step, a taut link moves the
    // pair back together along the line of centres, 25 % on the tower and 75 % on a movable towee (all on the tower when the towee is
    // pinned). Speeds are not touched: the towee's own drag and sideways damping already ran.
    for (&(tower, towee), &length) in cables.iter() {
        let Ok([(_, _, mut ta, ha, _, _), (_, mut b, mut tb, hb, db, _)]) = ships.get_many_mut([tower, towee]) else { continue };
        if ha.destroyed || hb.destroyed {
            continue;
        }
        let separation = (tb.translation - ta.translation).with_y(0.0);
        let distance = separation.length();
        if distance <= length || distance < 0.001 {
            continue;
        }
        let stretch = separation / distance * (distance - length);
        if pinned(towee) {
            ta.translation += stretch;
            continue;
        }
        ta.translation += stretch * 0.25;
        tb.translation -= stretch * 0.75;
        // PROVISIONAL rope torque on the towee (the original's constants are not decoded): the pull acts at its tow point, so the
        // hull swings to trail the rope. Only the angular part of the pull of that 75 % move is applied.
        let cb = &lib.classes[b.class];
        let body = ship_body(&b, &tb, &db, cb, tb.transform_point(cb.tow_points[1]));
        let pull = -stretch * 0.75 / substep / body.inverse_mass;
        b.ang_vel += body.arm.cross(pull).y * body.inverse_inertia * substep;
    }
    // The collision pass, after every object has moved: ships against the land and rocks, then ship against ship. Ships, rocks and
    // pairs go in spawn order: the bump sound draws random numbers, and each push moves a hull the next pair sees.
    let ship_order = crate::scenario::in_spawn_order(&spawn_order, ships.iter().map(|s| s.0), |e| *e);
    let rock_order = crate::scenario::in_spawn_order(&spawn_order, rocks.iter().map(|r| r.0), |e| *e);
    let mut walk = ships.iter_many_mut(&ship_order);
    while let Some(item) = walk.fetch_next() {
        let Ok((e, s, mut tf, health, drift, docker)) = item else { continue };
        if health.destroyed || ghost(e) || docker.is_some_and(|d| d.gliding() || d.is_docked()) || pinned(e) {
            continue;
        }
        let class = &lib.classes[s.class];
        for (oe, obstacle, visible) in &obstacles {
            if visible == Some(&Visibility::Hidden) || ghost(oe) || docker.is_some_and(|d| d.ignores_collision(oe)) {
                continue;
            }
            if !obstacle.tree.is_empty() {
                // The original: the ship's box against the mesh's box tree, one push out along the
                // horizontal normal per tick, no iteration (docs/re/collision.md).
                let ship = tree::Box3::placed(class.collision_bounds, &tf);
                if let Some(push) = tree::land_push(&obstacle.tree, &ship) {
                    tf.translation += push;
                }
                continue;
            }
            // Rebuild after each correction: adjacent faces must not use a stale box.
            for _ in 0..3 {
                let body = box_body(class.collision_bounds, &tf);
                let contacts = static_contacts(&body, obstacle);
                let Some(contact) = closest_contact(contacts) else { break };
                // The original: land and bases only push the hull out along the horizontal normal; speed,
                // turn, damage and sound are untouched, so a ship slides along the shore and can still turn free.
                tf.translation += contact.push.with_y(0.0);
            }
        }
        // PROVISIONAL: native asteroid restitution/inertia remain unresolved.
        // shortcut: share the planar ship solver for rocks; upgrade when native asteroid response is decoded.
        let mut rock_walk = rocks.iter_many_mut(&rock_order);
        while let Some(item) = rock_walk.fetch_next() {
            let Ok((rock_e, mut rock, mut rock_tf, mut obstacle, mut rock_drift, visible)) = item else { continue };
            if rock.hp <= 0.0 || visible == Some(&Visibility::Hidden) {
                continue;
            }
            // One response per pair and tick.
            let body = box_body(class.collision_bounds, &tf);
            let Some(contact) = closest_contact(static_contacts(&body, &obstacle)) else { continue };
            let a = ship_body(&s, &tf, &drift, class, contact.point);
            let size = obstacle.body.bounds.max - obstacle.body.bounds.min;
            let b = ContactBody {
                velocity: rock.velocity + rock_drift.0,
                inverse_mass: 1.0 / rock.mass,
                inverse_inertia: 12.0 / (rock.mass * (size.x * size.x + size.z * size.z).max(0.001)),
                arm: (contact.point - rock_tf.translation).with_y(0.0),
            };
            // The original: pushed apart by mass, the ship's speed and turn untouched, the rock shoved away from the ship
            // (0.25 x the centres' offset per second), and both damaged every tick by the rock's mass x the closing speed.
            let push = contact.push.with_y(0.0);
            tf.translation += push * (a.inverse_mass / (a.inverse_mass + b.inverse_mass));
            let delta = -push * (b.inverse_mass / (a.inverse_mass + b.inverse_mass));
            rock_tf.translation += delta;
            obstacle.translate(delta);
            // PROVISIONAL: a rock touch sounds like a ship touch on the ship's side (the rock's own impact sound is not read).
            bump([(class, &*tf), (class, &*tf)], contact.point, substep);
            // ...and dropped from its belt for good (waypoints cleared on every touching tick); it keeps that speed afterwards.
            rock_drift.0 = Vec3::ZERO;
            crate::combat::knock_off(&mut rock, rock_tf.translation, tf.translation);
            let hurt = rock_hurt(rock.mass, (a.velocity - b.velocity).length(), substep);
            // A rock the touch breaks counts as destroyed by the ship (its owner is credited, as for a shell).
            if rock.damage(hurt)
                && let Some(d) = broken.as_mut()
            {
                d.write(crate::combat::Destroyed { by: e, victim: rock_e });
            }
            if let Some(r) = rams.as_mut().filter(|_| hurt > 0.0) {
                r.write(Ram { a: e, b: rock_e, damage: hurt, at: contact.point });
            }
        }
    }
    // One pass over all pairs: an overlapping pair responds once per tick; a formation overlap settles over several ticks.
    let pairs = (0..ship_order.len()).flat_map(|i| (i + 1..ship_order.len()).map(move |j| (i, j)));
    for (i, j) in pairs {
        let Ok([(ea, mut a, mut ta, ha, da, docka), (eb, mut b, mut tb, hb, db, dockb)]) = ships.get_many_mut([ship_order[i], ship_order[j]]) else { continue };
        // In the original a ship hitting a lifeboat has no response: lifeboats pass through ships and each other (they still touch for pickup).
        if ha.destroyed
            || hb.destroyed
            || ghost(ea)
            || ghost(eb)
            || boats.contains(ea)
            || boats.contains(eb)
            || docka.is_some_and(|d| d.ignores_collision(eb))
            || dockb.is_some_and(|d| d.ignores_collision(ea))
        {
            continue;
        }
        let ca = &lib.classes[a.class];
        let cb = &lib.classes[b.class];
        let ba = box_body(ca.collision_bounds, &ta);
        let bb = box_body(cb.collision_bounds, &tb);
        if !ba.bounds.overlaps(bb.bounds) {
            continue;
        }
        // The original: the two boxes' 3-D separating-axis test, the least-penetration axis as the normal; the
        // pair is pushed apart along its horizontal part by depth x |cos| (a tilted or vertical normal moves the hulls less).
        let Some(push) = hull_push(&tree::Box3::placed(ca.collision_bounds, &ta), &tree::Box3::placed(cb.collision_bounds, &tb)) else { continue };
        if push == Vec3::ZERO {
            continue;
        }
        let normal = push.normalize();
        let point = contact_point(&ba.points, &bb.points, normal);
        let fixed = |body: ContactBody, pin: bool| if pin { ContactBody { inverse_mass: 0.0, inverse_inertia: 0.0, ..body } } else { body };
        let a_body = fixed(ship_body(&a, &ta, &da, ca, point), pinned(ea));
        let b_body = fixed(ship_body(&b, &tb, &db, cb, point), pinned(eb));
        let total = a_body.inverse_mass + b_body.inverse_mass;
        if total <= 0.0 {
            continue;
        }
        // The original: pushed apart by mass, the heavier hull moving less; no bounce, both keep their
        // velocity. Each gets a yaw kick at the contact point from 0.1 x its mass x its change to the pair's common velocity
        // per 0.033 s, over a box's inertia (the own mass cancels: a light ship swings, a heavy one hardly turns).
        ta.translation += push * (a_body.inverse_mass / total);
        tb.translation -= push * (b_body.inverse_mass / total);
        let (ma, mb) = (ca.phys.mass, cb.phys.mass);
        let common = (a_body.velocity * ma + b_body.velocity * mb) / (ma + mb);
        let kick = |body: &ContactBody, size: Vec3, dir: Vec3| {
            let force = dir * 0.1 * (common - body.velocity).length() / TICK; // per unit of own mass
            (body.arm.cross(force).y * 12.0 / (size.x * size.x + size.z * size.z).max(1e-3)).clamp(-60.0, 60.0) * substep
        };
        // The box inertia's width and length: twice the collision box's starboard and forward extents.
        let size = |c: &crate::data::Class| Vec3::new(2.0 * c.collision_bounds[1].x, 0.0, -2.0 * c.collision_bounds[0].z);
        if !pinned(ea) {
            a.ang_vel += kick(&a_body, size(ca), normal);
        }
        if !pinned(eb) {
            b.ang_vel += kick(&b_body, size(cb), -normal);
        }
        bump([(ca, &*ta), (cb, &*tb)], point, substep);
        // Ram damage: the other ship's mass x the closing speed on the section nearest the contact, at most once per 3.5 s
        // per ship, never between allies.
        let closing = (a_body.velocity - b_body.velocity).length();
        for (victim, other, m, side, other_side) in [(ea, eb, mb, a.side, b.side), (eb, ea, ma, b.side, a.side)] {
            if side != other_side && !rammed.contains_key(&victim) && closing > 0.0 {
                rammed.insert(victim, now + RAM_COOLDOWN);
                if let Some(r) = rams.as_mut() {
                    r.write(Ram { a: victim, b: other, damage: RAM_FACTOR * m * closing, at: point });
                }
            }
        }
    }
}

pub(crate) mod avoid;
pub mod spot;
#[cfg(test)]
mod tests;
pub mod tree;
