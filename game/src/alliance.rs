//! The in-battle player list (docs/re/player-names.md): one row per lobby slot with its flag, name, whether its fleet still floats
//! and, on every row but the human's, the alliance icon, which on free-for-all maps (`Is Alliance Change Allowed`) is a button.
//! State, rules and input live here; the view only draws `Roster` and spawns the `AllianceIcon` / `RosterName` buttons.
#![allow(deprecated)] // bevy 0.20 Interaction, as in panels.rs
use bevy::prelude::*;

use crate::combat::Health;
use crate::scenario::MapObject;
use crate::ship::{FleetMember, Fleets};
use crate::{Orbit, Ship, Side};

#[derive(Clone, PartialEq)]
pub struct Row {
    /// Key of the slot's ships (`FleetMember.0`).
    pub owner: String,
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
    pub fn new(owner: String, name: Option<String>, flag: usize, colour: Color, allied: bool) -> Row {
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
    if let Some((AllianceIcon(i), _)) = icons.iter().find(|(_, i)| **i != Interaction::None) {
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
                && let Some(s) = sk.sides.get_mut(&owner)
            {
                *s = side;
            }
        }
        return;
    }
    if let Some((RosterName(i), _)) = names.iter().find(|(_, i)| **i != Interaction::None) {
        // The camera goes to the player's first ship, when the local player has none left or is allied with it.
        let Some(row) = roster.rows.get(*i).filter(|r| r.alive && !r.local && (!local_alive || r.allied)) else { return };
        if let Some((e, ..)) = ships.iter().filter(|s| s.2.0 == row.owner && !s.3.destroyed).min_by_key(|s| s.2.1) {
            orbit.focus = Some(e);
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
}
