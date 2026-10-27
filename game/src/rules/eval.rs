use bevy::prelude::*;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::campaign::Mission;
use crate::game_to_bevy;

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
    pub(super) fn parse(s: &str) -> Stance {
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

/// What the interpreter needs to know about one unit; built each frame from the ECS.
#[derive(Clone)]
pub struct Unit {
    pub e: Entity,
    /// TWT world object id (-1: none); `Script::tag` adds the named TWT groups containing it to `groups`.
    pub id: i64,
    pub name: String,
    pub owner: String,
    /// A named TWT group shadows this owner as a rule selector; set by `Script::tag`.
    pub owner_is_named_group: bool,
    pub groups: Vec<String>,
    pub pos: Vec3,
    pub alive: bool,
    pub visible: bool,
    pub primary: bool,
    pub damage: f32,
    pub flag: String,
    pub attack: Vec<String>,
}

pub(super) struct Rule {
    pub(super) name: String,
    pub(super) once: bool,
    pub(super) conds: Vec<Value>,
    pub(super) acts: Vec<Value>,
}
pub struct MusicCmd {
    pub(super) file: String,
    pub(super) fade_in: f32,
    pub(super) fade_out: f32,
    pub(super) vol: f32,
}
pub(super) struct Task {
    pub(super) name: String,
    pub(super) text: String,
    pub(super) active: bool,
    pub(super) complete: bool,
    pub(super) failed: bool,
}
#[derive(Default)]
pub(super) struct Cam {
    pub(super) path: Option<(String, f32, f32)>,
    pub(super) attach: Option<(String, f32, f32, f32)>,
    pub(super) focus_unit: Option<String>,
    pub(super) focus_point: Option<String>,
    /// `attach` / `focus_unit` resolved to an entity each step (TWT groups need the unit list).
    pub(super) attach_e: Option<Entity>,
    pub(super) focus_e: Option<Entity>,
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
    /// '*State Init* Setup Etherium Current': the path the current flows along.
    Current(Vec<Vec3>),
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
}

pub struct Script {
    pub(super) rules: Vec<Rule>,
    pub(super) fired: Vec<bool>,
    /// Rules that fired at least once, repeating ones included (the autoplay report lists the others).
    pub(super) ran: Vec<bool>,
    pub(super) init_done: bool,
    pub time: f32,
    pub(super) flags: HashMap<String, bool>,
    pub(super) timers: HashMap<String, f32>,
    pub(super) stopped: HashSet<String>,
    pub(super) shots: HashMap<Entity, u32>,
    pub(super) point_sets: Value,
    pub played: HashSet<String>,
    pub(super) queue: VecDeque<String>,
    pub(super) speaking: Option<(String, f32)>,
    pub subtitle: String,
    pub(super) tasks: Vec<Task>,
    pub point: Option<usize>,
    /// Star map switches set by rules: the current objective point's visibility (PROVISIONAL default shown) and per-name map text visibility.
    pub point_on_map: bool,
    pub map_text: HashMap<String, bool>,
    pub(super) hold_fire: HashSet<String>,
    pub(super) hostile: HashSet<(String, String)>,
    pub(super) player: String,
    pub(super) tows: Vec<(Entity, Entity)>,
    pub(super) tow_specs: Vec<(String, String)>,
    pub(super) docked: HashSet<(Entity, Entity)>,
    pub(super) dock_times: HashMap<Entity, f32>,
    pub(super) cam: Cam,
    pub nis: bool,
    pub ended: Option<(String, bool)>,
    /// 'Open/Close HUD Texture Overlay': the full-screen texture (assets/hud/overlay) the views draw, e.g. the end credits.
    pub overlay: Option<String>,
    pub score: u32,
    pub(super) music: Vec<MusicCmd>,
    pub(super) media: Value,
    pub(super) paths: Value,
    pub(super) points: Value,
    pub(super) volumes: HashMap<String, Region>,
    pub(super) nebulae: Vec<Region>,
    pub(super) hits: HashMap<Entity, u32>,
    pub(super) hits_by: HashMap<(String, Entity), u32>,
    pub(super) fired_at: HashMap<Entity, (f32, Entity)>,
    pub(super) prev_inside: HashMap<(String, usize), bool>,
    /// 'All Units from Group have entered trigger volume Once': members seen inside, per group|volume.
    pub(super) entered: HashMap<String, HashSet<Entity>>,
    /// Every asteroid seen, with the group names it answered to (a destroyed rock leaves the unit list but still counts as attacked).
    pub(super) belt: HashMap<Entity, Vec<String>>,
    /// TWT named groups by world object id (a unit also answers to its current owner, `matches`).
    pub(super) groups: HashMap<i64, Vec<String>>,
    group_names: HashSet<String>,
    /// Lifeboats saved as the original tallies them: (collecting player, lifeboat owner, count). The only source of the rules'
    /// capture counts apart from `base_captures`; boarding a ship never touches it.
    pub(super) lifeboats_saved: Vec<(String, String, u32)>,
    /// Owner of every lifeboat in the world, refreshed by lifeboats.rs each frame (`Player has no lifeboats`).
    pub(super) lifeboat_owners: Vec<String>,
    /// PROVISIONAL bridge, bases only: boardings of a base by (capturing player, base's previous player). The original's capture count has no
    /// writer but lifeboat pickup, yet mission 12's defeat reads 'Procs vs RoyalPalace' (the palace is a boardable base), so it is kept alive.
    /// Ship boardings are not counted here: mission 6's 'PlayerStart vs PirateRunner' is the lifeboat route, its boarding route is 'Team has captured'.
    pub(super) base_captures: HashMap<(String, String), u32>,
    /// Units captured by boarding (boarding.rs) and the capturing player, for 'Team has captured a ship'.
    pub(super) captured: Vec<(String, Entity)>,
    /// Ships destroyed and the player who destroyed them, for 'Team has destroyed a ship'.
    pub(super) destroyed: Vec<(String, Entity)>,
    /// Skirmish teams: team name id -> its players (`set_teams`). Empty in the campaign, where a team condition counts anyone.
    pub(super) teams: HashMap<String, Vec<String>>,
    /// 'Grant Team X Points' totals per team name id ('Team has X points').
    pub team_points: HashMap<String, i32>,
    /// Event conditions (kills, captures, per-unit volume entries): events seen and events already reported, per condition key.
    /// PROVISIONAL: a rule that fires reports one pending event, so a repeating rule fires once per event.
    pub(super) events: HashMap<String, (u32, u32)>,
    /// Event keys the rule being evaluated would report if all its conditions hold.
    pub(super) consume: Vec<String>,
    /// The TWT's players (the capture conditions credit a captor's allies too).
    pub(super) players: Vec<String>,
    /// 'Set Current Objective Point On Ship': the objective marker follows this unit (`point_pos`).
    pub(super) point_on: Option<String>,
    pub point_pos: Option<Vec3>,
    pub(super) kills: HashMap<String, u32>,
    pub hint: String,
    pub(super) warned: HashSet<String>,
    /// 'Set Radar Active State': radar contacts shown.
    pub radar: bool,
    /// 'Crew Speech - Toggle On Off': crew callouts (crew.rs) allowed.
    pub crew_speech: bool,
    /// 'Crew Speech - Helm Off Course': a pending helmsman off-course line (crew.rs takes it).
    pub helm_off_course: bool,
    /// 'NIS Zoom': (vertical field of view in radians, rate per second) for the NIS camera; cleared at NIS End.
    pub zoom: Option<(f32, f32)>,
    /// 'NIS Toggle NIS mode Gun Accuracy': scripted broadsides always hit.
    pub nis_accuracy: bool,
    /// 'NIS Toggle All Objects Visibility': false hides every unit but the camera's subject.
    pub all_visible: bool,
    /// 'Set Nebula to Lock Objects': ships inside a moving storm are carried with it.
    pub nebula_trap: bool,
    /// 'Set Nebula Fadeout Distance': clouds fade out beyond this camera distance.
    pub nebula_fade: Option<f32>,
    /// 'Set Is In Calyan Abyss': the abyss ambience plays.
    pub abyss: bool,
    /// 'TUTORIAL - Pause when starmap opens?' and the star map's open state (written by starmap.rs).
    pub starmap_pause: bool,
    pub starmap_open: bool,
    /// Lob-weapon (mortar) shell bursts this frame, world positions ('Mission 9 - If Mortar explodes within Area').
    pub mortar_bursts: Vec<Vec3>,
    pub(super) mortar_seen: HashSet<String>,
    /// 'Open Crew and Arms Screens': the front end shows the crew and arms screen once (menu.rs clears it).
    pub crew_screen: bool,
}

pub(super) fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
pub(super) fn f(v: &Value, k: &str) -> f32 {
    v[k].as_f64().unwrap_or(0.0) as f32
}
pub(super) fn b(v: &Value, k: &str) -> bool {
    crate::data::wot_flag(&v[k])
}
pub(super) fn cmp(eq: &str, a: f32, bb: f32) -> bool {
    match eq {
        "Greater Than" => a > bb,
        "Less Than" => a < bb,
        "Equal To" => (a - bb).abs() < 1e-4,
        _ => false,
    }
}
/// Group part of a 'Group,Unit' spec.
pub(super) fn base(spec: &str) -> &str {
    spec.split(',').next().unwrap_or(spec)
}
pub(super) const PLAYER: &str = "Player0 Group";
/// 'Group,Unit' selects a single unit. Bare names select a TWT group, or an owner if no such group exists.
pub(super) fn matches(u: &Unit, spec: &str) -> bool {
    match spec.split_once(',') {
        Some((_, unit)) => u.name == unit || (unit == "HUMAN CONTROLLED COMMAND SHIP" && u.primary),
        None => (!u.owner_is_named_group && u.owner == spec) || u.groups.iter().any(|g| g == spec),
    }
}
pub(super) fn members<'a>(w: &'a [Unit], spec: &str) -> Vec<&'a Unit> {
    w.iter().filter(|u| matches(u, spec)).collect()
}
pub(super) fn ids(w: &[Unit], spec: &str) -> Vec<Entity> {
    members(w, spec).iter().map(|u| u.e).collect()
}

/// Every mission's rules call the human player's group "Player0 Group", whatever the TWT names that player.
pub(super) fn alias_player(v: Value, player: &str) -> Value {
    match v {
        Value::String(x) if x == PLAYER || x.starts_with("Player0 Group,") => Value::String(x.replacen(PLAYER, player, 1)),
        Value::Object(m) => Value::Object(m.into_iter().map(|(k, x)| (k, alias_player(x, player))).collect()),
        other => other,
    }
}

/// A mission volume: a closed polygon in game axes (x, y), read once from the exported `[[x, y], ...]` list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Region(pub Vec<Vec2>);
/// What an unknown volume name reads as: contains nothing.
static NO_VOLUME: Region = Region(Vec::new());
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
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
/// Whether a Bevy-space position lies inside a volume (game y is Bevy -z).
pub(super) fn in_volume(vol: &Region, pos: Vec3) -> bool {
    vol.contains(Vec2::new(pos.x, -pos.z))
}
/// Victory line of 'Goto Next Level'. Its World is the mission being started ("Mission02.twt" after mission 1), so the line
/// names the one just won (the ending overlay reads "Mission 1 complete", docs/re/campaign.md).
pub(super) fn victory_text(next_world: &str) -> String {
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

impl Script {
    pub fn new(data: &Value, media: Value) -> Script {
        let player = data["player_start"]["name"].as_str().unwrap_or(PLAYER).to_string();
        let rules: Vec<Rule> = data["rules"]
            .as_array()
            .expect("rules")
            .iter()
            .map(|r| Rule {
                name: s(r, "name").into(),
                once: r["Run Once"] != false,
                conds: r["conditions"].as_array().cloned().unwrap_or_default().into_iter().map(|c| alias_player(c, &player)).collect(),
                acts: r["actions"].as_array().cloned().unwrap_or_default().into_iter().map(|a| alias_player(a, &player)).collect(),
            })
            .collect();
        let n = rules.len();
        // The TWT's PlayerAllianceInfoVector lists the allied player pairs; every other pair of players starts hostile. Owners that are
        // not players ('NO PLAYER', unset) are never hostile. The rules' alliance actions then change it.
        let players: Vec<&str> = data["players"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let allied: HashSet<(&str, &str)> =
            data["alliances"].as_array().into_iter().flatten().filter_map(|p| Some((p[0].as_str()?, p[1].as_str()?))).flat_map(|(a, b)| [(a, b), (b, a)]).collect();
        let mut hostile = HashSet::new();
        for (i, a) in players.iter().enumerate() {
            for bb in &players[i + 1..] {
                if a != bb && !allied.contains(&(*a, *bb)) {
                    hostile.insert((a.to_string(), bb.to_string()));
                }
            }
        }
        let mut groups: HashMap<i64, Vec<String>> = HashMap::new();
        for (name, ids) in data["groups"].as_object().into_iter().flatten() {
            for id in ids.as_array().into_iter().flatten().filter_map(Value::as_i64) {
                groups.entry(id).or_default().push(name.clone());
            }
        }
        let tasks = data["tasks"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| Task { name: s(t, "Name").into(), text: s(t, "text").into(), active: t["Active"] == true, complete: false, failed: false })
            .collect();
        Script {
            rules,
            fired: vec![false; n],
            ran: vec![false; n],
            init_done: false,
            time: 0.0,
            flags: HashMap::new(),
            timers: HashMap::new(),
            stopped: HashSet::new(),
            shots: HashMap::new(),
            point_sets: data["environment"]["point_sets"].clone(),
            played: HashSet::new(),
            queue: VecDeque::new(),
            speaking: None,
            subtitle: String::new(),
            tasks,
            point: None,
            point_on_map: true,
            map_text: HashMap::new(),
            hold_fire: HashSet::new(),
            hostile,
            player,
            tows: vec![],
            tow_specs: vec![],
            docked: HashSet::new(),
            dock_times: HashMap::new(),
            cam: Cam::default(),
            nis: false,
            ended: None,
            overlay: None,
            score: 0,
            music: vec![],
            media,
            paths: data["paths"].clone(),
            points: data["points"].clone(),
            volumes: data["volumes"].as_object().into_iter().flatten().map(|(name, v)| (name.clone(), Region::from_json(v))).collect(),
            nebulae: crate::nebula::polygons(data, &crate::nebula::StormShift::default()),
            hits: HashMap::new(),
            hits_by: HashMap::new(),
            fired_at: HashMap::new(),
            prev_inside: HashMap::new(),
            entered: HashMap::new(),
            belt: HashMap::new(),
            groups,
            lifeboats_saved: vec![],
            lifeboat_owners: vec![],
            base_captures: HashMap::new(),
            captured: vec![],
            destroyed: vec![],
            teams: HashMap::new(),
            team_points: HashMap::new(),
            events: HashMap::new(),
            consume: vec![],
            players: players.iter().map(|p| p.to_string()).collect(),
            group_names: data["groups"].as_object().into_iter().flat_map(|g| g.keys().cloned()).collect(),
            point_on: None,
            point_pos: None,
            kills: HashMap::new(),
            hint: String::new(),
            warned: HashSet::new(),
            radar: true,
            crew_speech: true,
            helm_off_course: false,
            zoom: None,
            nis_accuracy: false,
            all_visible: true,
            nebula_trap: false,
            nebula_fade: None,
            abyss: false,
            starmap_pause: false,
            starmap_open: false,
            mortar_bursts: vec![],
            mortar_seen: HashSet::new(),
            crew_screen: false,
        }
    }

    fn named_group(&self, name: &str) -> bool {
        self.group_names.contains(name)
    }
    /// Adds TWT groups and resolves owner/group name collisions for rule selectors.
    pub fn tag(&self, w: &mut [Unit]) {
        for u in w {
            u.owner_is_named_group = self.named_group(&u.owner);
            if let Some(g) = self.groups.get(&u.id) {
                u.groups.extend(g.iter().cloned());
            }
        }
    }
    /// Whether a unit (TWT id, owner, name; `primary` for the human's command ship) answers to a rule spec, as `matches` does.
    pub fn answers(&self, id: i64, owner: &str, name: &str, primary: bool, spec: &str) -> bool {
        match spec.split_once(',') {
            Some((_, unit)) => name == unit || (unit == "HUMAN CONTROLLED COMMAND SHIP" && primary),
            None => (!self.named_group(owner) && owner == spec) || self.groups.get(&id).is_some_and(|g| g.iter().any(|g| g == spec)),
        }
    }
    /// Talking-head sprite of the line being spoken: the TWT speech event's FaceTexture (media `face`), if exported.
    pub(super) fn face(&self) -> Option<&str> {
        self.media["speech"][self.speaking.as_ref()?.0.as_str()]["face"].as_str()
    }
    pub fn hostile(&self, a: &str, bb: &str) -> bool {
        a != bb && (self.hostile.contains(&(a.into(), bb.into())) || self.hostile.contains(&(bb.into(), a.into())))
    }
    /// The human player's group name.
    pub fn player(&self) -> &str {
        &self.player
    }
    /// Called by docking.rs when a docker reaches or leaves its dock point.
    pub fn set_docked(&mut self, docker: Entity, dockee: Entity, on: bool) {
        if on {
            self.docked.insert((docker, dockee));
        } else {
            self.docked.remove(&(docker, dockee));
        }
    }
    /// The ship `tower` is towing (the first link), if any.
    pub fn towee_of(&self, tower: Entity) -> Option<Entity> {
        self.tows.iter().find(|t| t.0 == tower).map(|t| t.1)
    }
    /// Called by docking.rs when a tow is registered (`on`) or broken: links are the same list the rules' Tow / Break Tow use.
    pub fn set_tow(&mut self, tower: Entity, towee: Entity, on: bool) {
        self.tows.retain(|t| *t != (tower, towee));
        if on {
            self.tows.push((tower, towee));
        }
    }
    /// Breaks every link `tower` has (the menu's Tow stop).
    pub fn break_tows(&mut self, tower: Entity) {
        self.tows.retain(|t| t.0 != tower);
    }
    pub fn dock_time(&self, docker: Entity) -> Option<f32> {
        self.dock_times.get(&docker).copied()
    }
    /// Dev shortcut (`--script dock*`): mission 1 as it stands after the nebula battle, heading for the weapons tender (Objective 4).
    /// Every story rule before "Player Reaches Tender", the earlier objectives and the black-flag rules count as fired; their end state
    /// (flags, task, objective point) is applied. The tender volume, Objective 5 and the dock rules then run normally.
    pub fn skip_to_tender(&mut self) {
        let pos = |p: &str| self.rules.iter().position(|r| r.name.starts_with(p)).expect("mission 1 rule");
        let (tender, obj5) = (pos("Player Reaches Tender"), pos("Objective 5 "));
        for (i, r) in self.rules.iter().enumerate() {
            if i < tender || (i < obj5 && r.name.starts_with("Objective ")) || r.name.starts_with("Black Flag") {
                self.fired[i] = true;
            }
        }
        for f in ["Archapeligo Battle Over", "Lower Nebula Mark", "All Ships @ Nebula", "Battle @ Nebula", "Congrats Winners Played"] {
            self.flags.insert(f.into(), true);
        }
        let acts = [
            serde_json::json!({"Type": "Set Objective Task Active State", "Objective Task": "Head West Around Archipelago", "Active State": "FALSE"}),
            serde_json::json!({"Type": "Set Objective Task Active State", "Objective Task": "Head East To Weapons Tender", "Active State": "TRUE"}),
            serde_json::json!({"Type": "Set Current Objective Point", "Objective Point": "Head to Weapons Tender"}),
        ];
        for a in &acts {
            self.act(a, &[], &mut vec![], "dev skip");
        }
    }
    pub fn flag(&self, name: &str) -> bool {
        self.flags.get(name).copied().unwrap_or(false)
    }
    pub fn task_complete(&self, name: &str) -> bool {
        self.tasks.iter().any(|t| t.name == name && t.complete)
    }
    pub fn task_active(&self, name: &str) -> bool {
        self.tasks.iter().any(|t| t.name == name && t.active && !t.complete)
    }
    pub fn holds_fire(&self, owner: &str) -> bool {
        self.hold_fire.contains(owner)
    }
    /// Called by boarding.rs when `unit` (owned by `victim`) is captured by player `by`; `base`: the unit is a base, not a ship.
    /// PROVISIONAL (bases only, see `base_captures`): the captor's allies share the credit (mission 12's loss counts the capture under
    /// 'Procs', its ironclads belong to the allied 'Procs2').
    pub fn record_capture(&mut self, by: &str, victim: &str, unit: Entity, base: bool) {
        if base {
            let credit: Vec<String> = std::iter::once(by.to_string()).chain(self.players.iter().filter(|p| *p != by && *p != victim && !self.hostile(p, by)).cloned()).collect();
            for p in credit {
                *self.base_captures.entry((p, victim.into())).or_default() += 1;
            }
        }
        self.captured.push((by.into(), unit));
    }
    /// A ship was destroyed by player `by` (skirmish kills, 'Team has destroyed a ship').
    pub fn record_destroyed(&mut self, by: &str, unit: Entity) {
        self.destroyed.push((by.into(), unit));
    }
    /// Skirmish teams: (team name id, its players).
    pub fn set_teams(&mut self, teams: Vec<(String, Vec<String>)>) {
        self.teams = teams.into_iter().collect();
    }
    /// Whether `player` plays for `team`; true for any player when the team is unknown or not given (campaign rules).
    pub(super) fn in_team(&self, team: &str, player: &str) -> bool {
        team.is_empty() || self.teams.get(team).is_none_or(|p| p.iter().any(|x| x == player))
    }
    /// An event condition: `seen` events so far under `key`; true while some are unreported (more than `quota`), queuing one
    /// to be reported if the rule fires.
    pub(super) fn event(&mut self, key: String, seen: u32, quota: f32) -> bool {
        let e = self.events.entry(key.clone()).or_default();
        e.0 = e.0.max(seen);
        let pending = (e.0 - e.1) as f32 > quota;
        if pending {
            self.consume.push(key);
        }
        pending
    }
    /// Whether two players are allied (a player is allied with itself; names that are no player, e.g. 'NO PLAYER', are not).
    pub fn allied(&self, a: &str, b: &str) -> bool {
        a == b || (self.players.iter().any(|p| p == a) && self.players.iter().any(|p| p == b) && !self.hostile(a, b))
    }
    /// A ship of `collector` picked up a lifeboat of `owner` (lifeboats.rs). Counts it, and the local player collecting a boat of a
    /// player it is not allied with scores victory points, as the original does (the same counter as 'Add Victory Points').
    pub fn lifeboat_saved(&mut self, collector: &str, owner: &str) {
        match self.lifeboats_saved.iter_mut().find(|r| r.0 == collector && r.1 == owner) {
            Some(r) => r.2 += 1,
            None => self.lifeboats_saved.push((collector.into(), owner.into(), 1)),
        }
        if collector == self.player && !self.allied(collector, owner) {
            self.score += crate::lifeboats::PICKUP_POINTS;
        }
    }
    pub fn lifeboat_owners(&self) -> &[String] {
        &self.lifeboat_owners
    }
    pub fn set_lifeboat_owners(&mut self, owners: Vec<String>) {
        self.lifeboat_owners = owners;
    }
    /// Lifeboats `collector` saved of `owner`'s (the stats screen's future source).
    pub fn lifeboats_saved_by(&self, collector: &str, owner: &str) -> u32 {
        self.lifeboats_saved.iter().find(|r| r.0 == collector && r.1 == owner).map_or(0, |r| r.2)
    }
    pub fn tows(&self) -> &[(Entity, Entity)] {
        &self.tows
    }
    pub(super) fn warn(&mut self, what: &str) {
        if self.warned.insert(what.into()) {
            warn!("mission rules: unsupported {what} (ignored)");
        }
    }
    pub(super) fn volume(&self, name: &str) -> &Region {
        self.volumes.get(name).unwrap_or(&NO_VOLUME)
    }
    pub(super) fn timer(&self, name: &str) -> Option<f32> {
        self.timers.get(name).copied()
    }

    pub fn record_hit(&mut self, shooter_owner: Option<&str>, victim: Entity) {
        *self.hits.entry(victim).or_default() += 1;
        if let Some(o) = shooter_owner {
            *self.hits_by.entry((o.into(), victim)).or_default() += 1;
        }
    }
    pub fn record_fire(&mut self, shooter: Entity, target: Option<Entity>) {
        *self.shots.entry(shooter).or_default() += 1;
        if let Some(t) = target {
            self.fired_at.insert(shooter, (self.time, t));
        }
    }
    #[cfg(test)]
    pub fn queue_speech(&mut self, name: &str) {
        self.queue.push_back(name.into());
    }
    /// Names of the rules that have not fired yet, in script order. The opening NIS rules are left out: briefing.rs plays them.
    pub fn never_fired(&self) -> Vec<&str> {
        self.rules.iter().zip(&self.ran).filter(|(r, ran)| !**ran && !opening_rule(&r.name)).map(|(r, _)| r.name.trim()).collect()
    }
    pub fn tasks_text(&self) -> String {
        self.tasks
            .iter()
            .filter(|t| t.active)
            .map(|t| {
                format!(
                    "{}{}",
                    if t.complete {
                        "[done] "
                    } else if t.failed {
                        "[failed] "
                    } else {
                        ""
                    },
                    t.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Speech queue: one line at a time. Lines without audio show their text for a length-based time.
    /// Returns the sound file to start, if a new line began.
    /// Cut the line being spoken short (it counts as played). False if nothing was playing.
    pub(super) fn skip_speech(&mut self) -> bool {
        self.speaking.as_mut().map(|x| x.1 = 0.0).is_some()
    }

    pub(super) fn speech_tick(&mut self, dt: f32) -> Option<String> {
        if let Some((name, left)) = &mut self.speaking {
            *left -= dt;
            if *left <= 0.0 {
                self.played.insert(name.clone());
                self.speaking = None;
                self.subtitle.clear();
            }
        }
        if self.speaking.is_none() {
            let name = self.queue.pop_front()?;
            let m = &self.media["speech"][name.as_str()];
            let text = m["text"].as_str().unwrap_or("");
            let dur = m["duration"].as_f64().map(|d| d as f32 + 0.3).unwrap_or((text.len() as f32 / 14.0).max(2.0));
            self.subtitle = text.into();
            self.speaking = Some((name.clone(), dur));
            // The sound file of the TWT speech event (media `file`); mission 1 names its events after their files.
            return m["duration"].as_f64().map(|_| m["file"].as_str().map_or(name, String::from));
        }
        None
    }

    pub(super) fn cond(&mut self, c: &Value, w: &[Unit], repeating: bool) -> bool {
        let t = s(c, "Type");
        match t {
            "World Initialize" => !self.init_done,
            "Flag Condition" => self.flags.get(s(c, "Flag Name")).copied().unwrap_or(false) == b(c, "Boolean Value"),
            "Timer Condition" => self.timer(s(c, "Timer Name")).is_some_and(|v| cmp(s(c, "Equivalence"), v, f(c, "Time in seconds"))),
            "Speech Event Played Once" => self.played.contains(s(c, "Speech Event Name")),
            "Speech Event Not Played Yet" => {
                let n = s(c, "Speech Event Name");
                !self.played.contains(n) && !self.queue.iter().any(|q| q == n) && self.speaking.as_ref().is_none_or(|x| x.0 != n)
            }
            "Is Group In Volume" | "Enter Volume" => {
                let key = (s(c, "Group Name").to_string() + "|" + s(c, "Volume Name"), 0);
                let vol = self.volume(s(c, "Volume Name"));
                let m: Vec<_> = members(w, s(c, "Group Name")).into_iter().filter(|u| u.alive && u.visible).collect();
                let all = t == "Is Group In Volume" && b(c, "Entire Group");
                let now = !m.is_empty() && if all { m.iter().all(|u| in_volume(vol, u.pos)) } else { m.iter().any(|u| in_volume(vol, u.pos)) };
                // Hidden placement/reveal is not entry: track physical membership before applying visibility.
                let inside = members(w, s(c, "Group Name")).into_iter().any(|u| u.alive && in_volume(vol, u.pos));
                let armed = self.prev_inside.entry((key.0.clone(), 2)).or_insert(now);
                *armed |= !inside;
                let entered = *armed;
                let was = self.prev_inside.insert(key, inside).unwrap_or(false);
                if t == "Enter Volume" { now && if repeating { !was } else { entered } } else { now }
            }
            "Exit Volume" => {
                let key = (s(c, "Group Name").to_string() + "|" + s(c, "Volume Name"), 1);
                let vol = self.volume(s(c, "Volume Name"));
                let m: Vec<_> = members(w, s(c, "Group Name")).into_iter().filter(|u| u.alive && u.visible).collect();
                let now = !m.is_empty() && m.iter().any(|u| in_volume(vol, u.pos));
                let was = self.prev_inside.insert(key, now).unwrap_or(false);
                was && !now
            }
            "Group Destroyed" => {
                let m = members(w, s(c, "Group Name"));
                !m.is_empty() && m.iter().all(|u| !u.alive)
            }
            // PROVISIONAL: 'under attack' = any member has been hit since its hit count was last reset.
            "Group Under Attack" => members(w, s(c, "Group Name")).iter().any(|u| self.hits.get(&u.e).is_some_and(|n| *n > 0)),
            "Group/Unit Fired X Shots" => {
                let n: u32 = members(w, s(c, "Group/Unit Name")).iter().map(|u| self.shots.get(&u.e).copied().unwrap_or(0)).sum();
                cmp(s(c, "Equivalence"), n as f32, f(c, "Number Of Rounds Fired"))
            }
            // PROVISIONAL: vital sections are not modelled; the unit's overall damage stands in.
            "Group/Unit Vital Section has >,<,= X damage" => {
                let m = members(w, s(c, "Group/Unit Name"));
                !m.is_empty() && cmp(s(c, "Equivalence"), m.iter().map(|u| u.damage).fold(0.0, f32::max), f(c, "Damage Percent"))
            }
            "Does Group Contain Unit Name" => {
                let (g, unit) = s(c, "Unit Name").split_once(',').unwrap_or((s(c, "Unit Name"), ""));
                w.iter().any(|u| u.alive && u.owner == g && u.name == unit) == b(c, "Exists?")
            }
            "Is Ship In Tow" => {
                let (x, y) = (ids(w, s(c, "Towing Group")), ids(w, s(c, "Target to Tow")));
                self.tows.iter().any(|t| x.contains(&t.0) && y.contains(&t.1)) == b(c, "Boolean")
            }
            "Is Group A attacking Group B" => self.attacking(w, s(c, "GroupA Name"), s(c, "GroupB Name")),
            "Group/Unit Hit at least x Times" => {
                let n: u32 = members(w, s(c, "Group/Unit Name")).iter().map(|u| self.hits.get(&u.e).copied().unwrap_or(0)).sum();
                n as f32 >= f(c, "Number Of Times Hit")
            }
            "Group/Unit Hit at least x Times by Player ( with eqivalence )" => {
                let by = base(s(c, "Player Name"));
                let n: u32 = members(w, s(c, "Group/Unit Name")).iter().map(|u| self.hits_by.get(&(by.into(), u.e)).copied().unwrap_or(0)).sum();
                cmp(s(c, "Equivalence"), n as f32, f(c, "Number Of Times Hit"))
            }
            "Player has hit Group/Unit at least x Times" => {
                let by = base(s(c, "Player Name"));
                let n: u32 = members(w, s(c, "Group/Unit Name")).iter().map(|u| self.hits_by.get(&(by.into(), u.e)).copied().unwrap_or(0)).sum();
                n as f32 >= f(c, "Number Of Times Hit")
            }
            "Group has X members" => cmp(s(c, "Equivalence"), members(w, s(c, "Group Name")).iter().filter(|u| u.alive).count() as f32, f(c, "number")),
            "Group/Unit has >,<,= X damage" => {
                let m = members(w, s(c, "Group/Unit Name"));
                !m.is_empty() && cmp(s(c, "Equivalence"), m.iter().map(|u| u.damage).fold(0.0, f32::max), f(c, "Damage Percent"))
            }
            "Group/Unit contains no mission essential ships" => !w.iter().any(|u| u.owner == base(s(c, "Group Name")) && u.primary && u.alive),
            "Unit Flag Texture" => {
                // TWT group membership: ships that left the group (owner 'Dead Ships') still count.
                let m = members(w, base(s(c, "Unit Name")));
                !m.is_empty() && m.iter().all(|u| u.flag == s(c, "Flag Type")) == b(c, "Boolean")
            }
            "Group to Group Distance" => {
                let (a, bb) = (members(w, s(c, "GroupA Name")), members(w, s(c, "GroupB Name")));
                let d = a
                    .iter()
                    .filter(|u| u.alive && u.visible)
                    .flat_map(|x| bb.iter().filter(|u| u.alive && u.visible).map(move |y| x.pos.distance(y.pos)))
                    .fold(f32::INFINITY, f32::min);
                d.is_finite() && cmp(s(c, "Equivalence"), d, f(c, "Distance"))
            }
            "Group/Unit Docked" | "Group/Unit Is Docked" => {
                let (a, bb) = (members(w, s(c, "Dockers Group/Unit Name")), members(w, s(c, "Targets Group/Unit Name")));
                a.iter().any(|x| bb.iter().any(|y| self.docked.contains(&(x.e, y.e))))
            }
            "Unit is Within any Nebula" => members(w, s(c, "Unit Name")).iter().any(|u| u.alive && self.nebulae.iter().any(|p| in_volume(p, u.pos))),
            "All Units from Group have entered trigger volume Once" => {
                let key = s(c, "Group Name").to_string() + "|" + s(c, "Volume Name");
                let vol = self.volume(s(c, "Volume Name")).clone();
                let m: Vec<_> = members(w, s(c, "Group Name")).into_iter().filter(|u| u.alive).collect();
                let seen = self.entered.entry(key).or_default();
                seen.extend(m.iter().filter(|u| in_volume(&vol, u.pos)).map(|u| u.e));
                !m.is_empty() && m.iter().all(|u| seen.contains(&u.e))
            }
            // The original's count is the lifeboat table; an unknown player makes the condition false.
            "Player Vs Player capture count" => {
                let (a, bb) = (base(s(c, "Player Name A")), base(s(c, "Player Name B")));
                let known = |p: &str| self.players.iter().any(|x| x == p);
                let n = self.lifeboats_saved_by(a, bb) + self.base_captures.get(&(a.into(), bb.into())).copied().unwrap_or(0);
                known(a) && known(bb) && cmp(s(c, "Equivalence"), n as f32, f(c, "Number Of Captures"))
            }
            // Campaign maps have no teams: a capture by anyone of a unit of the group counts there.
            "Team has captured a ship from Group/Unit" => {
                let (team, group) = (s(c, "Team Name"), s(c, "Group/Unit"));
                let ids = ids(w, group);
                let n = self.captured.iter().filter(|(by, e)| ids.contains(e) && self.in_team(team, by)).count() as u32;
                self.event(format!("captured|{team}|{group}"), n, f(c, "Number Of Captures"))
            }
            "Team has destroyed a ship from Group/Unit" => {
                let (team, group) = (s(c, "Team Name"), s(c, "Group/Unit"));
                let ids = ids(w, group);
                let n = self.destroyed.iter().filter(|(by, e)| ids.contains(e) && self.in_team(team, by)).count() as u32;
                self.event(format!("destroyed|{team}|{group}"), n, f(c, "Number Of Ships Destroyed"))
            }
            // Each unit of the group counts once, the first time it is inside. PROVISIONAL: 'Team Name' is read as the team the rule
            // credits, not a filter on the units (Convoy Raid: the convoys entering the pirates' theft area score for the pirates).
            "Unit from Group enters trigger volume ( Once per Unit )" => {
                let key = format!("once|{}|{}", s(c, "Group Name"), s(c, "Volume Name"));
                let vol = self.volume(s(c, "Volume Name")).clone();
                let inside: Vec<Entity> = members(w, s(c, "Group Name")).iter().filter(|u| u.alive && in_volume(&vol, u.pos)).map(|u| u.e).collect();
                let seen = self.entered.entry(key.clone()).or_default();
                seen.extend(inside);
                let n = seen.len() as u32;
                self.event(key, n, 0.0)
            }
            "Team has X points" => self.team_points.get(s(c, "Team Name")).copied().unwrap_or(0) as f32 >= f(c, "Points"),
            // True for an unknown player; otherwise no lifeboat is owned by the player (a boat counts until it is removed).
            "Player has no lifeboats" => {
                let p = base(s(c, "Player Name"));
                !self.players.iter().any(|x| x == p) || !self.lifeboat_owners.iter().any(|o| o == p)
            }
            "Is Starmap Open" => self.starmap_open,
            // A mortar burst inside the volume since the rule last looked (bursts accumulate until the frame's step ends).
            "Mission 9 - If Mortar explodes within Area" => {
                let vol = self.volume(s(c, "Volume Name"));
                let hit = self.mortar_bursts.iter().any(|p| in_volume(vol, *p));
                if hit {
                    self.mortar_seen.insert(s(c, "Volume Name").into());
                }
                hit || self.mortar_seen.contains(s(c, "Volume Name"))
            }
            "Player Killed A Object" => s(c, "World Object Type") == "Asteroid" && self.kills.get(base(s(c, "Player Name"))).is_some_and(|n| *n > 0),
            other => {
                self.warn(&format!("condition '{other}'"));
                false
            }
        }
    }

    pub(super) fn attacking(&self, w: &[Unit], a: &str, bb: &str) -> bool {
        let target_group = base(bb);
        let targets = members(w, bb);
        // Rocks: a shot rock may already be gone from the unit list, so `belt` remembers which group names it answered to.
        let hit = |tg: &Entity| targets.iter().any(|x| x.e == *tg) || self.belt.get(tg).is_some_and(|g| g.iter().any(|g| g == target_group));
        members(w, a).iter().any(|u| u.attack.iter().any(|g| base(g) == target_group) || self.fired_at.get(&u.e).is_some_and(|(t, tg)| self.time - t < 6.0 && hit(tg)))
    }

    pub(super) fn act(&mut self, a: &Value, w: &[Unit], out: &mut Vec<Fx>, rule: &str) {
        let t = s(a, "Type");
        let g = |k: &str| ids(w, s(a, k));
        match t {
            "Start Timer" => {
                self.stopped.remove(s(a, "Timer Name"));
                self.timers.insert(s(a, "Timer Name").into(), 0.0);
            }
            "Stop Timer" => {
                self.stopped.insert(s(a, "Timer Name").into());
            }
            "Set Flag Action" => {
                self.flags.insert(s(a, "Flag Name").into(), b(a, "Boolean Value"));
            }
            "Play speech event" => self.queue.push_back(s(a, "Speech Event Name").into()),
            "Play Music Track" => self.music.push(MusicCmd {
                file: s(a, "File Name").into(),
                fade_in: f(a, "Fade In Time ( secs )"),
                fade_out: f(a, "Fade Out Time ( secs )"),
                vol: f(a, "New Volume ( 0 to 1 )"),
            }),
            "Set Objective Task Active State" => {
                let n = s(a, "Objective Task");
                if let Some(t) = self.tasks.iter_mut().find(|t| t.name == n) {
                    t.active = b(a, "Active State");
                }
            }
            "Set Objective Task Failed State" => {
                let n = s(a, "Objective Task");
                if let Some(t) = self.tasks.iter_mut().find(|t| t.name == n) {
                    t.failed = b(a, "Failed State");
                }
            }
            "Set Objective Task Complete State" => {
                let n = s(a, "Objective Task");
                if let Some(t) = self.tasks.iter_mut().find(|t| t.name == n) {
                    t.complete = b(a, "Complete State");
                }
            }
            "Set Current Objective Point" => {
                let n = s(a, "Objective Point");
                self.point = self.points.as_array().and_then(|p| p.iter().position(|x| x["Name"] == n));
                self.point_on = None;
            }
            "Set Current Objective Point On Ship" => self.point_on = Some(s(a, "Group Name").into()),
            "Set Current Objective Point Visible On Starmap" => self.point_on_map = b(a, "Visible On Starmap"),
            "Set Map Text Visibility" => {
                self.map_text.insert(s(a, "Map Text Name").into(), b(a, "Visible On Starmap"));
            }
            "Group to follow path" => {
                out.push(Fx::Path { units: g("Group Name"), path: s(a, "Path Name").into(), closest: b(a, "Find Closest Point"), looping: s(a, "Follow Mode") == "loop" });
                out.push(Fx::MeteorPath {
                    group: s(a, "Group Name").into(),
                    path: s(a, "Path Name").into(),
                    closest: b(a, "Find Closest Point"),
                    mode: crate::combat::PathMode::parse(s(a, "Follow Mode")),
                });
            }
            "Setup Space Animal Flock" => {
                out.push(Fx::Path { units: g("Group Name"), path: s(a, "Path Name").into(), closest: b(a, "Find Closest Point"), looping: s(a, "Follow Mode") == "loop" });
                out.push(Fx::Velocity(g("Group Name"), f(a, "Velocity")));
            }
            "Setup Asteroid Belt" => out.push(Fx::Asteroids {
                units: g("Group Name"),
                path: s(a, "Path Name").into(),
                closest: b(a, "Find Closest Point"),
                mode: crate::combat::PathMode::parse(s(a, "Follow Mode")),
                speed: [f(a, "Velocity Lower m/sec"), f(a, "Velocity Upper m/sec")],
                tumble: [f(a, "Tumble Lower Rads/sec"), f(a, "Tumble Upper Rads/sec")],
            }),
            "Set Group Throttle Percent" => out.push(Fx::Speed(g("Group Name"), f(a, "Throttle Percent"))),
            "Destroy Group/Unit" => out.push(Fx::Destroy(g("Group Name"))),
            "Transfer Group/Unit to Group" => out.push(Fx::Owner(g("Group/Unit to transfer"), s(a, "Target Group").into())),
            // An attack order flown bow-on at emergency speed; the contact damage is collision.rs `Ram` (PROVISIONAL).
            "GroupA to Ram GroupB" => out.push(Fx::Ram(g("Group Name"), s(a, "Group Unit Target").into())),
            "Teleport Group/Unit" => {
                if let Some(p) = self.point_sets[s(a, "Point Set Name")].get(0) {
                    out.push(Fx::Teleport(g("Group/Unit Name"), game_to_bevy(&p["pos"])));
                }
            }
            "Dock Ships" => out.push(Fx::Dock(g("Docking Group"), g("Target to Dock to"))),
            "Set Group/Unit Dockable" => out.push(Fx::Dockable(g("Group Name"), b(a, "Boolean"))),
            "Set Group/Unit Boardable" => out.push(Fx::Boardable(g("Group Name"), b(a, "Boolean"))),
            "Toggle Island Repair When Docked" => out.push(Fx::IslandRepair(g("Group Name"), b(a, "Repair when docked"))),
            "Set Dock Time" => out.push(Fx::DockTime(g("Docking GroupUnit"), f(a, "Dock Time"))),
            "Set Group Space Objects velocity" => {
                out.push(Fx::Velocity(g("Group Name"), f(a, "Velocity")));
                out.push(Fx::MeteorSpeed(s(a, "Group Name").into(), f(a, "Velocity")));
            }
            "*State Init* Setup Nebula" => {} // Spawned from the exported setup and point set by environment.rs.
            "Set Max Throttle Percent (Max user settable)" => out.push(Fx::Throttle(g("Group Name"), f(a, "Throttle Percent"))),
            // PROVISIONAL: a top-speed cap is the same speed limit as the max throttle (mission 9 nebula drain).
            "Set Ship Top Speed Percentage" => out.push(Fx::Throttle(g("Group Name"), f(a, "Throttle Speed"))),
            // The retail name string comes from the mission media (tools/export_campaign_media.py); unknown keys are ignored.
            "Set Ship Name" => match self.media["ship_names"][s(a, "New Ship Name")].as_str() {
                Some(name) => out.push(Fx::Label(g("Group/Unit Name"), name.into())),
                None => self.warn(&format!("ship name '{}'", s(a, "New Ship Name"))),
            },
            "Set Group/Unit AI Stance" => {
                let st = Stance::parse(s(a, "AI Stance"));
                out.push(Fx::Stance(g("Group/Unit Name"), st));
            }
            "Set Group/Unit Owner" => out.push(Fx::Owner(g("Group/Unit Name"), s(a, "New Owner").into())),
            "Set Group/Unit Visibility" => out.push(Fx::Visible(g("Group/Unit Name"), b(a, "Boolean"))),
            "Set Group/Unit Vulnerability" => out.push(Fx::Invuln(g("Group Name"), f(a, "Invulnerable Percent"))),
            "Set Ships Flag Texture" => out.push(Fx::Flag(g("Group Name"), s(a, "Flag Texture").into())),
            "Set Ship Banner Type" => out.push(Fx::Banner(g("Group/Unit Name"), s(a, "Banner Type").into())),
            "GroupA to attack GroupB" => out.push(Fx::Attack(g("Attack Group"), s(a, "Target Group").into())),
            "Clear all AI commands" => out.push(Fx::Clear(g("Group/Unit Name"))),
            "Damage Group/Unit by X percent" => out.push(Fx::Damage(g("Group Name"), f(a, "Percent"))),
            // A missing Boolean (older rules) means hold.
            "Set Group/Unit Hold Position" => out.push(Fx::Hold(g("Group/Unit Name"), a["Boolean"].is_null() || b(a, "Boolean"))),
            "Reset Hit Count" => out.push(Fx::ResetHits(g("Group/Unit Name"))),
            "Set FleetFormation Type" => {
                out.push(Fx::Formation(base(s(a, "Player Name")).into(), Some(crate::ship::Formation::parse(s(a, "Formation Type"))), b(a, "Hold Formation")))
            }
            "Set Fleet Hold Formation" => out.push(Fx::Formation(base(s(a, "Player Name")).into(), None, b(a, "Hold Formation"))),
            "Set Fleet Primary Ship" => out.push(Fx::Primary(g("Group/Unit Name"))),
            "Set Fleet Hold Fire" => {
                let o = base(s(a, "PLAYER FLEET")).to_string();
                if b(a, "HOLD FIRE") {
                    self.hold_fire.insert(o);
                } else {
                    self.hold_fire.remove(&o);
                }
            }
            "Set alliance between PlayerA and PlayerB to TRUE/FALSE" => {
                let (x, y) = (base(s(a, "PLAYER A")).to_string(), base(s(a, "PLAYER B")).to_string());
                self.hostile.retain(|p| !((p.0 == x && p.1 == y) || (p.0 == y && p.1 == x)));
                if !b(a, "SET ALLIANCE TO") {
                    self.hostile.insert((x, y));
                }
            }
            "Tow Ship" => self.tow_specs.push((s(a, "Docking Group").into(), s(a, "Target to Tow").into())),
            "Break Tow" => {
                let (x, y) = (ids(w, s(a, "TowerGroupA")), ids(w, s(a, "TowerGroupB")));
                self.tows.retain(|t| !(x.contains(&t.0) && y.contains(&t.1)));
                self.tow_specs.retain(|p| !(p.0 == s(a, "TowerGroupA") && p.1 == s(a, "TowerGroupB")));
            }
            "NIS Start" => {
                self.nis = true;
                self.cam = Cam::default();
            }
            "NIS End" => {
                self.nis = false;
                self.cam = Cam::default();
                self.zoom = None;
                self.all_visible = true;
            }
            "NIS Set Camera Path" => {
                self.cam.path = Some((s(a, "Path Name").into(), 0.0, 60.0));
            }
            "NIS Set Camera Speed" | "NIS Set Transition Camera Speed" => {
                if let Some(p) = &mut self.cam.path {
                    p.2 = f(a, "Max Velocity");
                }
            }
            "NIS Focus camera on Group/Unit" => {
                self.cam.focus_unit = Some(s(a, "GroupUnit Name").into());
                self.cam.focus_point = None;
                self.cam.attach = None;
            }
            "NIS Focus camera on Point" => {
                self.cam.focus_point = Some(s(a, "Point Set Name").into());
                self.cam.focus_unit = None;
            }
            "NIS Attach Camera" => {
                self.cam.attach = Some((s(a, "Group/Unit Name").into(), f(a, "Distance"), f(a, "Angle YZ"), f(a, "Angle XY")));
                self.cam.path = None;
            }
            // PROVISIONAL: a fixed offset from the unit ('Jump to point'), shown like an attached camera.
            "NIS Position Camera Relative to Object" => {
                self.cam.attach = Some((s(a, "Unit Name").into(), f(a, "Distance"), f(a, "Angle YZ"), f(a, "Angle XY")));
                self.cam.path = None;
            }
            "NIS Focus on Main Ship" => {
                self.cam.focus_unit = Some(format!("{},HUMAN CONTROLLED COMMAND SHIP", self.player));
                self.cam.focus_point = None;
                self.cam.attach = None;
            }
            "Set Group/Unit Cloak State" => out.push(Fx::Cloak(g("Group Name"), b(a, "Cloak State"), b(a, "AI Can Override"))),
            "Focus Camera On Group" => {
                let unit = members(w, s(a, "GroupUnit Name")).first().filter(|u| !u.primary).map(|u| u.e);
                out.push(Fx::Orbit { unit, dist: f(a, "Distance"), angle: f(a, "Relative Angle") });
            }
            "Add Victory Points for SinglePlayer" => self.score += a["Victory points to be added"].as_u64().unwrap_or(0) as u32,
            "Goto Next Level" => {
                if self.ended.is_none() {
                    self.ended = Some((victory_text(s(a, "World")), true));
                }
            }
            "End Game" => {
                if self.ended.is_none() {
                    let id = s(a, "Loser - Custom Message String ID");
                    self.ended = Some((self.media["messages"][id].as_str().unwrap_or("Mission failed").into(), false));
                }
            }
            "Create/Release Event Effect" => out.push(Fx::Event(s(a, "Point Set").into(), s(a, "Effect Name").into(), b(a, "Boolean State"))),
            "Open HUD Texture Overlay" => self.overlay = Some(s(a, "Texture Base Name").to_lowercase()),
            "Close HUD Texture Overlay" => self.overlay = None,
            "Open Weapon Bar" => out.push(Fx::WeaponBar(b(a, "OpenState"))),
            "Play Special Effect" => out.push(Fx::Sound(s(a, "File Name").into())),
            "Set Lifeboat Creation State" => out.push(Fx::Lifeboats(g("Group Name"), b(a, "Boolean"))),
            // Placement-time AI stance and path of one ship (the name/owner/primary fields are applied at spawn, campaign.rs).
            "*State Init* Setup Ship" => {
                let id = a["World Object ID"].as_i64().unwrap_or(-2);
                let units: Vec<Entity> = w.iter().filter(|u| u.id == id).map(|u| u.e).collect();
                out.push(Fx::Stance(units.clone(), Stance::parse(s(a, "AI Stance"))));
                if self.paths.get(s(a, "Ship Path")).is_some() {
                    out.push(Fx::Path { units, path: s(a, "Ship Path").into(), closest: false, looping: s(a, "Follow Mode") == "loop" });
                }
            }
            // Island setups are applied at spawn (campaign.rs: owner, combat strength); mission essential is the primary-alive loss rule.
            "*State Init* Setup Island" | "Set Group/Unit Mission Essential" => {}
            "Set Radar Active State" => self.radar = b(a, "Radar Active"),
            "Set Group/Unit Towable" => out.push(Fx::Towable(g("Group Name"), b(a, "Boolean"))),
            "Set Group/Unit Movable" => out.push(Fx::Movable(g("Group Name"), b(a, "Boolean"))),
            "SetCollidable" => out.push(Fx::Collidable(g("Group/Unit Name"), b(a, "Boolean"))),
            "Set Group/Unit Border Zone" => out.push(Fx::Border(g("Group/Unit Name"), Some(s(a, "Polygon Name").into()))),
            "Clear Group/Unit Border Zone" => out.push(Fx::Border(g("Group/Unit Name"), None)),
            "Set Group/Unit Border Zone Return Point" => {
                if let Some(p) = self.point_sets[s(a, "Pointset Name")].get(0) {
                    out.push(Fx::ReturnPoint(g("Group/Unit Name"), game_to_bevy(&p["pos"])));
                }
            }
            "Crew Speech - Toggle On Off" => self.crew_speech = b(a, "Crew Speech State State"),
            "Crew Speech - Helm Off Course" => self.helm_off_course = true,
            "Open Crew and Arms Screens" => self.crew_screen = true,
            "Set Is In Calyan Abyss" => self.abyss = b(a, "Boolean Value"),
            // Only Captain_Human is used by the shipped maps; the other captains keep the stance AI.
            "Set Group/Unit AI Captain" => {
                let units = g("Group/Unit Name");
                let human = s(a, "AI Stance") == "Captain_Human";
                out.push(Fx::Captain(units.clone(), human));
                if human {
                    out.push(Fx::Primary(units));
                }
            }
            // PROVISIONAL: FOV is the vertical angle in radians, speed its change per second.
            "NIS Zoom" => self.zoom = Some((f(a, "FOV").max(0.05), f(a, "Speed").max(0.01))),
            "NIS Toggle NIS mode Gun Accuracy" => self.nis_accuracy = b(a, "Use NIS Gun Accuracy"),
            "NIS Toggle All Objects Visibility" => self.all_visible = b(a, "All Objects Visible"),
            "Set Nebula Fadeout Distance" => self.nebula_fade = Some(f(a, "Fadeout Distance")),
            "Set Nebula to Lock Objects" => self.nebula_trap = b(a, "Trap"),
            "Set Group/Unit Warning Shot Mode" => out.push(Fx::WarningShots(g("GroupUnit Name"), b(a, "Boolean"))),
            "TUTORIAL - Pause when starmap opens?" => self.starmap_pause = b(a, "Pause when starmap opens"),
            "*State Init* Setup Etherium Current" => match self.paths[s(a, "Etherium Path")].as_array() {
                Some(p) => out.push(Fx::Current(p.iter().map(game_to_bevy).collect())),
                // Mission 10's 'bend' current names a path the TWT does not have (retail data); it has no effect there either.
                None => debug!("etherium current without path '{}'", s(a, "Etherium Path")),
            },
            "Mission 9 - Do Dark Matter Explosion" => out.push(Fx::DarkMatter(s(a, "Affected Area").into())),
            "Mission 9 - Teleport Longboat" => out.push(Fx::Longboat(g("Longboat Group Name"))),
            "Dragon - Set AI Stance" => out.push(Fx::DragonStance(g("Group/Unit Name"), Stance::parse(s(a, "AI Stance")))),
            "Dragon - Set Damage Threshold" => out.push(Fx::DragonThreshold(g("Group Name"), f(a, "Damage Threshold"))),
            "Grant Team X Points" => *self.team_points.entry(s(a, "Team Name").into()).or_default() += f(a, "Points") as i32,
            // Skirmish only: the winner is decided by victory.rs from the map's end rules; objective markers are not shown yet.
            "Team X Wins" | "Remaining Team Wins" | "Setup Team Objective" => {}
            "Reset Shots Fired Count" => {
                for e in g("Group/Unit Name") {
                    self.shots.remove(&e);
                }
            }
            other => self.warn(&format!("action '{other}' in rule '{rule}'")),
        }
    }

    /// Advance the script by `dt`: timers, speech, rules. Returns the ECS effects and any speech file to start.
    pub fn step(&mut self, w: &[Unit], dt: f32, opening: bool) -> (Vec<Fx>, Option<String>) {
        self.time += dt;
        // Timers wait out the opening briefing so the fleets stay put until it is done.
        for (k, v) in self.timers.iter_mut().filter(|_| !opening) {
            if !self.stopped.contains(k) {
                *v += dt;
            }
        }
        let voice = if opening { None } else { self.speech_tick(dt) };
        let mut out = vec![];
        let mut w = w.to_vec();
        let rules = std::mem::take(&mut self.rules);
        for (i, r) in rules.iter().enumerate() {
            // The opening NIS camera/speech rules are replaced by briefing.rs (which marks its speech as played).
            if self.fired[i] || opening_rule(&r.name) {
                continue;
            }
            let mut ok = true;
            self.consume.clear();
            for c in &r.conds {
                if !self.cond(c, &w, !r.once) {
                    ok = false;
                }
            } // evaluate all: Enter Volume edge state must advance
            if !ok {
                continue;
            }
            for key in std::mem::take(&mut self.consume) {
                self.events.entry(key).or_default().1 += 1;
            }
            if r.once {
                self.fired[i] = true;
            }
            self.ran[i] = true;
            info!("t={:.1} rule #{i} fired: {}", self.time, r.name.trim());
            for a in &r.acts {
                let from = out.len();
                self.act(a, &w, &mut out, &r.name);
                // Later rules must see visibility changes immediately, not one ECS frame later.
                for fx in &out[from..] {
                    if let Fx::Visible(ids, visible) = fx {
                        for u in w.iter_mut().filter(|u| ids.contains(&u.e)) {
                            u.visible = *visible;
                        }
                    }
                }
            }
        }
        self.rules = rules;
        self.init_done = true;
        self.mortar_bursts.clear();
        let first = |spec: Option<&str>| members(&w, spec?).into_iter().find(|u| u.alive).map(|u| u.e);
        self.cam.attach_e = first(self.cam.attach.as_ref().map(|a| a.0.as_str()));
        self.cam.focus_e = first(self.cam.focus_unit.as_deref());
        self.point_pos = self.point_on.as_deref().and_then(|sp| members(&w, sp).into_iter().find(|u| u.alive)).map(|u| u.pos);
        for (tower, towed) in std::mem::take(&mut self.tow_specs) {
            if let (Some(a), Some(bb)) = (ids(&w, &tower).first(), ids(&w, &towed).first()) {
                self.tows.push((*a, *bb));
            } else {
                self.tow_specs.push((tower, towed));
            }
        }
        (out, voice)
    }
}

/// A skirmish map's script without its end rules (those that name a winner or end the game): victory.rs decides how a
/// skirmish ends, from the same rules (`map.json` `victory`).
pub fn without_end_rules(data: &Value) -> Value {
    let mut data = data.clone();
    if let Some(rules) = data["rules"].as_array_mut() {
        rules.retain(|r| !r["actions"].as_array().into_iter().flatten().any(|a| matches!(s(a, "Type"), "End Game" | "Team X Wins" | "Remaining Team Wins")));
    }
    data
}

fn opening_rule(name: &str) -> bool {
    name.starts_with("Opening NIS") || name.starts_with("START Opening NIS")
}

pub fn locked(m: Option<&Mission>) -> bool {
    m.is_some_and(|m| m.script.nis || m.script.crew_screen || m.script.ended.is_some())
}
pub fn nis(m: Option<&Mission>) -> bool {
    m.is_some_and(|m| m.script.nis)
}
