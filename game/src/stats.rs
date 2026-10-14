//! Battle statistics (retail `GameStats`, docs/re/scoring-saves.md §3): per player the starting and ending fleet size, ships sunk,
//! damage inflicted and received. The rules' end text (missions) and the skirmish summary show them as the "Game Summary".
//! PROVISIONAL: damage is hit points lost by hulls (sections + core) credited to the last shooter of that hull; the retail column set,
//! bar art and kill-by-object-type table are not reproduced (only ships are counted).
use bevy::prelude::*;
use std::collections::{BTreeMap, HashMap};

use crate::combat::{Health, Hit};
use crate::scenario::Outcome;
use crate::{Scenario, Ship, Side};

#[derive(Default, Clone, Debug, PartialEq)]
pub struct Line {
    pub start: u32,
    pub end: u32,
    pub sunk: u32,
    pub inflicted: f32,
    pub taken: f32,
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
}

/// Text node for the skirmish end summary (missions append the table to their own end text).
#[derive(Component)]
pub struct Summary;

pub fn plugin(app: &mut App) {
    app.init_resource::<GameStats>()
        .add_systems(OnEnter(crate::menu::Screen::Battle), reset)
        .add_systems(Update, (collect.after(crate::sail), show).run_if(in_state(crate::menu::Screen::Battle)));
}

fn reset(mut commands: Commands) {
    commands.insert_resource(GameStats::default());
    commands.spawn((
        Scenario,
        Summary,
        Text::new(""),
        TextFont { font_size: FontSize::Px(18.0), ..default() },
        TextColor(Color::srgb(1.0, 0.95, 0.8)),
        BackgroundColor(Color::srgba(0.0, 0.02, 0.08, 0.8)),
        Visibility::Hidden,
        GlobalZIndex(175),
        Node { position_type: PositionType::Absolute, top: Val::Percent(25.0), left: Val::Percent(25.0), padding: UiRect::all(Val::Px(16.0)), ..default() },
    ));
}

fn owner(ship: &Ship, m: Option<&crate::campaign::MissionShip>) -> String {
    m.map(|m| m.owner.clone()).unwrap_or_else(|| if ship.side == Side::Player { "Your fleet".into() } else { "Enemy fleet".into() })
}

fn collect(
    mut stats: ResMut<GameStats>,
    lib: Res<crate::combat::Library>,
    mut hits: MessageReader<Hit>,
    ships: Query<(Entity, &Ship, &Health, Option<&crate::campaign::MissionShip>), Without<crate::lifeboats::Lifeboat>>,
) {
    let owners: HashMap<Entity, String> = ships.iter().map(|(e, s, _, m)| (e, owner(s, m))).collect();
    for h in hits.read() {
        if let (Some(by), Some(entry)) = (owners.get(&h.shooter).cloned(), stats.hulls.get_mut(&h.victim)) {
            entry.3 = Some(by);
        }
    }
    let stats = &mut *stats;
    for (e, s, h, _) in &ships {
        let class = &lib.classes[s.class];
        if class.phys.max_speed <= 0.0 {
            continue; // bases and scenery hulls are not fleet ships
        }
        let hp = h.sections.iter().sum::<f32>() + h.core.max(0.0);
        let me = owners[&e].clone();
        let Some(entry) = stats.hulls.get_mut(&e) else {
            stats.hulls.insert(e, (me.clone(), hp, h.destroyed, None));
            let l = stats.players.entry(me).or_default();
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
        let lost = (entry.1 - hp).max(0.0);
        if lost > 0.0 {
            stats.players.entry(me.clone()).or_default().taken += lost;
            if let Some(by) = entry.3.clone() {
                stats.players.entry(by).or_default().inflicted += lost;
            }
        }
        if h.destroyed && !entry.2 {
            let l = stats.players.entry(me.clone()).or_default();
            l.end = l.end.saturating_sub(1);
            if let Some(by) = entry.3.clone().filter(|b| *b != me) {
                stats.players.entry(by).or_default().sunk += 1;
            }
        }
        entry.1 = hp;
        entry.2 = h.destroyed;
    }
}

/// Skirmish: the summary under the VICTORY / DEFEAT line once the battle is decided.
fn show(stats: Res<GameStats>, out: Res<Outcome>, mission: Option<Res<crate::campaign::Mission>>, mut q: Query<(&mut Text, &mut Visibility), With<Summary>>) {
    let shown = mission.is_none() && *out != Outcome::Playing;
    for (mut t, mut v) in &mut q {
        v.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
        if shown {
            let text = format!("{}\n\nGame Summary\n{}\nR restart   F10 menu", if *out == Outcome::Victory { "VICTORY!" } else { "DEFEAT!" }, stats.table());
            if **t != text {
                **t = text;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn table_lists_every_player() {
        let mut s = GameStats::default();
        s.players.insert("Navy".into(), Line { start: 3, end: 2, sunk: 4, inflicted: 512.0, taken: 300.0 });
        let t = s.table();
        assert!(t.contains("Navy") && t.contains("2/3") && t.contains("512"));
    }
}
