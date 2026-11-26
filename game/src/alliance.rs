//! The in-battle player list (docs/re/player-names.md): one row per lobby slot with its flag, name, whether its fleet still floats
//! and, on every row but the human's, the alliance icon, which on free-for-all maps (`Is Alliance Change Allowed`) is a button.
//! State, rules and input live here (with the battle's alliance table, `Alliances`); the view only draws `Roster` and spawns the
//! `AllianceIcon` / `RosterName` buttons.
use bevy::prelude::*;

use crate::combat::Health;
use crate::names::Owner;
use crate::ship::{FleetMember, Fleets};
use crate::{Orbit, Side};

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
    /// The slot's side in the alliance table.
    pub side: Side,
}
impl Row {
    pub fn new(owner: crate::names::Owner, name: Option<String>, flag: usize, colour: Color, side: Side) -> Row {
        Row { owner, name, flag, colour, local: false, alive: true, allied: side == Side::Player, request_out: false, request_in: false, side }
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
    app.init_resource::<Alliances>().add_systems(Update, update.run_if(resource_exists::<Roster>).run_if(in_state(crate::menu::Screen::Battle))).add_systems(
        PreUpdate,
        click.after(bevy::ui::UiSystems::Focus).run_if(resource_exists::<Roster>).run_if(in_state(crate::menu::Screen::Battle)).run_if(crate::menu::overlay_closed),
    );
}

/// Alive, allied and the offers are read each tick: from the ships and the alliance table.
fn update(mut roster: ResMut<Roster>, fleets: Res<Fleets>, ships: Query<(&FleetMember, &Health)>, table: Hostility) {
    let local = table.side_of(&fleets.local);
    let next: Vec<[bool; 5]> = roster
        .rows
        .iter()
        .map(|r| {
            let alive = ships.iter().any(|s| s.0.0 == r.owner && !s.1.destroyed);
            let (me, them) = (local.unwrap_or(Side::Player), table.side_of(&r.owner).unwrap_or(r.side));
            let allied = table.allied(me, them);
            [r.owner == fleets.local, alive, allied, !allied && table.offered(me, them), !allied && table.offered(them, me)]
        })
        .collect();
    if roster.rows.iter().zip(&next).all(|(r, n)| [r.local, r.alive, r.allied, r.request_out, r.request_in] == *n) {
        return;
    }
    for (r, n) in roster.rows.iter_mut().zip(next) {
        [r.local, r.alive, r.allied, r.request_out, r.request_in] = n;
    }
}

/// Pressing row `them`'s alliance icon, as the local player `me` (`press`): edits the table.
pub fn apply_press(table: &mut Alliances, me: Side, them: Side) {
    match press(table.allied(me, them), table.offered(me, them), table.offered(them, me)) {
        Press::Break => table.set(me, them, false),
        Press::Withdraw => table.offer(me, them, false),
        Press::Accept => table.set(me, them, true),
        Press::Offer => table.offer(me, them, true),
    }
}

#[allow(deprecated)] // Interaction, see pointer.rs
fn click(
    mouse: Res<ButtonInput<MouseButton>>,
    roster: Res<Roster>,
    icons: Query<(&AllianceIcon, &Interaction)>,
    names: Query<(&RosterName, &Interaction)>,
    mut orbit: ResMut<Orbit>,
    ships: Query<(Entity, &FleetMember, &Health)>,
    (fleets, mut table): (Res<Fleets>, ResMut<Alliances>),
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let local_alive = roster.rows.iter().any(|r| r.local && r.alive);
    if let Some((AllianceIcon(i), _)) = icons.iter().find(|(_, i)| crate::pointer::over(i)) {
        let Some(row) = roster.rows.get(*i).filter(|r| roster.changeable && !r.local && local_alive) else { return };
        let me = table.side_of(&fleets.local).unwrap_or(Side::Player);
        let them = table.side_of(&row.owner).unwrap_or(row.side);
        apply_press(&mut table, me, them);
        return;
    }
    if let Some((RosterName(i), _)) = names.iter().find(|(_, i)| crate::pointer::over(i)) {
        // The camera goes to the player's first ship, when the local player has none left or is allied with it.
        let Some(row) = roster.rows.get(*i).filter(|r| r.alive && !r.local && (!local_alive || r.allied)) else { return };
        if let Some((e, ..)) = ships.iter().filter(|s| s.1.0 == row.owner && !s.2.destroyed).min_by_key(|s| s.1.1) {
            orbit.focus = Some(e);
        }
    }
}

/// The side of things of the no-owner owner ('NO PLAYER', an unset owner): allied with every player, so never a
/// captain's target, a friend's dock, and an order against one is dropped; the human may still fire at one by hand. PROVISIONAL.
pub const NEUTRAL: Side = Side(u8::MAX - 1);
/// The sides kept for owners that name no player of the battle (`Alliances::side_or_neutral`); the battle's players come first.
const OUTSIDERS: std::ops::Range<u8> = 128..252;

/// The battle's alliance table: the one answer to "is X hostile to Y" for ships, shells, bases, docks, boarding and the rules. Each
/// player has a side (the human's is `Side::Player`); two sides are allied only while each counts the other an ally, and hostile
/// otherwise. A side is always its own ally. Filled at setup (each lobby team, a mission's allied player pairs), then changed by the
/// rules' 'Set alliance' and the player list's alliance button.
#[derive(Resource, Clone, Default, Debug)]
pub struct Alliances {
    /// The side of each player name (several may share one: a skirmish map's base player fights for its race's fleet).
    sides: Vec<(Owner, Side)>,
    /// One-way offers: (a, b) = a counts b as an ally.
    offers: Vec<(Side, Side)>,
}

static EMPTY: Alliances = Alliances { sides: Vec::new(), offers: Vec::new() };

impl Alliances {
    pub fn add_player(&mut self, owner: &str, side: Side) {
        if self.side_of(owner).is_none() {
            self.sides.push((owner.into(), side));
        }
    }
    /// The side of player `owner`; none for a name that is no player.
    pub fn side_of(&self, owner: &str) -> Option<Side> {
        self.sides.iter().find(|p| p.0 == owner).map(|p| p.1)
    }
    /// The side a thing of `owner` fights for: its player's; `NEUTRAL` for no owner ('NO PLAYER', unset); and for an owner that names
    /// no player of this battle (a map's base player whose slot is not in play, say) a side of its own, allied with nobody else, so
    /// hostile to everyone (PROVISIONAL). Such a side is the same for one name every time (`OUTSIDERS`).
    pub fn side_or_neutral(&self, owner: &str) -> Side {
        if let Some(side) = self.side_of(owner) {
            return side;
        }
        if owner.trim().is_empty() || owner.trim().eq_ignore_ascii_case("NO PLAYER") {
            return NEUTRAL;
        }
        let hash = owner.bytes().fold(2166136261u32, |h, b| (h ^ u32::from(b)).wrapping_mul(16777619));
        Side(OUTSIDERS.start + (hash % u32::from(OUTSIDERS.end - OUTSIDERS.start)) as u8)
    }
    /// `a` counts `b` an ally.
    pub fn offered(&self, a: Side, b: Side) -> bool {
        a == b || self.offers.contains(&(a, b))
    }
    pub fn allied(&self, a: Side, b: Side) -> bool {
        a == NEUTRAL || b == NEUTRAL || (self.offered(a, b) && self.offered(b, a))
    }
    pub fn hostile(&self, a: Side, b: Side) -> bool {
        !self.allied(a, b)
    }
    /// A ship of side `shooter` may shoot at `target`: a hostile, or (`by_hand`, the human's own aim) a thing of no player. PROVISIONAL.
    pub fn targetable(&self, shooter: Side, target: Side, by_hand: bool) -> bool {
        self.hostile(shooter, target) || (by_hand && target == NEUTRAL)
    }
    /// On the human's side of the table (the human's own ships included).
    pub fn with_human(&self, s: Side) -> bool {
        self.allied(Side::Player, s)
    }
    /// `a` offers (or withdraws) its alliance to `b`; it holds once `b` offers back.
    pub fn offer(&mut self, a: Side, b: Side, on: bool) {
        self.offers.retain(|o| *o != (a, b));
        if on && a != b {
            self.offers.push((a, b));
        }
    }
    /// Both directions at once.
    pub fn set(&mut self, a: Side, b: Side, on: bool) {
        self.offer(a, b, on);
        self.offer(b, a, on);
    }
    /// By player names (non-players are neutral).
    pub fn hostile_owners(&self, a: &str, b: &str) -> bool {
        self.hostile(self.side_or_neutral(a), self.side_or_neutral(b))
    }
    /// Two players allied with each other (one player with itself); a name that is no player is nobody's ally.
    pub fn players_allied(&self, a: &str, b: &str) -> bool {
        a == b || self.side_of(a).zip(self.side_of(b)).is_some_and(|(x, y)| self.allied(x, y))
    }
    /// The side of a base: a skirmish base's own, else its mission owner's.
    pub fn base_side(&self, base: Option<&crate::campaign::MissionBase>, side: Option<&crate::combat::BaseSide>) -> Option<Side> {
        side.map(|s| s.0).or_else(|| base.map(|b| self.side_or_neutral(&b.owner)))
    }
}

/// The battle's `Alliances` as a system parameter; a world without one (a unit test) has the empty table: each side on its own.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Hostility<'w> {
    table: Option<Res<'w, Alliances>>,
}
impl std::ops::Deref for Hostility<'_> {
    type Target = Alliances;
    fn deref(&self) -> &Alliances {
        self.table.as_deref().unwrap_or(&EMPTY)
    }
}

/// The empty table: each side on its own.
pub fn empty() -> &'static Alliances {
    &EMPTY
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

    /// Three factions, A and B each at war with the player and with each other: the one table says A and B are hostile to each other
    /// too, by name and by side alike, and every ship's side comes from it (each player its own).
    #[test]
    fn three_factions_are_each_hostile_to_the_other_two() {
        let data = serde_json::json!({"rules": [], "players": ["Hero", "A", "B"], "alliances": [], "player_start": {"name": "Hero"}});
        let t = crate::rules::Script::new(&data, serde_json::Value::Null).alliances;
        let side = |o: &str| t.side_of(o).unwrap();
        assert_eq!((side("Hero"), side("A"), side("B")), (Side::Player, Side(1), Side(2)), "each player its own side, the human's first");
        for (x, y) in [("A", "Hero"), ("B", "Hero"), ("A", "B"), ("B", "A")] {
            assert!(t.hostile_owners(x, y) && t.hostile(side(x), side(y)), "{x} vs {y}");
        }
        assert!(!t.hostile_owners("A", "A") && !t.hostile_owners("A", "NO PLAYER"), "a player is its own ally; a non-player is nobody's foe");
    }

    #[test]
    fn an_alliance_needs_both_directions() {
        let (a, b) = (Side(1), Side(2));
        let mut t = Alliances::default();
        assert!(t.hostile(a, b) && t.allied(a, a));
        t.offer(a, b, true);
        assert!(t.hostile(a, b) && t.hostile(b, a), "one offer is no alliance");
        t.offer(b, a, true);
        assert!(t.allied(a, b) && t.allied(b, a));
        assert!(t.allied(NEUTRAL, a), "neutral things are nobody's foe");
    }

    #[test]
    fn the_alliance_button_offers_accepts_and_breaks() {
        let (me, them) = (Side::Player, Side(1));
        let mut t = Alliances::default();
        apply_press(&mut t, me, them);
        assert!(t.offered(me, them) && t.hostile(me, them), "an offer");
        apply_press(&mut t, me, them);
        assert!(!t.offered(me, them), "pressed again: withdrawn");
        t.offer(them, me, true);
        apply_press(&mut t, me, them);
        assert!(t.allied(me, them), "their offer accepted");
        apply_press(&mut t, me, them);
        assert!(t.hostile(me, them) && !t.offered(them, me), "a break clears both directions");
    }

    #[test]
    fn the_lobby_team_starts_allied() {
        let group = |side, team| crate::Group { player: side as usize, side: Side(side), team, ..default() };
        let t = crate::scenario::lobby_alliances(&[group(0, 1), group(1, 1), group(2, 2), group(3, -1), group(4, -1)]);
        assert!(t.allied(Side(0), Side(1)), "same lobby team");
        assert!(t.hostile(Side(0), Side(2)) && t.hostile(Side(1), Side(2)), "another team");
        assert!(t.hostile(Side(3), Side(4)), "slots on no team are each their own");
    }

    /// A thing of no player is allied with every player: never a target or a foe's dock, its mines bounce off every hull (they damage
    /// no one); only the human's own aim may still fire at it.
    #[test]
    fn a_thing_of_no_player_is_everyones_ally_but_can_be_fired_on_by_hand() {
        let mut t = Alliances::default();
        t.add_player("Hero", Side::Player);
        let nobody = t.side_or_neutral("NO PLAYER");
        let camp = crate::campaign::MissionBase { id: 1, owner: "NO PLAYER".into() };
        for s in [Side::Player, Side(1), crate::scenario::HOSTILE_TO_ALL] {
            assert!(t.allied(nobody, s) && !t.targetable(s, nobody, false), "never a captain's target");
        }
        assert_eq!(t.base_side(Some(&camp), None), Some(nobody), "its base is a friendly dock");
        assert!(t.targetable(Side::Player, nobody, true) && !t.targetable(Side::Player, Side::Player, true), "by hand: things of no player only");
    }

    /// Only the no-owner owner is everyone's ally; an owner that names no player of this battle (Border Dispute's base players when
    /// their slots are not in play) fights alone, hostile to every player and to other such owners.
    #[test]
    fn an_owner_outside_the_battle_fights_alone_and_only_no_owner_is_everyones_ally() {
        let mut t = Alliances::default();
        t.add_player("Hero", Side::Player);
        t.add_player("Raiders", Side(1));
        let (outsider, other) = (t.side_or_neutral("player 8"), t.side_or_neutral("player 9"));
        assert_eq!(outsider, t.side_or_neutral("player 8"), "the same side every time");
        for s in [Side::Player, Side(1), other] {
            assert!(t.hostile(outsider, s) && t.targetable(s, outsider, false), "hostile and a target");
        }
        for nobody in ["NO PLAYER", "", "No Player"] {
            assert_eq!(t.side_or_neutral(nobody), NEUTRAL, "{nobody:?}");
        }
        let base = crate::campaign::MissionBase { id: 9, owner: "player 9".into() };
        assert_eq!(t.base_side(Some(&base), None), Some(other));
    }
}
