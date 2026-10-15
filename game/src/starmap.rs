//! Star map: clicking the radar disk (or `M`) opens the retail world map, a full-screen overlay (exe class `MapBackdrop`).
//!
//! From the exe: the map is the mission's `Backdrop Texture Name` (`maps/<id>/backdrop.png`) stretched over the playable square
//! (world size minus the border buffer, `world_half` in the map JSON). It opens over `Animation Speed MapBackdrop` (1 s):
//! blue background and world lines fade in over 0..1, ten "alien text" strips drawn from `MapBackdrop_RandomText0` scroll
//! across it (10..100 px/s, fading in at 0.8..1) and wrap at the right edge. Icons are drawn per world object at its world
//! position, turned by its orientation: the player's ship and the objective point animate (0.1 s/frame), enemies, allies and
//! whales are static sprites. Map text points show their translated name when visible (rules `Set Map Text Visibility`); the
//! current objective point obeys `Set Current Objective Point Visible On Starmap`. Open/close play `HUD_Map_Open/Close`, a
//! `HUD_Map_BackgroundLoop` runs while open and `HUD_Map_MouseOver` plays when the cursor enters the radar.
//! PROVISIONAL: the window layout (centred square, flipped quadrants of the world-line corner texture), close controls
//! (click, Esc, M), label colour and icon orientation sign are not decoded; the exe's pause-when-open gate is a tutorial rule
//! (`Pause when starmap opens`) the remake does not model, so the battle keeps running behind the map.
#![allow(deprecated)] // shortcut: bevy 0.20 Interaction, same as panels.rs
use bevy::audio::{AudioPlayer, PlaybackSettings};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::audio::Sfx;
use crate::combat::Health;
use crate::panels::UiClick;
use crate::panels::{REF_H, RadarButton};
use crate::sprites::{Fonts, HudData, inset, sprite_of};
use crate::{PlayerShip, Ship, Side};

const OPEN_SECS: f32 = 1.0;
const FRAME_SECS: f32 = 0.1;
const ALIEN_COUNT: u32 = 10;

#[derive(Component)]
pub struct Starmap {
    opened: f32,
    side: f32,
    half: f32,
}
/// The square the world maps onto; icons and labels are its children.
#[derive(Component)]
struct Square;
/// Rebuilt every frame.
#[derive(Component)]
struct Dynamic;
#[derive(Component)]
struct Fade(f32, f32); // fade-in window start/end in open-animation seconds
#[derive(Component)]
struct Alien {
    speed: f32,
    width: f32,
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (toggle, animate, redraw).chain().run_if(in_state(crate::menu::Screen::Battle)).run_if(crate::menu::overlay_closed));
}

/// Pseudo-random in [0,1) from a slot and a salt (the exe uses its own LCG; only the spread matters).
fn rnd(i: u32, salt: u32) -> f32 {
    ((i.wrapping_add(1).wrapping_mul(2654435761) ^ salt.wrapping_mul(40503)).wrapping_mul(2246822519) >> 8) as f32 / 16_777_216.0
}

/// World position (bevy x, z) to pixels inside the square.
fn to_square(p: Vec3, half: f32, side: f32) -> Vec2 {
    Vec2::new(p.x + half, p.z + half) / (2.0 * half) * side
}

fn sprite(hud: &HudData, assets: &AssetServer, name: &str) -> Option<(ImageNode, Vec2)> {
    let (sheet, r) = sprite_of(hud, name)?;
    let mut node = ImageNode::new(assets.load(format!("hud/tex/{sheet}.png")));
    node.rect = Some(inset(&sheet, r));
    Some((node, r.size()))
}

/// Run after `panels::hud_input`: while the map is open every click belongs to it.
pub fn block_world(open: Query<(), With<Starmap>>, mut ui: ResMut<UiClick>) {
    ui.0 |= !open.is_empty();
}

#[allow(clippy::too_many_arguments)]
fn toggle(
    mut commands: Commands,
    assets: Res<AssetServer>,
    cfg: Res<crate::Config>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    binds: Res<crate::bindings::Bindings>,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time<Real>>,
    window: Single<&Window, With<PrimaryWindow>>,
    radar: Query<&Interaction, With<RadarButton>>,
    open: Query<Entity, With<Starmap>>,
    hud: Option<Res<HudData>>,
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
    if !(first || (click && hovered) || binds.just_pressed(&keys, crate::bindings::Act::StarMap)) || hud.is_none() {
        return;
    }
    let map: serde_json::Value = cfg.map.as_ref().and_then(|id| crate::data::map_json(id)).unwrap_or_default();
    let half = map["world_half"].as_f64().unwrap_or(1000.0) as f32;
    let side = window.height() * 0.92;
    let (w, h) = (window.width(), window.height());
    let root = commands
        .spawn((
            Starmap { opened: time.elapsed_secs(), side, half },
            crate::Scenario,
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.1, 0.3, 0.0)),
            Fade(0.0, 1.0),
            Interaction::None,
            bevy::ui::FocusPolicy::Block,
            GlobalZIndex(30),
        ))
        .id();
    commands.spawn((AudioPlayer::new(assets.load(crate::audio::sound("hud_map_backgroundloop"))), PlaybackSettings::LOOP, ChildOf(root)));
    sfx.write(Sfx::ui("HUD_Map_Open"));
    // World lines: the corner texture mirrored into the four quadrants.
    for (i, (fx, fy)) in [(false, false), (true, false), (false, true), (true, true)].into_iter().enumerate() {
        let mut img = ImageNode::new(assets.load("hud/tex/640hud_20.png"));
        (img.flip_x, img.flip_y, img.color) = (fx, fy, Color::srgba(1.0, 1.0, 1.0, 0.0));
        commands.spawn((
            img,
            Fade(0.0, 1.0),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(if i % 2 == 0 { 0.0 } else { w / 2.0 }),
                top: Val::Px(if i < 2 { 0.0 } else { h / 2.0 }),
                width: Val::Px(w / 2.0),
                height: Val::Px(h / 2.0),
                ..default()
            },
            ChildOf(root),
        ));
    }
    let square = commands
        .spawn((
            Square,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px((w - side) / 2.0),
                top: Val::Px((h - side) / 2.0),
                width: Val::Px(side),
                height: Val::Px(side),
                ..default()
            },
            ChildOf(root),
        ))
        .id();
    if let (Some(id), true) = (cfg.map.as_ref(), map["backdrop"].is_string()) {
        let mut img = ImageNode::new(assets.load(format!("maps/{id}/backdrop.png")));
        img.color = Color::srgba(1.0, 1.0, 1.0, 0.0);
        commands.spawn((img, Fade(0.0, 1.0), Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, ChildOf(square)));
    }
    let hud = hud.unwrap();
    let s = h / REF_H;
    for i in 0..ALIEN_COUNT {
        let Some((mut img, size)) = sprite(&hud, &assets, "MapBackdrop_RandomText0") else { break };
        img.color = Color::srgba(1.0, 1.0, 1.0, 0.0);
        let speed = 10.0 + rnd(i, 3) * 90.0;
        commands.spawn((
            img,
            Fade(0.8, 1.0),
            Alien { speed, width: size.x * s },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(rnd(i, 1) * w),
                top: Val::Px(rnd(i, 2) * h),
                width: Val::Px(size.x * s),
                height: Val::Px(size.y * s),
                ..default()
            },
            ChildOf(root),
        ));
    }
}

/// Open animation: fades the background, lines, map and alien text; scrolls the alien text.
fn animate(
    time: Res<Time<Real>>,
    window: Single<&Window, With<PrimaryWindow>>,
    root: Query<(Entity, &Starmap)>,
    mut fades: Query<(&Fade, Option<&mut BackgroundColor>, Option<&mut ImageNode>)>,
    mut alien: Query<(&Alien, &mut Node)>,
) {
    let Ok((_, map)) = root.single() else { return };
    let t = (time.elapsed_secs() - map.opened) / OPEN_SECS;
    for (f, bg, img) in &mut fades {
        let a = ((t - f.0) / (f.1 - f.0)).clamp(0.0, 1.0);
        if let Some(mut img) = img {
            img.color = img.color.with_alpha(a);
        } else if let Some(mut bg) = bg {
            bg.0 = bg.0.with_alpha(a * 0.95); // ImageNode has its own (transparent) background: only the root fades one
        }
    }
    for (a, mut node) in &mut alien {
        let x = match node.left {
            Val::Px(x) => x + a.speed * time.delta_secs(),
            _ => 0.0,
        };
        node.left = Val::Px(if x > window.width() { -a.width } else { x });
    }
}

/// Icons and labels for the current world state.
#[allow(clippy::too_many_arguments)]
fn redraw(
    mut commands: Commands,
    assets: Res<AssetServer>,
    hud: Option<Res<HudData>>,
    fonts: Option<Res<Fonts>>,
    time: Res<Time<Real>>,
    window: Single<&Window, With<PrimaryWindow>>,
    root: Query<&Starmap>,
    square: Query<Entity, With<Square>>,
    old: Query<Entity, With<Dynamic>>,
    mission: Option<Res<crate::campaign::Mission>>,
    ships: Query<(Entity, &Ship, &Health, &Transform, Option<&crate::campaign::MissionShip>, Has<PlayerShip>)>,
    animals: Query<(Entity, &Transform), (With<crate::environment::SpaceAnimal>, Without<Ship>)>,
    sensors: Res<crate::environment::Sensors>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    let (Ok(map), Ok(square), Some(hud)) = (root.single(), square.single(), hud) else { return };
    let (half, side, s) = (map.half, map.side, window.height() / REF_H);
    let frame = (time.elapsed_secs() / FRAME_SECS) as usize % 4;
    let icon = |commands: &mut Commands, name: &str, p: Vec3, yaw: f32| {
        let Some((img, size)) = sprite(&hud, &assets, name) else { return };
        let (size, c) = (size * s, to_square(p, half, side));
        if c.x < 0.0 || c.y < 0.0 || c.x > side || c.y > side {
            return;
        }
        commands.spawn((
            Dynamic,
            img,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(c.x - size.x / 2.0),
                top: Val::Px(c.y - size.y / 2.0),
                width: Val::Px(size.x),
                height: Val::Px(size.y),
                ..default()
            },
            UiTransform::from_rotation(Rot2::radians(-yaw)),
            ChildOf(square),
        ));
    };
    let yaw = |t: &Transform| t.rotation.to_euler(EulerRot::YXZ).0;
    for (e, ship, health, tf, ms, is_player) in &ships {
        if is_player {
            icon(&mut commands, &format!("MAPBACKDROP_ICON_PRIMARYSHIP_FRAME{frame}"), tf.translation, yaw(tf));
        } else if !health.destroyed && ms.is_none_or(|m| m.visible) && sensors.player_sees(e, mission.as_deref()) {
            icon(&mut commands, if ship.side == Side::Player { "MapBackdrop_Icon_AlliedShip" } else { "MapBackdrop_Icon_EnemyShip" }, tf.translation, yaw(tf));
        }
    }
    for (e, tf) in &animals {
        if sensors.player_sees(e, mission.as_deref()) {
            icon(&mut commands, "MapBackdrop_Icon_Whale", tf.translation, yaw(tf));
        }
    }
    let Some(m) = mission else { return };
    if m.script.point_on_map
        && let Some(p) = crate::campaign::objective_point(&m)
    {
        icon(&mut commands, &format!("MapBackdrop_Icon_ObjectivePoint_Frame{frame}"), p, 0.0);
    }
    let Some(font) = fonts.as_ref().and_then(|f| f.0.get("Univers12")) else { return };
    for t in m.data["map_text"].as_array().into_iter().flatten() {
        let name = t["name"].as_str().unwrap_or_default();
        if !m.script.map_text.get(name).copied().unwrap_or(t["visible"].as_bool().unwrap_or(false)) {
            continue;
        }
        let c = to_square(crate::game_to_bevy(&t["pos"]), half, side);
        let text = t["text"].as_str().unwrap_or_default();
        let w = font.width(text) * s;
        commands.spawn((
            Dynamic,
            font.text(text, s, Color::srgb(0.75, 1.0, 0.9)),
            Node { position_type: PositionType::Absolute, left: Val::Px(c.x - w / 2.0), top: Val::Px(c.y), ..default() },
            ChildOf(square),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_maps_onto_square() {
        // Mission 1: the playable square is +-1500; the origin is the middle and north (bevy -z) is up.
        assert_eq!(to_square(Vec3::ZERO, 1500.0, 100.0), Vec2::splat(50.0));
        assert_eq!(to_square(Vec3::new(-1500.0, 0.0, -1500.0), 1500.0, 100.0), Vec2::ZERO);
        assert!(to_square(Vec3::new(0.0, 0.0, -750.0), 1500.0, 100.0).y < 50.0);
    }

    #[test]
    fn exported_maps_carry_backdrop_and_world_size() {
        let m: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(crate::assets_dir().join("maps/mission01/map.json")).unwrap()).unwrap();
        assert_eq!(m["world_half"], 1500.0);
        assert!(crate::assets_dir().join("maps/mission01/backdrop.png").exists());
    }
}
