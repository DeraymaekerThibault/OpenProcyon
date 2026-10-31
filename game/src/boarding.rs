//! Boarding and grappling (docs/re/boarding.md): crew strengths, the grapple link, the boarding fight and capture.
//! A ship boards a hostile ship only while grappled to it (harpoon), a base only while beside one of its boarding points and slow;
//! the fight then runs as in the original: one casualty per interval, a coin flip for who takes it, until a side's
//! strength is gone. The winner captures: the loser's owner changes (mission rules count it) and the grapple is released.
//! PROVISIONAL: crew skill (generic crew combat ability, no leader bonus, no per-man crews), the harpoon flight (no projectile; the hook
//! lands after distance / muzzle speed), the winch that pulls grappled hulls together, one grapple per ship, AI boarding of boardable
//! bases (the original has no AI path to the command), first-mate lines picked by meaning, and the boarding status bar placement.
use crate::Nearest;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde_json::Value;
use std::sync::OnceLock;

use crate::audio::Sfx;
use crate::campaign::{Mission, MissionBase, MissionShip};
use crate::collision::Drift;
use crate::combat::{Health, Library, Rng};
use crate::data::conv;
use crate::fx::FxLib;
use crate::{PlayerShip, Ship, Side, assets_dir};

/// Boarder and target must be this close (hull surface to hull surface) and this slow relative to each other.
const SHIP_RANGE: f32 = 10.0;
const SHIP_SPEED: f32 = 4.5;
/// Boarder to a base's boarding point, and the boarder's own speed.
const BASE_RANGE: f32 = 15.0;
const BASE_SPEED: f32 = 2.0;
/// Casualty interval: (9 + 1.5·r) / (2·weaker strength) seconds.
const TICK_BASE: f32 = 9.0;
const TICK_SPREAD: f32 = 1.5;
/// The harpoon's longest reach and speed (Gun_Harpoon_Light MaximumRange and MuzzleSpeed).
const HARPOON_RANGE: f32 = 100.0;
const HARPOON_SPEED: f32 = 45.0;
/// A grapple snaps when the ships' centres get this much further apart than when the hook bit.
const GRAPPLE_SLACK: f32 = 2.0;
/// While a boarding is registered on the grapple the line is reeled in at this speed, down to this centre distance.
const REEL_SPEED: f32 = 3.0;
const REEL_MIN: f32 = 12.0;
/// PROVISIONAL: how quickly the reel brings the ships' closing speed to `REEL_SPEED` (1/s); the original's rope physics is not ported.
const REEL_RATE: f32 = 2.0;

/// WOT 'Crew Skill Level'. The per-man combat ability is the generic crew's (Crew_General_*: 1, 3, 4 of 10).
/// PROVISIONAL: 'Players' (the campaign roster) uses Average, and generic crews get no leadership bonus (named crews do).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Skill {
    Green,
    Average,
    Elite,
    Players,
}
impl Skill {
    pub fn parse(s: &str) -> Self {
        match s {
            "Green" => Self::Green,
            "Elite" => Self::Elite,
            "Players" => Self::Players,
            _ => Self::Average,
        }
    }
    /// Engineer/helmsman factor (`Ship::crew_speed`, `crew_turn`) of a generic crew: retail saves show such ships with crew
    /// factors 1.15 / 1.30-1.37 / 1.40-1.47, and speed follows the saved factor / 1.125 (docs/re/crew.md); 'Players' ships take
    /// theirs from their posts instead. PROVISIONAL: the mean of the saved values (1.163 / 1.326 / 1.441).
    pub fn handling(self) -> f32 {
        match self {
            Self::Green => 1.163 / 1.125,
            Self::Average => 1.326 / 1.125,
            Self::Elite => 1.441 / 1.125,
            Self::Players => 1.0,
        }
    }
    /// Crew factor on sight range (`environment::ScanFactor`): retail saves' values for generic crews (Green 1.15, Average
    /// 1.30-1.37, Elite 1.40-1.47); 'Players' ships keep their plain sight. PROVISIONAL: the lower value of each band.
    pub fn scan(self) -> f32 {
        match self {
            Self::Green => 1.15,
            Self::Average => 1.30,
            Self::Elite => 1.40,
            Self::Players => 1.0,
        }
    }
    /// Gunnery accuracy factor relative to an Average crew (1 - 0.8 x gunnery, as for crewed ships in gunnery.rs); below 1 is better.
    /// Gunnery is the generic crewman's (`Crew_General_*`: 3 / 6 / 8 out of 10). PROVISIONAL: that island gunners use these crews.
    pub fn gunnery_vs_average(self) -> f32 {
        let gunnery = |s| match s {
            Self::Green => 0.3,
            Self::Average | Self::Players => 0.6,
            Self::Elite => 0.8,
        };
        (1.0 - 0.8 * gunnery(self)) / (1.0 - 0.8 * gunnery(Self::Average))
    }
    fn member(self) -> f32 {
        match self {
            Self::Green => 0.1,
            Self::Average | Self::Players => 0.3,
            Self::Elite => 0.4,
        }
    }
}

/// One object's boarding numbers, named as in the original: `fighters` (Fighter Combat Strength) and `crew` (Crew Combat Strength) are the
/// casualty pools; `attack` (Cached Attacking = crew/2 + fighters) is what it boards with, `defend` (Cached Defending = crew + fighters)
/// what it holds with; `full` is the Initial Defending. A base has only `defend` (its 'Combat Strength').
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Strength {
    pub fighters: i32,
    pub crew: i32,
    pub attack: i32,
    pub defend: i32,
    pub full: i32,
    base: bool,
}
impl Strength {
    /// `counts`: Gunners, Captains, First Mates, Navigators, Engineers, Riggers, Fighters, Lookouts; `member`: combat ability 0..1.
    pub fn ship(counts: [u32; 8], member: f32) -> Self {
        let sum = |n: u32| (10.0 * member * n as f32).round() as i32;
        let (fighters, crew) = (sum(counts[6]), sum(counts.iter().sum::<u32>() - counts[6]));
        Self { fighters, crew, attack: crew / 2 + fighters, defend: crew + fighters, full: crew + fighters, base: false }
    }
    pub fn base(combat: i32) -> Self {
        Self { fighters: 0, crew: 0, attack: 0, defend: combat, full: combat, base: true }
    }
    /// After a fight: casualties come out of the fighters first, then the crew; the totals are derived again.
    fn normalise(&mut self) {
        if self.base {
            return;
        }
        let loss = self.fighters + self.crew - self.defend;
        if self.fighters < loss {
            self.crew -= loss - self.fighters;
            self.fighters = 0;
        } else {
            self.fighters -= loss;
        }
        self.attack = self.crew / 2 + self.fighters;
        self.defend = self.crew + self.fighters;
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Outcome {
    Won,
    Lost,
    Draw,
}
/// One casualty: the boarder's attack, then its defend, each lose one; or the target's defend, then its attack.
/// Returns whether the fight is over (the casualty's side is out).
pub fn casualty(boarder: &mut Strength, target: &mut Strength, boarder_hit: bool) -> bool {
    if boarder_hit {
        if boarder.attack > 0 {
            boarder.attack -= 1;
            boarder.defend -= (boarder.defend > 0) as i32;
        }
        boarder.attack < 1
    } else {
        if target.defend > 0 {
            target.defend -= 1;
            target.attack -= (target.attack > 0) as i32;
        }
        target.defend < 1
    }
}
/// Result of a finished fight: the boarder's attack against the target's defend, read before the totals are normalised.
pub fn settle(boarder: &mut Strength, target: &mut Strength) -> Outcome {
    let (a, d) = (boarder.attack, target.defend);
    boarder.normalise();
    target.normalise();
    match d.cmp(&a) {
        std::cmp::Ordering::Less => Outcome::Won,
        std::cmp::Ordering::Greater => Outcome::Lost,
        std::cmp::Ordering::Equal => Outcome::Draw,
    }
}
/// Seconds between casualties, fixed when the fight starts (`r` uniform in 0..1).
pub fn interval(boarder: &Strength, target: &Strength, r: f32) -> f32 {
    let weaker = boarder.attack.min(target.defend);
    if weaker <= 0 { 0.0 } else { (TICK_BASE + TICK_SPREAD * r) / (2.0 * weaker as f32) }
}

static DATA: OnceLock<Value> = OnceLock::new();
fn data() -> &'static Value {
    DATA.get_or_init(|| crate::data::try_json(&assets_dir().join("data/boarding.json")).unwrap_or(Value::Null))
}
pub fn counts(entity: &str) -> [u32; 8] {
    let mut out = [0; 8];
    for (o, v) in out.iter_mut().zip(data()[entity]["crew"].as_array().into_iter().flatten()) {
        *o = v.as_u64().unwrap_or(0) as u32;
    }
    out
}
/// The object's BoardingEffectPoint nodes in its Bevy frame.
fn points(entity: &str) -> Vec<Vec3> {
    let f = |v: &Value, i: usize| v[i].as_f64().unwrap_or(0.0) as f32;
    data()[entity]["points"].as_array().into_iter().flatten().map(|p| conv(f(p, 0), f(p, 1), f(p, 2))).collect()
}

/// A ship or base that can be boarded. `on` is the target's Boardable flag (WOT 'Boardable' / 'Set Group/Unit Boardable').
#[derive(Component, Clone, Debug)]
pub struct Boardable {
    pub on: bool,
    pub strength: Strength,
    pub skill: Skill,
    counts: [u32; 8],
    points: Vec<Vec3>,
}
impl Boardable {
    pub fn ship(entity: &str, skill: Skill, on: bool) -> Self {
        let counts = counts(entity);
        Self { on, strength: Strength::ship(counts, skill.member()), skill, counts, points: points(entity) }
    }
    fn replace_strength(&mut self, mut strength: Strength) {
        let losses = (self.strength.full - self.strength.defend).max(0);
        strength.defend = (strength.defend - losses).max(0);
        strength.normalise();
        self.strength = strength;
    }

    pub fn reset_crew(&mut self) {
        self.replace_strength(Strength::ship(self.counts, self.skill.member()));
    }

    /// Named crew replace generic hands; `bonus` (`crew_arms::leadership_bonus`) lifts every slot but the captain's and first mate's.
    pub fn apply_posts(&mut self, posts: &crate::crew_arms::CrewPosts, bonus: f32) {
        let mut totals = [0.0; 8];
        for (role, total) in totals.iter_mut().enumerate() {
            let b = if matches!(role, 1 | 2) { 0.0 } else { bonus };
            let members = posts.combat(role, self.skill.member());
            let hands = self.counts[role].saturating_sub(members.len() as u32) as f32;
            *total = members.iter().map(|v| (v + b).min(1.0)).sum::<f32>() + hands * (self.skill.member() + b).min(1.0);
        }
        let fighters = (totals[6] * 10.0).round() as i32;
        let crew = (totals.iter().enumerate().filter(|(i, _)| *i != 6).map(|(_, v)| v).sum::<f32>() * 10.0).round() as i32;
        self.replace_strength(Strength { fighters, crew, attack: crew / 2 + fighters, defend: crew + fighters, full: crew + fighters, base: false });
    }

    pub fn base(entity: &str, combat: i32, on: bool) -> Self {
        Self { on, strength: Strength::base(combat), skill: Skill::Average, counts: [0; 8], points: points(entity) }
    }
}

/// The boarding fight this ship (the boarder) is in.
#[derive(Component)]
pub struct Boarding {
    target: Entity,
    interval: f32,
    since: f32,
    /// False while the order waits for the grapple to bring the hulls into range; once in range, leaving it ends the fight.
    live: bool,
    fx: Vec<Entity>,
}
impl Boarding {
    pub fn target(&self) -> Entity {
        self.target
    }
    /// The hulls are in range and the crews are fighting.
    pub fn live(&self) -> bool {
        self.live
    }
}

/// What a boarding status bar shows: the friendly side's share of the two strengths still standing (the boarder's attack, the
/// target's defend).
pub fn friendly_share(boarder: &Strength, target: &Strength, target_friendly: bool) -> f32 {
    let (a, d) = (boarder.attack.max(0) as f32, target.defend.max(0) as f32);
    let (mine, theirs) = if target_friendly { (d, a) } else { (a, d) };
    if mine + theirs > 0.0 { mine / (mine + theirs) } else { 0.5 }
}
/// A harpoon on its way (`flight` > 0) or the grapple it became: this ship holds `target`.
#[derive(Component)]
pub struct Grapple {
    target: Entity,
    flight: f32,
    /// Centre distance when the hook bit (0 until then).
    span: f32,
    /// Per end (grappler, target): the hull axis the other ship lay along when the hook bit, pointing toward it.
    sides: [Vec3; 2],
}
impl Grapple {
    fn new(target: Entity, flight: f32) -> Self {
        Grapple { target, flight, span: 0.0, sides: [Vec3::ZERO; 2] }
    }
}

/// The hull axis (local bow-stern or beam, signed) closest to the direction toward `other`: "grappled on the front" or "on the side".
fn side_of(tf: &Transform, other: Vec3) -> Vec3 {
    let d = tf.rotation.inverse() * (other - tf.translation);
    if d.z.abs() >= d.x.abs() { Vec3::Z * d.z.signum() } else { Vec3::X * d.x.signum() }
}
/// The Grapple order: steer for `target` until it is inside harpoon range, then fire.
#[derive(Component)]
pub struct GrappleOrder(Entity);

#[derive(Message)]
pub enum BoardMsg {
    /// `boarder: None` is the player's ship.
    Board {
        boarder: Option<Entity>,
        target: Entity,
    },
    Unboard {
        boarder: Option<Entity>,
    },
    Grapple {
        grappler: Option<Entity>,
        target: Entity,
    },
    Ungrapple {
        grappler: Option<Entity>,
    },
    /// 'Set Group/Unit Boardable'.
    Boardable(Entity, bool),
}
/// A capture, for the mission rules' capture counts (owner names; skirmish sides are "Player" / "Enemy").
#[derive(Message)]
pub struct Captured {
    pub by: String,
    pub victim: String,
    pub unit: Entity,
}
#[derive(Message)]
struct Settled {
    boarder: Entity,
    target: Entity,
    outcome: Outcome,
}

/// What the player's ship can do to each ship right now, for the options menu (refreshed every frame).
#[derive(Resource, Default)]
pub struct BoardStatus {
    can_grapple: bool,
    /// Per ship: (the player is boarding it, the player may board it, the player holds a grapple on it).
    targets: std::collections::HashMap<Entity, (bool, bool, bool)>,
}
impl BoardStatus {
    /// The Board slot: (shows stop, enabled). Stop is always enabled.
    pub fn board(&self, target: Entity) -> (bool, bool) {
        self.targets.get(&target).map_or((false, false), |s| if s.0 { (true, true) } else { (false, s.1) })
    }
    /// The Grapple slot: (shows stop, enabled).
    pub fn grapple(&self, target: Entity) -> (bool, bool) {
        self.targets.get(&target).map_or((false, false), |s| if s.2 { (true, true) } else { (false, self.can_grapple) })
    }
}
fn status(boarders: Boarders, mut st: ResMut<BoardStatus>, ships: Query<Entity, With<Ship>>) {
    st.targets.clear();
    let Some(me) = boarders.me() else { return };
    st.can_grapple = boarders.can_grapple(me);
    for e in ships.iter().filter(|e| *e != me) {
        st.targets.insert(e, (boarders.boarding(me, e), boarders.can_board(me, e), boarders.grappling(me, e)));
    }
}

pub fn plugin(app: &mut App) {
    app.add_message::<BoardMsg>()
        .add_message::<Captured>()
        .add_message::<Settled>()
        .init_resource::<BoardStatus>()
        .init_resource::<Ropes>()
        .add_systems(
            FixedUpdate,
            (handle_messages, set_flags, grapple_orders, winch, ai_boarding, fights, settled)
                .chain()
                .in_set(crate::BattleSet::Orders)
                .run_if(in_state(crate::menu::Screen::Battle))
                .run_if(crate::menu::overlay_closed),
        )
        .add_systems(
            Update,
            (dev_board, cancel_grapple_order, status, ropes).chain().before(crate::input).run_if(in_state(crate::menu::Screen::Battle)).run_if(crate::menu::overlay_closed),
        );
}

/// The lines between ships, for views to draw (the original's Rope effect): world end points and how much longer than the gap the
/// line is (its slack).
#[derive(Resource, Default)]
pub struct Ropes(pub Vec<(Vec3, Vec3, f32)>);

/// Bitten grapples run from the grappler's harpoon mount to the target's nearest boarding point (the hull centre without one), slack
/// by how much the centres closed since the hook bit; tows run taut from the tower's stern tow point to the towee's bow.
/// PROVISIONAL: the original's attachment points on the target are not decoded.
fn ropes(
    mut out: ResMut<Ropes>,
    lib: Res<Library>,
    mission: Option<Res<Mission>>,
    hooks: Query<(Entity, &Grapple)>,
    ships: Query<(&Ship, &Transform, &Health, Option<&crate::combat::GunStates>, Option<&Boardable>)>,
) {
    out.0.clear();
    for (e, g) in &hooks {
        let (Ok((ship, tf, health, guns, _)), Ok((_, ttf, _, _, tb))) = (ships.get(e), ships.get(g.target)) else { continue };
        if g.flight > 0.0 {
            continue;
        }
        let from = harpoons(&lib, ship, health, guns).next().map_or(tf.translation, |(m, _)| tf.transform_point(m.pos));
        let to = tb.into_iter().flat_map(|b| &b.points).map(|p| ttf.transform_point(*p)).nearest_to(from, |a| *a).unwrap_or(ttf.translation);
        out.0.push((from, to, (g.span - tf.translation.distance(ttf.translation)).max(0.0)));
    }
    for &(tower, towee) in mission.as_deref().map_or(&[][..], |m| m.script.tows()) {
        if let (Ok((a, ta, ..)), Ok((b, tb, ..))) = (ships.get(tower), ships.get(towee)) {
            out.0.push((ta.transform_point(lib.classes[a.class].tow_points[0]), tb.transform_point(lib.classes[b.class].tow_points[1]), 0.0));
        }
    }
}

type Body<'a> = (&'a Transform, Option<&'a Ship>, Option<&'a Drift>);
fn velocity(b: &Body) -> Vec3 {
    let (tf, ship, drift) = *b;
    ship.map_or(Vec3::ZERO, |s| tf.rotation * Vec3::NEG_Z * s.speed) + drift.map_or(Vec3::ZERO, |d| d.0)
}
fn radius(lib: &Library, ship: Option<&Ship>) -> f32 {
    ship.map_or(0.0, |s| lib.classes[s.class].radius)
}
/// Ship targets need the hulls' surfaces close and nearly co-moving; a base needs a boarding point near the boarder's
/// hull and the boarder nearly stopped.
fn in_range(lib: &Library, boarder: &Body, target: &Body, target_points: &[Vec3]) -> bool {
    let (rb, vb) = (radius(lib, boarder.1), velocity(boarder));
    if target.1.is_some() {
        let surface = boarder.0.translation.distance(target.0.translation) - rb - radius(lib, target.1);
        surface < SHIP_RANGE && (vb - velocity(target)).length() < SHIP_SPEED
    } else {
        target_points.iter().any(|p| boarder.0.translation.distance(target.0.transform_point(*p)) - rb < BASE_RANGE) && vb.length() < BASE_SPEED
    }
}
/// The ship's live harpoon mounts (a gun whose section still stands) with the gun fitted there.
fn harpoons<'a>(lib: &'a Library, ship: &Ship, health: &'a Health, guns: Option<&'a crate::combat::GunStates>) -> impl Iterator<Item = (&'a crate::data::Mount, &'a str)> {
    lib.classes[ship.class]
        .mounts
        .iter()
        .enumerate()
        .map(move |(i, m)| (m, guns.map_or(m.gun.as_str(), |g| g.gun(i))))
        .filter(|(m, gun)| gun.contains("Harpoon") && m.parent.is_none_or(|p| health.sections[p] > 0.0))
}
fn has_harpoon(lib: &Library, ship: &Ship, health: &Health, guns: Option<&crate::combat::GunStates>) -> bool {
    harpoons(lib, ship, health, guns).next().is_some()
}
/// A live harpoon mount whose horizontal arc covers `at` (world point) within the gun's LongRange from the ship's centre.
/// PROVISIONAL range rule, chosen to match the retail runs (docs/re/boarding.md §5).
fn harpoon_bears(lib: &Library, ship: &Ship, health: &Health, guns: Option<&crate::combat::GunStates>, tf: &Transform, at: Vec3) -> bool {
    harpoons(lib, ship, health, guns).any(|(m, gun)| {
        let local = (tf.rotation * m.rot).inverse() * (at - tf.transform_point(m.pos));
        (-local.x).atan2(-local.z).abs() <= m.arc / 2.0 && tf.translation.distance(at) <= lib.guns[gun].range[1]
    })
}

/// Whether two objects are hostile: by the mission's alliance table (owners), else by skirmish side.
fn hostile_pair(mission: Option<&Mission>, a: (Option<&str>, Option<Side>), b: (Option<&str>, Option<Side>)) -> bool {
    if let (Some(m), Some(x), Some(y)) = (mission, a.0, b.0) {
        return m.script.hostile(x, y);
    }
    matches!((a.1, b.1), (Some(x), Some(y)) if x != y)
}
fn owner_of<'a>(ms: Option<&'a MissionShip>, mb: Option<&'a MissionBase>) -> Option<&'a str> {
    ms.map(|m| m.owner.as_str()).or_else(|| mb.map(|m| m.owner.as_str()))
}

/// What the menu and the AI need to know about boarding; the original's boarding tests in one place.
#[derive(SystemParam)]
pub struct Boarders<'w, 's> {
    bodies: Query<'w, 's, (Body<'static>, &'static Boardable)>,
    owners: Query<'w, 's, (Option<&'static MissionShip>, Option<&'static MissionBase>)>,
    base_sides: Query<'w, 's, &'static crate::combat::BaseSide>,
    fighters: Query<'w, 's, (Entity, &'static Boarding)>,
    grapples: Query<'w, 's, (Entity, &'static Grapple)>,
    ships: Query<'w, 's, (&'static Ship, &'static Health, Option<&'static crate::combat::GunStates>)>,
    player: Query<'w, 's, Entity, With<PlayerShip>>,
    lib: Res<'w, Library>,
    mission: Option<Res<'w, Mission>>,
}
impl Boarders<'_, '_> {
    pub fn hostile(&self, a: Entity, b: Entity) -> bool {
        let who = |e: Entity| {
            (self.owners.get(e).ok().and_then(|(ms, mb)| owner_of(ms, mb)), self.ships.get(e).ok().map(|s| s.0.side).or_else(|| self.base_sides.get(e).ok().map(|b| b.0)))
        };
        hostile_pair(self.mission.as_deref(), who(a), who(b))
    }
    pub fn grappled(&self, a: Entity, b: Entity) -> bool {
        self.grapples.iter().any(|(e, g)| g.flight <= 0.0 && ((e == a && g.target == b) || (e == b && g.target == a)))
    }
    pub fn boarding(&self, boarder: Entity, target: Entity) -> bool {
        self.fighters.iter().any(|(e, f)| e == boarder && f.target == target)
    }
    /// Hostile, not already fighting, the target's Boardable flag on, and a grapple (ship) / in range (base).
    pub fn can_board(&self, boarder: Entity, target: Entity) -> bool {
        let (Ok((bb, _)), Ok((tb, t))) = (self.bodies.get(boarder), self.bodies.get(target)) else { return false };
        let alive = |e: Entity| self.ships.get(e).is_ok_and(|s| !s.1.destroyed);
        t.on && boarder != target
            && alive(boarder)
            && (tb.1.is_none() || alive(target))
            && !self.boarding(boarder, target)
            && self.hostile(boarder, target)
            && if tb.1.is_some() { self.grappled(boarder, target) } else { in_range(&self.lib, &bb, &tb, &t.points) }
    }
    /// The ship has a harpoon (a live gun section) and no grapple out.
    pub fn can_grapple(&self, e: Entity) -> bool {
        self.ships.get(e).is_ok_and(|(s, h, guns)| !h.destroyed && has_harpoon(&self.lib, s, h, guns)) && !self.grapples.contains(e)
    }
    pub fn grappling(&self, grappler: Entity, target: Entity) -> bool {
        self.grapples.iter().any(|(e, g)| e == grappler && g.target == target)
    }
    pub fn me(&self) -> Option<Entity> {
        self.player.single().ok()
    }
}

const CREW_LINES: [(&str, &[&str]); 11] = [
    ("boarding_party", &["hmfirstmate02", "hmfirstmate02b"]),
    ("landing_party", &["hmfirstmate16", "hmfirstmate16b"]),
    ("repell_boarders", &["hmfirstmate01", "hmfirstmate01b"]),
    ("own_ship_boarded", &["hmfirstmate23"]),
    ("enemy_ship_conquered", &["hmfirstmate19"]),
    ("enemy_base_conquered", &["hmfirstmate18"]),
    ("own_ship_conquered", &["hmfirstmate22"]),
    ("allied_base_conquered", &["hmfirstmate21"]),
    ("boarding_repelled", &["hmfirstmate27", "hmfirstmate27b"]),
    ("retreat", &["hmfirstmate20", "hmfirstmate20b"]),
    ("grappled", &["hmfirstmate24"]),
];
/// PROVISIONAL: the Navy human-male first mate speaks (as the helmsman does), the event picked by meaning rather than the original's id;
/// `r` (0..1) picks between a line's variants.
fn say(commands: &mut Commands, assets: Option<&AssetServer>, event: &str, r: f32) {
    let (Some(assets), Some((_, files))) = (assets, CREW_LINES.iter().find(|l| l.0 == event)) else { return };
    commands.spawn((
        AudioPlayer::new(assets.load(format!("voice/{}.ogg", files[(r * files.len() as f32) as usize % files.len()]))),
        PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(crate::menu::volume(crate::menu::Setting::Dialogue))),
    ));
}

fn end_fight(commands: &mut Commands, e: Entity, fx: &[Entity]) {
    for f in fx {
        if let Ok(mut c) = commands.get_entity(*f) {
            c.despawn();
        }
    }
    commands.entity(e).remove::<Boarding>();
}

fn handle_messages(mut msgs: MessageReader<BoardMsg>, mut commands: Commands, assets: Option<Res<AssetServer>>, mut rng: ResMut<Rng>, boarders: Boarders) {
    let me = boarders.me();
    for m in msgs.read() {
        match *m {
            BoardMsg::Board { boarder, target } => {
                let Some(b) = boarder.or(me) else { continue };
                if !boarders.can_board(b, target) {
                    continue;
                }
                let (Ok((_, bt)), Ok((_, tt))) = (boarders.bodies.get(b), boarders.bodies.get(target)) else { continue };
                let tick = interval(&bt.strength, &tt.strength, rng.f());
                info!("boarding: {b} boards {target} (attack {} against defend {}, a casualty every {tick:.2}s)", bt.strength.attack, tt.strength.defend);
                commands.entity(b).insert(Boarding { target, interval: tick, since: 0.0, live: false, fx: vec![] });
            }
            BoardMsg::Unboard { boarder } => {
                let Some(b) = boarder.or(me) else { continue };
                if let Some((_, f)) = boarders.fighters.iter().find(|(e, _)| *e == b) {
                    end_fight(&mut commands, b, &f.fx);
                    if Some(b) == me {
                        say(&mut commands, assets.as_deref(), "retreat", rng.f());
                    }
                }
            }
            BoardMsg::Grapple { grappler, target } => {
                let Some(g) = grappler.or(me) else { continue };
                if g != target && boarders.can_grapple(g) && boarders.hostile(g, target) && boarders.ships.contains(target) {
                    commands.entity(g).insert(GrappleOrder(target));
                }
            }
            BoardMsg::Ungrapple { grappler } => {
                let Some(g) = grappler.or(me) else { continue };
                commands.entity(g).remove::<(GrappleOrder, Grapple)>();
                // A fight needs the grapple: it ends with it.
                for (e, f) in boarders.fighters.iter().filter(|(e, f)| (*e == g || f.target == g) && boarders.bodies.get(f.target).is_ok_and(|b| b.0.1.is_some())) {
                    end_fight(&mut commands, e, &f.fx);
                }
            }
            BoardMsg::Boardable(..) => {}
        }
    }
}

fn set_flags(mut msgs: MessageReader<BoardMsg>, mut flags: Query<&mut Boardable>) {
    for m in msgs.read() {
        if let BoardMsg::Boardable(e, on) = *m
            && let Ok(mut b) = flags.get_mut(e)
        {
            b.on = on;
        }
    }
}

fn cancel_grapple_order(keys: Res<ButtonInput<KeyCode>>, mut commands: Commands, player: Query<Entity, With<PlayerShip>>) {
    if keys.just_pressed(KeyCode::Backspace) {
        for e in &player {
            commands.entity(e).remove::<GrappleOrder>();
        }
    }
}

/// The Grapple order: close to harpoon range, stop, fire; the hook bites after its flight.
fn grapple_orders(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<Library>,
    assets: Option<Res<AssetServer>>,
    mut rng: ResMut<Rng>,
    mut sfx: MessageWriter<Sfx>,
    mut orders: Query<(Entity, &GrappleOrder, &mut Ship, &Transform, &Health)>,
    mut hooks: Query<(Entity, &mut Grapple, &Transform)>,
    others: Query<(&Transform, &Ship, &Health), Without<GrappleOrder>>,
    player: Query<Entity, With<PlayerShip>>,
) {
    for (e, order, mut ship, tf, health) in &mut orders {
        let Ok((ttf, tship, thealth)) = others.get(order.0) else {
            commands.entity(e).remove::<GrappleOrder>();
            continue;
        };
        if health.destroyed || thealth.destroyed {
            commands.entity(e).remove::<GrappleOrder>();
            continue;
        }
        let surface = tf.translation.distance(ttf.translation) - lib.classes[ship.class].radius - lib.classes[tship.class].radius;
        if surface <= HARPOON_RANGE {
            ship.halt();
            commands.entity(e).remove::<GrappleOrder>().insert(Grapple::new(order.0, (surface / HARPOON_SPEED).max(0.1)));
            sfx.write(Sfx { name: "Weapon_GrapHarpoon_Fire".into(), at: Some(tf.translation), vol: 0.7, max: 600.0 });
        } else {
            ship.waypoints = vec![ttf.translation];
            ship.order = 2;
        }
    }
    let dt = time.delta_secs();
    for (e, mut g, tf) in &mut hooks {
        if g.flight <= 0.0 {
            continue;
        }
        g.flight -= dt;
        if g.flight <= 0.0 {
            sfx.write(Sfx { name: "Weapon_GrapHarpoon_Impact_Wood".into(), at: Some(tf.translation), vol: 1.0, max: 600.0 });
            if Some(g.target) == player.single().ok() {
                say(&mut commands, assets.as_deref(), "grappled", rng.f());
            }
            info!("grapple: {e} holds {}", g.target);
        }
    }
}

/// A bitten grapple does not tow: the ships sail freely and the line snaps when their centres get `GRAPPLE_SLACK` further apart than
/// when the hook bit. Only while a boarding is registered on the pair is the line reeled in (`REEL_SPEED`, down to `REEL_MIN`), so a
/// ship that pulls away hard enough still breaks free. A grapple on a destroyed or vanished ship is let go.
/// It also snaps when either ship's partner crosses to the other side of the hull it was grappled on.
/// PROVISIONAL: the original also lets go when the ships become allied.
fn winch(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<Library>,
    mut sfx: MessageWriter<Sfx>,
    mut hooks: Query<(Entity, &mut Grapple)>,
    fighters: Query<(Entity, &Boarding)>,
    mut ships: Query<(&mut Ship, &Transform, &mut Drift, &Health)>,
) {
    let dt = time.delta_secs();
    for (e, mut g) in &mut hooks {
        if g.flight > 0.0 {
            continue;
        }
        let Ok([(mut a, atf, mut da, ha), (mut b, btf, mut db, hb)]) = ships.get_many_mut([e, g.target]) else {
            commands.entity(e).remove::<Grapple>();
            continue;
        };
        if ha.destroyed || hb.destroyed {
            commands.entity(e).remove::<Grapple>();
            continue;
        }
        let axis = (btf.translation - atf.translation).with_y(0.0);
        let distance = axis.length();
        if g.span <= 0.0 {
            g.span = distance;
            g.sides = [side_of(atf, btf.translation), side_of(btf, atf.translation)];
        }
        // PROVISIONAL: the axis is the nearer of bow-stern and beam; the original picks it from the hull's proportions.
        let crossed = |tf: &Transform, other: Vec3, side: Vec3| (tf.rotation * side).dot(other - tf.translation) <= 0.0;
        if distance > g.span + GRAPPLE_SLACK || crossed(atf, btf.translation, g.sides[0]) || crossed(btf, atf.translation, g.sides[1]) {
            info!("grapple: {e} loses {} (centres {distance:.1} apart, bit at {:.1})", g.target, g.span);
            sfx.write(Sfx { name: "rope_break_placeholder".into(), at: Some(atf.translation.lerp(btf.translation, 0.5)), vol: 1.0, max: 600.0 });
            commands.entity(e).remove::<Grapple>();
            continue;
        }
        let boarding = fighters.iter().any(|(f, fight)| (f == e && fight.target == g.target) || (f == g.target && fight.target == e));
        if !boarding || distance <= REEL_MIN {
            continue;
        }
        // Bring the closing speed along the line up to the reel's, mass-weighted (momentum kept).
        let (ca, cb) = (&lib.classes[a.class], &lib.classes[b.class]);
        let n = axis / distance.max(1e-3);
        let (va, vb) = (atf.rotation * Vec3::NEG_Z * a.speed + da.0, btf.rotation * Vec3::NEG_Z * b.speed + db.0);
        let opening = (vb - va).dot(n);
        if opening <= -REEL_SPEED {
            continue;
        }
        let dv = n * (-REEL_SPEED - opening) * (1.0 - (-REEL_RATE * dt).exp());
        let (ia, ib) = (1.0 / ca.phys.mass, 1.0 / cb.phys.mass);
        crate::collision::set_velocity(&mut a, &mut da, va - dv * (ia / (ia + ib)));
        crate::collision::set_velocity(&mut b, &mut db, vb + dv * (ib / (ia + ib)));
    }
}

/// Chance per retail AI tick (1/30 s) that a computer captain fires its harpoons at its target, and that a grappled one sends its
/// boarders; boarding also needs an attacking strength above `AI_MIN_ATTACK` (docs/re/boarding.md §5).
const AI_HARPOON_CHANCE: f32 = 0.05;
const AI_BOARD_CHANCE: f32 = 0.2;
const AI_MIN_ATTACK: i32 = 10;
/// A chance per 1/30 s tick turned into one for a frame of `dt` seconds.
fn per_tick(chance: f32, dt: f32) -> f32 {
    1.0 - (1.0 - chance).powf(dt * 30.0)
}

/// The original's grapple-and-board routine, run by every AI-steered ship (not the player's command ship) against its current
/// target: with a free harpoon whose arc covers the target in harpoon range it fires at a 5 % chance per tick; once the grapple holds (or, at a
/// base, a boarding point is in reach) it boards at 20 % per tick if its boarding party is strong enough. PROVISIONAL: the
/// original also skips the harpoon while a friendly ship or scenery is in the line of fire; and only a ship its target has fired
/// at grapples (a retail Barque left an idle target alone and grappled ~17 s after being attacked).
#[allow(clippy::type_complexity)]
fn ai_boarding(
    time: Res<Time>,
    lib: Res<Library>,
    mut rng: ResMut<Rng>,
    mut commands: Commands,
    mut sfx: MessageWriter<Sfx>,
    boarders: Boarders,
    mut msg: MessageWriter<BoardMsg>,
    ships: Query<(Entity, &Ship, &Transform, &Health, Option<&crate::combat::GunStates>, &Boardable, Option<&MissionShip>, Option<&crate::ship::FleetPilot>), Without<PlayerShip>>,
    bases: Query<Entity, With<MissionBase>>,
    targets: Query<(&Transform, &Ship)>,
    (mut fired, mut attacked, captains): (MessageReader<crate::combat::Fired>, Local<std::collections::HashSet<(Entity, Entity)>>, Option<Res<crate::ship::CaptainTargets>>),
) {
    attacked.extend(fired.read().filter_map(|f| f.target.map(|t| (f.shooter, t))));
    let dt = time.delta_secs();
    let me = boarders.me();
    for (e, ship, tf, health, guns, own, ms, pilot) in &ships {
        if ms.is_some_and(|m| m.stance == crate::rules::Stance::Dummy || m.hold_fire) || pilot.is_some_and(|p| p.hold_fire) {
            continue;
        }
        // Mission ships: the rules' target. Others: an explicit fleet order, else their captain's target (ship::CaptainTargets),
        // else (skirmish enemies) the player's ship.
        let target = match ms {
            Some(m) => m.target,
            None => pilot.and_then(|p| p.target).or_else(|| captains.as_deref().and_then(|c| c.of(e))).or_else(|| me.filter(|_| ship.side != Side::Player)),
        };
        let strong = own.strength.attack > AI_MIN_ATTACK;
        if let Some(t) = target.filter(|t| boarders.hostile(e, *t))
            && let Ok((ttf, tship)) = targets.get(t)
        {
            let surface = tf.translation.distance(ttf.translation) - lib.classes[ship.class].radius - lib.classes[tship.class].radius;
            if attacked.contains(&(t, e)) && boarders.can_grapple(e) && harpoon_bears(&lib, ship, health, guns, tf, ttf.translation) && rng.f() < per_tick(AI_HARPOON_CHANCE, dt) {
                commands.entity(e).insert(Grapple::new(t, (surface.max(0.0) / HARPOON_SPEED).max(0.1)));
                sfx.write(Sfx { name: "Weapon_GrapHarpoon_Fire".into(), at: Some(tf.translation), vol: 0.7, max: 600.0 });
            }
            if strong && boarders.can_board(e, t) && rng.f() < per_tick(AI_BOARD_CHANCE, dt) {
                msg.write(BoardMsg::Board { boarder: Some(e), target: t });
            }
        }
        // PROVISIONAL: mission ships also board a boardable hostile base in reach without targeting it.
        if ms.is_some()
            && strong
            && let Some(t) = bases.iter().find(|t| boarders.can_board(e, *t))
            && rng.f() < per_tick(AI_BOARD_CHANCE, dt)
        {
            msg.write(BoardMsg::Board { boarder: Some(e), target: t });
        }
    }
}

/// Runs the casualties of every fight and ends those whose conditions lapsed.
fn fights(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<Library>,
    mut rng: ResMut<Rng>,
    assets: Option<Res<AssetServer>>,
    fxl: Option<Res<FxLib>>,
    mut sfx: MessageWriter<Sfx>,
    mission: Option<Res<Mission>>,
    mut settled: MessageWriter<Settled>,
    mut fighters: Query<(Entity, &mut Boarding)>,
    grapples: Query<(Entity, &Grapple)>,
    player: Query<Entity, With<PlayerShip>>,
    mut things: Query<(&Transform, Option<&Ship>, Option<&Drift>, &mut Boardable, Option<&Health>, Option<&MissionShip>, Option<&MissionBase>, Option<&crate::combat::BaseSide>)>,
) {
    let (dt, me) = (time.delta_secs(), player.single().ok());
    for (b, mut f) in &mut fighters {
        let t = f.target;
        let Ok([mut bi, mut ti]) = things.get_many_mut([b, t]) else {
            end_fight(&mut commands, b, &f.fx);
            continue;
        };
        let alive = |h: Option<&Health>| h.is_none_or(|h| !h.destroyed);
        let hostile = hostile_pair(
            mission.as_deref(),
            (owner_of(bi.5, bi.6), bi.1.map(|s| s.side).or(bi.7.map(|b| b.0))),
            (owner_of(ti.5, ti.6), ti.1.map(|s| s.side).or(ti.7.map(|b| b.0))),
        );
        let joined = ti.1.is_none() || grapples.iter().any(|(e, g)| g.flight <= 0.0 && ((e == b && g.target == t) || (e == t && g.target == b)));
        let near = in_range(&lib, &(bi.0, bi.1, bi.2), &(ti.0, ti.1, ti.2), &ti.3.points);
        if !alive(bi.4) || !alive(ti.4) || !hostile || !joined || (f.live && !near) {
            let (surface, rel) =
                (bi.0.translation.distance(ti.0.translation) - radius(&lib, bi.1) - radius(&lib, ti.1), (velocity(&(bi.0, bi.1, bi.2)) - velocity(&(ti.0, ti.1, ti.2))).length());
            info!("boarding: {b} breaks off {t} (hostile {hostile}, joined {joined}, surface {surface:.1}, relative speed {rel:.1})");
            end_fight(&mut commands, b, &f.fx);
            if Some(b) == me {
                say(&mut commands, assets.as_deref(), "retreat", rng.f());
            }
            continue;
        }
        // PROVISIONAL: the order to board a grappled ship waits for the winch to close the hulls instead of failing at once.
        if !near {
            continue;
        }
        if !f.live {
            // The crews meet: the fight effect plays on every boarding point of the target, and the first mates speak.
            f.live = true;
            f.fx = fxl.as_ref().map_or(vec![], |fxl| {
                ti.3.points
                    .iter()
                    .filter_map(|p| {
                        let local = Transform::from_translation(*p);
                        crate::fx::spawn(&mut commands, fxl, "Boarding_Fight", Some(t), local, *ti.0 * local, false)
                    })
                    .collect()
            });
            sfx.write(Sfx { name: "Ship_Boarding_Party".into(), at: Some(bi.0.translation), vol: 1.0, max: 600.0 });
            let line = if Some(b) == me {
                Some(if ti.1.is_some() { "boarding_party" } else { "landing_party" })
            } else if Some(t) == me {
                Some("repell_boarders")
            } else if ti.1.is_some_and(|s| s.side == Side::Player) {
                Some("own_ship_boarded")
            } else {
                None
            };
            if let Some(line) = line {
                say(&mut commands, assets.as_deref(), line, rng.f());
            }
        }
        f.since += dt;
        if f.since < f.interval {
            continue;
        }
        f.since = 0.0;
        if casualty(&mut bi.3.strength, &mut ti.3.strength, rng.f() >= 0.5) {
            let outcome = settle(&mut bi.3.strength, &mut ti.3.strength);
            info!("boarding: {b} against {t}: {outcome:?} (attack {} / defend {} left)", bi.3.strength.attack, ti.3.strength.defend);
            end_fight(&mut commands, b, &f.fx);
            settled.write(Settled { boarder: b, target: t, outcome });
        }
    }
}

/// The consequences of a finished fight: a won fight captures the target (new owner, stopped, crewed afresh by the captor's skill, a base
/// back at full strength; the grapple is released), the rules get a capture, and the player's crew speaks.
fn settled(
    mut msgs: MessageReader<Settled>,
    mut commands: Commands,
    assets: Option<Res<AssetServer>>,
    mut rng: ResMut<Rng>,
    mut sfx: MessageWriter<Sfx>,
    mut captured: MessageWriter<Captured>,
    mut things: Query<(Option<&mut Ship>, Option<&mut MissionShip>, Option<&mut MissionBase>, &mut Boardable, Option<&mut crate::combat::BaseSide>)>,
    members: Query<&crate::ship::FleetMember>,
) {
    for s in msgs.read() {
        let Ok([(bship, bms, _, bflag, _), (tship, tms, tbase, mut tflag, tside)]) = things.get_many_mut([s.boarder, s.target]) else { continue };
        let side_of = |ship: &Option<Mut<Ship>>| ship.as_ref().map(|x| x.side);
        let (ours, theirs, is_ship) =
            (side_of(&bship) == Some(Side::Player), side_of(&tship) == Some(Side::Player) || (tship.is_none() && side_of(&bship) == Some(Side::Enemy)), tship.is_some());
        let r = rng.f();
        match s.outcome {
            Outcome::Won => {
                let owner = |ms: &Option<Mut<MissionShip>>, mb: &Option<Mut<MissionBase>>, ship: &Option<Mut<Ship>>| {
                    ms.as_ref()
                        .map(|m| m.owner.clone())
                        .or_else(|| mb.as_ref().map(|m| m.owner.clone()))
                        .unwrap_or_else(|| if side_of(ship) == Some(Side::Player) { "Player".into() } else { "Enemy".into() })
                };
                let (by, victim) = (owner(&bms, &None, &bship), owner(&tms, &tbase, &tship));
                info!("boarding: {} captured {}", by, victim);
                captured.write(Captured { by: by.clone(), victim, unit: s.target });
                if let Some(mut m) = tms {
                    m.owner = by.clone();
                    m.target = None;
                    m.attack.clear();
                }
                if let Some(mut b) = tbase {
                    b.owner = by;
                }
                // A skirmish base changes sides with its guns and its place in the end rules.
                if let (Some(mut bs), Some(side)) = (tside, side_of(&bship)) {
                    bs.0 = side;
                }
                if let (Ok(member), true) = (members.get(s.boarder), is_ship) {
                    let mut member = member.clone();
                    member.1 = s.target.to_bits() as usize;
                    commands.entity(s.target).insert((member, crate::ship::FleetPilot::default()));
                }
                if let Some(mut sh) = tship {
                    sh.halt();
                    if let Some(side) = side_of(&bship) {
                        sh.side = side;
                    }
                }
                tflag.strength = if is_ship { Strength::ship(tflag.counts, bflag.skill.member()) } else { Strength::base(tflag.strength.full) };
                commands.entity(s.boarder).remove::<Grapple>();
                commands.entity(s.target).remove::<Grapple>();
                if ours {
                    sfx.write(Sfx::ui("HUD_Options_Dock"));
                    say(&mut commands, assets.as_deref(), if is_ship { "enemy_ship_conquered" } else { "enemy_base_conquered" }, r);
                } else if theirs {
                    say(&mut commands, assets.as_deref(), if is_ship { "own_ship_conquered" } else { "allied_base_conquered" }, r);
                }
            }
            Outcome::Lost if ours => say(&mut commands, assets.as_deref(), "retreat", r),
            Outcome::Lost if theirs => say(&mut commands, assets.as_deref(), "boarding_repelled", r),
            _ => {}
        }
    }
}

/// Dev hook `--script board`: the player grapples the nearest enemy and boards it as soon as the menu would allow (smoke test, screenshots).
fn dev_board(
    cfg: Res<crate::Config>,
    status: Res<BoardStatus>,
    mut msg: MessageWriter<BoardMsg>,
    mut step: Local<u8>,
    me: Query<&Transform, With<PlayerShip>>,
    foes: Query<(Entity, &Transform, &Ship)>,
) {
    if cfg.script.as_deref() != Some("board") || cfg.frame < 5 {
        return;
    }
    let Ok(me) = me.single() else { return };
    let Some((foe, ..)) = foes.iter().filter(|f| f.2.side == Side::Enemy).nearest_to(me.translation, |a| a.1.translation) else {
        return;
    };
    match *step {
        0 => {
            msg.write(BoardMsg::Grapple { grappler: None, target: foe });
            *step = 1;
        }
        1 if status.board(foe).1 => {
            msg.write(BoardMsg::Board { boarder: None, target: foe });
            *step = 2;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
