//! Classic star map art (`starmap.rs` opens it and says what it shows): the mission backdrop stretched over the playable square, blue
//! background and world lines fading in over the 1 s open animation (`Animation Speed MapBackdrop`), ten "alien text" strips from
//! `MapBackdrop_RandomText0` scrolling across (10..100 px/s, fading in at 0.8..1) and wrapping at the right edge, and per-object icons
//! turned by their orientation; the player's ship and the objective point animate at 0.1 s/frame.
//! PROVISIONAL: the window layout (centred square, flipped quadrants of the world-line corner texture), label colour and icon
//! orientation sign are not decoded.
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::panels::REF_H;
use crate::sprites::{Fonts, HudData, image};
use crate::starmap::{MapIconKind, Starmap, StarmapContents};

const OPEN_SECS: f32 = 1.0;
const FRAME_SECS: f32 = 0.1;
const ALIEN_COUNT: u32 = 10;

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (build, animate, redraw).chain().after(crate::starmap::StarmapSet).run_if(in_state(crate::menu::Screen::Battle)));
}

/// The square the world maps onto; icons and labels are its children.
#[derive(Component)]
struct Square {
    side: f32,
}
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

/// Pseudo-random in [0,1) from a slot and a salt (only the spread matters).
fn rnd(i: u32, salt: u32) -> f32 {
    ((i.wrapping_add(1).wrapping_mul(2654435761) ^ salt.wrapping_mul(40503)).wrapping_mul(2246822519) >> 8) as f32 / 16_777_216.0
}

/// World position (bevy x, z) to pixels inside the square.
fn to_square(p: Vec3, half: f32, side: f32) -> Vec2 {
    Vec2::new(p.x + half, p.z + half) / (2.0 * half) * side
}

#[allow(deprecated)] // shortcut: bevy 0.20 Interaction, same as panels.rs
fn build(
    mut commands: Commands,
    assets: Res<AssetServer>,
    window: Single<&Window, With<PrimaryWindow>>,
    opened: Query<(Entity, &Starmap), Added<Starmap>>,
    hud: Option<Res<HudData>>,
) {
    let (Ok((root, map)), Some(hud)) = (opened.single(), hud) else { return };
    let (w, h) = (window.width(), window.height());
    let side = h * 0.92;
    commands.entity(root).insert((
        Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
        BackgroundColor(Color::srgba(0.0, 0.1, 0.3, 0.0)),
        Fade(0.0, 1.0),
        Interaction::None,
        bevy::ui::FocusPolicy::Block,
        GlobalZIndex(30),
    ));
    // World lines: the corner texture mirrored into the four quadrants.
    for (i, (flip_x, flip_y)) in [(false, false), (true, false), (false, true), (true, true)].into_iter().enumerate() {
        let mut img = ImageNode::new(assets.load("hud/tex/640hud_20.png"));
        (img.flip_x, img.flip_y, img.color) = (flip_x, flip_y, Color::srgba(1.0, 1.0, 1.0, 0.0));
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
            Square { side },
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
    if let Some(id) = &map.backdrop {
        let mut img = ImageNode::new(assets.load(format!("maps/{id}/backdrop.png")));
        img.color = Color::srgba(1.0, 1.0, 1.0, 0.0);
        commands.spawn((img, Fade(0.0, 1.0), Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, ChildOf(square)));
    }
    let scale = h / REF_H;
    for i in 0..ALIEN_COUNT {
        let Some((mut img, size)) = image(&assets, &hud, "MapBackdrop_RandomText0") else { break };
        img.color = Color::srgba(1.0, 1.0, 1.0, 0.0);
        let speed = 10.0 + rnd(i, 3) * 90.0;
        commands.spawn((
            img,
            Fade(0.8, 1.0),
            Alien { speed, width: size.x * scale },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(rnd(i, 1) * w),
                top: Val::Px(rnd(i, 2) * h),
                width: Val::Px(size.x * scale),
                height: Val::Px(size.y * scale),
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
    root: Query<&Starmap>,
    mut fades: Query<(&Fade, Option<&mut BackgroundColor>, Option<&mut ImageNode>)>,
    mut alien: Query<(&Alien, &mut Node)>,
) {
    let Ok(map) = root.single() else { return };
    let t = (time.elapsed_secs() - map.opened) / OPEN_SECS;
    for (fade, background, img) in &mut fades {
        let alpha = ((t - fade.0) / (fade.1 - fade.0)).clamp(0.0, 1.0);
        if let Some(mut img) = img {
            img.color = img.color.with_alpha(alpha);
        } else if let Some(mut background) = background {
            background.0 = background.0.with_alpha(alpha * 0.95); // ImageNode has its own (transparent) background: only the root fades one
        }
    }
    for (alien, mut node) in &mut alien {
        let x = match node.left {
            Val::Px(x) => x + alien.speed * time.delta_secs(),
            _ => 0.0,
        };
        node.left = Val::Px(if x > window.width() { -alien.width } else { x });
    }
}

/// Retail's map text colour (a yellow-gold, read off a Border Dispute screenshot).
const LABEL_GOLD: Color = Color::srgb(0.95, 0.85, 0.3);

/// Icons and labels for `StarmapContents`.
fn redraw(
    mut commands: Commands,
    assets: Res<AssetServer>,
    hud: Option<Res<HudData>>,
    fonts: Option<Res<Fonts>>,
    time: Res<Time<Real>>,
    window: Single<&Window, With<PrimaryWindow>>,
    root: Query<(Entity, &Starmap)>,
    square: Query<(Entity, &Square)>,
    old: Query<Entity, With<Dynamic>>,
    contents: Res<StarmapContents>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    let (Ok((root, map)), Ok((square, &Square { side })), Some(hud)) = (root.single(), square.single(), hud) else { return };
    let (half, scale) = (map.half, window.height() / REF_H);
    let frame = (time.elapsed_secs() / FRAME_SECS) as usize % 4;
    for icon in &contents.icons {
        let name = match icon.kind {
            MapIconKind::PlayerShip => format!("MAPBACKDROP_ICON_PRIMARYSHIP_FRAME{frame}"),
            MapIconKind::AlliedShip => "MapBackdrop_Icon_AlliedShip".into(),
            MapIconKind::EnemyShip => "MapBackdrop_Icon_EnemyShip".into(),
            MapIconKind::Whale => "MapBackdrop_Icon_Whale".into(),
            MapIconKind::Objective => format!("MapBackdrop_Icon_ObjectivePoint_Frame{frame}"),
        };
        let Some((img, size)) = image(&assets, &hud, &name) else { continue };
        let (size, centre) = (size * scale, to_square(icon.position, half, side));
        if centre.x < 0.0 || centre.y < 0.0 || centre.x > side || centre.y > side {
            continue;
        }
        commands.spawn((
            Dynamic,
            img,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(centre.x - size.x / 2.0),
                top: Val::Px(centre.y - size.y / 2.0),
                width: Val::Px(size.x),
                height: Val::Px(size.y),
                ..default()
            },
            UiTransform::from_rotation(Rot2::radians(-icon.yaw)),
            ChildOf(square),
        ));
    }
    let Some(font) = fonts.as_ref().and_then(|f| f.0.get(crate::sprites::BODY_FONT)) else { return };
    if let Some(text) = &contents.objective {
        // Retail: a blue panel with a gold border at the top left, "Objectives:" over the task text (Border Dispute, 1440x1080: ~160x58 px).
        commands
            .spawn((
                Dynamic,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(8.0 * scale),
                    top: Val::Px(8.0 * scale),
                    padding: UiRect::axes(Val::Px(10.0 * scale), Val::Px(6.0 * scale)),
                    border: UiRect::all(Val::Px(3.0 * scale)),
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.1, 0.35, 0.6)),
                BorderColor::all(Color::srgb(0.85, 0.7, 0.25)),
                ChildOf(root),
            ))
            .with_children(|panel| {
                panel.spawn(font.text("Objectives:", scale, Color::WHITE));
                panel.spawn(font.text(text, scale, LABEL_GOLD));
            });
    }
    for label in &contents.labels {
        let at = to_square(label.position, half, side);
        let width = font.width(&label.text) * scale;
        commands.spawn((
            Dynamic,
            font.text(&label.text, scale, LABEL_GOLD),
            Node { position_type: PositionType::Absolute, left: Val::Px(at.x - width / 2.0), top: Val::Px(at.y), ..default() },
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
}
