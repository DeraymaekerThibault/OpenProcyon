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
    /// `--check-mod` only: a warning the base game's own files have too, for the same map or mission without the mod, so it is not the
    /// mod's. Errors are never inherited. Always `false` when no imported assets were found to compare with.
    pub inherited: bool,
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
            inherited: false,
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

/// The fewest vertices of a polygon volume (a smaller one is an empty region, an error where a rule uses it).
const MIN_VOLUME_VERTICES: usize = 3;

/// The kinds of rule field that take a name the mission defines (`names_by_kind` keys, except `unit`).
const NAMED_KINDS: [Kind; 17] = [
    Kind::Group,
    Kind::Player,
    Kind::Path,
    Kind::Volume,
    Kind::Task,
    Kind::Point,
    Kind::PointSet,
    Kind::Timer,
    Kind::Flag,
    Kind::Counter,
    Kind::CampaignCounter,
    Kind::Rule,
    Kind::Speech,
    Kind::MapText,
    Kind::Message,
    Kind::Team,
    Kind::ShipName,
];

/// For each kind of rule field (the keys of [`defined_names`], `group`, `volume`, `flag`, ...), exactly the names a field of that
/// kind takes without the checker reporting the name, sorted. Where [`defined_names`] says what the mission defines under each
/// heading, this says what may be written in a field: a `group` field takes the TWT groups, the players and the groups of placed
/// ships and of the environment, but not a unit's own name (that goes after the comma: `unit`); a `volume` only when it has three
/// vertices; a `rule` by its trimmed name. The membership test is the checker's own (`Defs::accepts`), so the two cannot disagree.
/// Kinds the checker does not look names up for (`timer`, `flag`, `counter`, `campaign_counter`, `team`) list what the mission
/// defines, and accept any name. `speech_event`, `message` and `ship_name` list the entries of `media.json` (any id is accepted when
/// `media` is `Value::Null`, so the list is then what `opening_speech` and nothing else gives). The editor placeholders (`NO PATH`,
/// ...) are always accepted and not listed. Enum values are per field, in the rule schema.
pub fn accepted_names(mission: &Value, map: &Value, media: &Value) -> BTreeMap<String, Vec<String>> {
    Defs::collect(mission, map, media).accepted()
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

/// One string field of a rule (or of a task's `complete_when` / `fail_when`) that the rule schema knows: where it is, what kind of
/// thing it names, and its text. The unit of [`visit_located`] and of `authoring::visit_fields`.
pub struct FieldVisit<'a> {
    /// The JSON pointer of the field in the mission document (`/rules/3/actions/0/Group`, `/tasks/1/complete_when/0/Volume`;
    /// `~` and `/` in a key escaped as `~0` and `~1`), nested conditions of `OP Any Of` / `OP Not` included.
    pub pointer: String,
    /// The field's text.
    pub text: &'a str,
    pub(crate) role: Role,
    pub(crate) def: &'static TypeDef,
    pub(crate) field: &'static Field,
}

impl FieldVisit<'_> {
    /// The field's kind as the rule schema names it (`Kind::name`: `group`, `volume`, `point_set`, `speech_event`, `rule`, ...).
    pub fn kind(&self) -> &'static str {
        self.field.kind.name()
    }
    /// The field's name in its Type (`Group`, `Volume`).
    pub fn field(&self) -> &'static str {
        self.field.name
    }
    /// The `Type` of the condition or action that holds the field.
    pub fn item_type(&self) -> &'static str {
        self.def.name
    }
    /// `"condition"` or `"action"`, the `role` of the rule schema.
    pub fn role(&self) -> &'static str {
        match self.role {
            Role::Condition => "condition",
            Role::Action => "action",
        }
    }
}

/// The two names of a `group` field: `Group` and, after the first comma, the `Unit` (the interpreter splits at the first comma). The
/// text of any other kind has no such part. A `player` field is the same namespace as `group` (see [`accepted_names`]).
pub fn split_group(text: &str) -> (&str, Option<&str>) {
    match text.split_once(',') {
        Some((group, unit)) => (group, Some(unit)),
        None => (text, None),
    }
}

/// Calls `f` for every string a rule of the mission writes in a field of the schema, with its JSON pointer: nested conditions
/// (`OP Any Of`, `OP Not`) and the `complete_when` / `fail_when` lists of tasks included. Entries of an unknown Type are skipped. The
/// one walk behind the checker, `defined_names` and `authoring::visit_fields`.
pub fn visit_located(mission: &Value, f: &mut dyn FnMut(&FieldVisit)) {
    fn entry(role: Role, e: &Value, at: JsonPath, f: &mut dyn FnMut(&FieldVisit)) {
        let Some(def) = e["Type"].as_str().and_then(|ty| lookup(role, ty)) else { return };
        for field in def.fields {
            let here = at.clone().key(field.name);
            match (&e[field.name], field.kind) {
                (Value::Array(list), Kind::ConditionList) => list.iter().enumerate().for_each(|(i, c)| entry(Role::Condition, c, here.clone().index(i), f)),
                (v @ Value::Object(_), Kind::Condition) => entry(Role::Condition, v, here, f),
                (Value::String(text), _) => f(&FieldVisit { pointer: here.pointer(), text, role, def, field }),
                _ => {}
            }
        }
    }
    for (r, rule) in mission["rules"].as_array().into_iter().flatten().enumerate() {
        let at = JsonPath::default().key("rules").index(r);
        for (key, role) in [("conditions", Role::Condition), ("actions", Role::Action)] {
            for (i, e) in rule[key].as_array().into_iter().flatten().enumerate() {
                entry(role, e, at.clone().key(key).index(i), f);
            }
        }
    }
    for (t, task) in mission["tasks"].as_array().into_iter().flatten().enumerate() {
        for key in ["complete_when", "fail_when"] {
            for (i, e) in task[key].as_array().into_iter().flatten().enumerate() {
                entry(Role::Condition, e, JsonPath::default().key("tasks").index(t).key(key).index(i), f);
            }
        }
    }
}

/// [`visit_located`] where `f` may change the text (a rename): `f` gets the field and a copy of its text, and what it leaves in the
/// copy is written back to the field. Visits the same fields in the same order; the fields are found first, then written, so `f` never
/// sees an edit of its own.
pub fn visit_located_mut(mission: &mut Value, f: &mut dyn FnMut(&FieldVisit, &mut String)) {
    let mut edits = vec![];
    visit_located(mission, &mut |v| {
        let mut text = v.text.to_string();
        f(v, &mut text);
        if text != v.text {
            edits.push((v.pointer.clone(), text));
        }
    });
    for (pointer, text) in edits {
        if let Some(slot) = mission.pointer_mut(&pointer) {
            *slot = Value::String(text);
        }
    }
}

/// Calls `f(role, type, field, text)` for every string a rule of the mission writes in a field of the schema (see [`visit_located`]).
pub(crate) fn visit_fields(mission: &Value, f: &mut dyn FnMut(Role, &TypeDef, &Field, &str)) {
    visit_located(mission, &mut |v| f(v.role, v.def, v.field, v.text));
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

    /// Whether the checker lets a field of `kind` hold `name` without a finding about the name itself: the one test the checker's
    /// field arms and [`accepted_names`] both use. A `Group` field is the group part of a spec (before any `,unit`). Kinds the checker
    /// does not look names up for (flags, timers and counters are matched across rules later, teams, enums, text, ...) accept any name.
    /// Speech, message and ship-name ids are only looked up when `media.json` was given.
    pub(crate) fn accepts(&self, kind: Kind, name: &str) -> bool {
        match kind {
            Kind::Group | Kind::Player => self.group_known(name),
            Kind::Path => self.paths.contains(name),
            Kind::Volume => self.volumes.get(name).is_some_and(|n| *n >= MIN_VOLUME_VERTICES),
            Kind::Task => self.tasks.contains(name),
            Kind::Point => self.points.contains(name),
            Kind::PointSet => self.point_sets.contains(name),
            Kind::MapText => self.map_text.contains(name),
            Kind::Speech => !self.media || self.speech.contains(name),
            Kind::Message => !self.media || self.messages.contains(name),
            Kind::ShipName => !self.media || self.ship_names.contains(name),
            Kind::Rule => self.rules.contains(name.trim()),
            _ => true,
        }
    }

    /// Whether the `unit` part of a group spec (`Group,Unit`) is a ship the mission places (or the human's command ship).
    fn accepts_unit(&self, unit: &str) -> bool {
        unit == "HUMAN CONTROLLED COMMAND SHIP" || self.units.contains(unit)
    }

    /// The names a field of each kind accepts, by `Kind::name` (see [`accepted_names`]).
    fn accepted(&self) -> BTreeMap<String, Vec<String>> {
        let all = self.names_by_kind();
        let mut out = BTreeMap::new();
        for (kind, defined) in &all {
            let Some(k) = NAMED_KINDS.iter().find(|k| k.name() == kind) else {
                // `unit`: the part after the comma of a group spec.
                let units = defined.iter().cloned().chain(["HUMAN CONTROLLED COMMAND SHIP".to_string()]).filter(|u| self.accepts_unit(u));
                out.insert(kind.clone(), units.collect());
                continue;
            };
            // The candidates: what the mission defines, and for a group or player field also every name of the other of the two.
            let mut candidates: BTreeSet<&String> = defined.iter().collect();
            if matches!(k, Kind::Group | Kind::Player) {
                candidates.extend(all["group"].iter().chain(&all["player"]).chain(&self.other_groups));
            }
            out.insert(kind.clone(), candidates.into_iter().filter(|n| self.accepts(*k, n)).cloned().collect());
        }
        out
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
            Kind::Player if !d.accepts(Kind::Player, text) => {
                let hint = self.suggest(text, d.players.iter().chain(d.groups.keys()).map(String::as_str));
                self.err(name, format!("unknown player '{text}'{hint}"));
            }
            Kind::Path if !d.accepts(Kind::Path, text) => {
                let hint = self.suggest(text, d.paths.iter().map(String::as_str));
                self.err(name, format!("unknown path '{text}'{hint}"));
            }
            Kind::Volume => match d.volumes.get(text) {
                None => {
                    let hint = self.suggest(text, d.volumes.keys().map(String::as_str));
                    self.err(name, format!("unknown volume '{text}'{hint}; it is an empty region that nothing is ever inside"));
                }
                Some(n) if *n < MIN_VOLUME_VERTICES => self.err(name, format!("volume '{text}' has {n} vertices; fewer than 3 is an empty region")),
                Some(_) => {}
            },
            Kind::Task if !d.accepts(Kind::Task, text) => {
                let hint = self.suggest(text, d.tasks.iter().map(String::as_str));
                self.err(name, format!("unknown objective task '{text}'{hint}"));
            }
            Kind::Point if !d.accepts(Kind::Point, text) => {
                let hint = self.suggest(text, d.points.iter().map(String::as_str));
                self.err(name, format!("unknown objective point '{text}'{hint}"));
            }
            Kind::PointSet if !d.accepts(Kind::PointSet, text) => {
                let hint = self.suggest(text, d.point_sets.iter().map(String::as_str));
                self.err(name, format!("unknown point set '{text}'{hint}"));
            }
            Kind::MapText if !d.accepts(Kind::MapText, text) => {
                let hint = self.suggest(text, d.map_text.iter().map(String::as_str));
                self.err(name, format!("unknown map text '{text}'{hint}"));
            }
            Kind::Speech if !d.accepts(Kind::Speech, text) => {
                let hint = self.suggest(text, d.speech.iter().map(String::as_str));
                self.warn(name, format!("speech event '{text}' has no media entry (no text, no sound){hint}"));
            }
            Kind::Message if !d.accepts(Kind::Message, text) => self.warn(name, format!("message '{text}' has no media entry; the game falls back to a default")),
            Kind::ShipName if !d.accepts(Kind::ShipName, text) => self.warn(name, format!("ship name '{text}' has no media entry; the rule does nothing")),
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
            Kind::Rule if !d.accepts(Kind::Rule, text) => {
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
        if !d.accepts(Kind::Group, group) {
            let hint = self.suggest(group, d.groups.keys().chain(&d.players).chain(&d.other_groups).map(String::as_str));
            self.err(field, format!("unknown group '{group}'{hint}; it has no members"));
        } else if role == Role::Condition && unit.is_none() && d.groups.get(group) == Some(&0) && !d.players.contains(group) {
            self.warn(field, format!("group '{group}' is defined with no members (in '{}')", def.name));
        }
        if let Some(unit) = unit
            && !d.accepts_unit(unit)
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
    let half = crate::data::MapDoc::from_value(map).half();
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

/// Warnings for world object ids that two entries share. Groups, rules and docks address a placed thing by its `id`, and the ids of
/// ships, scenery, animals and mines are one namespace, so a repeated id makes a group (or `Set Group/Unit Owner`) reach the wrong
/// one. Looks at `map.json` `objects`, `ships` and `mines` and the mission's `ships`, `environment.animals` and `environment.dragons`
/// (an entry with no integer `id` is decoration and never counts). One warning per entry that repeats an earlier id, at that
/// entry's `id`, its message naming the earlier entry's file and JSON pointer (`map.json /objects/2`; `mission /ships/0`). `file` is
/// "map.json" for a map entry and empty for a mission one, as for [`outside_world`]. PROVISIONAL: a warning rather than an error
/// because no retail file could be read to see whether the originals ever repeat an id.
pub fn duplicate_ids(mission: &Value, map: &Value) -> Vec<Finding> {
    let lists: [(&str, &Value, &str, &[&str]); 6] = [
        ("map.json", map, "objects", &["objects"]),
        ("map.json", map, "ships", &["ships"]),
        ("map.json", map, "mines", &["mines"]),
        ("", mission, "mission", &["ships"]),
        ("", mission, "mission", &["environment", "animals"]),
        ("", mission, "mission", &["environment", "dragons"]),
    ];
    let mut seen: std::collections::HashMap<i64, String> = Default::default();
    let mut out = vec![];
    for (file, doc, label, keys) in lists {
        let list = keys.iter().fold(doc, |v, k| &v[*k]);
        for (i, entry) in list.as_array().into_iter().flatten().enumerate() {
            let Some(id) = entry["id"].as_i64() else { continue };
            let path = keys.iter().fold(JsonPath::default(), |p, k| p.key(*k)).index(i);
            let here = format!("{} {}", if file.is_empty() { label } else { file }, path.pointer());
            match seen.get(&id) {
                Some(first) => {
                    out.push(Finding::in_file(Level::Warning, file, path.key("id"), format!("id {id} is also used by {first}; groups and rules address a placed thing by its id")))
                }
                None => {
                    seen.insert(id, here);
                }
            }
        }
    }
    out
}

/// Warnings for a nebula whose three records disagree (docs/missions.md section 8): `map.json` `nebulae[]` (what is drawn, drains and
/// damages), the mission's `environment.nebulae[]` (the `*State Init* Setup Nebula` record: what hides ships) and the `volumes` entry
/// its `Polygon Name` names (what `Unit is Within any Nebula` and sight occlusion read). Reported: a map nebula with no record of that
/// `New Nebula Name`, a record with no map nebula of that name, a `Polygon Name` that is no volume (or one with fewer than three
/// points), and a volume whose points differ from the map nebula's `polygon` (same points in the same order, to 0.001). `file` as for
/// [`duplicate_ids`]. PROVISIONAL: reads the rules of agreement from the docs and the exporter (the retail exporter copies the polygon
/// from the volume, so retail agrees by construction); the order of the points is compared as written.
pub fn nebula_agreement(mission: &Value, map: &Value) -> Vec<Finding> {
    let records = mission["environment"]["nebulae"].as_array().map_or(&[][..], Vec::as_slice);
    let setups = map["nebulae"].as_array().map_or(&[][..], Vec::as_slice);
    let named = |v: &Value, key: &str| v[key].as_str().filter(|n| !n.is_empty()).map(String::from);
    let mut out = vec![];
    for (i, n) in setups.iter().enumerate() {
        let Some(name) = named(n, "name") else { continue };
        if !records.iter().any(|r| named(r, "New Nebula Name").as_deref() == Some(&name)) {
            out.push(Finding::in_file(
                Level::Warning,
                "map.json",
                JsonPath::from("nebulae").index(i).key("name"),
                format!("nebula '{name}' has no record with that \"New Nebula Name\" in the mission's environment.nebulae: it is drawn and damages, but hides no ship and does not count for \"Unit is Within any Nebula\""),
            ));
        }
    }
    let points = |v: &Value| -> Vec<(f64, f64)> { v.as_array().into_iter().flatten().map(|p| (p[0].as_f64().unwrap_or(f64::NAN), p[1].as_f64().unwrap_or(f64::NAN))).collect() };
    for (j, r) in records.iter().enumerate() {
        let at = JsonPath::from("environment").key("nebulae").index(j);
        let name = named(r, "New Nebula Name");
        let setup = name.as_ref().and_then(|name| setups.iter().enumerate().find(|(_, n)| named(n, "name").as_ref() == Some(name)));
        if let (Some(name), None) = (&name, setup) {
            out.push(Finding::in_file(
                Level::Warning,
                "",
                at.clone().key("New Nebula Name"),
                format!("no nebula named '{name}' in map.json nebulae: nothing is drawn or damages there, only the polygon hides ships"),
            ));
        }
        let Some(polygon) = named(r, "Polygon Name") else { continue };
        let volume = &mission["volumes"][polygon.as_str()];
        if volume.as_array().is_none_or(|v| v.len() < 3) {
            out.push(Finding::in_file(Level::Warning, "", at.key("Polygon Name"), format!("'{polygon}' is not a volume of three or more points: this nebula has no region")));
        } else if let Some((i, n)) = setup {
            let (a, b) = (points(volume), points(&n["polygon"]));
            let same = a.len() == b.len() && a.iter().zip(&b).all(|(p, q)| (p.0 - q.0).abs() < 1e-3 && (p.1 - q.1).abs() < 1e-3);
            if !same {
                let place = JsonPath::from("volumes").key(polygon.as_str());
                out.push(Finding::in_file(
                    Level::Warning,
                    "",
                    place,
                    format!("volume '{polygon}' differs from the polygon of nebula '{}' (map.json /nebulae/{i}/polygon): the nebula is drawn where the map says and hides ships where the volume says", named(n, "name").unwrap_or_default()),
                ));
            }
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
    fn a_player_listed_only_in_mission_json_is_defined_and_accepted() {
        let m = json!({"players": ["Hero", "Raiders"], "ships": [], "rules": []});
        let map = json!({"players": [{"name": "Rival"}], "teams": []});
        let d = defined_names(&m, &map, &Value::Null);
        assert!(d["player"].contains(&"Raiders".to_string()), "{:?}", d["player"]);
        let a = accepted_names(&m, &map, &Value::Null);
        for kind in ["player", "group"] {
            assert!(a[kind].contains(&"Raiders".to_string()) && a[kind].contains(&"Hero".to_string()), "{kind}: {:?}", a[kind]);
        }
        let ok = json!({"players": ["Hero", "Raiders"], "ships": [], "rules": [{"name": "R", "Run Once": true, "conditions": [{"Type": "World Initialize"}],
            "actions": [{"Type": "Set Fleet Hold Fire", "PLAYER FLEET": "Raiders", "HOLD FIRE": "TRUE"}]}]});
        let found: Vec<String> = check(&ok, &map, &Value::Null).iter().map(ToString::to_string).collect();
        assert!(!found.iter().any(|f| f.contains("Raiders")), "{found:?}");
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
    fn accepted_names_are_what_the_checker_accepts_per_kind() {
        let mut m = mission(json!([rule(json!([]), json!([]))]));
        m["ships"] = json!([{"id": 1, "name": "RLS Hawk", "group": "Player0 Group"}, {"id": 2, "name": "Raider", "group": "Escort"}]);
        m["environment"]["animals"] = json!([{"group": "Whales"}]);
        let map = json!({"teams": [{"name_id": "NAVY"}], "players": [{"name": "Rival"}]});
        let media = json!({"speech": {"HELLO": {}}, "messages": {"LOST": "x"}, "ship_names": {"HAWK": "y"}});
        let accepted = accepted_names(&m, &map, &media);
        let get = |k: &str| accepted[k].iter().map(String::as_str).collect::<Vec<_>>();
        // A group field takes players and the groups of placed ships and of the environment too, but not a unit's own name.
        for name in ["Fleet", "Empty", "Pirates", "Rival", "Player0 Group", "Escort", "Whales"] {
            assert!(get("group").contains(&name), "{name} in {:?}", get("group"));
        }
        assert!(!get("group").contains(&"RLS Hawk") && !get("group").contains(&"Raider"));
        assert_eq!(get("group"), get("player"));
        assert!(get("unit").contains(&"Raider") && get("unit").contains(&"HUMAN CONTROLLED COMMAND SHIP"));
        assert_eq!(get("volume"), ["Area"], "a volume of one vertex is an error where it is used");
        assert_eq!((get("rule"), get("speech_event"), get("team")), (vec!["R"], vec!["HELLO"], vec!["NAVY"]));
        // The editor's superset (`defined_names`) still lists what is defined, a lone vertex included.
        assert!(defined_names(&m, &map, &media)["volume"].contains(&"Dot".to_string()));
        // Without media.json any id is accepted and none is listed beyond opening_speech.
        assert!(accepted_names(&m, &map, &Value::Null)["message"].is_empty());
        // Every name offered passes the checker in a field of its kind; one not offered is reported.
        let checked =
            [Kind::Group, Kind::Player, Kind::Path, Kind::Volume, Kind::Task, Kind::Point, Kind::PointSet, Kind::Rule, Kind::Speech, Kind::MapText, Kind::Message, Kind::ShipName];
        for kind in checked {
            let (def, field) = RULE_TYPES
                .iter()
                .flat_map(|t| t.fields.iter().map(move |f| (t, f)))
                .filter(|(_, f)| f.kind == kind && f.used)
                .min_by_key(|(t, f)| (t.role == Role::Condition, f.soft))
                .unwrap_or_else(|| panic!("no rule field of kind {}", kind.name()));
            let reported = |name: &str, spec: &str| {
                let mut m = m.clone();
                let list = if def.role == Role::Action { "actions" } else { "conditions" };
                m["rules"][0][list] = json!([{"Type": def.name, field.name: spec}]);
                check_with(&m, &map, &media, false).into_iter().filter(|p| p.field == field.name).map(|p| (name.to_string(), p.message)).collect::<Vec<_>>()
            };
            assert!(!accepted[kind.name()].is_empty(), "{} has names", kind.name());
            for name in &accepted[kind.name()] {
                assert_eq!(reported(name, name), vec![], "{} field {}.{} refuses an offered name", kind.name(), def.name, field.name);
            }
            assert!(!reported("zzz", "Nope zzz").is_empty(), "{} field {}.{} accepts a name nothing defines", kind.name(), def.name, field.name);
        }
        // A group spec's unit part.
        let (def, field) = RULE_TYPES.iter().flat_map(|t| t.fields.iter().map(move |f| (t, f))).find(|(t, f)| t.role == Role::Action && f.kind == Kind::Group).unwrap();
        let spec = |unit: &str| {
            let mut m = m.clone();
            m["rules"][0]["actions"] = json!([{"Type": def.name, field.name: format!("Fleet,{unit}")}]);
            check_with(&m, &map, &media, false).into_iter().filter(|p| p.field == field.name).count()
        };
        assert!(accepted["unit"].iter().all(|u| spec(u) == 0) && spec("Nobody") == 1);
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
    fn a_repeated_world_object_id_is_a_warning_naming_both_places() {
        let mission = json!({"ships": [{"id": 5}, {"id": 9}], "environment": {"animals": [{"id": 5, "group": "W"}], "dragons": [{"id": 12}]}});
        let map = json!({"objects": [{"id": 9, "type": "Rock"}, {"type": "Decor"}, {"type": "Decor"}, {"id": 7}], "ships": [{"id": 7}], "mines": [{"id": 12}, {"id": 12}, {"id": "x"}, {"id": "x"}]});
        let found = duplicate_ids(&mission, &map);
        let at: Vec<_> = found.iter().map(|f| (f.file.as_str(), f.pointer.as_str(), f.level)).collect();
        assert_eq!(
            at,
            [
                ("map.json", "/ships/0/id", Level::Warning),
                ("map.json", "/mines/1/id", Level::Warning),
                ("", "/ships/1/id", Level::Warning),
                ("", "/environment/animals/0/id", Level::Warning),
                ("", "/environment/dragons/0/id", Level::Warning),
            ]
        );
        assert!(found[0].message.contains("id 7 is also used by map.json /objects/3"), "{}", found[0].message);
        assert!(found[2].message.contains("id 9 is also used by map.json /objects/0"), "{}", found[2].message);
        assert!(found[3].message.contains("id 5 is also used by mission /ships/0"), "{}", found[3].message);
        assert!(duplicate_ids(&json!({}), &json!({"objects": [{"id": 1}, {"id": 2}]})).is_empty());
        assert!(duplicate_ids(&Value::Null, &Value::Null).is_empty());
    }

    #[test]
    fn a_nebula_whose_three_records_disagree_is_a_warning() {
        let tri = json!([[0, 0], [10, 0], [10, 10]]);
        let map = json!({"nebulae": [{"name": "Cloud", "polygon": tri}, {"name": "Lonely", "polygon": tri}, {"polygon": tri}]});
        let mission = |records: Value, volumes: Value| json!({"environment": {"nebulae": records}, "volumes": volumes});
        let agree = mission(json!([{"New Nebula Name": "Cloud", "Polygon Name": "Cloud"}, {"New Nebula Name": "Lonely", "Polygon Name": "Cloud"}]), json!({"Cloud": tri}));
        assert!(nebula_agreement(&agree, &map).is_empty(), "{:?}", nebula_agreement(&agree, &map));
        // a map nebula without a record, a record without a map nebula, and a polygon name that is no volume
        let broken =
            mission(json!([{"New Nebula Name": "Cloud", "Polygon Name": "Nowhere"}, {"New Nebula Name": "Ghost", "Polygon Name": "Small"}]), json!({"Small": [[0, 0], [1, 1]]}));
        let found: Vec<_> = nebula_agreement(&broken, &map).into_iter().map(|f| (f.file, f.pointer)).collect();
        assert_eq!(
            found,
            [
                ("map.json".to_string(), "/nebulae/1/name".to_string()),
                ("".to_string(), "/environment/nebulae/0/Polygon Name".to_string()),
                ("".to_string(), "/environment/nebulae/1/New Nebula Name".to_string()),
                ("".to_string(), "/environment/nebulae/1/Polygon Name".to_string()),
            ]
        );
        // the volume is not the map's polygon
        let moved = mission(
            json!([{"New Nebula Name": "Cloud", "Polygon Name": "Cloud"}, {"New Nebula Name": "Lonely", "Polygon Name": "Cloud"}]),
            json!({"Cloud": [[0, 0], [10, 0], [10, 11]]}),
        );
        let found = nebula_agreement(&moved, &map);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].pointer == "/volumes/Cloud" && found[0].message.contains("differs from the polygon of nebula 'Cloud' (map.json /nebulae/0/polygon)"), "{:?}", found[0]);
        // a mission with no nebulae and a map with none say nothing
        assert!(nebula_agreement(&json!({}), &json!({})).is_empty());
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
