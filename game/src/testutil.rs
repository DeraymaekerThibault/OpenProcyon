//! Shared fixtures for unit tests (crate-private, test builds only).
//!
//! `Library` is not `Clone`, so there is no cached copy: `lib()` loads the imported assets each call. Tests that need game data are
//! `#[ignore = "needs imported assets"]` and read it through here, so a cache could replace the body in one place.

use bevy::app::TaskPoolPlugin;
use bevy::asset::AssetPlugin;
use bevy::prelude::*;

use crate::assets_dir;
use crate::data::Library;

/// An app with the asset server and the asset types ship and base models load into (scene, image) and transform propagation.
pub fn asset_app() -> App {
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default(), TransformPlugin)).init_asset::<bevy::world_serialization::WorldAsset>().init_asset::<Image>();
    app
}

/// The imported game data (needs `assets/`).
pub fn lib() -> Library {
    Library::load(assets_dir())
}

/// The local command ship (the one entity carrying `PlayerShip` or `Commander` in a test world).
fn command_ship(w: &mut World) -> Entity {
    w.flush();
    w.query_filtered::<Entity, Or<(With<crate::PlayerShip>, With<crate::commands::Commander>)>>().iter(w).next().expect("a command ship")
}

/// The command ship's attack click (its ordered target), as the HUD shows it.
pub fn command_target(w: &mut World) -> Option<Entity> {
    let e = command_ship(w);
    w.get::<crate::ship::TargetSlot>(e).and_then(crate::ship::TargetSlot::ordered).map(|o| o.0)
}

/// An attack click of the command ship on `target` (none clears it), with no spot.
pub fn set_command_target(w: &mut World, target: Option<Entity>) {
    let e = command_ship(w);
    w.entity_mut(e).insert(crate::ship::TargetSlot::order(target, None));
}
