//! Engine-added rule features (not in the retail TWT vocabulary; documented in `docs/modding.md`). Every Type is prefixed `OP ` so it
//! can never clash with a retail name, and a mission that uses none of it behaves exactly as before.
//!
//! * Conditions: `OP Any Of`, `OP Not`, `OP Counter Condition`, `OP Group Is Cloaked`, `OP Group Speed`, `OP Task State`.
//! * Actions: `OP Wait`, `OP Wait Until Speech Done` (sequences), `OP Set Counter`, `OP Add To Counter`, `OP Set Rule Active`,
//!   `OP Board Ship`, `OP Abandon Ship`, `OP Teleport`, `OP Follow Group`, `OP Place Mines`, `OP Etherium Current`,
//!   `OP Set Campaign Counter`, `OP Add To Campaign Counter`, `OP Fade`; condition `OP Campaign Counter Condition`.
//! * Rule field `"Is Active"` (a rule with `false` is not evaluated), task keys `complete_when` / `fail_when` / `required`, and the
//!   mission key `"victory": "objectives"`.

use serde_json::Value;

use super::*;

/// What a sequence waits for.
enum Wait {
    /// Seconds left; counts down only while the opening-briefing pause is off, as the timers do.
    Secs(f32),
    /// A speech event to be neither queued nor playing; the empty name waits for the whole speech queue to be empty.
    Speech(String),
}

/// A rule whose action list stopped at a wait: the cursor is the index of the next action to run.
struct Pending {
    rule: usize,
    next: usize,
    wait: Wait,
}

/// A task's self-completion keys (`mission.json` `tasks`).
struct TaskRule {
    name: String,
    complete_when: Vec<Value>,
    fail_when: Vec<Value>,
    required: bool,
    /// The skirmish team the task belongs to (empty: none).
    team: String,
}

/// The interpreter state of the engine-added features.
#[derive(Default)]
pub(in crate::rules) struct Ext {
    counters: HashMap<String, f32>,
    /// `OP Set Campaign Counter` and friends: loaded from the campaign when the mission starts, handed on when it is won (campaign.rs).
    campaign: HashMap<String, f32>,
    campaign_committed: bool,
    /// `OP Fade`: (seconds in all, seconds so far). The view draws it (`Script::fade_alpha`); nothing in the simulation reads it.
    fade: Option<(f32, f32)>,
    /// Per rule (same order as `Script::rules`): evaluated at all? From the rule's `Is Active`, changed by `OP Set Rule Active`.
    active: Vec<bool>,
    names: Vec<String>,
    pending: Vec<Pending>,
    tasks: Vec<TaskRule>,
    /// `"victory": "objectives"`.
    objectives_decide: bool,
    /// `victory_message` / `defeat_message`: ids into the media `messages` (as `End Game` reads its loser message).
    victory_message: String,
    defeat_message: String,
}

/// A JSON boolean or a retail-style "TRUE"/"FALSE" string; `None` when the value is neither.
fn flag(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(x) => Some(*x),
        Value::String(x) if x.eq_ignore_ascii_case("true") => Some(true),
        Value::String(x) if x.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

impl Ext {
    pub(in crate::rules) fn new(data: &Value, player: &str) -> Ext {
        let rules = data["rules"].as_array().map(Vec::as_slice).unwrap_or_default();
        let conds = |t: &Value, k: &str| t[k].as_array().into_iter().flatten().map(|c| alias_player(c.clone(), player)).collect::<Vec<_>>();
        Ext {
            counters: HashMap::new(),
            campaign: HashMap::new(),
            campaign_committed: false,
            fade: None,
            // Every retail rule says true; a rule without the field is active.
            active: rules.iter().map(|r| flag(&r["Is Active"]).unwrap_or(true)).collect(),
            names: rules.iter().map(|r| s(r, "name").trim().to_string()).collect(),
            pending: vec![],
            tasks: data["tasks"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|t| TaskRule {
                    name: s(t, "Name").into(),
                    complete_when: conds(t, "complete_when"),
                    fail_when: conds(t, "fail_when"),
                    required: flag(&t["required"]) == Some(true),
                    team: s(t, "team").into(),
                })
                .collect(),
            objectives_decide: data["victory"].as_str() == Some("objectives"),
            victory_message: s(data, "victory_message").into(),
            defeat_message: s(data, "defeat_message").into(),
        }
    }
    pub(in crate::rules) fn is_active(&self, rule: usize) -> bool {
        self.active.get(rule).copied().unwrap_or(true)
    }
    /// Whether the rule's action list is waiting; a firing of it is skipped meanwhile (not queued).
    pub(in crate::rules) fn busy(&self, rule: usize) -> bool {
        self.pending.iter().any(|p| p.rule == rule)
    }
    /// For the rule trace: what the rule's pending sequence waits for.
    pub(in crate::rules) fn busy_note(&self, rule: usize) -> String {
        let Some(p) = self.pending.iter().find(|p| p.rule == rule) else { return "not waiting".into() };
        let what = match &p.wait {
            Wait::Secs(_) => "an OP Wait to end".to_string(),
            Wait::Speech(n) if n.is_empty() => "the speech queue to empty (OP Wait Until Speech Done)".to_string(),
            Wait::Speech(n) => format!("speech '{n}' to finish (OP Wait Until Speech Done)"),
        };
        format!("busy: its sequence waits for {what} before action #{}; the conditions hold again but this firing is skipped", p.next + 1)
    }
    /// Every counter that has been set, by name (the console's `counters`).
    pub(in crate::rules) fn counter_list(&self) -> Vec<(&str, f32)> {
        let mut v: Vec<_> = self.counters.iter().map(|(k, x)| (k.as_str(), *x)).collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        v
    }
    pub fn counter(&self, name: &str) -> f32 {
        self.counters.get(name).copied().unwrap_or(0.0)
    }
}

impl Script {
    /// `OP ...` conditions. An unknown one warns once and is false, like any unknown condition.
    pub(in crate::rules) fn ext_cond(&mut self, c: &Value, w: &[Unit], repeating: bool) -> bool {
        match s(c, "Type") {
            // Every listed condition is evaluated (no short-circuit), so an edge condition inside (Enter Volume, event quotas) advances
            // its state exactly as it would at the top level.
            "OP Any Of" => {
                let mut any = false;
                for sub in c["Conditions"].as_array().into_iter().flatten() {
                    any |= self.cond(sub, w, repeating);
                }
                any
            }
            // Inverts what the inner condition answers this frame. An edge condition (Enter Volume in a repeating rule, an event quota)
            // is true for one frame only, so `Not` of it is true on every other frame; use a level condition or `Exit Volume` instead.
            // An event the inner condition would have reported is never consumed (the rule cannot fire while the inner is true).
            "OP Not" => {
                if !c["Condition"].is_object() {
                    self.warn("'OP Not' without a Condition object");
                    return false;
                }
                let mark = self.consume.len();
                let inner = self.cond(&c["Condition"], w, repeating);
                self.consume.truncate(mark);
                !inner
            }
            "OP Counter Condition" => cmp(s(c, "Equivalence"), self.ext.counter(s(c, "Counter Name")), f(c, "Value")),
            "OP Campaign Counter Condition" => cmp(s(c, "Equivalence"), self.ext.campaign.get(s(c, "Counter Name")).copied().unwrap_or(0.0), f(c, "Value")),
            // `Boolean` TRUE (the default): every live member is fully cloaked; FALSE: none is. A group with no live member holds neither.
            "OP Group Is Cloaked" => {
                let m: Vec<&Unit> = members(w, s(c, "Group Name")).into_iter().filter(|u| u.alive).collect();
                let on = c["Boolean"].is_null() || b(c, "Boolean");
                !m.is_empty() && m.iter().all(|u| u.cloaked == on)
            }
            // The fastest or slowest of the live ships' own speeds (units per second, along their headings) against `Speed`; a group with
            // no live ship holds nothing.
            "OP Group Speed" => {
                let speeds = members(w, s(c, "Group Name")).into_iter().filter(|u| u.alive).filter_map(|u| u.speed);
                let pick = if s(c, "Which") == "Slowest" { speeds.reduce(f32::min) } else { speeds.reduce(f32::max) };
                pick.is_some_and(|v| cmp(s(c, "Equivalence"), v, f(c, "Speed")))
            }
            // Active: switched on and neither complete nor failed yet. Complete and Failed are the task's flags, however they were set.
            "OP Task State" => self.tasks.iter().find(|t| t.name == s(c, "Objective Task")).is_some_and(|t| match s(c, "State") {
                "Active" => t.active && !t.complete && !t.failed,
                "Complete" => t.complete,
                "Failed" => t.failed,
                _ => false,
            }),
            other => {
                self.warn(&format!("condition '{other}'"));
                false
            }
        }
    }

    /// `OP ...` actions that are not waits (a wait reached outside a rule's action list, e.g. through `act` alone, does nothing).
    pub(in crate::rules) fn ext_act(&mut self, a: &Value, w: &[Unit], out: &mut Vec<Fx>, rule: &str) {
        match s(a, "Type") {
            // The same grapple-then-board a player's orders make (boarding.rs): the ships of `Group Name` take the nearest live ship of
            // `Target Group`.
            "OP Board Ship" => out.push(Fx::Board(ids(w, s(a, "Group Name")), ids(w, s(a, "Target Group")))),
            // Each member keeps its place in the group's current layout: the group's centre goes to the point, and with a `Heading` the
            // whole layout turns with the group's first ship (rotated about the centre by the change of its heading).
            "OP Teleport" => {
                let (set, group) = (s(a, "Point Set Name"), s(a, "Group/Unit Name"));
                let index = f(a, "Point Index").max(0.0) as usize;
                let Some(p) = self.point_sets[set].get(index) else {
                    self.warn(&format!("point {index} of point set '{set}' in 'OP Teleport' of rule '{rule}'"));
                    return;
                };
                let dest = game_to_bevy(&p["pos"]);
                let all = members(w, group);
                let live: Vec<&Unit> = all.iter().copied().filter(|u| u.alive).collect();
                let anchor = if live.is_empty() { &all } else { &live };
                if anchor.is_empty() {
                    return;
                }
                let centre = anchor.iter().map(|u| u.pos).sum::<Vec3>() / anchor.len() as f32;
                let heading = (!a["Heading"].is_null()).then(|| -f(a, "Heading").to_radians());
                let turn = heading.map_or(0.0, |h| h - anchor.iter().find(|u| u.speed.is_some()).map_or(h, |u| u.heading));
                let spin = Quat::from_rotation_y(turn);
                out.push(Fx::Place { units: all.iter().map(|u| (u.e, dest + spin * (u.pos - centre))).collect(), heading });
            }
            // The current on a mission path, made when none runs there yet. A setting left out stays as it is (a new current starts at the
            // retail strength 1, 120 wide, on).
            "OP Etherium Current" => match self.paths[s(a, "Etherium Path")].as_array() {
                Some(p) => out.push(Fx::CurrentSet {
                    name: s(a, "Etherium Path").into(),
                    path: p.iter().map(game_to_bevy).collect(),
                    strength: a["Strength"].as_f64().map(|v| v as f32),
                    width: a["Width"].as_f64().map(|v| v as f32),
                    on: (!a["Boolean"].is_null()).then(|| b(a, "Boolean")),
                }),
                None => self.warn(&format!("path '{}' in 'OP Etherium Current' of rule '{rule}'", s(a, "Etherium Path"))),
            },
            // Presentation only: the picture goes to black and back over `Seconds` (half each way); a new fade replaces one that runs.
            "OP Fade" => self.ext.fade = Some((f(a, "Seconds"), 0.0)).filter(|f| f.0 > 0.0),
            "OP Place Mines" => {
                let bullet = Some(s(a, "Bullet")).filter(|b| !b.is_empty()).unwrap_or("Bullet_StarMine_Med");
                out.push(Fx::Mines { set: s(a, "Point Set Name").into(), bullet: bullet.into(), owner: s(a, "Player/Owner").into() });
            }
            "OP Follow Group" => match Station::parse(s(a, "Station")) {
                Some(station) => out.push(Fx::Follow { units: ids(w, s(a, "Group Name")), group: s(a, "Target Group").into(), station, distance: f(a, "Distance").max(0.0) }),
                None => self.warn(&format!("station '{}' in 'OP Follow Group' of rule '{rule}'", s(a, "Station"))),
            },
            // The crews take to the boats as on a death by damage (lifeboats.rs) and the hulls stay afloat, disabled.
            "OP Abandon Ship" => out.push(Fx::Abandon(ids(w, s(a, "Group Name")))),
            "OP Set Counter" => {
                self.ext.counters.insert(s(a, "Counter Name").into(), f(a, "Value"));
            }
            "OP Add To Counter" => *self.ext.counters.entry(s(a, "Counter Name").into()).or_default() += f(a, "Value"),
            "OP Set Campaign Counter" => {
                self.ext.campaign.insert(s(a, "Counter Name").into(), f(a, "Value"));
            }
            "OP Add To Campaign Counter" => *self.ext.campaign.entry(s(a, "Counter Name").into()).or_default() += f(a, "Value"),
            "OP Set Rule Active" => {
                let (name, on) = (s(a, "Rule Name").trim(), b(a, "Boolean"));
                let mut found = false;
                for (i, n) in self.ext.names.iter().enumerate() {
                    if n == name {
                        self.ext.active[i] = on;
                        if !on {
                            // A deactivated rule stops its waiting sequence: a deactivated cutscene does not finish.
                            self.ext.pending.retain(|p| p.rule != i);
                        }
                        found = true;
                    }
                }
                if !found {
                    self.warn(&format!("rule '{name}' in 'OP Set Rule Active' of rule '{rule}'"));
                }
            }
            "OP Wait" | "OP Wait Until Speech Done" => {}
            other => self.warn(&format!("action '{other}' in rule '{rule}'")),
        }
    }

    fn wait_of(a: &Value) -> Option<Wait> {
        match s(a, "Type") {
            "OP Wait" => Some(Wait::Secs(f(a, "Seconds"))),
            "OP Wait Until Speech Done" => Some(Wait::Speech(s(a, "Speech Event Name").into())),
            _ => None,
        }
    }

    fn wait_over(&self, wait: &Wait) -> bool {
        match wait {
            Wait::Secs(t) => *t <= 0.0,
            Wait::Speech(n) if n.is_empty() => self.queue.is_empty() && self.speaking.is_none(),
            Wait::Speech(n) => !self.queue.iter().any(|q| q == n) && self.speaking.as_ref().is_none_or(|x| x.0 != *n),
        }
    }

    /// Runs the actions of rule `i` from index `from` in order, stopping at a wait that is not over yet: the rest resumes in a later
    /// `step` (`resume_sequences`). Later rules must see visibility changes immediately, not one ECS frame later.
    pub(in crate::rules) fn run_from(&mut self, i: usize, r: &Rule, from: usize, w: &mut [Unit], out: &mut Vec<Fx>) {
        let was_active = self.ext.is_active(i);
        for (k, a) in r.acts.iter().enumerate().skip(from) {
            if let Some(wait) = Self::wait_of(a) {
                if !self.wait_over(&wait) {
                    self.ext.pending.push(Pending { rule: i, next: k + 1, wait });
                    return;
                }
                continue;
            }
            let at = out.len();
            self.act(a, w, out, &r.name);
            if was_active && !self.ext.is_active(i) {
                return; // the rule switched itself off: the rest of its sequence is dropped
            }
            for fx in &out[at..] {
                if let Fx::Visible(ids, visible) = fx {
                    for u in w.iter_mut().filter(|u| ids.contains(&u.e)) {
                        u.visible = *visible;
                    }
                }
            }
        }
    }

    /// Counts the waits down and continues the sequences whose wait is over. Time waits do not run during the opening-briefing pause
    /// (the same pause the timers have); speech waits follow the speech queue, which is also paused then.
    pub(in crate::rules) fn resume_sequences(&mut self, rules: &[Rule], w: &mut [Unit], out: &mut Vec<Fx>, dt: f32, opening: bool) {
        for mut p in std::mem::take(&mut self.ext.pending) {
            if let (Wait::Secs(t), false) = (&mut p.wait, opening) {
                *t -= dt;
            }
            match rules.get(p.rule) {
                Some(r) if self.wait_over(&p.wait) => self.run_from(p.rule, r, p.next, w, out),
                Some(_) => self.ext.pending.push(p),
                None => {}
            }
        }
    }

    /// True when every condition holds (all are evaluated so edge state advances); false for an empty list.
    fn all_hold(&mut self, conds: &[Value], w: &[Unit]) -> bool {
        let mut all = !conds.is_empty();
        for c in conds {
            all &= self.cond(c, w, false);
        }
        all
    }

    /// Self-completing objectives: an active task that is neither complete nor failed is completed or failed when its `complete_when` /
    /// `fail_when` hold (failing wins when both do). Then `"victory": "objectives"` ends the mission: lost when a required task has
    /// failed, won when every required task is complete (a required task that never became active keeps the mission going).
    pub(in crate::rules) fn update_objectives(&mut self, w: &[Unit]) {
        if self.ext.tasks.is_empty() {
            return;
        }
        let specs = std::mem::take(&mut self.ext.tasks);
        for spec in &specs {
            let Some(i) = self.tasks.iter().position(|t| t.name == spec.name) else { continue };
            if !self.tasks[i].active || self.tasks[i].complete || self.tasks[i].failed {
                continue;
            }
            let (failed, complete) = (self.all_hold(&spec.fail_when, w), self.all_hold(&spec.complete_when, w));
            self.tasks[i].failed = failed;
            self.tasks[i].complete = complete && !failed;
        }
        // Event-quota conditions (kills, captures) are evaluated but their events are not consumed: a task settles once, so it never asks twice.
        self.consume.clear();
        if self.skirmish_decides {
            if self.end.is_none() {
                self.team_objectives_end(&specs);
            }
        } else if self.ext.objectives_decide && self.ended.is_none() {
            let required: Vec<&Task> = specs.iter().filter(|x| x.required).filter_map(|x| self.tasks.iter().find(|t| t.name == x.name)).collect();
            if required.iter().any(|t| t.failed) {
                self.ended = Some((self.media["messages"][self.ext.defeat_message.as_str()].as_str().unwrap_or("Mission failed").into(), false));
            } else if !required.is_empty() && required.iter().all(|t| t.complete) {
                self.ended = Some((self.media["messages"][self.ext.victory_message.as_str()].as_str().unwrap_or("Mission complete.").into(), true));
            }
        }
        self.ext.tasks = specs;
    }
}

impl Script {
    /// A skirmish script that decides its own game: sets what `Team X Wins`, `Remaining Team Wins`, `End Game` and the team
    /// objectives say. The first rule to speak wins within a step, and the step after it nothing changes the end any more.
    pub(in crate::rules) fn edit_end(&mut self, f: impl FnOnce(&mut ScriptEnd)) {
        if !self.skirmish_decides || self.end_sealed || (self.end.is_some() && self.end_by != self.diag.cur) {
            return;
        }
        self.end_by = self.diag.cur;
        f(self.end.get_or_insert_with(ScriptEnd::default));
    }

    /// Per-team objectives of a skirmish script: the tasks that name a `team` and are `required`. A team with a failed required task
    /// loses; the first team (in the order of the tasks) whose required tasks are all complete wins. Tasks without a team never
    /// decide a skirmish (the checker says so), so the outcome is the same on every machine.
    fn team_objectives_end(&mut self, specs: &[TaskRule]) {
        let mut teams: Vec<&str> = vec![];
        for t in specs.iter().filter(|t| t.required && !t.team.is_empty()) {
            if !teams.contains(&t.team.as_str()) {
                teams.push(&t.team);
            }
        }
        for team in teams {
            let tasks: Vec<&Task> = specs.iter().filter(|x| x.required && x.team == team).filter_map(|x| self.tasks.iter().find(|t| t.name == x.name)).collect();
            let kind = if tasks.iter().any(|t| t.failed) {
                EndKind::Loser(team.to_string())
            } else if !tasks.is_empty() && tasks.iter().all(|t| t.complete) {
                EndKind::Team(team.to_string())
            } else {
                continue;
            };
            let (win, lose) = (Some(self.ext.victory_message.clone()).filter(|m| m.starts_with("IDGS_")), Some(self.ext.defeat_message.clone()).filter(|m| m.starts_with("IDGS_")));
            self.end_by = self.diag.cur;
            self.end = Some(ScriptEnd { kind, winner_message: win, loser_message: lose });
            return;
        }
    }
}

impl Script {
    /// How black the picture is now, 0..1: 0 with no fade running, 1 at its middle (`OP Fade`).
    pub fn fade_alpha(&self) -> f32 {
        self.ext.fade.map_or(0.0, |(total, t)| (1.0 - (2.0 * t / total - 1.0).abs()).clamp(0.0, 1.0))
    }
    /// Moves a running fade on by `dt` seconds of the mission's time.
    pub(in crate::rules) fn advance_fade(&mut self, dt: f32) {
        if let Some((total, t)) = &mut self.ext.fade {
            *t += dt;
            if *t >= *total {
                self.ext.fade = None;
            }
        }
    }
    /// The campaign counters the mission has now (a counter that was never set is not in the map and reads 0).
    pub fn campaign_values(&self) -> &HashMap<String, f32> {
        &self.ext.campaign
    }
    /// The campaign counters the earlier missions of the mod's campaign handed on, loaded before the mission's first step.
    pub fn set_campaign_values(&mut self, values: HashMap<String, f32>) {
        self.ext.campaign = values;
    }
    /// Once, when the mission has been won: the campaign counters to hand to the next mission. A mission that is lost or restarted hands on nothing.
    pub fn campaign_commit(&mut self) -> Option<HashMap<String, f32>> {
        let won = self.ended.as_ref().is_some_and(|e| e.1);
        (won && !std::mem::replace(&mut self.ext.campaign_committed, true)).then(|| self.ext.campaign.clone())
    }
}

impl Script {
    /// Lets the script decide the skirmish (`"victory": "script"`): its end rules and team objectives fill `end` (victory.rs reads it).
    pub fn decide_game(&mut self, on: bool) {
        self.skirmish_decides = on;
    }
}
