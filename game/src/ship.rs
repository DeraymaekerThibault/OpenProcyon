//! Shared ship state, movement integration and skirmish AI.
use crate::*;

/// All Stop, Ahead Slow, Ahead Full, Emergency Speed as fractions of Ahead Full speed (measured on the retail cutter, docs/re/movement.md).
pub const ORDERS: [(&str, f32); 4] = [("All Stop", 0.0), ("Ahead Slow", 0.5), ("Ahead Full", 1.0), ("Emergency Speed", 2.0)];

/// Ahead Full without crew settles at this fraction of WOT MaxSpeed (so Emergency is 1.125 x MaxSpeed). Measured on one class;
/// PROVISIONAL for the others; the engineers raise it (`Ship::crew_speed`).
pub const FULL_SPEED: f32 = 0.5625;

/// Full-rudder torque relative to the decoded turn formula; the measured turn rate needs it at both Slow and Full.
/// PROVISIONAL: the same 1.125 as Emergency speed, source not identified.
pub const TURN_SCALE: f32 = 1.125;

/// A braking ship comes to rest when it would drop through this speed (retail: 1.05 -> 0 in one tick).
pub const STOP_SNAP: f32 = 1.0;

/// Ships lean into the rudder (positive roll = starboard up = into a port turn): measured limit and rate.
pub const MAX_ROLL: f32 = 0.25;
pub const ROLL_RATE: f32 = 0.06;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Side {
    Player,
    Enemy,
}

/// The player's ship when nothing else is chosen (`--ship`, campaign start, synthetic allies): the first retail skirmish ship.
pub const DEFAULT_SHIP: &str = "Ship_Navy_Cutter";

#[derive(Component, Clone, PartialEq)]
pub struct Ship {
    pub class: usize,
    pub side: Side,
    pub heading: f32, // radians about +Y; forward is -Z at heading 0 (the GLB export maps game +y bow to -Z)
    pub speed: f32,
    pub ang_vel: f32,
    pub order: usize,
    pub waypoints: Vec<Vec3>,
    pub goal: Option<f32>,     // AI: heading to hold when there are no waypoints
    pub cap: f32,              // script 'Max Throttle Percent': fraction of the ordered speed the ship may use
    pub throttle: Option<f32>, // continuous command/captain speed fraction, independent of script cap
    pub sail_energy: f32,      // nebula drain, separate from permanent sail damage
    pub crew_speed: f32,       // engineers' factor on top speed (crew_arms::skill_factor)
    pub crew_turn: f32,        // helmsman's factor on rudder torque
}

#[derive(Component)]
pub struct PlayerShip;

/// 'AI_LOCK_POSITION' (right-click menu Hold): the ship's captain keeps it where it is; it still fires.
#[derive(Component)]
pub struct HoldPosition;

/// The name on a ship's banner (a mission ship's comes from `MissionShip`; otherwise the class name is shown).
#[derive(Component)]
pub struct ShipName(pub String);

pub fn angdiff(a: f32, b: f32) -> f32 {
    (a - b + PI).rem_euclid(TAU) - PI
}

pub type CaptainBusy = Or<(With<docking::Docker>, With<docking::Tower>, With<boarding::GrappleOrder>, With<boarding::Grapple>, With<boarding::Boarding>)>;

/// PROVISIONAL AI (mine): close to about the guns' range, then keep a broadside toward the foe and hold at slow speed.
/// Enemies hunt the nearest player-side ship; the player ship only uses it in demo (--shot) mode as an autopilot.
pub fn ai(
    lib: Res<Library>,
    cfg: Res<Config>,
    mission: Option<Res<campaign::Mission>>,
    mut ships: Query<(Entity, &Transform, &mut Ship, &Health, Has<PlayerShip>, Option<&FleetPilot>)>,
    busy: Query<(), CaptainBusy>,
    sensors: Res<crate::environment::Sensors>,
    held: Query<(), With<HoldPosition>>,
) {
    if mission.is_some() {
        return;
    }
    let snap: Vec<(Entity, Vec3, Side, bool)> = ships.iter().map(|(e, t, s, h, _, _)| (e, t.translation, s.side, !h.destroyed)).collect();
    for (e, tf, mut s, h, me, pilot) in &mut ships {
        if busy.contains(e) {
            continue;
        }
        if pilot.is_some_and(|p| p.hold || p.destination.is_some()) {
            continue;
        }
        if held.contains(e) {
            s.order = 0;
            s.goal = None;
            continue;
        }
        if h.destroyed
            || (me && (cfg.shot.is_none() || cfg.script.as_deref() == Some("fleet")))
            || matches!(cfg.script.as_deref(), Some("fxtest") | Some("damage") | Some("course") | Some("speedtest"))
        {
            continue;
        }
        s.throttle = None;
        let explicit = pilot.and_then(|p| p.target);
        let foe = snap
            .iter()
            .filter(|f| f.2 != s.side && f.3 && (explicit == Some(f.0) || sensors.in_sight(e, f.0, None)) && explicit.is_none_or(|e| e == f.0))
            .min_by(|a, b| a.1.distance(tf.translation).total_cmp(&b.1.distance(tf.translation)));
        let Some(foe) = foe else {
            s.order = 0;
            s.goal = None;
            continue;
        };
        let d = foe.1 - tf.translation;
        let dist = Vec2::new(d.x, d.z).length();
        let bearing = (-d.x).atan2(-d.z); // heading that points the bow at the foe
        let engage = lib.classes[s.class].engage;
        let side = [bearing + PI / 2.0, bearing - PI / 2.0].into_iter().min_by(|a, b| angdiff(*a, s.heading).abs().total_cmp(&angdiff(*b, s.heading).abs())).unwrap();
        let (goal, order) = if dist > engage * 1.1 {
            (bearing, 2)
        } else if dist < engage * 0.5 {
            (side - 0.5 * angdiff(bearing, side).signum(), 1)
        } else {
            (side, 1)
        };
        s.goal = Some(goal);
        s.order = order;
    }
}

fn max_speed(ship: &Ship, class: &combat::Class, health: &Health) -> f32 {
    full_speed(class) * combat::speed_factor(class, health) * ship.cap * ship.sail_energy * ship.crew_speed
}

/// Undamaged Ahead Full speed.
pub fn full_speed(class: &combat::Class) -> f32 {
    class.phys.max_speed * FULL_SPEED
}

/// One movement step for a ship; also used to predict the player's course.
pub fn step(ship: &mut Ship, tf: &mut Transform, class: &combat::Class, health: &Health, dt: f32) {
    {
        let ph = &class.phys;
        let drag = ph.thrust / ph.mass / ph.max_speed; // 1/s; retail settles with this rate for every order change
        let fraction = if ship.order == 0 { 0.0 } else { ship.throttle.unwrap_or(ORDERS[ship.order].1) };
        let target = fraction * max_speed(ship, class, health);
        let mut steer = 0.0;
        let mut want = None;
        if let Some(&wp) = ship.waypoints.first() {
            let d = wp - tf.translation;
            let dist = d.length();
            // Intermediate markers are taken early (about 3 s of travel) so the ship cuts the corner instead of overshooting wide.
            let reach = if ship.waypoints.len() > 1 { (ship.speed.abs() * 3.0).max(3.0) } else { 3.0 };
            if dist < reach {
                // The original ship sails straight on past the last marker at the same order and speed.
                ship.waypoints.remove(0);
            } else {
                want = Some((-d.x).atan2(-d.z));
            }
        } else if let Some(g) = ship.goal {
            want = Some(g);
        }
        if let Some(w) = want {
            steer = (angdiff(w, ship.heading) * 2.0).clamp(-1.0, 1.0);
        }
        // Turning follows the original's ship physics (docs/re/movement.md). Assumed mapping (not verified):
        // rudder in [-1,1] is `steer`, and the torque scale is WOT MaxAngularAcceleration.
        let v = ship.speed.abs();
        let mut a = steer * TURN_SCALE * ship.crew_turn * ph.max_ang_acc * (1.0 - 1.0 / (0.1 * v + 1.0)) * combat::turn_factor(class, health);
        if v > 0.67 * ph.max_speed {
            a /= 3.0
        } else if v < 0.33 * ph.max_speed {
            a *= 5.0 / 3.0
        }
        a -= ship.ang_vel.signum() * ship.ang_vel * ship.ang_vel * ph.rot_friction; // quadratic rotational drag
        a = a.clamp(-60.0, 60.0);
        ship.ang_vel += a * dt;
        ship.heading += ship.ang_vel * dt;
        ship.speed = approach_speed(ship.speed, target, drag, dt);
        let fwd = Vec3::new(-ship.heading.sin(), 0.0, -ship.heading.cos());
        tf.translation += fwd * ship.speed * dt;
        // Roll about the bow axis toward the rudder, rate-limited. PROVISIONAL: target proportional to rudder; retail also
        // weaves slightly on a straight course and does not lean at Emergency Speed (not modelled).
        let roll = tf.rotation.to_euler(EulerRot::YXZ).2;
        let roll = roll + (steer * MAX_ROLL - roll).clamp(-ROLL_RATE * dt, ROLL_RATE * dt);
        tf.rotation = Quat::from_rotation_y(ship.heading) * Quat::from_rotation_z(roll);
    }
}

pub fn approach_speed(speed: f32, target: f32, rate: f32, dt: f32) -> f32 {
    let next = target + (speed - target) * (-rate * dt).exp();
    // The snap is the engine braking through STOP_SNAP; a slow push (collision, tow) still settles gradually.
    if target == 0.0 && (next.abs() < 0.01 || speed.abs() >= STOP_SNAP && next.abs() < STOP_SNAP) { 0.0 } else { next }
}

/// Fleet identity is not allegiance: allied skirmish players have different owners.
#[derive(Component, Clone)]
pub struct FleetMember(pub String, pub usize);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Formation {
    #[default]
    None,
    Custom,
    LineAbreast,
    EchelonRight,
    EchelonLeft,
    Column,
    Diamond,
    Convoy,
}
impl Formation {
    pub const BUTTONS: [Self; 6] = [Self::LineAbreast, Self::EchelonLeft, Self::EchelonRight, Self::Column, Self::Diamond, Self::Convoy];
    pub fn parse(name: &str) -> Self {
        match name {
            "Custom" => Self::Custom,
            "LineAbreast" => Self::LineAbreast,
            "EchelonRight" => Self::EchelonRight,
            "EchelonLeft" => Self::EchelonLeft,
            "Column" => Self::Column,
            "Diamond" => Self::Diamond,
            "Convoy" => Self::Convoy,
            _ => Self::None,
        }
    }
    pub fn from_index(i: usize) -> Self {
        [Self::None, Self::Custom, Self::LineAbreast, Self::EchelonRight, Self::EchelonLeft, Self::Column, Self::Diamond, Self::Convoy].get(i).copied().unwrap_or_default()
    }
}

#[derive(Default, Clone)]
pub struct Fleet {
    pub members: Vec<Entity>,
    pub formation: Formation,
    pub cease_fire: bool,
}
#[derive(Resource, Default)]
pub struct Fleets {
    pub groups: std::collections::BTreeMap<String, Fleet>,
    pub local: String,
    pub selected: Option<Entity>,
    pub bar_open: bool,
    pub commands_open: bool,
}
impl Fleets {
    pub fn roster(&self) -> &[Entity] {
        self.groups.get(&self.local).map_or(&[], |f| f.members.as_slice())
    }
    pub fn primary(&self) -> Option<Entity> {
        self.roster().first().copied()
    }
    pub fn direct(&self) -> bool {
        self.selected.is_some() && self.selected == self.primary()
    }
    pub fn select(&mut self, e: Entity, orbit: &mut Orbit) -> bool {
        if !self.roster().contains(&e) {
            return false;
        }
        // PROVISIONAL: follow new selections (manual default); native preference flag is unknown.
        self.selected = Some(e);
        orbit.focus = Some(e);
        true
    }
    pub fn cycle(&mut self, forward: bool, orbit: &mut Orbit) {
        let Some(i) = self.roster().iter().position(|e| Some(*e) == self.selected) else { return };
        let n = self.roster().len();
        let e = self.roster()[(i + if forward { 1 } else { n - 1 }) % n];
        self.select(e, orbit);
    }
}

/// Captain orders and formation hold are per ship, never inferred from selection or camera focus.
#[derive(Component, Default, Clone, PartialEq)]
pub struct FleetPilot {
    pub hold: bool,
    pub slot: Vec3,
    pub target: Option<Entity>,
    pub destination: Option<Vec3>,
    pub hold_fire: bool,
}

impl FleetPilot {
    pub fn move_to(&mut self, point: Vec3) {
        self.hold = false;
        self.destination = Some(point);
        self.target = None;
    }
    pub fn attack(&mut self, target: Entity) {
        self.target = Some(target);
        self.destination = None;
    }
}

/// Native layout construction in game axes, converted to leader-local Bevy coordinates.
/// PROVISIONAL: section-box radius and stable roster assignment replace native size/cost assignment.
pub fn formation_slots(kind: Formation, radii: &[f32]) -> Vec<Vec3> {
    let r = radii.iter().copied().fold(0.0, f32::max);
    let mut slots = vec![Vec3::ZERO; radii.len()];
    for i in 1..slots.len() {
        let sign = if i % 2 == 1 { -1.0 } else { 1.0 };
        let (parent, delta) = match kind {
            Formation::LineAbreast => (i.saturating_sub(2), Vec3::X * sign * (r + 30.0)),
            Formation::Column => (i - 1, Vec3::Z * (2.0 * r + 30.0)),
            Formation::EchelonLeft | Formation::EchelonRight => {
                let sign = if kind == Formation::EchelonLeft { -1.0 } else { 1.0 };
                (i - 1, Vec3::new(sign * 35f32.to_radians().cos(), 0.0, 35f32.to_radians().sin()) * (r + 30.0))
            }
            Formation::Diamond | Formation::Convoy => {
                let angle = if kind == Formation::Diamond { 35f32 } else { 45f32 }.to_radians();
                (i.saturating_sub(2), Vec3::new(sign * angle.cos(), 0.0, angle.sin()) * (r + 30.0))
            }
            _ => (0, Vec3::ZERO),
        };
        slots[i] = slots[parent] + delta;
    }
    slots
}

fn captain_target_valid(world: &World, actor: Entity, target: Entity) -> bool {
    world.get_entity(target).is_ok()
        && world.get::<Health>(target).is_none_or(|h| !h.destroyed)
        && world.get::<combat::Asteroid>(target).is_none_or(|a| a.hp > 0.0)
        && world.get::<environment::SpaceAnimal>(target).is_none_or(|a| a.visible)
        && world.get::<Ship>(actor).zip(world.get::<Ship>(target)).is_none_or(|(a, b)| a.side != b.side)
}

/// Reconcile live ownership, retaining roster order. PROVISIONAL loss fallback: first living member;
/// first surviving member becomes command ship after death/capture; empty fleets have no command ship.
pub fn reconcile_fleets(world: &mut World) {
    let mut live: Vec<_> = world
        .query::<(Entity, &Ship, &Health, &FleetMember, Option<&campaign::MissionShip>)>()
        .iter(world)
        .filter(|(_, _, h, _, _)| !h.destroyed)
        .map(|(e, _, _, f, m)| (e, m.map_or_else(|| f.0.clone(), |m| m.owner.clone()), f.1))
        .collect();
    live.sort_by_key(|s| (s.2, s.0.to_bits()));
    world.resource_scope(|world, mut fleets: Mut<Fleets>| {
        for (owner, fleet) in &mut fleets.groups {
            fleet.members.retain(|e| live.iter().any(|s| s.0 == *e && s.1 == *owner));
        }
        for (e, owner, _) in &live {
            let f = fleets.groups.entry(owner.clone()).or_default();
            let joined = !f.members.contains(e);
            if joined {
                f.members.push(*e);
            }
            // PROVISIONAL joins/captures start detached; inherit fleet cease-fire, not a guessed formation slot.
            if joined || world.get::<FleetPilot>(*e).is_none() {
                world.entity_mut(*e).insert(FleetPilot { hold_fire: f.cease_fire, ..default() });
            }
        }
        let command = world.query_filtered::<Entity, With<PlayerShip>>().iter(world).next();
        let valid = command.filter(|e| live.iter().any(|s| s.0 == *e && s.1 == fleets.local));
        let local = fleets.local.clone();
        if let Some(e) = valid
            && let Some(f) = fleets.groups.get_mut(&local)
        {
            f.members.retain(|m| *m != e);
            f.members.insert(0, e);
        }
        if valid.is_none() {
            if let Some(e) = command {
                world.entity_mut(e).remove::<PlayerShip>();
            }
            if let Some(e) = fleets.primary() {
                let target = world.get::<FleetPilot>(e).and_then(|p| p.target);
                install_primary(world, e, target);
            }
        }
        if fleets.selected.is_none_or(|e| !fleets.roster().contains(&e)) {
            fleets.selected = fleets.primary();
        }
        let pilots: Vec<_> = world.query::<(Entity, &FleetPilot)>().iter(world).filter_map(|(e, p)| p.target.filter(|t| !captain_target_valid(world, e, *t)).map(|_| e)).collect();
        for e in pilots {
            world.get_mut::<FleetPilot>(e).unwrap().target = None;
        }
        if let Some(actor) = fleets.primary()
            && world.resource::<Target>().0.is_some_and(|t| !captain_target_valid(world, actor, t))
        {
            world.resource_mut::<Target>().0 = None;
            world.resource_mut::<Target>().1 = None;
        }
        let stale_focus = world.resource::<Orbit>().focus.is_some_and(|e| world.get_entity(e).is_err() || world.get::<Health>(e).is_some_and(|h| h.destroyed));
        if stale_focus {
            world.resource_mut::<Orbit>().focus = None;
        }
        let primaries: Vec<_> = world.query_filtered::<Entity, With<PlayerShip>>().iter(world).collect();
        for e in primaries {
            if live.iter().any(|s| s.0 == e && s.1 != fleets.local) {
                world.entity_mut(e).remove::<PlayerShip>();
            }
        }
    });
}

pub fn set_formation(world: &mut World, owner: &str, kind: Option<Formation>, hold: bool) {
    reconcile_fleets(world);
    let Some(fleet) = world.resource_mut::<Fleets>().groups.get_mut(owner).map(|f| {
        if let Some(k) = kind {
            f.formation = k;
        }
        f.clone()
    }) else {
        return;
    };
    let radii: Vec<_> = fleet.members.iter().map(|e| world.get::<Ship>(*e).map_or(0.0, |s| world.resource::<Library>().classes[s.class].radius)).collect();
    let slots = formation_slots(fleet.formation, &radii);
    let leader = fleet.members.first().and_then(|e| world.get::<Transform>(*e)).copied().unwrap_or_default();
    for (i, e) in fleet.members.iter().enumerate().skip(1) {
        let custom = world.get::<Transform>(*e).map_or(Vec3::ZERO, |t| leader.rotation.inverse() * (t.translation - leader.translation)).with_y(0.0);
        let was_held = world.get::<FleetPilot>(*e).is_some_and(|p| p.hold);
        if let Some(mut p) = world.get_mut::<FleetPilot>(*e) {
            if kind.is_some() {
                p.slot = if fleet.formation == Formation::Custom { custom } else { slots[i] };
            }
            p.hold = hold && fleet.formation != Formation::None;
            if hold {
                p.destination = None;
            }
        }
        if (hold || was_held)
            && let Some(mut s) = world.get_mut::<Ship>(*e)
        {
            s.waypoints.clear();
            s.goal = None;
            if !hold {
                s.order = 0;
                s.throttle = None;
            }
        }
    }
}

fn install_primary(world: &mut World, candidate: Entity, target: Option<Entity>) {
    let max = world.get::<Ship>(candidate).zip(world.get::<Health>(candidate)).map_or(0.0, |(s, h)| max_speed(s, &world.resource::<Library>().classes[s.class], h));
    if let Some(mut s) = world.get_mut::<Ship>(candidate) {
        s.throttle = Some(if max > 0.001 { (s.speed.abs() / max).clamp(0.0, 1.0) } else { 0.0 });
        s.order = 2;
    }
    world.entity_mut(candidate).insert((PlayerShip, FleetPilot::default()));
    if let Some(mut m) = world.get_mut::<campaign::MissionShip>(candidate) {
        m.hold_fire = false;
        m.target = None;
        m.attack.clear();
    }
    world.resource_mut::<Target>().0 = target;
    world.resource_mut::<Target>().1 = None;
    world.resource_mut::<Target>().2 = Default::default();
}

pub fn promote(world: &mut World, candidate: Entity, scripted: bool) -> bool {
    if !scripted && world.contains_resource::<campaign::Mission>() {
        return false;
    }
    let owner = world.resource::<Fleets>().local.clone();
    if scripted && world.get::<Health>(candidate).is_some_and(|h| !h.destroyed) {
        // PROVISIONAL cross-group script handoff: transfer candidate to the local fleet, retaining other group members.
        if let Some(mut m) = world.get_mut::<campaign::MissionShip>(candidate) {
            m.owner = owner.clone();
        }
    }
    reconcile_fleets(world);
    if !world.resource::<Fleets>().roster().contains(&candidate) {
        return false;
    }
    let next_target = if world.get::<PlayerShip>(candidate).is_some() { world.resource::<Target>().0 } else { world.get::<FleetPilot>(candidate).and_then(|p| p.target) };
    let old_target = world.resource::<Target>().0;
    let mut retired = Vec::new();
    let old = world.query_filtered::<Entity, With<PlayerShip>>().iter(world).collect::<Vec<_>>();
    for e in old {
        world.entity_mut(e).remove::<PlayerShip>();
        if scripted && e != candidate && world.get::<campaign::MissionShip>(e).is_some_and(|m| m.id == -1) {
            world.entity_mut(e).despawn();
            retired.push(e);
            continue;
        }
        if let Some(mut p) = world.get_mut::<FleetPilot>(e) {
            p.hold = false;
            p.target = old_target;
        }
    }
    install_primary(world, candidate, next_target);
    let f = {
        let mut fleets = world.resource_mut::<Fleets>();
        let f = fleets.groups.get_mut(&owner).unwrap();
        f.members.retain(|e| *e != candidate && !retired.contains(e));
        f.members.insert(0, candidate);
        fleets.selected = Some(candidate);
        fleets.groups.get(&owner).unwrap().clone()
    };
    let radii: Vec<_> = f.members.iter().map(|e| world.get::<Ship>(*e).map_or(0.0, |s| world.resource::<Library>().classes[s.class].radius)).collect();
    let slots = formation_slots(f.formation, &radii);
    for (i, e) in f.members.iter().enumerate().skip(1) {
        if f.formation != Formation::Custom
            && let Some(mut p) = world.get_mut::<FleetPilot>(*e)
        {
            p.slot = slots[i];
        }
    }
    true
}

pub fn fleet_help(world: &mut World, owner: &str, attack: bool) {
    let Some(fleet) = world.resource::<Fleets>().groups.get(owner).cloned() else { return };
    let target = if fleet.members.first().copied() == world.resource::<Fleets>().primary() {
        world.resource::<Target>().0
    } else {
        fleet.members.first().and_then(|e| world.get::<campaign::MissionShip>(*e)).and_then(|m| m.target)
    };
    let target = target.filter(|e| fleet.members.first().is_some_and(|leader| captain_target_valid(world, *leader, *e)));
    if attack && target.is_none() {
        return;
    }
    if !attack && let Some(f) = world.resource_mut::<Fleets>().groups.get_mut(owner) {
        f.cease_fire = !f.cease_fire;
    }
    for e in fleet.members.into_iter().skip(1) {
        if let Some(mut p) = world.get_mut::<FleetPilot>(e) {
            if attack {
                p.target = target;
                p.destination = None;
            } else {
                p.hold_fire = !fleet.cease_fire;
            }
        }
    }
}

pub fn fleet_follow(world: &mut World) {
    let fleets = world.resource::<Fleets>().groups.clone();
    let busy: std::collections::HashSet<_> = world.query_filtered::<Entity, CaptainBusy>().iter(world).collect();
    let held: std::collections::HashSet<_> = world.query_filtered::<Entity, With<HoldPosition>>().iter(world).collect();
    for e in &held {
        if world.get::<PlayerShip>(*e).is_none()
            && let Some(mut s) = world.get_mut::<Ship>(*e)
        {
            s.order = 0;
            s.waypoints.clear();
            s.goal = None;
        }
    }
    for fleet in fleets.values() {
        let Some(leader) = fleet.members.first().copied() else { continue };
        let Some(tf) = world.get::<Transform>(leader).copied() else { continue };
        let speed = world.get::<Ship>(leader).map_or(0.0, |s| s.speed);
        for e in fleet.members.iter().copied() {
            if busy.contains(&e) {
                if let Some(mut s) = world.get_mut::<Ship>(e) {
                    s.throttle = None;
                }
                if let Some(mut p) = world.get_mut::<FleetPilot>(e) {
                    p.hold = false;
                    p.destination = None;
                }
                continue;
            }
            if e == leader || held.contains(&e) {
                continue;
            }
            let Some(pilot) = world.get::<FleetPilot>(e).cloned() else { continue };
            let Some(here) = world.get::<Transform>(e).map(|t| t.translation) else { continue };
            let Some(ship) = world.get::<Ship>(e) else { continue };
            let max = world.get::<Health>(e).map_or(0.0, |h| max_speed(ship, &world.resource::<Library>().classes[ship.class], h));
            let goal = if pilot.hold { Some(tf.translation + tf.rotation * pilot.slot) } else { pilot.destination };
            if let Some(goal) = goal {
                // PROVISIONAL: captain movement keeps altitude; the shared integrator has no vertical thrust.
                let goal = goal.with_y(here.y);
                let distance = here.distance(goal);
                // PROVISIONAL proportional catch-up controller; native look-ahead/heading thresholds await calibration.
                let wanted = if pilot.hold {
                    (speed + distance * 0.15).min(max)
                } else if distance < 5.0 {
                    0.0
                } else {
                    max
                };
                if let Some(mut s) = world.get_mut::<Ship>(e) {
                    s.waypoints = if distance > 3.0 { vec![goal] } else { vec![] };
                    s.goal = if pilot.hold { Some((-(tf.rotation * Vec3::NEG_Z).x).atan2(-(tf.rotation * Vec3::NEG_Z).z)) } else { None };
                    s.order = if wanted < 0.01 { 0 } else { 2 };
                    s.throttle = Some(if max > 0.001 { (wanted / max).clamp(0.0, 1.0) } else { 0.0 });
                }
            }
        }
    }
}

#[cfg(test)]
pub mod movement_tests {
    use super::*;
    pub fn fleet_world() -> (World, [Entity; 6]) {
        let mut w = World::new();
        let lib = Library::load(&assets_dir());
        let ci = lib.index("Ship_Navy_Cutter");
        let class = &lib.classes[ci];
        let ids = std::array::from_fn(|i| {
            w.spawn((
                Ship {
                    class: ci,
                    side: if i == 5 { Side::Enemy } else { Side::Player },
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
                },
                Health::new(class),
                Transform::from_xyz(i as f32 * 50.0, 0.0, 0.0),
                FleetMember(
                    if i < 4 {
                        "Player"
                    } else if i == 4 {
                        "Ally"
                    } else {
                        "Enemy"
                    }
                    .into(),
                    i,
                ),
            ))
            .id()
        });
        w.entity_mut(ids[0]).insert(PlayerShip);
        w.insert_resource(lib);
        w.insert_resource(Target::default());
        w.insert_resource(Orbit { yaw: 0.0, pitch: 0.0, dist: 60.0, focus: None });
        w.insert_resource(Fleets { local: "Player".into(), ..default() });
        reconcile_fleets(&mut w);
        (w, ids)
    }

    #[test]
    fn secondary_selection_focus_and_orders_do_not_promote_or_touch_siblings() {
        let (mut w, ids) = fleet_world();
        w.resource_scope(|w, mut f: Mut<Fleets>| {
            assert!(f.select(ids[2], &mut w.resource_mut::<Orbit>()));
            assert!(!f.direct());
        });
        assert!(w.get::<PlayerShip>(ids[0]).is_some());
        assert!(w.get::<PlayerShip>(ids[2]).is_none());
        w.resource_mut::<Orbit>().focus = Some(ids[5]);
        let actor = w.resource::<Fleets>().selected.unwrap();
        w.get_mut::<FleetPilot>(actor).unwrap().move_to(Vec3::new(100.0, 0.0, -200.0));
        assert!(w.get::<FleetPilot>(ids[0]).unwrap().destination.is_none());
        assert!(w.get::<FleetPilot>(ids[1]).unwrap().destination.is_none());
        w.get_mut::<FleetPilot>(actor).unwrap().attack(ids[5]);
        assert_eq!(w.get::<FleetPilot>(actor).unwrap().target, Some(ids[5]));
        assert_eq!(w.resource::<Target>().0, None);
        assert_eq!(w.resource::<Fleets>().primary(), Some(ids[0]));
        w.resource_scope(|w, mut f: Mut<Fleets>| {
            assert!(!f.select(ids[4], &mut w.resource_mut::<Orbit>()));
        });
    }

    #[test]
    fn promotion_checks_membership_reorders_and_preserves_speed_not_camera() {
        let (mut w, ids) = fleet_world();
        assert!(!promote(&mut w, ids[4], false));
        assert!(!promote(&mut w, ids[5], false));
        let max = full_speed(&w.resource::<Library>().classes[w.get::<Ship>(ids[2]).unwrap().class]);
        w.get_mut::<Ship>(ids[2]).unwrap().speed = max * 0.6;
        w.get_mut::<Ship>(ids[2]).unwrap().cap = 0.75;
        w.resource_mut::<Orbit>().focus = Some(ids[5]);
        assert!(promote(&mut w, ids[2], false));
        assert_eq!(w.resource::<Fleets>().roster(), &[ids[2], ids[0], ids[1], ids[3]]);
        assert_eq!(w.resource::<Orbit>().focus, Some(ids[5]));
        assert!(w.get::<PlayerShip>(ids[0]).is_none());
        assert!((w.get::<Ship>(ids[2]).unwrap().throttle.unwrap() - 0.8).abs() < 0.001);
        assert_eq!(w.get::<Ship>(ids[2]).unwrap().cap, 0.75, "command fraction must not overwrite the mission speed cap");
        assert!(promote(&mut w, ids[2], false)); // Already primary still resets.
        assert!(promote(&mut w, ids[1], false));
        assert_eq!(w.get::<Ship>(ids[1]).unwrap().throttle, Some(0.0));
        w.insert_resource(campaign::load_id("mission05"));
        assert!(!promote(&mut w, ids[2], false));
        assert!(promote(&mut w, ids[2], true));
    }

    #[test]
    fn scripted_cross_group_primary_retires_standin_and_updates_mission_identity() {
        let (mut w, ids) = fleet_world();
        let ms = |id, owner: &str, name: &str| campaign::MissionShip {
            id,
            owner: owner.into(),
            name: name.into(),
            hold_fire: false,
            invuln: 0.0,
            visible: true,
            stance: rules::Stance::Neutral,
            target: None,
            attack: vec![],
            flag: String::new(),
            banner: String::new(),
            looping: None,
            stop_at_end: false,
            create_lifeboats: false,
            ..Default::default()
        };
        w.entity_mut(ids[0]).insert(ms(-1, "Player", "RLS Stand-in"));
        w.entity_mut(ids[4]).insert(ms(99, "Players Ships", "RLS St Roch"));
        w.insert_resource(campaign::load_id("mission09"));
        assert!(!promote(&mut w, ids[4], false));
        assert!(promote(&mut w, ids[4], true));
        assert!(w.get_entity(ids[0]).is_err());
        assert_eq!(w.resource::<Fleets>().primary(), Some(ids[4]));
        assert_eq!(w.resource::<Fleets>().selected, Some(ids[4]));
        assert_eq!(w.get::<campaign::MissionShip>(ids[4]).unwrap().owner, "Player");
        assert_eq!(w.query_filtered::<Entity, With<PlayerShip>>().iter(&w).count(), 1);
        assert!(!w.resource::<Fleets>().roster().contains(&ids[0]));
    }

    #[test]
    fn formation_detach_attack_rejoin_release_and_fleet_help_are_per_secondary() {
        let (mut w, ids) = fleet_world();
        set_formation(&mut w, "Player", Some(Formation::Diamond), true);
        assert!(!w.get::<FleetPilot>(ids[0]).unwrap().hold);
        w.get_mut::<FleetPilot>(ids[1]).unwrap().move_to(Vec3::X * 100.0);
        w.get_mut::<FleetPilot>(ids[2]).unwrap().attack(ids[5]);
        assert!(!w.get::<FleetPilot>(ids[1]).unwrap().hold);
        assert!(w.get::<FleetPilot>(ids[2]).unwrap().hold);
        assert!(w.get::<FleetPilot>(ids[3]).unwrap().hold);
        set_formation(&mut w, "Player", Some(Formation::Diamond), true);
        assert!(ids[1..4].iter().all(|e| w.get::<FleetPilot>(*e).unwrap().hold));
        assert!(w.get::<FleetPilot>(ids[1]).unwrap().destination.is_none());
        set_formation(&mut w, "Player", Some(Formation::Diamond), false);
        assert_eq!(w.resource::<Fleets>().groups["Player"].formation, Formation::Diamond);
        assert!(ids[1..4].iter().all(|e| !w.get::<FleetPilot>(*e).unwrap().hold));
        w.get_mut::<FleetPilot>(ids[2]).unwrap().target = None;
        fleet_help(&mut w, "Player", true); // No primary target: no invented attack.
        assert!(w.get::<FleetPilot>(ids[2]).unwrap().target.is_none());
        w.resource_mut::<Target>().0 = Some(ids[5]);
        fleet_help(&mut w, "Player", true);
        assert!(ids[1..4].iter().all(|e| w.get::<FleetPilot>(*e).unwrap().target == Some(ids[5])));
        fleet_help(&mut w, "Player", false);
        assert!(!w.get::<FleetPilot>(ids[0]).unwrap().hold_fire);
        assert!(ids[1..4].iter().all(|e| w.get::<FleetPilot>(*e).unwrap().hold_fire));
        fleet_help(&mut w, "Player", false);
        assert!(ids[1..4].iter().all(|e| !w.get::<FleetPilot>(*e).unwrap().hold_fire));
    }

    #[test]
    fn all_layouts_custom_and_rotated_following_preserve_altitude_and_limits() {
        let radii = [10.0, 20.0, 15.0, 5.0];
        assert_eq!(formation_slots(Formation::LineAbreast, &radii), vec![Vec3::ZERO, Vec3::new(-50.0, 0.0, 0.0), Vec3::new(50.0, 0.0, 0.0), Vec3::new(-100.0, 0.0, 0.0)]);
        assert_eq!(formation_slots(Formation::Column, &radii)[3], Vec3::Z * 210.0);
        for kind in Formation::BUTTONS {
            let v = formation_slots(kind, &radii);
            assert_eq!(v.len(), 4);
            assert!(v[1].length() > 0.0);
            assert_eq!(v[1].y, 0.0);
        }
        assert_eq!(formation_slots(Formation::None, &radii), vec![Vec3::ZERO; 4]);
        let (mut w, ids) = fleet_world();
        w.get_mut::<Transform>(ids[0]).unwrap().rotation = Quat::from_rotation_y(PI / 2.0);
        set_formation(&mut w, "Player", Some(Formation::Custom), true);
        let before = w.get::<Transform>(ids[1]).unwrap().translation;
        fleet_follow(&mut w);
        assert_eq!(w.get::<Ship>(ids[1]).unwrap().waypoints, Vec::<Vec3>::new()); // Already at its captured slot.
        assert_eq!(w.get::<Transform>(ids[1]).unwrap().translation, before);
        set_formation(&mut w, "Player", Some(Formation::Column), true);
        let slot = w.get::<FleetPilot>(ids[1]).unwrap().slot;
        fleet_follow(&mut w);
        let leader = w.get::<Transform>(ids[0]).unwrap();
        let expected = leader.translation + leader.rotation * slot;
        assert!(w.get::<Ship>(ids[1]).unwrap().waypoints[0].distance(expected) < 0.001);
        assert!((0.0..=1.0).contains(&w.get::<Ship>(ids[1]).unwrap().throttle.unwrap()));
        w.get_mut::<Transform>(ids[1]).unwrap().translation.y = 25.0;
        w.get_mut::<Ship>(ids[1]).unwrap().cap = 0.2;
        w.get_mut::<Health>(ids[1]).unwrap().sections.iter_mut().for_each(|h| *h *= 0.25);
        fleet_follow(&mut w);
        assert_eq!(w.get::<Ship>(ids[1]).unwrap().waypoints[0].y, 25.0);
        assert_eq!(w.get::<Ship>(ids[1]).unwrap().cap, 0.2);
        assert!((0.0..=1.0).contains(&w.get::<Ship>(ids[1]).unwrap().throttle.unwrap()));
        set_formation(&mut w, "Player", Some(Formation::None), false);
        assert!(ids[1..4].iter().all(|e| !w.get::<FleetPilot>(*e).unwrap().hold));
    }

    #[test]
    fn destruction_despawn_empty_and_scenario_reset_clear_stale_fleet_state() {
        let (mut w, ids) = fleet_world();
        w.resource_mut::<Fleets>().selected = Some(ids[1]);
        w.resource_mut::<Orbit>().focus = Some(ids[1]);
        w.get_mut::<Health>(ids[1]).unwrap().destroyed = true;
        reconcile_fleets(&mut w);
        assert_eq!(w.resource::<Fleets>().selected, Some(ids[0]));
        assert_eq!(w.resource::<Orbit>().focus, None);
        w.despawn(ids[0]);
        reconcile_fleets(&mut w);
        assert_eq!(w.resource::<Fleets>().primary(), Some(ids[2]));
        assert!(w.get::<PlayerShip>(ids[2]).is_some());
        w.get_mut::<FleetMember>(ids[2]).unwrap().0 = "Enemy".into();
        reconcile_fleets(&mut w);
        assert_eq!(w.resource::<Fleets>().roster(), &[ids[3]]);
        assert!(w.get::<PlayerShip>(ids[2]).is_none());
        w.get_mut::<FleetPilot>(ids[3]).unwrap().target = Some(ids[5]);
        w.get_mut::<Ship>(ids[5]).unwrap().side = Side::Player;
        reconcile_fleets(&mut w);
        assert!(w.get::<FleetPilot>(ids[3]).unwrap().target.is_none(), "capture/alliance changes must not leave a friendly attack target");
        w.despawn(ids[3]);
        reconcile_fleets(&mut w);
        assert_eq!(w.resource::<Fleets>().selected, None);
        w.resource_mut::<Fleets>().bar_open = true;
        w.resource_mut::<Fleets>().commands_open = true;
        w.insert_resource(Time::<Virtual>::default());
        w.resource_mut::<Time<Virtual>>().pause();
        scenario::resume_scenario(&mut w);
        assert!(!w.resource::<Time<Virtual>>().is_paused());
        assert!(w.resource::<Fleets>().groups.is_empty());
        assert!(!w.resource::<Fleets>().bar_open);
        assert!(!w.resource::<Fleets>().commands_open);
    }

    #[test]
    fn rebound_fleet_keys_cycle_including_primary_and_never_change_time_scale() {
        use bevy::ecs::system::RunSystemOnce;
        let (mut w, ids) = fleet_world();
        w.insert_resource(bindings::Bindings::default());
        w.insert_resource(ButtonInput::<KeyCode>::default());
        w.insert_resource(Time::<Virtual>::default());
        w.insert_resource(Messages::<audio::Sfx>::default());
        let press = |w: &mut World, key| {
            w.resource_mut::<ButtonInput<KeyCode>>().reset_all();
            w.resource_mut::<ButtonInput<KeyCode>>().press(key);
            w.run_system_once(view::fleet_keys).unwrap();
        };
        press(&mut w, KeyCode::Digit2);
        assert_eq!(w.resource::<Fleets>().selected, Some(ids[2]));
        assert!(w.get::<PlayerShip>(ids[0]).is_some());
        press(&mut w, KeyCode::Digit0); // Out of range.
        assert_eq!(w.resource::<Fleets>().selected, Some(ids[2]));
        press(&mut w, KeyCode::BracketRight);
        press(&mut w, KeyCode::BracketRight);
        assert_eq!(w.resource::<Fleets>().selected, Some(ids[0]));
        press(&mut w, KeyCode::BracketLeft);
        assert_eq!(w.resource::<Fleets>().selected, Some(ids[3]));
        assert_eq!(w.resource::<Time<Virtual>>().relative_speed(), 1.0);
        let row = w.resource::<bindings::Bindings>().rows.iter().position(|r| r.0 == bindings::Act::Primary).unwrap();
        w.resource_mut::<bindings::Bindings>().rows[row].2 = KeyCode::Quote;
        press(&mut w, KeyCode::Quote);
        assert_eq!(w.resource::<Fleets>().selected, Some(ids[0]));
        w.resource_mut::<Fleets>().selected = Some(ids[4]);
        press(&mut w, KeyCode::BracketLeft);
        assert_eq!(w.resource::<Fleets>().selected, Some(ids[4]));
    }

    #[test]
    fn pause_toggle_freezes_virtual_time_and_keeps_speed() {
        let mut vtime = Time::<Virtual>::default();
        vtime.set_relative_speed(4.0);
        toggle_pause(&mut vtime);
        assert!(vtime.is_paused());
        assert_eq!(vtime.relative_speed(), 4.0, "pausing keeps the speed setting");
        toggle_pause(&mut vtime);
        assert!(!vtime.is_paused());
        assert_eq!(vtime.relative_speed(), 4.0, "resuming keeps the speed setting");
    }
    #[test]
    fn fresh_scenario_resumes_paused_time_without_changing_speed() {
        let mut world = World::new();
        let mut time = Time::<Virtual>::default();
        set_time_scale(&mut time, 4.0);
        time.pause();
        world.insert_resource(time);
        let mut queue = bevy::ecs::world::CommandQueue::default();
        Commands::new(&mut queue, &world).queue(resume_scenario);
        queue.apply(&mut world);
        let time = world.resource::<Time<Virtual>>();
        assert!(!time.is_paused());
        assert_eq!(time.relative_speed(), 4.0);
    }
    #[test]
    fn open_map_ships_get_distinct_names_from_their_type_pool() {
        let lib = combat::Library::load(&crate::assets_dir());
        let mut namer = Namer::new();
        let pool = &lib.names["Ship_Pirate_Sloop"];
        let picked: std::collections::HashSet<String> = (0..pool.len()).map(|_| namer.pick(&lib, "Ship_Pirate_Sloop").unwrap()).collect();
        assert_eq!(picked.len(), pool.len(), "no repeats until the pool is used up");
        assert!(namer.pick(&lib, "Ship_Pirate_Sloop").is_some(), "a used-up pool repeats");
        assert!(namer.pick(&lib, "Ship_Nonexistent").is_none());
    }
    #[test]
    fn time_scale_never_exceeds_max_sim_step() {
        use bevy::time::{TimePlugin, TimeUpdateStrategy};
        use std::time::Duration;
        // Virtual delta after one real frame of `raw` seconds at `speed`.
        let step = |speed: f32, raw: f32| {
            let mut app = App::new();
            app.add_plugins(TimePlugin).insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(raw)));
            set_time_scale(&mut app.world_mut().resource_mut::<Time<Virtual>>(), speed);
            app.update();
            app.update();
            app.world().resource::<Time<Virtual>>().delta_secs()
        };
        assert!((step(4.0, 0.01) - 0.04).abs() < 1e-4); // fast frame: full 4x
        assert!(step(16.0, 1.0 / 60.0) <= MAX_SIM_STEP + 1e-4); // vsync frame: capped
        assert!((step(1.0, 0.2) - 0.2).abs() < 1e-4); // 1x keeps Bevy's normal 0.25 s hitch clamp
    }

    #[test]
    fn ship_keeps_speed_and_heading_past_last_waypoint() {
        let lib = combat::Library::load(&crate::assets_dir());
        // Not class 0: the sorted ship folders now start with the (immobile) armed bases.
        let ci = lib.index("Ship_Navy_Cutter");
        let class = &lib.classes[ci];
        let mut ship = Ship {
            class: ci,
            side: Side::Player,
            heading: 0.0,
            speed: 0.0,
            ang_vel: 0.0,
            order: 3,
            waypoints: vec![Vec3::new(-30.0, 0.0, -80.0)],
            goal: None,
            cap: 1.0,
            throttle: None,
            sail_energy: 1.0,
            crew_speed: 1.0,
            crew_turn: 1.0,
        };
        let mut tf = Transform::default();
        let h = Health { sections: vec![1.0; class.sections.len()], core: 1.0, destroyed: false };
        for _ in 0..3000 {
            step(&mut ship, &mut tf, class, &h, 0.033);
        }
        assert!(ship.waypoints.is_empty());
        assert!(ship.order == 3 && ship.speed > 1.0, "speed {}", ship.speed);
    }
    /// Retail Navy Cutter without crew, sampled from the running original (docs/re/movement.md, "Measured").
    #[test]
    fn cutter_matches_measured_retail_movement() {
        let lib = combat::Library::load(&crate::assets_dir());
        let ci = lib.index("Ship_Navy_Cutter");
        let class = &lib.classes[ci];
        let h = Health { sections: vec![1.0; class.sections.len()], core: 1.0, destroyed: false };
        let dt = 0.0344; // retail update interval
        let run = |ship: &mut Ship, tf: &mut Transform, secs: f32| {
            for _ in 0..(secs / dt) as usize {
                step(ship, tf, class, &h, dt);
            }
        };
        let mut ship = Ship {
            class: ci,
            side: Side::Player,
            heading: 0.0,
            speed: 0.0,
            ang_vel: 0.0,
            order: 1,
            waypoints: vec![],
            goal: None,
            cap: 1.0,
            throttle: None,
            sail_energy: 1.0,
            crew_speed: 1.0,
            crew_turn: 1.0,
        };
        let mut tf = Transform::default();
        run(&mut ship, &mut tf, 1.0);
        assert!((ship.speed - 1.95).abs() < 0.1, "Slow from rest after 1 s: {}", ship.speed); // 0.53 s time constant
        for (order, measured) in [(1, 2.2655), (2, 4.531), (3, 9.0623)] {
            ship.order = order;
            run(&mut ship, &mut tf, 8.0);
            assert!((ship.speed - measured).abs() < 0.01, "order {order}: {} vs {measured}", ship.speed);
        }
        ship.order = 2;
        run(&mut ship, &mut tf, 8.0);
        ship.order = 0;
        run(&mut ship, &mut tf, 0.75); // retail: 23 ticks of decay to 1.05, then rest on the 24th (0.83 s)
        assert!(ship.speed > 1.0, "still slowing after 0.75 s: {}", ship.speed);
        run(&mut ship, &mut tf, 0.15);
        assert_eq!(ship.speed, 0.0, "All Stop from Full is at rest after about 0.83 s");
        for order in [1, 2] {
            ship.order = order;
            run(&mut ship, &mut tf, 8.0);
            ship.goal = Some(ship.heading + 3.0); // hard to port
            run(&mut ship, &mut tf, 3.0);
            let roll = tf.rotation.to_euler(EulerRot::YXZ).2;
            assert!(roll > 0.1 && roll < MAX_ROLL, "leaning into the turn, not yet at the limit: {roll}");
            assert!((ship.ang_vel.to_degrees() - 13.6).abs() < 0.4, "order {order} turn rate {}", ship.ang_vel.to_degrees());
            ship.goal = Some(ship.heading + 3.0);
            run(&mut ship, &mut tf, 2.0);
            assert!((tf.rotation.to_euler(EulerRot::YXZ).2 - MAX_ROLL).abs() < 1e-3, "full lean after about 4 s");
            assert!((tf.rotation * Vec3::NEG_Z).y.abs() < 1e-4, "roll never pitches the bow");
            ship.goal = None;
            ship.ang_vel = 0.0;
        }
    }

    /// Retail Navy Cutters with engineer and helmsman both at skill 5 and 9 (docs/re/crew.md): Ahead Full and full-rudder turn.
    #[test]
    fn crewed_cutter_matches_measured_retail_speed_and_turn() {
        let lib = combat::Library::load(&crate::assets_dir());
        let ci = lib.index("Ship_Navy_Cutter");
        let class = &lib.classes[ci];
        let h = Health { sections: vec![1.0; class.sections.len()], core: 1.0, destroyed: false };
        assert_eq!(crate::crew_arms::skill_factor(0.1), 1.0, "default hands change nothing");
        for (skill, full, turn) in [(0.5, 5.336, 15.61), (0.9, 6.041, 9.99)] {
            let f = crate::crew_arms::skill_factor(skill);
            let mut ship = Ship {
                class: ci,
                side: Side::Player,
                heading: 0.0,
                speed: 0.0,
                ang_vel: 0.0,
                order: 2,
                waypoints: vec![],
                goal: None,
                cap: 1.0,
                throttle: None,
                sail_energy: 1.0,
                crew_speed: f,
                crew_turn: f,
            };
            let mut tf = Transform::default();
            for _ in 0..300 {
                step(&mut ship, &mut tf, class, &h, 0.0344);
            }
            assert!((ship.speed - full).abs() < 0.01, "skill {skill}: Full {} vs {full}", ship.speed);
            ship.goal = Some(ship.heading + 3.0);
            for _ in 0..90 {
                step(&mut ship, &mut tf, class, &h, 0.0344);
            }
            assert!((ship.ang_vel.to_degrees() - turn).abs() < 0.4, "skill {skill}: turn {} vs {turn}", ship.ang_vel.to_degrees());
        }
    }

    #[test]
    fn stop_is_frame_rate_independent_and_reaches_rest() {
        let rate = 18000.0 / 2000.0 / 11.111111; // retail Comet
        let mut speed = 11.111111;
        for _ in 0..50 {
            speed = approach_speed(speed, 0.0, rate, 0.1);
        }
        assert!(speed < 0.2);
        assert!((speed - approach_speed(11.111111, 0.0, rate, 5.0)).abs() < 0.0001);
        for _ in 0..50 {
            speed = approach_speed(speed, 0.0, rate, 0.1);
        }
        assert_eq!(speed, 0.0);
        assert!(approach_speed(0.0, 11.0, rate, 100.0) <= 11.0);
    }
}
