//! Mouse-wheel scrolling of the front-end lists. Which list scrolls on which screen, how many rows it shows and how far it can
//! go are model decisions, so they live here; the view only draws the visible rows from the first-row fields.

use bevy::prelude::*;

use super::{CtrlUi, InGame, MISSION_ROWS, MOD_ROWS, MissionList, ModList, Page, SaveUi, Screen, Skirmish, offered};
use crate::bindings::Bindings;
use crate::combat::Library;

/// Rows of the Create Skirmish map list (the original's eight open maps fill it exactly).
pub const MAP_ROWS: usize = 8;
/// Rows of the Ship Types Available list.
pub const SHIP_ROWS: usize = 8;
/// Rows of a save list.
pub const SAVE_ROWS: usize = 10;
/// Rows of the Control Options list on its own screen and inside the in-game menu.
pub const CONTROL_ROWS: usize = 17;
pub const CONTROL_ROWS_IN_GAME: usize = 8;

/// One mouse-wheel step of a list's first visible row: up scrolls back, down forward up to `max`.
pub fn wheel_step(first: usize, dy: f32, max: usize) -> usize {
    if dy > 0.0 { first.saturating_sub(1) } else { (first + 1).min(max) }
}

/// Last first-row of a list of `total` rows of which `rows` fit.
fn max_first(total: usize, rows: usize) -> usize {
    total.saturating_sub(rows)
}

/// Rows of the Control Options list that fit: fewer inside the in-game menu.
fn control_rows(in_game_page: bool) -> usize {
    if in_game_page { CONTROL_ROWS_IN_GAME } else { CONTROL_ROWS }
}

fn wheel_dy(wheel: &mut MessageReader<bevy::input::mouse::MouseWheel>) -> f32 {
    wheel.read().map(|e| e.y).sum()
}

/// Mouse wheel scrolls the Control Options list.
pub fn scroll_controls(mut wheel: MessageReader<bevy::input::mouse::MouseWheel>, state: Res<State<Screen>>, ig: InGame, mut cui: ResMut<CtrlUi>, binds: Res<Bindings>) {
    let dy = wheel_dy(&mut wheel);
    if dy != 0.0 && (*state.get() == Screen::Controls || ig.page() == Some(Page::Controls)) {
        let max = max_first(binds.rows.len(), control_rows(ig.page().is_some()));
        cui.scroll = wheel_step(cui.scroll, dy, max);
    }
}

/// Mouse wheel scrolls the Mods table.
pub fn scroll_mods(mut wheel: MessageReader<bevy::input::mouse::MouseWheel>, state: Res<State<Screen>>, mut mods: ResMut<ModList>) {
    let dy = wheel_dy(&mut wheel);
    if dy != 0.0 && *state.get() == Screen::Mods {
        let max = max_first(mods.rows().len(), MOD_ROWS);
        mods.scroll = wheel_step(mods.scroll, dy, max);
    }
}

/// Mouse wheel scrolls the Custom Missions list.
pub fn scroll_missions(mut wheel: MessageReader<bevy::input::mouse::MouseWheel>, state: Res<State<Screen>>, mut missions: ResMut<MissionList>) {
    let dy = wheel_dy(&mut wheel);
    if dy != 0.0 && *state.get() == Screen::Missions {
        let max = max_first(missions.list.len(), MISSION_ROWS);
        missions.scroll = wheel_step(missions.scroll, dy, max);
    }
}

/// Mouse wheel scrolls the save lists.
pub fn scroll_saves(mut wheel: MessageReader<bevy::input::mouse::MouseWheel>, state: Res<State<Screen>>, ig: InGame, mut ui: ResMut<SaveUi>) {
    let dy = wheel_dy(&mut wheel);
    if dy != 0.0 && (*state.get() == Screen::Load || matches!(ig.page(), Some(Page::Load | Page::Save))) {
        let max = max_first(ui.list.len(), SAVE_ROWS);
        ui.scroll = wheel_step(ui.scroll, dy, max);
    }
}

/// Mouse wheel scrolls the map list (Create Skirmish) and the Ship Types Available list (Fleet).
pub fn scroll_skirmish(mut wheel: MessageReader<bevy::input::mouse::MouseWheel>, state: Res<State<Screen>>, mut sk: ResMut<Skirmish>, lib: Res<Library>) {
    let dy = wheel_dy(&mut wheel);
    if dy == 0.0 {
        return;
    }
    match *state.get() {
        Screen::CreateSkirmish => {
            let n = sk.maps.iter().filter(|m| m.2 != sk.historical).count();
            sk.map_scroll = wheel_step(sk.map_scroll, dy, max_first(n, MAP_ROWS));
        }
        Screen::Fleet => {
            let n = sk.human().map_or(0, |h| offered(&lib, h.race).len());
            sk.scroll = wheel_step(sk.scroll, dy, max_first(n, SHIP_ROWS));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_step_stays_in_range() {
        assert_eq!(wheel_step(0, 1.0, 5), 0);
        assert_eq!(wheel_step(5, -1.0, 5), 5);
        assert_eq!(wheel_step(2, -1.0, 5), 3);
        assert_eq!(wheel_step(2, 1.0, 5), 1);
    }

    #[test]
    fn a_list_shorter_than_its_window_does_not_scroll() {
        assert_eq!(max_first(3, MAP_ROWS), 0);
        assert_eq!(max_first(MAP_ROWS, MAP_ROWS), 0);
        assert_eq!(max_first(MAP_ROWS + 3, MAP_ROWS), 3);
    }

    #[test]
    fn the_in_game_controls_page_shows_fewer_rows() {
        assert_eq!((control_rows(false), control_rows(true)), (17, 8));
    }

    fn app(saves: usize) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .init_state::<Screen>()
            .add_sub_state::<crate::menu::Overlay>()
            .insert_resource(SaveUi { list: (0..saves).map(|_| (Default::default(), false)).collect(), ..default() })
            .add_message::<bevy::input::mouse::MouseWheel>()
            .add_systems(Update, scroll_saves);
        app
    }

    fn wheel(app: &mut App, y: f32) {
        app.world_mut().write_message(bevy::input::mouse::MouseWheel {
            unit: bevy::input::mouse::MouseScrollUnit::Line,
            x: 0.0,
            y,
            window: Entity::PLACEHOLDER,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
        app.update();
    }

    #[test]
    fn the_wheel_only_scrolls_the_list_of_the_open_screen() {
        let mut app = app(SAVE_ROWS + 2);
        wheel(&mut app, -1.0);
        assert_eq!(app.world().resource::<SaveUi>().scroll, 0, "Main screen: the save list does not move");
        app.world_mut().resource_mut::<NextState<Screen>>().set(Screen::Load);
        app.update();
        for _ in 0..5 {
            wheel(&mut app, -1.0);
        }
        assert_eq!(app.world().resource::<SaveUi>().scroll, 2, "stops at the last full window");
        wheel(&mut app, 1.0);
        assert_eq!(app.world().resource::<SaveUi>().scroll, 1);
    }
}
