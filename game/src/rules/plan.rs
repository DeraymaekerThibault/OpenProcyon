//! The rule graph and what the dev autopilot wants from it (`--script mission`, tools/autoplay.py).
//!
//! A rule *provides* what its actions leave behind that another rule's gate can read (a flag set TRUE, a started timer, a played speech
//! event) and *needs* what its flag, timer and speech gates read. Rule A enables rule B when A provides something B needs. Chaining
//! backwards from the rules that win the mission marks every rule that can lead to the win `Critical` (with its number of hops to the
//! win); chaining back from the rules that lose it marks the rest of those `Hazard`; the others are `Optional`. Nothing here reads a
//! rule, task, group or unit name: only condition and action types and the names they carry as data.
//!
//! The world conditions of the rules the bot is after become `Want`s (`Script::plan`). The ones of the hazard rules become constraints:
//! groups to protect and volumes to keep out of. PROVISIONAL: only flag, timer and speech gates are followed, so a rule whose world
//! condition depends on another rule's side effect (a group made visible, a ship given to the player) is not linked to it.

use bevy::prelude::*;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};

use super::eval::{Rule, RuleClass, Script, Unit, Want, b, base, f, in_volume, members, s};
use crate::Nearest;

/// How a rule relates to the mission's ending.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// Can lead to a win; the number is the rules' hops to it (a win rule is 0).
    Critical(u32),
    /// Can lead to a loss (and to no win); the number is the hops to the loss.
    Hazard(u32),
    Optional,
}
impl Role {
    pub fn label(self) -> String {
        match self {
            Role::Critical(d) => format!("critical dist={d}"),
            Role::Hazard(d) => format!("hazard dist={d}"),
            Role::Optional => "optional".into(),
        }
    }
}

/// One thing the autopilot is after, and why: the rule it comes from, whether that rule is critical, and its hops to the win.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wanted {
    pub want: Want,
    pub rule: usize,
    pub critical: bool,
    pub dist: u32,
}

/// A want this close to the objective point counts as the one the point marks (PROVISIONAL).
const POINT_MARKS: f32 = 400.0;

/// Where a want is: the point or volume centre, or the unit's position.
fn want_at(want: Want, w: &[Unit]) -> Option<Vec3> {
    match want {
        Want::Reach(a) | Want::Lob(a) | Want::Leave(a) => Some(a),
        Want::Hit(e) | Want::Tow(e) | Want::Dock(e) => w.iter().find(|u| u.e == e).map(|u| u.pos),
        Want::Lifeboats => None,
    }
}

/// How far from a protected unit the autopilot looks for its attackers (PROVISIONAL).
const RAIDER_RADIUS: f32 = 600.0;

#[derive(Clone, PartialEq, Eq, Hash)]
enum Token {
    Flag(String),
    Timer(String),
    Speech(String),
    /// A group some rule makes visible: conditions about its units cannot hold before.
    Visible(String),
    /// A group some rule makes dockable.
    Dockable(String),
}

/// What the actions of a rule leave for the gates of others.
fn provides(r: &Rule) -> Vec<Token> {
    r.acts
        .iter()
        .filter_map(|a| match s(a, "Type") {
            "Set Flag Action" if b(a, "Boolean Value") => Some(Token::Flag(s(a, "Flag Name").into())),
            "Start Timer" => Some(Token::Timer(s(a, "Timer Name").into())),
            "Play speech event" => Some(Token::Speech(s(a, "Speech Event Name").into())),
            "Set Group/Unit Visibility" if b(a, "Boolean") => Some(Token::Visible(base(s(a, "Group/Unit Name")).into())),
            "Set Group/Unit Dockable" if b(a, "Boolean") => Some(Token::Dockable(base(s(a, "Group Name")).into())),
            _ => None,
        })
        .collect()
}

/// What a condition reads that some rule has to provide. A gate that already holds at the start (a flag that must be FALSE, a line
/// not yet played) needs nothing. A world condition about a group needs the group visible (and a dock target dockable) when some rule
/// ever makes it so: only then is there anything to wait for (PROVISIONAL: a group hidden for any other reason is not tracked).
fn needs(c: &Value) -> Vec<Token> {
    match s(c, "Type") {
        "Flag Condition" => b(c, "Boolean Value").then(|| Token::Flag(s(c, "Flag Name").into())).into_iter().collect(),
        "Timer Condition" => vec![Token::Timer(s(c, "Timer Name").into())],
        "Speech Event Played Once" => vec![Token::Speech(s(c, "Speech Event Name").into())],
        "Speech Event Not Played Yet" | "World Initialize" => vec![],
        _ => {
            const GROUPS: [&str; 10] = [
                "Group Name",
                "Group/Unit Name",
                "GroupA Name",
                "GroupB Name",
                "Dockers Group/Unit Name",
                "Targets Group/Unit Name",
                "Target to Tow",
                "Towing Group",
                "Unit Name",
                "Group/Unit",
            ];
            let mut out: Vec<Token> = GROUPS.iter().map(|k| s(c, k)).filter(|g| !g.is_empty()).map(|g| Token::Visible(base(g).into())).collect();
            if matches!(s(c, "Type"), "Group/Unit Docked" | "Group/Unit Is Docked") {
                out.push(Token::Dockable(base(s(c, "Targets Group/Unit Name")).into()));
            }
            out
        }
    }
}

/// Rules from which `seeds` can be reached by enabling steps, with the hops: a breadth-first walk from the seeds back to whoever
/// provides what their gates need.
fn chain(rules: &[Rule], providers: &HashMap<Token, Vec<usize>>, seeds: &[usize], met: &dyn Fn(&Token) -> bool) -> Vec<Option<u32>> {
    let mut dist = vec![None; rules.len()];
    let mut queue = VecDeque::new();
    for &i in seeds {
        dist[i] = Some(0);
        queue.push_back(i);
    }
    while let Some(i) = queue.pop_front() {
        let d = dist[i].unwrap_or(0);
        for t in rules[i].conds.iter().flat_map(needs).filter(|t| !met(t)) {
            for &j in providers.get(&t).into_iter().flatten() {
                if dist[j].is_none() {
                    dist[j] = Some(d + 1);
                    queue.push_back(j);
                }
            }
        }
    }
    dist
}

/// The rule graph of a mission, built once (`Graph::build`).
#[derive(Clone, Default)]
pub(super) struct Graph {
    roles: Vec<Role>,
    /// Groups whose destruction or damage a hazard rule waits for: the player is to keep them alive.
    protect: Vec<String>,
    /// Groups whose survival or number a scoring rule ('Add Victory Points') reads: kept alive when it costs nothing (mission 10's convoy).
    score: Vec<String>,
    /// (group, volume) a hazard rule waits for the group to be in: the player is to keep out of the volume.
    avoid: Vec<(String, String)>,
    /// (timer, equivalence, seconds) a hazard rule waits for: reported only.
    timers: Vec<(String, String, f32)>,
    /// Groups the player tows or is to tow, by the rules' own tow conditions and actions: the only ones the autopilot takes in tow.
    towable: HashSet<String>,
    /// Which rules provide each token.
    providers: HashMap<Token, Vec<usize>>,
}

impl Graph {
    pub(super) fn build(rules: &[Rule], player: &str) -> Graph {
        let mut providers: HashMap<Token, Vec<usize>> = HashMap::new();
        for (i, r) in rules.iter().enumerate() {
            for t in provides(r) {
                providers.entry(t).or_default().push(i);
            }
        }
        let seeds = |class: RuleClass| (0..rules.len()).filter(|&i| Script::rule_class(&rules[i]) == class).collect::<Vec<_>>();
        let (win, loss) = (chain(rules, &providers, &seeds(RuleClass::Win), &|_| false), chain(rules, &providers, &seeds(RuleClass::Loss), &|_| false));
        let roles: Vec<Role> = (0..rules.len())
            .map(|i| match (win[i], loss[i]) {
                (Some(d), _) => Role::Critical(d),
                (None, Some(d)) => Role::Hazard(d),
                _ => Role::Optional,
            })
            .collect();
        let mut g = Graph { roles, providers, ..Graph::default() };
        let keep = |list: &mut Vec<String>, spec: &str| {
            if !spec.is_empty() && !list.iter().any(|x| x == spec) {
                list.push(spec.into());
            }
        };
        for (i, r) in rules.iter().enumerate() {
            // What the rules expect the player to tow: the ship a tow condition names, and the other party of a tow action that has
            // the player on one side (mission 2's schooner, mission 4's sloop).
            for c in r.conds.iter().filter(|c| s(c, "Type") == "Is Ship In Tow" && base(s(c, "Towing Group")) == player) {
                g.towable.insert(base(s(c, "Target to Tow")).into());
            }
            for a in &r.acts {
                let (x, y) = match s(a, "Type") {
                    "Tow Ship" => (s(a, "Docking Group"), s(a, "Target to Tow")),
                    "Break Tow" => (s(a, "TowerGroupA"), s(a, "TowerGroupB")),
                    _ => continue,
                };
                if base(x) == player {
                    g.towable.insert(base(y).into());
                } else if base(y) == player {
                    g.towable.insert(base(x).into());
                }
            }
            if r.acts.iter().any(|a| s(a, "Type").contains("Victory Points")) {
                for c in &r.conds {
                    match s(c, "Type") {
                        "Group has X members" => keep(&mut g.score, s(c, "Group Name")),
                        "Group Destroyed" | "Group Under Attack" => keep(&mut g.score, s(c, "Group Name")),
                        "Group/Unit has >,<,= X damage" | "Group/Unit Vital Section has >,<,= X damage" | "Group/Unit Hit at least x Times" => {
                            keep(&mut g.score, s(c, "Group/Unit Name"))
                        }
                        _ => {}
                    }
                }
            }
            if !matches!(g.roles[i], Role::Hazard(_)) {
                continue;
            }
            for c in &r.conds {
                match s(c, "Type") {
                    "Group Destroyed" => keep(&mut g.protect, s(c, "Group Name")),
                    "Group has X members" if matches!(s(c, "Equivalence"), "Less Than" | "Equal To") => keep(&mut g.protect, s(c, "Group Name")),
                    "Group/Unit has >,<,= X damage" | "Group/Unit Vital Section has >,<,= X damage" if s(c, "Equivalence") != "Less Than" => {
                        keep(&mut g.protect, s(c, "Group/Unit Name"))
                    }
                    "Group Under Attack" => keep(&mut g.protect, s(c, "Group Name")),
                    "Group/Unit Hit at least x Times" => keep(&mut g.protect, s(c, "Group/Unit Name")),
                    "Enter Volume" | "Is Group In Volume" => g.avoid.push((s(c, "Group Name").into(), s(c, "Volume Name").into())),
                    "Timer Condition" => g.timers.push((s(c, "Timer Name").into(), s(c, "Equivalence").into(), f(c, "Time in seconds"))),
                    _ => {}
                }
            }
        }
        g
    }
}

impl Script {
    /// The rule's place in the graph (rules added after the build count as optional).
    pub fn role(&self, rule: usize) -> Role {
        self.graph.roles.get(rule).copied().unwrap_or(Role::Optional)
    }

    /// Where the objective marker is: a ship it follows, else the current objective point.
    pub fn objective_pos(&self) -> Option<Vec3> {
        self.point_pos.or_else(|| self.point.map(|i| crate::game_to_bevy(&self.points[i]["Position"])))
    }

    /// A world condition about a group some rule has yet to reveal (or make dockable) cannot hold before that rule has run: the rule
    /// is not open yet. PROVISIONAL: a group shown or made dockable from the start counts as pending until such a rule fires.
    pub fn waits_for_reveal(&self, rule: usize) -> bool {
        self.rules[rule]
            .conds
            .iter()
            .flat_map(needs)
            .any(|t| matches!(t, Token::Visible(_) | Token::Dockable(_)) && self.graph.providers.get(&t).is_some_and(|ps| !ps.is_empty() && ps.iter().all(|&j| !self.ran[j])))
    }

    /// Whether a token is already provided: the flag is set, the timer started, the line played, or the rule that reveals the group has fired.
    fn met(&self, t: &Token) -> bool {
        match t {
            Token::Flag(f) => self.flags.get(f).copied().unwrap_or(false),
            Token::Timer(n) => self.timers.contains_key(n),
            Token::Speech(n) => self.played.contains(n) || self.queue.iter().any(|q| q == n) || self.speaking.as_ref().is_some_and(|x| &x.0 == n),
            Token::Visible(_) | Token::Dockable(_) => self.graph.providers.get(t).is_some_and(|ps| ps.iter().any(|&j| self.ran[j])),
        }
    }

    /// The rules' roles now: the chains back from the endings skip every gate that is already met, so a rule only counts as critical
    /// while something it provides is still needed. Rules that have fired are left as the build had them.
    pub fn live_roles(&self) -> Vec<Role> {
        let seeds = |class: RuleClass| (0..self.rules.len()).filter(|&i| !self.fired[i] && Self::rule_class(&self.rules[i]) == class).collect::<Vec<_>>();
        let met = |t: &Token| self.met(t);
        let (win, loss) = (chain(&self.rules, &self.graph.providers, &seeds(RuleClass::Win), &met), chain(&self.rules, &self.graph.providers, &seeds(RuleClass::Loss), &met));
        (0..self.rules.len())
            .map(|i| match (win[i], loss[i]) {
                (Some(d), _) => Role::Critical(d),
                (None, Some(d)) => Role::Hazard(d),
                _ => Role::Optional,
            })
            .collect()
    }

    /// How many rules that have not fired are critical, optional and hazard now.
    pub fn role_counts(&self) -> (usize, usize, usize) {
        let roles = self.live_roles();
        let mut n = (0, 0, 0);
        for i in self.unfired() {
            match roles[i] {
                Role::Critical(_) => n.0 += 1,
                Role::Optional => n.1 += 1,
                Role::Hazard(_) => n.2 += 1,
            }
        }
        n
    }

    /// Whether the player is in the group of `spec`.
    fn player_in(w: &[Unit], p: &Unit, spec: &str) -> bool {
        members(w, spec).iter().any(|u| u.e == p.e)
    }

    /// What a condition of an unfired rule asks the player to do, now: `Some(wants)` (empty when it holds already, or is a flag, timer
    /// or speech gate) or `None` when the bot has no plan for it (reported as `no-plan`).
    ///
    /// Condition types the 13 missions put on rules and the bot leaves without a plan (PROVISIONAL): "Group/Unit Fired X Shots",
    /// "Group/Unit contains no mission essential ships", "Unit Flag Texture", "Unit is Within any Nebula", "Does Group Contain Unit Name",
    /// "Player has no lifeboats", "Team has captured a ship from Group/Unit", and any of the planned ones whose group is out of the
    /// player's reach (nobody to shoot in sight, the player not a party to a distance or attack condition).
    pub fn want_of(&self, c: &Value, w: &[Unit], p: &Unit) -> Option<Vec<Want>> {
        let player_in = |spec: &str| Self::player_in(w, p, spec);
        let flat = |at: Vec2| Vec3::new(at.x, 0.0, -at.y);
        // Shoot the live, visible members of a group; nothing to do when all are dead, no plan when none can be shot.
        let hit = |spec: &str, hostile_only: bool| -> Option<Vec<Want>> {
            let m = members(w, spec);
            if !m.is_empty() && m.iter().all(|u| !u.alive) {
                return Some(vec![]);
            }
            let v: Vec<Want> = m
                .iter()
                .filter(|u| u.alive && u.visible && u.owner != p.owner && (!hostile_only || self.hostile(&u.owner, &p.owner) || self.belt.contains_key(&u.e)))
                .map(|u| Want::Hit(u.e))
                .collect();
            (!v.is_empty()).then_some(v)
        };
        match s(c, "Type") {
            "World Initialize" | "Flag Condition" | "Timer Condition" | "Speech Event Played Once" | "Speech Event Not Played Yet" => Some(vec![]),
            "Enter Volume" | "Is Group In Volume" | "All Units from Group have entered trigger volume Once" | "Unit from Group enters trigger volume ( Once per Unit )" => {
                let (g, vol) = (s(c, "Group Name"), self.volume(s(c, "Volume Name")));
                let centre = flat(vol.centroid()?);
                if player_in(g) {
                    return Some(if in_volume(vol, p.pos) { vec![] } else { vec![Want::Reach(centre)] });
                }
                // A ship the rules tow (mission 2's schooner, mission 4's sloop): take it in tow, then sail it into the volume.
                let m: Vec<&Unit> = members(w, g).into_iter().filter(|u| u.alive && !u.primary && !self.hostile(&u.owner, &p.owner)).collect();
                if m.is_empty() || !self.graph.towable.contains(base(g)) {
                    return None;
                }
                let towed = |u: &Unit| self.tows.iter().any(|t| t.0 == p.e && t.1 == u.e);
                Some(if m.iter().all(|u| in_volume(vol, u.pos)) {
                    vec![]
                } else if m.iter().any(|u| towed(u)) {
                    vec![Want::Reach(centre)]
                } else {
                    m.iter().copied().nearest_to(p.pos, |u| u.pos).map(|u| Want::Tow(u.e)).into_iter().collect()
                })
            }
            "Exit Volume" => {
                let vol = self.volume(s(c, "Volume Name"));
                // Only a ship the player tows: the player's own ship starts inside big area volumes (mission 2), and running from their
                // centre took the bot off its objective.
                let mine = |u: &Unit| self.tows.iter().any(|t| t.0 == p.e && t.1 == u.e);
                let out = members(w, s(c, "Group Name")).iter().any(|u| mine(u) && u.alive && in_volume(vol, u.pos)).then(|| vol.centroid().map(flat)).flatten();
                match out {
                    Some(m) => Some(vec![Want::Leave(m)]),
                    None if player_in(s(c, "Group Name")) && in_volume(vol, p.pos) => None,
                    None => Some(vec![]),
                }
            }
            "Group to Group Distance" => {
                let (ga, gb) = (s(c, "GroupA Name"), s(c, "GroupB Name"));
                let other = if player_in(ga) {
                    gb
                } else if player_in(gb) {
                    ga
                } else {
                    return None;
                };
                let near = members(w, other).into_iter().filter(|u| u.alive && u.visible && u.e != p.e).nearest_to(p.pos, |u| u.pos)?;
                let d = near.pos.distance(p.pos);
                match s(c, "Equivalence") {
                    // Mission 4's stuck sloop and mission 6's pirate are spotted by closing in.
                    "Less Than" => Some(if d < f(c, "Distance") { vec![] } else { vec![Want::Reach(near.pos)] }),
                    "Greater Than" => Some(if d > f(c, "Distance") { vec![] } else { vec![Want::Leave(near.pos)] }),
                    _ => None,
                }
            }
            "Is Group A attacking Group B" if player_in(s(c, "GroupA Name")) => hit(s(c, "GroupB Name"), false),
            "Is Ship In Tow" if b(c, "Boolean") && player_in(s(c, "Towing Group")) => {
                // One ship, the nearest the condition names: a group is not towed member by member.
                let towed = |e: Entity| self.tows.iter().any(|t| t.0 == p.e && t.1 == e);
                let m = members(w, s(c, "Target to Tow"));
                if m.iter().any(|u| towed(u.e)) {
                    return Some(vec![]);
                }
                Some(m.into_iter().filter(|u| u.alive && u.e != p.e).nearest_to(p.pos, |u| u.pos).map(|u| Want::Tow(u.e)).into_iter().collect())
            }
            // The nearest point of the volume first, then a grid across it: a mortar mount covers only so much.
            "Mission 9 - If Mortar explodes within Area" => {
                let vol = self.volume(s(c, "Volume Name"));
                let at = |m: Vec2| Want::Lob(flat(m));
                Some(vol.near_inside(Vec2::new(p.pos.x, -p.pos.z)).map(at).into_iter().chain(vol.grid(8).into_iter().map(at)).collect())
            }
            "Player has hit Group/Unit at least x Times" | "Group/Unit Hit at least x Times by Player ( with eqivalence )" if base(s(c, "Player Name")) == p.owner => {
                hit(s(c, "Group/Unit Name"), false)
            }
            "Group Destroyed" => hit(s(c, "Group Name"), true),
            "Group/Unit Vital Section has >,<,= X damage" | "Group/Unit has >,<,= X damage" => hit(s(c, "Group/Unit Name"), true),
            "Group/Unit Hit at least x Times" => hit(s(c, "Group/Unit Name"), true),
            "Group Under Attack" => hit(s(c, "Group Name"), true),
            // Fewer members than now (a hostile group to wear down); more members is not something the player can arrange.
            "Group has X members" if matches!(s(c, "Equivalence"), "Less Than" | "Equal To") => hit(s(c, "Group Name"), true),
            "Player Killed A Object" if base(s(c, "Player Name")) == p.owner => {
                if s(c, "World Object Type") == "Asteroid" {
                    let rocks: Vec<Want> = w.iter().filter(|u| u.alive && self.belt.contains_key(&u.e)).map(|u| Want::Hit(u.e)).collect();
                    (!rocks.is_empty()).then_some(rocks)
                } else {
                    let ships: Vec<Want> =
                        w.iter().filter(|u| u.alive && u.visible && self.hostile(&u.owner, &p.owner) && !self.belt.contains_key(&u.e)).map(|u| Want::Hit(u.e)).collect();
                    (!ships.is_empty()).then_some(ships)
                }
            }
            "Group/Unit Docked" | "Group/Unit Is Docked" if player_in(s(c, "Dockers Group/Unit Name")) => {
                Some(members(w, s(c, "Targets Group/Unit Name")).iter().filter(|u| u.alive && u.e != p.e && !self.docked.contains(&(p.e, u.e))).map(|u| Want::Dock(u.e)).collect())
            }
            // Lifeboat pickup counts as a capture ('Player Vs Player capture count'): sail over the other side's boats.
            "Player Vs Player capture count" if base(s(c, "Player Name A")) == p.owner => Some(vec![Want::Lifeboats]),
            _ => None,
        }
    }

    /// Units that an unfired story rule (critical or optional) needs afloat: members of the groups its distance, volume, dock and tow
    /// conditions name, other than the player's own group. The bot does not attack them for an optional reason (mission 10's ironclad
    /// has to reach the convoy ship for the story to go on).
    pub fn needed_alive(&self, w: &[Unit], p: &Unit) -> HashSet<Entity> {
        let roles = self.live_roles();
        let mut out = HashSet::new();
        for i in self.unfired().into_iter().filter(|&i| !matches!(roles[i], Role::Hazard(_)) && Self::rule_class(&self.rules[i]).advances()) {
            for c in &self.rules[i].conds {
                let specs: Vec<&str> = match s(c, "Type") {
                    "Group to Group Distance" => vec![s(c, "GroupA Name"), s(c, "GroupB Name")],
                    "Enter Volume"
                    | "Is Group In Volume"
                    | "Exit Volume"
                    | "All Units from Group have entered trigger volume Once"
                    | "Unit from Group enters trigger volume ( Once per Unit )" => vec![s(c, "Group Name")],
                    "Group/Unit Docked" | "Group/Unit Is Docked" => vec![s(c, "Dockers Group/Unit Name"), s(c, "Targets Group/Unit Name")],
                    "Is Ship In Tow" => vec![s(c, "Target to Tow")],
                    _ => continue,
                };
                for spec in specs.into_iter().filter(|g| !g.is_empty() && !Self::player_in(w, p, g)) {
                    out.extend(members(w, spec).into_iter().filter(|u| u.alive && u.e != p.e).map(|u| u.e));
                }
            }
        }
        out
    }

    /// What the unfired rules ask the player to do, in the order to pursue it. Only rules that move the story on (class story or win)
    /// whose flag, timer and speech gates hold count. Critical rules come first, nearest to the win first, and the nearest want first
    /// within a rule distance; optional rules only count when no critical rule yields a want; hazard rules never do.
    pub fn plan(&self, w: &[Unit]) -> Vec<Wanted> {
        let Some(p) = w.iter().find(|u| u.primary && u.alive) else { return vec![] };
        let (mut critical, mut optional) = (vec![], vec![]);
        let roles = self.live_roles();
        let task_open = self.tasks.iter().any(|t| t.active && !t.complete && !t.failed);
        let point = self.objective_pos();
        for (i, r) in self.rules.iter().enumerate() {
            let role = roles[i];
            if self.fired[i] || matches!(role, Role::Hazard(_)) || (self.briefing_opening && super::eval::opening_rule(&r.name)) || !Self::rule_class(r).advances() {
                continue;
            }
            if r.conds.iter().any(|c| self.gate(c) == Some(false)) || self.waits_for_reveal(i) {
                continue;
            }
            let (crit, dist) = match role {
                Role::Critical(d) => (true, d),
                _ => (false, 0),
            };
            for want in r.conds.iter().filter_map(|c| self.want_of(c, w, p)).flatten() {
                // What the mission asks for right now: a want at the objective point while a task is open (mission 11's tow of the
                // galleon, mission 2's camp dock) counts as critical whatever its rule's place in the graph, tows and docks alike.
                let now = !crit && task_open && point.is_some_and(|o| want_at(want, w).is_some_and(|a| a.distance(o) < POINT_MARKS));
                let wanted = Wanted { want, rule: i, critical: crit || now, dist: if now { 0 } else { dist } };
                if crit || now { &mut critical } else { &mut optional }.push(wanted);
            }
        }
        // An optional reason to destroy a unit that a story rule needs afloat is dropped. A critical rule's reason is kept: those wants
        // are only ever in `critical`, which is used whenever it has any.
        if critical.is_empty() {
            let keep = self.needed_alive(w, p);
            optional.retain(|x| !matches!(x.want, Want::Hit(e) if keep.contains(&e)));
        }
        let mut out = if critical.is_empty() { optional } else { critical };
        let at = |x: &Wanted| want_at(x.want, w).map_or(if matches!(x.want, Want::Lifeboats) { 0.0 } else { f32::MAX }, |a| a.distance(p.pos));
        // Within a chain distance, the want the objective point marks (it is the designers' next stop), else the nearest.
        let marked = |x: &Wanted| match (self.objective_pos(), x.want) {
            (Some(o), Want::Reach(a) | Want::Lob(a) | Want::Leave(a)) => o.distance(a) < POINT_MARKS,
            _ => false,
        };
        out.sort_by(|a, b| a.dist.cmp(&b.dist).then(marked(b).cmp(&marked(a))).then(at(a).total_cmp(&at(b))));
        out
    }

    /// Live units of the groups the hazard rules need kept alive, other than the player's own fleet and the player's enemies.
    pub fn protected_units<'a>(&self, w: &'a [Unit], p: &Unit) -> Vec<&'a Unit> {
        self.graph.protect.iter().chain(&self.graph.score).flat_map(|g| members(w, g)).filter(|u| u.alive && u.owner != p.owner && !self.hostile(&u.owner, &p.owner)).collect()
    }

    /// The hostile that is closest to a protected unit and within `RAIDER_RADIUS` of it, among the units `ship` accepts. Stateless on
    /// purpose: the raider about to board is whichever is nearest the protected unit now (a pick kept while the bot chases one raider
    /// far off let mission 12's boarders take the Parliament).
    pub fn raider<'a>(&self, w: &'a [Unit], p: &Unit, ship: impl Fn(&Unit) -> bool) -> Option<&'a Unit> {
        let guarded = self.protected_units(w, p);
        w.iter()
            .filter(|u| u.alive && u.visible && self.hostile(&u.owner, &p.owner) && ship(u))
            .filter_map(|u| guarded.iter().map(|g| g.pos.distance(u.pos)).min_by(f32::total_cmp).filter(|d| *d < RAIDER_RADIUS).map(|d| (u, d)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|x| x.0)
    }

    /// Whether `at` lies in a volume a hazard rule would punish the player for entering.
    pub fn avoided(&self, w: &[Unit], p: &Unit, at: Vec3) -> bool {
        self.graph.avoid.iter().any(|(g, v)| Self::player_in(w, p, g) && in_volume(self.volume(v), at))
    }

    /// The constraints the hazard rules put on the player, for the stall report.
    pub fn hazards_text(&self) -> String {
        let timers: Vec<String> = self.graph.timers.iter().map(|(n, eq, t)| format!("{n} {eq} {t:.0}s")).collect();
        format!("protect={:?} protect_score={:?} avoid={:?} timers={timers:?}", self.graph.protect, self.graph.score, self.graph.avoid)
    }
}
