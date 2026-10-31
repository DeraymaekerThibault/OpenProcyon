//! Shared ship state, movement integration and skirmish AI.
use crate::*;

/// All Stop, Ahead Slow, Ahead Full, Emergency Speed as fractions of Ahead Full speed (measured on the retail cutter, docs/re/movement.md).
pub const ORDERS: [(&str, f32); 4] = [("All Stop", 0.0), ("Ahead Slow", 0.5), ("Ahead Full", 1.0), ("Emergency Speed", 2.0)];

/// Ahead Full without crew settles at this fraction of WOT MaxSpeed (so Emergency is 1.125 x MaxSpeed). Measured on the cutter,
/// Man-O-War, Carrack, Barque and Minelayer (docs/re/movement.md); the engineers raise it (`Ship::crew_speed`).
pub const FULL_SPEED: f32 = 0.5625;

/// Full-rudder torque relative to the decoded turn formula. Measured: cutter at Slow, Full and Emergency, Man-O-War at Full
/// and Emergency all fit it exactly (docs/re/movement.md); the same 1.125 as Emergency speed, source not identified.
pub const TURN_SCALE: f32 = 1.125;

/// The helm puts the rudder hard over beyond this heading error and proportionally inside it (the original's waypoint steering:
/// rudder = error / 20°).
pub const HELM_FULL: f32 = 20.0 * PI / 180.0;

/// A braking ship comes to rest when it would drop through this speed (retail: 1.05 -> 0 in one tick).
pub const STOP_SNAP: f32 = 1.0;

/// Ships lean into the rudder (positive roll = starboard up = into a port turn): measured limit and rate.
pub const MAX_ROLL: f32 = 0.25;
pub const ROLL_RATE: f32 = 0.06;

/// The ship's team: 0 = the human's team, others are computer teams hostile to it and to each other (skirmish maps with three
/// teams, free-for-all). Ships of different sides fight; `Side::Enemy` is the first (and outside skirmish the only) hostile team.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Side(pub u8);

#[allow(non_upper_case_globals)]
impl Side {
    pub const Player: Side = Side(0);
    pub const Enemy: Side = Side(1);
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
    pub avoid: f32,            // speed fraction with land close ahead (collision::contact_slow, AI ships only); 1.0 = clear
}

impl Ship {
    /// Drops the course and heading goal and orders a stop (the speed bleeds off on its own).
    pub fn halt(&mut self) {
        self.waypoints.clear();
        self.goal = None;
        self.order = 0;
    }
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

/// Speed factor (`Ship::crew_speed`) of computer skirmish ships: retail Carrack and Barque ran in at exactly 1.267 x their
/// Emergency Speed. A retail save of such a Carrack holds engineer and navigator factor 1.425, our uncrewed Cutter 1.125, and
/// 1.425 / 1.125 = 1.267 (docs/re/crew.md).
pub const COMPUTER_CREW: f32 = 1.267;
/// Rudder-torque factor (`Ship::crew_turn`) of computer skirmish ships: the square of their navigator factor. Measured on the
/// saved Carrack: it circled at 8.23°/s at 6.47 units/s, our turn model with torque x 1.267² predicts 8.25°/s (x 1.267: 7.33).
/// PROVISIONAL: why the computer's rudder takes the factor squared (one ship with its own save; two earlier tracks agree within 4 %).
pub const COMPUTER_TURN: f32 = COMPUTER_CREW * COMPUTER_CREW;
/// Computer captains keep this far outside a black hole's radius (retail: a Carrack rode 157-165 units from a 145-radius hole).
pub const HOLE_MARGIN: f32 = 15.0;
/// Map exploration of skirmish captains with no target (read from the original's code): each ship sails for the centre of a
/// quadrant (a quarter of the map's extent out on each axis) and, within half its sight range of it, picks a quadrant at random
/// (possibly the same one). PROVISIONAL: the map's border margin is ignored; the random draw is a fixed hash, not the original's.
#[derive(Default)]
pub struct Explore {
    map: Option<Option<String>>,
    half: f32,
    quadrant: std::collections::HashMap<Entity, usize>,
}
impl Explore {
    /// Centre (bevy coordinates) of quadrant `q` of a map `half` units across from the centre: bit 0 = +x, bit 1 = +y (game).
    pub fn centre(q: usize, half: f32) -> Vec3 {
        let (x, y) = (if q & 1 == 1 { half / 2.0 } else { -half / 2.0 }, if q & 2 == 2 { half / 2.0 } else { -half / 2.0 });
        Vec3::new(x, 0.0, -y)
    }
    fn goal(&mut self, cfg: &Config, e: Entity, here: Vec3, sight: f32) -> Vec3 {
        if self.map.as_ref() != Some(&cfg.map) {
            let map = cfg.map.as_ref().and_then(|id| crate::data::map_json(id)).unwrap_or_default();
            self.half = map["world_half"].as_f64().unwrap_or(1000.0) as f32;
            self.map = Some(cfg.map.clone());
            self.quadrant.clear();
        }
        let half = self.half;
        let q = self.quadrant.entry(e).or_insert(e.index_u32() as usize % 4);
        if Vec2::new(here.x, here.z).distance(Explore::centre(*q, half).xz()) < 0.5 * sight {
            let draw = (e.to_bits() ^ (here.x.to_bits() as u64) << 7 ^ here.z.to_bits() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            *q = (draw >> 62) as usize;
        }
        Explore::centre(*q, half)
    }
}

/// Near a black hole or damaging nebula (`holes`: centre, radius) a captain turns straight away from it: the heading to take.
pub fn flee(holes: &[(Vec3, f32)], pos: Vec3) -> Option<f32> {
    holes.iter().find(|(c, r)| Vec2::new(pos.x - c.x, pos.z - c.z).length() < r + HOLE_MARGIN).map(|(c, _)| {
        let d = pos - *c;
        (-d.x).atan2(-d.z)
    })
}

/// Whether `pos` is inside a keep-out area of `hole_list` (where `flee` turns away).
pub fn in_hole(holes: &[(Vec3, f32)], pos: Vec3) -> bool {
    holes.iter().any(|(c, r)| Vec2::new(pos.x - c.x, pos.z - c.z).length() < r + HOLE_MARGIN)
}

/// Areas captains keep out of, as (centre, radius) for `flee`: black holes and damaging nebulae.
pub fn hole_list(
    holes: &Query<(&Transform, &crate::blackhole::Pull), Without<Ship>>,
    storms: Option<&crate::nebula::Storms>,
    shift: Option<&crate::nebula::StormShift>,
) -> Vec<(Vec3, f32)> {
    let mut out: Vec<(Vec3, f32)> = holes.iter().map(|(t, p)| (t.translation, p.radius)).collect();
    if let (Some(storms), Some(shift)) = (storms, shift) {
        out.extend(crate::nebula::storm_circles(storms, shift));
    }
    out
}

/// Seconds a computer captain keeps its target before choosing again (the original's target-selection timer).
pub const RESELECT_SECS: f32 = 6.0;
/// Each skirmish captain's current target and how long it has held it: the original's target selection takes the nearest hostile
/// ship in sight, again every `RESELECT_SECS` or as soon as the old one is gone or out of sight (docs/re/movement.md). Steering,
/// fire (`combat::gunnery`) and boarding (`boarding::ai_boarding`) all follow it.
#[derive(Resource, Default)]
pub struct CaptainTargets(pub std::collections::HashMap<Entity, (Entity, f32)>);
impl CaptainTargets {
    pub fn of(&self, e: Entity) -> Option<Entity> {
        self.0.get(&e).map(|t| t.0)
    }
}

/// One target-selection step: keep the `held` target (with its age) while it is younger than `RESELECT_SECS` and still `valid`
/// (alive and hostile: the original follows a held target anywhere), else take the nearest candidate (entity, distance). Returns
/// the target and whether it was kept.
fn choose_target(held: Option<(Entity, f32)>, candidates: &[(Entity, f32)], valid: impl Fn(Entity) -> bool) -> Option<(Entity, bool)> {
    held.filter(|h| h.1 < RESELECT_SECS && valid(h.0)).map(|h| (h.0, true)).or_else(|| candidates.iter().min_by(|a, b| a.1.total_cmp(&b.1)).map(|c| (c.0, false)))
}

/// Skirmish captains, as read from the original's code (docs/re/movement.md "Computer captains"): every think the target is the nearer
/// of the nearest hostile ship in sight and the nearest hostile base at any range, kept for `RESELECT_SECS`; failing both, a fleet
/// mate's or an ally's target. The attack itself is `attack`. With no target a computer ship explores (`Explore`). The player ship
/// only uses this in demo (--shot) mode as an autopilot.
pub fn ai(
    cfg: Res<Config>,
    mission: Option<Res<campaign::Mission>>,
    mut ships: Query<(Entity, &Transform, &mut Ship, &Health, Has<PlayerShip>, Option<&FleetPilot>, Option<&FleetMember>, Option<&combat::GunStates>)>,
    busy: Query<(), CaptainBusy>,
    sensors: Res<crate::environment::Sensors>,
    held: Query<(), With<HoldPosition>>,
    holes: Query<(&Transform, &crate::blackhole::Pull), Without<Ship>>,
    storms: (Option<Res<crate::nebula::Storms>>, Option<Res<crate::nebula::StormShift>>),
    (mut explore, mut breakoffs): (Local<Explore>, Local<std::collections::HashMap<Entity, attack::Breakoff>>),
    (time, mut targets): (Res<Time>, ResMut<CaptainTargets>),
    (lib, fleets): (Res<combat::Library>, Res<Fleets>),
    bases: Query<(Entity, &Transform, &combat::BaseSide, &Health, &combat::ArmedBase)>,
    scripted: Query<&campaign::MissionShip>,
) {
    if mission.is_some() {
        return;
    }
    // A skirmish map's script steers its own ships (Convoy Raid's convoys keep to their routes, a stolen convoy idles); hidden ones
    // wait for the script to reveal them. PROVISIONAL: the stance is all the captain looks at.
    let left_to_script = |e: Entity| scripted.get(e).is_ok_and(|m| !m.visible || matches!(m.stance, crate::rules::Stance::Persistent | crate::rules::Stance::Dummy));
    let dt = time.delta_secs();
    let holes = hole_list(&holes, storms.0.as_deref(), storms.1.as_deref());
    let now = time.elapsed_secs();
    // (entity, position, side, afloat, radius, fleet, the aim point seen from a position)
    let snap: Vec<(Entity, Vec3, Side, bool, f32, Option<String>, (Transform, usize, Vec<f32>))> = ships
        .iter()
        .map(|(e, t, s, h, _, _, m, _)| (e, t.translation, s.side, !h.destroyed, lib.classes[s.class].radius, m.map(|m| m.0.clone()), (*t, s.class, h.sections.clone())))
        .collect();
    breakoffs.retain(|e, _| snap.iter().any(|f| f.0 == *e));
    for (e, tf, mut s, h, me, pilot, member, guns) in &mut ships {
        // The human's own fleet waits for orders; allied computer fleets share the human's side but hunt like any computer.
        let human = s.side == Side::Player && member.is_none_or(|m| m.0 == fleets.local);
        if busy.contains(e) || left_to_script(e) {
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
        let candidates: Vec<(Entity, f32)> = snap
            .iter()
            .filter(|f| f.2 != s.side && f.3 && (explicit == Some(f.0) || sensors.in_sight(e, f.0, None)) && explicit.is_none_or(|e| e == f.0))
            .map(|f| (f.0, f.1.distance(tf.translation)))
            .collect();
        // Aggressive computer captains also weigh the nearest hostile base, at any range in skirmish (the nearer of the two wins).
        // Armed volcano islands fire on everyone but are no base to go for (retail Diablo Strait: the computer came for our ships).
        let island = |b: &combat::ArmedBase| lib.classes[b.class].name.starts_with("Island_");
        let hostile_base = |b: &(Entity, &Transform, &combat::BaseSide, &Health, &combat::ArmedBase)| b.2.0 != s.side && !b.3.destroyed && !island(b.4);
        let mut candidates = candidates;
        if !human
            && explicit.is_none()
            && let Some(b) = crate::nearest(bases.iter().filter(hostile_base), tf.translation, |b| b.1.translation)
        {
            candidates.push((b.0, b.1.translation.distance(tf.translation)));
        }
        let valid = |t: Entity| snap.iter().any(|f| f.0 == t && f.2 != s.side && f.3) || bases.get(t).is_ok_and(|b| hostile_base(&b));
        let mut chosen = choose_target(targets.0.get(&e).copied(), &candidates, valid);
        // Nothing found: a fleet mate's target, else an allied ship's (same side, another fleet).
        if chosen.is_none() && !human && explicit.is_none() {
            let fleet = member.map(|m| m.0.as_str());
            let adopt =
                |same_fleet: bool| snap.iter().filter(|f| f.0 != e && f.2 == s.side && (f.5.as_deref() == fleet) == same_fleet).find_map(|f| targets.of(f.0).filter(|t| valid(*t)));
            chosen = adopt(true).or_else(|| adopt(false)).map(|t| (t, false));
        }
        match chosen {
            Some((t, true)) => targets.0.entry(e).or_insert((t, 0.0)).1 += dt,
            Some((t, false)) => _ = targets.0.insert(e, (t, 0.0)),
            None => _ = targets.0.remove(&e),
        }
        // (position, radius, is a base, aim point)
        let foe = chosen.and_then(|(t, _)| {
            snap.iter().find(|f| f.0 == t).map(|f| (f.1, f.4, false, attack::aim(&lib.classes[f.6.1].sections, &f.6.2, &f.6.0, tf.translation, false))).or_else(|| {
                bases
                    .get(t)
                    .ok()
                    .map(|b| (b.1.translation, lib.classes[b.4.class].radius, true, attack::aim(&lib.classes[b.4.class].sections, &b.3.sections, b.1, tf.translation, true)))
            })
        });
        let class = &lib.classes[s.class];
        let explore_now = |s: &mut Ship, explore: &mut Explore| {
            (s.waypoints, s.goal, s.order) = (vec![explore.goal(&cfg, e, tf.translation, class.sight)], None, 3);
        };
        let Some((foe_pos, foe_radius, foe_base, foe_aim)) = foe else {
            s.goal = None;
            if human || explicit.is_some() {
                s.order = 0;
                continue;
            }
            explore_now(&mut s, &mut explore);
            continue;
        };
        s.waypoints.clear(); // contact: drop any exploring course (the quadrant is kept for later)
        if let Some(away) = flee(&holes, tf.translation) {
            (s.goal, s.order) = (Some(away), 3);
            continue;
        }
        let Some((routine, range)) = attack::plan(&lib, class, h, guns) else {
            (s.goal, s.order) = (None, 0);
            continue;
        };
        if routine == attack::Routine::Ram && foe_base {
            explore_now(&mut s, &mut explore); // the original: no gun left and a base to attack, so explore instead
            continue;
        }
        let r = if foe_base { range[1] } else { range[0] };
        let gap = (foe_pos - tf.translation).with_y(0.0).length() - class.radius - foe_radius;
        let at = if routine == attack::Routine::Ram { foe_pos } else { foe_aim };
        let course = attack::course(routine, tf.translation, s.heading, at, gap, r, now, breakoffs.entry(e).or_default());
        s.throttle = attack::throttle(e, tf.translation, s.heading, course[0]);
        (s.waypoints, s.goal, s.order) = (course, None, 3);
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
        let target = fraction * max_speed(ship, class, health) * ship.avoid;
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
            steer = (angdiff(w, ship.heading) / HELM_FULL).clamp(-1.0, 1.0);
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
        // Roll about the bow axis toward the rudder, rate-limited; none at Emergency Speed (measured, docs/re/movement.md).
        // PROVISIONAL: target proportional to rudder; retail also weaves slightly while steering for a point (not modelled).
        let lean = if ship.order == 3 { 0.0 } else { steer * MAX_ROLL };
        let roll = tf.rotation.to_euler(EulerRot::YXZ).2;
        let roll = roll + (lean - roll).clamp(-ROLL_RATE * dt, ROLL_RATE * dt);
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
    /// Seconds to the next re-plan; runs whether or not the ship holds formation, so an order waits for the next one.
    pub replan_in: f32,
    pub plan: Option<SlotPlan>,
}

/// The path a held follower is on: its slot as it was at the last re-plan, carried along at the leader's velocity then.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SlotPlan {
    pub point: Vec3,
    pub velocity: Vec3,
    pub age: f32,
}

impl SlotPlan {
    pub fn goal(&self) -> Vec3 {
        self.point + self.velocity * self.age
    }
}

/// Seconds between a follower's re-plans. Retail (30 Hz logs of a Cutter in a Diamond slot): its movement path is reissued every 4.2 s
/// (the speed drops to 0 and ramps up again at each), a formation order is only picked up at the next one (first move 0-4.2 s later),
/// and a leader on a straight course is tracked exactly while one in a turn is not (the carried-along point is wrong between re-plans).
const REPLAN: f32 = 4.2;

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
/// R is the largest member's bounding-sphere radius: the retail Diamond slot sat R + 30 = 46.7 from a Man-O-War leader, whose WOT sphere
/// radius is 17 (docs/re/fleet-controls.md). PROVISIONAL: stable roster assignment replaces native size/cost assignment.
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
    let radii: Vec<_> = fleet.members.iter().map(|e| world.get::<Ship>(*e).map_or(0.0, |s| world.resource::<Library>().classes[s.class].sphere.1)).collect();
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
    let radii: Vec<_> = f.members.iter().map(|e| world.get::<Ship>(*e).map_or(0.0, |s| world.resource::<Library>().classes[s.class].sphere.1)).collect();
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

/// Distance to the slot inside which a follower stops closing (retail Cutter: full speed to ~4 units out, then stops).
const SLOT_ARRIVE: f32 = 4.5;

/// Heading error (radians) under which a follower runs at full speed, and the throttle it keeps while turned further off its slot. Retail
/// Cutter assembling into a Diamond slot: ~1.7 units/s (0.19 of its ~9) while the error was above about 4 degrees, 6-9 once it fell below.
const ALIGNED: f32 = 0.09;
const TURNING_THROTTLE: f32 = 0.2;

/// A follower's wanted speed. Measured on retail (docs/re/fleet-controls.md, Diamond slot behind a stationary Man-O-War): full speed until
/// the last few units, then it stops dead; in the slot it matches the leader's speed. No proportional slow-down on the way in. A ship
/// sent to a destination (not holding formation) stops within 5 units of it.
fn follow_speed(hold: bool, distance: f32, leader_speed: f32, max: f32, heading_error: f32) -> f32 {
    if hold {
        if distance > SLOT_ARRIVE { if heading_error.abs() > ALIGNED { TURNING_THROTTLE * max } else { max } } else { leader_speed.clamp(0.0, max) }
    } else if distance < 5.0 {
        0.0
    } else {
        max
    }
}

/// Where a follower aims on its way to `goal`: straight, unless that line passes within `clearance` of the leader (a slot behind the
/// leader is reached from beside it at spawn), then a point off the leader's flank on the side the line already passes. PROVISIONAL: the
/// original's follower pathing is unknown; this only keeps followers from driving through the ship they follow and shoving it.
fn steer_round(from: Vec3, goal: Vec3, leader: Vec3, clearance: f32) -> Vec3 {
    let (d, rel) = ((goal - from).with_y(0.0), (leader - from).with_y(0.0));
    let t = if d.length_squared() > 0.0 { (rel.dot(d) / d.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    let off = (from + d * t - leader).with_y(0.0);
    if off.length() >= clearance || (goal - leader).with_y(0.0).length() < clearance {
        return goal;
    }
    let side = off.try_normalize().unwrap_or_else(|| Vec3::new(-d.z, 0.0, d.x).normalize_or_zero());
    (leader + side * clearance * 1.2).with_y(goal.y)
}

pub fn fleet_follow(world: &mut World) {
    let fleets = world.resource::<Fleets>().groups.clone();
    let busy: std::collections::HashSet<_> = world.query_filtered::<Entity, CaptainBusy>().iter(world).collect();
    let held: std::collections::HashSet<_> = world.query_filtered::<Entity, With<HoldPosition>>().iter(world).collect();
    for e in &held {
        if world.get::<PlayerShip>(*e).is_none()
            && let Some(mut s) = world.get_mut::<Ship>(*e)
        {
            s.halt();
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
            let (class_index, heading) = (ship.class, ship.heading);
            let leader_velocity = world.get::<Ship>(leader).map_or(Vec3::ZERO, |l| Vec3::new(-l.heading.sin(), 0.0, -l.heading.cos()) * l.speed);
            let dt = world.get_resource::<Time>().map_or(0.0, |t| t.delta_secs());
            let pilot = {
                let mut p = pilot;
                p.replan_in -= dt;
                if !p.hold {
                    p.plan = None;
                } else if let Some(plan) = p.plan.as_mut() {
                    plan.age += dt;
                }
                if p.replan_in <= 0.0 {
                    p.replan_in = REPLAN;
                    if p.hold {
                        p.plan = Some(SlotPlan { point: tf.translation + tf.rotation * p.slot, velocity: leader_velocity, age: 0.0 });
                    }
                }
                if let Some(mut live) = world.get_mut::<FleetPilot>(e) {
                    live.replan_in = p.replan_in;
                    live.plan = p.plan;
                }
                p
            };
            let goal = if pilot.hold { pilot.plan.map(|p| p.goal()) } else { pilot.destination };
            if let Some(goal) = goal {
                // PROVISIONAL: captain movement keeps altitude; the shared integrator has no vertical thrust.
                let goal = goal.with_y(here.y);
                let distance = here.distance(goal);
                let lib = world.resource::<Library>();
                let clearance = lib.classes[class_index].radius + world.get::<Ship>(leader).map_or(0.0, |l| lib.classes[l.class].radius);
                let aim = steer_round(here, goal, tf.translation, clearance);
                let to_goal = goal - here;
                let heading_error = if to_goal.length_squared() > 1.0 { (-to_goal.x).atan2(-to_goal.z) - heading } else { 0.0 };
                let heading_error = (heading_error + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                let wanted = follow_speed(pilot.hold, distance, speed, max, heading_error);
                if let Some(mut s) = world.get_mut::<Ship>(e) {
                    s.waypoints = if distance > 3.0 { vec![aim] } else { vec![] };
                    s.goal = if pilot.hold { Some((-(tf.rotation * Vec3::NEG_Z).x).atan2(-(tf.rotation * Vec3::NEG_Z).z)) } else { None };
                    s.order = if wanted < 0.01 { 0 } else { 2 };
                    s.throttle = Some(if max > 0.001 { (wanted / max).clamp(0.0, 1.0) } else { 0.0 });
                }
            }
        }
    }
}

pub mod attack;
#[cfg(test)]
pub mod movement_tests;

#[cfg(test)]
mod steer_round_tests {
    use super::steer_round;
    use bevy::prelude::*;
    #[test]
    fn a_follower_goes_round_the_leader_it_would_cross() {
        let (from, goal, leader) = (Vec3::new(13.0, 0.0, -25.0), Vec3::new(41.0, 0.0, 30.0), Vec3::ZERO);
        let aim = steer_round(from, goal, leader, 40.0);
        assert!(aim.length() > 40.0 && aim != goal, "{aim:?}");
        assert_eq!(steer_round(Vec3::new(200.0, 0.0, 0.0), Vec3::new(300.0, 0.0, 0.0), leader, 60.0), Vec3::new(300.0, 0.0, 0.0));
    }
}

#[cfg(test)]
mod follow_speed_tests {
    use super::follow_speed;
    #[test]
    fn followers_run_full_speed_until_the_slot_then_match_the_leader() {
        assert_eq!(follow_speed(true, 30.0, 0.0, 9.0, 0.0), 9.0);
        assert_eq!(follow_speed(true, 5.0, 0.0, 9.0, 0.0), 9.0);
        assert_eq!(follow_speed(true, 2.0, 0.0, 9.0, 0.0), 0.0);
        assert_eq!(follow_speed(true, 2.0, 2.3, 9.0, 0.0), 2.3);
        assert_eq!(follow_speed(true, 2.0, 20.0, 9.0, 0.0), 9.0);
    }
    #[test]
    fn a_follower_crawls_while_turned_away_from_its_slot() {
        assert!((follow_speed(true, 30.0, 0.0, 9.0, 0.5) - 1.8).abs() < 1e-4);
        assert_eq!(follow_speed(true, 30.0, 0.0, 9.0, -0.05), 9.0);
    }
}

#[cfg(test)]
mod slot_plan_tests {
    use super::SlotPlan;
    use bevy::prelude::*;
    #[test]
    fn a_plan_follows_a_straight_leader_exactly_and_misses_a_turning_one() {
        let plan = SlotPlan { point: Vec3::new(0.0, 0.0, 10.0), velocity: Vec3::new(0.0, 0.0, -2.0), age: 3.0 };
        assert_eq!(plan.goal(), Vec3::new(0.0, 0.0, 4.0));
        // a leader that turned away since the plan: the carried-along point is off the real slot
        let real = Vec3::new(1.0, 0.0, 4.5);
        assert!(plan.goal().distance(real) > 1.0);
    }
}
