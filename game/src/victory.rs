//! How a skirmish ends. Every retail skirmish map carries its own end-game rules (exported as `victory` in `map.json`,
//! `docs/re/skirmish-setup.md` "Victory conditions"): base destroyed, no ships left, ... This module reads the ones we model and
//! decides the human's outcome from a snapshot of the battle; `scenario::outcome` builds the snapshot. A map with a condition
//! we do not model yet (volumes, points, group kills) keeps the plain "no enemy ships left" rule.

use crate::scenario::Outcome;
use crate::ship::Side;
use bevy::prelude::*;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
enum Cond {
    /// A base group's mission-critical damage (0-1) compared with the rule's number.
    BaseDamage {
        objects: Vec<i64>,
        test: Test,
        value: f32,
    },
    NoTeamHasShips,
    NoHumanShips,
    TeamHasNoShips(String),
    /// "Skirmish Game Complete" / "Team Game Complete". PROVISIONAL: at most one side still has ships.
    GameComplete,
    /// Understood but never true here: capturing a ship from a base (boarding bases is not finished).
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Test {
    Equal,
    Greater,
    Less,
}

#[derive(Clone, Debug)]
struct Rule {
    conditions: Vec<Cond>,
    /// Team name id the rule declares the winner; none for draws and for the game-complete rules (the last side wins).
    winner: Option<String>,
}

/// The end rules of the running skirmish and the sides of each map team (a lobby team can hold several sides).
#[derive(Resource, Debug)]
pub struct Victory {
    rules: Vec<Rule>,
    teams: Vec<(String, Vec<Side>)>,
    sides: usize,
}

/// What `decide` looks at: the living ships (side, commanded by the human) and each base's mission damage by object id.
pub struct Snapshot<'a> {
    pub alive: &'a [(Side, bool)],
    pub bases: &'a [(i64, f32)],
}

impl Victory {
    /// The map's rules, or none when the map has none or one of them needs something not modelled. `teams`: map team name id and
    /// the sides of the fleets on it.
    pub fn from_map(map: &Value, teams: Vec<(String, Vec<Side>)>) -> Option<Victory> {
        let rules = map["victory"].as_array().filter(|r| !r.is_empty())?;
        let mut out = vec![];
        for r in rules {
            let mut conditions = vec![];
            for c in r["if"].as_array()? {
                conditions.push(condition(c)?);
            }
            out.push(Rule { conditions, winner: r["winner"].as_str().map(String::from) });
        }
        let mut sides: Vec<Side> = teams.iter().flat_map(|t| t.1.iter().copied()).collect();
        sides.sort_by_key(|s| s.0);
        sides.dedup();
        Some(Victory { rules: out, teams, sides: sides.len() })
    }

    /// The human's outcome once a rule fires (the first in map order), or none while the battle goes on.
    pub fn decide(&self, snap: &Snapshot) -> Option<Outcome> {
        if snap.alive.is_empty() && snap.bases.is_empty() {
            return None; // nothing spawned yet
        }
        let alive_sides = {
            let mut s: Vec<u8> = snap.alive.iter().map(|a| a.0.0).collect();
            s.sort_unstable();
            s.dedup();
            s
        };
        let team_has_ships = |name: &str| self.teams.iter().any(|(n, sides)| n == name && sides.iter().any(|s| alive_sides.contains(&s.0)));
        for rule in &self.rules {
            let hit = rule.conditions.iter().all(|c| match c {
                Cond::BaseDamage { objects, test, value } => {
                    let damages: Vec<f32> = snap.bases.iter().filter(|b| objects.contains(&b.0)).map(|b| b.1).collect();
                    !damages.is_empty()
                        && damages.iter().all(|d| match test {
                            Test::Equal => (d - value).abs() < 0.001 || (*value >= 1.0 && *d >= 0.999),
                            Test::Greater => d > value,
                            Test::Less => d < value,
                        })
                }
                Cond::NoTeamHasShips => snap.alive.is_empty(),
                Cond::NoHumanShips => !snap.alive.iter().any(|a| a.1),
                Cond::TeamHasNoShips(team) => self.teams.iter().any(|t| &t.0 == team) && !team_has_ships(team),
                Cond::GameComplete => self.sides >= 2 && alive_sides.len() <= 1,
                Cond::Never => false,
            });
            if !hit {
                continue;
            }
            let human_wins = match &rule.winner {
                Some(team) => self.teams.iter().any(|(n, sides)| n == team && sides.contains(&Side::Player)),
                None => rule.conditions.contains(&Cond::GameComplete) && alive_sides == [Side::Player.0],
            };
            return Some(if human_wins { Outcome::Victory } else { Outcome::Defeat });
        }
        None
    }
}

fn condition(c: &Value) -> Option<Cond> {
    let objects = || c["objects"].as_array().map(|o| o.iter().filter_map(Value::as_i64).collect::<Vec<_>>());
    Some(match c["Type"].as_str()? {
        "Group/Unit Vital Section has >,<,= X damage" => Cond::BaseDamage {
            objects: objects()?,
            test: match c["Equivalence"].as_str()? {
                "Equal To" => Test::Equal,
                "Greater Than" => Test::Greater,
                "Less Than" => Test::Less,
                _ => return None,
            },
            value: c["Damage Percent"].as_f64()? as f32,
        },
        "No Team Has Ships" => Cond::NoTeamHasShips,
        "No Human Controlled Ships Remain" => Cond::NoHumanShips,
        "Team X has no ships" => Cond::TeamHasNoShips(c["Team Name"].as_str()?.to_string()),
        "Skirmish Game Complete" | "Team Game Complete" => Cond::GameComplete,
        "Team has captured a ship from Group/Unit" => Cond::Never,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NAVY: &str = "NAVY";
    const PROC: &str = "PROC";

    fn border_dispute() -> Victory {
        let base = |name: &str, base: i64, winner: &str| {
            json!({"name": name, "winner": winner, "if": [{"Type": "Group/Unit Vital Section has >,<,= X damage", "Equivalence": "Equal To",
                "Damage Percent": 1.0, "objects": [base]}]})
        };
        let map = json!({"victory": [
            base("procwin", 4, PROC),
            base("navywin", 225, NAVY),
            {"name": "draw", "winner": null, "if": [{"Type": "No Team Has Ships"}]},
            {"name": "dead humans", "winner": null, "if": [{"Type": "No Human Controlled Ships Remain"}]},
            {"name": "captured", "winner": NAVY, "if": [{"Type": "Team has captured a ship from Group/Unit", "objects": [225]}]},
        ]});
        Victory::from_map(&map, vec![(NAVY.into(), vec![Side::Player]), (PROC.into(), vec![Side(1)])]).unwrap()
    }

    #[test]
    fn destroying_the_enemy_base_wins_and_losing_ours_loses() {
        let v = border_dispute();
        let fleets = [(Side::Player, true), (Side(1), false)];
        let state = |ours: f32, theirs: f32| v.decide(&Snapshot { alive: &fleets, bases: &[(4, ours), (225, theirs)] });
        assert_eq!(state(0.0, 0.5), None);
        assert_eq!(state(0.0, 1.0), Some(Outcome::Victory));
        assert_eq!(state(1.0, 0.0), Some(Outcome::Defeat));
    }

    #[test]
    fn killing_every_enemy_ship_does_not_end_a_base_map() {
        let v = border_dispute();
        assert_eq!(v.decide(&Snapshot { alive: &[(Side::Player, true)], bases: &[(4, 0.0), (225, 0.0)] }), None);
    }

    #[test]
    fn losing_the_human_fleet_or_every_ship_ends_it_as_a_loss() {
        let v = border_dispute();
        let bases = [(4, 0.0), (225, 0.0)];
        assert_eq!(v.decide(&Snapshot { alive: &[(Side(1), false)], bases: &bases }), Some(Outcome::Defeat));
        assert_eq!(v.decide(&Snapshot { alive: &[], bases: &bases }), Some(Outcome::Defeat));
    }

    #[test]
    fn nothing_decides_before_the_battle_exists() {
        assert_eq!(border_dispute().decide(&Snapshot { alive: &[], bases: &[] }), None);
    }

    #[test]
    fn game_complete_names_the_last_side_standing() {
        let map = json!({"victory": [{"name": "end", "winner": null, "if": [{"Type": "Skirmish Game Complete"}]}]});
        let v = Victory::from_map(&map, vec![("A".into(), vec![Side::Player]), ("B".into(), vec![Side(1)])]).unwrap();
        let bases: [(i64, f32); 0] = [];
        assert_eq!(v.decide(&Snapshot { alive: &[(Side::Player, true), (Side(1), false)], bases: &bases }), None);
        assert_eq!(v.decide(&Snapshot { alive: &[(Side::Player, true)], bases: &bases }), Some(Outcome::Victory));
        assert_eq!(v.decide(&Snapshot { alive: &[(Side(1), false)], bases: &bases }), Some(Outcome::Defeat));
    }

    #[test]
    fn a_map_with_a_condition_we_do_not_model_keeps_the_plain_rule() {
        let map = json!({"victory": [{"name": "x", "winner": NAVY, "if": [{"Type": "Team has X points", "Points": 350}]}]});
        assert!(Victory::from_map(&map, vec![]).is_none());
        assert!(Victory::from_map(&json!({}), vec![]).is_none());
    }

    /// The exported rules of the real open maps: a base map keeps playing after the fleet dies and ends on its base; a map
    /// without bases ends on the last team.
    #[test]
    fn the_retail_open_maps_load() {
        let sides = vec![("IDGS_TPTEAMNAMES_NAVY".to_string(), vec![Side::Player]), ("IDGS_TPTEAMNAMES_PROCYON".to_string(), vec![Side(1)])];
        let map = crate::data::map_json("border_dispute").unwrap();
        let v = Victory::from_map(&map, sides).expect("Border Dispute's rules are all modelled");
        let fleets = [(Side::Player, true), (Side(1), false)];
        assert_eq!(v.decide(&Snapshot { alive: &fleets, bases: &[(4, 0.0), (225, 1.0)] }), Some(Outcome::Victory));
        for id in ["zemyatin", "diablo_straight", "red_rover"] {
            assert!(Victory::from_map(&crate::data::map_json(id).unwrap(), vec![]).is_some(), "{id}");
        }
        assert!(Victory::from_map(&crate::data::map_json("convoy_raid").unwrap(), vec![]).is_none(), "points rules are not modelled");
    }
}
