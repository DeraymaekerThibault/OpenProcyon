//! `list <kind> [text]` and `find <text>`: the names and ids a mission defines, as lines, so a rule's group, path, volume, point, task,
//! timer, flag, counter, player, speech event or scenery object can be found without opening the mission file. Queries: nothing here
//! changes the game.

use super::*;

/// The kinds `list` takes (also what Tab offers after it).
pub const KINDS: &[&str] = &["groups", "paths", "volumes", "points", "tasks", "timers", "flags", "counters", "players", "speech", "objects"];

/// Longest subtitle start shown by `list speech`.
const SUBTITLE: usize = 70;

fn cut(text: &str) -> String {
    let flat = text.replace('\n', " ");
    if flat.chars().count() > SUBTITLE { format!("{}...", flat.chars().take(SUBTITLE).collect::<String>().trim_end()) } else { flat }
}

/// Whether the line contains the text, ignoring case (an empty text matches everything).
fn has(line: &str, text: &str) -> bool {
    text.is_empty() || line.to_lowercase().contains(&text.to_lowercase())
}

/// `kind` as the user may type it: the plural, or the singular.
pub fn parse_kind(word: &str) -> Option<&'static str> {
    let w = word.to_lowercase();
    KINDS.iter().copied().find(|k| *k == w || k.strip_suffix('s') == Some(w.as_str()) || (*k == "counters" && w == "counter"))
}

fn xy(v: &Value) -> Option<(f64, f64)> {
    Some((v.get(0)?.as_f64()?, v.get(1)?.as_f64()?))
}

impl Script {
    /// The id and type of each scenery object (island, base, rock) the mission places.
    fn scenery(&self) -> impl Iterator<Item = (i64, &str, &Value)> {
        self.objects.as_array().into_iter().flatten().map(|o| (o["id"].as_i64().unwrap_or(-1), o["type"].as_str().unwrap_or("?"), &o["pos"]))
    }

    /// What a world object id is called: a unit's name, or the scenery kind.
    fn object_label(&self, id: i64, w: &[Unit]) -> String {
        if let Some(u) = w.iter().find(|u| u.id == id) {
            return if u.name.is_empty() { format!("{id} (unnamed)") } else { format!("{id} {}", u.name) };
        }
        match self.scenery().find(|o| o.0 == id) {
            Some((_, kind, _)) => format!("{id} {kind}"),
            None => id.to_string(),
        }
    }

    pub(in crate::rules) fn flag_lines(&self) -> Vec<String> {
        let mut v: Vec<_> = self.flags.iter().collect();
        v.sort();
        v.into_iter().map(|(k, on)| format!("{k} = {on}")).collect()
    }
    pub(in crate::rules) fn timer_lines(&self) -> Vec<String> {
        let mut v: Vec<_> = self.timers.iter().collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        v.into_iter().map(|(k, t)| format!("{k} = {t:.1} s{}", if self.stopped.contains(k) { " (stopped)" } else { "" })).collect()
    }
    pub(in crate::rules) fn counter_lines(&self) -> Vec<String> {
        self.ext.counter_list().into_iter().map(|(k, v)| format!("{k} = {v}")).collect()
    }
    pub(in crate::rules) fn task_lines(&self) -> Vec<String> {
        self.tasks
            .iter()
            .map(|t| {
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
            })
            .collect()
    }

    /// Every line of one kind, unfiltered.
    fn kind_lines(&self, kind: &str, w: &[Unit]) -> Vec<String> {
        match kind {
            "groups" => {
                let mut names: Vec<&String> = self.group_names.iter().collect();
                names.sort();
                names
                    .into_iter()
                    .map(|g| {
                        let mut ids: Vec<i64> = self.groups.iter().filter(|(_, n)| n.contains(g)).map(|(id, _)| *id).collect();
                        ids.sort_unstable();
                        let members: Vec<String> = ids.iter().map(|id| self.object_label(*id, w)).collect();
                        if members.is_empty() { format!("{g}: no members") } else { format!("{g} ({}): {}", members.len(), members.join(", ")) }
                    })
                    .collect()
            }
            "paths" => {
                let mut v: Vec<(&String, &Value)> = self.paths.as_object().into_iter().flatten().collect();
                v.sort_by(|a, b| a.0.cmp(b.0));
                v.into_iter()
                    .map(|(name, pts)| {
                        let pts = pts.as_array().map_or(&[][..], Vec::as_slice);
                        match (pts.first().and_then(xy), pts.last().and_then(xy)) {
                            (Some(a), Some(b)) => format!("{name}: {} waypoint(s), from ({:.0}, {:.0}) to ({:.0}, {:.0})", pts.len(), a.0, a.1, b.0, b.1),
                            _ => format!("{name}: no waypoints"),
                        }
                    })
                    .collect()
            }
            "volumes" => {
                let mut v: Vec<(&String, &Region)> = self.volumes.iter().collect();
                v.sort_by(|a, b| a.0.cmp(b.0));
                v.into_iter()
                    .map(|(name, r)| {
                        if r.0.is_empty() {
                            return format!("{name}: empty");
                        }
                        let (lo, hi) = r.0.iter().fold((r.0[0], r.0[0]), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
                        format!("{name}: {} corner(s), x {:.0}..{:.0}, y {:.0}..{:.0}", r.0.len(), lo.x, hi.x, lo.y, hi.y)
                    })
                    .collect()
            }
            "points" => {
                let mut lines: Vec<String> = self
                    .points
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|p| match xy(&p["Position"]) {
                        Some((x, y)) => format!("{} (objective point at ({x:.0}, {y:.0}))", p["Name"].as_str().unwrap_or("?")),
                        None => format!("{} (objective point)", p["Name"].as_str().unwrap_or("?")),
                    })
                    .collect();
                let mut sets: Vec<(&String, &Value)> = self.point_sets.as_object().into_iter().flatten().filter(|(k, _)| k.as_str() != TP_SET).collect();
                sets.sort_by(|a, b| a.0.cmp(b.0));
                lines.extend(sets.into_iter().map(|(name, pts)| {
                    let pts = pts.as_array().map_or(&[][..], Vec::as_slice);
                    match pts.first().and_then(|p| xy(&p["pos"])) {
                        Some((x, y)) => format!("{name} (point set, {} point(s), first at ({x:.0}, {y:.0}))", pts.len()),
                        None => format!("{name} (point set, {} point(s))", pts.len()),
                    }
                }));
                lines
            }
            "tasks" => self.task_lines(),
            "timers" => self.timer_lines(),
            "flags" => self.flag_lines(),
            "counters" => self.counter_lines(),
            "players" => self
                .players
                .iter()
                .map(|p| {
                    let ships = w.iter().filter(|u| u.owner == *p).count();
                    format!("{p}: {ships} unit(s){}", if *p == self.player { ", the human player" } else { "" })
                })
                .collect(),
            "speech" => {
                let mut v: Vec<(&String, &Value)> = self.media["speech"].as_object().into_iter().flatten().collect();
                v.sort_by(|a, b| a.0.cmp(b.0));
                v.into_iter()
                    .map(|(id, e)| {
                        let text = e["text"].as_str().unwrap_or("");
                        let played = if self.played.contains(id.as_str()) { " [played]" } else { "" };
                        if text.is_empty() { format!("{id}{played}: (no subtitle)") } else { format!("{id}{played}: {}", cut(text)) }
                    })
                    .collect()
            }
            "objects" => self
                .scenery()
                .map(|(id, kind, pos)| match (xy(pos), pos.get(2).and_then(Value::as_f64)) {
                    (Some((x, y)), z) => format!("#{id} {kind} at ({x:.0}, {y:.0}, {:.0})", z.unwrap_or(0.0)),
                    _ => format!("#{id} {kind}"),
                })
                .collect(),
            _ => vec![],
        }
    }

    /// `list <kind> [text]`: the lines of that kind that contain the text.
    pub(in crate::rules) fn list_lines(&self, kind: &str, text: &str, w: &[Unit]) -> Vec<String> {
        let all = self.kind_lines(kind, w);
        let total = all.len();
        let mut lines: Vec<String> = all.into_iter().filter(|l| has(l, text)).collect();
        if total == 0 {
            lines.push(format!("the mission has no {kind}"));
        } else if lines.is_empty() {
            lines.push(format!("no {kind} contain '{text}' ({total} in all)"));
        } else {
            lines.push(format!("{} of {total} {kind}", lines.len()));
        }
        lines
    }

    /// `find <text>`: every kind at once, each hit marked with its kind ("group Raiders (2): 25 Black Gull, 26 Cutpurse").
    pub(in crate::rules) fn find_lines(&self, text: &str, w: &[Unit]) -> Vec<String> {
        let singular = |k: &str| match k {
            "counters" => "counter".to_string(),
            other => other.trim_end_matches('s').to_string(),
        };
        let mut lines = vec![];
        for kind in KINDS {
            lines.extend(self.kind_lines(kind, w).into_iter().filter(|l| has(l, text)).map(|l| format!("{} {l}", singular(kind))));
        }
        for u in w.iter().filter(|u| !u.owner.is_empty() || !u.name.is_empty()) {
            let line = format!("#{} {} [{}]", u.id, if u.name.is_empty() { "(unnamed)" } else { &u.name }, u.owner);
            if has(&line, text) {
                lines.push(format!("ship {line}"));
            }
        }
        for (i, r) in self.rules.iter().enumerate().filter(|(_, r)| has(&r.name, text)) {
            lines.push(format!("rule #{i} {}", r.name.trim()));
        }
        if lines.is_empty() {
            lines.push(format!("nothing is called or contains '{text}'"));
        } else {
            lines.push(format!("{} hit(s)", lines.len()));
        }
        lines
    }
}

#[cfg(test)]
mod tests;
