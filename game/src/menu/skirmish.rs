//! Skirmish lobby model: map list, player slots, races, fleets within the victory points and ship stats for the views.
use bevy::prelude::*;
use serde_json::Value;

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Human,
    Computer,
    /// Nobody there (the original's 'Open', free for another player to join) ...
    Open,
    /// ... and 'Close'd by the host. Both are empty in a single-player skirmish.
    Closed,
}

impl Kind {
    /// Fields ships.
    pub fn live(self) -> bool {
        matches!(self, Kind::Human | Kind::Computer)
    }
}

/// A pull-down cell of the lobby's player table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cell {
    Player(usize),
    Fleet(usize),
    Team(usize),
}

/// An entry of a lobby pull-down. Retail lists the current value first, then the others in a fixed order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Computer,
    Open,
    Close,
    Fleet(u8),
    Team(i32),
}

/// The original's lobby has eight teams to pick from on free-for-all maps.
pub const TEAMS: i32 = 8;
/// Fleet slots the Ships / Crew / Arms screens show at once.
pub const SLOT_BAR: usize = 5;

/// One lobby row = one playable player of the map (docs/re/skirmish-setup.md).
#[derive(Clone, PartialEq, Debug)]
pub struct Slot {
    pub player: usize, // index in the map's player list
    pub name: String,
    pub kind: Kind,
    pub race: u8,  // fleet type as in the TWT: 0 Navy, 2 Pirate, 3 Procyon, 4 none chosen yet
    pub team: i32, // -1: on its own
    /// Which of the fleet type's ten flags the player flies (0-9); only the human slot's can be changed.
    pub flag: u8,
    pub fleet: Vec<String>,
    pub weapons: Vec<std::collections::BTreeMap<usize, String>>,
    /// The name picked from the ship-name drop-down per fleet entry (`None`: the default, see `ship_names`).
    pub names: Vec<Option<String>>,
    /// Named crew per fleet entry and post (`crew_arms::ROLES` order); posts left empty are filled by generic crew.
    pub crew: Vec<[Vec<String>; 8]>,
}

impl Slot {
    pub fn clear_fleet(&mut self) {
        self.fleet.clear();
        self.weapons.clear();
        self.names.clear();
        self.crew.clear();
    }
    pub fn truncate_fleet(&mut self, n: usize) {
        self.fleet.truncate(n);
        self.weapons.truncate(n);
        self.names.truncate(n);
        self.crew.truncate(n);
    }
    pub fn push_ship(&mut self, class: &str) {
        self.weapons.resize_with(self.fleet.len(), Default::default);
        self.names.resize_with(self.fleet.len(), Default::default);
        self.crew.resize_with(self.fleet.len(), Default::default);
        self.fleet.push(class.into());
        self.weapons.push(Default::default());
        self.names.push(None);
        self.crew.push(Default::default());
    }
    pub fn remove_ship(&mut self, i: usize) {
        self.fleet.remove(i);
        if i < self.weapons.len() {
            self.weapons.remove(i);
        }
        if i < self.names.len() {
            self.names.remove(i);
        }
        if i < self.crew.len() {
            self.crew.remove(i);
        }
    }
    /// The name of every fleet ship: the picked one, else the first name of the hull's pool that no other ship of the fleet bears.
    /// PROVISIONAL: the original's default looked random (two adds of the same hull gave different names).
    pub fn ship_names(&self, lib: &Library) -> Vec<Option<String>> {
        let mut taken: std::collections::HashSet<&str> = self.names.iter().flatten().map(String::as_str).collect();
        let mut out: Vec<Option<String>> = (0..self.fleet.len()).map(|i| self.names.get(i).cloned().flatten()).collect();
        for (i, n) in out.iter_mut().enumerate().filter(|(_, n)| n.is_none()) {
            *n = lib.names.get(&self.fleet[i]).and_then(|pool| pool.iter().find(|p| !taken.contains(p.as_str())).cloned());
            if let Some(n) = n {
                taken.insert(n);
            }
        }
        out
    }
    /// The pull-down's entries for fleet ship `i`: its hull's pool in order, without the name it bears.
    pub fn name_choices(&self, lib: &Library, i: usize) -> Vec<String> {
        let own = self.ship_names(lib).get(i).cloned().flatten();
        self.fleet.get(i).and_then(|c| lib.names.get(c)).map_or_else(Vec::new, |pool| pool.iter().filter(|n| Some(*n) != own.as_ref()).cloned().collect())
    }
    pub fn set_name(&mut self, i: usize, name: &str) {
        self.names.resize_with(self.fleet.len(), Default::default);
        if let Some(n) = self.names.get_mut(i) {
            *n = Some(name.into());
        }
    }
    /// The fitting of fleet entry `i`, created on demand.
    pub fn crew_mut(&mut self, i: usize) -> &mut [Vec<String>; 8] {
        if self.crew.len() <= i {
            self.crew.resize_with(i + 1, Default::default);
        }
        &mut self.crew[i]
    }
    /// Victory points the fleet costs: hulls, weapon swaps and named crew.
    pub fn spent(&self, lib: &Library) -> i64 {
        self.fleet
            .iter()
            .enumerate()
            .filter_map(|(i, n)| {
                let class = lib.classes.iter().find(|c| &c.name == n)?;
                Some(i64::from(class.points) + self.weapons.get(i).map_or(0, |fit| crate::crew_arms::fitting_delta(lib, class, fit)))
            })
            .sum::<i64>()
            + self.crew.iter().flatten().flatten().map(|id| i64::from(crate::crew_arms::crew_points(id))).sum::<i64>()
    }
    /// Where a named crew member is posted in this fleet: (fleet entry, post).
    pub fn crew_at(&self, id: &str) -> Option<(usize, usize)> {
        self.crew.iter().enumerate().find_map(|(i, c)| c.iter().position(|r| r.iter().any(|x| x == id)).map(|r| (i, r)))
    }
}

/// What the front end has chosen; read by the battle setup.
#[derive(Resource)]
pub struct Skirmish {
    pub map: Option<String>,
    pub slots: Vec<Slot>,
    pub maps: Vec<(String, String, bool, usize)>, // id, name, open (MustAssembleFleet), playable players
    pub selected: usize,
    pub historical: bool,
    pub fleet_size: usize,
    /// Victory points a fleet may cost (the setup's Victory Limit; ships spend their 'Victory Point Cost').
    pub victory_limit: u32,
    pub map_json: Value,
    pub error: Option<&'static str>, // string id (or plain text) shown under the player list
    /// The open lobby pull-down, if any (its list is drawn under the cell; a click elsewhere closes it).
    pub dropdown: Option<Cell>,
    /// The fleet screen's ship-name pull-down is open.
    pub name_open: bool,
    /// The lobby's flag picker is open (retail: a modal gold frame with the fleet type's ten flags).
    pub flag_picker: bool,
    pub ship_pick: usize,
    /// A hull type has been clicked in Available Ships (retail starts with none: no preview, Add greyed).
    pub ship_chosen: bool,
    /// The info panel and preview show the fleet slot `fleet_pick` rather than the clicked hull type.
    pub view_slot: bool,
    pub fleet_pick: usize,
    /// First fleet slot shown in the bar of five.
    pub slot_scroll: usize,
    /// First row of the Create Skirmish map list.
    pub map_scroll: usize,
    pub arms_open: bool,
    /// The crew & arms screen opens on the Arms tab (else Crew).
    pub arms_tab: bool,
    /// The lobby's Map Description dialog ('Map Info') is open.
    pub map_info: bool,
    /// First row of the classic fixed-height ship list.
    pub scroll: usize,
}

/// A Create Skirmish slider (measured on the original: victory points 50-1000 in steps of 10, default 300; fleet size 1-10, default 5).
pub struct SliderRange {
    pub min: u32,
    pub max: u32,
    pub step: u32,
    pub default: u32,
}

impl SliderRange {
    pub fn step_value(&self, value: u32, steps: i32) -> u32 {
        (i64::from(value) + i64::from(steps) * i64::from(self.step)).clamp(i64::from(self.min), i64::from(self.max)) as u32
    }
    /// The value a knob at `fraction` (0 to 1) stands for, rounded to a step.
    pub fn at(&self, fraction: f32) -> u32 {
        let steps = ((self.max - self.min) / self.step) as f32;
        self.min + (fraction.clamp(0.0, 1.0) * steps).round() as u32 * self.step
    }
    /// Knob position, 0 (min) to 1 (max).
    pub fn fraction(&self, value: u32) -> f32 {
        (value.saturating_sub(self.min)) as f32 / (self.max - self.min) as f32
    }
}

pub const VICTORY_LIMIT: SliderRange = SliderRange { min: 50, max: 1000, step: 10, default: 300 };
pub const FLEET_SIZE: SliderRange = SliderRange { min: 1, max: 10, step: 1, default: 5 };

pub const RACES: [(u8, &str, &str); 3] =
    [(0, "Navy", "IDGS_TPFRONTENDTEXT03_FLEET_NAVY"), (2, "Pirate", "IDGS_TPFRONTENDTEXT03_FLEET_PIRATE"), (3, "Procyon", "IDGS_TPFRONTENDTEXT03_FLEET_PROCYON")];
/// Hulls the retail fleet screen is not expected to offer: story/NIS/tutorial variants, lifeboats, wrecks, targets.
/// PROVISIONAL: the original lists every ship type of the chosen race; this name filter stands in for whatever hides these.
/// The original's Navy list includes the StarHammer and the Heavy Scout (Legacy hull), so those are not filtered.
pub(super) const NOT_OFFERED: [&str; 10] = ["NIS", "Tutorial", "Feather", "Jim", "Amelia", "Wrecked", "Lifeboat", "Target", "FastTurn", "WeaponBarge"];

/// Ship classes the fleet screen offers for a race (mods can opt hulls in with `"skirmish": true`).
pub fn offered(lib: &Library, race: u8) -> Vec<usize> {
    let Some((_, name, _)) = RACES.iter().find(|r| r.0 == race) else { return vec![] };
    let mut list: Vec<usize> =
        (0..lib.classes.len()).filter(|&i| lib.classes[i].race == *name && (lib.classes[i].skirmish || !NOT_OFFERED.iter().any(|n| lib.classes[i].name.contains(n)))).collect();
    // As the original's 'Available Ships': most expensive first (StarHammer, Man-O-War, Frigate, ...).
    list.sort_by(|&a, &b| lib.classes[b].points.cmp(&lib.classes[a].points).then_with(|| lib.classes[a].name.cmp(&lib.classes[b].name)));
    list
}

/// Auto Assign. Measured on the original (docs/re/skirmish-setup.md): it fills the fleet with hulls that fit the points still left,
/// mixing classes (300 points / 5 ships gave a Heavy Scout, a Heavy Escort and three Warsloops; 50 points two Torpedo Boats). Hulls it
/// adds itself also get weapon swaps (a hand-added ship only gets crew), and the points that remain buy named crew. The choice among
/// affordable hulls and weapons looked random: PROVISIONAL, here a fixed pseudo-random pick (a coin flip per bank) so the result
/// repeats; crew go to the posts in order, best skill first. Computers use it too (PROVISIONAL: the original's computer fleet was
/// not read out of a battle; the budget is the same victory limit, and `armed_only` keeps tenders and tugs out of its fleets).
pub fn auto_build(slot: &mut Slot, limit: u32, fleet_size: usize, lib: &Library, salt: u64, armed_only: bool) {
    let next = |seed: &mut u64| {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *seed >> 33
    };
    let mut seed = u64::from(limit) * 31 + fleet_size as u64 * 7 + 1 + salt * 977;
    let left = |slot: &Slot| i64::from(limit) - slot.spent(lib);
    let first_new = slot.fleet.len();
    while slot.fleet.len() < fleet_size {
        let budget = left(slot);
        let fits: Vec<usize> =
            offered(lib, slot.race).into_iter().filter(|&c| i64::from(lib.classes[c].points) <= budget && (!armed_only || !lib.classes[c].mounts.is_empty())).collect();
        if fits.is_empty() {
            break;
        }
        let pick = fits[next(&mut seed) as usize % fits.len()];
        slot.push_ship(&lib.classes[pick].name);
    }
    for ship in first_new..slot.fleet.len() {
        let class = &lib.classes[lib.index(&slot.fleet[ship])];
        for (bank, first) in crate::crew_arms::selectable_banks(lib, class) {
            if next(&mut seed) & 1 == 0 {
                continue;
            }
            let stock = &class.mounts[first].gun;
            let budget = left(slot);
            let stock_cost = i64::from(crate::crew_arms::bank_cost(lib, class, bank, stock).unwrap_or(0));
            let mut guns: Vec<&String> = lib
                .guns
                .keys()
                .filter(|g| *g != stock && crate::crew_arms::bank_compatible(lib, class, bank, g))
                .filter(|g| crate::crew_arms::bank_cost(lib, class, bank, g).is_some_and(|c| i64::from(c) - stock_cost <= budget))
                .collect();
            guns.sort();
            if guns.is_empty() {
                continue;
            }
            let gun = guns[next(&mut seed) as usize % guns.len()].clone();
            slot.weapons.resize_with(slot.fleet.len(), Default::default);
            slot.weapons[ship].insert(bank, gun);
        }
    }
    let name = RACES.iter().find(|r| r.0 == slot.race).map_or("Navy", |r| r.1);
    for ship in 0..slot.fleet.len() {
        let class = &lib.classes[lib.index(&slot.fleet[ship])];
        let caps = crate::boarding::counts(&class.name);
        for (role, &cap) in caps.iter().enumerate() {
            while slot.crew_mut(ship)[role].len() < cap as usize {
                let budget = left(slot);
                let used: Vec<String> = slot.crew.iter().flatten().flatten().cloned().collect();
                let skill = crate::crew_arms::post_skill_name(role);
                let best = crate::crew_arms::roster(name, 0)
                    .into_iter()
                    .filter(|id| !used.contains(id) && i64::from(crate::crew_arms::crew_points(id)) <= budget)
                    .max_by(|a, b| crate::crew_arms::ability(a, skill).unwrap_or(0.0).total_cmp(&crate::crew_arms::ability(b, skill).unwrap_or(0.0)).then_with(|| b.cmp(a)));
                let Some(id) = best else { break };
                slot.crew_mut(ship)[role].push(id);
            }
        }
    }
}

pub fn pretty(name: &str) -> String {
    name.trim_start_matches("Ship_").replace('_', " ")
}

pub(super) fn ships_of(map: &Value, player: usize) -> Vec<String> {
    map["ships"].as_array().into_iter().flatten().filter(|s| s["player"].as_u64() == Some(player as u64)).filter_map(|s| s["type"].as_str().map(String::from)).collect()
}

/// Lobby rows of a map. Historical maps (MustAssembleFleet false) keep their pre-placed fleets and fixed fleet types: the only
/// choice is which side you take. Open maps start with you, one opponent and the other slots closed; fleet types come from the
/// map's team list (locked) or are chosen by the player (maps without teams start with no fleet type).
pub(super) fn new_slots(map: &Value) -> Vec<Slot> {
    let open = map["assemble"].as_bool().unwrap_or(false);
    let teams = map["teams"].as_array().cloned().unwrap_or_default();
    let mut slots: Vec<Slot> = map["players"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, p)| p["playable"].as_bool().unwrap_or(false))
        .map(|(i, p)| {
            let team = p["team"].as_i64().unwrap_or(-1) as i32;
            let team_race = usize::try_from(team).ok().and_then(|t| teams.get(t)).map_or(4, |t| t["race"].as_u64().unwrap_or(4) as u8);
            // Historical maps name each player's race, or leave it open (4) and the team's race stands.
            let race = if open { team_race } else { Some(p["race"].as_u64().unwrap_or(4) as u8).filter(|r| *r != 4).unwrap_or(team_race) };
            let fleet = if open { vec![] } else { ships_of(map, i) };
            Slot { player: i, name: p["name"].as_str().unwrap_or("").trim().into(), kind: Kind::Computer, race, team, flag: 0, fleet, weapons: vec![], names: vec![], crew: vec![] }
        })
        .collect();
    if let Some(h) = slots.first_mut() {
        h.kind = Kind::Human;
    }
    if open {
        // Retail: you alone, every other slot Open until a pull-down turns it into a Computer. Free-for-all maps start you as Navy, Team 1.
        for s in slots.iter_mut().skip(1) {
            s.kind = Kind::Open;
        }
        if let Some(h) = slots.first_mut().filter(|_| teams.is_empty()) {
            (h.race, h.team) = (0, 0);
        }
        for s in slots.iter_mut().skip(1).filter(|_| teams.is_empty()) {
            (s.race, s.team) = (4, -1);
        }
    }
    slots
}

impl Skirmish {
    pub(super) fn load_map(&mut self) {
        self.map_json = self.maps.get(self.selected).and_then(|m| crate::data::map_json(&m.0)).unwrap_or(Value::Null);
        self.slots = new_slots(&self.map_json);
        self.error = None;
        (self.ship_pick, self.fleet_pick, self.scroll) = (0, 0, 0);
        (self.ship_chosen, self.view_slot) = (false, false);
        self.dropdown = None;
        self.flag_picker = false;
        self.slot_scroll = 0;
        self.arms_open = false;
    }
    /// The hull the Ship Information panel and the preview show: the picked fleet slot after clicking it (or adding a ship), else the
    /// clicked type, else nothing. Retail: the fleet screen opens blank.
    pub fn shown_class(&self, lib: &Library) -> Option<usize> {
        let h = self.human()?;
        if self.view_slot
            && let Some(name) = h.fleet.get(self.fleet_pick)
        {
            return lib.classes.iter().position(|c| &c.name == name);
        }
        self.ship_chosen.then(|| offered(lib, h.race).get(self.ship_pick).copied()).flatten()
    }
    /// Crew, Arms and Drop are greyed until the picked fleet slot holds a ship (retail, empty fleet).
    pub fn slot_filled(&self) -> bool {
        self.human().is_some_and(|h| self.fleet_pick < h.fleet.len())
    }
    /// Names the shown fleet ship (the human's) and closes the ship-name pull-down.
    pub fn pick_name(&mut self, name: &str) {
        let pick = self.fleet_pick;
        if let Some(h) = self.slots.iter_mut().find(|s| s.kind == Kind::Human) {
            h.set_name(pick, name);
        }
        self.name_open = false;
    }
    /// Whether fleet types and teams are chosen by the players (free-for-all open maps); team maps lock both to the map's team list.
    pub fn free_teams(&self) -> bool {
        self.open() && self.map_json["teams"].as_array().is_none_or(|t| t.is_empty())
    }
    /// The entries of a pull-down, the current value first (as the original lists them). Empty = the cell is not editable.
    pub fn choices(&self, cell: Cell) -> Vec<Choice> {
        let open = self.open();
        match cell {
            Cell::Player(i) => match self.slots.get(i).map(|s| s.kind) {
                Some(Kind::Computer) if open => vec![Choice::Computer, Choice::Open, Choice::Close],
                Some(Kind::Open) => vec![Choice::Open, Choice::Close, Choice::Computer],
                Some(Kind::Closed) => vec![Choice::Close, Choice::Open, Choice::Computer],
                _ => vec![],
            },
            Cell::Fleet(i) if self.free_teams() && self.slots.get(i).is_some_and(|s| s.kind.live()) => {
                let now = self.slots[i].race;
                let mut v: Vec<Choice> = [0u8, 2, 3].into_iter().filter(|&r| r == now).chain([0u8, 2, 3].into_iter().filter(|&r| r != now)).map(Choice::Fleet).collect();
                v.dedup();
                v
            }
            Cell::Team(i) if self.free_teams() && self.slots.get(i).is_some_and(|s| s.kind.live()) => {
                let now = self.slots[i].team;
                (0..TEAMS).filter(|&t| t == now).chain((0..TEAMS).filter(|&t| t != now)).map(Choice::Team).collect()
            }
            _ => vec![],
        }
    }
    /// The lowest of the ten flags of slot `i`'s fleet type that no other live slot of that type flies (retail: fleets cannot share a flag).
    pub fn free_flag(&self, i: usize) -> u8 {
        let Some(me) = self.slots.get(i) else { return 0 };
        (0..10).find(|&f| !self.flag_taken(i, me.race, f)).unwrap_or(0)
    }
    /// Another live slot of `race` flies flag `f`.
    pub fn flag_taken(&self, i: usize, race: u8, f: u8) -> bool {
        self.slots.iter().enumerate().any(|(j, s)| j != i && s.kind.live() && s.race == race && s.flag == f)
    }
    pub fn choose(&mut self, cell: Cell, choice: Choice) {
        self.choose_inner(cell, choice);
        // A slot that came alive or changed fleet type keeps its flag only while no other fleet of that type flies it.
        if let Cell::Player(i) | Cell::Fleet(i) = cell
            && let Some(s) = self.slots.get(i).filter(|s| s.kind.live())
            && self.flag_taken(i, s.race, s.flag)
        {
            self.slots[i].flag = self.free_flag(i);
        }
    }
    fn choose_inner(&mut self, cell: Cell, choice: Choice) {
        self.dropdown = None;
        self.error = None;
        let free = self.free_teams();
        match (cell, choice) {
            (Cell::Player(i), Choice::Computer | Choice::Open | Choice::Close) => {
                let used: Vec<i32> = self.slots.iter().filter(|s| s.kind.live()).map(|s| s.team).collect();
                let Some(s) = self.slots.get_mut(i).filter(|s| s.kind != Kind::Human) else { return };
                match choice {
                    Choice::Computer => {
                        s.kind = Kind::Computer;
                        if free && s.team < 0 {
                            // PROVISIONAL: the original gave the first computer on a free-for-all map Pirate, Team 2 beside a Navy, Team 1 player.
                            s.team = (0..TEAMS).find(|t| !used.contains(t)).unwrap_or(0);
                            // Navy, Pirate, Procyon by slot, round again (the player is slot 0, Navy).
                            s.race = [0u8, 2, 3][i % 3];
                        }
                    }
                    _ => {
                        s.kind = if choice == Choice::Open { Kind::Open } else { Kind::Closed };
                        s.clear_fleet();
                        if free {
                            (s.race, s.team) = (4, -1);
                        }
                    }
                }
            }
            (Cell::Fleet(i), Choice::Fleet(race)) if free => {
                if let Some(s) = self.slots.get_mut(i).filter(|s| s.race != race) {
                    s.race = race;
                    s.clear_fleet(); // a fleet only holds hulls of its fleet type
                }
            }
            (Cell::Team(i), Choice::Team(team)) if free => {
                if let Some(s) = self.slots.get_mut(i).filter(|s| s.team != team) {
                    s.team = team;
                    // The original: "The host has changed your team, resetting your fleet."
                    s.clear_fleet();
                }
            }
            _ => {}
        }
    }
    /// Scroll the slot bar by `d` slots, within the fleet size limit (retail shows five at a time).
    pub fn scroll_slots(&mut self, d: i32) {
        let max = self.fleet_size.saturating_sub(SLOT_BAR);
        self.slot_scroll = (self.slot_scroll as i64 + i64::from(d)).clamp(0, max as i64) as usize;
    }
    /// Keep the picked fleet slot inside the slot bar.
    pub fn show_picked_slot(&mut self) {
        if self.fleet_pick < self.slot_scroll {
            self.slot_scroll = self.fleet_pick;
        } else if self.fleet_pick >= self.slot_scroll + SLOT_BAR {
            self.slot_scroll = self.fleet_pick + 1 - SLOT_BAR;
        }
    }
    pub fn open(&self) -> bool {
        self.map_json["assemble"].as_bool().unwrap_or(false)
    }
    pub fn human(&self) -> Option<&Slot> {
        self.slots.iter().find(|s| s.kind == Kind::Human)
    }
    /// Why the game cannot launch yet (a string id, or plain text), if so.
    /// Victory points the human player has left to spend on ships ('Current Victory Points').
    pub fn points_left(&self, lib: &Library) -> i64 {
        i64::from(self.victory_limit) - self.human().map_or(0, |h| h.spent(lib))
    }
    pub fn launch_error(&self) -> Option<&'static str> {
        let h = self.human()?;
        if !self.slots.iter().any(|s| s.kind.live() && self.hostile(h, s)) {
            return Some("No opponent: open another slot on a different team.");
        }
        if self.open() {
            if h.race == 4 {
                return Some("IDGS_TPFRONTENDTEXT01_PLEASE_SELECT_A_RACE");
            }
            if h.fleet.is_empty() {
                return Some("IDGS_TPFRONTENDTEXT_SCREENS_ERROR_FLEET");
            }
        }
        None
    }
    pub(super) fn hostile(&self, a: &Slot, b: &Slot) -> bool {
        b.kind.live() && !(a.team >= 0 && a.team == b.team) && a.player != b.player
    }
    /// The battle: one group per open slot. Allies (same team as you) fight on our side; every other team (or teamless slot) is
    /// its own side, hostile to us and to the others (The Maw: Navy, Procyon and Pirates all fight each other).
    /// Historical maps use their pre-placed ships and positions; open maps use the assembled (or auto-built computer) fleets.
    pub fn plan(&self, lib: &Library) -> Vec<crate::Group> {
        let Some(h) = self.human() else { return vec![] };
        let open = self.open();
        // Hostile sides in slot order: Enemy for the first hostile team, then Side(2), Side(3), ...
        let mut foes: Vec<(i32, usize)> = vec![];
        self.slots
            .iter()
            .filter(|s| s.kind.live())
            .map(|s| {
                let mut computer_build = None;
                let (ships, ids): (Vec<_>, Vec<Option<i64>>) = if !open {
                    // The first ship becomes the command ship: the one the map flags as primary (its Setup Ship rule), else map order.
                    let mut placed: Vec<&Value> = self.map_json["ships"].as_array().into_iter().flatten().filter(|o| o["player"].as_u64() == Some(s.player as u64)).collect();
                    placed.sort_by_key(|o| o["primary"].as_bool() != Some(true));
                    placed
                        .into_iter()
                        .filter_map(|o| {
                            Some((
                                (o["type"].as_str()?.to_string(), Some([0, 1, 2].map(|i| o["pos"][i].as_f64().unwrap_or(0.0) as f32)), o["name"].as_str().map(String::from)),
                                o["id"].as_i64(),
                            ))
                        })
                        .filter(|(t, _)| lib.classes.iter().any(|c| c.name == t.0))
                        .unzip()
                } else if s.fleet.is_empty() {
                    // A computer builds its fleet with the same routine as Auto Assign, inside the same budget.
                    let mut c = s.clone();
                    auto_build(&mut c, self.victory_limit, self.fleet_size, lib, s.player as u64 + 1, true);
                    computer_build = Some((c.weapons.clone(), c.crew.clone()));
                    (c.fleet.iter().map(|n| (n.clone(), None, None)).collect(), vec![])
                } else {
                    let names = s.ship_names(lib);
                    (s.fleet.iter().zip(names).map(|(n, label)| (n.clone(), None, label)).collect(), vec![])
                };
                let side = if s.kind == Kind::Human || !self.hostile(h, s) {
                    crate::Side::Player
                } else {
                    let key = if s.team >= 0 { (s.team, 0) } else { (-1, s.player) };
                    let i = foes.iter().position(|k| *k == key).unwrap_or_else(|| {
                        foes.push(key);
                        foes.len() - 1
                    });
                    crate::Side(1 + i as u8)
                };
                crate::Group {
                    player: s.player,
                    side,
                    human: s.kind == Kind::Human,
                    race: if s.race == 4 { crate::scenario::faction(&s.name).unwrap_or(4) } else { s.race },
                    team: s.team,
                    ids,
                    ships,
                    // A built computer fleet keeps its weapons but not its named crew: computer ships run at the measured computer crew factor.
                    weapons: match &computer_build {
                        Some((weapons, _)) => weapons.clone(),
                        None if open => s.weapons.clone(),
                        None => vec![],
                    },
                    crew: if open && computer_build.is_none() { s.crew.clone() } else { vec![] },
                }
            })
            .collect()
    }
}

/// The skirmish list: every `maps/<id>/map.json` flagged `"skirmish": true` (campaign maps are not offered), sorted by id, with
/// whether it is open (fleets assembled) and its playable player count (the original's 'Players' column; ports and towns excluded).
pub fn skirmish_maps() -> Vec<(String, String, bool, usize)> {
    let mut maps: Vec<(String, String, bool, usize)> = std::fs::read_dir(assets_dir().join("maps"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let m = crate::data::try_json(&e.path().join("map.json"))?;
            let players = m["players"].as_array().map_or(0, |p| p.iter().filter(|p| p["playable"].as_bool().unwrap_or(false)).count());
            m["skirmish"].as_bool()?.then(|| (e.file_name().to_string_lossy().into(), m["name"].as_str().unwrap_or("").into(), m["assemble"].as_bool().unwrap_or(false), players))
        })
        .collect();
    maps.sort();
    maps
}

/// The fleet screen's ship information, as the original lists it.
#[derive(Debug, PartialEq)]
pub struct ShipStats {
    pub hit_points: u32,
    pub max_speed_kmh: u32,
    /// String id of the turning grade ('IDGS_TPFRONTENDTEXT_TURNINGRATE_*').
    pub turning: &'static str,
    pub crew: u32,
    pub heavy: usize,
    pub medium: usize,
    pub light: usize,
    pub points: u32,
}

/// Stats for one class: hit points = core plus every damage section, speed = WOT MaxSpeed (m/s) in km/h, crew = all crew posts,
/// guns counted by the mount type's size word (dual and non-selectable mounts included), point cost = 'Victory Point Cost'.
/// Matches the original's Navy Frigate panel (3150, 21 km/h, Average, 23, 6/6/9, 131). PROVISIONAL: the turning grade bands on
/// MaxAngularAcceleration are anchored on that one panel (frigate 0.609 = Average).
pub fn ship_stats(lib: &Library, ci: usize) -> ShipStats {
    let c = &lib.classes[ci];
    let size = |word: &str| c.mounts.iter().filter(|m| lib.guns.get(&m.gun).and_then(|g| g.mount_type.as_deref()).is_some_and(|t| t.contains(word))).count();
    let a = c.phys.max_ang_acc;
    let turning = match a {
        _ if a < 0.35 => "IDGS_TPFRONTENDTEXT_TURNINGRATE_VERYPOOR",
        _ if a < 0.55 => "IDGS_TPFRONTENDTEXT_TURNINGRATE_POOR",
        _ if a < 1.5 => "IDGS_TPFRONTENDTEXT_TURNINGRATE_AVERAGE",
        _ if a < 4.0 => "IDGS_TPFRONTENDTEXT_TURNINGRATE_GOOD",
        _ => "IDGS_TPFRONTENDTEXT_TURNINGRATE_VERYGOOD",
    };
    ShipStats {
        hit_points: (c.core_hp + c.sections.iter().map(|s| s.hp).sum::<f32>()).round() as u32,
        max_speed_kmh: (c.phys.max_speed * 3.6).round() as u32,
        turning,
        crew: c.crew_posts,
        heavy: size("Heavy"),
        medium: size("Medium"),
        light: size("Light"),
        points: c.points,
    }
}
