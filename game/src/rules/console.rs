//! Developer console (not retail behaviour): a one-shot runner for the mission rule vocabulary, opened with `~`.
//!
//! A typed command is not a second command system. It is turned into a rule action (or a condition, for a query) and run through
//! `Script::act` / `Script::cond`, so every action a mission can use is available, and `act {json}` takes any of them as raw JSON.
//! The short commands (`god`, `kill`, `reveal`, `tp`, ...) only build the action for the common cases. `repair` has no rule
//! action, so it is the one command that touches the ships directly (systems/console.rs). This file is the pure part: parsing,
//! the mapping to actions and the mission-debug queries; the ECS side and the keyboard live in `systems/console.rs`, the drawing in
//! the view. Using any command that changes the game marks the session as cheated (`Cheats`): campaign points are not recorded then.
use bevy::input::keyboard::Key;
use bevy::prelude::*;
use serde_json::{Value, json};

use super::schema::{self, Role};
use super::*;

/// Point set the `tp x y` command writes its destination to before running 'Teleport Group/Unit'.
const TP_SET: &str = "(console)";
/// The command words (Tab completion).
const COMMANDS: &[&str] =
    &["act", "cond", "do", "fire", "flags", "god", "help", "kill", "repair", "reveal", "schema", "set", "counter", "counters", "task", "tasks", "timer", "timers", "tp", "why"];
/// Lines of output kept.
const LOG_LINES: usize = 200;
/// Longest command line.
const MAX_LINE: usize = 400;

/// The console's state: shared, so views only draw it.
#[derive(Resource, Default)]
pub struct Console {
    /// Whether the console can be opened at all (debug builds, `--console` or `PROCYON_CONSOLE`; see `systems::console::enabled`).
    pub enabled: bool,
    pub open: bool,
    /// The line being typed.
    pub line: String,
    pub log: Vec<String>,
    history: Vec<String>,
    /// Index into `history` while Up / Down recall is in use.
    recall: Option<usize>,
    /// The console paused the simulation when it opened (and resumes it on closing).
    pub(in crate::rules) paused_by_us: bool,
    /// Lines submitted and not yet run (systems/console.rs runs them once per frame).
    pub(in crate::rules) pending: Vec<String>,
}

/// What a key press did to the line.
#[derive(Debug, PartialEq)]
pub enum Edit {
    None,
    /// Enter: a finished line (never empty).
    Submit(String),
    Close,
}

impl Console {
    pub fn new(enabled: bool) -> Self {
        Console { enabled, ..default() }
    }
    /// Adds a line (or several, split at newlines) to the output.
    pub fn say(&mut self, text: impl AsRef<str>) {
        self.log.extend(text.as_ref().lines().map(String::from));
        let over = self.log.len().saturating_sub(LOG_LINES);
        self.log.drain(..over);
    }
    /// Tab: finishes the command word (to the longest prefix the commands starting with it share, and a space when only one does).
    fn complete(&mut self) {
        if self.line.contains(char::is_whitespace) {
            return;
        }
        let word = self.line.to_lowercase();
        let hits: Vec<&str> = COMMANDS.iter().copied().filter(|c| c.starts_with(&word)).collect();
        let Some(first) = hits.first() else { return };
        let common = hits.iter().fold(first.len(), |n, c| n.min(first.chars().zip(c.chars()).take_while(|(a, b)| a == b).count()));
        self.line = first[..common].to_string();
        if hits.len() == 1 {
            self.line.push(' ');
        }
    }
    /// Queues a line to run as if it had been typed and submitted (the smoke script's way in).
    pub fn submit(&mut self, line: &str) {
        self.pending.push(line.to_string());
    }
    /// One key press on the open console: typing, Backspace, Up/Down for earlier lines, Enter, Escape / `~` to close.
    pub fn key(&mut self, key: &Key) -> Edit {
        match key {
            Key::Escape => return Edit::Close,
            Key::Character(c) if c.as_str() == "~" => return Edit::Close,
            Key::Character(c) => {
                let room = MAX_LINE.saturating_sub(self.line.chars().count());
                self.line.extend(c.chars().filter(|c| !c.is_control()).take(room));
            }
            Key::Space if self.line.chars().count() < MAX_LINE => self.line.push(' '),
            Key::Backspace => {
                self.line.pop();
            }
            Key::Tab => self.complete(),
            Key::ArrowUp if !self.history.is_empty() => {
                let i = self.recall.map_or(self.history.len() - 1, |i| i.saturating_sub(1));
                self.recall = Some(i);
                self.line.clone_from(&self.history[i]);
            }
            Key::ArrowDown => match self.recall {
                Some(i) if i + 1 < self.history.len() => {
                    self.recall = Some(i + 1);
                    self.line.clone_from(&self.history[i + 1]);
                }
                _ => {
                    self.recall = None;
                    self.line.clear();
                }
            },
            Key::Enter => {
                let line = std::mem::take(&mut self.line).trim().to_string();
                self.recall = None;
                if line.is_empty() {
                    return Edit::None;
                }
                if self.history.last() != Some(&line) {
                    self.history.push(line.clone());
                }
                return Edit::Submit(line);
            }
            _ => {}
        }
        Edit::None
    }
}

/// Whether cheats were used in this battle, and the state of the toggles.
#[derive(Resource, Default)]
pub struct Cheats {
    /// A command that changes the game ran: scores and campaign points of this session do not count.
    pub used: bool,
    /// `god` is on (the player's fleet takes no damage).
    pub god: bool,
}

/// A parsed console line.
#[derive(Debug, PartialEq)]
pub enum Command {
    Nothing,
    Help,
    /// Rule actions, each run through `Script::act`. `god` carries the new state of the god toggle.
    Acts {
        acts: Vec<Value>,
        god: Option<bool>,
    },
    /// A rule condition, evaluated and answered with true / false.
    Cond(Value),
    /// 'Set Group/Unit Visibility' for every owner on the map.
    Reveal,
    /// 'Destroy Group/Unit' on the current target.
    Kill,
    /// 'Teleport Group/Unit' for the player's ship to a point (game axes), or to the first point of a named point set.
    Teleport(Dest),
    /// Full repair of a group (not a rule action).
    Repair(String),
    /// Run a rule's actions now, as `--fire-rule` does at the start.
    Fire(String),
    /// Which conditions of a rule hold now.
    Why(String),
    Flags,
    Timers,
    Tasks,
    /// Every counter set so far (engine-added `OP` counters).
    Counters,
    /// The rule schema's entry for an action or condition Type (or the Types whose name contains the text): its fields.
    Schema(String),
}

#[derive(Debug, PartialEq)]
pub enum Dest {
    Xy(f32, f32, f32),
    Set(String),
}

impl Command {
    /// Whether running it changes the game (it then marks the session as cheated). Queries do not.
    pub fn cheats(&self) -> bool {
        matches!(self, Command::Acts { .. } | Command::Reveal | Command::Kill | Command::Teleport(_) | Command::Repair(_) | Command::Fire(_))
    }
}

pub const HELP: &str = "\
god [on|off]            the player's fleet takes no damage (Set Group/Unit Vulnerability)
repair [group]          full repair of the player's fleet, or of a group
kill                    destroy the current target (Destroy Group/Unit)
reveal                  show every hidden unit (Set Group/Unit Visibility)
tp <x> <y> [z] | <set>  move the player's ship to game coordinates, or to a point set of the mission
fire <rule>             run a rule's actions now (as --fire-rule); a part of the name is enough
why <rule>              which conditions of a rule hold now
flags | timers | tasks | counters  the mission's state
set <flag> [on|off]     Set Flag Action;  counter <name> <value>  OP Set Counter;  timer <name> start|stop;  task <name> active|complete|failed [off]
act <json>              any rule action, e.g. act {\"Type\": \"Set Flag Action\", \"Flag Name\": \"x\", \"Boolean Value\": true}
cond <json>             evaluate any rule condition
schema <text>           the fields of an action or condition Type (the rule schema); act / cond check their JSON against it
Tab completes a command name; Up / Down recall earlier lines.
Using a command that changes the game marks the session as cheated: campaign points are not recorded.";

/// Turns a line into a command. `player` is the human's group name (rules address the fleet by it); `god` the current god state.
pub fn parse(line: &str, player: &str, god: bool) -> Result<Command, String> {
    let line = line.trim();
    let (word, rest) = line.split_once(char::is_whitespace).map_or((line, ""), |(w, r)| (w, r.trim()));
    let args: Vec<&str> = rest.split_whitespace().collect();
    let act = |a: Value| Command::Acts { acts: vec![a], god: None };
    Ok(match word.to_lowercase().as_str() {
        "" => Command::Nothing,
        "help" | "?" => Command::Help,
        "god" => {
            let on = match args.as_slice() {
                [] => !god,
                [v] => flag_word(v)?,
                _ => return Err("usage: god [on|off]".into()),
            };
            Command::Acts { acts: vec![vulnerability(player, if on { 1.0 } else { 0.0 })], god: Some(on) }
        }
        "repair" => Command::Repair(if rest.is_empty() { player.to_string() } else { rest.to_string() }),
        "kill" => Command::Kill,
        "reveal" => Command::Reveal,
        "tp" => match args.as_slice() {
            [x, y] | [x, y, _] if x.parse::<f32>().is_ok() => {
                let n = |s: &str| s.parse::<f32>().map_err(|_| format!("'{s}' is not a number"));
                Command::Teleport(Dest::Xy(n(x)?, n(y)?, args.get(2).map_or(Ok(0.0), |z| n(z))?))
            }
            [] => return Err("usage: tp <x> <y> [z]  or  tp <point set name>".into()),
            _ => Command::Teleport(Dest::Set(rest.to_string())),
        },
        "spawn" => return Err("spawn: no rule action creates ships yet, so there is nothing to run".into()),
        "fire" if !rest.is_empty() => Command::Fire(unquote(rest).into()),
        "why" if !rest.is_empty() => Command::Why(unquote(rest).into()),
        "fire" | "why" => return Err(format!("usage: {word} <rule name>")),
        "flags" => Command::Flags,
        "timers" => Command::Timers,
        "tasks" => Command::Tasks,
        "counters" => Command::Counters,
        "counter" => match args.as_slice() {
            [name, value] => match value.parse::<f32>() {
                Ok(v) => act(json!({"Type": "OP Set Counter", "Counter Name": name, "Value": v})),
                Err(_) => return Err(format!("'{value}' is not a number")),
            },
            _ => return Err("usage: counter <name> <value>".into()),
        },
        "schema" if !rest.is_empty() => Command::Schema(unquote(rest).into()),
        "schema" => return Err("usage: schema <text>, e.g. schema Teleport".into()),
        "set" => match args.as_slice() {
            [] => return Err("usage: set <flag> [on|off]".into()),
            [name] => act(set_flag(name, true)),
            [name, v] => act(set_flag(name, flag_word(v)?)),
            _ => return Err("usage: set <flag> [on|off] (a flag name has no spaces)".into()),
        },
        "timer" => match args.as_slice() {
            [name, "start"] => act(json!({"Type": "Start Timer", "Timer Name": name})),
            [name, "stop"] => act(json!({"Type": "Stop Timer", "Timer Name": name})),
            _ => return Err("usage: timer <name> start|stop".into()),
        },
        "task" => match args.as_slice() {
            [name, state] | [name, state, _] => {
                let on = args.get(2).map_or(Ok(true), |v| flag_word(v))?;
                let (kind, key) = match *state {
                    "active" => ("Set Objective Task Active State", "Active State"),
                    "complete" => ("Set Objective Task Complete State", "Complete State"),
                    "failed" => ("Set Objective Task Failed State", "Failed State"),
                    other => return Err(format!("'{other}': expected active, complete or failed")),
                };
                act(json!({"Type": kind, "Objective Task": name, key: wot(on)}))
            }
            _ => return Err("usage: task <name> active|complete|failed [on|off]".into()),
        },
        "act" | "do" => match serde_json::from_str::<Value>(rest) {
            Ok(v) if v["Type"].is_string() => {
                validate(Role::Action, &v)?;
                Command::Acts { acts: vec![strings(v)], god: None }
            }
            Ok(_) => return Err("an action needs a \"Type\"".into()),
            Err(e) => return Err(format!("not JSON: {e}")),
        },
        "cond" => match serde_json::from_str::<Value>(rest) {
            Ok(v) if v["Type"].is_string() => {
                validate(Role::Condition, &v)?;
                Command::Cond(strings(v))
            }
            Ok(_) => return Err("a condition needs a \"Type\"".into()),
            Err(e) => return Err(format!("not JSON: {e}")),
        },
        other => return Err(format!("unknown command '{other}' (help lists them)")),
    })
}

fn flag_word(v: &str) -> Result<bool, String> {
    match v.to_lowercase().as_str() {
        "on" | "true" | "1" | "yes" => Ok(true),
        "off" | "false" | "0" | "no" => Ok(false),
        _ => Err(format!("'{v}': expected on or off")),
    }
}
fn wot(on: bool) -> &'static str {
    if on { "TRUE" } else { "FALSE" }
}
fn unquote(s: &str) -> &str {
    s.trim().trim_matches('"')
}
/// The rules' booleans are the strings TRUE / FALSE; JSON true / false typed by hand mean the same.
fn strings(v: Value) -> Value {
    match v {
        Value::Bool(b) => Value::String(wot(b).into()),
        Value::Array(a) => Value::Array(a.into_iter().map(strings).collect()),
        Value::Object(m) => Value::Object(m.into_iter().map(|(k, v)| (k, strings(v))).collect()),
        other => other,
    }
}

/// 'Set Group/Unit Vulnerability': 1.0 takes no damage, 0.0 is normal.
pub fn vulnerability(group: &str, percent: f32) -> Value {
    json!({"Type": "Set Group/Unit Vulnerability", "Group Name": group, "Invulnerable Percent": percent})
}
fn set_flag(name: &str, on: bool) -> Value {
    json!({"Type": "Set Flag Action", "Flag Name": name, "Boolean Value": wot(on)})
}
/// 'Destroy Group/Unit' for one unit, by its 'Group,Unit' spec.
pub fn destroy(spec: &str) -> Value {
    json!({"Type": "Destroy Group/Unit", "Group Name": spec})
}
pub fn visibility(group: &str, visible: bool) -> Value {
    json!({"Type": "Set Group/Unit Visibility", "Group/Unit Name": group, "Boolean": wot(visible)})
}
pub fn teleport(spec: &str, point_set: &str) -> Value {
    json!({"Type": "Teleport Group/Unit", "Group/Unit Name": spec, "Point Set Name": point_set})
}
/// The spec of the human's command ship.
pub fn flagship(player: &str) -> String {
    format!("{player},HUMAN CONTROLLED COMMAND SHIP")
}

/// What a command did besides the lines it prints.
#[derive(Default)]
pub struct Outcome {
    pub lines: Vec<String>,
    /// Units to repair fully (`repair`).
    pub repair: Vec<Entity>,
}

impl Script {
    /// Runs a command against the unit list `w` (what `systems::units` builds each frame). ECS effects go to `out`, as in `step`.
    /// `target`: the player's current target.
    pub fn console_run(&mut self, cmd: &Command, w: &mut [Unit], target: Option<Entity>, out: &mut Vec<Fx>) -> Outcome {
        let mut o = Outcome::default();
        let player = self.player.clone();
        match cmd {
            Command::Nothing => {}
            Command::Help => o.lines.extend(HELP.lines().map(String::from)),
            Command::Acts { acts, .. } => {
                for a in acts {
                    self.console_act(a, w, out);
                }
                o.lines.push(format!("ran {}", acts.iter().map(|a| s(a, "Type")).collect::<Vec<_>>().join(", ")));
            }
            Command::Cond(c) => {
                let held = self.console_cond(c, w);
                o.lines.push(held.to_string());
            }
            Command::Reveal => {
                let mut owners: Vec<&str> = w.iter().filter(|u| !u.owner.is_empty()).map(|u| u.owner.as_str()).collect();
                owners.sort_unstable();
                owners.dedup();
                let hidden = w.iter().filter(|u| !u.visible).count();
                for owner in owners.into_iter().map(String::from).collect::<Vec<_>>() {
                    self.console_act(&visibility(&owner, true), w, out);
                }
                o.lines.push(format!("{hidden} hidden unit(s) shown"));
            }
            Command::Kill => match target.and_then(|t| w.iter().find(|u| u.e == t)) {
                Some(u) if !u.name.is_empty() => {
                    let (spec, name) = (format!("{},{}", u.owner, u.name), u.name.clone());
                    self.console_act(&destroy(&spec), w, out);
                    o.lines.push(format!("destroyed {name}"));
                }
                Some(_) => o.lines.push("the target has no unit name for a rule to address".into()),
                None => o.lines.push("no target: click a ship first".into()),
            },
            Command::Teleport(dest) => {
                let set = match dest {
                    Dest::Xy(x, y, z) => {
                        self.point_sets[TP_SET] = json!([{"pos": [x, y, z]}]);
                        TP_SET.to_string()
                    }
                    Dest::Set(name) if self.point_sets[name.as_str()].get(0).is_some() => name.clone(),
                    Dest::Set(name) => {
                        let mut names: Vec<&str> = self.point_sets.as_object().into_iter().flatten().map(|(k, _)| k.as_str()).filter(|k| *k != TP_SET).collect();
                        names.sort_unstable();
                        o.lines.push(format!("no point set '{name}'; the mission has {}", names.join(", ")));
                        return o;
                    }
                };
                self.console_act(&teleport(&flagship(&player), &set), w, out);
                o.lines.push(format!(
                    "teleported to {}",
                    match dest {
                        Dest::Xy(x, y, _) => format!("{x}, {y}"),
                        Dest::Set(n) => n.clone(),
                    }
                ));
            }
            Command::Repair(spec) => {
                o.repair = ids(w, spec);
                o.lines.push(format!("repairing {} unit(s) of {spec}", o.repair.len()));
            }
            Command::Fire(name) => match self.rule_named(name) {
                Ok(i) => {
                    let r = &self.rules[i];
                    let rule = Rule { name: r.name.clone(), once: r.once, conds: vec![], acts: r.acts.clone() };
                    if rule.once {
                        self.fired[i] = true;
                    }
                    self.ran[i] = true;
                    info!("t={:.1} rule #{i} fired from the console: {}", self.time, rule.name.trim());
                    self.run_from(i, &rule, 0, w, out);
                    o.lines.push(format!("fired '{}' ({} action(s))", rule.name.trim(), rule.acts.len()));
                }
                Err(e) => o.lines.push(e),
            },
            Command::Why(name) => match self.rule_named(name) {
                Ok(i) => o.lines = self.why(i, w),
                Err(e) => o.lines.push(e),
            },
            Command::Flags => {
                let mut v: Vec<_> = self.flags.iter().collect();
                v.sort();
                o.lines.extend(v.into_iter().map(|(k, on)| format!("{k} = {on}")));
                if o.lines.is_empty() {
                    o.lines.push("no flag has been set".into());
                }
            }
            Command::Timers => {
                let mut v: Vec<_> = self.timers.iter().collect();
                v.sort_by(|a, b| a.0.cmp(b.0));
                o.lines.extend(v.into_iter().map(|(k, t)| format!("{k} = {t:.1} s{}", if self.stopped.contains(k) { " (stopped)" } else { "" })));
                if o.lines.is_empty() {
                    o.lines.push("no timer has been started".into());
                }
            }
            Command::Schema(query) => o.lines = schema_lines(query),
            Command::Counters => {
                o.lines.extend(self.ext.counter_list().into_iter().map(|(k, v)| format!("{k} = {v}")));
                if o.lines.is_empty() {
                    o.lines.push("no counter has been set".into());
                }
            }
            Command::Tasks => {
                o.lines.extend(self.tasks.iter().map(|t| {
                    let state = if t.complete {
                        "complete"
                    } else if t.failed {
                        "failed"
                    } else if t.active {
                        "active"
                    } else {
                        "inactive"
                    };
                    format!("{:<9} {}  {}", state, t.name, t.text.replace('\n', " "))
                }));
                if o.lines.is_empty() {
                    o.lines.push("the mission has no tasks".into());
                }
            }
        }
        o
    }

    /// One action, with a visibility change shown to the actions after it (as `run_acts` does for a rule).
    fn console_act(&mut self, a: &Value, w: &mut [Unit], out: &mut Vec<Fx>) {
        let from = out.len();
        self.act(a, w, out, "(console)");
        for fx in &out[from..] {
            if let Fx::Visible(ids, visible) = fx {
                for u in w.iter_mut().filter(|u| ids.contains(&u.e)) {
                    u.visible = *visible;
                }
            }
        }
    }

    /// A condition asked once from outside the rule loop: the edge state the rules' own evaluation depends on is put back after it.
    fn console_cond(&mut self, c: &Value, w: &[Unit]) -> bool {
        let saved = (self.prev_inside.clone(), self.entered.clone(), self.events.clone(), self.mortar_seen.clone());
        let held = self.cond(c, w, false);
        (self.prev_inside, self.entered, self.events, self.mortar_seen) = saved;
        self.consume.clear();
        held
    }

    /// The rule a (part of a) name stands for: the exact name, else the only rule whose name contains it (any case).
    fn rule_named(&self, name: &str) -> Result<usize, String> {
        let name = name.trim();
        if let Some(i) = self.rules.iter().position(|r| r.name.trim() == name) {
            return Ok(i);
        }
        let found: Vec<usize> = (0..self.rules.len()).filter(|&i| self.rules[i].name.to_lowercase().contains(&name.to_lowercase())).collect();
        match found.as_slice() {
            [i] => Ok(*i),
            [] => Err(format!("no rule named '{name}'")),
            many => Err(format!("'{name}' matches {} rules: {}", many.len(), many.iter().take(8).map(|&i| self.rules[i].name.trim()).collect::<Vec<_>>().join("; "))),
        }
    }

    /// Each condition of rule `i` with whether it holds now, and whether the rule already ran.
    fn why(&mut self, i: usize, w: &[Unit]) -> Vec<String> {
        let (name, once, conds) = (self.rules[i].name.trim().to_string(), self.rules[i].once, self.rules[i].conds.clone());
        let mut lines =
            vec![format!("{name}: {}{}", if self.ran[i] { "has run" } else { "has not run" }, if once && self.fired[i] { " (run-once: it will not run again)" } else { "" })];
        if !self.ext.is_active(i) {
            lines.push("  inactive (\"Is Active\" is false or OP Set Rule Active turned it off): its conditions are not evaluated; OP Set Rule Active turns it on".into());
        }
        if self.ext.busy(i) {
            lines.push(format!("  {}", self.ext.busy_note(i)));
        }
        let mut all = true;
        for c in &conds {
            let held = self.console_cond(c, w);
            all &= held;
            lines.push(format!("  {} {}", if held { "yes" } else { "NO " }, self.explain(c, w)));
        }
        if conds.is_empty() {
            lines.push("  (no conditions)".into());
        } else if all && !(once && self.fired[i]) && self.ext.is_active(i) && !self.ext.busy(i) {
            lines.push("every condition holds now: the rule runs on the next step".into());
        }
        lines
    }
}

/// Checks a raw action or condition against the rule schema: a known Type, no field it does not have, none of its required fields
/// missing. The interpreter itself ignores what it does not know; at the console that would be a silent no-op.
pub fn validate(role: Role, v: &Value) -> Result<(), String> {
    let ty = s(v, "Type");
    let what = if role == Role::Action { "action" } else { "condition" };
    let Some(def) = schema::lookup(role, ty) else {
        let near: Vec<&str> = schema::RULE_TYPES
            .iter()
            .filter(|t| t.role == role && (t.name.to_lowercase().contains(&ty.to_lowercase()) || ty.to_lowercase().contains(&t.name.to_lowercase())))
            .map(|t| t.name)
            .take(5)
            .collect();
        let hint = if near.is_empty() { "`schema <text>` searches the Types".to_string() } else { format!("did you mean: {}", near.join("; ")) };
        return Err(format!("no {what} Type '{ty}' ({hint})"));
    };
    let unknown: Vec<&str> = v.as_object().into_iter().flatten().map(|(k, _)| k.as_str()).filter(|k| *k != "Type" && def.field(k).is_none()).collect();
    if !unknown.is_empty() {
        return Err(format!("'{ty}' has no field {}; its fields are: {}", unknown.join(", "), def.fields.iter().map(|f| f.name).collect::<Vec<_>>().join(", ")));
    }
    let missing: Vec<&str> = def.fields.iter().filter(|f| f.required && v[f.name].is_null()).map(|f| f.name).collect();
    if !missing.is_empty() {
        return Err(format!("'{ty}' needs {}", missing.join(", ")));
    }
    Ok(())
}

/// The schema entries (actions and conditions) whose Type is `query` or contains it (any case), one line each, then their fields.
pub fn schema_lines(query: &str) -> Vec<String> {
    let q = query.trim().to_lowercase();
    let mut types: Vec<&schema::TypeDef> = schema::RULE_TYPES.iter().filter(|t| t.name.to_lowercase() == q).collect();
    if types.is_empty() {
        types = schema::RULE_TYPES.iter().filter(|t| t.name.to_lowercase().contains(&q)).collect();
    }
    let mut lines = vec![];
    match types.len() {
        0 => lines.push(format!("no Type contains '{query}'")),
        1..=3 => {
            for t in types {
                lines.push(format!(
                    "{} {}{}",
                    if t.role == Role::Action { "action   " } else { "condition" },
                    t.name,
                    if t.origin == schema::Origin::Engine { "  (engine-added)" } else { "" }
                ));
                for f in t.fields.iter().filter(|f| f.used) {
                    lines.push(format!(
                        "  {} ({}){} default {}",
                        f.name,
                        f.kind.name(),
                        if f.required { " required" } else { "" },
                        if f.default.is_empty() { "\"\"" } else { f.default }
                    ));
                }
            }
        }
        n => {
            lines.push(format!("{n} Types match; a longer text narrows it:"));
            lines.extend(types.iter().take(30).map(|t| format!("  {} {}", if t.role == Role::Action { "action   " } else { "condition" }, t.name)));
        }
    }
    lines
}

#[cfg(test)]
mod tests;
