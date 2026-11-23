//! `--check-mission <map id>`: validates a mission's rules against the rule schema (`eval/schema.rs`) and the mission's own
//! definitions (groups, volumes, paths, tasks, points, point sets, map text, media), without starting the game.
//!
//! The interpreter treats a bad rule quietly: a misspelled group has no members so `Group Destroyed` never holds, a missing field reads
//! as "" or 0, a missing volume is an empty region, a JSON `true` reads as FALSE. Each of those is a problem here, one line each
//! with the rule name and the field. Errors are rules that cannot do what they say; warnings are suspicious but harmless, and the
//! oddities the retail missions carry themselves.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;

use super::schema::{Field, Kind, RULE_TYPES, Role, TypeDef, lookup};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub level: Level,
    pub rule: String,
    /// "condition #2 'Flag Condition'" / "action #5 'Start Timer'" / "rule".
    pub at: String,
    pub field: String,
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = if self.level == Level::Error { "ERROR" } else { "warning" };
        write!(f, "{level}: rule \"{}\": {}", self.rule, self.at)?;
        if !self.field.is_empty() {
            write!(f, ", field \"{}\"", self.field)?;
        }
        write!(f, ": {}", self.message)
    }
}

/// What a mission defines, collected from `mission.json` (or a skirmish map's `script.json`), `map.json` and `media.json`.
#[derive(Default)]
struct Defs {
    /// TWT groups by name, with their member count.
    groups: BTreeMap<String, usize>,
    players: BTreeSet<String>,
    /// Names a bare group spec may use besides the TWT groups and players: the owners of placed ships, nebula and animal groups.
    other_groups: BTreeSet<String>,
    units: BTreeSet<String>,
    paths: BTreeSet<String>,
    volumes: BTreeMap<String, usize>,
    tasks: BTreeSet<String>,
    /// The mission's rule names, trimmed (`OP Set Rule Active`).
    rules: BTreeSet<String>,
    points: BTreeSet<String>,
    point_sets: BTreeSet<String>,
    map_text: BTreeSet<String>,
    speech: BTreeSet<String>,
    messages: BTreeSet<String>,
    ship_names: BTreeSet<String>,
    /// The map's team name ids (`map.json` `teams`); empty without a map.json.
    teams: BTreeSet<String>,
    /// `media.json` was given, so speech, messages and ship names can be checked.
    media: bool,
}

/// Every name a group spec of this mission can mean: its TWT groups, players, the groups and names of placed ships and of the
/// environment (animals, dragons, nebulae). The interpreter's start-of-mission audit warns about specs that are none of these.
pub fn known_names(mission: &Value) -> HashSet<String> {
    let d = Defs::collect(mission, &Value::Null, &Value::Null);
    d.groups.into_keys().chain(d.players).chain(d.other_groups).chain(d.units).collect()
}

fn names(v: &Value, key: &str) -> BTreeSet<String> {
    match &v[key] {
        Value::Object(m) => m.keys().cloned().collect(),
        Value::Array(a) => a.iter().filter_map(|x| x.as_str().or_else(|| x["name"].as_str()).or_else(|| x["Name"].as_str())).map(String::from).collect(),
        _ => BTreeSet::new(),
    }
}

impl Defs {
    fn collect(mission: &Value, map: &Value, media: &Value) -> Defs {
        let mut d = Defs {
            players: names(mission, "players"),
            paths: names(mission, "paths"),
            tasks: mission["tasks"].as_array().into_iter().flatten().filter_map(|t| t["Name"].as_str()).map(String::from).collect(),
            rules: mission["rules"].as_array().into_iter().flatten().filter_map(|r| r["name"].as_str()).map(|n| n.trim().to_string()).collect(),
            points: mission["points"].as_array().into_iter().flatten().filter_map(|t| t["Name"].as_str()).map(String::from).collect(),
            point_sets: names(&mission["environment"], "point_sets"),
            map_text: names(mission, "map_text"),
            speech: names(media, "speech").into_iter().chain(names(mission, "opening_speech")).collect(),
            messages: names(media, "messages"),
            ship_names: names(media, "ship_names"),
            media: media.is_object(),
            ..Default::default()
        };
        d.players.extend(names(map, "players"));
        d.teams = map["teams"].as_array().into_iter().flatten().filter_map(|t| t["name_id"].as_str()).map(String::from).collect();
        // The interpreter reads the human player under its TWT name or as "Player0 Group" (`alias_player`).
        d.players.insert("Player0 Group".into());
        d.players.extend(mission["player_start"]["name"].as_str().map(String::from));
        for (name, ids) in mission["groups"].as_object().into_iter().flatten() {
            d.groups.insert(name.clone(), ids.as_array().map_or(0, Vec::len));
        }
        for (name, v) in mission["volumes"].as_object().into_iter().flatten() {
            d.volumes.insert(name.clone(), v.as_array().map_or(0, Vec::len));
        }
        for ship in mission["ships"].as_array().into_iter().flatten() {
            d.units.extend(ship["name"].as_str().map(String::from));
            d.other_groups.extend(ship["group"].as_str().map(String::from));
        }
        // Groups of the environment (animals, dragons) and of the nebula setups, by any `group` / nebula name they carry.
        collect_strings(&mission["environment"], &mut d.other_groups);
        for r in mission["rules"].as_array().into_iter().flatten() {
            for a in r["actions"].as_array().into_iter().flatten() {
                match a["Type"].as_str() {
                    Some("*State Init* Setup Ship") => {
                        d.units.extend(a["Ship Name"].as_str().filter(|n| !n.is_empty()).map(String::from));
                        d.other_groups.extend(a["Player/Owner"].as_str().map(String::from));
                    }
                    Some("*State Init* Setup Nebula") => {
                        d.other_groups.extend(["New Nebula Name", "Polygon Name"].iter().filter_map(|k| a[*k].as_str()).map(String::from));
                    }
                    _ => {}
                }
            }
        }
        d
    }

    fn group_known(&self, name: &str) -> bool {
        self.groups.contains_key(name) || self.players.contains(name) || self.other_groups.contains(name)
    }
}

/// Every string stored under a `group` or `name` key anywhere below `v` (the environment's animals, dragons and nebulae).
fn collect_strings(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                match (k.as_str(), x.as_str()) {
                    ("group" | "name" | "New Nebula Name" | "Polygon Name", Some(s)) => {
                        out.insert(s.to_string());
                    }
                    _ => collect_strings(x, out),
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_strings(x, out)),
        _ => {}
    }
}

/// Edit distance with adjacent transpositions counting once ("Aera" is one edit from "Area").
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![(0..=b.len()).collect::<Vec<usize>>(); a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// " (did you mean 'x'?)" for the nearest candidate: the same name in another case, or a name a couple of edits away.
fn suggest<'a>(name: &str, candidates: impl Iterator<Item = &'a str>) -> String {
    let best = candidates
        .filter(|c| c.eq_ignore_ascii_case(name) || (name.len() > 3 && distance(&name.to_lowercase(), &c.to_lowercase()) <= (name.len() / 6).clamp(1, 3)))
        .min_by_key(|c| distance(name, c));
    best.map(|c| format!(" (did you mean '{c}'?)")).unwrap_or_default()
}

struct Checker<'a> {
    defs: &'a Defs,
    /// Checking a skirmish map's `script.json`.
    skirmish: bool,
    out: Vec<Problem>,
    rule: String,
    at: String,
    /// (name, rule) of every flag read as TRUE / FALSE, timer read, and the names written.
    flags_set: HashSet<String>,
    timers_started: HashSet<String>,
    reads: Vec<(Kind, String, bool, String, String)>,
    /// The condition being checked asks for TRUE (its 'Boolean Value').
    wants_true: bool,
    /// The field being checked is a `soft` reference.
    soft: bool,
    /// Prefix of `at` while checking the conditions nested in an `OP Any Of` / `OP Not`.
    parent: String,
    /// Inside a combinator: a name that is never set or started is a warning, not an error (the other branch, or the negation, may be intended).
    lenient: bool,
    /// Counters written by 'OP Set Counter' / 'OP Add To Counter'.
    counters_set: HashSet<String>,
}

impl Checker<'_> {
    fn push(&mut self, level: Level, field: &str, message: String) {
        self.out.push(Problem { level, rule: self.rule.clone(), at: self.at.clone(), field: field.into(), message });
    }
    fn err(&mut self, field: &str, message: String) {
        let level = if self.soft { Level::Warning } else { Level::Error };
        self.push(level, field, message);
    }
    fn warn(&mut self, field: &str, message: String) {
        self.push(Level::Warning, field, message);
    }

    fn entry(&mut self, role: Role, index: usize, e: &Value) {
        let kind_word = if role == Role::Condition { "condition" } else { "action" };
        let Some(ty) = e["Type"].as_str() else {
            self.at = format!("{}{kind_word} #{}", self.parent, index + 1);
            self.err("Type", "has no Type".into());
            return;
        };
        self.at = format!("{}{kind_word} #{} '{ty}'", self.parent, index + 1);
        let Some(def) = lookup(role, ty) else {
            let other = if role == Role::Condition { Role::Action } else { Role::Condition };
            let hint = if lookup(other, ty).is_some() {
                format!(
                    " ('{ty}' is {} Type, not {} one)",
                    if other == Role::Action { "an action" } else { "a condition" },
                    if role == Role::Action { "an action" } else { "a condition" }
                )
            } else {
                suggest(ty, RULE_TYPES.iter().filter(|t| t.role == role).map(|t| t.name))
            };
            self.err("Type", format!("unknown {kind_word} Type{hint}; the interpreter ignores it"));
            return;
        };
        if def.campaign_only && self.skirmish {
            self.err("Type", format!("'{ty}' is campaign-only: only a campaign mission shows its effect, so a skirmish map script cannot use it"));
        }
        let Some(obj) = e.as_object() else { return };
        self.wants_true = e["Boolean Value"].as_str().is_none_or(|v| !v.eq_ignore_ascii_case("FALSE"));
        if def.name == "OP Counter Condition" {
            // A counter nobody sets reads 0: the condition needs a writer unless 0 already satisfies it.
            let value = e["Value"].as_f64().unwrap_or(0.0);
            let holds_at_zero = match e["Equivalence"].as_str() {
                Some("Greater Than") => 0.0 > value,
                Some("Less Than") => 0.0 < value,
                Some("Equal To") => value.abs() < 1e-4,
                _ => false,
            };
            self.wants_true = !holds_at_zero;
        }
        for key in obj.keys().filter(|k| k.as_str() != "Type") {
            if def.field(key).is_none() {
                let hint = suggest(key, def.fields.iter().map(|f| f.name));
                self.err(key, format!("unknown field for '{ty}'{hint}; it is ignored"));
            }
        }
        for field in def.fields {
            match obj.get(field.name) {
                None if field.required => self.err(field.name, format!("missing; reads as {}", describe_default(field))),
                None => {}
                Some(v) if field.used => {
                    self.value(def, field, v, role);
                    self.soft = false;
                }
                Some(_) => {}
            }
        }
    }

    fn value(&mut self, def: &TypeDef, field: &Field, v: &Value, role: Role) {
        let name = field.name;
        self.soft = field.soft;
        if matches!(field.kind, Kind::ConditionList | Kind::Condition) {
            let list: Vec<&Value> = match v {
                Value::Array(a) if field.kind == Kind::ConditionList => a.iter().collect(),
                Value::Object(_) if field.kind == Kind::Condition => vec![v],
                _ => {
                    let want = if field.kind == Kind::ConditionList { "a list of condition objects" } else { "one condition object" };
                    self.err(name, format!("{v} is not {want}"));
                    return;
                }
            };
            if list.is_empty() {
                self.warn(name, "is an empty list; it is never true".into());
            }
            let saved = (self.at.clone(), self.parent.clone(), self.wants_true, self.lenient);
            (self.parent, self.lenient) = (format!("{}, nested ", self.at), true);
            for (i, e) in list.into_iter().enumerate() {
                self.entry(Role::Condition, i, e);
            }
            (self.at, self.parent, self.wants_true, self.lenient) = saved;
            self.soft = false;
            return;
        }
        if let Kind::Bool = field.kind {
            if v.is_boolean() {
                self.err(name, format!("the JSON boolean {v} is not allowed; write the string \"{}\" (a JSON boolean reads as FALSE)", if v == true { "TRUE" } else { "FALSE" }));
            } else if !v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("TRUE") || s.eq_ignore_ascii_case("FALSE")) {
                self.err(name, format!("{v} is not \"TRUE\" or \"FALSE\"; it reads as FALSE"));
            }
            return;
        }
        if let Kind::Number = field.kind {
            let Some(n) = v.as_f64() else {
                self.err(name, format!("{v} is not a number; it reads as 0"));
                return;
            };
            if let Some((lo, hi)) = field.range
                && !(f64::from(lo)..=f64::from(hi)).contains(&n)
            {
                self.err(name, format!("{n} is outside {lo}..{hi} (a fraction, not a percentage)"));
            }
            return;
        }
        let Some(text) = v.as_str() else {
            self.err(name, format!("{v} is not a string; it reads as \"\""));
            return;
        };
        if text.is_empty() {
            if field.required && !matches!(field.kind, Kind::Text) {
                self.warn(name, "is empty; it matches nothing".into());
            }
            return;
        }
        if field.kind.placeholders().contains(&text) {
            return;
        }
        let d = self.defs;
        match field.kind {
            Kind::Enum(values) if !values.contains(&text) => {
                let hint = suggest(text, values.iter().copied());
                self.err(name, format!("'{text}' is not one of {values:?}{hint}"));
            }
            Kind::Group => self.group(name, text, def, role),
            Kind::Player if !d.group_known(text) => {
                let hint = suggest(text, d.players.iter().chain(d.groups.keys()).map(String::as_str));
                self.err(name, format!("unknown player '{text}'{hint}"));
            }
            Kind::Path if !d.paths.contains(text) => {
                let hint = suggest(text, d.paths.iter().map(String::as_str));
                self.err(name, format!("unknown path '{text}'{hint}"));
            }
            Kind::Volume => match d.volumes.get(text) {
                None => {
                    let hint = suggest(text, d.volumes.keys().map(String::as_str));
                    self.err(name, format!("unknown volume '{text}'{hint}; it is an empty region that nothing is ever inside"));
                }
                Some(n) if *n < 3 => self.err(name, format!("volume '{text}' has {n} vertices; fewer than 3 is an empty region")),
                Some(_) => {}
            },
            Kind::Task if !d.tasks.contains(text) => {
                let hint = suggest(text, d.tasks.iter().map(String::as_str));
                self.err(name, format!("unknown objective task '{text}'{hint}"));
            }
            Kind::Point if !d.points.contains(text) => {
                let hint = suggest(text, d.points.iter().map(String::as_str));
                self.err(name, format!("unknown objective point '{text}'{hint}"));
            }
            Kind::PointSet if !d.point_sets.contains(text) => {
                let hint = suggest(text, d.point_sets.iter().map(String::as_str));
                self.err(name, format!("unknown point set '{text}'{hint}"));
            }
            Kind::MapText if !d.map_text.contains(text) => {
                let hint = suggest(text, d.map_text.iter().map(String::as_str));
                self.err(name, format!("unknown map text '{text}'{hint}"));
            }
            Kind::Speech if d.media && !d.speech.contains(text) => {
                let hint = suggest(text, d.speech.iter().map(String::as_str));
                self.warn(name, format!("speech event '{text}' has no media entry (no text, no sound){hint}"));
            }
            Kind::Message if d.media && !d.messages.contains(text) => self.warn(name, format!("message '{text}' has no media entry; the game falls back to a default")),
            Kind::ShipName if d.media && !d.ship_names.contains(text) => self.warn(name, format!("ship name '{text}' has no media entry; the rule does nothing")),
            Kind::Flag => {
                if role == Role::Action {
                    self.flags_set.insert(text.into());
                } else {
                    self.reads.push((Kind::Flag, text.into(), self.wants_true && !self.lenient, self.rule.clone(), self.at.clone()));
                }
            }
            Kind::Counter => {
                if role == Role::Action {
                    self.counters_set.insert(text.into());
                } else {
                    self.reads.push((Kind::Counter, text.into(), self.wants_true && !self.lenient, self.rule.clone(), self.at.clone()));
                }
            }
            Kind::Rule if !d.rules.contains(text.trim()) => {
                let hint = suggest(text, d.rules.iter().map(String::as_str));
                self.err(name, format!("unknown rule '{text}'{hint}; nothing is activated"));
            }
            Kind::Timer => {
                if def.name == "Start Timer" {
                    self.timers_started.insert(text.into());
                } else if role == Role::Condition {
                    self.reads.push((Kind::Timer, text.into(), !self.lenient, self.rule.clone(), self.at.clone()));
                }
            }
            _ => {}
        }
    }

    fn group(&mut self, field: &str, spec: &str, def: &TypeDef, role: Role) {
        let d = self.defs;
        let (group, unit) = spec.split_once(',').map_or((spec, None), |(g, u)| (g, Some(u)));
        if !d.group_known(group) {
            let hint = suggest(group, d.groups.keys().chain(&d.players).chain(&d.other_groups).map(String::as_str));
            self.err(field, format!("unknown group '{group}'{hint}; it has no members"));
        } else if role == Role::Condition && unit.is_none() && d.groups.get(group) == Some(&0) && !d.players.contains(group) {
            self.warn(field, format!("group '{group}' is defined with no members (in '{}')", def.name));
        }
        if let Some(unit) = unit
            && unit != "HUMAN CONTROLLED COMMAND SHIP"
            && !d.units.contains(unit)
        {
            let hint = suggest(unit, d.units.iter().map(String::as_str));
            self.warn(field, format!("unit '{unit}' is not the name of any ship the mission places{hint}"));
        }
    }
}

fn describe_default(f: &Field) -> String {
    match f.kind {
        Kind::Bool | Kind::Number => f.default.to_string(),
        _ => format!("\"{}\"", f.default),
    }
}

/// Not retail: the engine-added mission keys, a task's `complete_when` / `fail_when` / `required`, and `victory`,
/// `victory_message`, `defeat_message`.
fn check_tasks_and_victory(c: &mut Checker, mission: &Value) {
    let mut required = false;
    for (i, t) in mission["tasks"].as_array().into_iter().flatten().enumerate() {
        let name = t["Name"].as_str().map_or_else(|| format!("#{}", i + 1), String::from);
        c.at = "task".into();
        match &t["required"] {
            Value::Null | Value::Bool(_) => required |= t["required"] == true,
            other => {
                c.rule = format!("task '{name}'");
                c.err("required", format!("{other} is not true/false; the task is not required"));
            }
        }
        // Engine-added `team` (skirmish script): the team a required task decides for.
        if !t["team"].is_null() {
            c.rule = format!("task '{name}'");
            match t["team"].as_str() {
                None => c.err("team", format!("{} is not a team name id (a string)", t["team"])),
                Some(team) if !c.skirmish => c.warn("team", format!("'{team}' is ignored outside a skirmish map script")),
                Some(team) if !c.defs.teams.is_empty() && !c.defs.teams.contains(team) => c.warn("team", format!("no team '{team}' in map.json")),
                Some(_) => {}
            }
        } else if c.skirmish && t["required"] == true {
            c.rule = format!("task '{name}'");
            c.err("team", "a required task of a skirmish script must name its \"team\": without one it never decides the game".into());
        }
        for key in ["complete_when", "fail_when"] {
            if t[key].is_null() {
                continue;
            }
            c.rule = format!("task '{name}' {key}");
            let Some(list) = t[key].as_array() else {
                c.err(key, format!("{} is not a list of condition objects", t[key]));
                continue;
            };
            if list.is_empty() {
                c.warn(key, "is an empty list; it never holds".into());
            }
            c.parent.clear();
            for (j, e) in list.iter().enumerate() {
                c.entry(Role::Condition, j, e);
            }
        }
    }
    c.rule = "(mission)".into();
    c.at = "mission".into();
    let victory = &mission["victory"];
    let objectives = victory.as_str() == Some("objectives");
    // A skirmish script may carry a `victory` object of its own; only a plain value is the engine-added mode.
    let script_decides = c.skirmish && victory.as_str() == Some("script");
    if !victory.is_null() && !victory.is_object() && !script_decides {
        if !objectives {
            let want = if c.skirmish { "\"objectives\" or \"script\"" } else { "\"objectives\" (\"script\" is for a skirmish map's script.json)" };
            c.err("victory", format!("{victory} is not {want}; the rules decide how the game ends"));
        } else if !required {
            c.warn("victory", "no task has \"required\": true, so the mission never ends this way".into());
        }
    }
    for key in ["victory_message", "defeat_message"] {
        let v = &mission[key];
        if v.is_null() {
            continue;
        }
        match v.as_str() {
            None => c.err(key, format!("{v} is not a message id (a string)")),
            Some(id) => {
                if c.defs.media && !c.defs.messages.contains(id) {
                    c.warn(key, format!("message '{id}' has no media entry; the game falls back to a default"));
                }
            }
        }
        if !objectives {
            c.warn(key, "is ignored unless \"victory\" is \"objectives\"".into());
        }
    }
}

/// Validates the rules in `mission` (a `mission.json` / `script.json` document) against the schema and its own definitions.
/// `map` is the map's `map.json` and `media` its `media.json`; either may be `Value::Null`.
pub fn check(mission: &Value, map: &Value, media: &Value) -> Vec<Problem> {
    check_with(mission, map, media, false)
}

/// `check`, for a skirmish map's `script.json` when `skirmish`: a campaign-only Type (`TypeDef::campaign_only`) is an error there.
pub fn check_with(mission: &Value, map: &Value, media: &Value, skirmish: bool) -> Vec<Problem> {
    let defs = Defs::collect(mission, map, media);
    let mut c = Checker {
        defs: &defs,
        skirmish,
        out: vec![],
        rule: String::new(),
        at: "rule".into(),
        flags_set: HashSet::new(),
        timers_started: HashSet::new(),
        reads: vec![],
        wants_true: true,
        soft: false,
        parent: String::new(),
        lenient: false,
        counters_set: HashSet::new(),
    };
    let Some(rules) = mission["rules"].as_array() else {
        c.rule = "(mission)".into();
        c.err("rules", "the mission has no rules list".into());
        return c.out;
    };
    let mut seen = HashSet::new();
    for (i, r) in rules.iter().enumerate() {
        c.rule = r["name"].as_str().map_or_else(|| format!("#{}", i + 1), |n| n.trim().to_string());
        c.at = "rule".into();
        if r["name"].as_str().is_none_or(str::is_empty) {
            c.warn("name", "the rule has no name (--fire-rule and --why cannot address it)".into());
        } else if !seen.insert(c.rule.clone()) {
            c.warn("name", "another rule has the same name (--fire-rule and --why take the first)".into());
        }
        let active = &r["Is Active"];
        if !active.is_null() && !active.is_boolean() && !active.as_str().is_some_and(|x| x.eq_ignore_ascii_case("TRUE") || x.eq_ignore_ascii_case("FALSE")) {
            c.warn("Is Active", format!("{active} is not true/false; the rule is active"));
        }
        if !r["Run Once"].is_null() && !r["Run Once"].is_boolean() {
            c.warn("Run Once", format!("{} is not true/false; the rule runs once", r["Run Once"]));
        }
        let conds = r["conditions"].as_array().map_or(&[][..], Vec::as_slice);
        let acts = r["actions"].as_array().map_or(&[][..], Vec::as_slice);
        if acts.is_empty() {
            c.warn("actions", "the rule has no actions".into());
        }
        if conds.is_empty() {
            c.warn("conditions", "the rule has no conditions; it fires on the first step".into());
        }
        for (i, e) in conds.iter().enumerate() {
            c.entry(Role::Condition, i, e);
        }
        for (i, e) in acts.iter().enumerate() {
            c.entry(Role::Action, i, e);
        }
    }
    check_tasks_and_victory(&mut c, mission);
    let (flags, timers, counters) = (std::mem::take(&mut c.flags_set), std::mem::take(&mut c.timers_started), std::mem::take(&mut c.counters_set));
    for (kind, name, wants_true, rule, at) in std::mem::take(&mut c.reads) {
        c.rule = rule;
        c.at = at;
        match kind {
            Kind::Flag if !flags.contains(&name) => {
                let hint = suggest(&name, flags.iter().map(String::as_str));
                if wants_true {
                    c.err("Flag Name", format!("flag '{name}' is never set, so this can never hold{hint}"));
                } else {
                    c.warn("Flag Name", format!("flag '{name}' is never set (always FALSE){hint}"));
                }
            }
            Kind::Counter if !counters.contains(&name) => {
                let hint = suggest(&name, counters.iter().map(String::as_str));
                if wants_true {
                    c.err("Counter Name", format!("counter '{name}' is never set (it reads 0), so this can never hold{hint}"));
                } else {
                    c.warn("Counter Name", format!("counter '{name}' is never set (it always reads 0){hint}"));
                }
            }
            Kind::Timer if !timers.contains(&name) => {
                let hint = suggest(&name, timers.iter().map(String::as_str));
                c.err("Timer Name", format!("timer '{name}' is never started, so this can never hold{hint}"));
            }
            _ => {}
        }
    }
    c.out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mission(rules: Value) -> Value {
        json!({
            "players": ["Player0 Group", "Pirates"],
            "groups": {"Fleet": [1, 2], "Empty": []},
            "ships": [{"id": 1, "name": "RLS Hawk", "group": "Player0 Group"}],
            "paths": {"Route": [[0.0, 0.0, 0.0]]},
            "volumes": {"Area": [[0, 0], [10, 0], [10, 10]], "Dot": [[0, 0]]},
            "tasks": [{"Name": "Task A"}],
            "points": [{"Name": "Point A"}],
            "environment": {"point_sets": {"Set A": []}},
            "map_text": [{"name": "Label"}],
            "rules": rules,
        })
    }
    fn rule(conds: Value, acts: Value) -> Value {
        json!({"name": "R", "Run Once": true, "conditions": conds, "actions": acts})
    }
    fn problems(rules: Value) -> Vec<Problem> {
        check(&mission(rules), &Value::Null, &Value::Null)
    }
    fn text(rules: Value) -> String {
        problems(rules).iter().map(ToString::to_string).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn a_correct_rule_passes() {
        let r = rule(
            json!([{"Type": "Group Destroyed", "Group Name": "Fleet"}, {"Type": "Timer Condition", "Timer Name": "T", "Equivalence": "Greater Than", "Time in seconds": 5}]),
            json!([
                {"Type": "Start Timer", "Timer Name": "T"},
                {"Type": "Group to follow path", "Group Name": "Fleet", "Path Name": "Route", "Find Closest Point": "TRUE", "Follow Mode": "loop"},
                {"Type": "Set Group/Unit Vulnerability", "Group Name": "Fleet,RLS Hawk", "Invulnerable Percent": 0.5},
            ]),
        );
        assert_eq!(problems(json!([r])), vec![]);
    }

    #[test]
    fn unknown_type_field_and_missing_field() {
        let t = text(json!([rule(
            json!([{"Type": "Group Destoyed", "Group Name": "Fleet"}]),
            json!([{"Type": "Start Timer", "Timer Nam": "T"}, {"Type": "Group Destroyed", "Group Name": "Fleet"}]),
        )]));
        assert!(t.contains("unknown condition Type (did you mean 'Group Destroyed'?)"), "{t}");
        assert!(t.contains("field \"Timer Nam\": unknown field for 'Start Timer' (did you mean 'Timer Name'?)"), "{t}");
        assert!(t.contains("field \"Timer Name\": missing; reads as \"\""), "{t}");
        assert!(t.contains("'Group Destroyed' is a condition Type, not an action one"), "{t}");
    }

    #[test]
    fn references_the_mission_does_not_define() {
        let t = text(json!([rule(
            json!([
                {"Type": "Enter Volume", "Group Name": "Fleat", "Volume Name": "Aera"},
                {"Type": "Enter Volume", "Group Name": "Fleet", "Volume Name": "Dot"},
                {"Type": "Group Destroyed", "Group Name": "Empty"},
            ]),
            json!([
                {"Type": "Group to follow path", "Group Name": "Fleet", "Path Name": "Rout", "Find Closest Point": "TRUE", "Follow Mode": "loop"},
                {"Type": "Set Objective Task Active State", "Objective Task": "Task B", "Active State": "TRUE"},
                {"Type": "Set Current Objective Point", "Objective Point": "Point B"},
                {"Type": "Teleport Group/Unit", "Group/Unit Name": "Fleet", "Point Set Name": "Set B"},
                {"Type": "Set Map Text Visibility", "Map Text Name": "Lable", "Visible On Starmap": "TRUE"},
                {"Type": "Set Group/Unit Owner", "Group/Unit Name": "Fleet,RLS Hwak", "New Owner": "Pirate"},
                {"Type": "Set Current Objective Point", "Objective Point": "NO OBJECTIVE POINT"},
            ])
        )]));
        for want in [
            "unknown group 'Fleat' (did you mean 'Fleet'?)",
            "unknown volume 'Aera' (did you mean 'Area'?)",
            "volume 'Dot' has 1 vertices",
            "group 'Empty' is defined with no members",
            "unknown path 'Rout' (did you mean 'Route'?)",
            "unknown objective task 'Task B'",
            "unknown objective point 'Point B'",
            "unknown point set 'Set B'",
            "unknown map text 'Lable'",
            "unit 'RLS Hwak' is not the name of any ship the mission places (did you mean 'RLS Hawk'?)",
            "unknown player 'Pirate' (did you mean 'Pirates'?)",
        ] {
            assert!(t.contains(want), "missing {want:?} in\n{t}");
        }
        assert!(!t.contains("NO OBJECTIVE POINT"), "{t}");
    }

    #[test]
    fn values_that_the_interpreter_misreads() {
        let t = text(json!([rule(
            json!([{"Type": "Flag Condition", "Flag Name": "F", "Boolean Value": true}]),
            json!([
                {"Type": "Set Flag Action", "Flag Name": "F", "Boolean Value": false},
                {"Type": "Damage Group/Unit by X percent", "Group Name": "Fleet", "Percent": 50},
                {"Type": "Set Group Throttle Percent", "Group Name": "Fleet", "Throttle Percent": "0.5"},
                {"Type": "Set Group/Unit AI Stance", "Group/Unit Name": "Fleet", "AI Stance": "Stance_Agressive"},
            ])
        )]));
        for want in [
            "field \"Boolean Value\": the JSON boolean true is not allowed; write the string \"TRUE\"",
            "action #1 'Set Flag Action', field \"Boolean Value\": the JSON boolean false is not allowed; write the string \"FALSE\"",
            "50 is outside 0..1",
            "\"0.5\" is not a number; it reads as 0",
            "'Stance_Agressive' is not one of",
        ] {
            assert!(t.contains(want), "missing {want:?} in\n{t}");
        }
    }

    #[test]
    fn flags_and_timers_read_but_never_set() {
        let t = text(json!([
            rule(
                json!([{"Type": "Flag Condition", "Flag Name": "Done", "Boolean Value": "TRUE"}, {"Type": "Timer Condition", "Timer Name": "Clock", "Equivalence": "Less Than", "Time in seconds": 1}]),
                json!([{"Type": "Set Flag Action", "Flag Name": "Dnoe", "Boolean Value": "TRUE"}]),
            ),
            json!({"name": "S", "conditions": [{"Type": "Flag Condition", "Flag Name": "Idle", "Boolean Value": "FALSE"}], "actions": [{"Type": "Stop Timer", "Timer Name": "Clock"}]}),
        ]));
        assert!(t.contains("flag 'Done' is never set, so this can never hold (did you mean 'Dnoe'?)"), "{t}");
        assert!(t.contains("timer 'Clock' is never started"), "{t}");
        assert!(t.contains("warning: rule \"S\": condition #1 'Flag Condition', field \"Flag Name\": flag 'Idle' is never set (always FALSE)"), "{t}");
    }

    #[test]
    fn media_references_are_checked_only_with_media() {
        let m = mission(json!([rule(
            json!([{"Type": "Speech Event Played Once", "Speech Event Name": "Hello"}]),
            json!([{"Type": "Play speech event", "Speech Event Name": "Hello"}])
        )]));
        assert_eq!(check(&m, &Value::Null, &Value::Null), vec![]);
        let with = check(&m, &Value::Null, &json!({"speech": {"Other": {}}, "messages": {}, "ship_names": {}}));
        assert_eq!(with.len(), 2);
        assert!(with[0].message.contains("has no media entry"));
        assert_eq!(check(&m, &Value::Null, &json!({"speech": {"Hello": {}}})), vec![]);
    }

    #[test]
    fn campaign_only_types_are_errors_in_a_skirmish_script_only() {
        let m = mission(json!([rule(json!([{"Type": "World Initialize"}]), json!([{"Type": "NIS Start"}, {"Type": "Open Weapon Bar", "OpenState": "TRUE"}]))]));
        let skirmish: Vec<String> = check_with(&m, &Value::Null, &Value::Null, true).iter().filter(|p| p.level == Level::Error).map(ToString::to_string).collect();
        assert_eq!(skirmish.len(), 2, "{skirmish:?}");
        assert!(skirmish[0].contains("action #1 'NIS Start', field \"Type\": 'NIS Start' is campaign-only"), "{skirmish:?}");
        assert_eq!(check(&m, &Value::Null, &Value::Null), vec![], "a campaign mission may use them");
    }

    #[test]
    fn a_skirmish_script_may_decide_its_game_and_its_required_tasks_name_a_team() {
        let mut m = mission(json!([]));
        m["victory"] = json!("script");
        m["tasks"] = json!([{"Name": "Task A", "required": true}, {"Name": "Task B", "required": true, "team": "NAVY"}]);
        let map = json!({"teams": [{"name_id": "NAVY"}]});
        let text = |skirmish| check_with(&m, &map, &Value::Null, skirmish).iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
        let t = text(true);
        assert!(t.contains("task 'Task A'") && t.contains("must name its \"team\""), "{t}");
        assert!(!t.contains("Task B") && !t.contains("\"script\" is not"), "{t}");
        assert!(text(false).contains("\"script\" is for a skirmish map's script.json"), "{}", text(false));
        m["tasks"][1]["team"] = json!("PIRATES");
        assert!(check_with(&m, &map, &Value::Null, true).iter().any(|p| p.message.contains("no team 'PIRATES' in map.json")));
    }

    /// docs/rule-types.md is generated from the schema (tools/gen_rule_types.py); a Type added without re-running it fails here.
    #[test]
    fn every_rule_type_is_documented() {
        let doc = include_str!("../../../docs/rule-types.md");
        let missing: Vec<&str> = RULE_TYPES.iter().map(|t| t.name).filter(|n| !doc.contains(&format!("`{n}`"))).collect();
        assert!(missing.is_empty(), "docs/rule-types.md does not document {missing:?}: run tools/gen_rule_types.py (see its docstring)");
    }
}
