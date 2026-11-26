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
    Snapshot {
        ships: Box::leak(ships.to_vec().into_boxed_slice()),
        bases: Box::leak(bases.to_vec().into_boxed_slice()),
        captured: &[],
        points: &[],
        table: crate::alliance::empty(),
    }
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
        {"name": "lost", "winner": null, "if": [{"Type": "Does Group Contain Unit Name", "objects": [129], "Exists?": "FALSE"}]},
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
fn a_team_no_map_names_has_no_ships() {
    let rule = |team: &str| json!({"victory": [{"name": "x", "winner": NAVY, "if": [{"Type": "Team X has no ships", "Team Name": team}]}]});
    let fleets = [ship(Side::Player, true), ship(Side(1), false)];
    let at = |team: &str| Victory::from_map(&rule(team), teams()).unwrap().decide(&state(&fleets, &[]));
    assert_eq!(at("NOBODY"), Some(Outcome::Victory), "an unknown team name reads true");
    assert_eq!(at(PROC), None, "a team with ships afloat reads false");
}

/// `Exists?` reads like every rule boolean: only the string "TRUE" (any case) is true; a JSON boolean reads as FALSE, which
/// `--check-mission` reports as an error.
#[test]
fn exists_reads_only_the_retail_strings() {
    let exists = |v: Value| match condition(&json!({"Type": "Does Group Contain Unit Name", "objects": [129], "Exists?": v}), &json!({})) {
        Some(Cond::UnitPresent { exists, .. }) => exists,
        _ => panic!("not parsed"),
    };
    assert!(exists(json!("TRUE")) && exists(json!("true")));
    assert!(!exists(json!("FALSE")) && !exists(json!(true)) && !exists(json!(false)));
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
    let at = |points: &[(String, i32)]| {
        convoy.decide(&Snapshot { ships: &fleets, bases: &[], captured: &[], points: Box::leak(points.to_vec().into_boxed_slice()), table: crate::alliance::empty() })
    };
    assert_eq!(at(&[(navy.into(), 340), (pirates.into(), 390)]), None);
    assert_eq!(at(&[(navy.into(), 350)]), Some(Outcome::Victory));
    assert_eq!(at(&[(pirates.into(), 400)]), Some(Outcome::Defeat));
}

#[test]
fn a_script_end_gives_the_human_the_outcome_of_its_team() {
    let v = Victory::scripted(teams());
    let fleets = [ship(Side::Player, true), ship(Side(1), false)];
    let snap = state(&fleets, &[]);
    let end = |kind| ScriptEnd { kind, winner_message: Some("IDGS_W".into()), loser_message: Some("IDGS_L".into()) };
    assert_eq!(v.decide_end(&snap, &end(EndKind::Team(NAVY.into()))), (Outcome::Victory, Some("IDGS_W".into())));
    assert_eq!(v.decide_end(&snap, &end(EndKind::Team(PROC.into()))), (Outcome::Defeat, Some("IDGS_L".into())));
    assert_eq!(v.decide_end(&snap, &end(EndKind::Loser(PROC.into()))).0, Outcome::Victory);
    assert_eq!(v.decide_end(&snap, &end(EndKind::Loser(NAVY.into()))).0, Outcome::Defeat);
    // Remaining Team Wins: the survivors, when they are one team.
    assert_eq!(v.decide_end(&snap, &end(EndKind::Winner)).0, Outcome::Defeat, "both teams still sail");
    let mut sunk = ship(Side(1), false);
    sunk.alive = false;
    assert_eq!(v.decide_end(&state(&[ship(Side::Player, true), sunk], &[]), &end(EndKind::Winner)).0, Outcome::Victory);
}

#[test]
fn conditions_may_name_groups_of_the_script() {
    let map = json!({"victory": [{"name": "convoy", "winner": NAVY, "if": [{"Type": "Group Destroyed", "objects": ["Convoy", 9]}]}]});
    let groups = json!({"Convoy": [7, 8]});
    let v = Victory::from_map_in(&map, teams(), &groups).unwrap();
    let at = |alive: [bool; 3]| {
        let fleets = [ship(Side::Player, true), placed(Side(1), 7, alive[0], Vec2::ZERO), placed(Side(1), 8, alive[1], Vec2::ZERO), placed(Side(1), 9, alive[2], Vec2::ZERO)];
        v.decide(&state(&fleets, &[]))
    };
    assert_eq!(at([false, false, true]), None);
    assert_eq!(at([false, false, false]), Some(Outcome::Victory));
    let single = json!({"victory": [{"name": "convoy", "winner": NAVY, "if": [{"Type": "Group Destroyed", "group": "Convoy"}]}]});
    assert!(Victory::from_map_in(&single, teams(), &groups).is_some());
    assert!(Victory::from_map_in(&single, teams(), &json!({})).is_none(), "an unknown group makes the rules unusable");
}
