use bevy::prelude::*;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::Nearest;
use crate::campaign::Mission;
use crate::game_to_bevy;

mod act;
mod cond;
mod diag;
mod ext;
pub mod schema;
mod state;
mod types;

pub use diag::{Diag, TraceOptions, set_trace};
use ext::Ext;
pub use state::Script;
pub use types::*;

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
        let diag = Diag::new(rules.iter().map(|r| r.name.trim().to_string()).collect(), crate::rules::check::known_names(data));
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
            ext: Ext::new(data, &player),
            rules,
            fired: vec![false; n],
            ran: vec![false; n],
            init_done: false,
            start_rules: vec![],
            start_at: None,
            briefing_opening: false,
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
            skirmish_decides: false,
            end: None,
            end_by: 0,
            end_sealed: false,
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
            sunk: vec![],
            teams: HashMap::new(),
            team_points: HashMap::new(),
            team_objectives: HashMap::new(),
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
            diag,
        }
    }

    /// Advance the script by `dt`: timers, speech, rules. Returns the ECS effects and any speech file to start.
    pub fn step(&mut self, w: &[Unit], dt: f32, opening: bool) -> (Vec<Fx>, Option<String>) {
        self.time += dt;
        let first_step = !self.init_done;
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
        if first_step {
            self.audit(&rules, &w);
        }
        self.resume_sequences(&rules, &mut w, &mut out, dt, opening);
        for (i, r) in rules.iter().enumerate() {
            self.diag.cur = i;
            // The opening NIS camera/speech rules are replaced by briefing.rs (which marks its speech as played).
            if self.fired[i] || (self.briefing_opening && opening_rule(&r.name)) {
                continue;
            }
            if !self.ext.is_active(i) {
                if self.diag.active() {
                    self.trace_state(i, "inactive (\"Is Active\" is false or OP Set Rule Active turned it off): its conditions are not evaluated");
                }
                continue;
            }
            let mut ok = true;
            self.consume.clear();
            let mut failed = vec![];
            for (ci, c) in r.conds.iter().enumerate() {
                if !self.cond(c, &w, !r.once) {
                    ok = false;
                    if self.diag.active() {
                        failed.push(ci);
                    }
                }
            } // evaluate all: Enter Volume edge state must advance
            if !ok {
                if self.diag.active() {
                    self.trace_waiting(i, r, &failed, &w);
                }
                continue;
            }
            if self.ext.busy(i) {
                if self.diag.active() {
                    let note = self.ext.busy_note(i);
                    self.trace_state(i, &note);
                }
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
            self.run_from(i, r, 0, &mut w, &mut out);
        }
        self.update_objectives(&w);
        self.end_sealed |= self.end.is_some();
        if first_step {
            self.fire_start_rules(&rules, &mut w, &mut out);
        }
        self.rules = rules;
        self.init_done = true;
        self.mortar_bursts.clear();
        let first = |spec: Option<&str>| members(&w, spec?).into_iter().find(|u| u.alive).map(|u| u.e);
        self.cam.attach_e = first(self.cam.attach.as_ref().map(|a| a.0.as_str()));
        self.cam.focus_e = first(self.cam.focus_unit.as_deref());
        self.point_pos = self.point_on.as_deref().and_then(|sp| members(&w, sp).into_iter().find(|u| u.alive)).map(|u| u.pos);
        // A tow ends when either ship is destroyed (mission 10: the Plunder sunk with the SS Spirit of the West in tow).
        self.tows.retain(|t| [t.0, t.1].iter().all(|e| w.iter().any(|u| u.e == *e && u.alive)));
        for (tower, towed) in std::mem::take(&mut self.tow_specs) {
            if let (Some(a), Some(bb)) = (ids(&w, &tower).first(), ids(&w, &towed).first()) {
                out.push(Fx::Tow(*a, *bb));
            } else {
                self.tow_specs.push((tower, towed));
            }
        }
        (out, voice)
    }
}

impl Script {
    /// Debug aid for mission authors (`--fire-rule`, `--start-at`; not retail behaviour): once the world-initialise rules have run, the
    /// actions of each named rule run in the order given as if its conditions had held, and the rule counts as having run, so a run-once
    /// rule does not fire again. An unknown name is a warning listing the valid ones. `start_at` moves the primary ship.
    fn fire_start_rules(&mut self, rules: &[Rule], w: &mut [Unit], out: &mut Vec<Fx>) {
        for name in std::mem::take(&mut self.start_rules) {
            let Some(i) = rules.iter().position(|r| r.name.trim() == name.trim()) else {
                let valid: Vec<&str> = rules.iter().map(|r| r.name.trim()).collect();
                warn!("--fire-rule {name:?}: no such rule; the mission's rules are {valid:?}");
                continue;
            };
            if rules[i].once {
                self.fired[i] = true;
            }
            self.ran[i] = true;
            info!("t={:.1} rule #{i} fired at start (--fire-rule): {}", self.time, name.trim());
            self.run_from(i, &rules[i], 0, w, out);
        }
        if let Some((x, y)) = self.start_at.take() {
            match w.iter().find(|u| u.primary && u.alive) {
                Some(p) => out.push(Fx::Teleport(vec![p.e], crate::data::conv(x, y, 0.0))),
                None => warn!("--start-at: the mission has no primary ship to place"),
            }
        }
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
