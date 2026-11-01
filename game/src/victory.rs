//! How a skirmish ends. Every retail skirmish map carries its own end-game rules (exported as `victory` in `map.json`,
//! `docs/re/skirmish-setup.md` "Victory conditions"): base destroyed, no ships left, ... This module reads the ones we model and
//! decides the human's outcome from a snapshot of the battle; `scenario::outcome` builds the snapshot. A map with a condition
//! we do not model yet keeps the plain "no enemy ships left" rule. Team points come from the map's own script (rules::skirmish).

use crate::rules::Region;
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

#[derive(Clone, Copy, Debug, PartialEq)]
enum Test {
    Equal,
    Greater,
    Less,
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
        let rules = map["victory"].as_array().filter(|r| !r.is_empty())?;
        let mut out = vec![];
        for r in rules {
            let mut conditions = vec![];
            for c in r["if"].as_array()? {
                conditions.push(condition(c)?);
            }
            let message = |who: &str| r["end"][who].as_str().filter(|m| m.starts_with("IDGS_")).map(String::from);
            out.push(Rule { conditions, winner: r["winner"].as_str().map(String::from), messages: (message("winner"), message("loser")) });
        }
        let mut sides: Vec<Side> = teams.iter().flat_map(|t| t.1.iter().copied()).collect();
        sides.sort_by_key(|s| s.0);
        sides.dedup();
        Some(Victory { rules: out, teams, sides: sides.len() })
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
                    !damages.is_empty()
                        && damages.iter().all(|d| match test {
                            Test::Equal => (d - value).abs() < 0.001 || (*value >= 1.0 && *d >= 0.999),
                            Test::Greater => d > value,
                            Test::Less => d < value,
                        })
                }
                Cond::NoTeamHasShips => alive_sides.is_empty(),
                Cond::NoHumanShips => !human_alive,
                Cond::TeamHasNoShips(team) => self.teams.iter().any(|t| &t.0 == team) && !team_has_ships(team),
                // ponytail: a battle with one side only (a test or dev run) never completes.
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
        "Skirmish Game Complete" => Cond::SkirmishComplete,
        "Team Game Complete" => Cond::TeamComplete,
        // a base's capture comes from the snapshot's captured bases
        "Team has captured a ship from Group/Unit" => Cond::Captured(objects()?),
        "Group Destroyed" => Cond::GroupDestroyed(objects()?),
        "Does Group Contain Unit Name" => {
            Cond::UnitPresent { objects: objects()?, exists: c["Exists?"].as_bool().or_else(|| c["Exists?"].as_str().map(|v| v.eq_ignore_ascii_case("true")))? }
        }
        "Enter Volume" => Cond::InVolume { objects: objects()?, polygon: Region::from_json(c.get("polygon").filter(|p| p.as_array().is_some_and(|a| a.len() > 2))?) },
        "Team has destroyed a ship from Group/Unit" => Cond::DestroyedFrom(objects()?),
        "Team has X points" => Cond::TeamPoints(c["Team Name"].as_str()?.to_string(), c["Points"].as_i64()? as i32),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NAVY: &str = "NAVY";
    const PROC: &str = "PROC";

    fn ship(side: Side, human: bool) -> ShipState {
        ShipState { side, start: side, alive: true, human, id: None, at: Vec2::ZERO }
    }
    fn placed(side: Side, id: i64, alive: bool, at: Vec2) -> ShipState {
        ShipState { side, start: side, alive, human: false, id: Some(id), at }
    }
    fn teams() -> Vec<(String, Vec<Side>)> {
        vec![(NAVY.into(), vec![Side::Player]), (PROC.into(), vec![Side(1)])]
    }
    fn state(ships: &[ShipState], bases: &[(i64, f32)]) -> Snapshot<'static> {
        Snapshot { ships: Box::leak(ships.to_vec().into_boxed_slice()), bases: Box::leak(bases.to_vec().into_boxed_slice()), captured: &[], points: &[] }
    }

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
        Victory::from_map(&map, teams()).unwrap()
    }

    #[test]
    fn destroying_the_enemy_base_wins_and_losing_ours_loses() {
        let v = border_dispute();
        let fleets = [ship(Side::Player, true), ship(Side(1), false)];
        let at = |ours: f32, theirs: f32| v.decide(&state(&fleets, &[(4, ours), (225, theirs)]));
        assert_eq!(at(0.0, 0.5), None);
        assert_eq!(at(0.0, 1.0), Some(Outcome::Victory));
        assert_eq!(at(1.0, 0.0), Some(Outcome::Defeat));
    }

    #[test]
    fn capturing_the_enemy_base_wins() {
        let v = border_dispute();
        let fleets = [ship(Side::Player, true), ship(Side(1), false)];
        let snap = |captured: &'static [i64]| Snapshot { captured, ..state(&fleets, &[(4, 0.0), (225, 0.0)]) };
        assert_eq!(v.decide(&snap(&[])), None);
        assert_eq!(v.decide(&snap(&[225])), Some(Outcome::Victory));
    }

    #[test]
    fn losing_the_human_ship_is_still_a_win_when_allies_cleared_the_enemy() {
        let v = border_dispute();
        let bases = [(4, 0.0), (225, 0.0)];
        let mut dead = ship(Side::Player, true);
        dead.alive = false;
        let ally = ship(Side::Player, false);
        assert_eq!(v.decide(&state(&[dead.clone(), ally, ship(Side(1), false)], &bases)), Some(Outcome::Defeat), "the Procyon fleet still stands");
        let mut gone = ship(Side(1), false);
        gone.alive = false;
        assert_eq!(v.decide(&state(&[dead.clone(), ship(Side::Player, false), gone.clone()], &bases)), Some(Outcome::Victory));
        assert_eq!(v.decide(&state(&[dead, gone], &bases)), Some(Outcome::Defeat), "no ship of ours left");
    }

    #[test]
    fn killing_every_enemy_ship_does_not_end_a_base_map() {
        let v = border_dispute();
        assert_eq!(v.decide(&state(&[ship(Side::Player, true)], &[(4, 0.0), (225, 0.0)])), None);
    }

    #[test]
    fn losing_the_human_fleet_or_every_ship_ends_it_as_a_loss() {
        let v = border_dispute();
        let bases = [(4, 0.0), (225, 0.0)];
        assert_eq!(v.decide(&state(&[ship(Side(1), false)], &bases)), Some(Outcome::Defeat));
        assert_eq!(v.decide(&state(&[], &bases)), Some(Outcome::Defeat));
    }

    #[test]
    fn the_firing_rule_gives_the_humans_message() {
        let map = json!({"victory": [{"name": "w", "winner": NAVY, "end": {"winner": "IDGS_TPINGAMEMESSAGE_A", "loser": "GAME STRING"},
            "if": [{"Type": "Group/Unit Vital Section has >,<,= X damage", "Equivalence": "Equal To", "Damage Percent": 1.0, "objects": [4]}]}]});
        let v = Victory::from_map(&map, teams()).unwrap();
        let fleets = [ship(Side::Player, true), ship(Side(1), false)];
        assert_eq!(v.decide_with_message(&state(&fleets, &[(4, 1.0)])), Some((Outcome::Victory, Some("IDGS_TPINGAMEMESSAGE_A".into()))));
        let map = json!({"victory": [{"name": "l", "winner": PROC, "end": {"winner": "GAME STRING", "loser": "IDGS_TPINGAMEMESSAGE_B"},
            "if": [{"Type": "No Team Has Ships"}]}]});
        let v = Victory::from_map(&map, teams()).unwrap();
        assert_eq!(v.decide_with_message(&state(&[], &[(4, 0.0)])), Some((Outcome::Defeat, Some("IDGS_TPINGAMEMESSAGE_B".into()))));
    }

    #[test]
    fn nothing_decides_before_the_battle_exists() {
        assert_eq!(border_dispute().decide(&state(&[], &[])), None);
    }

    #[test]
    fn game_complete_names_the_last_side_standing() {
        let map = json!({"victory": [{"name": "end", "winner": null, "if": [{"Type": "Skirmish Game Complete"}]}]});
        let v = Victory::from_map(&map, teams()).unwrap();
        let (a, mut b) = (ship(Side::Player, true), ship(Side(1), false));
        assert_eq!(v.decide(&state(&[a.clone(), b.clone()], &[])), None);
        b.alive = false;
        assert_eq!(v.decide(&state(&[a.clone(), b.clone()], &[])), Some(Outcome::Victory));
        let mut a = a;
        (a.alive, b.alive) = (false, true);
        assert_eq!(v.decide(&state(&[a, b], &[])), Some(Outcome::Defeat));
    }

    #[test]
    fn skirmish_complete_also_ends_when_the_human_is_sunk_and_team_complete_needs_one_team_left() {
        let teams = vec![(NAVY.into(), vec![Side::Player, Side(2)]), (PROC.into(), vec![Side(1)])];
        let rule = |t: &str| Victory::from_map(&json!({"victory": [{"name": "end", "winner": null, "if": [{"Type": t}]}]}), teams.clone()).unwrap();
        let (mut human, ally, foe) = (ship(Side::Player, true), ship(Side(2), false), ship(Side(1), false));
        let sunk = |mut s: ShipState| {
            s.alive = false;
            s
        };
        assert_eq!(rule("Skirmish Game Complete").decide(&state(&[human.clone(), ally.clone(), sunk(foe.clone())], &[])), Some(Outcome::Victory), "allies are one team");
        human.alive = false;
        assert_eq!(rule("Skirmish Game Complete").decide(&state(&[human.clone(), ally.clone(), foe.clone()], &[])), Some(Outcome::Defeat), "no human ship: over");
        assert_eq!(rule("Team Game Complete").decide(&state(&[human.clone(), ally.clone(), foe.clone()], &[])), None, "two teams afloat");
        assert_eq!(rule("Team Game Complete").decide(&state(&[human, ally, sunk(foe)], &[])), Some(Outcome::Victory), "a sunk human's team won");
    }

    /// Ambush: reach the volume with the Mercy, or destroy all three pirate groups; losing the Mercy ends it.
    #[test]
    fn ship_group_rules_read_ids_volumes_and_captures() {
        let square = json!([[0, 0], [10, 0], [10, 10], [0, 10]]);
        let map = json!({"victory": [
            {"name": "reach", "winner": NAVY, "if": [{"Type": "Enter Volume", "objects": [129], "polygon": square}]},
            {"name": "taken", "winner": PROC, "if": [{"Type": "Team has captured a ship from Group/Unit", "objects": [129]}]},
            {"name": "sunk", "winner": NAVY, "if": [{"Type": "Group Destroyed", "objects": [130, 131]}, {"Type": "Group Destroyed", "objects": [136]}]},
            {"name": "lost", "winner": null, "if": [{"Type": "Does Group Contain Unit Name", "objects": [129], "Exists?": false}]},
        ]});
        let v = Victory::from_map(&map, teams()).unwrap();
        let far = Vec2::new(500.0, 500.0);
        let mercy = |side, alive, at| ShipState { start: Side::Player, ..placed(side, 129, alive, at) };
        let pirates = |a, b, c| [placed(Side(1), 130, a, far), placed(Side(1), 131, b, far), placed(Side(1), 136, c, far)];
        let ships = |m: ShipState, p: [ShipState; 3]| [vec![m], p.to_vec()].concat();
        let run = |m, p| v.decide(&state(&ships(m, p), &[]));
        assert_eq!(run(mercy(Side::Player, true, far), pirates(true, true, true)), None);
        assert_eq!(run(mercy(Side::Player, true, Vec2::new(5.0, 5.0)), pirates(true, true, true)), Some(Outcome::Victory));
        assert_eq!(run(mercy(Side(1), true, far), pirates(true, true, true)), Some(Outcome::Defeat), "captured");
        assert_eq!(run(mercy(Side::Player, true, far), pirates(false, false, true)), None, "one group still afloat");
        assert_eq!(run(mercy(Side::Player, true, far), pirates(false, false, false)), Some(Outcome::Victory));
        assert_eq!(run(mercy(Side::Player, false, far), pirates(true, true, true)), Some(Outcome::Defeat), "the Mercy sank");
    }

    #[test]
    fn a_map_with_a_condition_we_do_not_model_keeps_the_plain_rule() {
        let map = json!({"victory": [{"name": "x", "winner": NAVY, "if": [{"Type": "Team Member Enters Volume"}]}]});
        assert!(Victory::from_map(&map, vec![]).is_none());
        assert!(Victory::from_map(&json!({}), vec![]).is_none());
    }

    /// The exported rules of the real maps: base maps keep playing after the fleet dies and end on a base; Convoy Raid ends on its
    /// script's team points.
    #[test]
    #[ignore = "needs imported assets"]
    fn the_retail_maps_load() {
        let map = crate::data::map_json("border_dispute").unwrap();
        let real = vec![("IDGS_TPTEAMNAMES_NAVY".to_string(), vec![Side::Player]), ("IDGS_TPTEAMNAMES_PROCYON".to_string(), vec![Side(1)])];
        let v = Victory::from_map(&map, real).expect("Border Dispute's rules are all modelled");
        let fleets = [ship(Side::Player, true), ship(Side(1), false)];
        assert_eq!(v.decide(&state(&fleets, &[(4, 0.0), (225, 1.0)])), Some(Outcome::Victory));
        for id in ["zemyatin", "diablo_straight", "red_rover", "ambush", "bayles_stand", "locusts", "mousetrap", "convoy_raid"] {
            assert!(Victory::from_map(&crate::data::map_json(id).unwrap(), vec![]).is_some(), "{id}");
        }
        let (navy, pirates) = ("IDGS_TPTEAMNAMES_NAVY", "IDGS_TPTEAMNAMES_PIRATES");
        let real = vec![(navy.to_string(), vec![Side::Player]), (pirates.to_string(), vec![Side(1)])];
        let convoy = Victory::from_map(&crate::data::map_json("convoy_raid").unwrap(), real).unwrap();
        let at = |points: &[(String, i32)]| convoy.decide(&Snapshot { ships: &fleets, bases: &[], captured: &[], points: Box::leak(points.to_vec().into_boxed_slice()) });
        assert_eq!(at(&[(navy.into(), 340), (pirates.into(), 390)]), None);
        assert_eq!(at(&[(navy.into(), 350)]), Some(Outcome::Victory));
        assert_eq!(at(&[(pirates.into(), 400)]), Some(Outcome::Defeat));
    }
}
