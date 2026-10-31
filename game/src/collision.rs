//! Retail WOT boxes for ships/asteroids, mesh triangles for static objects (docs/re/collision.md).
use bevy::prelude::*;
use std::collections::HashMap;

struct Shape {
    min: [f32; 3],
    max: [f32; 3],
    vertices: Vec<[f32; 3]>,
    indices: Vec<[usize; 3]>,
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
    pub fn translate(&mut self, delta: Vec3) {
        for p in &mut self.body.points {
            *p += delta;
        }
        self.body.bounds.min += delta;
        self.body.bounds.max += delta;
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
        Some(Obstacle { body: box_body([Vec3::from_array(s.min), Vec3::from_array(s.max)], tf), triangles })
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
    spin: f32,
    inverse_mass: f32,
    inverse_inertia: f32,
    arm: Vec3,
}
impl ContactBody {
    fn at_point(&self) -> Vec3 {
        self.velocity + Vec3::Y.cross(self.arm) * self.spin
    }
    fn effective_mass(&self, normal: Vec3) -> f32 {
        self.inverse_mass + self.arm.cross(normal).y.powi(2) * self.inverse_inertia
    }
}
fn ship_body(ship: &crate::Ship, tf: &Transform, drift: &Drift, class: &crate::data::Class, point: Vec3) -> ContactBody {
    let size = (class.collision_bounds[1] - class.collision_bounds[0]) * tf.scale.abs();
    // PROVISIONAL box inertia; native contact torque exists, its exact inertia mapping is not recovered.
    ContactBody {
        velocity: forward(ship) * ship.speed + drift.0,
        spin: ship.ang_vel,
        inverse_mass: 1.0 / class.phys.mass,
        inverse_inertia: 12.0 / (class.phys.mass * (size.x * size.x + size.z * size.z).max(0.001)),
        arm: (point - tf.transform_point(class.mass_centre)).with_y(0.0),
    }
}
/// Inelastic contact response is PROVISIONAL; include both linear and angular effective mass.
fn impulse(a: &ContactBody, b: &ContactBody, normal: Vec3) -> Vec3 {
    normal * (-(a.at_point() - b.at_point()).dot(normal).min(0.0) / (a.effective_mass(normal) + b.effective_mass(normal)))
}
fn apply_impulse(ship: &mut crate::Ship, drift: &mut Drift, body: &ContactBody, j: Vec3) {
    set_velocity(ship, drift, body.velocity + j * body.inverse_mass);
    ship.ang_vel += body.arm.cross(j).y * body.inverse_inertia;
}
fn fixed_body() -> ContactBody {
    ContactBody { velocity: Vec3::ZERO, spin: 0.0, inverse_mass: 0.0, inverse_inertia: 0.0, arm: Vec3::ZERO }
}

/// A hard contact (impulse above `RAM_IMPULSE`, mass x speed units) between ships `a` and `b`, or ship `a` and scenery (`a == b`):
/// combat.rs turns it into hull damage. PROVISIONAL: the original's collision damage is unknown (docs/re/collision.md).
#[derive(Message)]
pub struct Ram {
    pub a: Entity,
    pub b: Entity,
    pub impulse: f32,
    pub at: Vec3,
}
pub const RAM_IMPULSE: f32 = 8000.0;

pub fn sail(
    time: Res<Time>,
    lib: Res<crate::Library>,
    obstacles: Query<(Entity, &Obstacle, Option<&Visibility>), (Without<crate::Ship>, Without<crate::combat::Asteroid>)>,
    mut rocks: Query<(&mut crate::combat::Asteroid, &mut Transform, &mut Obstacle, &mut Drift, Option<&Visibility>), Without<crate::Ship>>,
    mission: Option<Res<crate::campaign::Mission>>,
    mut cables: Local<HashMap<(Entity, Entity), f32>>,
    human: Query<(), With<crate::PlayerShip>>,
    boats: Query<(), With<crate::lifeboats::Lifeboat>>,
    mut ships: Query<(Entity, &mut crate::Ship, &mut Transform, &crate::Health, &mut Drift, Option<&crate::docking::Docker>)>,
    mut sfx: Option<ResMut<Messages<crate::audio::Sfx>>>,
    mut quiet: Local<f32>,
    flags: Query<(Has<Pinned>, Has<Ghost>)>,
    currents: Query<&crate::environment::Current>,
    mut rams: Option<ResMut<Messages<Ram>>>,
    captains: Query<&crate::campaign::MissionShip>,
    (fleets, members, mut avoiders): (Option<Res<crate::ship::Fleets>>, Query<&crate::ship::FleetMember>, Local<HashMap<Entity, avoid::Avoid>>),
) {
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
    *quiet -= dt;
    let mut bump = |class: &crate::data::Class, at: Vec3, j: Vec3| {
        // Contacts resolve every frame while overlapping: one sound per half second, none for grazing touches.
        if *quiet > 0.0 || j.length() < 1.0 {
            return;
        }
        *quiet = 0.5;
        if let Some(sfx) = sfx.as_mut() {
            let pool = if class.sections.first().is_some_and(|s| s.material.contains("Iron")) { "Metal" } else { "Wood" };
            sfx.write(crate::audio::Sfx { name: format!("Ship_Collision_{pool}0{}", 1 + (time.elapsed_secs() * 7.0) as u32 % 5), at: Some(at), vol: 1.0, max: 200.0 });
        }
    };
    let tows = mission.as_ref().map_or(&[][..], |m| m.script.tows());
    cables.retain(|link, _| tows.contains(link));
    for &(tower, towee) in tows {
        if cables.contains_key(&(tower, towee)) {
            continue;
        }
        if let Ok([(_, a, ta, ha, _, _), (_, b, tb, hb, _, _)]) = ships.get_many_mut([tower, towee])
            && !ha.destroyed
            && !hb.destroyed
        {
            let p = ta.transform_point(lib.classes[a.class].tow_points[0]);
            let q = tb.transform_point(lib.classes[b.class].tow_points[1]);
            // PROVISIONAL: retain the registration distance as cable length; native spring constants are not decoded.
            cables.insert((tower, towee), (p - q).with_y(0.0).length());
        }
    }
    // The original's collision avoider (`avoid`) for every AI-steered ship: not the human's own ship, not while docking.
    {
        use avoid::{Avoid, Key, Me, Other, Shape};
        let local = fleets.as_ref().map(|f| f.local.as_str());
        // The human's ships (`Captain_Human` in the original): the player ship and the human's fleet; allied computer fleets share the side.
        let humans = |e: Entity, side: crate::Side| side == crate::Side::Player && (human.contains(e) || members.get(e).ok().is_none_or(|m| Some(m.0.as_str()) == local));
        let priority = |e: Entity, s: &crate::Ship| s.speed.abs() + if s.speed.abs() >= 0.001 && humans(e, s.side) { avoid::HUMAN_PRIORITY } else { 0.0 };
        let mut others: Vec<Other> = vec![];
        for (oe, o, v) in &obstacles {
            if v != Some(&Visibility::Hidden) && !ghost(oe) {
                let (centre, radius) = o.sphere();
                others.push(Other { key: Key::Entity(oe), centre, radius, priority: 0.0, lifeboat: false, shape: Shape::Fixed { obstacle: o, island: true } });
            }
        }
        for (i, (r, _, o, _, v)) in rocks.iter().enumerate() {
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
            // The human's own ship avoids only on its dock routine (the original's dock orders run through the avoider too).
            if health.destroyed || docker.map_or(human.contains(e), |d| !d.avoids()) || pinned(e) {
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
    // shortcut: small adaptive steps instead of continuous OBB sweeps; upgrade if speeds exceed retail orders.
    let fastest = ships.iter().map(|(_, s, _, _, d, _)| s.speed.abs() + d.0.length() + 5.0).chain(rocks.iter().map(|(_, _, _, d, _)| d.0.length())).fold(0.0, f32::max);
    let steps = (fastest * dt / 0.5).max(dt * 60.0).ceil().clamp(1.0, 32.0) as usize; // cap: a runaway drift must not freeze the frame
    let substep = dt / steps as f32;
    for _ in 0..steps {
        for (rock, mut tf, mut obstacle, drift, visible) in &mut rocks {
            if rock.hp <= 0.0 || visible == Some(&Visibility::Hidden) || drift.0 == Vec3::ZERO {
                continue;
            }
            let delta = drift.0 * substep;
            tf.translation += delta;
            obstacle.translate(delta);
        }
        for (e, mut s, mut tf, health, mut drift, docker) in &mut ships {
            if health.destroyed {
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
            crate::step(&mut s, &mut tf, class, health, substep);
            // Etherium currents push along their path (environment.rs); force / mass, as for thrust.
            for c in &currents {
                drift.0 += c.force(tf.translation) / class.phys.mass * substep;
            }
            // PROVISIONAL lateral damping; the original has lateral drag but its full mapping is not recovered.
            drift.0 *= (-2.0 * substep).exp();
            tf.translation += drift.0 * substep;
            if ghost(e) {
                continue;
            }
            for (oe, obstacle, visible) in &obstacles {
                if visible == Some(&Visibility::Hidden) || ghost(oe) || docker.is_some_and(|d| d.ignores_collision(oe)) {
                    continue;
                }
                // Rebuild after each correction: adjacent faces must not use a stale box.
                for _ in 0..3 {
                    let body = box_body(class.collision_bounds, &tf);
                    let contacts = static_contacts(&body, obstacle);
                    let Some(contact) = closest_contact(contacts) else { break };
                    let body = ship_body(&s, &tf, &drift, class, contact.point);
                    let j = impulse(&body, &fixed_body(), contact.push.normalize());
                    tf.translation += contact.push;
                    bump(class, contact.point, j);
                    if let Some(r) = rams.as_mut().filter(|_| j.length() > RAM_IMPULSE) {
                        r.write(Ram { a: e, b: e, impulse: j.length(), at: contact.point });
                    }
                    apply_impulse(&mut s, &mut drift, &body, j);
                }
            }
            // PROVISIONAL: native asteroid restitution/inertia remain unresolved.
            // shortcut: share the planar ship solver for rocks; upgrade when native asteroid response is decoded.
            for (mut rock, mut rock_tf, mut obstacle, mut rock_drift, visible) in &mut rocks {
                if rock.hp <= 0.0 || visible == Some(&Visibility::Hidden) {
                    continue;
                }
                for _ in 0..3 {
                    let body = box_body(class.collision_bounds, &tf);
                    let Some(contact) = closest_contact(static_contacts(&body, &obstacle)) else { break };
                    let a = ship_body(&s, &tf, &drift, class, contact.point);
                    let size = obstacle.body.bounds.max - obstacle.body.bounds.min;
                    let b = ContactBody {
                        velocity: rock.velocity + rock_drift.0,
                        spin: rock.angular_velocity().y,
                        inverse_mass: 1.0 / rock.mass,
                        inverse_inertia: 12.0 / (rock.mass * (size.x * size.x + size.z * size.z).max(0.001)),
                        arm: (contact.point - rock_tf.translation).with_y(0.0),
                    };
                    let j = impulse(&a, &b, contact.push.normalize());
                    tf.translation += contact.push * (a.inverse_mass / (a.inverse_mass + b.inverse_mass));
                    let delta = -contact.push * (b.inverse_mass / (a.inverse_mass + b.inverse_mass));
                    rock_tf.translation += delta;
                    obstacle.translate(delta);
                    bump(class, contact.point, j);
                    apply_impulse(&mut s, &mut drift, &a, j);
                    rock_drift.0 -= j * b.inverse_mass;
                    rock.add_contact_spin(-b.arm.cross(j).y * b.inverse_inertia);
                }
            }
        }
        // Slack cables only pull, never push; both hulls remain independently simulated.
        for (&(tower, towee), &length) in cables.iter() {
            let Ok([(_, mut a, mut ta, ha, mut da, _), (_, mut b, mut tb, hb, mut db, _)]) = ships.get_many_mut([tower, towee]) else { continue };
            if ha.destroyed || hb.destroyed {
                continue;
            }
            let ca = &lib.classes[a.class];
            let cb = &lib.classes[b.class];
            let p = ta.transform_point(ca.tow_points[0]);
            let q = tb.transform_point(cb.tow_points[1]);
            let separation = (p - q).with_y(0.0);
            let distance = separation.length();
            if distance <= length || distance < 0.001 {
                continue;
            }
            let normal = -separation / distance;
            let ba = ship_body(&a, &ta, &da, ca, p);
            let bb = ship_body(&b, &tb, &db, cb, q);
            let j = impulse(&ba, &bb, normal);
            let error = separation * ((distance - length) / distance);
            ta.translation -= error * (ba.inverse_mass / (ba.inverse_mass + bb.inverse_mass));
            tb.translation += error * (bb.inverse_mass / (ba.inverse_mass + bb.inverse_mass));
            apply_impulse(&mut a, &mut da, &ba, j);
            apply_impulse(&mut b, &mut db, &bb, -j);
        }
        // Two passes also settle small initial formation overlaps without moving destroyed ships.
        for _ in 0..2 {
            let mut pairs = ships.iter_combinations_mut();
            while let Some([(ea, mut a, mut ta, ha, mut da, docka), (eb, mut b, mut tb, hb, mut db, dockb)]) = pairs.fetch_next() {
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
                let Some(push) = contact(&ba.points, &bb.points) else { continue };
                let normal = push.normalize();
                let point = contact_point(&ba.points, &bb.points, normal);
                let fixed = |body: ContactBody, pin: bool| if pin { ContactBody { inverse_mass: 0.0, inverse_inertia: 0.0, ..body } } else { body };
                let a_body = fixed(ship_body(&a, &ta, &da, ca, point), pinned(ea));
                let b_body = fixed(ship_body(&b, &tb, &db, cb, point), pinned(eb));
                let total = a_body.inverse_mass + b_body.inverse_mass;
                if total <= 0.0 {
                    continue;
                }
                ta.translation += push * (a_body.inverse_mass / total);
                tb.translation -= push * (b_body.inverse_mass / total);
                let j = impulse(&a_body, &b_body, normal);
                bump(ca, point, j);
                if let Some(r) = rams.as_mut().filter(|_| j.length() > RAM_IMPULSE) {
                    r.write(Ram { a: ea, b: eb, impulse: j.length(), at: point });
                }
                apply_impulse(&mut a, &mut da, &a_body, j);
                apply_impulse(&mut b, &mut db, &b_body, -j);
            }
        }
    }
}

mod avoid;
#[cfg(test)]
mod tests;
