//! Debug aids for mission authors (not retail behaviour; they only log): the rule trace (`--rule-trace`, `--why "<rule>"`) and the
//! start-of-mission audit that warns, once per rule, about a group, volume, path or point set a rule names that the mission does not define.

use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use super::schema::{self, Kind, Role};
use super::*;

/// What the launch asked to see.
#[derive(Clone, Default, Debug)]
pub struct TraceOptions {
    /// `--rule-trace` (or `PROCYON_RULE_TRACE=1`): every rule that has not fired.
    pub all: bool,
    /// `--why "<rule name>"` (repeatable): only the rules with that name (a case-insensitive part of a name when no rule is called so).
    pub why: Vec<String>,
}

static OPTIONS: OnceLock<TraceOptions> = OnceLock::new();
/// Set once by `run`; every mission loaded afterwards traces the same way.
pub fn set_trace(options: TraceOptions) {
    let _ = OPTIONS.set(options);
}

/// Trace and audit state of one `Script`.
#[derive(Default)]
pub struct Diag {
    all: bool,
    traced: HashSet<usize>,
    /// `--why` names that match no rule, reported at the first step.
    unmatched: Vec<String>,
    names: Vec<String>,
    /// Every group, player and unit name the mission defines (`check::known_names`).
    known: HashSet<String>,
    /// The rule being evaluated (`Script::warn_unresolved` names it).
    pub(in crate::rules) cur: usize,
    /// Per rule: the conditions that were false when it was last reported.
    last: HashMap<usize, Vec<usize>>,
    /// Per rule: the inactive / busy note last reported, so a state is logged once, not every frame.
    state: HashMap<usize, String>,
}

impl Diag {
    pub(in crate::rules) fn new(names: Vec<String>, known: HashSet<String>) -> Diag {
        let options = OPTIONS.get().cloned().unwrap_or_default();
        let env = std::env::var_os("PROCYON_RULE_TRACE").is_some_and(|v| !v.is_empty() && v != "0");
        let (traced, unmatched) = resolve_why(&options.why, &names);
        Diag { all: options.all || env, traced, unmatched, names, known, ..Default::default() }
    }
    /// Whether any trace is on (the interpreter then keeps track of the false conditions).
    pub(in crate::rules) fn active(&self) -> bool {
        self.all || !self.traced.is_empty()
    }
    fn wants(&self, rule: usize) -> bool {
        self.all || self.traced.contains(&rule)
    }
    fn name(&self, rule: usize) -> &str {
        self.names.get(rule).map_or("?", String::as_str)
    }
}

/// The rules `--why` asks about: each name selects the rules called exactly that, else those whose name contains it (any case).
/// Returns the rule indices, and the names that select nothing.
fn resolve_why(why: &[String], names: &[String]) -> (HashSet<usize>, Vec<String>) {
    let (mut traced, mut unmatched) = (HashSet::new(), vec![]);
    for want in why.iter().map(|w| w.trim()) {
        let exact: Vec<usize> = names.iter().enumerate().filter(|(_, n)| n.as_str() == want).map(|(i, _)| i).collect();
        let found = if exact.is_empty() { names.iter().enumerate().filter(|(_, n)| n.to_lowercase().contains(&want.to_lowercase())).map(|(i, _)| i).collect() } else { exact };
        if found.is_empty() {
            unmatched.push(want.to_string());
        }
        traced.extend(found);
    }
    (traced, unmatched)
}

impl Script {
    /// A rule that is not evaluated or whose sequence is still running (engine-added `Is Active` / `OP Wait`): logs the state when it changes.
    pub(in crate::rules) fn trace_state(&mut self, i: usize, note: &str) {
        if !self.diag.wants(i) || self.diag.state.get(&i).is_some_and(|s| s == note) {
            return;
        }
        self.diag.state.insert(i, note.to_string());
        self.diag.last.remove(&i);
        info!("t={:.1} rule trace: '{}' (rule #{i}) is {note}", self.time, self.diag.name(i));
    }

    /// A rule that has not fired: logs which of its conditions are false, when that set changed since the last report.
    pub(in crate::rules) fn trace_waiting(&mut self, i: usize, r: &Rule, failed: &[usize], w: &[Unit]) {
        if !self.diag.wants(i) || self.diag.last.get(&i).is_some_and(|l| l == failed) {
            return;
        }
        self.diag.last.insert(i, failed.to_vec());
        self.diag.state.remove(&i);
        let why: Vec<String> = failed.iter().map(|&c| format!("#{} {}", c + 1, self.explain(&r.conds[c], w))).collect();
        info!("t={:.1} rule trace: '{}' (rule #{i}) waits for {} of {} conditions: {}", self.time, self.diag.name(i), failed.len(), r.conds.len(), why.join(" | "));
    }

    /// One condition as the trace shows it: its Type and fields, then what the world says about each name it reads.
    pub(in crate::rules) fn explain(&self, c: &Value, w: &[Unit]) -> String {
        let ty = s(c, "Type");
        let Some(def) = schema::lookup(Role::Condition, ty) else { return ty.to_string() };
        let mut fields = vec![];
        let mut notes = vec![];
        for field in def.fields.iter().filter(|f| !c[f.name].is_null()) {
            let v = s(c, field.name);
            // Engine-added combinators: explain the nested conditions instead of dumping their JSON.
            if matches!(field.kind, Kind::ConditionList | Kind::Condition) {
                let subs: Vec<&Value> = if field.kind == Kind::Condition { vec![&c[field.name]] } else { c[field.name].as_array().into_iter().flatten().collect() };
                let nested: Vec<String> = subs.iter().map(|x| self.explain(x, w)).collect();
                notes.push(format!("nested: {}", nested.join(" ; ")));
                continue;
            }
            fields.push(format!("{}={}", field.name, c[field.name]));
            match field.kind {
                Kind::Group if !v.is_empty() => {
                    let m = members(w, v);
                    notes.push(format!("group '{v}' has {} members, {} alive", m.len(), m.iter().filter(|u| u.alive).count()));
                }
                Kind::Volume if !v.is_empty() => match self.volumes.get(v) {
                    Some(r) => notes.push(format!("volume '{v}' has {} vertices", r.len())),
                    None => notes.push(format!("no volume '{v}'")),
                },
                Kind::Flag => notes.push(format!("flag '{v}' is {}", if self.flag(v) { "TRUE" } else { "FALSE (never set or set false)" })),
                Kind::Counter => notes.push(format!("counter '{v}' reads {}", self.ext.counter(v))),
                Kind::Timer => match self.timer(v) {
                    Some(t) => notes.push(format!("timer '{v}' reads {t:.1}s{}", if self.stopped.contains(v) { " (stopped)" } else { "" })),
                    None => notes.push(format!("timer '{v}' was never started")),
                },
                Kind::Speech => {
                    let state = if self.played.contains(v) {
                        "played"
                    } else if self.queue.iter().any(|q| q == v) || self.speaking.as_ref().is_some_and(|x| x.0 == v) {
                        "playing or queued"
                    } else {
                        "not played yet"
                    };
                    notes.push(format!("speech '{v}' is {state}"));
                }
                _ => {}
            }
        }
        format!("{ty} [{}]{}", fields.join(", "), if notes.is_empty() { String::new() } else { format!(" ({})", notes.join("; ")) })
    }

    /// Once, at the first step with the world in place: `--why` names that match no rule, and every group, volume, path and point
    /// set a rule names that the mission does not define (a misspelling is otherwise silent: no members, an empty region, no points).
    pub(in crate::rules) fn audit(&mut self, rules: &[Rule], w: &[Unit]) {
        for want in std::mem::take(&mut self.diag.unmatched) {
            let names: Vec<&str> = rules.iter().map(|r| r.name.trim()).collect();
            warn!("--why {want:?}: no such rule; the mission's rules are {names:?}");
        }
        for (i, r) in rules.iter().enumerate() {
            self.diag.cur = i;
            for (role, entries) in [(Role::Condition, &r.conds), (Role::Action, &r.acts)] {
                for e in entries {
                    let ty = s(e, "Type");
                    let Some(def) = schema::lookup(role, ty) else { continue };
                    for field in def.fields.iter().filter(|f| f.used) {
                        let v = s(e, field.name);
                        if v.is_empty() || field.kind.placeholders().contains(&v) {
                            continue;
                        }
                        let empty = match field.kind {
                            // A name the mission defines may still have no unit now (animals, storms, a group that lost its ships).
                            Kind::Group => {
                                let name = v.split_once(',').map_or(v, |(_, unit)| unit);
                                members(w, v).is_empty() && !self.diag.known.contains(name) && name != "HUMAN CONTROLLED COMMAND SHIP"
                            }
                            Kind::Volume => self.volumes.get(v).is_none_or(|r| r.len() < 3),
                            Kind::PointSet => self.point_sets[v].as_array().is_none_or(Vec::is_empty),
                            Kind::Path => self.paths[v].as_array().is_none_or(Vec::is_empty),
                            _ => continue,
                        };
                        if empty {
                            self.warn_unresolved(field.kind, field.name, v, ty);
                        }
                    }
                }
            }
        }
    }

    /// Warns once per rule and name: a rule names something that resolves to nothing.
    pub(in crate::rules) fn warn_unresolved(&mut self, kind: Kind, field: &str, name: &str, ty: &str) {
        let rule = self.diag.name(self.diag.cur).to_string();
        let key = format!("unresolved|{rule}|{field}|{name}");
        if self.warned.insert(key) {
            let what = match kind {
                Kind::Group => "names no group, player or unit of the mission",
                Kind::Volume => "is no volume (or has fewer than 3 vertices)",
                Kind::PointSet => "is no point set (or has no points)",
                _ => "is no path (or has no points)",
            };
            warn!("mission rules: rule '{rule}': {} '{name}' in field '{field}' of '{ty}' {what}", kind.name());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn script() -> Script {
        let data = json!({
            "players": ["Player0 Group"],
            "groups": {"Fleet": [1]},
            "volumes": {"Area": [[0, 0], [10, 0], [10, 10]]},
            "environment": {"point_sets": {"Spot": [{"pos": [0.0, 0.0, 0.0]}]}},
            "rules": [
                {"name": "Typos", "Run Once": true, "conditions": [{"Type": "Group Destroyed", "Group Name": "Fleat"}],
                 "actions": [{"Type": "Teleport Group/Unit", "Group/Unit Name": "Fleet", "Point Set Name": "Spt"}, {"Type": "Set Group/Unit Visibility", "Group/Unit Name": "Fleet", "Boolean": "TRUE"}]},
                {"name": "Gate", "Run Once": true, "conditions": [{"Type": "Flag Condition", "Flag Name": "Go", "Boolean Value": "TRUE"}, {"Type": "Enter Volume", "Group Name": "Fleet", "Volume Name": "Aera"}], "actions": []},
            ],
        });
        Script::new(&data, Value::Null)
    }
    fn fleet() -> Unit {
        let mut u = Unit::new(Entity::PLACEHOLDER, 1, Vec3::ZERO);
        u.groups = vec!["Fleet".into()];
        u
    }

    #[test]
    fn the_audit_warns_once_about_names_the_mission_does_not_define() {
        let mut s = script();
        s.step(&[fleet()], 0.1, false);
        let warned: Vec<&String> = s.warned.iter().filter(|w| w.starts_with("unresolved|")).collect();
        assert_eq!(warned.len(), 3, "{warned:?}");
        for key in ["unresolved|Typos|Group Name|Fleat", "unresolved|Typos|Point Set Name|Spt", "unresolved|Gate|Volume Name|Aera"] {
            assert!(s.warned.contains(key), "{key} not in {warned:?}");
        }
        s.step(&[fleet()], 0.1, false);
        assert_eq!(s.warned.iter().filter(|w| w.starts_with("unresolved|")).count(), 3, "warned once");
    }

    #[test]
    fn the_trace_reports_the_false_conditions_when_they_change() {
        let mut s = script();
        s.diag.all = true;
        s.step(&[fleet()], 0.1, false);
        assert_eq!(s.diag.last.get(&1), Some(&vec![0, 1]), "the flag and the volume are both false");
        s.flags.insert("Go".into(), true);
        s.step(&[fleet()], 0.1, false);
        assert_eq!(s.diag.last.get(&1), Some(&vec![1]), "the flag now holds");
        let text = s.explain(&s.rules[1].conds[0], &[fleet()]);
        assert!(text.contains("flag 'Go' is TRUE"), "{text}");
        let text = s.explain(&s.rules[1].conds[1], &[fleet()]);
        assert!(text.contains("group 'Fleet' has 1 members, 1 alive") && text.contains("no volume 'Aera'"), "{text}");
    }

    #[test]
    fn why_picks_rules_by_name() {
        let names: Vec<String> = ["Objective 1", "Objective 10", "Fail Mission"].map(String::from).to_vec();
        let (hit, none) = resolve_why(&["Objective 1".into(), "fail".into(), "nope".into()], &names);
        assert_eq!(hit, HashSet::from([0, 2]));
        assert_eq!(none, vec!["nope".to_string()]);
    }

    #[test]
    fn the_trace_says_why_a_rule_is_inactive_or_busy() {
        let data = json!({"rules": [
            {"name": "sleeper", "Run Once": true, "Is Active": false, "conditions": [], "actions": []},
            {"name": "busy", "Run Once": false, "Is Active": true, "conditions": [], "actions": [{"Type": "OP Wait", "Seconds": 5}]},
        ]});
        let mut s = Script::new(&data, Value::Null);
        s.diag.all = true;
        s.step(&[], 0.1, false);
        s.step(&[], 0.1, false);
        assert!(s.diag.state.get(&0).is_some_and(|n| n.contains("inactive")), "{:?}", s.diag.state);
        assert!(s.diag.state.get(&1).is_some_and(|n| n.contains("busy") && n.contains("OP Wait")), "{:?}", s.diag.state);
        let nested = json!({"Type": "OP Any Of", "Conditions": [
            {"Type": "OP Counter Condition", "Counter Name": "c", "Equivalence": "Greater Than", "Value": 1},
            {"Type": "Flag Condition", "Flag Name": "f", "Boolean Value": "TRUE"}]});
        let text = s.explain(&nested, &[]);
        assert!(text.contains("counter 'c' reads 0") && text.contains("flag 'f' is FALSE"), "{text}");
    }
}
