//! The in-battle player list (docs/re/player-names.md): one row per lobby slot with its flag, name, whether its fleet still floats
//! and, on every row but the human's, the alliance icon, which on free-for-all maps (`Is Alliance Change Allowed`) is a button.
//! State, rules and input live here; the view only draws `Roster` and spawns the `AllianceIcon` / `RosterName` buttons.
use bevy::prelude::*;

use crate::combat::Health;
use crate::scenario::MapObject;
use crate::ship::{FleetMember, Fleets};
use crate::{Orbit, Ship, Side};

#[derive(Clone, PartialEq)]
pub struct Row {
    /// Key of the slot's ships (`FleetMember.0`).
    pub owner: crate::names::Owner,
    /// The human's lobby name; none: the computer's label.
    pub name: Option<String>,
    /// Flag icon number (`FrontEnd_Flag_Small_Icon_NN`).
    pub flag: usize,
    pub colour: Color,
    pub local: bool,
    /// The fleet still has a ship afloat; a struck-out name otherwise.
    pub alive: bool,
    /// Allied with the local player (both directions).
    pub allied: bool,
    /// The local player has offered an alliance / this player has offered one to the local player.
    pub request_out: bool,
    pub request_in: bool,
}
impl Row {
    pub fn new(owner: crate::names::Owner, name: Option<String>, flag: usize, colour: Color, allied: bool) -> Row {
        Row { owner, name, flag, colour, local: false, alive: true, allied, request_out: false, request_in: false }
    }
}

/// The rows (lobby slot order) and whether alliances can be changed. Absent when the battle has no lobby slots (the window is then hidden).
#[derive(Resource, Default)]
pub struct Roster {
    pub rows: Vec<Row>,
    pub changeable: bool,
}

/// Marks the alliance icon button of row `.0`; the name and flag of a row are `RosterName`.
#[derive(Component)]
pub struct AllianceIcon(pub usize);
#[derive(Component)]
pub struct RosterName(pub usize);

/// What pressing a row's alliance icon does, from the row's state (retail: the command's four cases).
#[derive(Debug, PartialEq)]
pub enum Press {
    /// Allied: both directions are cleared at once.
    Break,
    /// Only we had offered: the offer is withdrawn.
    Withdraw,
    /// Only they had offered: both directions are set.
    Accept,
    /// Neither: our offer is set (it only becomes an alliance when they press back).
    Offer,
}
pub fn press(allied: bool, out: bool, inn: bool) -> Press {
    match (allied, out, inn) {
        (true, ..) => Press::Break,
        (_, true, false) => Press::Withdraw,
        (_, _, true) => Press::Accept,
        _ => Press::Offer,
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, update.run_if(resource_exists::<Roster>).run_if(in_state(crate::menu::Screen::Battle))).add_systems(
        PreUpdate,
        click.after(bevy::ui::UiSystems::Focus).run_if(resource_exists::<Roster>).run_if(in_state(crate::menu::Screen::Battle)).run_if(crate::menu::overlay_closed),
    );
}

/// Alive and allied are read from the ships each tick (an owner change or a break moves a fleet to another side).
fn update(mut roster: ResMut<Roster>, fleets: Res<Fleets>, ships: Query<(&Ship, &FleetMember, &Health)>) {
    let next: Vec<(bool, bool, bool)> = roster
        .rows
        .iter()
        .map(|r| {
            let mine = || ships.iter().filter(|s| s.1.0 == r.owner);
            (r.owner == fleets.local, mine().any(|s| !s.2.destroyed), mine().next().map_or(r.allied, |s| s.0.side == Side::Player))
        })
        .collect();
    if roster.rows.iter().zip(&next).all(|(r, n)| (r.local, r.alive, r.allied) == *n) {
        return;
    }
    for (r, n) in roster.rows.iter_mut().zip(next) {
        (r.local, r.alive, r.allied) = n;
    }
}

#[allow(deprecated)] // Interaction, see pointer.rs
fn click(
    mouse: Res<ButtonInput<MouseButton>>,
    mut roster: ResMut<Roster>,
    icons: Query<(&AllianceIcon, &Interaction)>,
    names: Query<(&RosterName, &Interaction)>,
    mut orbit: ResMut<Orbit>,
    mut ships: Query<(Entity, &mut Ship, &FleetMember, &Health, Option<&mut MapObject>)>,
    sk: Option<ResMut<crate::rules::skirmish::SkirmishScript>>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let local_alive = roster.rows.iter().any(|r| r.local && r.alive);
    if let Some((AllianceIcon(i), _)) = icons.iter().find(|(_, i)| crate::pointer::over(i)) {
        let i = *i;
        let Some(row) = roster.rows.get(i).filter(|r| roster.changeable && !r.local && local_alive) else { return };
        let (owner, side) = match press(row.allied, row.request_out, row.request_in) {
            Press::Break => (row.owner.clone(), Some(Side(ships.iter().map(|s| s.1.side.0).max().unwrap_or(1) + 1))),
            Press::Accept => (row.owner.clone(), Some(Side::Player)),
            _ => (row.owner.clone(), None),
        };
        let row = &mut roster.rows[i];
        match press(row.allied, row.request_out, row.request_in) {
            Press::Withdraw => row.request_out = false,
            Press::Offer => row.request_out = true,
            Press::Break | Press::Accept => (row.request_out, row.request_in) = (false, false),
        }
        if let Some(side) = side {
            row.allied = side == Side::Player;
            for (_, mut ship, m, _, object) in &mut ships {
                if m.0 == owner {
                    ship.side = side;
                    if let Some(mut o) = object {
                        o.start = side;
                    }
                }
            }
            if let Some(mut sk) = sk
                && let Some(s) = sk.sides.get_mut(owner.as_str())
            {
                *s = side;
            }
        }
        return;
    }
    if let Some((RosterName(i), _)) = names.iter().find(|(_, i)| crate::pointer::over(i)) {
        // The camera goes to the player's first ship, when the local player has none left or is allied with it.
        let Some(row) = roster.rows.get(*i).filter(|r| r.alive && !r.local && (!local_alive || r.allied)) else { return };
        if let Some((e, ..)) = ships.iter().filter(|s| s.2.0 == row.owner && !s.3.destroyed).min_by_key(|s| s.2.1) {
            orbit.focus = Some(e);
        }
    }
}

/// The answers to "is X hostile to Y" that the battle code gives today, each under its own name. They are NOT one rule and do not
/// always agree (see `three_factions_show_where_the_two_answers_part`); callers keep the one they always used. MAINTAINER QUESTION:
/// which of them is the original's?
pub mod hostility {
    use crate::campaign::{Mission, MissionBase};
    use crate::{Ship, Side};

    /// By skirmish side: hostile when the sides differ. Used by gunnery, shells, cloak, crew and the captain's target check.
    pub fn by_side(a: Side, b: Side) -> bool {
        a != b
    }

    /// By the mission script's alliance table (owner names): hostile when the pair is at war; false for one owner and for non-players.
    pub fn by_script(script: &crate::rules::Script, a: &str, b: &str) -> bool {
        script.hostile(a, b)
    }

    /// Two objects (owner, side): by the mission's table when both have owners in a mission, else by side when both have one.
    pub fn pair(mission: Option<&Mission>, a: (Option<&str>, Option<Side>), b: (Option<&str>, Option<Side>)) -> bool {
        if let (Some(m), Some(x), Some(y)) = (mission, a.0, b.0) {
            return by_script(&m.script, x, y);
        }
        matches!((a.1, b.1), (Some(x), Some(y)) if by_side(x, y))
    }

    /// A mission base is hostile when the script says so of its owner and the player; in a skirmish when its side is not the player's.
    pub fn base(mission: Option<&Mission>, base: Option<&MissionBase>, side: Option<&crate::combat::BaseSide>) -> bool {
        let Some(base) = base else { return false };
        match mission {
            Some(m) => by_script(&m.script, m.script.player(), &base.owner),
            None => side.is_some_and(|s| s.0 != Side::Player),
        }
    }

    /// A dock owner is a friend of `me` (its owner, in a mission) when the script does not call them hostile; without a mission or an
    /// owner on either side, when the dock's ship (if any) is on `me`'s side.
    pub fn friendly_dock(script: Option<&crate::rules::Script>, my_owner: Option<&str>, my_side: Side, dock_ship: Option<&Ship>, dock_owner: Option<&str>) -> bool {
        match (my_owner, script, dock_owner) {
            (Some(m), Some(sc), Some(o)) => !by_script(sc, m, o),
            _ => dock_ship.is_none_or(|d| !by_side(d.side, my_side)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_follows_the_retail_command_cases() {
        assert_eq!(press(true, false, false), Press::Break);
        assert_eq!(press(false, true, false), Press::Withdraw);
        assert_eq!(press(false, false, true), Press::Accept);
        assert_eq!(press(false, true, true), Press::Accept);
        assert_eq!(press(false, false, false), Press::Offer);
    }

    /// Three factions, A and B each at war with the player and with each other: by script they are hostile to each other, by side
    /// both sit on the enemy side and are "the same". The skirmish-side answer (gunnery, shells, cloak, crew) and the script answer
    /// (mission AI, boarding, docking) disagree here today. MAINTAINER QUESTION: which does the original use for three-way maps?
    #[test]
    fn three_factions_show_where_the_two_answers_part() {
        let data = serde_json::json!({"rules": [], "players": ["Hero", "A", "B"], "alliances": []});
        let sc = crate::rules::Script::new(&data, serde_json::Value::Null);
        // Mission ships are sorted into two sides: the player's group and everyone hostile to it.
        let side = |owner: &str| if hostility::by_script(&sc, owner, "Hero") { Side::Enemy } else { Side::Player };
        for (x, y) in [("A", "Hero"), ("B", "Hero"), ("Hero", "A")] {
            assert!(hostility::by_script(&sc, x, y) && hostility::by_side(side(x), side(y)), "{x} vs {y}: both answers say hostile");
        }
        assert!(hostility::by_script(&sc, "A", "B") && hostility::by_script(&sc, "B", "A"));
        assert!(!hostility::by_side(side("A"), side("B")), "by side, A and B are on the same (enemy) side");
        assert!(!hostility::by_script(&sc, "A", "A"));
    }
}
