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

use super::schema::{Equivalence, Field, Kind, Origin, RULE_TYPES, Role, TypeDef, lookup};

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warning,
}

/// One finding of `--check-mission` or `--check-mod`, as data (`--check-mod --json` prints these) and as the line the command prints
/// (`Display`). A rule finding names the rule, the list and the item; a finding about a plain JSON file names the file and the path.
/// Indices are 0-based (the printed line counts from 1). A field that does not apply is empty, `None` or an empty list.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Finding {
    pub level: Level,
    /// The file, relative to the folder checked (`maps/<id>/mission.json`, `ships/<Id>/ship.json`); empty when the checker was not told.
    pub file: String,
    /// Where in a non-rule file, as a JSON path (`guns.Gun_A.arc_deg`, `sections.Hull.layer`); empty for a rule finding.
    pub path: String,
    /// The same place as a JSON pointer (RFC 6901: `/guns/Gun_A/arc_deg`, `/engines/0/fx`), which a tool can follow without the
    /// ambiguity of the dotted `path` (a key may itself hold `.` or `[`, or be made of digits); empty for a rule finding.
    pub pointer: String,
    /// The rule's name as the printed line shows it: its trimmed name, `#N` for an unnamed rule, `task 'X'`, `(mission)`. Empty for a finding that is not about a mission.
    pub rule: String,
    /// Position of the rule in `rules` (0-based); `None` outside a rule.
    pub rule_index: Option<usize>,
    /// The rule's own `name`, when it has one.
    pub rule_name: Option<String>,
    /// `"conditions"` or `"actions"` (a task's own lists: `"complete_when"` / `"fail_when"`); `None` for the rule itself.
    pub list: Option<String>,
    /// Index of the condition or action in `list` (0-based); `None` when the finding is about the rule.
    pub item: Option<usize>,
    /// Further indices when the condition sits inside another's list (`OP Any Of`, `OP Not`): the path below `item`.
    pub nested: Vec<usize>,
    /// "condition #2 'Flag Condition'" / "action #5 'Start Timer'" / "rule", as printed.
    pub at: String,
    pub field: String,
    pub message: String,
    /// The name the message suggests ("did you mean"), also inside `message`.
    pub suggestion: Option<String>,
}

/// The name `--check-mission` has always used for a finding.
pub type Problem = Finding;

impl Finding {
    /// A finding with the printed pieces and nothing else; callers fill in the rest.
    pub fn new(level: Level, rule: impl Into<String>, at: impl Into<String>, field: impl Into<String>, message: impl Into<String>) -> Finding {
        Finding {
            level,
            file: String::new(),
            path: String::new(),
            pointer: String::new(),
            rule: rule.into(),
            rule_index: None,
            rule_name: None,
            list: None,
            item: None,
            nested: vec![],
            at: at.into(),
            field: field.into(),
            message: message.into(),
            suggestion: None,
        }
    }
    /// A finding about a place in a file that is not a mission (a ship, a gun, `mod.json`); `path` fills both `path` and `pointer`.
    pub fn in_file(level: Level, file: impl Into<String>, path: impl Into<JsonPath>, message: impl Into<String>) -> Finding {
        let path = path.into();
        Finding { file: file.into(), path: path.dotted(), pointer: path.pointer(), ..Finding::new(level, "", "", "", message) }
    }
    /// Sets `path` and `pointer` to `path`.
    pub fn set_path(&mut self, path: impl Into<JsonPath>) {
        let path = path.into();
        (self.path, self.pointer) = (path.dotted(), path.pointer());
    }
    /// The same finding in `file`.
    pub fn with_file(mut self, file: &str) -> Finding {
        self.file = file.into();
        self
    }
}

/// A place inside a JSON file as its keys and list positions, built where the place is known so that it is never parsed back from
/// text: [`JsonPath::dotted`] is the form the printed line shows (`guns.Gun_A.arc_deg`, `engines[0].fx`), [`JsonPath::pointer`] the
/// JSON pointer a tool follows (`/guns/Gun_A/arc_deg`, `/engines/0/fx`; `~` and `/` in a key escaped as `~0` and `~1`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JsonPath(pub Vec<Segment>);

/// One step of a [`JsonPath`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    Key(String),
    Index(usize),
}

impl JsonPath {
    /// The path one key further down.
    pub fn key(mut self, key: impl Into<String>) -> JsonPath {
        self.0.push(Segment::Key(key.into()));
        self
    }
    /// The path one list position further down.
    pub fn index(mut self, index: usize) -> JsonPath {
        self.0.push(Segment::Index(index));
        self
    }
    /// `guns.Gun_A.arc_deg`, `engines[0].fx`; empty for the whole file.
    pub fn dotted(&self) -> String {
        let mut out = String::new();
        for s in &self.0 {
            match s {
                Segment::Key(k) if out.is_empty() => out.push_str(k),
                Segment::Key(k) => {
                    out.push('.');
                    out.push_str(k);
                }
                Segment::Index(i) => out.push_str(&format!("[{i}]")),
            }
        }
        out
    }
    /// `/guns/Gun_A/arc_deg`, `/engines/0/fx`; empty for the whole file.
    pub fn pointer(&self) -> String {
        self.0
            .iter()
            .map(|s| match s {
                Segment::Key(k) => format!("/{}", k.replace('~', "~0").replace('/', "~1")),
                Segment::Index(i) => format!("/{i}"),
            })
            .collect()
    }
}

/// A single top-level key (`"format"`), or the whole file for `""`.
impl From<&str> for JsonPath {
    fn from(key: &str) -> JsonPath {
        if key.is_empty() { JsonPath::default() } else { JsonPath::default().key(key) }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = if self.level == Level::Error { "ERROR" } else { "warning" };
        if self.rule.is_empty() {
            // Not a mission: "<file>: <level>: <path>: <message>" (the file is printed by whoever prints the line).
            let at = if self.path.is_empty() { String::new() } else { format!("{}: ", self.path) };
            return write!(f, "{level}: {at}{}", self.message);
        }
        write!(f, "{level}: rule \"{}\": {}", self.rule, self.at)?;
        if !self.field.is_empty() {
            write!(f, ", field \"{}\"", self.field)?;
        }
        write!(f, ": {}", self.message)
    }
}

/// What a mission defines, collected from `mission.json` (or a skirmish map's `script.json`), `map.json` and `media.json`.
#[derive(Default)]
pub(crate) struct Defs {
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
    /// How many points each point set has (`OP Teleport`'s `Point Index`).
    point_set_len: BTreeMap<String, usize>,
    map_text: BTreeSet<String>,
    speech: BTreeSet<String>,
    messages: BTreeSet<String>,
    ship_names: BTreeSet<String>,
    /// The map's team name ids (`map.json` `teams`); empty without a map.json.
    teams: BTreeSet<String>,
    /// Flags, timers and counters the rules write (`Set Flag`, `Start Timer`, `OP Set Counter`, ...), by name.
    flags: BTreeSet<String>,
    timers: BTreeSet<String>,
    counters: BTreeSet<String>,
    campaign_counters: BTreeSet<String>,
    /// `media.json` was given, so speech, messages and ship names can be checked.
    media: bool,
}

/// What a mission defines, by the kind of rule field that takes the name (`group`, `player`, `path`, `volume`, `task`, `point`,
/// `point_set`, `timer`, `flag`, `counter`, `campaign_counter`, `rule`, `speech_event`, `map_text`, `message`, `team`, `ship_name`)
/// and `unit` (ships placed by name), each a sorted list. `mission` is `mission.json` (or `script.json`), `map` and `media` are
/// `map.json` and `media.json` (`Value::Null` when there is none). The same collection the checker validates names against: timers,
/// flags and counters are the ones a rule starts or sets.
pub fn defined_names(mission: &Value, map: &Value, media: &Value) -> BTreeMap<String, Vec<String>> {
    Defs::collect(mission, map, media).names_by_kind()
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

/// Calls `f(role, type, field, text)` for every string a rule of the mission writes in a field of the schema, nested conditions
/// (`OP Any Of`, `OP Not`) and the `complete_when` / `fail_when` lists of tasks included. Entries of an unknown Type are skipped.
pub(crate) fn visit_fields(mission: &Value, f: &mut dyn FnMut(Role, &TypeDef, &Field, &str)) {
    fn entry(role: Role, e: &Value, f: &mut dyn FnMut(Role, &TypeDef, &Field, &str)) {
        let Some(def) = e["Type"].as_str().and_then(|ty| lookup(role, ty)) else { return };
        for field in def.fields {
            match (&e[field.name], field.kind) {
                (Value::Array(list), Kind::ConditionList) => list.iter().for_each(|c| entry(Role::Condition, c, f)),
                (v @ Value::Object(_), Kind::Condition) => entry(Role::Condition, v, f),
                (Value::String(text), _) => f(role, def, field, text),
                _ => {}
            }
        }
    }
    for r in mission["rules"].as_array().into_iter().flatten() {
        r["conditions"].as_array().into_iter().flatten().for_each(|e| entry(Role::Condition, e, f));
        r["actions"].as_array().into_iter().flatten().for_each(|e| entry(Role::Action, e, f));
    }
    for t in mission["tasks"].as_array().into_iter().flatten() {
        for key in ["complete_when", "fail_when"] {
            t[key].as_array().into_iter().flatten().for_each(|e| entry(Role::Condition, e, f));
        }
    }
}

impl Defs {
    pub(crate) fn collect(mission: &Value, map: &Value, media: &Value) -> Defs {
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
        d.point_set_len = mission["environment"]["point_sets"].as_object().into_iter().flatten().map(|(k, v)| (k.clone(), v.as_array().map_or(0, Vec::len))).collect();
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
        // Names the rules bring into being: flags, timers, counters (the same fields the checker's "never set" test reads).
        visit_fields(mission, &mut |role, def, field, text| {
            let set = match field.kind {
                Kind::Flag => &mut d.flags,
                Kind::Counter => &mut d.counters,
                Kind::CampaignCounter => &mut d.campaign_counters,
                Kind::Timer if def.name == "Start Timer" => &mut d.timers,
                _ => return,
            };
            if role == Role::Action && !text.is_empty() {
                set.insert(text.to_string());
            }
        });
        d
    }

    /// Every name the mission defines, by the kind of rule field that takes it (`Kind::name`, e.g. `group`, `volume`, `flag`), plus
    /// `unit` (the ships it places by name). Sorted. `player` includes the always-present `Player0 Group`.
    pub(crate) fn names_by_kind(&self) -> BTreeMap<String, Vec<String>> {
        let list = |kind: &str, names: Vec<String>| (kind.to_string(), names);
        let set = |s: &BTreeSet<String>| s.iter().cloned().collect::<Vec<_>>();
        BTreeMap::from([
            list(Kind::Group.name(), self.groups.keys().cloned().collect()),
            list(Kind::Player.name(), set(&self.players)),
            list(Kind::Path.name(), set(&self.paths)),
            list(Kind::Volume.name(), self.volumes.keys().cloned().collect()),
            list(Kind::Task.name(), set(&self.tasks)),
            list(Kind::Point.name(), set(&self.points)),
            list(Kind::PointSet.name(), set(&self.point_sets)),
            list(Kind::Timer.name(), set(&self.timers)),
            list(Kind::Flag.name(), set(&self.flags)),
            list(Kind::Counter.name(), set(&self.counters)),
            list(Kind::CampaignCounter.name(), set(&self.campaign_counters)),
            list(Kind::Rule.name(), set(&self.rules)),
            list(Kind::Speech.name(), set(&self.speech)),
            list(Kind::MapText.name(), set(&self.map_text)),
            list(Kind::Message.name(), set(&self.messages)),
            list(Kind::Team.name(), set(&self.teams)),
            list(Kind::ShipName.name(), set(&self.ship_names)),
            list("unit", set(&self.units)),
        ])
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

/// The nearest candidate to `name`: the same name in another case, or a name a couple of edits away.
pub fn nearest<'a>(name: &str, candidates: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    candidates
        .filter(|c| c.eq_ignore_ascii_case(name) || (name.len() > 3 && distance(&name.to_lowercase(), &c.to_lowercase()) <= (name.len() / 6).clamp(1, 3)))
        .min_by_key(|c| distance(name, c))
}

/// " (did you mean 'x'?)" for a suggestion, or nothing.
pub fn did_you_mean(suggestion: Option<&str>) -> String {
    suggestion.map(|c| format!(" (did you mean '{c}'?)")).unwrap_or_default()
}

/// Which rule, list and item a finding is about.
#[derive(Clone, Default)]
struct Place {
    rule_index: Option<usize>,
    rule_name: Option<String>,
    list: Option<&'static str>,
    /// The item's index, then the indices of nested conditions below it.
    items: Vec<usize>,
}

struct Checker<'a> {
    defs: &'a Defs,
    /// Checking a skirmish map's `script.json`.
    skirmish: bool,
    out: Vec<Problem>,
    rule: String,
    at: String,
    /// Where the finding being made sits (`Finding::rule_index` and the rest).
    place: Place,
    /// The nearest name the last `suggest` found; the next finding carries it.
    suggestion: Option<String>,
    /// (name, rule) of every flag read as TRUE / FALSE, timer read, and the names written.
    flags_set: HashSet<String>,
    timers_started: HashSet<String>,
    reads: Vec<(Kind, String, bool, String, String, Place)>,
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
        let mut f = Finding::new(level, self.rule.clone(), self.at.clone(), field, message);
        f.rule_index = self.place.rule_index;
        f.rule_name = self.place.rule_name.clone();
        f.list = self.place.list.map(String::from);
        f.item = self.place.items.first().copied();
        f.nested = self.place.items.iter().skip(1).copied().collect();
        f.suggestion = self.suggestion.take();
        self.out.push(f);
    }
    /// ` (did you mean 'x'?)` for the nearest candidate (or nothing); the finding pushed next also carries the name on its own.
    fn suggest<'a>(&mut self, name: &str, candidates: impl Iterator<Item = &'a str>) -> String {
        self.suggestion = nearest(name, candidates).map(String::from);
        did_you_mean(self.suggestion.as_deref())
    }
    fn err(&mut self, field: &str, message: String) {
        let level = if self.soft { Level::Warning } else { Level::Error };
        self.push(level, field, message);
    }
    fn warn(&mut self, field: &str, message: String) {
        self.push(Level::Warning, field, message);
    }

    fn entry(&mut self, role: Role, index: usize, e: &Value) {
        self.place.items.push(index);
        self.suggestion = None;
        self.entry_at(role, index, e);
        self.place.items.pop();
    }

    fn entry_at(&mut self, role: Role, index: usize, e: &Value) {
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
                self.suggest(ty, RULE_TYPES.iter().filter(|t| t.role == role).map(|t| t.name))
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
            let holds_at_zero = e["Equivalence"].as_str().and_then(Equivalence::parse).is_some_and(|q| q.holds(0.0, value as f32, 1e-4));
            self.wants_true = !holds_at_zero;
        }
        if def.name == "OP Teleport" {
            self.teleport_index(e);
        }
        for key in obj.keys().filter(|k| k.as_str() != "Type") {
            if def.field(key).is_none() {
                let hint = self.suggest(key, def.fields.iter().map(|f| f.name));
                self.err(key, format!("unknown field for '{ty}'{hint}; it is ignored"));
            }
        }
        for field in def.fields {
            match obj.get(field.name) {
                None if field.required => self.err(field.name, format!("missing; reads as {}", describe_default(field))),
                None => {}
                // Engine-added Types: an optional field written as "" is the same as leaving it out (the interpreter reads it so).
                Some(v) if def.origin == Origin::Engine && !field.required && v.as_str() == Some("") => {}
                Some(v) if field.used => {
                    self.value(def, field, v, role);
                    self.soft = false;
                }
                Some(_) => {}
            }
        }
    }

    /// `OP Teleport`: `Point Index` is a whole number inside the point set (the interpreter would skip the action with a warning).
    fn teleport_index(&mut self, e: &Value) {
        let Some(index) = e["Point Index"].as_f64() else { return };
        if index < 0.0 || index.fract() != 0.0 {
            self.err("Point Index", format!("{index} is not a whole number from 0 up; nothing is moved"));
        } else if let Some(len) = e["Point Set Name"].as_str().and_then(|n| self.defs.point_set_len.get(n))
            && index as usize >= *len
        {
            self.err(
                "Point Index",
                format!(
                    "point set '{}' has {len} points (0..{}); index {index} is past its end, so nothing is moved",
                    e["Point Set Name"].as_str().unwrap_or(""),
                    len.saturating_sub(1)
                ),
            );
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
                let hint = self.suggest(text, values.iter().copied());
                self.err(name, format!("'{text}' is not one of {values:?}{hint}"));
            }
            Kind::Group => self.group(name, text, def, role),
            Kind::Player if !d.group_known(text) => {
                let hint = self.suggest(text, d.players.iter().chain(d.groups.keys()).map(String::as_str));
                self.err(name, format!("unknown player '{text}'{hint}"));
            }
            Kind::Path if !d.paths.contains(text) => {
                let hint = self.suggest(text, d.paths.iter().map(String::as_str));
                self.err(name, format!("unknown path '{text}'{hint}"));
            }
            Kind::Volume => match d.volumes.get(text) {
                None => {
                    let hint = self.suggest(text, d.volumes.keys().map(String::as_str));
                    self.err(name, format!("unknown volume '{text}'{hint}; it is an empty region that nothing is ever inside"));
                }
                Some(n) if *n < 3 => self.err(name, format!("volume '{text}' has {n} vertices; fewer than 3 is an empty region")),
                Some(_) => {}
            },
            Kind::Task if !d.tasks.contains(text) => {
                let hint = self.suggest(text, d.tasks.iter().map(String::as_str));
                self.err(name, format!("unknown objective task '{text}'{hint}"));
            }
            Kind::Point if !d.points.contains(text) => {
                let hint = self.suggest(text, d.points.iter().map(String::as_str));
                self.err(name, format!("unknown objective point '{text}'{hint}"));
            }
            Kind::PointSet if !d.point_sets.contains(text) => {
                let hint = self.suggest(text, d.point_sets.iter().map(String::as_str));
                self.err(name, format!("unknown point set '{text}'{hint}"));
            }
            Kind::MapText if !d.map_text.contains(text) => {
                let hint = self.suggest(text, d.map_text.iter().map(String::as_str));
                self.err(name, format!("unknown map text '{text}'{hint}"));
            }
            Kind::Speech if d.media && !d.speech.contains(text) => {
                let hint = self.suggest(text, d.speech.iter().map(String::as_str));
                self.warn(name, format!("speech event '{text}' has no media entry (no text, no sound){hint}"));
            }
            Kind::Message if d.media && !d.messages.contains(text) => self.warn(name, format!("message '{text}' has no media entry; the game falls back to a default")),
            Kind::ShipName if d.media && !d.ship_names.contains(text) => self.warn(name, format!("ship name '{text}' has no media entry; the rule does nothing")),
            Kind::Flag => {
                if role == Role::Action {
                    self.flags_set.insert(text.into());
                } else {
                    self.reads.push((Kind::Flag, text.into(), self.wants_true && !self.lenient, self.rule.clone(), self.at.clone(), self.place.clone()));
                }
            }
            Kind::Counter => {
                if role == Role::Action {
                    self.counters_set.insert(text.into());
                } else {
                    self.reads.push((Kind::Counter, text.into(), self.wants_true && !self.lenient, self.rule.clone(), self.at.clone(), self.place.clone()));
                }
            }
            Kind::Rule if !d.rules.contains(text.trim()) => {
                let hint = self.suggest(text, d.rules.iter().map(String::as_str));
                self.err(name, format!("unknown rule '{text}'{hint}; nothing is activated"));
            }
            Kind::Timer => {
                if def.name == "Start Timer" {
                    self.timers_started.insert(text.into());
                } else if role == Role::Condition {
                    self.reads.push((Kind::Timer, text.into(), !self.lenient, self.rule.clone(), self.at.clone(), self.place.clone()));
                }
            }
            _ => {}
        }
    }

    fn group(&mut self, field: &str, spec: &str, def: &TypeDef, role: Role) {
        let d = self.defs;
        let (group, unit) = spec.split_once(',').map_or((spec, None), |(g, u)| (g, Some(u)));
        if !d.group_known(group) {
            let hint = self.suggest(group, d.groups.keys().chain(&d.players).chain(&d.other_groups).map(String::as_str));
            self.err(field, format!("unknown group '{group}'{hint}; it has no members"));
        } else if role == Role::Condition && unit.is_none() && d.groups.get(group) == Some(&0) && !d.players.contains(group) {
            self.warn(field, format!("group '{group}' is defined with no members (in '{}')", def.name));
        }
        if let Some(unit) = unit
            && unit != "HUMAN CONTROLLED COMMAND SHIP"
            && !d.units.contains(unit)
        {
            let hint = self.suggest(unit, d.units.iter().map(String::as_str));
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
        c.place = Place::default();
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
            c.place = Place { list: Some(key), ..Place::default() };
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
    c.place = Place::default();
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

/// Warnings for groups, paths, volumes and objective points the mission defines and nothing uses: no rule field of that kind names
/// them, and the placed ships, environment, tasks and (skirmish) victory conditions do not name them either. Not part of `check`
/// (retail missions define leftovers); `--check-mod` adds it for a mod's missions.
pub fn unused_definitions(mission: &Value) -> Vec<Finding> {
    let mut used: BTreeSet<(&'static str, String)> = BTreeSet::new();
    visit_fields(mission, &mut |_, _, field, text| {
        let kind = match field.kind {
            Kind::Group | Kind::Player => "group",
            Kind::Path => "path",
            Kind::Volume => "volume",
            Kind::Point => "point",
            _ => return,
        };
        // A group spec is `Group` or `Group,Unit`; a player field may name a group too.
        used.insert((kind, text.split(',').next().unwrap_or(text).to_string()));
    });
    // Everything outside the rules and the definitions themselves may use a name by any key.
    let mut elsewhere = BTreeSet::new();
    for (_, v) in mission.as_object().into_iter().flatten().filter(|(k, _)| !["rules", "groups", "paths", "volumes", "points"].contains(&k.as_str())) {
        all_strings(v, &mut elsewhere);
    }
    let mut out = vec![];
    let mut unused = |kind: &'static str, key: &str, names: Vec<&String>, noun: &str| {
        for name in names {
            if !used.contains(&(kind, name.clone())) && !elsewhere.contains(name) && !name.is_empty() {
                let mut f = Finding::new(Level::Warning, "(mission)", format!("{noun} '{name}'"), key, format!("{noun} '{name}' is defined but no rule uses it"));
                f.set_path(JsonPath::from(key).key(name.as_str()));
                out.push(f);
            }
        }
    };
    let keys = |key: &str| -> Vec<String> { mission[key].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default() };
    let groups = keys("groups");
    unused("group", "groups", groups.iter().collect(), "group");
    let paths = names(mission, "paths").into_iter().collect::<Vec<_>>();
    unused("path", "paths", paths.iter().collect(), "path");
    let volumes = keys("volumes");
    unused("volume", "volumes", volumes.iter().collect(), "volume");
    let points: Vec<String> = mission["points"].as_array().into_iter().flatten().filter_map(|t| t["Name"].as_str()).map(String::from).collect();
    unused("point", "points", points.iter().collect(), "objective point");
    out
}

/// Warnings for ships the mission places, and scenery and mines `map.json` places, outside the playable square (`world_half`,
/// default 1000, on x and y). A finding about `map.json` has `file` "map.json"; one about the mission leaves `file` empty.
pub fn outside_world(mission: &Value, map: &Value) -> Vec<Finding> {
    let half = map["world_half"].as_f64().unwrap_or(1000.0);
    let outside = |p: &Value| p.as_array().is_some_and(|p| p.iter().take(2).filter_map(Value::as_f64).any(|v| v.abs() > half));
    let mut out = vec![];
    for (i, s) in mission["ships"].as_array().into_iter().flatten().enumerate().filter(|(_, s)| outside(&s["pos"])) {
        let who = s["name"].as_str().or_else(|| s["type"].as_str()).unwrap_or("ship");
        out.push(Finding::in_file(Level::Warning, "", JsonPath::from("ships").index(i).key("pos"), format!("{who} is at {} , outside world_half {half}", s["pos"])));
    }
    for key in ["objects", "mines"] {
        for (i, o) in map[key].as_array().into_iter().flatten().enumerate().filter(|(_, o)| outside(&o["pos"])) {
            let who = o["type"].as_str().unwrap_or(key);
            out.push(Finding::in_file(Level::Warning, "map.json", JsonPath::from(key).index(i).key("pos"), format!("{who} is at {} , outside world_half {half}", o["pos"])));
        }
    }
    out
}

/// Warnings for `map.json` scenery whose `scene` model is not among `models` (the file names under `models/`, `Rock.glb`): it would
/// be an invisible obstacle. `file` is "map.json".
pub fn missing_scenery(map: &Value, models: &HashSet<String>) -> Vec<Finding> {
    let mut out = vec![];
    for (i, o) in map["objects"].as_array().into_iter().flatten().enumerate() {
        let Some(scene) = o["scene"].as_str().filter(|s| !s.is_empty() && !models.contains(&format!("{s}.glb"))) else { continue };
        let best = nearest(scene, models.iter().filter_map(|m| m.strip_suffix(".glb")));
        let mut f = Finding::in_file(
            Level::Warning,
            "map.json",
            JsonPath::from("objects").index(i).key("scene"),
            format!("the scenery model '{scene}.glb' is not under models/ of the game or the mods{}", did_you_mean(best)),
        );
        f.suggestion = best.map(String::from);
        out.push(f);
    }
    out
}

/// Every string anywhere below `v`, keys excluded.
fn all_strings(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::String(s) => {
            out.insert(s.clone());
        }
        Value::Object(m) => m.values().for_each(|x| all_strings(x, out)),
        Value::Array(a) => a.iter().for_each(|x| all_strings(x, out)),
        _ => {}
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
        place: Place::default(),
        suggestion: None,
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
        c.place = Place { rule_index: Some(i), rule_name: r["name"].as_str().filter(|n| !n.is_empty()).map(|n| n.trim().to_string()), ..Place::default() };
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
        c.place.list = Some("conditions");
        for (i, e) in conds.iter().enumerate() {
            c.entry(Role::Condition, i, e);
        }
        c.place.list = Some("actions");
        for (i, e) in acts.iter().enumerate() {
            c.entry(Role::Action, i, e);
        }
    }
    c.place = Place::default();
    check_tasks_and_victory(&mut c, mission);
    let (flags, timers, counters) = (std::mem::take(&mut c.flags_set), std::mem::take(&mut c.timers_started), std::mem::take(&mut c.counters_set));
    for (kind, name, wants_true, rule, at, place) in std::mem::take(&mut c.reads) {
        c.rule = rule;
        c.at = at;
        c.place = place;
        match kind {
            Kind::Flag if !flags.contains(&name) => {
                let hint = c.suggest(&name, flags.iter().map(String::as_str));
                if wants_true {
                    c.err("Flag Name", format!("flag '{name}' is never set, so this can never hold{hint}"));
                } else {
                    c.warn("Flag Name", format!("flag '{name}' is never set (always FALSE){hint}"));
                }
            }
            Kind::Counter if !counters.contains(&name) => {
                let hint = c.suggest(&name, counters.iter().map(String::as_str));
                if wants_true {
                    c.err("Counter Name", format!("counter '{name}' is never set (it reads 0), so this can never hold{hint}"));
                } else {
                    c.warn("Counter Name", format!("counter '{name}' is never set (it always reads 0){hint}"));
                }
            }
            Kind::Timer if !timers.contains(&name) => {
                let hint = c.suggest(&name, timers.iter().map(String::as_str));
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

    #[test]
    fn a_finding_says_where_it_is_as_data_and_keeps_the_printed_line() {
        let named = |n: &str, acts: Value| json!({"name": n, "Run Once": true, "conditions": [], "actions": acts});
        let m = mission(json!([
            named("First", json!([])),
            named(
                " Second ",
                json!([
                    {"Type": "Start Timer", "Timer Name": "T"},
                    {"Type": "Group to follow path", "Group Name": "Fleat", "Path Name": "Route", "Find Closest Point": "TRUE", "Follow Mode": "loop"}
                ])
            ),
            json!({"name": "Third", "conditions": [{"Type": "OP Any Of", "Conditions": [{"Type": "Enter Volume", "Group Name": "Fleet", "Volume Name": "Aera"}]}], "actions": []}),
        ]));
        let found = check(&m, &Value::Null, &Value::Null);
        let typo = found.iter().find(|p| p.field == "Group Name").unwrap();
        assert_eq!((typo.rule_index, typo.rule_name.as_deref(), typo.list.as_deref(), typo.item), (Some(1), Some("Second"), Some("actions"), Some(1)));
        assert_eq!((typo.level, typo.suggestion.as_deref(), typo.nested.as_slice()), (Level::Error, Some("Fleet"), &[][..]));
        assert!(typo.message.ends_with("(did you mean 'Fleet'?); it has no members"), "the printed text keeps the hint: {}", typo.message);
        assert_eq!(
            typo.to_string(),
            "ERROR: rule \"Second\": action #2 'Group to follow path', field \"Group Name\": unknown group 'Fleat' (did you mean 'Fleet'?); it has no members"
        );
        let nested = found.iter().find(|p| p.field == "Volume Name").unwrap();
        assert_eq!(
            (nested.rule_index, nested.list.as_deref(), nested.item, nested.nested.as_slice(), nested.suggestion.as_deref()),
            (Some(2), Some("conditions"), Some(0), &[0][..], Some("Area"))
        );
        let unnamed = found.iter().find(|p| p.message.contains("no name")).map(|p| (p.rule_index, p.rule_name.clone()));
        assert_eq!(unnamed, None, "every rule here is named");
        // the whole finding is JSON for tools
        let v = serde_json::to_value(typo).unwrap();
        assert_eq!((v["level"].as_str(), v["list"].as_str(), v["item"].as_u64(), v["suggestion"].as_str()), (Some("error"), Some("actions"), Some(1), Some("Fleet")));
        assert!(v["file"].is_string() && v["nested"].is_array());
    }

    #[test]
    fn a_repeated_rule_name_is_a_warning() {
        let t = text(json!([rule(json!([]), json!([{"Type": "OP Wait", "Seconds": 1}])), rule(json!([]), json!([{"Type": "OP Wait", "Seconds": 1}]))]));
        assert_eq!(t.matches("another rule has the same name").count(), 1, "{t}");
        assert!(t.contains("field \"name\": another rule has the same name (--fire-rule and --why take the first)"), "{t}");
        let second = problems(json!([rule(json!([]), json!([])), rule(json!([]), json!([]))])).into_iter().find(|p| p.message.contains("same name")).unwrap();
        assert_eq!((second.level, second.rule_index), (Level::Warning, Some(1)));
    }

    #[test]
    fn defined_names_lists_what_a_mission_defines_per_kind() {
        let m = mission(json!([{"name": "R", "Run Once": true, "conditions": [], "actions": [
            {"Type": "Set Flag Action", "Flag Name": "Seen", "Boolean Value": "TRUE"}, {"Type": "Start Timer", "Timer Name": "Clock"},
            {"Type": "OP Set Counter", "Counter Name": "Kills", "Value": 0}
        ]}]));
        let media = json!({"speech": {"HELLO": {}}, "messages": {"LOST": "x"}, "ship_names": {"HAWK": "y"}});
        let map = json!({"teams": [{"name_id": "NAVY"}], "players": [{"name": "Rival"}]});
        let d = defined_names(&m, &map, &media);
        let get = |k: &str| d[k].iter().map(String::as_str).collect::<Vec<_>>();
        assert_eq!(get("group"), ["Empty", "Fleet"]);
        assert_eq!(get("volume"), ["Area", "Dot"]);
        assert_eq!((get("path"), get("task"), get("point"), get("point_set"), get("rule")), (vec!["Route"], vec!["Task A"], vec!["Point A"], vec!["Set A"], vec!["R"]));
        assert_eq!((get("flag"), get("timer"), get("counter")), (vec!["Seen"], vec!["Clock"], vec!["Kills"]));
        assert_eq!(
            (get("speech_event"), get("message"), get("ship_name"), get("team"), get("map_text"), get("unit")),
            (vec!["HELLO"], vec!["LOST"], vec!["HAWK"], vec!["NAVY"], vec!["Label"], vec!["RLS Hawk"])
        );
        assert!(get("player").contains(&"Pirates") && get("player").contains(&"Rival") && get("player").contains(&"Player0 Group"));
        assert!(defined_names(&Value::Null, &Value::Null, &Value::Null)["group"].is_empty());
    }

    #[test]
    fn placements_outside_world_half_and_scenery_without_a_model_are_warnings_naming_their_file() {
        let mission = json!({"ships": [{"name": "Far", "pos": [0, 1200, 0]}, {"pos": [10, 10, 0]}]});
        let map =
            json!({"objects": [{"type": "Rock", "scene": "Rokk", "pos": [-1001, 0, 0]}, {"type": "Isle", "scene": "Isle", "pos": [0, 0, 0]}], "mines": [{"pos": [0, 0, 900]}]});
        let far = outside_world(&mission, &map);
        let at: Vec<_> = far.iter().map(|f| (f.file.as_str(), f.path.as_str())).collect();
        assert_eq!(at, [("", "ships[0].pos"), ("map.json", "objects[0].pos")], "z does not count");
        assert!(far[0].message.contains("Far is at") && far[0].message.contains("world_half 1000"));
        assert!(outside_world(&mission, &json!({"world_half": 1500})).is_empty(), "the map's own square");
        let models = HashSet::from(["Rock.glb".to_string(), "Isle.glb".to_string()]);
        let missing = missing_scenery(&map, &models);
        assert_eq!(missing.len(), 1);
        assert_eq!((missing[0].file.as_str(), missing[0].path.as_str(), missing[0].suggestion.as_deref()), ("map.json", "objects[0].scene", Some("Rock")));
    }

    #[test]
    fn what_no_rule_uses_is_listed_but_not_what_the_rest_of_the_mission_uses() {
        let mut m = mission(json!([rule(
            json!([{"Type": "Enter Volume", "Group Name": "Fleet,RLS Hawk", "Volume Name": "Area"}]),
            json!([{"Type": "Group to follow path", "Group Name": "Fleet", "Path Name": "Route", "Find Closest Point": "TRUE", "Follow Mode": "loop"}])
        )]));
        let found = |m: &Value| unused_definitions(m).iter().map(|f| f.message.clone()).collect::<Vec<_>>();
        assert_eq!(
            found(&m),
            ["group 'Empty' is defined but no rule uses it", "volume 'Dot' is defined but no rule uses it", "objective point 'Point A' is defined but no rule uses it"]
        );
        let first = &unused_definitions(&m)[0];
        assert_eq!((first.level, first.path.as_str()), (Level::Warning, "groups.Empty"));
        // a ship's own group and the skirmish victory conditions count as uses
        m["ships"][0]["group"] = json!("Empty");
        m["victory"] = json!({"if": [{"Type": "Enter Volume", "polygon": "Dot"}]});
        m["points"] = json!([]);
        assert_eq!(found(&m), Vec::<String>::new());
    }

    /// docs/rule-types.md is generated from the schema (tools/gen_rule_types.py); a Type added without re-running it fails here.
    #[test]
    fn every_rule_type_is_documented() {
        let doc = include_str!("../../../docs/rule-types.md");
        let missing: Vec<&str> = RULE_TYPES.iter().map(|t| t.name).filter(|n| !doc.contains(&format!("`{n}`"))).collect();
        assert!(missing.is_empty(), "docs/rule-types.md does not document {missing:?}: run tools/gen_rule_types.py (see its docstring)");
    }

    #[test]
    fn a_json_path_prints_dotted_and_as_a_pointer() {
        let p = JsonPath::from("guns").key("A/b~c").key("pos").index(2);
        assert_eq!((p.dotted(), p.pointer()), ("guns.A/b~c.pos[2]".to_string(), "/guns/A~1b~0c/pos/2".to_string()));
        assert_eq!((JsonPath::from("").dotted(), JsonPath::from("").pointer()), (String::new(), String::new()));
        let f = Finding::in_file(Level::Warning, "x.json", JsonPath::default().key("Gun").index(0), "m");
        assert_eq!((f.path.as_str(), f.pointer.as_str()), ("Gun[0]", "/Gun/0"));
    }
}
