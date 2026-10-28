//! Battle statistics (retail `GameStats`, docs/re/scoring-saves.md §3): per player the starting and ending fleet size, ships sunk,
//! damage inflicted and received. The rules' end text (missions) and the skirmish Game Summary page (classic view) show them.
//! PROVISIONAL: damage is hit points lost by hulls (sections + core) credited to the last shooter of that hull; the retail column set,
//! bar art and kill-by-object-type table are not reproduced (only ships are counted).
use bevy::prelude::*;
use std::collections::{BTreeMap, HashMap};

use crate::combat::{Health, Hit};
use crate::{Ship, Side};

#[derive(Default, Clone, Debug, PartialEq)]
pub struct Line {
    pub start: u32,
    pub end: u32,
    pub sunk: u32,
    pub inflicted: f32,
    pub taken: f32,
    /// Race of the player's first ship (picks the flag on the summary).
    pub race: String,
}
impl Line {
    /// The summary columns in retail order: Ships Sunk, Ships Left, Damage Inflicted, Damage Taken.
    pub fn cells(&self) -> [String; 4] {
        [self.sunk.to_string(), format!("{}/{}", self.end, self.start), format!("{:.0}", self.inflicted), format!("{:.0}", self.taken)]
    }
}

#[derive(Resource, Default)]
pub struct GameStats {
    pub players: BTreeMap<String, Line>,
    /// Per hull: (owner, hit points last frame, destroyed, last shooter's owner).
    hulls: HashMap<Entity, (String, f32, bool, Option<String>)>,
}
impl GameStats {
    /// The summary table as text.
    pub fn table(&self) -> String {
        let mut s = format!("{:<22}{:>7}{:>7}{:>11}{:>9}\n", "", "Left", "Sunk", "Inflicted", "Taken");
        for (name, l) in &self.players {
            s += &format!("{:<22}{:>4}/{:<2}{:>7}{:>11.0}{:>9.0}\n", name.chars().take(21).collect::<String>(), l.end, l.start, l.sunk, l.inflicted, l.taken);
        }
        s
    }
    /// Per player and column: whether that cell shows the green bar. Observed on two retail summaries: the best value of each column
    /// is green, ties included (most sunk / left / inflicted, least taken); the rest are plain boxes. PROVISIONAL: only 2-player games seen.
    pub fn best(&self) -> Vec<[bool; 4]> {
        let key = |l: &Line| [l.sunk as f32, l.end as f32 / l.start.max(1) as f32, l.inflicted.round(), -l.taken.round()];
        let top = self.players.values().map(key).fold([f32::MIN; 4], |a, k| std::array::from_fn(|i| a[i].max(k[i])));
        self.players.values().map(|l| std::array::from_fn(|i| key(l)[i] >= top[i])).collect()
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<GameStats>()
        .add_systems(OnEnter(crate::menu::Screen::Battle), reset)
        .add_systems(Update, collect.after(crate::sail).run_if(in_state(crate::menu::Screen::Battle)));
}

fn reset(mut commands: Commands) {
    commands.insert_resource(GameStats::default());
}

fn owner(ship: &Ship, m: Option<&crate::campaign::MissionShip>) -> String {
    m.map_or_else(|| if ship.side == Side::Player { "Your fleet".into() } else { "Enemy fleet".into() }, |m| m.owner.clone())
}

pub(crate) fn collect(
    mut stats: ResMut<GameStats>,
    lib: Res<crate::combat::Library>,
    mut hits: MessageReader<Hit>,
    ships: Query<(Entity, &Ship, &Health, Option<&crate::campaign::MissionShip>, Has<crate::blackhole::Swallowed>), Without<crate::lifeboats::Lifeboat>>,
) {
    let owners: HashMap<Entity, String> = ships.iter().map(|(e, s, _, m, _)| (e, owner(s, m))).collect();
    for h in hits.read() {
        if let (Some(by), Some(entry)) = (owners.get(&h.shooter).cloned(), stats.hulls.get_mut(&h.victim)) {
            entry.3 = Some(by);
        }
    }
    let stats = &mut *stats;
    for (e, s, h, _, swallowed) in &ships {
        let class = &lib.classes[s.class];
        if class.phys.max_speed <= 0.0 {
            continue; // bases and scenery hulls are not fleet ships
        }
        let hp = h.sections.iter().sum::<f32>() + h.core.max(0.0);
        let me = owners[&e].clone();
        let Some(entry) = stats.hulls.get_mut(&e) else {
            stats.hulls.insert(e, (me.clone(), hp, h.destroyed, None));
            let l = stats.players.entry(me).or_default();
            if l.race.is_empty() {
                l.race = class.race.clone();
            }
            l.start += 1;
            l.end += !h.destroyed as u32;
            continue;
        };
        if entry.0 != me {
            // Captured or transferred: it now counts for its new owner.
            let old = stats.players.entry(entry.0.clone()).or_default();
            old.end = old.end.saturating_sub(!entry.2 as u32);
            stats.players.entry(me.clone()).or_default().end += !h.destroyed as u32;
            entry.0 = me.clone();
        }
        // A black hole's kill zeroes the core but counts as nobody's damage or kill (retail summary).
        let lost = if swallowed { 0.0 } else { (entry.1 - hp).max(0.0) };
        if lost > 0.0 {
            stats.players.entry(me.clone()).or_default().taken += lost;
            if let Some(by) = entry.3.clone() {
                stats.players.entry(by).or_default().inflicted += lost;
            }
        }
        if h.destroyed && !entry.2 {
            let l = stats.players.entry(me.clone()).or_default();
            l.end = l.end.saturating_sub(1);
            if let Some(by) = entry.3.clone().filter(|b| *b != me && !swallowed) {
                stats.players.entry(by).or_default().sunk += 1;
            }
        }
        entry.1 = hp;
        entry.2 = h.destroyed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn table_lists_every_player() {
        let mut s = GameStats::default();
        s.players.insert("Navy".into(), Line { start: 3, end: 2, sunk: 4, inflicted: 512.0, taken: 300.0, ..default() });
        let t = s.table();
        assert!(t.contains("Navy") && t.contains("2/3") && t.contains("512"));
    }
    #[test]
    fn best_cells_match_the_retail_summaries() {
        // The two retail defeats: Computer 0 sunk 2/2 1201 65 vs player 0 sunk 0/1 0 1469; Computer 2 2/3 2340 1253 vs 1 0/2 1253 6566.
        let mut s = GameStats::default();
        s.players.insert("A".into(), Line { start: 2, end: 2, sunk: 0, inflicted: 1201.0, taken: 65.0, ..default() });
        s.players.insert("B".into(), Line { start: 1, end: 0, sunk: 0, inflicted: 0.0, taken: 1469.0, ..default() });
        assert_eq!(s.best(), vec![[true; 4], [true, false, false, false]]);
        s.players.insert("A".into(), Line { start: 3, end: 2, sunk: 2, inflicted: 2340.0, taken: 1253.0, ..default() });
        s.players.insert("B".into(), Line { start: 2, end: 0, sunk: 1, inflicted: 1253.0, taken: 6566.0, ..default() });
        assert_eq!(s.best(), vec![[true; 4], [false; 4]]);
        assert_eq!(s.players["A"].cells(), ["2", "2/3", "2340", "1253"].map(String::from));
    }
}
