//! Shared ship state, movement integration and skirmish AI.
use crate::commands::{Commander, PlayerId};
use crate::*;

/// A ship's engine order: All Stop, Ahead Slow, Ahead Full, Emergency Speed. The discriminants are the retail dial and sound
/// indices (0..=3), and the order of the variants is the order of the speeds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum EngineOrder {
    #[default]
    Stop = 0,
    Slow = 1,
    Full = 2,
    Emergency = 3,
}

impl EngineOrder {
    pub const ALL: [EngineOrder; 4] = [EngineOrder::Stop, EngineOrder::Slow, EngineOrder::Full, EngineOrder::Emergency];

    /// The order at dial/sound index `i` (0 All Stop .. 3 Emergency); `None` past the last.
    pub fn from_index(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }

    /// The dial/sound index, 0..=3.
    pub fn index(self) -> usize {
        self as usize
    }

    /// The HUD name of the order.
    pub fn label(self) -> &'static str {
        ["All Stop", "Ahead Slow", "Ahead Full", "Emergency Speed"][self.index()]
    }

    /// The fraction of Ahead Full speed the order commands (measured on the retail cutter, docs/re/movement.md).
    pub fn fraction(self) -> f32 {
        [0.0, 0.5, 1.0, 2.0][self.index()]
    }

    /// One step up the dial, stopping at Emergency.
    pub fn faster(self) -> Self {
        Self::from_index(self.index() + 1).unwrap_or(self)
    }

    /// One step down the dial, stopping at All Stop.
    pub fn slower(self) -> Self {
        Self::from_index(self.index().saturating_sub(1)).unwrap_or(self)
    }
}

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

/// The player a ship fights for: its index in the battle's alliance table (`alliance::Alliances`), 0 the human's. Whether two
/// sides fight is the table's answer alone; `Side::Enemy` is the first computer player.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Side(pub u8);

#[allow(non_upper_case_globals)]
impl Side {
    pub const Player: Side = Side(0);
    pub const Enemy: Side = Side(1);
}

/// The player's ship when nothing else is chosen (`--ship`, campaign start, synthetic allies): the first retail skirmish ship.
pub const DEFAULT_SHIP: &str = "Ship_Navy_Cutter";

#[derive(Component, Clone, PartialEq)]
#[require(TargetSlot)]
pub struct Ship {
    pub class: usize,
    pub side: Side,
    pub heading: f32, // radians about +Y; forward is -Z at heading 0 (the GLB export maps game +y bow to -Z)
    pub speed: f32,
    pub ang_vel: f32,
    pub order: EngineOrder,
    pub waypoints: Vec<Vec3>,
    pub goal: Option<f32>,     // AI: heading to hold when there are no waypoints
    pub cap: f32,              // script 'Max Throttle Percent': fraction of the ordered speed the ship may use
    pub throttle: Option<f32>, // continuous command/captain speed fraction, independent of script cap
    pub sail_energy: f32,      // nebula drain, separate from permanent sail damage
    pub crew_speed: f32,       // engineers' factor on top speed (crew_arms::skill_factor)
    pub crew_turn: f32,        // helmsman's factor on rudder torque
    pub net: f32,              // energy nets on the hull: product of (1 - dissipated) over the active nets; 1.0 = free
    pub avoid: f32,            // speed fraction with land close ahead (collision::contact_slow, AI ships only); 1.0 = clear
    pub helm: Helm,            // rudder and torque boost as last set by the steer-to-point routine, held between its runs
    pub towed: bool,           // on the end of a tow cable (rules `Tow Ship`): its engines are idle (exhaust)
    pub tow_mass: f32,         // mass of the ship this one tows (0 = none): thrust works on both hulls
}

/// The original runs its steer-to-point law once per think (`THINK_SECS`) and keeps the rudder between runs; the physics still
/// steps every tick. This hold is what keeps a ship that steers for a point hunting a few degrees instead of settling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Helm {
    pub steer: f32,
    pub boost: f32,
    pub age: f32, // seconds since the law last ran
}
impl Default for Helm {
    fn default() -> Self {
        Helm { steer: 0.0, boost: 1.0, age: THINK_SECS } // due at the first step
    }
}

impl Ship {
    /// A ship at rest: no orders or course, every crew, net and nebula multiplier neutral (1.0). Callers set what differs.
    pub fn new(class: usize, side: Side) -> Self {
        Ship {
            class,
            side,
            heading: 0.0,
            speed: 0.0,
            ang_vel: 0.0,
            order: EngineOrder::Stop,
            waypoints: vec![],
            goal: None,
            cap: 1.0,
            throttle: None,
            sail_energy: 1.0,
            crew_speed: 1.0,
            crew_turn: 1.0,
            net: 1.0,
            avoid: 1.0,
            helm: Helm::default(),
            towed: false,
            tow_mass: 0.0,
        }
    }

    /// Drops the course and heading goal and orders a stop (the speed bleeds off on its own).
    pub fn halt(&mut self) {
        self.waypoints.clear();
        self.goal = None;
        self.order = EngineOrder::Stop;
    }

    /// The Hold and Stop orders' effect on the controls: rudder and throttle to zero, the current order and course end. A held ship
    /// does not steer or follow a path (`HoldPosition` is the marker); neither order touches the formation hold.
    pub fn stand_still(&mut self) {
        self.halt();
    }

    /// The commanded share of Ahead Full: 0 at All Stop (or a throttle of 0), otherwise the throttle or the order's fraction.
    pub fn ordered(&self) -> f32 {
        if self.order == EngineOrder::Stop { 0.0 } else { self.throttle.unwrap_or(self.order.fraction()) }
    }
}

/// The local player's ship, for the view (camera, HUD, crew lines). The simulation's mark of a human's command ship is
/// `commands::Commander`, and this marker follows the local player's (its hooks, `commands.rs`). A ship marked with this alone
/// (a world built by hand: tests, smokes) gets the local player's `Commander`, and loses it with this marker.
#[derive(Component)]
#[component(on_add = command_ship_added, on_remove = command_ship_removed)]
pub struct PlayerShip;

fn command_ship_added(mut world: bevy::ecs::world::DeferredWorld, ctx: bevy::ecs::lifecycle::HookContext) {
    if world.get::<Commander>(ctx.entity).is_none() {
        let local = crate::commands::local_player(&world);
        world.commands().entity(ctx.entity).try_insert(Commander(local));
    }
}
fn command_ship_removed(mut world: bevy::ecs::world::DeferredWorld, ctx: bevy::ecs::lifecycle::HookContext) {
    let local = crate::commands::local_player(&world);
    if world.get::<Commander>(ctx.entity).is_some_and(|c| c.0 == local) {
        world.commands().entity(ctx.entity).try_remove::<Commander>();
    }
}

/// 'AI_LOCK_POSITION' (right-click menu Hold): the ship's captain keeps it where it is; it still fires.
#[derive(Component)]
pub struct HoldPosition;

/// A move, dock, tow or board order ends a hold (any other order, an attack among them, leaves it).
pub fn release_hold(commands: &mut Commands, e: Entity) {
    commands.entity(e).remove::<HoldPosition>();
}

/// The name on a ship's banner (a mission ship's comes from `MissionShip`; otherwise the class name is shown).
#[derive(Component)]
pub struct ShipName(pub String);

pub use crate::geom::angdiff;

pub type CaptainBusy = Or<(With<docking::Docker>, With<docking::Tower>, With<boarding::GrappleOrder>, With<boarding::Grapple>, With<boarding::Boarding>)>;

/// Speed factor (`Ship::crew_speed`) of computer skirmish ships: retail Carrack and Barque ran in at exactly 1.267 x their
/// Emergency Speed. A retail save of such a Carrack holds engineer and navigator factor 1.425, our uncrewed Cutter 1.125, and
/// 1.425 / 1.125 = 1.267 (docs/re/crew.md).
pub const COMPUTER_CREW: f32 = 1.267;
/// The original's computer helm: while the course point is at least this far off the bow, the ship's rudder
/// torque rises linearly from x1 to x4 at 180°. Human-captained ships never get it.
const AI_BOOST_FROM: f32 = 11.25 * PI / 180.0;
/// Computer captains keep this far outside a black hole's radius (retail: a Carrack rode 157-165 units from a 145-radius hole).
pub const HOLE_MARGIN: f32 = 15.0;
/// Map exploration of an Aggressive computer ship with no target: it sails straight (the collision avoider keeps it clear) for one of
/// four fixed points, the centre of a quarter of the map (a quarter of the map's extent out on each axis), chosen at random, and
/// within half its scan range of it picks again (possibly the same one); known enemies do not draw it. The draws come from the
/// battle's seeded generator (`SimRng::explore`), so every machine of a lockstep battle explores alike. PROVISIONAL: the map's border
/// margin is ignored.
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
    fn goal(&mut self, cfg: &Config, e: Entity, here: Vec3, scan: f32, rng: &mut combat::Rng) -> Vec3 {
        if self.map.as_ref() != Some(&cfg.map) {
            let map = cfg.map.as_ref().and_then(|id| crate::data::map_json(id)).unwrap_or_default();
            self.half = map["world_half"].as_f64().unwrap_or(1000.0) as f32;
            self.map = Some(cfg.map.clone());
            self.quadrant.clear();
        }
        let half = self.half;
        let mut draw = || ((rng.f() * 4.0) as usize).min(3);
        let q = *self.quadrant.entry(e).or_insert_with(&mut draw);
        let q = if Vec2::new(here.x, here.z).distance(Explore::centre(q, half).xz()) < 0.5 * scan { draw() } else { q };
        self.quadrant.insert(e, q);
        Explore::centre(q, half)
    }
}

/// Near a black hole or damaging nebula (`holes`: centre, radius) a captain turns straight away from it: the heading to take.
pub fn flee(holes: &[(Vec3, f32)], pos: Vec3) -> Option<f32> {
    holes.iter().find(|(c, r)| Vec2::new(pos.x - c.x, pos.z - c.z).length() < r + HOLE_MARGIN).map(|(c, _)| {
        let d = pos - *c;
        crate::geom::heading_of(d)
    })
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

/// Seconds that must pass between two thinks of a captain. With the 0.033 s step that is every 16th step (0.528 s): 15 steps are
/// 0.495 s, not enough (measured on the original: 0.528 s between thinks, every ship, every time).
pub const THINK_SECS: f32 = 0.5;
/// A captain's think clock, as in the original: while the start delay runs the clock is held at now, so the first think comes one
/// full period after the delay; after each think the clock restarts from now (the remainder is dropped, not carried over).
/// The start delay is 0.05 s x (the ship's spawn number mod 10), one count for all sides.
#[derive(Clone, Copy)]
pub struct ThinkClock {
    born: f32,
    delay: f32,
    last: f32,
}
impl ThinkClock {
    pub fn new(now: f32, spawn_number: usize) -> Self {
        ThinkClock { born: now, delay: 0.05 * (spawn_number % 10) as f32, last: now }
    }
    /// Seconds since the last think when the captain thinks at `now`, else None.
    pub fn due(&mut self, now: f32) -> Option<f32> {
        if now - self.born < self.delay {
            self.last = now;
        }
        let since = now - self.last;
        (since >= THINK_SECS).then(|| {
            self.last = now;
            since
        })
    }
}

/// Seconds a computer captain keeps its target before choosing again (the original's target-selection timer).
pub const RESELECT_SECS: f32 = 6.0;
/// A ship's one target slot (#369 phase 2 step C; the original keeps one per ship: procyon-re `evidence/ai-captain/`, stage 2
/// Read). The captain picks the nearest hostile ship in sight, again every `RESELECT_SECS` or as soon as the old one is gone or
/// out of sight (docs/re/movement.md). Steering, fire (`combat::gunnery`) and boarding (`boarding::ai_boarding`) all follow it.
/// Every ship has one (`Ship` requires it); empty while it holds no target.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct TargetSlot(pub Option<Held>);
/// What a target slot holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Held {
    pub target: Entity,
    /// Seconds the target has been held.
    pub age: f32,
    /// Where the guns aim, in the target's frame: its section nearest to the captain when the target was chosen
    /// (`attack::aim_local`), kept until the next choice.
    pub aim: Vec3,
    /// The original's "AI can override" flag: set when the captain picked the target itself, cleared when an order named it (see
    /// `choose_target`).
    pub can_override: bool,
}
impl TargetSlot {
    pub fn target(&self) -> Option<Entity> {
        self.0.map(|h| h.target)
    }
    /// The target and the guns' aim point in its frame.
    pub fn aim(&self) -> Option<(Entity, Vec3)> {
        self.0.map(|h| (h.target, h.aim))
    }
}
/// The captains' working copy of every ship's slot during one think pass (`ai`): a captain adopting a fleet mate's target sees
/// the targets chosen before it in the same pass.
#[derive(Default)]
struct CaptainTargets(std::collections::HashMap<Entity, (Entity, f32, Vec3, bool)>);
impl CaptainTargets {
    fn of(&self, e: Entity) -> Option<Entity> {
        self.0.get(&e).map(|t| t.0)
    }
}

/// One target-selection step. "Can override" belongs to the target (PROVISIONAL): an `explicit` target (a player's Attack, fleet
/// help-attack, Board, Grapple, Dock or Tow order, the script's Attack Group, Ram, Dock, Tow, Board or Grapple actions, a chosen
/// repair dock) is taken at once and cannot be overridden; the captain's own pick can. A `held` target (entity, age, can) that is
/// still `valid` (alive and hostile: the original follows a held target anywhere) is kept until lost, and one that can be overridden
/// only until it is `RESELECT_SECS` old; then the captain scans again (`pick`, `stance_pick`), and when that finds nothing it keeps the
/// old target, scanning again after another `RESELECT_SECS` (PROVISIONAL). Returns the target, whether it was kept (age running on;
/// otherwise its age starts again), and its flag.
fn choose_target(held: Option<(Entity, f32, bool)>, explicit: Option<Entity>, pick: Option<Entity>, valid: impl Fn(Entity) -> bool) -> Option<(Entity, bool, bool)> {
    if let Some(x) = explicit.filter(|x| valid(*x)) {
        return Some((x, held.is_some_and(|h| h.0 == x), false));
    }
    let held = held.filter(|h| valid(h.0));
    held.filter(|h| !h.2 || h.1 < RESELECT_SECS).map(|h| (h.0, true, h.2)).or_else(|| pick.map(|c| (c, false, true))).or_else(|| held.map(|h| (h.0, false, h.2)))
}

/// A hostile a captain could pick: a ship, a dragon or a base, its distance, whether it is within the captain's scan range, and
/// whether it attacks the captain's ship: it is visible and holds that ship as its current target (whether it has fired is not
/// asked, so a Defensive ship may answer before any shot; volcanoes, lobs at a ground point and mines never count). PROVISIONAL.
#[derive(Clone, Copy)]
pub(crate) struct Seen {
    pub e: Entity,
    pub dist: f32,
    pub base: bool,
    pub in_scan: bool,
    pub targets_me: bool,
}

/// A captain's own pick by its stance (PROVISIONAL, read, not observed): Dummy and Unknown none. Persistent, Defensive and Neutral retaliate only:
/// the nearest hostile within scan range whose own target is this ship. Aggressive takes the nearest hostile ship or dragon in scan
/// range, else the nearest hostile base in reach (`in_reach`).
pub(crate) fn stance_pick(stance: crate::rules::Stance, campaign: bool, seen: &[Seen]) -> Option<Entity> {
    let nearest = |f: &dyn Fn(&Seen) -> bool| seen.iter().filter(|s| f(s)).min_by(|a, b| a.dist.total_cmp(&b.dist)).map(|s| s.e);
    match stance {
        crate::rules::Stance::Dummy | crate::rules::Stance::Unknown => None,
        crate::rules::Stance::Aggressive => nearest(&|s| !s.base && s.in_scan).or_else(|| nearest(&|s| s.base && in_reach(campaign, || s.in_scan))),
        _ => nearest(&|s| s.targets_me && s.in_scan),
    }
}

/// Whether a target or dock is in reach: in a campaign (a mission is loaded) only within the ship's scan range (`in_scan`), in a
/// skirmish at any range.
pub fn in_reach(campaign: bool, in_scan: impl FnOnce() -> bool) -> bool {
    !campaign || in_scan()
}

/// The computer captain, one for skirmish and missions (docs/re/movement.md "Computer captains"): every think (16 ticks,
/// docs/re/tick.md) the target is the nearer of the nearest hostile ship in sight and the nearest hostile base (at any range in a
/// skirmish, within scan range in a campaign), re-chosen after `RESELECT_SECS` only while the ship's "AI can override" flag is set;
/// failing both, a fleet mate's or an ally's target. An explicit order names a target that cannot be overridden (`choose_target`;
/// nearly all ships in the retail saved games have the flag set: their captains chose their targets). The attack itself is `attack`. With no target a skirmish computer ship explores (`Explore`); a mission
/// ship keeps its scripted course (PROVISIONAL: today's behaviour). The rules' scripted layer (`rules::mission_ai`: tows, border
/// leash, follow orders, path ends) runs after this and wins. The player ship only uses this in demo (--shot) mode as an autopilot.
pub fn ai(
    cfg: Res<Config>,
    mission: Option<Res<campaign::Mission>>,
    mut ships: Query<(Entity, &Transform, &mut Ship, &Health, Has<crate::commands::Commander>, Option<&FleetPilot>, Option<&FleetMember>, Option<&combat::GunStates>)>,
    busy: Query<(), CaptainBusy>,
    sensors: Res<crate::environment::Sensors>,
    held: Query<(), With<HoldPosition>>,
    holes: Query<(&Transform, &crate::blackhole::Pull), Without<Ship>>,
    storms: (Option<Res<crate::nebula::Storms>>, Option<Res<crate::nebula::StormShift>>),
    (mut explore, mut breakoffs, mut courses): (Local<Explore>, Local<std::collections::HashMap<Entity, attack::Breakoff>>, Local<std::collections::HashMap<Entity, Vec3>>),
    (mut clocks, spawn_order, mut start): (Local<std::collections::HashMap<Entity, ThinkClock>>, Query<&crate::scenario::SpawnIndex>, crate::scenario::BattleStart),
    (time, mut slots, orders): (
        Res<Time>,
        Query<(Entity, &mut TargetSlot)>,
        Query<(Option<&docking::Docker>, Option<&docking::Tower>, Option<&boarding::GrappleOrder>, Option<&boarding::Boarding>)>,
    ),
    (lib, fleets, mut rng, scans): (Res<combat::Library>, Res<Fleets>, ResMut<combat::SimRng>, Query<&crate::environment::ScanFactor>),
    bases: Query<(Entity, &Transform, Option<&combat::BaseSide>, &Health, &combat::ArmedBase, Option<&campaign::MissionBase>)>,
    mut scripted: Query<&mut campaign::MissionShip>,
    (dying, cloaks, table, rocks): (
        Query<(), With<combat::Sinking>>,
        Query<&crate::cloak::Cloak>,
        crate::alliance::Hostility,
        Query<(&Transform, &combat::Asteroid), Without<Ship>>,
    ),
    (dragons, aim, mut latched): (
        Query<(Entity, &Transform, &crate::environment::SpaceAnimal, &crate::dragons::Dragon), Without<Ship>>,
        Option<Res<Target>>,
        Local<std::collections::HashSet<Entity>>,
    ),
) {
    if start.is_new() {
        (*explore, *breakoffs, *clocks, *courses, *latched) = Default::default();
    }
    let mut targets = CaptainTargets(slots.iter().filter_map(|(e, s)| s.0.map(|h| (e, (h.target, h.age, h.aim, h.can_override)))).collect());
    let campaign = mission.is_some();
    // Scripted ships: hidden ones and dummies wait for the script; one following a group takes a target but no course from the captain.
    let left_to_script = |m: &campaign::MissionShip| !m.visible || m.stance == crate::rules::Stance::Dummy;
    let holes = hole_list(&holes, storms.0.as_deref(), storms.1.as_deref());
    let now = time.elapsed_secs();
    // (entity, position, side, afloat, radius, fleet, the aim point seen from a position, its ordered or aimed target, visible)
    type Snap = (Entity, Vec3, Side, bool, f32, Option<crate::names::Owner>, (Transform, usize, Vec<f32>), Option<Entity>, bool);
    // In spawn order: a ship adopts the first fleet mate's or ally's target in this list, which must not follow query order (#369 section 3).
    let snap: Vec<Snap> = crate::scenario::in_spawn_order(&spawn_order, ships.iter(), |x| x.0)
        .into_iter()
        .map(|(e, t, s, h, me, p, m, _)| {
            let aimed = p.and_then(|p| p.target).or(aim.as_ref().filter(|_| me).and_then(|a| a.0));
            let visible = !scripted.get(e).is_ok_and(|m| !m.visible) && !cloaks.get(e).is_ok_and(|c| c.state == crate::cloak::State::Full);
            (e, t.translation, s.side, !h.destroyed, lib.classes[s.class].radius, m.map(|m| m.0.clone()), (*t, s.class, h.sections.clone()), aimed, visible)
        })
        .collect();
    breakoffs.retain(|e, _| snap.iter().any(|f| f.0 == *e));
    clocks.retain(|e, _| snap.iter().any(|f| f.0 == *e));
    // Captains think in creation order (docs/re/tick.md step 8): a captain adopting a mate's target sees the targets chosen before it.
    for (e, tf, mut s, h, me, pilot, member, guns) in crate::scenario::in_spawn_order(&spawn_order, ships.iter_mut(), |x| x.0) {
        let key = crate::scenario::SpawnIndex::of(&spawn_order, e);
        // Between thinks the ship keeps the course, speed order and target of its last think.
        let clock = clocks.entry(e).or_insert_with(|| ThinkClock::new(now, key.0 as usize));
        let Some(dt) = clock.due(now) else { continue };
        // The ship the human steers is not the captain's, except as the demo autopilot of a skirmish screenshot run (in a mission, its
        // cutscenes included, never). Every other ship of the human's fleet (a Captain_Human ship too) is captained with its own stance
        // but never explores and never docks for repair on its own (PROVISIONAL). A ship that was just sunk has left its fleet and is
        // computer-run, whoever steered it (docs/re/effects.md "Death drive").
        let sinking = dying.contains(e);
        let demo = me && !campaign && cfg.shot.is_some() && cfg.script.as_deref() != Some("fleet");
        let steered = me && !sinking && !demo;
        let human = !sinking && !demo && (is_human_ship(s.side, member, Some(&fleets)) || scripted.get(e).is_ok_and(|m| m.human));
        let script = scripted.get(e).ok().map(|m| (m.ordered, m.ram, m.follow.is_some()));
        if busy.contains(e) || scripted.get(e).is_ok_and(left_to_script) {
            // A dock, tow, grapple or board order (the player's, the script's, or a chosen repair dock) names the ship's target.
            let order = orders.get(e).ok().and_then(|(d, t, g, b)| d.map(|d| d.dockee()).or(t.map(|t| t.towee())).or(g.map(|g| g.0)).or(b.map(|b| b.target())));
            if let Some(t) = order.filter(|t| targets.of(e) != Some(*t)) {
                targets.0.insert(e, (t, 0.0, Vec3::ZERO, false));
            }
            continue;
        }
        if pilot.is_some_and(FleetPilot::is_steering) {
            continue;
        }
        if held.contains(e) {
            s.order = EngineOrder::Stop;
            s.goal = None;
            continue;
        }
        if (h.destroyed && !sinking) || steered || matches!(cfg.script.as_deref(), Some("fxtest") | Some("damage") | Some("course") | Some("speedtest")) {
            continue;
        }
        let explicit = pilot.and_then(|p| p.target).or(script.and_then(|m| m.0));
        let steers = !script.is_some_and(|m| m.2);
        if steers {
            s.throttle = None;
        }
        // What it could pick: hostile ships and dragons, and hostile bases. Armed volcano islands fire
        // on everyone but are no base to go for (retail Diablo Strait: the computer came for our ships).
        let island = |b: &combat::ArmedBase| lib.classes[b.class].name.starts_with("Island_");
        type Base<'a> = (Entity, &'a Transform, Option<&'a combat::BaseSide>, &'a Health, &'a combat::ArmedBase, Option<&'a campaign::MissionBase>);
        let hostile_base = |b: &Base| table.base_side(b.5, b.2).is_some_and(|o| table.hostile(o, s.side)) && !b.3.destroyed && !island(b.4);
        let dragon = |t: Entity| dragons.get(t).ok().filter(|d| d.2.visible && d.3.hp > 0.0 && table.hostile(table.side_or_neutral(&d.2.owner), s.side));
        let rock = |t: Entity| rocks.get(t).ok().filter(|r| r.1.hp > 0.0); // a rock the script ordered attacked ('attack Asteroid Belt')
        let seen = |at: Vec3, t, base, targets_me| Seen { e: t, dist: at.distance(tf.translation), base, in_scan: sensors.in_sight(e, t, None), targets_me };
        let mut options: Vec<Seen> =
            snap.iter().filter(|f| f.0 != e && table.hostile(f.2, s.side) && f.3).map(|f| seen(f.1, f.0, false, f.8 && (targets.of(f.0) == Some(e) || f.7 == Some(e)))).collect();
        options.extend(dragons.iter().filter(|d| dragon(d.0).is_some()).map(|d| seen(d.1.translation, d.0, false, d.3.target() == Some(e))));
        options.extend(bases.iter().filter(|b| hostile_base(b)).map(|b| seen(b.1.translation, b.0, true, false)));
        let valid = |t: Entity| {
            snap.iter().any(|f| f.0 == t && table.hostile(f.2, s.side) && f.3) || bases.get(t).is_ok_and(|b| hostile_base(&b)) || dragon(t).is_some() || rock(t).is_some()
        };
        let stance = scripted.get(e).map_or(crate::rules::Stance::Aggressive, |m| m.stance);
        let aggressive = stance == crate::rules::Stance::Aggressive;
        let mut chosen = choose_target(targets.0.get(&e).map(|t| (t.0, t.1, t.3)), explicit, stance_pick(stance, campaign, &options), valid);
        // An order whose target is gone or not hostile is dropped: the ship then picks, adopts or explores as one without orders.
        let explicit = explicit.filter(|t| valid(*t));
        // Nothing found: an Aggressive ship takes a fleet mate's target, else an allied ship's (same side, another fleet).
        if chosen.is_none() && aggressive && explicit.is_none() {
            let fleet = member.map(|m| m.0.as_str());
            let adopt = |same_fleet: bool| {
                snap.iter().filter(|f| f.0 != e && f.3 && table.allied(f.2, s.side) && (f.5.as_deref() == fleet) == same_fleet).find_map(|f| targets.of(f.0).filter(|t| valid(*t)))
            };
            chosen = adopt(true).or_else(|| adopt(false)).map(|t| (t, false, true));
        }
        // An Aggressive ship with no working gun and nothing else to attack rams the nearest hostile ship at any range (PROVISIONAL).
        let gunless = attack::plan(&lib, &lib.classes[s.class], h, guns).is_none_or(|p| p.0 == attack::Routine::Ram);
        if chosen.is_none() && aggressive && gunless && !human {
            chosen = options.iter().filter(|o| !o.base).min_by(|a, b| a.dist.total_cmp(&b.dist)).map(|o| (o.e, false, true));
        }
        match chosen {
            Some((t, true, can)) => {
                let held = targets.0.entry(e).or_insert((t, 0.0, Vec3::ZERO, can));
                (held.1, held.3) = (held.1 + dt, can);
            }
            Some((t, false, can)) => {
                let aim = snap.iter().find(|f| f.0 == t).map(|f| attack::aim_local(&lib.classes[f.6.1].sections, &f.6.2, &f.6.0, tf.translation, false));
                let aim = aim.or_else(|| bases.get(t).ok().map(|b| attack::aim_local(&lib.classes[b.4.class].sections, &b.3.sections, b.1, tf.translation, true)));
                targets.0.insert(e, (t, 0.0, aim.unwrap_or(Vec3::ZERO), can));
            }
            None => _ = targets.0.remove(&e),
        }
        // (position, radius, is a base, aim point)
        let foe = chosen.and_then(|(t, ..)| {
            snap.iter()
                .find(|f| f.0 == t)
                .map(|f| (f.1, f.4, false, attack::aim(&lib.classes[f.6.1].sections, &f.6.2, &f.6.0, tf.translation, false)))
                .or_else(|| {
                    bases
                        .get(t)
                        .ok()
                        .map(|b| (b.1.translation, lib.classes[b.4.class].radius, true, attack::aim(&lib.classes[b.4.class].sections, &b.3.sections, b.1, tf.translation, true)))
                })
                .or_else(|| rock(t).map(|r| (r.0.translation, r.1.radius, false, r.0.translation)))
                .or_else(|| dragon(t).map(|d| (d.1.translation, d.2.radius, false, d.1.translation)))
        });
        let class = &lib.classes[s.class];
        let scan = class.sight * scans.get(e).map_or(1.0, |f| f.0);
        let explore_now = |s: &mut Ship, explore: &mut Explore, rng: &mut combat::SimRng| {
            (s.waypoints, s.goal, s.order) = (vec![explore.goal(&cfg, e, tf.translation, scan, &mut rng.explore)], None, EngineOrder::Emergency);
        };
        // A course the captain did not set itself and the ship is still under way on: a script's path or one the player drew. An
        // ended path (no waypoints left) or leftover points of a stopped ship are none.
        let path = s.order != EngineOrder::Stop && s.waypoints.last().is_some_and(|p| courses.get(&e) != Some(p));
        // A Persistent ship on a path keeps to it and fires at its target without turning to it; once it has done so with a target
        // held it keeps doing so until the target is gone, and holds its position when the path ends. Without a path it attacks
        // like the others. A formation comes first (the fleet's, above). PROVISIONAL.
        if chosen.is_none() {
            latched.remove(&e);
        } else if stance == crate::rules::Stance::Persistent && (path || latched.contains(&e)) {
            latched.insert(e);
            if !path {
                (s.goal, s.order) = (None, EngineOrder::Stop);
            }
            continue;
        }
        if !steers {
            continue;
        }
        let Some((foe_pos, foe_radius, foe_base, foe_aim)) = foe else {
            // With no target an Aggressive ship explores (a mission ship on a path keeps to it: PROVISIONAL); the others keep a path or
            // stop dead (a formation is the fleet's, above).
            if aggressive && !human && explicit.is_none() && !(campaign && path) {
                explore_now(&mut s, &mut explore, &mut rng);
                courses.insert(e, s.waypoints[0]);
            } else if !path {
                (s.goal, s.order) = (None, EngineOrder::Stop);
                s.waypoints.clear();
            }
            continue;
        };
        let kept = std::mem::take(&mut s.waypoints); // contact: drop any exploring course (the quadrant is kept for later)
        if let Some(away) = flee(&holes, tf.translation) {
            (s.goal, s.order) = (Some(away), EngineOrder::Emergency);
            continue;
        }
        // A script's ram order ('OP Ram') rams its ordered target.
        let ram = script.is_some_and(|m| m.1 && m.0.is_some());
        let Some((routine, range)) = attack::plan(&lib, class, h, guns).map(|(r, range)| (if ram { attack::Routine::Ram } else { r }, range)) else {
            (s.goal, s.order) = (None, EngineOrder::Stop);
            continue;
        };
        if routine == attack::Routine::Ram && foe_base {
            explore_now(&mut s, &mut explore, &mut rng); // the original: no gun left and a base to attack, so explore instead
            continue;
        }
        let r = if foe_base { range[1] } else { range[0] };
        let gap = attack::range_gap(routine, tf.translation, class.radius, foe_pos, foe_radius, foe_aim);
        let at = if routine == attack::Routine::Ram { foe_pos } else { foe_aim };
        let submerged = cloaks.get(e).is_ok_and(|c| c.level() == 1.0);
        let ranges = if routine == attack::Routine::Frontal { attack::frontal_ranges(r, submerged) } else { (r, r) };
        let breakoff = breakoffs.entry(e).or_default();
        let Some(course) = attack::course_to(routine, tf.translation, s.heading, at, gap, ranges, now, breakoff) else {
            s.waypoints = kept; // no new course: the one it held stands, with its speed order
            continue;
        };
        s.throttle = if routine == attack::Routine::Standoff && breakoff.is_none() {
            attack::standoff_throttle(gap, r)
        } else {
            attack::throttle(routine, key, tf.translation, s.heading, course[0])
        };
        courses.insert(e, *course.last().unwrap_or(&tf.translation));
        (s.waypoints, s.goal, s.order) = (course, None, EngineOrder::Emergency);
        // A finished chase waits for orders at its last point (rules::mission_ai's path end).
        if let Ok(mut m) = scripted.get_mut(e) {
            m.stop_at_end = true;
        }
    }
    for (e, mut slot) in &mut slots {
        let held = targets.0.get(&e).map(|t| Held { target: t.0, age: t.1, aim: t.2, can_override: t.3 });
        if slot.0 != held {
            slot.0 = held;
        }
    }
}

pub fn max_speed(ship: &Ship, class: &combat::Class, health: &Health) -> f32 {
    full_speed(class) * combat::speed_factor(class, health) * ship.cap * ship.sail_energy * ship.crew_speed * ship.net
}

/// Undamaged Ahead Full speed.
pub fn full_speed(class: &combat::Class) -> f32 {
    class.phys.max_speed * FULL_SPEED
}

/// One movement step for a ship; also used to predict the player's course.
pub fn step(ship: &mut Ship, tf: &mut Transform, class: &combat::Class, health: &Health, dt: f32) {
    helm_step(ship, tf, class, health, dt, false);
}

/// `step`, for a ship whose helm is the computer's (`ai`) or the human's.
pub fn helm_step(ship: &mut Ship, tf: &mut Transform, class: &combat::Class, health: &Health, dt: f32, ai: bool) {
    {
        let ph = &class.phys;
        // 1/s; retail settles with this rate for every order change. A towed hull keeps it: it brakes itself and the tow keeper
        // drags it along (collision.rs). Thrust divided by both masses against a drag set by the hull's own lowers a tower's
        // terminal speed to M / (M + tow mass) of its top speed.
        let drag = ph.thrust / ph.mass / ph.max_speed;
        let fraction = ship.ordered();
        let target = fraction * max_speed(ship, class, health) * ship.avoid * ph.mass / (ph.mass + ship.tow_mass);
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
                want = Some(crate::geom::heading_of(d));
            }
        } else if let Some(g) = ship.goal {
            want = Some(g);
        }
        ship.helm.age += dt;
        // PROVISIONAL: one phase for every ship (the original staggers captains by spawn number), and a ship with no course to steer
        // for keeps the law due, so a new course acts on the next step (retail's delay for a fresh order is not measured).
        if let Some(w) = want {
            if ship.helm.age >= THINK_SECS {
                let off = angdiff(w, ship.heading);
                ship.helm = Helm { steer: (off / HELM_FULL).clamp(-1.0, 1.0), boost: 1.0, age: 0.0 };
                if ai {
                    ship.helm.boost = (1.0 + 3.0 * (off.abs() - AI_BOOST_FROM) / (PI - AI_BOOST_FROM)).clamp(1.0, 4.0);
                }
            }
        } else {
            ship.helm = Helm { age: THINK_SECS, ..Helm::default() };
        }
        let (steer, boost) = (ship.helm.steer, ship.helm.boost);
        // Turning follows the original's ship physics (docs/re/movement.md). Assumed mapping (not verified):
        // rudder in [-1,1] is `steer`, and the torque scale is WOT MaxAngularAcceleration.
        let v = ship.speed.abs();
        let mut a = steer * TURN_SCALE * ship.crew_turn * boost * ph.max_ang_acc * (1.0 - 1.0 / (0.1 * v + 1.0)) * combat::turn_factor(class, health);
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
        // Roll about the bow axis toward the rudder, rate-limited; none at Emergency Speed (measured, docs/re/movement.md) and none
        // at Stop: a stopped ship cannot turn, so a rudder still over for a course it cannot reach must not keep it heeled.
        // PROVISIONAL: target proportional to rudder; retail also weaves slightly while steering for a point (not modelled); the
        // levelling at Stop is from play.
        let lean = if matches!(ship.order, EngineOrder::Emergency | EngineOrder::Stop) { 0.0 } else { steer * MAX_ROLL };
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
pub struct FleetMember(pub crate::names::Owner, pub usize);

/// Whether a ship is the human's own: on the player's side and, when fleets are tracked, in the local player's fleet (or in none).
/// Allied computer fleets are NOT human. This is the "waits for orders / human scoring" question (captain AI, fire and ram damage
/// scaling, the battle report). Not the same question as the alliance-table difficulty scaling of shells and blasts
/// (`combat::shells::side_damage_scale`), nor collision's `humans` (it also counts the `Human` marker). MAINTAINER QUESTION:
/// shells and blasts scale by alliance, fire and ramming by this ownership test; the original's rule is unconfirmed.
pub fn is_human_ship(side: Side, member: Option<&FleetMember>, fleets: Option<&Fleets>) -> bool {
    side == Side::Player && member.is_none_or(|m| fleets.is_none_or(|f| m.0 == f.local))
}

/// A fleet's formation. Named in command logs (`commands.rs`) in snake case.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
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
    pub groups: std::collections::BTreeMap<crate::names::Owner, Fleet>,
    pub local: crate::names::Owner,
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

impl FleetPilot {
    /// A fleet order is steering the ship: it holds a formation slot or sails to a destination. Computer captains leave such a ship alone.
    pub fn is_steering(&self) -> bool {
        self.hold || self.destination.is_some()
    }
    /// Hold: the captain's own destination ends with the order.
    pub fn hold_position(&mut self) {
        self.destination = None;
    }

    /// Stop: the destination and the target end; the formation hold and hold fire stay.
    pub fn cancel_actions(&mut self) {
        self.destination = None;
        self.target = None;
    }
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
        self.hold_fire = false;
    }
}

/// Native layout construction in game axes, converted to leader-local Bevy coordinates.
/// R is the largest member's bounding-sphere radius: the retail Diamond slot sat R + 30 = 46.7 from a Man-O-War leader, whose WOT sphere
/// radius is 17 (docs/re/fleet-controls.md). The swap pass over the result is `swap_slots`.
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

/// Not yet measured: after a layout the slots are given a single swap pass, so ships that are already
/// closer to another slot take that one. Not for Custom and Column. `ships` is each member's position in the leader's frame and the
/// angle between its heading and the leader's. For each slot i from 1, the member that fits its point best (lowest index on a tie,
/// the leader included) swaps its own slot with slot i when it fits that point strictly better than its own. Fit is 50 falling to 0
/// over an angle of 0..pi, plus 50 falling to 0 over a distance of 0..500 units.
pub fn swap_slots(slots: &mut [Vec3], ships: &[(Vec3, f32)]) {
    let fit = |m: usize, p: Vec3| {
        let (pos, angle) = ships[m];
        50.0 * (1.0 - (angle / std::f32::consts::PI).clamp(0.0, 1.0)) + 50.0 * (1.0 - (pos.distance(p) / 500.0).clamp(0.0, 1.0))
    };
    for i in 1..slots.len().min(ships.len()) {
        let a = slots[i];
        let mut j = 0;
        for m in 1..ships.len() {
            if fit(m, a) > fit(j, a) {
                j = m;
            }
        }
        if fit(j, a) > fit(j, slots[j]) {
            slots[i] = slots[j];
            slots[j] = a;
        }
    }
}

/// The slots of a fleet's formation: the layout, then the swap pass from where the ships are now.
fn laid_out_slots(world: &World, fleet: &Fleet) -> Vec<Vec3> {
    let radii: Vec<_> = fleet.members.iter().map(|e| world.get::<Ship>(*e).map_or(0.0, |s| world.resource::<Library>().classes[s.class].sphere.1)).collect();
    let mut slots = formation_slots(fleet.formation, &radii);
    if matches!(fleet.formation, Formation::None | Formation::Custom | Formation::Column) {
        return slots;
    }
    let Some(leader) = fleet.members.first().and_then(|e| world.get::<Transform>(*e)).copied() else { return slots };
    let ships: Vec<_> = fleet
        .members
        .iter()
        .map(|e| {
            let t = world.get::<Transform>(*e).copied().unwrap_or(leader);
            ((leader.rotation.inverse() * (t.translation - leader.translation)).with_y(0.0), (leader.rotation.inverse() * t.rotation * Vec3::NEG_Z).angle_between(Vec3::NEG_Z))
        })
        .collect();
    swap_slots(&mut slots, &ships);
    slots
}

fn captain_target_valid(world: &World, actor: Entity, target: Entity) -> bool {
    world.get_entity(target).is_ok()
        && world.get::<Health>(target).is_none_or(|h| !h.destroyed)
        && world.get::<combat::Asteroid>(target).is_none_or(|a| a.hp > 0.0)
        && world.get::<environment::SpaceAnimal>(target).is_none_or(|a| a.visible)
        && world
            .get::<Ship>(actor)
            .zip(world.get::<Ship>(target))
            .is_none_or(|(a, b)| world.get_resource::<crate::alliance::Alliances>().unwrap_or(crate::alliance::empty()).hostile(a.side, b.side))
}

/// Reconcile live ownership, retaining roster order. PROVISIONAL loss fallback: first living member;
/// first surviving member becomes command ship after death/capture; empty fleets have no command ship.
pub fn reconcile_fleets(world: &mut World) {
    let humans = crate::commands::humans(world);
    let mut live: Vec<_> = world
        .query::<(Entity, &Ship, &Health, &FleetMember, Option<&campaign::MissionShip>, Option<&crate::scenario::SpawnIndex>)>()
        .iter(world)
        .filter(|(_, _, h, _, _, _)| !h.destroyed)
        .map(|(e, _, _, f, m, i)| (e, m.map_or_else(|| f.0.clone(), |m| m.owner.clone()), f.1, i.copied()))
        .collect();
    live.sort_by_key(|s| (s.2, s.3));
    world.resource_scope(|world, mut fleets: Mut<Fleets>| {
        for (owner, fleet) in &mut fleets.groups {
            fleet.members.retain(|e| live.iter().any(|s| s.0 == *e && s.1 == *owner));
        }
        for (e, owner, _, _) in &live {
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
        // Each human's command ship heads its fleet; one lost (sunk, captured) hands over to the fleet's first ship.
        for (player, owner) in &humans {
            let command = world.query::<(Entity, &Commander)>().iter(world).find(|(_, c)| c.0 == *player).map(|(e, _)| e);
            let valid = command.filter(|e| live.iter().any(|s| s.0 == *e && s.1 == *owner));
            if let Some(e) = valid
                && let Some(f) = fleets.groups.get_mut(owner)
            {
                f.members.retain(|m| *m != e);
                f.members.insert(0, e);
            }
            if valid.is_some() {
                continue;
            }
            // The script's throttle cap on the command ship ("HUMAN CONTROLLED COMMAND SHIP") belongs to the command, not the hull: the
            // replacement takes it over, instead of keeping whatever cap a rule set on it before (a ship captured from a capped enemy
            // group, a stationary escort), which could leave the player with a ship that cannot move. PROVISIONAL: the
            // original's handoff of the cap after a capture or loss has not been observed. Only a command ship that is still the human's
            // (sunk, not captured) hands its cap on, and never a 0: a captured ship's cap is the new owner's rules', and a 0 (an NIS stop)
            // would strand the player again. Otherwise the replacement gets full throttle.
            let owner_of = |e: Entity| world.get::<campaign::MissionShip>(e).map(|m| m.owner.clone()).or_else(|| world.get::<FleetMember>(e).map(|f| f.0.clone()));
            let cap = command.filter(|e| owner_of(*e).as_deref() == Some(owner.as_str())).and_then(|e| world.get::<Ship>(e)).map(|s| s.cap).filter(|c| *c > 0.0).unwrap_or(1.0);
            if let Some(e) = command {
                world.entity_mut(e).remove::<Commander>();
            }
            if let Some(e) = fleets.groups.get(owner).and_then(|f| f.members.first()).copied() {
                let target = world.get::<FleetPilot>(e).and_then(|p| p.target);
                if let Some(mut s) = world.get_mut::<Ship>(e) {
                    s.cap = cap;
                }
                install_primary(world, e, *player, target);
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
    let slots = laid_out_slots(world, &fleet);
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
                s.order = EngineOrder::Stop;
                s.throttle = None;
            }
        }
    }
}

/// `candidate` becomes `player`'s command ship (`Commander`), at the speed it sails now. The manual target is the local player's
/// (one `Target` until #369 phase 2 PR C), so another player's hand-over leaves it alone.
fn install_primary(world: &mut World, candidate: Entity, player: PlayerId, target: Option<Entity>) {
    let max = world.get::<Ship>(candidate).zip(world.get::<Health>(candidate)).map_or(0.0, |(s, h)| max_speed(s, &world.resource::<Library>().classes[s.class], h));
    if let Some(mut s) = world.get_mut::<Ship>(candidate) {
        s.throttle = Some(speed_fraction(s.speed, max));
        s.order = EngineOrder::Full;
    }
    world.entity_mut(candidate).insert((Commander(player), FleetPilot::default()));
    if let Some(mut m) = world.get_mut::<campaign::MissionShip>(candidate) {
        m.hold_fire = false;
        m.ordered = None;
        m.attack.clear();
    }
    if player != crate::commands::local_player(world) {
        return;
    }
    world.resource_mut::<Target>().0 = target;
    world.resource_mut::<Target>().1 = None;
    world.resource_mut::<Target>().2 = Default::default();
}

/// P (Change Primary): `candidate` becomes `player`'s command ship, its old one a captain again (skirmish only, unless `scripted`: a
/// rule's hand-over, which may also move a ship of another group into the player's fleet). Only the local player's selection follows.
pub fn promote(world: &mut World, player: PlayerId, candidate: Entity, scripted: bool) -> bool {
    if !scripted && world.contains_resource::<campaign::Mission>() {
        return false;
    }
    let Some(owner) = crate::commands::fleet_of(world, player) else { return false };
    let local = player == crate::commands::local_player(world);
    if scripted && world.get::<Health>(candidate).is_some_and(|h| !h.destroyed) {
        // PROVISIONAL cross-group script handoff: transfer candidate to the local fleet, retaining other group members.
        if let Some(mut m) = world.get_mut::<campaign::MissionShip>(candidate) {
            m.owner = owner.clone();
        }
    }
    reconcile_fleets(world);
    if !world.resource::<Fleets>().groups.get(&owner).is_some_and(|f| f.members.contains(&candidate)) {
        return false;
    }
    // The manual target is the local player's (one `Target` until #369 phase 2 PR C); another player's goes with the captains'.
    let manual = |world: &World, e: Entity| if local { world.resource::<Target>().0 } else { world.get::<FleetPilot>(e).and_then(|p| p.target) };
    let old: Vec<Entity> = world.query::<(Entity, &Commander)>().iter(world).filter(|(_, c)| c.0 == player).map(|(e, _)| e).collect();
    let next_target = if old.contains(&candidate) { manual(world, candidate) } else { world.get::<FleetPilot>(candidate).and_then(|p| p.target) };
    let old_target = old.first().map_or_else(|| manual(world, candidate), |e| manual(world, *e));
    let mut retired = Vec::new();
    for e in old {
        world.entity_mut(e).remove::<Commander>();
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
    install_primary(world, candidate, player, next_target);
    let f = {
        let mut fleets = world.resource_mut::<Fleets>();
        let f = fleets.groups.get_mut(&owner).unwrap();
        f.members.retain(|e| *e != candidate && !retired.contains(e));
        f.members.insert(0, candidate);
        if local {
            fleets.selected = Some(candidate);
        }
        fleets.groups.get(&owner).unwrap().clone()
    };
    let slots = laid_out_slots(world, &f);
    for (i, e) in f.members.iter().enumerate().skip(1) {
        if f.formation != Formation::Custom
            && let Some(mut p) = world.get_mut::<FleetPilot>(*e)
        {
            p.slot = slots[i];
        }
    }
    true
}

/// A player's Attack or "fire when possible" order makes the ship Aggressive (PROVISIONAL).
pub fn aggressive(world: &mut World, e: Entity) {
    if let Some(mut m) = world.get_mut::<campaign::MissionShip>(e) {
        m.stance = crate::rules::Stance::Aggressive;
    }
}

pub fn fleet_help(world: &mut World, owner: &str, attack: bool) {
    let Some(fleet) = world.resource::<Fleets>().groups.get(owner).cloned() else { return };
    let target = if fleet.members.first().copied() == world.resource::<Fleets>().primary() {
        world.resource::<Target>().0
    } else {
        fleet.members.first().and_then(|e| world.get::<TargetSlot>(*e).and_then(TargetSlot::target))
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
        if attack || fleet.cease_fire {
            aggressive(world, e);
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
pub(crate) fn follow_speed(hold: bool, distance: f32, leader_speed: f32, max: f32, heading_error: f32) -> f32 {
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
pub(crate) fn steer_round(from: Vec3, goal: Vec3, leader: Vec3, clearance: f32) -> Vec3 {
    let (d, rel) = ((goal - from).with_y(0.0), (leader - from).with_y(0.0));
    let t = crate::geom::segment_t(rel, d);
    let off = (from + d * t - leader).with_y(0.0);
    if off.length() >= clearance || (goal - leader).with_y(0.0).length() < clearance {
        return goal;
    }
    let side = off.try_normalize().unwrap_or_else(|| Vec3::new(-d.z, 0.0, d.x).normalize_or_zero());
    (leader + side * clearance * 1.2).with_y(goal.y)
}

/// What the presentation (sail billow, flag, deck rules) reads as "how fast the ship goes": |speed| over the class's full speed,
/// 0..1. Distinct from `speed_fraction` (top speed passed in, 0 below 0.001, no floor on a zero class speed here).
pub fn speed_share(speed: f32, class: &combat::Class) -> f32 {
    (speed.abs() / full_speed(class).max(1e-3)).clamp(0.0, 1.0)
}

/// A ship's speed as a fraction of its top speed `top` (0..1; 0 when it has none): the path-end and install-primary throttle.
pub(crate) fn speed_fraction(speed: f32, top: f32) -> f32 {
    if top > 0.001 { (speed.abs() / top).clamp(0.0, 1.0) } else { 0.0 }
}

/// The bearing error a follower steers out toward `to` (its slot, relative to the ship): 0 within a unit of it.
pub(crate) fn follow_error(to: Vec3, heading: f32) -> f32 {
    if to.length_squared() > 1.0 { angdiff(crate::geom::heading_of(to), heading) } else { 0.0 }
}

/// How a follower's orders map `wanted` speed onto the helm (`follow_speed` supplies it).
#[derive(Clone, Copy)]
pub(crate) struct FollowHelm {
    /// The speed `wanted` is measured against for the throttle share (the ship's top speed).
    pub top: f32,
    /// The throttle share is clamped to 0..this.
    pub max_share: f32,
    /// A wanted speed over `top` orders Emergency (the fleet's followers); else the order is Full (mission followers).
    pub emergency_over_top: bool,
}

/// Puts a follower on its station: the course (`aim` while farther than 3 from the slot), the heading to hold, and the speed order and
/// throttle for `wanted`. Shared by the fleet's followers (`fleet_follow`) and `OP Follow Group` (rules/systems/mission_ai.rs).
pub(crate) fn follow_station(ship: &mut Ship, aim: Vec3, distance: f32, goal: Option<f32>, wanted: f32, helm: FollowHelm) {
    ship.waypoints = if distance > 3.0 { vec![aim] } else { vec![] };
    ship.goal = goal;
    ship.order = if wanted < 0.01 {
        EngineOrder::Stop
    } else if helm.emergency_over_top && wanted > helm.top {
        EngineOrder::Emergency
    } else {
        EngineOrder::Full
    };
    ship.throttle = Some(if helm.top > 0.001 { (wanted / helm.top).clamp(0.0, helm.max_share) } else { 0.0 });
}

pub fn fleet_follow(world: &mut World) {
    let fleets = world.resource::<Fleets>().groups.clone();
    let busy: std::collections::HashSet<_> = world.query_filtered::<Entity, CaptainBusy>().iter(world).collect();
    let held: std::collections::HashSet<_> = world.query_filtered::<Entity, With<HoldPosition>>().iter(world).collect();
    for e in &held {
        if let Some(mut s) = world.get_mut::<Ship>(*e) {
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
            // Retail: a follower's speeds are shares of its own top speed at full throttle, which is its Emergency speed, so it
            // closes on its slot at Emergency and keeps up with a leader at Emergency (docs/re/fleet-controls.md "Follower speed").
            let full = world.get::<Health>(e).map_or(0.0, |h| max_speed(ship, &world.resource::<Library>().classes[ship.class], h));
            let max = full * EngineOrder::Emergency.fraction();
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
                let heading_error = follow_error(to_goal, heading);
                let wanted = follow_speed(pilot.hold, distance, speed, max, heading_error);
                if let Some(mut s) = world.get_mut::<Ship>(e) {
                    let goal = if pilot.hold { Some(crate::geom::heading_of(tf.rotation * Vec3::NEG_Z)) } else { None };
                    let helm = FollowHelm { top: full, max_share: EngineOrder::Emergency.fraction(), emergency_over_top: true };
                    follow_station(&mut s, aim, distance, goal, wanted, helm);
                }
            }
        }
    }
}

pub mod attack;
#[cfg(test)]
pub mod movement_tests;
pub mod path_end;

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

#[cfg(test)]
mod think_clock_tests {
    use super::*;
    /// Step numbers (0.033 s each) on which a captain with this spawn number thinks, over the first two seconds.
    fn thinks(spawn_number: usize) -> Vec<usize> {
        let mut c = ThinkClock::new(0.0, spawn_number);
        (1..=60).filter(|k| c.due(*k as f32 * 0.033).is_some()).collect()
    }
    #[test]
    fn a_captain_thinks_every_sixteenth_step() {
        assert_eq!(thinks(0), [16, 32, 48]); // 0.528 s apart, as measured on the original; 15 steps (0.495 s) are not enough
    }
    #[test]
    fn the_start_delay_shifts_the_first_think_and_not_the_period() {
        // Spawn number 4: 0.2 s delay, held through step 6 (0.198 s), so the first think is 16 steps after that.
        assert_eq!(thinks(4), [22, 38, 54]);
        assert_eq!(thinks(14), thinks(4)); // the delay repeats every ten ships
    }
    #[test]
    fn start_delays_are_005_times_the_spawn_number_and_wrap_at_ten() {
        for n in 0..25 {
            let c = ThinkClock::new(0.0, n);
            assert!((c.delay - 0.05 * (n % 10) as f32).abs() < 1e-6, "ship {n}");
        }
        assert_eq!(ThinkClock::new(0.0, 9).delay, 0.05 * 9.0); // 0.45 s for the tenth ship, then 0 again
        assert_eq!(ThinkClock::new(0.0, 10).delay, 0.0);
    }
    #[test]
    fn the_clock_restarts_at_each_think_without_carrying_a_remainder() {
        let mut c = ThinkClock::new(0.0, 0);
        let first = (1..=40).find(|k| c.due(*k as f32 * 0.033).is_some()).unwrap();
        assert_eq!(first, 16);
        // Ticks 17..=31 are not due: the next think is 16 ticks after the last, not 0.5 s after the last's nominal time.
        assert!((17..=31).all(|k| c.due(k as f32 * 0.033).is_none()));
        assert!(c.due(32.0 * 0.033).is_some());
    }
    #[test]
    fn a_think_reports_the_time_since_the_last_one() {
        let mut c = ThinkClock::new(0.0, 0);
        let since = (1..=16).find_map(|k| c.due(k as f32 * 0.033)).unwrap();
        assert!((since - 0.528).abs() < 1e-4);
    }
}

#[cfg(test)]
mod engine_order_tests {
    use super::EngineOrder::{self, *};
    #[test]
    fn the_dial_indices_round_trip_and_stop_at_both_ends() {
        for (i, o) in [Stop, Slow, Full, Emergency].into_iter().enumerate() {
            assert_eq!(EngineOrder::from_index(i), Some(o));
            assert_eq!(o.index(), i);
        }
        assert_eq!(EngineOrder::from_index(4), None);
        assert_eq!((Emergency.faster(), Stop.slower(), Slow.faster(), Full.slower()), (Emergency, Stop, Full, Slow));
    }
    #[test]
    fn labels_and_speed_fractions_are_the_retail_ones() {
        assert_eq!(EngineOrder::ALL.map(|o| (o.label(), o.fraction())), [("All Stop", 0.0), ("Ahead Slow", 0.5), ("Ahead Full", 1.0), ("Emergency Speed", 2.0)]);
        assert!(Stop < Slow && Slow < Full && Full < Emergency);
    }
}
