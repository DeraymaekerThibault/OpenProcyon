//! Asteroids: scripted path motion, breakup on destruction and the demo field.
use super::*;

/// Ordinary asteroid state; authored strength, breakup and rule-driven physics (docs/re/asteroids.md).
#[derive(Component)]
#[require(crate::collision::Drift)]
pub struct Asteroid {
    pub kind: String,
    /// TWT group from the map export; also the rules' owner of the rock. `id`: TWT world object id (fragments keep their parent's).
    pub group: String,
    pub id: i64,
    pub radius: f32,
    pub mass: f32,
    pub hp: f32,
    pub velocity: Vec3,
    /// The rock's speed cap: drawn at belt setup, or the speed a fragment or a ship contact gave it (docs/re/asteroids.md).
    pub speed: f32,
    /// The rock's own acceleration (the object definition's `acceleration`), applied along `thrust`.
    pub acceleration: f32,
    /// Direction of the push on the rock each tick: toward its current path point, or away from the ship that touched it.
    pub thrust: Vec3,
    pub axis: Vec3,
    pub tumble: f32,
    pub path: Vec<Vec3>,
    pub next: usize,
    pub mode: PathMode,
    pub forward: bool, // EndToEnd direction
}

impl Asteroid {
    pub fn angular_velocity(&self) -> Vec3 {
        self.axis * self.tumble
    }
    pub fn add_contact_spin(&mut self, delta: f32) {
        let spin = self.angular_velocity() + Vec3::Y * delta;
        self.tumble = spin.length();
        self.axis = spin.try_normalize().unwrap_or(Vec3::X);
    }
    pub fn new(kind: &str, group: &str, defs: &crate::data::AsteroidDefs) -> Option<Self> {
        let d = defs.0.get(kind)?;
        Some(Self {
            kind: kind.into(),
            group: group.into(),
            id: -1,
            radius: d.radius,
            mass: d.mass,
            hp: d.hp,
            velocity: Vec3::ZERO,
            speed: 0.0,
            acceleration: d.acceleration,
            thrust: Vec3::ZERO,
            axis: Vec3::X,
            tumble: 0.0,
            path: vec![],
            next: 0,
            mode: PathMode::ToEnd,
            forward: true,
        })
    }
    pub fn damage(&mut self, amount: f32) -> bool {
        if self.hp <= 0.0 {
            return false;
        }
        self.hp = (self.hp - amount).max(0.0);
        self.hp == 0.0
    }
    pub fn unit(&self, e: Entity, tf: &Transform) -> crate::rules::Unit {
        crate::rules::Unit { name: self.kind.as_str().into(), owner: self.group.as_str().into(), alive: self.hp > 0.0, ..crate::rules::Unit::new(e, self.id, tf.translation) }
    }
}

#[derive(Message)]
pub struct AsteroidSetup {
    pub units: Vec<Entity>,
    pub speed: [f32; 2],
    pub tumble: [f32; 2],
    pub path: Vec<Vec3>,
    pub closest: bool,
    pub mode: PathMode,
}

/// Rule 'Follow Mode' (values: to end 0, end to end 1, loop 2, teleport loop 3).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum PathMode {
    #[default]
    ToEnd,
    EndToEnd,
    Loop,
    /// At the last point the object jumps to the first and carries on (an open conveyor path).
    Teleport,
}

impl PathMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "end to end" => Self::EndToEnd,
            "loop" => Self::Loop,
            "teleport loop" => Self::Teleport,
            _ => Self::ToEnd,
        }
    }
}

pub fn asteroid_motion(
    time: Res<Time>,
    mut setups: MessageReader<AsteroidSetup>,
    (mut rng, spawn_order): (ResMut<SimRng>, Query<&crate::scenario::SpawnIndex>),
    mut rocks: Query<(Entity, &mut Asteroid, &mut Transform, &mut crate::collision::Drift)>,
) {
    for setup in setups.read() {
        assert!(
            setup.speed.iter().chain(&setup.tumble).all(|v| v.is_finite() && *v >= 0.0) && setup.speed[0] <= setup.speed[1] && setup.tumble[0] <= setup.tumble[1],
            "Invalid asteroid setup intervals"
        );
        // In spawn order: each rock draws its velocity and axis from the battle generator.
        for e in crate::scenario::in_spawn_order(&spawn_order, setup.units.iter().copied(), |e| *e) {
            let Ok((_, mut rock, tf, mut drift)) = rocks.get_mut(e) else { continue };
            drift.0 = Vec3::ZERO; // A new rule-controlled velocity replaces previous contact motion.
            let speed = rng.range(setup.speed[0], setup.speed[1]);
            rock.velocity = rng.direction() * speed;
            rock.speed = speed;
            rock.axis = rng.direction();
            rock.tumble = rng.range(setup.tumble[0], setup.tumble[1]);
            rock.path = rock_path(&setup.path, tf.translation);
            rock.next = if setup.closest { closest_point(&rock.path, tf.translation) } else { 0 };
            rock.mode = setup.mode;
            rock.forward = true;
        }
    }
    let dt = time.delta_secs();
    for (_, mut rock, mut tf, _) in &mut rocks {
        if rock.hp <= 0.0 {
            continue;
        }
        let r = &mut *rock;
        control(&mut tf.translation, &mut r.thrust, &r.path, &mut r.next, &mut r.forward, r.mode);
        physics(&mut tf.translation, &mut r.velocity, r.thrust, r.acceleration, r.speed, dt);
        // PROVISIONAL world-axis multiplication order; native axis-angle rate is recovered.
        tf.rotation = (Quat::from_axis_angle(rock.axis, rock.tumble * dt) * tf.rotation).normalize();
    }
}

/// The rock's own copy of a belt path (docs/re/asteroids.md): every point moved sideways by the rock's signed horizontal offset
/// from the path line nearest to it, and raised by the rock's height, so a belt keeps its cross-section instead of every rock
/// flying to the same points. Sideways is the horizontal normal: the segment's perpendicular at the ends, the corner bisector
/// inside. PROVISIONAL: the original's side test signs are inferred; a path of fewer than two points is left as is.
pub fn rock_path(path: &[Vec3], at: Vec3) -> Vec<Vec3> {
    if path.len() < 2 {
        return path.to_vec();
    }
    let flat = |v: Vec3| v.with_y(0.0);
    let last = path.len() - 1;
    let normal = |i: usize| {
        let d = flat(path[(i + 1).min(last)] - path[i.saturating_sub(1)]).normalize_or_zero();
        Vec3::new(-d.z, 0.0, d.x) // the path direction turned 90 degrees (to starboard of travel)
    };
    let near = (0..path.len()).min_by(|&a, &b| path[a].distance_squared(at).total_cmp(&path[b].distance_squared(at))).unwrap_or(0);
    let lateral = flat(at - path[near]).dot(normal(near));
    (0..path.len()).map(|i| path[i] + normal(i) * lateral + Vec3::Y * at.y).collect()
}

/// Find Closest Point: the nearest path point that lies within 90 degrees of the bearing to the path's last point, as seen from
/// the rock (so a rock never starts by flying back along the belt); point 0 when none does.
pub fn closest_point(path: &[Vec3], at: Vec3) -> usize {
    let Some(&end) = path.last() else { return 0 };
    let bearing = |p: Vec3| (p - at).with_y(0.0);
    let to_end = bearing(end);
    (0..path.len())
        .filter(|&i| bearing(path[i]).angle_between(to_end) < std::f32::consts::FRAC_PI_2)
        .min_by(|&a, &b| path[a].distance_squared(at).total_cmp(&path[b].distance_squared(at)))
        .unwrap_or(0)
}

/// Has the rock crossed the line through path point `i` across the path (docs/re/asteroids.md)? Horizontal test: the line runs
/// across the path (perpendicular to the segment at the two ends, along the corner bisector inside) and a point counts as reached
/// once the rock is on the far side, however far from it. A rock exactly on the point has not crossed. Travelling back (end to
/// end, PROVISIONAL) mirrors the side. A path of fewer than two points is never reached (the original's distance test has radius 0).
pub fn crossed(path: &[Vec3], i: usize, at: Vec3, forward: bool) -> bool {
    let last = path.len().wrapping_sub(1);
    if path.len() < 2 || i > last {
        return false;
    }
    let flat = |v: Vec3| v.with_y(0.0);
    let unit = |v: Vec3| flat(v).normalize_or_zero();
    let along = match i {
        0 => unit(path[1] - path[0]),
        _ if i == last => unit(path[last] - path[last - 1]),
        // Neighbour bisector: the direction out of the previous point plus the direction into the next one, so the test line is
        // the bisector itself. (A collinear point has no bisector in the original and may never be reached; here the plain
        // perpendicular line serves: PROVISIONAL.)
        _ => unit(path[i + 1] - path[i]) - unit(path[i - 1] - path[i]),
    };
    let side = flat(at - path[i]);
    side != Vec3::ZERO && side.dot(if forward { along } else { -along }) >= 0.0
}

/// The path controller, once per tick (docs/re/asteroids.md): moves on to the next point when the rock has crossed the line through
/// the current one, applies the follow mode past the last point, and aims the thrust at the current point. `to end` never reaches its
/// last point, so the rock keeps pushing at it and hovers there. A teleport loop jumps the rock to the first point and keeps its
/// velocity. An empty path leaves the thrust alone (a rock a ship touched flies away on it).
pub fn control(pos: &mut Vec3, thrust: &mut Vec3, path: &[Vec3], next: &mut usize, forward: &mut bool, mode: PathMode) {
    if path.is_empty() {
        return;
    }
    let last = path.len() - 1;
    *next = (*next).min(last);
    if !(mode == PathMode::ToEnd && *next == last && *forward) && crossed(path, *next, *pos, *forward) {
        match (mode, *forward) {
            (_, true) if *next < last => *next += 1,
            (PathMode::Loop, true) => *next = 0,
            (PathMode::Teleport, true) => {
                *pos = path[0];
                *next = 0;
            }
            // PROVISIONAL (unused by the retail data; the original's end-to-end arithmetic is faulty at the start end).
            (PathMode::EndToEnd, true) => {
                *forward = false;
                *next = last.saturating_sub(1);
            }
            (PathMode::EndToEnd, false) if *next > 0 => *next -= 1,
            (PathMode::EndToEnd, false) => {
                *forward = true;
                *next = 1.min(last);
            }
            _ => {}
        }
    }
    if let Some(dir) = (path[*next] - *pos).try_normalize() {
        *thrust = dir;
    }
}

/// One physics step of a rock (docs/re/asteroids.md): the position moves by the velocity it had, then the velocity gains
/// `acceleration x thrust x dt` and is cut back to the speed cap. The push is constant, so there is no slowing on approach.
pub fn physics(pos: &mut Vec3, velocity: &mut Vec3, thrust: Vec3, acceleration: f32, cap: f32, dt: f32) {
    *pos += *velocity * dt;
    *velocity += thrust * (acceleration * dt);
    *velocity = velocity.clamp_length_max(cap);
}

/// A ship touching a rock (docs/re/asteroids.md): the rock leaves its belt for good (waypoints cleared) and flies away from the
/// ship at a quarter of the centre distance per second; that is also its new speed cap, so the push keeps it there.
pub fn knock_off(rock: &mut Asteroid, rock_centre: Vec3, ship_centre: Vec3) {
    let away = rock_centre - ship_centre;
    rock.path.clear();
    rock.next = 0;
    rock.velocity = away * 0.25;
    rock.speed = rock.velocity.length();
    rock.thrust = away.normalize_or_zero();
}

/// Waypoint following for the nebula's meteor emitters (belt rocks use `control` and `physics`). PROVISIONAL: straight legs replace the native
/// path controller. Moves `pos` toward `path[*next]` by `speed * dt` and, on reaching it, applies
/// `mode` at the path end. Returns the velocity and whether the object has stopped for good (`to end` at the last point).
/// Callers must pass a non-empty path with `*next` in range.
pub fn follow_path(pos: &mut Vec3, path: &[Vec3], next: &mut usize, forward: &mut bool, mode: PathMode, speed: f32, dt: f32) -> (Vec3, bool) {
    let Some(&point) = path.get(*next) else { return (Vec3::ZERO, true) };
    let delta = point - *pos;
    let velocity = delta.normalize_or_zero() * speed;
    *pos += delta.normalize_or_zero() * (speed * dt).min(delta.length());
    if delta.length() > speed * dt {
        return (velocity, false);
    }
    let last = path.len() - 1;
    match (mode, *forward) {
        (_, true) if *next < last => *next += 1,
        (PathMode::Loop, true) => *next = 0,
        (PathMode::Teleport, true) => {
            *pos = path[0];
            *next = 1.min(last);
        }
        (PathMode::EndToEnd, true) => {
            *forward = false;
            *next = last.saturating_sub(1);
        }
        (PathMode::EndToEnd, false) if *next > 0 => *next -= 1,
        (PathMode::EndToEnd, false) => {
            *forward = true;
            *next = 1.min(last);
        }
        _ => return (Vec3::ZERO, true),
    }
    (velocity, false)
}

pub fn asteroid_deaths(
    mut commands: Commands,
    mut killed: MessageReader<Destroyed>,
    rocks: Query<(&Asteroid, &Transform, &crate::collision::Drift)>,
    defs: Res<crate::data::AsteroidDefs>,
    shapes: Res<crate::collision::Shapes>,
    assets: Res<AssetServer>,
    fxl: Res<FxLib>,
    (mut rng, spawn_order): (ResMut<SimRng>, Query<&crate::scenario::SpawnIndex>),
    mut sfx: MessageWriter<Sfx>,
    mut target: ResMut<Target>,
) {
    for death in crate::scenario::in_spawn_order(&spawn_order, killed.read(), |d| d.victim) {
        let Ok((parent, tf, drift)) = rocks.get(death.victim) else { continue };
        let def = &defs.0[&parent.kind];
        // PROVISIONAL: default effect scale and existing explosion pool until the native caller is fully traced.
        fx::spawn(&mut commands, &fxl, &def.effect, None, Transform::from_translation(tf.translation), Transform::from_translation(tf.translation), true);
        sfx.write(Sfx { name: "Explosion_Med01".into(), at: Some(tf.translation), vol: 1.0, max: 6000.0 });
        for (slot, kind) in def.children.iter().enumerate().filter(|(_, kind)| !kind.is_empty()) {
            let d = &defs.0[kind];
            let direction = [Vec3::X, Vec3::NEG_X, Vec3::NEG_Z, Vec3::Z, Vec3::Y, Vec3::NEG_Y, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO][slot];
            // PROVISIONAL: interpret the native collision-size field as radius; fragment orientation starts at identity.
            let child_tf = Transform::from_translation(tf.translation + direction * d.radius * 1.1);
            // PROVISIONAL: children inherit the named rule group; native owner inheritance is confirmed, group insertion is not.
            let mut child = Asteroid::new(kind, &parent.group, &defs).unwrap();
            child.id = parent.id;
            // Native stationary-parent ejection range; moving-parent velocity inheritance remains provisional.
            child.velocity = parent.velocity + drift.0 + direction * (0.8 + rng.f() * d.mass / 60000.0 * 2.2);
            child.speed = child.velocity.length();
            child.axis = rng.direction();
            child.tumble = rng.f() * std::f32::consts::FRAC_PI_2;
            let mut entity = commands.spawn((crate::Scenario, child, child_tf, crate::data::scene(&assets, &format!("{}.glb", d.scene))));
            if let Some(body) = shapes.obstacle(kind, &child_tf) {
                entity.insert(body);
            }
        }
        if target.0 == Some(death.victim) {
            target.0 = None;
            target.1 = None;
        }
        crate::queue_removal(&mut commands, death.victim);
    }
}

/// `--script asteroid`: frame a belt rock, then destroy it to smoke-test breakup and effects.
pub fn asteroid_demo(
    cfg: Res<crate::Config>,
    mut commands: Commands,
    mut orbit: ResMut<crate::Orbit>,
    player: Single<Entity, With<crate::PlayerShip>>,
    mut rocks: Query<(Entity, &mut Asteroid, &Transform)>,
    mut selected: Local<Option<Entity>>,
    mut killed: MessageWriter<Destroyed>,
) {
    if cfg.script.as_deref() != Some("asteroid") {
        return;
    }
    if cfg.frame == 90
        && let Some((e, _, tf)) = rocks.iter().find(|(_, a, _)| a.group == "Asteroid Belt" && a.kind == "Asteroid_Large")
    {
        *selected = Some(e);
        orbit.focus = Some(commands.spawn((crate::Scenario, *tf)).id());
        orbit.dist = 95.0;
        orbit.pitch = 0.35;
    }
    if cfg.frame == 180
        && let Some(e) = *selected
        && let Ok((_, mut rock, _)) = rocks.get_mut(e)
        && rock.damage(f32::MAX)
    {
        killed.write(Destroyed { by: *player, victim: e });
    }
}

#[cfg(test)]
mod tests;
