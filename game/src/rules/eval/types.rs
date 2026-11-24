//! Rule data types, their parsing and the small selector/value helpers.

use serde_json::Value;

use super::*;
use crate::names::name_newtype;

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Stance {
    Dummy,
    #[default]
    Neutral,
    Defensive,
    Aggressive,
    Persistent,
}
impl Stance {
    pub(in crate::rules) fn parse(s: &str) -> Stance {
        let s = s.to_lowercase();
        if s.contains("dummy") {
            Stance::Dummy
        } else if s.contains("aggress") {
            Stance::Aggressive
        } else if s.contains("persist") {
            Stance::Persistent
        } else if s.contains("defens") {
            Stance::Defensive
        } else {
            Stance::Neutral
        }
    }
}

/// Where `OP Follow Group` keeps a ship relative to the ship it follows (the follower's side of the leader).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Station {
    #[default]
    Astern,
    Ahead,
    AbeamStarboard,
    AbeamPort,
}
impl Station {
    pub const NAMES: [&'static str; 4] = ["Astern", "Ahead", "Abeam Starboard", "Abeam Port"];
    pub fn parse(s: &str) -> Option<Station> {
        match s {
            "Astern" => Some(Station::Astern),
            "Ahead" => Some(Station::Ahead),
            "Abeam Starboard" => Some(Station::AbeamStarboard),
            "Abeam Port" => Some(Station::AbeamPort),
            _ => None,
        }
    }
    /// The station `distance` units from the leader's centre, in the leader's frame (Bevy axes: bow -Z, starboard +X).
    pub fn offset(self, distance: f32) -> Vec3 {
        distance
            * match self {
                Station::Astern => Vec3::Z,
                Station::Ahead => Vec3::NEG_Z,
                Station::AbeamStarboard => Vec3::X,
                Station::AbeamPort => Vec3::NEG_X,
            }
    }
}

/// A standing `OP Follow Group` order of one ship (`MissionShip::follow`), carried out by the mission AI.
#[derive(Clone, PartialEq, Debug)]
pub struct FollowOrder {
    /// The group (or owner) whose ship it keeps station on.
    pub group: String,
    pub station: Station,
    pub distance: f32,
    /// The ship it chose from that group; the choice stands until that ship is gone.
    pub leader: Option<Entity>,
}

name_newtype!(
    /// A mission ship's name as rules refer to it ("Group/Unit Name" selectors, the script's `name` field).
    ShipId
);
name_newtype!(
    /// An owner or a named group: both act as group selectors in rules ("Player/Owner", `group`).
    GroupName
);

/// What the interpreter needs to know about one unit; built each frame from the ECS.
#[derive(Clone)]
pub struct Unit {
    pub e: Entity,
    /// TWT world object id (-1: none); `Script::tag` adds the named TWT groups containing it to `groups`.
    pub id: i64,
    pub name: ShipId,
    pub owner: GroupName,
    /// A named TWT group shadows this owner as a rule selector; set by `Script::tag`.
    pub owner_is_named_group: bool,
    pub groups: Vec<GroupName>,
    pub pos: Vec3,
    pub alive: bool,
    pub visible: bool,
    pub primary: bool,
    pub damage: f32,
    pub flag: String,
    pub attack: Vec<String>,
    /// A ship's heading (radians about +Y, `Ship::heading`; 0 for everything else) and its own speed along it in units per second
    /// (None for what is not a ship); `OP Teleport` and `OP Group Speed` read them.
    pub heading: f32,
    pub speed: Option<f32>,
    /// Fully cloaked (`cloak::State::Full`), not while cloaking or decloaking; `OP Group Is Cloaked`.
    pub cloaked: bool,
}

impl Unit {
    /// A live, visible, unnamed unit with no owner, groups, damage, flag or attack list; callers set what they have.
    pub fn new(e: Entity, id: i64, pos: Vec3) -> Self {
        Unit {
            e,
            id,
            name: ShipId::default(),
            owner: GroupName::default(),
            owner_is_named_group: false,
            groups: vec![],
            pos,
            alive: true,
            visible: true,
            primary: false,
            damage: 0.0,
            flag: String::new(),
            attack: vec![],
            heading: 0.0,
            speed: None,
            cloaked: false,
        }
    }
}

pub(in crate::rules) struct Rule {
    pub(in crate::rules) name: String,
    pub(in crate::rules) once: bool,
    pub(in crate::rules) conds: Vec<Value>,
    pub(in crate::rules) acts: Vec<Value>,
}
pub struct MusicCmd {
    pub(in crate::rules) file: String,
    pub(in crate::rules) fade_in: f32,
    pub(in crate::rules) fade_out: f32,
    pub(in crate::rules) vol: f32,
}
pub(in crate::rules) struct Task {
    pub(in crate::rules) name: String,
    pub(in crate::rules) text: String,
    pub(in crate::rules) active: bool,
    pub(in crate::rules) complete: bool,
    pub(in crate::rules) failed: bool,
}
#[derive(Default)]
pub(in crate::rules) struct Cam {
    pub(in crate::rules) path: Option<(String, f32, f32)>,
    pub(in crate::rules) attach: Option<(String, f32, f32, f32)>,
    pub(in crate::rules) focus_unit: Option<String>,
    pub(in crate::rules) focus_point: Option<String>,
    /// `attach` / `focus_unit` resolved to an entity each step (TWT groups need the unit list).
    pub(in crate::rules) attach_e: Option<Entity>,
    pub(in crate::rules) focus_e: Option<Entity>,
}

/// Effects that need the ECS; produced by `Script::step`.
pub enum Fx {
    Path {
        units: Vec<Entity>,
        path: String,
        closest: bool,
        looping: bool,
    },
    Asteroids {
        units: Vec<Entity>,
        path: String,
        closest: bool,
        mode: crate::combat::PathMode,
        speed: [f32; 2],
        tumble: [f32; 2],
    },
    Formation(String, Option<crate::ship::Formation>, bool),
    Primary(Vec<Entity>),
    Throttle(Vec<Entity>, f32),
    /// 'Dock Ships': dockers, then the dockee group's units (the first with a free dock point is used).
    Dock(Vec<Entity>, Vec<Entity>),
    /// 'Tow Ship' (tower, towee): linked at once when in tow range, else the tower sails over first (docking.rs `Tower`).
    Tow(Entity, Entity),
    Dockable(Vec<Entity>, bool),
    Boardable(Vec<Entity>, bool),
    /// 'Set Lifeboat Creation State': the ships' Create Lifeboats flag (lifeboats.rs reads it when they are destroyed).
    Lifeboats(Vec<Entity>, bool),
    IslandRepair(Vec<Entity>, bool),
    /// 'Set Dock Time': seconds a docker stays docked.
    DockTime(Vec<Entity>, f32),
    /// 'Set Group Throttle Percent': the current speed (0 stops the ship), not the player's selectable maximum.
    Speed(Vec<Entity>, f32),
    Destroy(Vec<Entity>),
    Teleport(Vec<Entity>, Vec3),
    Velocity(Vec<Entity>, f32),
    /// The same group-level rules routed to a nebula group of meteor emitters (nebula.rs), by group name.
    MeteorPath {
        group: String,
        path: String,
        closest: bool,
        mode: crate::combat::PathMode,
    },
    MeteorSpeed(String, f32),
    Stance(Vec<Entity>, Stance),
    /// 'Create/Release Event Effect': (point set, effect name, on) - the effect plays at every point of the set.
    Event(String, String, bool),
    /// 'Play Special Effect': a dialogue-folder sound cue or line, played without subtitle.
    Sound(String),
    /// 'Open Weapon Bar' (tutorial): the weapon panel's open state.
    WeaponBar(bool),
    Owner(Vec<Entity>, String),
    Visible(Vec<Entity>, bool),
    Invuln(Vec<Entity>, f32),
    Flag(Vec<Entity>, String),
    Banner(Vec<Entity>, String),
    /// 'Set Ship Name': the banner label shown for the ships (the group/unit name the rules match on is unchanged).
    Label(Vec<Entity>, String),
    Attack(Vec<Entity>, String),
    Ram(Vec<Entity>, String),
    Clear(Vec<Entity>),
    Damage(Vec<Entity>, f32),
    Hold(Vec<Entity>, bool),
    ResetHits(Vec<Entity>),
    /// 'Set/Clear Group/Unit Border Zone': the volume the units are leashed to (None clears it).
    Border(Vec<Entity>, Option<String>),
    /// 'Set Group/Unit Border Zone Return Point': where a leashed unit heads when it strays outside its zone.
    ReturnPoint(Vec<Entity>, Vec3),
    Towable(Vec<Entity>, bool),
    Movable(Vec<Entity>, bool),
    Collidable(Vec<Entity>, bool),
    /// 'Set Group/Unit AI Captain': human control flag; Captain_Human also emits a primary-ship handoff.
    Captain(Vec<Entity>, bool),
    WarningShots(Vec<Entity>, bool),
    /// 'Set Group/Unit Cloak State': (units, cloaked, AI may drop it to fire) (cloak.rs).
    Cloak(Vec<Entity>, bool, bool),
    /// '*State Init* Setup Etherium Current': the name of the path the current flows along, and its points.
    Current(String, Vec<Vec3>),
    /// 'OP Etherium Current' (engine-added): the current on the path `name` (made when there is none) gets the settings that are given.
    CurrentSet {
        name: String,
        path: Vec<Vec3>,
        strength: Option<f32>,
        width: Option<f32>,
        on: Option<bool>,
    },
    /// 'Mission 9 - Do Dark Matter Explosion': a blast over the volume.
    DarkMatter(String),
    /// 'Mission 9 - Teleport Longboat': the longboat appears beside the human's command ship.
    Longboat(Vec<Entity>),
    /// 'Dragon - Set AI Stance' / 'Dragon - Set Damage Threshold' (environment.rs dragons).
    DragonStance(Vec<Entity>, Stance),
    DragonThreshold(Vec<Entity>, f32),
    Orbit {
        unit: Option<Entity>,
        dist: f32,
        angle: f32,
    },
    /// 'OP Board Ship' (engine-added): the boarders, then the target group's units; each boarder takes the nearest live ship of it.
    Board(Vec<Entity>, Vec<Entity>),
    /// 'OP Teleport' (engine-added): each unit's new position, and the heading (radians about +Y) every ship takes when one is given;
    /// ships also lose their speed and turn rate.
    Place {
        units: Vec<(Entity, Vec3)>,
        heading: Option<f32>,
    },
    /// 'OP Follow Group' (engine-added): the ships keep `station` at `distance` from a ship of `group` (`MissionShip::follow`).
    Follow {
        units: Vec<Entity>,
        group: String,
        station: Station,
        distance: f32,
    },
    /// 'OP Place Mines' (engine-added): a mine of `bullet` at every point of the point set, given to `owner` (empty: hostile to every ship).
    Mines {
        set: String,
        bullet: String,
        owner: String,
    },
    /// 'OP Abandon Ship' (engine-added): the crews leave in their lifeboats and the hulls are left disabled afloat (lifeboats.rs `Abandoned`).
    Abandon(Vec<Entity>),
}

pub(in crate::rules) fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
pub(in crate::rules) fn f(v: &Value, k: &str) -> f32 {
    v[k].as_f64().unwrap_or(0.0) as f32
}
pub(in crate::rules) fn b(v: &Value, k: &str) -> bool {
    crate::data::wot_flag(&v[k])
}
pub(in crate::rules) fn cmp(eq: &str, a: f32, bb: f32) -> bool {
    super::schema::Equivalence::parse(eq).is_some_and(|e| e.holds(a, bb, 1e-4))
}
/// Group part of a 'Group,Unit' spec.
pub(in crate::rules) fn base(spec: &str) -> &str {
    spec.split(',').next().unwrap_or(spec)
}
pub(in crate::rules) const PLAYER: &str = "Player0 Group";
/// 'Group,Unit' selects a single unit. Bare names select a TWT group, or an owner if no such group exists.
pub(in crate::rules) fn matches(u: &Unit, spec: &str) -> bool {
    match spec.split_once(',') {
        Some((_, unit)) => u.name == unit || (unit == "HUMAN CONTROLLED COMMAND SHIP" && u.primary),
        None => (!u.owner_is_named_group && u.owner == spec) || u.groups.iter().any(|g| g == spec),
    }
}
pub(in crate::rules) fn members<'a>(w: &'a [Unit], spec: &str) -> Vec<&'a Unit> {
    w.iter().filter(|u| matches(u, spec)).collect()
}
pub(in crate::rules) fn ids(w: &[Unit], spec: &str) -> Vec<Entity> {
    members(w, spec).iter().map(|u| u.e).collect()
}

/// Every mission's rules call the human player's group "Player0 Group", whatever the TWT names that player.
pub(in crate::rules) fn alias_player(v: Value, player: &str) -> Value {
    match v {
        Value::String(x) if x == PLAYER || x.starts_with("Player0 Group,") => Value::String(x.replacen(PLAYER, player, 1)),
        Value::Object(m) => Value::Object(m.into_iter().map(|(k, x)| (k, alias_player(x, player))).collect()),
        other => other,
    }
}

/// A mission volume: a closed polygon in game axes (x, y), read once from the exported `[[x, y], ...]` list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Region(pub Vec<Vec2>);
/// Something an unfired rule waits for the player to do (`Script::wants`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Want {
    /// Sail into a trigger volume (its centroid, Bevy space).
    Reach(Vec3),
    /// Shoot this unit.
    Hit(Entity),
    /// Land a mortar shell in a volume (its centroid, Bevy space): mission 9's Dark Matter.
    Lob(Vec3),
    /// Take this ship in tow (mission 11's SS Harvester).
    Tow(Entity),
    /// Get the ship the player tows out of a volume (its centroid, Bevy space): mission 11's black hole.
    Leave(Vec3),
}
/// What an unknown volume name reads as: contains nothing.
pub(super) static NO_VOLUME: Region = Region(Vec::new());
impl Region {
    /// Vertices that are not two numbers are skipped, so a malformed mod volume never panics mid-battle.
    pub fn from_json(v: &Value) -> Self {
        Region(v.as_array().into_iter().flatten().filter_map(|p| Some(Vec2::new(p[0].as_f64()? as f32, p[1].as_f64()? as f32))).collect())
    }
    /// Even-odd rule; an empty polygon contains nothing.
    pub fn contains(&self, p: Vec2) -> bool {
        let v = &self.0;
        let mut inside = false;
        for i in 0..v.len() {
            let (a, b) = (v[i], v[(i + 1) % v.len()]);
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                inside = !inside;
            }
        }
        inside
    }
    /// Mean of the vertices (game axes); None for an empty polygon.
    pub fn centroid(&self) -> Option<Vec2> {
        (!self.0.is_empty()).then(|| self.0.iter().sum::<Vec2>() / self.0.len() as f32)
    }
    /// The point of the polygon nearest `from` (itself when inside), moved 15 units in toward the centroid so it lies inside.
    pub fn near_inside(&self, from: Vec2) -> Option<Vec2> {
        let c = self.centroid()?;
        if self.contains(from) {
            return Some(from);
        }
        let edge = |i: usize| {
            let (a, b) = (self.0[i], self.0[(i + 1) % self.0.len()]);
            let t = ((from - a).dot(b - a) / (b - a).length_squared().max(1e-6)).clamp(0.0, 1.0);
            a + (b - a) * t
        };
        let near = (0..self.0.len()).map(edge).min_by(|x, y| x.distance_squared(from).total_cmp(&y.distance_squared(from)))?;
        Some(near + (c - near).normalize_or_zero() * 15.0_f32.min(near.distance(c)))
    }
    /// Points inside the polygon on an `n` x `n` grid over its bounding box (cell centres).
    pub fn grid(&self, n: usize) -> Vec<Vec2> {
        let Some(lo) = self.0.iter().copied().reduce(Vec2::min) else { return vec![] };
        let hi = self.0.iter().copied().fold(lo, Vec2::max);
        let step = (hi - lo) / n as f32;
        (0..n * n).map(|k| lo + step * Vec2::new((k % n) as f32 + 0.5, (k / n) as f32 + 0.5)).filter(|q| self.contains(*q)).collect()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
/// Whether a Bevy-space position lies inside a volume (game y is Bevy -z).
pub(in crate::rules) fn in_volume(vol: &Region, pos: Vec3) -> bool {
    vol.contains(Vec2::new(pos.x, -pos.z))
}
/// Victory line of 'Goto Next Level'. Its World is the mission being started ("Mission02.twt" after mission 1), so the line
/// names the one just won (the ending overlay reads "Mission 1 complete", docs/re/campaign.md).
pub(in crate::rules) fn victory_text(next_world: &str) -> String {
    let digits: String = next_world.chars().filter(char::is_ascii_digit).collect();
    match digits.parse::<u32>() {
        Ok(n) if n > 1 => format!("Mission {} complete.", n - 1),
        _ => "Mission complete.".into(),
    }
}
/// Same test for game-axes (x, y) coordinates.
pub fn in_game_volume(vol: &Value, x: f32, y: f32) -> bool {
    Region::from_json(vol).contains(Vec2::new(x, y))
}

/// How a skirmish map script ended the game (`"victory": "script"`, see `SkirmishScript`); victory.rs turns it into the human's
/// outcome.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum EndKind {
    /// 'Team X Wins' names the team; 'Remaining Team Wins' (and an 'End Game' alone) leaves it open: the survivors win when they are
    /// one team.
    #[default]
    Winner,
    Team(String),
    /// A team whose required objective failed: it loses, the others win (engine-added objectives, `ext.rs`).
    Loser(String),
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ScriptEnd {
    pub kind: EndKind,
    /// String ids of the 'End Game' messages for the winners and the losers (placeholders such as "GAME STRING" are dropped).
    pub winner_message: Option<String>,
    pub loser_message: Option<String>,
}

/// What 'Player Killed A Object' counts: the schema's `World Object Type` choice. Anything but "Asteroid" reads as a ship, as the
/// condition always did (`--check-mission` rejects other spellings).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(in crate::rules) enum KillKind {
    Asteroid,
    Ship,
}
impl KillKind {
    pub(in crate::rules) fn parse(s: &str) -> KillKind {
        if s == "Asteroid" { KillKind::Asteroid } else { KillKind::Ship }
    }
}
