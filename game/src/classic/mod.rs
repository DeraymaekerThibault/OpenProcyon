//! Classic presentation: the faithful retail look (default). Views only: state, rules and input live in the shared modules
//! (`menu.rs`, `panels.rs`, `options_menu.rs`, ...). Other views live in other crates; see docs/development.md "Layers".
use bevy::prelude::*;

mod boarding_bar;
pub mod crew_arms;
pub mod menu;
mod mission_ui;
pub mod options_menu;
pub mod panels;
mod ropes;
pub mod setup;
mod ship_preview;
mod starmap;
mod widgets;
mod world_hud;

pub fn plugin(app: &mut App) {
    app.add_plugins((
        menu::plugin,
        widgets::plugin,
        panels::plugin,
        options_menu::plugin,
        crew_arms::plugin,
        ship_preview::plugin,
        boarding_bar::plugin,
        starmap::plugin,
        world_hud::plugin,
        mission_ui::plugin,
        ropes::plugin,
    ));
}
