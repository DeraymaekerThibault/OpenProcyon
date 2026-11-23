//! `Script`: the interpreter state and its world bookkeeping (docks, tows, flags, tasks, teams, speech).

use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};

use super::*;

/// Which event condition a count belongs to, with the names that tell its instances apart.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(in crate::rules) enum EventKey {
    Captured {
        team: String,
        group: String,
    },
    Destroyed {
        team: String,
        group: String,
    },
    /// Each unit of `group` entering `volume`, counted once.
    Once {
        group: String,
        volume: String,
    },
    Killed {
        player: String,
        kind: String,
    },
}

/// The units seen inside a volume so far, for the two "entered trigger volume" conditions (they keep separate sets).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(in crate::rules) enum Entered {
    AllOnce { group: String, volume: String },
    OncePerUnit { group: String, volume: String },
}

pub struct Script {
    pub(in crate::rules) rules: Vec<Rule>,
    /// Engine-added rule features (`eval/ext.rs`).
    pub(in crate::rules) ext: Ext,
    pub(in crate::rules) fired: Vec<bool>,
    /// Rules that fired at least once, repeating ones included (the autoplay report lists the others).
    pub(in crate::rules) ran: Vec<bool>,
    pub(in crate::rules) init_done: bool,
    /// Debug launch options (`--fire-rule`, `--start-at`, see `fire_start_rules`); consumed by the first step.
    pub start_rules: Vec<String>,
    pub start_at: Option<(f32, f32)>,
    /// The mission's opening NIS rules are played by briefing.rs instead (mission 1 only), so the interpreter skips them. Other
    /// missions (7, 8) have rules named "Opening NIS ..." of their own, which must run: mission 8's end its opening cutscene.
    pub briefing_opening: bool,
    pub time: f32,
    pub(in crate::rules) flags: HashMap<String, bool>,
    pub(in crate::rules) timers: HashMap<String, f32>,
    pub(in crate::rules) stopped: HashSet<String>,
    pub(in crate::rules) shots: HashMap<Entity, u32>,
    pub(in crate::rules) point_sets: Value,
    pub played: HashSet<String>,
    pub(in crate::rules) queue: VecDeque<String>,
    pub(in crate::rules) speaking: Option<(String, f32)>,
    pub subtitle: String,
    /// Text lines that started playing since the HUD last took them, with the speech event's text colour: the Chat History feed.
    pub started: Vec<(String, Option<[f32; 4]>)>,
    pub(in crate::rules) tasks: Vec<Task>,
    pub point: Option<usize>,
    /// Star map switches set by rules: the current objective point's visibility (PROVISIONAL default shown) and per-name map text visibility.
    pub point_on_map: bool,
    pub map_text: HashMap<String, bool>,
    /// Skirmish `Setup Team Objective`: team name id -> (current objective point name, active task names). A setup activates its task
    /// (older ones stay active), sets the point when the name exists in the map and clears it for "NO OBJECTIVE POINT"; any other
    /// unknown name leaves the point as it was (decoded).
    pub(in crate::rules) team_objectives: HashMap<String, (String, Vec<String>)>,
    pub(in crate::rules) hold_fire: HashSet<String>,
    pub(in crate::rules) hostile: HashSet<(String, String)>,
    pub(in crate::rules) player: String,
    pub(in crate::rules) tows: Vec<(Entity, Entity)>,
    pub(in crate::rules) tow_specs: Vec<(String, String)>,
    pub(in crate::rules) docked: HashSet<(Entity, Entity)>,
    pub(in crate::rules) dock_times: HashMap<Entity, f32>,
    pub(in crate::rules) cam: Cam,
    pub nis: bool,
    pub ended: Option<(String, bool)>,
    /// A skirmish script that decides its own game (`"victory": "script"`): set by `SkirmishScript::load`, which keeps the end rules.
    pub(in crate::rules) skirmish_decides: bool,
    /// What the script ended the game with (only while `skirmish_decides`); victory.rs reads it.
    pub end: Option<ScriptEnd>,
    /// The rule that set `end` and whether the step it was set in is over: later rules do not change it.
    pub(in crate::rules) end_by: usize,
    pub(in crate::rules) end_sealed: bool,
    /// 'Open/Close HUD Texture Overlay': the full-screen texture (assets/hud/overlay) the views draw, e.g. the end credits.
    pub overlay: Option<String>,
    pub score: u32,
    pub(in crate::rules) music: Vec<MusicCmd>,
    pub(in crate::rules) media: Value,
    pub(in crate::rules) paths: Value,
    pub(in crate::rules) points: Value,
    /// The mission's placed scenery (`objects`: islands, bases, rocks), for the console's `list objects`.
    pub(in crate::rules) objects: Value,
    pub(in crate::rules) volumes: HashMap<String, Region>,
    pub(in crate::rules) nebulae: Vec<Region>,
    pub(in crate::rules) hits: HashMap<Entity, u32>,
    pub(in crate::rules) hits_by: HashMap<(String, Entity), u32>,
    pub(in crate::rules) fired_at: HashMap<Entity, (f32, Entity)>,
    pub(in crate::rules) prev_inside: HashMap<(String, usize), bool>,
    /// 'All Units from Group have entered trigger volume Once': members seen inside, per group|volume.
    pub(in crate::rules) entered: HashMap<Entered, HashSet<Entity>>,
    /// Every asteroid seen, with the group names it answered to (a destroyed rock leaves the unit list but still counts as attacked).
    pub(in crate::rules) belt: HashMap<Entity, Vec<String>>,
    /// TWT named groups by world object id (a unit also answers to its current owner, `matches`).
    pub(in crate::rules) groups: HashMap<i64, Vec<String>>,
    pub(in crate::rules) group_names: HashSet<String>,
    /// Lifeboats saved as the original tallies them: (collecting player, lifeboat owner, count). The only source of the rules'
    /// capture counts apart from `base_captures`; boarding a ship never touches it.
    pub(in crate::rules) lifeboats_saved: Vec<(String, String, u32)>,
    /// Owner of every lifeboat in the world, refreshed by lifeboats.rs each frame (`Player has no lifeboats`).
    pub(in crate::rules) lifeboat_owners: Vec<String>,
    /// PROVISIONAL bridge, bases only: boardings of a base by (capturing player, base's previous player). The original's capture count has no
    /// writer but lifeboat pickup, yet mission 12's defeat reads 'Procs vs RoyalPalace' (the palace is a boardable base), so it is kept alive.
    /// Ship boardings are not counted here: mission 6's 'PlayerStart vs PirateRunner' is the lifeboat route, its boarding route is 'Team has captured'.
    pub(in crate::rules) base_captures: HashMap<(String, String), u32>,
    /// Units captured by boarding (boarding.rs) and the capturing player, for 'Team has captured a ship'.
    pub(in crate::rules) captured: Vec<(String, Entity)>,
    /// Ships destroyed and the player who destroyed them, for 'Team has destroyed a ship'.
    pub(in crate::rules) destroyed: Vec<(String, Entity)>,
    /// Ships that were destroyed, as they last stood: a sunk ship is removed from the world a tick after its break-up, but 'Group
    /// Destroyed' and the kill counts still answer for it (`keep_sunk`).
    pub(in crate::rules) sunk: Vec<Unit>,
    /// Skirmish teams: team name id -> its players (`set_teams`). Empty in the campaign, where a team condition counts anyone.
    pub(in crate::rules) teams: HashMap<String, Vec<String>>,
    /// 'Grant Team X Points' totals per team name id ('Team has X points').
    pub team_points: HashMap<String, i32>,
    /// Event conditions (kills, captures, per-unit volume entries): events seen and events already reported, per condition key.
    /// PROVISIONAL: a rule that fires reports one pending event, so a repeating rule fires once per event.
    pub(in crate::rules) events: HashMap<EventKey, (u32, u32)>,
    /// Event keys the rule being evaluated would report if all its conditions hold.
    pub(in crate::rules) consume: Vec<EventKey>,
    /// The TWT's players (the capture conditions credit a captor's allies too).
    pub(in crate::rules) players: Vec<String>,
    /// 'Set Current Objective Point On Ship': the objective marker follows this unit (`point_pos`).
    pub(in crate::rules) point_on: Option<String>,
    pub point_pos: Option<Vec3>,
    pub(in crate::rules) kills: HashMap<(String, bool), u32>,
    pub hint: String,
    pub(in crate::rules) warned: HashSet<String>,
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
    pub(in crate::rules) mortar_seen: HashSet<String>,
    /// 'Open Crew and Arms Screens': the front end shows the crew and arms screen once (menu.rs clears it).
    pub crew_screen: bool,
    /// Rule trace and start-of-mission audit (diag.rs).
    pub(in crate::rules) diag: Diag,
}

impl Script {
    fn named_group(&self, name: &str) -> bool {
        self.group_names.contains(name)
    }
    /// Remembers the destroyed ships of `w` and puts back, as destroyed units, those no longer in the world (removed after their break-up).
    pub fn keep_sunk(&mut self, w: &mut Vec<Unit>) {
        for u in w.iter().filter(|u| !u.alive) {
            match self.sunk.iter_mut().find(|k| k.e == u.e) {
                Some(k) => *k = u.clone(),
                None => self.sunk.push(u.clone()),
            }
        }
        let gone: Vec<Unit> = self.sunk.iter().filter(|k| !w.iter().any(|u| u.e == k.e)).cloned().collect();
        w.extend(gone);
    }
    /// Adds TWT groups and resolves owner/group name collisions for rule selectors.
    pub fn tag(&self, w: &mut [Unit]) {
        for u in w {
            u.owner_is_named_group = self.named_group(&u.owner);
            if let Some(g) = self.groups.get(&u.id) {
                u.groups.extend(g.iter().cloned());
            }
            // "Player0 Group" (aliased to the human's name) is the human's fleet: every ship the human owns, even where a TWT group
            // of the same name shadows the owner. Mission 5's TWT group "Player" is the starting Sea Sparrow and Comet; a rule later hands
            // the RLS Superior (TWT group "Jim") to the player as fleet primary. Without this its warning shot never counted.
            if u.owner == self.player && !u.groups.contains(&self.player) {
                u.groups.push(self.player.clone());
            }
        }
    }
    /// Whether a unit (TWT id, owner, name; `primary` for the human's command ship) answers to a rule spec, as `matches` does.
    pub fn answers(&self, id: i64, owner: &str, name: &str, primary: bool, spec: &str) -> bool {
        match spec.split_once(',') {
            Some((_, unit)) => name == unit || (unit == "HUMAN CONTROLLED COMMAND SHIP" && primary),
            None => ((!self.named_group(owner) || owner == self.player) && owner == spec) || self.groups.get(&id).is_some_and(|g| g.iter().any(|g| g == spec)),
        }
    }
    /// Talking-head sprite of the line being spoken: the TWT speech event's FaceTexture (media `face`), if exported.
    pub(in crate::rules) fn face(&self) -> Option<&str> {
        self.media["speech"][self.speaking.as_ref()?.0.as_str()]["face"].as_str()
    }
    /// Subtitle colour (RGBA) of the line being spoken: the TWT speech event's 'Text Color' (media `color`), if exported.
    pub fn speech_colour(&self) -> Option<[f32; 4]> {
        let c = self.media["speech"][self.speaking.as_ref()?.0.as_str()]["color"].as_array()?;
        Some(std::array::from_fn(|i| c.get(i).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32))
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
    /// Whether `docker` is registered as docked with `dockee` (what the rules' docked condition reads).
    pub fn is_docked(&self, docker: Entity, dockee: Entity) -> bool {
        self.docked.contains(&(docker, dockee))
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
    /// Ends the tow joining two ships, whichever of them is the tower: a dock release removes any link between docker and dockee,
    /// so docking at one's own tower (or tower docking at its towee) ends the tow.
    pub fn break_tow_between(&mut self, a: Entity, b: Entity) {
        self.tows.retain(|t| *t != (a, b) && *t != (b, a));
    }
    /// Breaks every link `tower` has (the menu's Tow stop).
    pub fn break_tows(&mut self, tower: Entity) {
        self.tows.retain(|t| t.0 != tower);
    }
    /// Drops every tow link `ship` is in, as tower or towee (a won boarding fight).
    pub fn drop_tows_of(&mut self, ship: Entity) {
        self.tows.retain(|t| t.0 != ship && t.1 != ship);
    }
    pub fn dock_time(&self, docker: Entity) -> Option<f32> {
        self.dock_times.get(&docker).copied()
    }
    /// The dock routine stores the longer of the dock time and the repair time back, so the ship's later dockings keep it.
    pub fn keep_dock_time(&mut self, docker: Entity, secs: f32) {
        self.dock_times.insert(docker, secs);
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
    /// A skirmish team's objective as the star map shows it: the objective point's game position (none for 'NO OBJECTIVE POINT') and
    /// the task's text.
    pub fn team_objective(&self, team: &str) -> Option<(Option<Value>, Option<String>)> {
        let (point, tasks) = self.team_objectives.get(team)?;
        let position = self.points.as_array().and_then(|p| p.iter().find(|x| x["Name"] == point.as_str())).map(|x| x["Position"].clone());
        let texts: Vec<&str> = tasks.iter().filter_map(|n| self.tasks.iter().find(|t| &t.name == n)).map(|t| t.text.as_str()).filter(|t| !t.is_empty()).collect();
        let text = (!texts.is_empty()).then(|| texts.join("\n"));
        Some((position, text))
    }
    /// The skirmish team (name id) `player` plays for.
    pub fn team_of(&self, player: &str) -> Option<&str> {
        self.teams.iter().find(|(_, p)| p.iter().any(|x| x == player)).map(|(n, _)| n.as_str())
    }
    /// Skirmish teams: (team name id, its players).
    pub fn set_teams(&mut self, teams: Vec<(String, Vec<String>)>) {
        self.teams = teams.into_iter().collect();
    }
    /// Whether `player` plays for `team`; true for any player when the team is unknown or not given (campaign rules).
    pub(in crate::rules) fn in_team(&self, team: &str, player: &str) -> bool {
        team.is_empty() || self.teams.get(team).is_none_or(|p| p.iter().any(|x| x == player))
    }
    /// An event condition: `seen` events so far under `key`; true while some are unreported (more than `quota`), queuing one
    /// to be reported if the rule fires.
    pub(in crate::rules) fn event(&mut self, key: EventKey, seen: u32, quota: f32) -> bool {
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
    pub(in crate::rules) fn warn(&mut self, what: &str) {
        if self.warned.insert(what.into()) {
            warn!("mission rules: unsupported {what} (ignored)");
        }
    }
    pub(in crate::rules) fn volume(&self, name: &str) -> &Region {
        self.volumes.get(name).unwrap_or(&NO_VOLUME)
    }
    pub(in crate::rules) fn timer(&self, name: &str) -> Option<f32> {
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
        self.rules.iter().zip(&self.ran).filter(|(r, ran)| !**ran && !(self.briefing_opening && opening_rule(&r.name))).map(|(r, _)| r.name.trim()).collect()
    }
    /// What a rule that has not fired yet waits for the player's ship to dock with ("Group/Unit Docked" with the player among the dockers),
    /// minus the pairs already docked: the dev autopilot's dock targets (missions 5, 7, 8 dock with bases their tasks only name loosely).
    pub fn dock_targets(&self, w: &[Unit]) -> Vec<Entity> {
        let Some(p) = w.iter().find(|u| u.primary && u.alive) else { return vec![] };
        let mut out = vec![];
        for (r, _) in self.rules.iter().zip(&self.fired).filter(|(_, fired)| !**fired) {
            // A dock behind an unmet flag/timer/speech gate is not wanted yet (mission 5's Elzrand waits for the warning shot; docking
            // there early just docks and undocks forever).
            if r.conds.iter().any(|c| self.gate(c) == Some(false)) {
                continue;
            }
            for c in r.conds.iter().filter(|c| matches!(s(c, "Type"), "Group/Unit Docked" | "Group/Unit Is Docked")) {
                if !members(w, s(c, "Dockers Group/Unit Name")).iter().any(|u| u.e == p.e) {
                    continue;
                }
                for t in members(w, s(c, "Targets Group/Unit Name")) {
                    if t.e != p.e && !self.docked.contains(&(p.e, t.e)) && !out.contains(&t.e) {
                        out.push(t.e);
                    }
                }
            }
        }
        out
    }
    /// What the unfired rules wait for the player to do, for the dev autopilot (`--script mission`) when the objective point does not
    /// say (mission 4's "Search the Area Marked Red", mission 5's warning shot). A rule counts once its flag, timer and speech gates
    /// hold (other conditions are not checked); rules that only warn or fail the player are left out. PROVISIONAL heuristic.
    pub fn wants(&self, w: &[Unit]) -> Vec<Want> {
        let Some(p) = w.iter().find(|u| u.primary && u.alive) else { return vec![] };
        let mut out = vec![];
        for (r, _) in self.rules.iter().zip(&self.fired).filter(|(_, fired)| !**fired) {
            if (self.briefing_opening && opening_rule(&r.name)) || !self.worth_wanting(r) {
                continue;
            }
            if r.conds.iter().any(|c| self.gate(c) == Some(false)) {
                continue;
            }
            let player_in = |spec: &str| members(w, spec).iter().any(|u| u.e == p.e);
            for c in &r.conds {
                match s(c, "Type") {
                    "Enter Volume" | "Is Group In Volume" if player_in(s(c, "Group Name")) => {
                        // The nearest point inside, not the centroid: mission 11's "Pursue Silver 1" is a bent band whose centroid lies
                        // outside it, and the bot circled there.
                        let vol = self.volume(s(c, "Volume Name"));
                        if let Some(m) = vol.near_inside(Vec2::new(p.pos.x, -p.pos.z)).filter(|_| !in_volume(vol, p.pos)) {
                            out.push(Want::Reach(Vec3::new(m.x, 0.0, -m.y)));
                        }
                    }
                    // Mission 4's stuck sloop and mission 6's PirateBigBrother are spotted by closing in.
                    "Group to Group Distance" if s(c, "Equivalence") == "Less Than" && player_in(s(c, "GroupA Name")) => {
                        if let Some(u) = members(w, s(c, "GroupB Name")).into_iter().filter(|u| u.alive && u.visible && u.e != p.e).nearest_to(p.pos, |u| u.pos)
                            && u.pos.distance(p.pos) >= f(c, "Distance")
                        {
                            out.push(Want::Reach(u.pos));
                        }
                    }
                    // Mission 5's warning shot: firing on the blockading fishing boats.
                    "Is Group A attacking Group B" if player_in(s(c, "GroupA Name")) => {
                        out.extend(members(w, s(c, "GroupB Name")).iter().filter(|u| u.alive && u.visible && u.owner != p.owner).map(|u| Want::Hit(u.e)));
                    }
                    "Is Ship In Tow" if b(c, "Boolean") && player_in(s(c, "Towing Group")) => {
                        let towed = |e: Entity| self.tows.iter().any(|t| t.0 == p.e && t.1 == e);
                        out.extend(members(w, s(c, "Target to Tow")).iter().filter(|u| u.alive && u.e != p.e && !towed(u.e)).map(|u| Want::Tow(u.e)));
                    }
                    "Exit Volume" => {
                        let vol = self.volume(s(c, "Volume Name"));
                        // Only a ship the player tows: the player's own ship starts inside big area volumes (mission 2), and
                        // running from their centre took the bot off its objective.
                        let mine = |u: &Unit| self.tows.iter().any(|t| t.0 == p.e && t.1 == u.e);
                        if members(w, s(c, "Group Name")).iter().any(|u| mine(u) && u.alive && in_volume(vol, u.pos))
                            && let Some(m) = vol.centroid()
                        {
                            out.push(Want::Leave(Vec3::new(m.x, 0.0, -m.y)));
                        }
                    }
                    // The part of the volume nearest the player: mission 9's StarHammer cannot move, and the Dark Matter's centre lies
                    // beyond the Star Mortar's 350 reach.
                    "Mission 9 - If Mortar explodes within Area" => {
                        // The nearest point first, then a grid across the volume: the wreck's one Nova Mortar covers only 15
                        // degrees dead ahead, so the bot takes the first point a mount can reach.
                        let vol = self.volume(s(c, "Volume Name"));
                        let at = |m: Vec2| Want::Lob(Vec3::new(m.x, 0.0, -m.y));
                        out.extend(vol.near_inside(Vec2::new(p.pos.x, -p.pos.z)).map(at));
                        out.extend(vol.grid(8).into_iter().map(at));
                    }
                    "Player has hit Group/Unit at least x Times" | "Group/Unit Hit at least x Times by Player ( with eqivalence )" if base(s(c, "Player Name")) == p.owner => {
                        out.extend(members(w, s(c, "Group/Unit Name")).iter().filter(|u| u.alive && u.visible && u.owner != p.owner).map(|u| Want::Hit(u.e)));
                    }
                    "Group Destroyed" | "Group/Unit Vital Section has >,<,= X damage" | "Group/Unit has >,<,= X damage" => {
                        let spec = if s(c, "Type") == "Group Destroyed" { s(c, "Group Name") } else { s(c, "Group/Unit Name") };
                        out.extend(members(w, spec).iter().filter(|u| u.alive && u.visible && self.hostile(&u.owner, &p.owner)).map(|u| Want::Hit(u.e)));
                    }
                    _ => {}
                }
            }
        }
        out
    }
    /// A condition the rules decide without the world: Some(holds) for flag, timer and speech gates, None for the rest.
    fn gate(&self, c: &Value) -> Option<bool> {
        Some(match s(c, "Type") {
            "World Initialize" => false,
            "Flag Condition" => self.flags.get(s(c, "Flag Name")).copied().unwrap_or(false) == b(c, "Boolean Value"),
            "Timer Condition" => self.timer(s(c, "Timer Name")).is_some_and(|v| cmp(s(c, "Equivalence"), v, f(c, "Time in seconds"))),
            "Speech Event Played Once" => self.played.contains(s(c, "Speech Event Name")),
            "Speech Event Not Played Yet" => !self.played.contains(s(c, "Speech Event Name")),
            _ => return None,
        })
    }
    /// A rule that moves the story on: it changes state (flags, objectives, paths, timers, the level) and neither ends the game nor
    /// only warns ("Player Goes Wrong Way", "Out Of Bounds", "FAIL Mission").
    fn worth_wanting(&self, r: &Rule) -> bool {
        let name = r.name.to_ascii_lowercase();
        let bad = ["fail", "wrong", "bounds", "dead", "punish", "too long", "ignores", "death", "lose", "lost"];
        if bad.iter().any(|x| name.contains(x)) || r.acts.iter().any(|a| s(a, "Type") == "End Game") {
            return false;
        }
        r.acts.iter().any(|a| {
            let t = s(a, "Type");
            !(t.starts_with("Play ") || t.starts_with("NIS") || t.starts_with("Focus Camera") || t.contains("Music") || t.contains("Victory Points"))
        })
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
    pub(in crate::rules) fn skip_speech(&mut self) -> bool {
        self.speaking.as_mut().map(|x| x.1 = 0.0).is_some()
    }

    pub(in crate::rules) fn speech_tick(&mut self, dt: f32) -> Option<String> {
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
            let dur = m["duration"].as_f64().map_or((text.len() as f32 / 14.0).max(2.0), |d| d as f32 + 0.3);
            self.subtitle = text.into();
            self.speaking = Some((name.clone(), dur));
            if !text.is_empty() {
                let colour = self.speech_colour();
                self.started.push((text.into(), colour));
            }
            // The sound file of the TWT speech event (media `file`); mission 1 names its events after their files.
            return m["duration"].as_f64().map(|_| m["file"].as_str().map_or(name, String::from));
        }
        None
    }

    pub(super) fn group_in_volume(&mut self, c: &Value, w: &[Unit], entering: bool, repeating: bool) -> bool {
        let key = (s(c, "Group Name").to_string() + "|" + s(c, "Volume Name"), 0);
        let vol = self.volume(s(c, "Volume Name"));
        let m: Vec<_> = members(w, s(c, "Group Name")).into_iter().filter(|u| u.alive && u.visible).collect();
        let all = !entering && b(c, "Entire Group");
        let now = !m.is_empty() && if all { m.iter().all(|u| in_volume(vol, u.pos)) } else { m.iter().any(|u| in_volume(vol, u.pos)) };
        // Hidden placement/reveal is not entry: track physical membership before applying visibility.
        let inside = members(w, s(c, "Group Name")).into_iter().any(|u| u.alive && in_volume(vol, u.pos));
        let armed = self.prev_inside.entry((key.0.clone(), 2)).or_insert(now);
        *armed |= !inside;
        let entered = *armed;
        let was = self.prev_inside.insert(key, inside).unwrap_or(false);
        if entering { now && if repeating { !was } else { entered } } else { now }
    }

    pub(super) fn group_exited_volume(&mut self, c: &Value, w: &[Unit]) -> bool {
        let key = (s(c, "Group Name").to_string() + "|" + s(c, "Volume Name"), 1);
        let vol = self.volume(s(c, "Volume Name"));
        let m: Vec<_> = members(w, s(c, "Group Name")).into_iter().filter(|u| u.alive && u.visible).collect();
        let now = !m.is_empty() && m.iter().any(|u| in_volume(vol, u.pos));
        let was = self.prev_inside.insert(key, now).unwrap_or(false);
        was && !now
    }
}

#[cfg(test)]
mod event_key_tests {
    use super::{Entered, EventKey};
    #[test]
    fn event_keys_tell_conditions_and_names_apart() {
        let (g, v) = ("Group".to_string(), "Vol".to_string());
        let once = EventKey::Once { group: g.clone(), volume: v.clone() };
        assert_eq!(once, EventKey::Once { group: g.clone(), volume: v.clone() });
        assert_ne!(once, EventKey::Captured { team: g.clone(), group: v.clone() });
        assert_ne!(EventKey::Captured { team: "T".into(), group: g.clone() }, EventKey::Destroyed { team: "T".into(), group: g.clone() });
        assert_ne!(Entered::AllOnce { group: g.clone(), volume: v.clone() }, Entered::OncePerUnit { group: g, volume: v });
    }
}
