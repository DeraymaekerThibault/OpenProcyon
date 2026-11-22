//! Tab completion and "did you mean" for the console: the names the mission defines (rules, groups, point sets, flags, timers, tasks,
//! counters, volumes, paths) and the rule schema's Types and fields, matched against what has been typed.

use std::collections::BTreeSet;

use super::schema::Kind;
use super::*;

/// The names the console completes and checks against; refreshed each frame while it is open (`Script::console_names`).
#[derive(Default, Clone, Debug)]
pub struct Names {
    pub rules: Vec<String>,
    /// Groups, players (owners) and TWT groups a rule can address.
    pub groups: Vec<String>,
    /// Unit names (the part after the comma in 'Group,Unit').
    pub units: Vec<String>,
    pub point_sets: Vec<String>,
    pub volumes: Vec<String>,
    pub paths: Vec<String>,
    pub flags: Vec<String>,
    pub timers: Vec<String>,
    pub tasks: Vec<String>,
    pub counters: Vec<String>,
    /// Point sets and objective points (`show points`).
    pub points: Vec<String>,
    /// Speech events of the mission's media (`say`).
    pub speech: Vec<String>,
}

fn sorted(set: BTreeSet<String>) -> Vec<String> {
    set.into_iter().filter(|s| !s.is_empty()).collect()
}
fn keys(v: &Value) -> impl Iterator<Item = String> + '_ {
    v.as_object().into_iter().flatten().map(|(k, _)| k.clone())
}

impl Script {
    /// What the mission defines, plus every flag, timer and counter its rules mention (they exist once a rule sets them).
    pub fn console_names(&self, w: &[Unit]) -> Names {
        let (mut flags, mut timers, mut counters, mut tasks): (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>, BTreeSet<String>) = Default::default();
        flags.extend(self.flags.keys().cloned());
        timers.extend(self.timers.keys().cloned());
        counters.extend(self.ext.counter_list().into_iter().map(|c| c.0.to_string()));
        tasks.extend(self.tasks.iter().map(|t| t.name.clone()));
        for r in &self.rules {
            for (role, list) in [(Role::Condition, &r.conds), (Role::Action, &r.acts)] {
                for e in list {
                    let Some(def) = schema::lookup(role, s(e, "Type")) else { continue };
                    for f in def.fields {
                        let v = s(e, f.name);
                        let set = match f.kind {
                            Kind::Flag => &mut flags,
                            Kind::Timer => &mut timers,
                            Kind::Counter => &mut counters,
                            Kind::Task => &mut tasks,
                            _ => continue,
                        };
                        if !v.is_empty() && !f.kind.placeholders().contains(&v) {
                            set.insert(v.to_string());
                        }
                    }
                }
            }
        }
        let groups: BTreeSet<String> = self
            .group_names
            .iter()
            .cloned()
            .chain(self.players.iter().cloned())
            .chain(w.iter().flat_map(|u| std::iter::once(u.owner.clone()).chain(u.groups.iter().cloned())))
            .collect();
        Names {
            rules: self.rules.iter().map(|r| r.name.trim().to_string()).collect(),
            groups: sorted(groups),
            units: sorted(w.iter().map(|u| u.name.clone()).collect()),
            point_sets: sorted(keys(&self.point_sets).filter(|k| k != TP_SET).collect()),
            volumes: sorted(self.volumes.keys().cloned().collect()),
            paths: sorted(keys(&self.paths).collect()),
            flags: sorted(flags),
            timers: sorted(timers),
            tasks: sorted(tasks),
            counters: sorted(counters),
            points: self.shape_names(ShapeKind::Points),
            speech: sorted(keys(&self.media["speech"]).collect()),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------- suggestions

fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.to_lowercase().chars().collect(), b.to_lowercase().chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = std::mem::replace(&mut row[0], i + 1);
        for (j, cb) in b.iter().enumerate() {
            let here = (row[j] + 1).min(row[j + 1] + 1).min(prev + usize::from(ca != cb));
            prev = std::mem::replace(&mut row[j + 1], here);
        }
    }
    row[b.len()]
}

/// The letters of a word in order, so that a swap of two ("Gaet") still matches.
fn letters(s: &str) -> Vec<char> {
    let mut v: Vec<char> = s.chars().collect();
    v.sort_unstable();
    v
}

/// The candidate nearest to a misspelled `word`: the same in another case, one that contains it or starts with it, or a few edits away
/// (one edit for short names, two for longer ones). Words under three letters are never guessed.
pub fn nearest_name<'a>(word: &str, items: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let w = word.trim().to_lowercase();
    if w.chars().count() < 3 {
        return None;
    }
    let reach = if w.chars().count() < 8 { 1 } else { 2 };
    items
        .into_iter()
        .filter(|c| {
            let l = c.to_lowercase();
            l == w || l.contains(&w) || w.contains(&l) && l.chars().count() >= 3 || distance(&l, &w) <= reach || (distance(&l, &w) <= 2 && letters(&l) == letters(&w))
        })
        .min_by_key(|c| distance(c, &w))
}

/// ` (did you mean 'X'?)`, or nothing.
pub fn mean<'a>(word: &str, items: impl IntoIterator<Item = &'a str>) -> String {
    nearest_name(word, items).map_or(String::new(), |n| format!("; did you mean '{n}'?"))
}

/// "no group 'X'", with the closest group and, for a unit's name, how a rule writes it.
fn group_error(spec: &str, w: &[Unit], n: &Names) -> String {
    let base = spec.split_once(',').map_or(spec, |x| x.0);
    let unit = w.iter().find(|u| u.name == spec && !u.owner.is_empty()).map_or(String::new(), |u| format!(" (the unit is written '{},{}')", u.owner, u.name));
    format!("no group '{spec}'{}{unit}", mean(base, n.groups.iter().map(String::as_str)))
}

/// Whether a rule's 'Group' or 'Group,Unit' spec names something in the mission.
fn group_known(spec: &str, w: &[Unit], n: &Names) -> bool {
    let (group, unit) = spec.split_once(',').map_or((spec, ""), |(g, u)| (g, u));
    !members(w, spec).is_empty()
        || n.groups.iter().any(|g| g == group) && (unit.is_empty() || n.units.iter().any(|u| u == unit) || unit == "HUMAN CONTROLLED COMMAND SHIP")
        || (group.is_empty() && n.units.iter().any(|u| u == unit))
}

impl Script {
    /// Names in a command that the mission does not define, with the closest ones. Groups, volumes, paths, point sets, tasks and
    /// rules must exist; a flag, timer or counter may be new, so it is only refused when it looks like a misspelling of an existing one.
    pub(in crate::rules) fn check_names(&self, cmd: &Command, w: &[Unit]) -> Result<(), String> {
        let n = self.console_names(w);
        let group = |spec: &str| {
            if spec.is_empty() || group_known(spec, w, &n) {
                return Ok(());
            }
            Err(group_error(spec, w, &n))
        };
        match cmd {
            Command::Repair(spec) => group(spec),
            Command::Teleport(Dest::Set(name)) if !n.point_sets.contains(name) => {
                Err(format!("no point set '{name}'{}; the mission has {}", mean(name, n.point_sets.iter().map(String::as_str)), n.point_sets.join(", ")))
            }
            Command::Acts { acts, god: None } | Command::Soft(acts) => acts.iter().try_for_each(|a| self.check_fields(Role::Action, a, w, &n)),
            Command::Cam(CamMode::Follow(spec)) => group(spec),
            Command::Show(kind, Some(name)) | Command::Hide(Some(kind), Some(name)) => {
                let known = self.shape_names(*kind);
                if known.contains(name) { Ok(()) } else { Err(format!("no {} '{name}'{}", kind.name(), mean(name, known.iter().map(String::as_str)))) }
            }
            Command::Cond(c) => self.check_fields(Role::Condition, c, w, &n),
            _ => Ok(()),
        }
    }

    fn check_fields(&self, role: Role, v: &Value, w: &[Unit], n: &Names) -> Result<(), String> {
        let Some(def) = schema::lookup(role, s(v, "Type")) else { return Ok(()) };
        for f in def.fields {
            let x = s(v, f.name);
            if x.is_empty() || f.kind.placeholders().contains(&x) {
                continue;
            }
            let (known, creatable): (&[String], bool) = match f.kind {
                Kind::Group | Kind::Player => {
                    if group_known(x, w, n) || f.kind == Kind::Player && n.groups.iter().any(|g| g == base(x)) {
                        continue;
                    }
                    return Err(format!("{} (for {} in '{}')", group_error(x, w, n), f.name, def.name));
                }
                Kind::PointSet => (&n.point_sets, false),
                Kind::Volume => (&n.volumes, false),
                Kind::Path => (&n.paths, false),
                Kind::Task => (&n.tasks, false),
                Kind::Rule => (&n.rules, false),
                Kind::Flag => (&n.flags, true),
                Kind::Timer => (&n.timers, true),
                Kind::Counter => (&n.counters, true),
                Kind::Speech if !n.speech.is_empty() => (&n.speech, false),
                _ => continue,
            };
            if known.iter().any(|k| k == x) {
                continue;
            }
            let hint = mean(base(x), known.iter().map(String::as_str));
            if !creatable || !hint.is_empty() {
                return Err(format!("no {} '{x}' for {} in '{}'{hint}", f.kind.name(), f.name, def.name));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------------- tokens

/// A word of the line, with quotes undone. `start` is the byte offset of its first character (the opening quote, if any).
#[derive(Debug, PartialEq)]
pub struct Token {
    pub text: String,
    pub start: usize,
}

/// Words split at whitespace; a double-quoted run is one word. A trailing word still being typed (an open quote, or no whitespace after
/// it) is the last one; whitespace after the last word starts an empty one when `open_end`.
pub fn tokens(s: &str, open_end: bool) -> Vec<Token> {
    let mut out: Vec<Token> = vec![];
    let (mut cur, mut quoted): (Option<Token>, bool) = (None, false);
    for (i, c) in s.char_indices() {
        match c {
            '"' => {
                quoted = !quoted;
                cur.get_or_insert(Token { text: String::new(), start: i });
            }
            c if c.is_whitespace() && !quoted => out.extend(cur.take()),
            c => cur.get_or_insert(Token { text: String::new(), start: i }).text.push(c),
        }
    }
    let ended_on_space = s.chars().last().is_some_and(char::is_whitespace) && !quoted;
    out.extend(cur);
    if open_end && (ended_on_space || s.is_empty()) {
        out.push(Token { text: String::new(), start: s.len() });
    }
    out
}

/// A name as the console reads it back: quoted when it holds whitespace.
fn quoted(name: &str) -> String {
    if name.contains(char::is_whitespace) { format!("\"{name}\"") } else { name.to_string() }
}

// ------------------------------------------------------------------------------------------------------------ complete

/// What Tab can put at the end of the line: each candidate as `(shown, replacement)`; the replacement replaces `line[start..]`.
#[derive(Debug, Default, PartialEq)]
pub struct Completion {
    pub start: usize,
    pub items: Vec<(String, String)>,
}

fn matching<'a>(partial: &str, names: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let p = partial.to_lowercase();
    let all: Vec<&str> = names.into_iter().collect();
    let prefix: Vec<&str> = all.iter().copied().filter(|n| n.to_lowercase().starts_with(&p)).collect();
    if !prefix.is_empty() || p.is_empty() {
        return prefix;
    }
    all.into_iter().filter(|n| n.to_lowercase().contains(&p)).collect()
}

fn pick(start: usize, partial: &str, names: &[String], wrap: impl Fn(&str) -> String) -> Completion {
    let mut hits = matching(partial, names.iter().map(String::as_str));
    hits.dedup();
    Completion { start, items: hits.into_iter().map(|h| (h.to_string(), wrap(h))).collect() }
}
fn words(start: usize, partial: &str, list: &[&str]) -> Completion {
    Completion { start, items: matching(partial, list.iter().copied()).into_iter().map(|w| (w.to_string(), w.to_string())).collect() }
}

/// Completes the line's last word: the command; then, by command, rule, group, point set, flag, timer, task or counter names, schema
/// Types, and inside `act {json}` / `cond {json}` the Type, the field names of that Type and the names a field's kind takes.
pub fn completions(line: &str, n: &Names) -> Completion {
    let Some(split) = line.find(char::is_whitespace) else {
        let w = line.to_lowercase();
        let mut hits: Vec<&str> = TABLE.iter().flat_map(|c| std::iter::once(c.name).chain(c.alias.iter().copied())).filter(|c| c.starts_with(&w)).collect();
        hits.sort_unstable();
        return Completion { start: 0, items: hits.into_iter().map(|h| (h.to_string(), h.to_string())).collect() };
    };
    let Some(cmd) = entry_named(&line[..split].to_lowercase()) else { return Completion::default() };
    let off = split + (line[split..].len() - line[split..].trim_start().len());
    let rest = &line[off..];
    let name_wrap = |s: &str| quoted(s);
    match cmd.name {
        // These take the rest of the line as one name.
        "fire" | "why" | "rules" => {
            let t = rest.strip_prefix('"').unwrap_or(rest);
            pick(off, t, &n.rules, name_wrap)
        }
        "schema" => {
            let t = rest.strip_prefix('"').unwrap_or(rest);
            let types: Vec<String> = type_names(None);
            pick(off, t, &types, name_wrap)
        }
        "act" | "cond" => json_completion(cmd.name == "cond", line, off, n),
        _ => {
            let toks = tokens(rest, true);
            let Some(cur) = toks.last() else { return Completion::default() };
            let (start, partial, index) = (off + cur.start, cur.text.as_str(), toks.len() - 1);
            match (cmd.name, index) {
                ("help", 0) => {
                    let names: Vec<String> = TABLE.iter().map(|e| e.name.to_string()).collect();
                    pick(start, partial, &names, str::to_string)
                }
                ("repair" | "primary" | "ships", 0) | ("damage", 1) | ("attack" | "ally" | "hostile" | "tow" | "untow", 0 | 1) | ("stance", 0) => {
                    pick(start, partial, &n.groups, name_wrap)
                }
                ("stance", 1) => words(start, partial, &["dummy", "neutral", "defensive", "aggressive", "persistent"]),
                ("nis", 0) => words(start, partial, &["end"]),
                ("hud", 0) => words(start, partial, &["on", "off"]),
                ("cam", 0) => words(start, partial, &["free", "player", "follow"]),
                ("cam", 1) if toks[0].text == "follow" => pick(start, partial, &n.groups, name_wrap),
                ("show", 0) => words(start, partial, &["volumes", "paths", "points"]),
                ("hide", 0) => words(start, partial, &["volumes", "paths", "points", "all"]),
                ("show" | "hide", 1) => match ShapeKind::parse(&toks[0].text) {
                    Some(ShapeKind::Volumes) => pick(start, partial, &n.volumes, name_wrap),
                    Some(ShapeKind::Paths) => pick(start, partial, &n.paths, name_wrap),
                    Some(ShapeKind::Points) => pick(start, partial, &n.points, name_wrap),
                    None => Completion::default(),
                },
                ("list", 0) => words(start, partial, list::KINDS),
                ("say", 0) => pick(start, partial, &n.speech, name_wrap),
                ("tp", 0) => pick(start, partial, &n.point_sets, name_wrap),
                ("set", 0) => pick(start, partial, &n.flags, name_wrap),
                ("set", 1) => words(start, partial, &["on", "off"]),
                ("timer", 0) => pick(start, partial, &n.timers, name_wrap),
                ("timer", 1) => words(start, partial, &["start", "stop"]),
                ("task", 0) => pick(start, partial, &n.tasks, name_wrap),
                ("task", 1) => words(start, partial, &["active", "complete", "failed"]),
                ("task", 2) => words(start, partial, &["on", "off"]),
                ("counter", 0) => pick(start, partial, &n.counters, name_wrap),
                _ => Completion::default(),
            }
        }
    }
}

/// Action and condition Type names (`role` None: both).
fn type_names(role: Option<Role>) -> Vec<String> {
    let mut v: Vec<String> = schema::RULE_TYPES.iter().filter(|t| role.is_none_or(|r| t.role == r)).map(|t| t.name.to_string()).collect();
    v.sort();
    v.dedup();
    v
}

/// Where the cursor is in the JSON typed so far.
struct JsonAt {
    /// Byte offset (in the line) just after the opening quote of the string being typed; None when not inside a string.
    open: Option<usize>,
    /// Typing a key (true) or a value.
    key: bool,
    /// The key the value being typed belongs to.
    of: String,
    /// The Type given so far.
    ty: String,
    /// Keys already present.
    used: Vec<String>,
}

fn json_at(line: &str, from: usize) -> JsonAt {
    let mut at = JsonAt { open: None, key: true, of: String::new(), ty: String::new(), used: vec![] };
    let (mut in_str, mut esc) = (false, false);
    let (mut cur, mut last, mut last_key, mut prev_sig) = (String::new(), String::new(), String::new(), ' ');
    for (i, c) in line[from..].char_indices() {
        let i = from + i;
        if in_str {
            if esc {
                esc = false;
                cur.push(c);
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
                last = std::mem::take(&mut cur);
                if prev_sig == ':' && last_key == "Type" {
                    at.ty = last.clone();
                }
                prev_sig = '"';
            } else {
                cur.push(c);
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                at.open = Some(i + 1);
                at.key = prev_sig != ':';
                at.of = last_key.clone();
                cur.clear();
            }
            ':' => {
                last_key = last.clone();
                at.used.push(last.clone());
                prev_sig = ':';
            }
            c if c.is_whitespace() => {}
            c => prev_sig = c,
        }
    }
    if !in_str {
        at.open = None;
    }
    at
}

fn json_completion(cond: bool, line: &str, off: usize, n: &Names) -> Completion {
    let at = json_at(line, off);
    let Some(open) = at.open else { return Completion::default() };
    let partial = &line[open..];
    let role = if cond { Role::Condition } else { Role::Action };
    let close = |s: &str| format!("{s}\"");
    if at.key {
        let def = schema::lookup(role, &at.ty);
        let fields: Vec<String> = match def {
            Some(d) => d.fields.iter().map(|f| f.name.to_string()).filter(|f| !at.used.contains(f)).collect(),
            None if !at.used.iter().any(|u| u == "Type") => vec!["Type".into()],
            None => vec![],
        };
        return pick(open, partial, &fields, |s| format!("{s}\": "));
    }
    if at.of == "Type" {
        return pick(open, partial, &type_names(Some(role)), close);
    }
    let Some(f) = schema::lookup(role, &at.ty).and_then(|d| d.field(&at.of)) else { return Completion::default() };
    let list: Vec<String> = match f.kind {
        Kind::Group | Kind::Player => n.groups.clone(),
        Kind::PointSet => n.point_sets.clone(),
        Kind::Volume => n.volumes.clone(),
        Kind::Path => n.paths.clone(),
        Kind::Task => n.tasks.clone(),
        Kind::Rule => n.rules.clone(),
        Kind::Flag => n.flags.clone(),
        Kind::Timer => n.timers.clone(),
        Kind::Counter => n.counters.clone(),
        Kind::Enum(values) => values.iter().map(|v| v.to_string()).collect(),
        Kind::Bool => vec!["TRUE".into(), "FALSE".into()],
        _ => vec![],
    };
    pick(open, partial, &list, close)
}

#[cfg(test)]
mod tests;
