//! Classic presentation: the faithful retail look (default). Views only: state, rules and input live in the shared modules
//! (`menu.rs`, `panels.rs`, `options_menu.rs`, ...). Other views live in other crates; see docs/development.md "Layers".
use bevy::prelude::*;

pub mod crew_arms;
pub mod menu;
pub mod options_menu;
pub mod panels;

pub fn plugin(app: &mut App) {
    app.add_plugins((menu::plugin, panels::plugin, options_menu::plugin, crew_arms::plugin));
}
