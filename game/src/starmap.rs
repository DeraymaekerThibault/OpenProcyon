//! Star map: clicking the radar disk (or `M`) opens the retail world map, a full-screen overlay.
//!
//! From the original: the map is the mission's `Backdrop Texture Name` (`maps/<id>/backdrop.png`) stretched over the playable square
//! (world size minus the border buffer, `world_half` in the map JSON). It opens over `Animation Speed MapBackdrop` (1 s):
//! blue background and world lines fade in over 0..1, ten "alien text" strips drawn from `MapBackdrop_RandomText0` scroll
//! across it (10..100 px/s, fading in at 0.8..1) and wrap at the right edge. Icons are drawn per world object at its world
//! position, turned by its orientation: the player's ship and the objective point animate (0.1 s/frame), enemies, allies and
//! whales are static sprites. Map text points show their translated name when visible (rules `Set Map Text Visibility`); the
//! current objective point obeys `Set Current Objective Point Visible On Starmap`. Open/close play `HUD_Map_Open/Close`, a
//! `HUD_Map_BackgroundLoop` runs while open and `HUD_Map_MouseOver` plays when the cursor enters the radar.
//! This module owns opening/closing, the sounds and what the map shows (`StarmapContents`); `classic/starmap.rs` draws it.
//! PROVISIONAL: the window layout (centred square, flipped quadrants of the world-line corner texture), close controls
//! (click, Esc, M), label colour and icon orientation sign are unknown; the original's pause-when-open gate is a tutorial rule
//! (`Pause when starmap opens`) the remake does not model, so the battle keeps running behind the map.
#![allow(deprecated)] // shortcut: bevy 0.20 Interaction, same as panels.rs
use bevy::audio::{AudioPlayer, PlaybackSettings};
use bevy::prelude::*;

use crate::audio::Sfx;
use crate::combat::Health;
use crate::panels::RadarButton;
use crate::panels::UiClick;
use crate::{PlayerShip, Ship, Side};

/// The open star map; despawning it closes the map. Views build their art under it.
#[derive(Component)]
pub struct Starmap {
    /// Real time it opened, for the open animation.
    pub opened: f32,
    /// Half the playable square's side in world units (map JSON `world_half`).
    pub half: f32,
    /// The map's id, for its backdrop texture (`maps/<id>/backdrop.png`), when it has one.
    pub backdrop: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapIconKind {
    PlayerShip,
    AlliedShip,
    EnemyShip,
    Whale,
    Objective,
}
pub struct MapIcon {
    pub kind: MapIconKind,
    pub position: Vec3,
    pub yaw: f32,
}
pub struct MapLabel {
    pub text: String,
    pub position: Vec3,
}
/// What the open map shows this frame: only what the player may see (sensors, mission visibility rules).
#[derive(Resource, Default)]
pub struct StarmapContents {
    pub icons: Vec<MapIcon>,
    pub labels: Vec<MapLabel>,
}

/// Opening, closing and `describe`; views draw after it.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub struct StarmapSet;

pub fn plugin(app: &mut App) {
    app.init_resource::<StarmapContents>()
        .add_systems(Update, (toggle, describe).chain().in_set(StarmapSet).run_if(in_state(crate::menu::Screen::Battle)).run_if(crate::menu::overlay_closed));
}

/// Run after `panels::hud_input`: while the map is open every click belongs to it.
pub fn block_world(open: Query<(), With<Starmap>>, mut ui: ResMut<UiClick>) {
    ui.0 |= !open.is_empty();
}

fn toggle(
    mut commands: Commands,
    assets: Res<AssetServer>,
    cfg: Res<crate::Config>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    binds: Res<crate::bindings::Bindings>,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time<Real>>,
    radar: Query<&Interaction, With<RadarButton>>,
    open: Query<Entity, With<Starmap>>,
    mut over: Local<bool>,
    mut sfx: MessageWriter<Sfx>,
    mut auto: Local<Option<bool>>,
) {
    let hovered = radar.iter().any(|i| *i != Interaction::None);
    if hovered && !*over {
        sfx.write(Sfx::ui("HUD_Map_MouseOver"));
    }
    *over = hovered;
    // `--starmap` opens the map on the first frame (screenshots).
    let first = auto.is_none_or(|a| !a) && std::env::args().any(|a| a == "--starmap");
    *auto = Some(true);
    let click = mouse.just_pressed(MouseButton::Left);
    if let Ok(root) = open.single() {
        if click || keys.just_pressed(KeyCode::Escape) || binds.just_pressed(&keys, crate::bindings::Act::StarMap) {
            keys.clear_just_pressed(KeyCode::Escape); // this Esc closed the map; it must not also open the in-game menu
            commands.entity(root).despawn();
            sfx.write(Sfx::ui("HUD_Map_Close"));
        }
        return;
    }
    if !(first || (click && hovered) || binds.just_pressed(&keys, crate::bindings::Act::StarMap)) {
        return;
    }
    let map: serde_json::Value = cfg.map.as_ref().and_then(|id| crate::data::map_json(id)).unwrap_or_default();
    let half = map["world_half"].as_f64().unwrap_or(1000.0) as f32;
    let backdrop = cfg.map.clone().filter(|_| map["backdrop"].is_string());
    let root = commands.spawn((Starmap { opened: time.elapsed_secs(), half, backdrop }, crate::Scenario)).id();
    commands.spawn((
        AudioPlayer::new(assets.load(crate::audio::sound("hud_map_backgroundloop"))),
        PlaybackSettings::LOOP.with_volume(bevy::audio::Volume::Linear(crate::menu::volume(crate::menu::Setting::Effects))),
        ChildOf(root),
    ));
    sfx.write(Sfx::ui("HUD_Map_Open"));
}

/// Fills `StarmapContents` while the map is open.
fn describe(
    mut contents: ResMut<StarmapContents>,
    open: Query<(), With<Starmap>>,
    mission: Option<Res<crate::campaign::Mission>>,
    ships: Query<(Entity, &Ship, &Health, &Transform, Option<&crate::campaign::MissionShip>, Has<PlayerShip>)>,
    animals: Query<(Entity, &Transform), (With<crate::environment::SpaceAnimal>, Without<Ship>)>,
    sensors: Res<crate::environment::Sensors>,
) {
    contents.icons.clear();
    contents.labels.clear();
    if open.is_empty() {
        return;
    }
    let yaw = |t: &Transform| t.rotation.to_euler(EulerRot::YXZ).0;
    let mut icon = |kind, transform: &Transform| contents.icons.push(MapIcon { kind, position: transform.translation, yaw: yaw(transform) });
    for (entity, ship, health, transform, mission_ship, is_player) in &ships {
        if is_player {
            icon(MapIconKind::PlayerShip, transform);
        } else if !health.destroyed && mission_ship.is_none_or(|m| m.visible) && sensors.player_sees(entity, mission.as_deref()) {
            icon(if ship.side == Side::Player { MapIconKind::AlliedShip } else { MapIconKind::EnemyShip }, transform);
        }
    }
    for (entity, transform) in &animals {
        if sensors.player_sees(entity, mission.as_deref()) {
            icon(MapIconKind::Whale, transform);
        }
    }
    let Some(mission) = mission else { return };
    if mission.script.point_on_map
        && let Some(position) = crate::campaign::objective_point(&mission)
    {
        contents.icons.push(MapIcon { kind: MapIconKind::Objective, position, yaw: 0.0 });
    }
    for text in mission.data["map_text"].as_array().into_iter().flatten() {
        let name = text["name"].as_str().unwrap_or_default();
        if mission.script.map_text.get(name).copied().unwrap_or(text["visible"].as_bool().unwrap_or(false)) {
            contents.labels.push(MapLabel { text: text["text"].as_str().unwrap_or_default().into(), position: crate::game_to_bevy(&text["pos"]) });
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn exported_maps_carry_backdrop_and_world_size() {
        let m: serde_json::Value = crate::data::read_json(&crate::assets_dir().join("maps/mission01/map.json"));
        assert_eq!(m["world_half"], 1500.0);
        assert!(crate::assets_dir().join("maps/mission01/backdrop.png").exists());
    }
}
