//! How a skirmish ends. Every retail skirmish map carries its own end-game rules (exported as `victory` in `map.json`,
//! `docs/re/skirmish-setup.md` "Victory conditions"): base destroyed, no ships left, ... This module reads the ones we model and
//! decides the human's outcome from a snapshot of the battle; `scenario::outcome` builds the snapshot. A map with a condition
//! we do not model yet keeps the plain "no enemy ships left" rule. Team points come from the map's own script (rules::skirmish).

use crate::rules::schema::Equivalence;
use crate::rules::{EndKind, Region, ScriptEnd};
use crate::scenario::Outcome;
use crate::ship::Side;
use bevy::prelude::*;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
enum Cond {
    /// A base group's mission-critical damage (0-1) compared with the rule's number.
    BaseDamage {
        objects: Vec<i64>,
        test: Equivalence,
        value: f32,
    },
    NoTeamHasShips,
    NoHumanShips,
    TeamHasNoShips(String),
    /// "Skirmish Game Complete": every ship left sails for one team (allies), or no human ship is left.
    SkirmishComplete,
    /// "Team Game Complete": exactly one team still has ships.
    TeamComplete,
    /// Every ship of the group is gone (and the group has ships).
    GroupDestroyed(Vec<i64>),
    /// A named unit is (or is not) still afloat.
    UnitPresent {
        objects: Vec<i64>,
        exists: bool,
    },
    /// A living ship of the group is inside the volume (game x/y polygon).
    InVolume {
        objects: Vec<i64>,
        polygon: Region,
    },
    /// A living ship of the group sails for another side than it started with. PROVISIONAL: the capturing team is not checked.
    Captured(Vec<i64>),
    /// A ship of the group was destroyed. PROVISIONAL: who destroyed it is not checked.
    DestroyedFrom(Vec<i64>),
    /// The team has scored at least this many points through the map's script (rules::skirmish).
    TeamPoints(String, i32),
}

#[derive(Clone, Debug)]
struct Rule {
    conditions: Vec<Cond>,
    /// Team name id the rule declares the winner; none: the survivors win if they are one team, else nobody does.
    winner: Option<String>,
    /// String ids of the message shown to the winners and to the losers (`end` in `map.json`); none for the default "GAME STRING".
    messages: (Option<String>, Option<String>),
}

/// The end rules of the running skirmish and the sides of each map team (a lobby team can hold several sides).
#[derive(Resource, Debug)]
pub struct Victory {
    rules: Vec<Rule>,
    teams: Vec<(String, Vec<Side>)>,
    sides: usize,
}

/// One ship as the rules see it.
#[derive(Clone, Debug)]
pub struct ShipState {
    pub side: Side,
    /// The side it started on (a captured ship has changed).
    pub start: Side,
    pub alive: bool,
    /// Commanded by the human player.
    pub human: bool,
    /// The map's world-object id, for ships placed by the map.
    pub id: Option<i64>,
    /// Game x/y.
    pub at: Vec2,
}

/// What `decide` looks at: every ship (also the destroyed ones), each base's mission damage by object id and the teams' points.
pub struct Snapshot<'a> {
    pub ships: &'a [ShipState],
    pub bases: &'a [(i64, f32)],
    /// Ids of the bases a boarding party took.
    pub captured: &'a [i64],
    pub points: &'a [(String, i32)],
}

impl Victory {
    /// The map's rules, or none when the map has none or one of them needs something not modelled. `teams`: map team name id and
    /// the sides of the fleets on it.
    pub fn from_map(map: &Value, teams: Vec<(String, Vec<Side>)>) -> Option<Victory> {
        Victory::from_map_in(map, teams, &Value::Null)
    }

    /// `from_map`, where a condition may name a group of the map's script (`groups`: name -> world-object ids) instead of listing
    /// object ids: `"objects": [12, "Convoy"]` or `"group": "Convoy"`. A group the script does not define makes the rules unusable
    /// (the plain "no enemy ships left" outcome stays), with a warning.
    pub fn from_map_in(map: &Value, teams: Vec<(String, Vec<Side>)>, groups: &Value) -> Option<Victory> {
        let rules = map["victory"].as_array().filter(|r| !r.is_empty())?;
        let mut out = vec![];
        for r in rules {
            let mut conditions = vec![];
            for c in r["if"].as_array()? {
                conditions.push(condition(c, groups)?);
            }
            let message = |who: &str| r["end"][who].as_str().filter(|m| m.starts_with("IDGS_")).map(String::from);
            out.push(Rule { conditions, winner: r["winner"].as_str().map(String::from), messages: (message("winner"), message("loser")) });
        }
        let mut sides: Vec<Side> = teams.iter().flat_map(|t| t.1.iter().copied()).collect();
        sides.sort_by_key(|s| s.0);
        sides.dedup();
        Some(Victory { rules: out, teams, sides: sides.len() })
    }

    /// A map whose own script ends the game (`"victory": "script"`) and has no usable end rules in `map.json`: only `decide_end`
    /// speaks.
    pub fn scripted(teams: Vec<(String, Vec<Side>)>) -> Victory {
        let mut sides: Vec<Side> = teams.iter().flat_map(|t| t.1.iter().copied()).collect();
        sides.sort_by_key(|s| s.0);
        sides.dedup();
        Victory { rules: vec![], teams, sides: sides.len() }
    }

    /// The name of the human's team.
    fn ours(&self) -> String {
        self.teams.iter().find(|(_, sides)| sides.contains(&Side::Player)).map_or_else(|| format!("side {}", Side::Player.0), |t| t.0.clone())
    }

    /// The human's outcome once the map's script has ended the game (`Script::end`): the team that wins is the one it names, the
    /// survivors when they are one team, or every team but the one whose required objective failed. The message is the one the
    /// script gave the human's side.
    pub fn decide_end(&self, snap: &Snapshot, end: &ScriptEnd) -> (Outcome, Option<String>) {
        let ours = self.ours();
        let human_wins = match &end.kind {
            EndKind::Team(team) => *team == ours,
            EndKind::Loser(team) => *team != ours,
            // The map teams with ships afloat (a side on no team counts as its own); mutual allies are one team.
            EndKind::Winner => {
                let mut teams: Vec<String> = snap
                    .ships
                    .iter()
                    .filter(|a| a.alive)
                    .map(|a| self.teams.iter().find(|t| t.1.contains(&a.side)).map_or_else(|| format!("side {}", a.side.0), |t| t.0.clone()))
                    .collect();
                teams.sort();
                teams.dedup();
                teams == [ours]
            }
        };
        if human_wins { (Outcome::Victory, end.winner_message.clone()) } else { (Outcome::Defeat, end.loser_message.clone()) }
    }

    /// The human's outcome once a rule fires (the first in map order), or none while the battle goes on.
    pub fn decide(&self, snap: &Snapshot) -> Option<Outcome> {
        self.decide_with_message(snap).map(|d| d.0)
    }

    /// `decide`, plus the string id of the message the firing rule gives the human's side (none: the default VICTORY/DEFEAT).
    pub fn decide_with_message(&self, snap: &Snapshot) -> Option<(Outcome, Option<String>)> {
        if snap.ships.is_empty() && snap.bases.is_empty() {
            return None; // nothing spawned yet
        }
        let alive_sides = {
            let mut s: Vec<u8> = snap.ships.iter().filter(|a| a.alive).map(|a| a.side.0).collect();
            s.sort_unstable();
            s.dedup();
            s
        };
        let team_has_ships = |name: &str| self.teams.iter().any(|(n, sides)| n == name && sides.iter().any(|s| alive_sides.contains(&s.0)));
        // The map teams with ships afloat (a side on no team counts as its own); mutual allies are one team.
        let mut alive_teams: Vec<String> =
            alive_sides.iter().map(|s| self.teams.iter().find(|t| t.1.iter().any(|x| x.0 == *s)).map_or_else(|| format!("side {s}"), |t| t.0.clone())).collect();
        alive_teams.sort();
        alive_teams.dedup();
        let human_alive = snap.ships.iter().any(|a| a.alive && a.human);
        let of = |objects: &[i64]| snap.ships.iter().filter(|s| s.id.is_some_and(|i| objects.contains(&i))).collect::<Vec<_>>();
        for rule in &self.rules {
            let hit = rule.conditions.iter().all(|c| match c {
                Cond::BaseDamage { objects, test, value } => {
                    let damages: Vec<f32> = snap.bases.iter().filter(|b| objects.contains(&b.0)).map(|b| b.1).collect();
                    !damages.is_empty() && damages.iter().all(|d| test.holds(*d, *value, 0.001) || (*test == Equivalence::EqualTo && *value >= 1.0 && *d >= 0.999))
                }
                Cond::NoTeamHasShips => alive_sides.is_empty(),
                Cond::NoHumanShips => !human_alive,
                // A team name no map team has reads true, as in the original.
                Cond::TeamHasNoShips(team) => !team_has_ships(team),
                // A battle with one side only (a test or dev run) never completes.
                Cond::SkirmishComplete => self.sides >= 2 && (alive_teams.len() <= 1 || !human_alive),
                Cond::TeamComplete => self.sides >= 2 && alive_teams.len() == 1,
                Cond::GroupDestroyed(objects) => {
                    let group = of(objects);
                    !group.is_empty() && group.iter().all(|s| !s.alive)
                }
                Cond::UnitPresent { objects, exists } => {
                    let group = of(objects);
                    !group.is_empty() && group.iter().any(|s| s.alive) == *exists
                }
                Cond::InVolume { objects, polygon } => of(objects).iter().any(|s| s.alive && polygon.contains(s.at)),
                Cond::Captured(objects) => of(objects).iter().any(|s| s.alive && s.side != s.start) || snap.captured.iter().any(|b| objects.contains(b)),
                Cond::DestroyedFrom(objects) => of(objects).iter().any(|s| !s.alive),
                Cond::TeamPoints(team, points) => snap.points.iter().any(|(t, p)| t == team && p >= points),
            });
            if !hit {
                continue;
            }
            // The winners: the named team's players (sunk or not); with no team named, the survivors when they are one team, and
            // all their allies (a sunk human whose allies cleared the field wins: retail Border Dispute read VICTORY!), else nobody.
            let ours = self.teams.iter().find(|(_, sides)| sides.contains(&Side::Player)).map_or_else(|| format!("side {}", Side::Player.0), |t| t.0.clone());
            let human_wins = match &rule.winner {
                Some(team) => *team == ours,
                None => alive_teams == [ours],
            };
            let (win, lose) = &rule.messages;
            return Some(if human_wins { (Outcome::Victory, win.clone()) } else { (Outcome::Defeat, lose.clone()) });
        }
        None
    }
}

fn condition(c: &Value, groups: &Value) -> Option<Cond> {
    // The object ids of the condition, and the ids of the script's groups named in `objects` or `group`.
    let objects = || {
        let mut ids: Vec<i64> = vec![];
        let mut named: Vec<&str> = c["group"].as_str().into_iter().collect();
        for o in c["objects"].as_array().map(Vec::as_slice).unwrap_or_default() {
            match (o.as_i64(), o.as_str()) {
                (Some(id), _) => ids.push(id),
                (_, Some(name)) => named.push(name),
                _ => {}
            }
        }
        if c["objects"].as_array().is_none() && named.is_empty() {
            return None;
        }
        for name in named {
            let Some(members) = groups[name].as_array() else {
                warn!("skirmish victory rule: unknown group '{name}' (a condition may name the groups of the map's script)");
                return None;
            };
            ids.extend(members.iter().filter_map(Value::as_i64));
        }
        Some(ids)
    };
    Some(match c["Type"].as_str()? {
        "Group/Unit Vital Section has >,<,= X damage" => {
            Cond::BaseDamage { objects: objects()?, test: Equivalence::parse(c["Equivalence"].as_str()?)?, value: c["Damage Percent"].as_f64()? as f32 }
        }
        "No Team Has Ships" => Cond::NoTeamHasShips,
        "No Human Controlled Ships Remain" => Cond::NoHumanShips,
        "Team X has no ships" => Cond::TeamHasNoShips(c["Team Name"].as_str()?.to_string()),
        "Skirmish Game Complete" => Cond::SkirmishComplete,
        "Team Game Complete" => Cond::TeamComplete,
        // a base's capture comes from the snapshot's captured bases
        "Team has captured a ship from Group/Unit" => Cond::Captured(objects()?),
        "Group Destroyed" => Cond::GroupDestroyed(objects()?),
        "Does Group Contain Unit Name" => Cond::UnitPresent { objects: objects()?, exists: crate::data::wot_flag(&c["Exists?"]) },
        "Enter Volume" => Cond::InVolume { objects: objects()?, polygon: Region::from_json(c.get("polygon").filter(|p| p.as_array().is_some_and(|a| a.len() > 2))?) },
        "Team has destroyed a ship from Group/Unit" => Cond::DestroyedFrom(objects()?),
        "Team has X points" => Cond::TeamPoints(c["Team Name"].as_str()?.to_string(), c["Points"].as_i64()? as i32),
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
