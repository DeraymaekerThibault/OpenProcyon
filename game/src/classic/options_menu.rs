//! Classic right-click options menu art: the variant's four retail background corners and the retail icons at their slot offsets
//! (hud.hdt), and the context cursor sprite (`Cursor_*` with its `Offset_Cursor_*` hot spot). Opening, hit testing, actions and the
//! cursor choice are in the shared `options_menu.rs`.
use bevy::prelude::*;

use crate::options_menu::{HoverCursor, OptionsMenuArt};
use crate::panels::REF_H;
use crate::sprites::HudData;
use crate::sprites::{image, offset};

pub fn plugin(app: &mut App) {
    app.add_systems(PostUpdate, (draw, draw_cursor).before(bevy::ui::UiSystems::Prepare));
}

fn draw(
    mut commands: Commands,
    assets: Res<AssetServer>,
    hud: Option<Res<HudData>>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    menus: Query<(Entity, &OptionsMenuArt), Added<OptionsMenuArt>>,
) {
    let Some(hud) = hud else { return };
    let s = window.height() / REF_H;
    for (root, art) in &menus {
        // A zero-size anchor at the menu centre; the art hangs off it.
        commands.entity(root).insert((GlobalZIndex(170), Node { position_type: PositionType::Absolute, left: Val::Px(art.centre.x), top: Val::Px(art.centre.y), ..default() }));
        let mut put = |img: ImageNode, at: Vec2, size: Vec2| {
            let node = Node {
                position_type: PositionType::Absolute,
                left: Val::Px(at.x * s),
                top: Val::Px(at.y * s),
                width: Val::Px(size.x * s),
                height: Val::Px(size.y * s),
                ..default()
            };
            commands.spawn((img, node, ChildOf(root)));
        };
        for (name, at) in art.background.iter().zip([Vec2::new(-66.0, -68.0), Vec2::new(0.0, -68.0), Vec2::new(0.0, 0.0), Vec2::new(-66.0, 0.0)]) {
            if let Some((img, size)) = image(&assets, &hud, name) {
                put(img, at, size);
            }
        }
        for (sprite, on, at) in &art.slots {
            let Some((mut img, size)) = image(&assets, &hud, sprite) else { continue };
            if !on {
                img.color = Color::srgba(1.0, 1.0, 1.0, 0.35);
            }
            put(img, *at, size);
        }
    }
}

#[derive(Component)]
struct ContextCursor;

fn draw_cursor(
    mut commands: Commands,
    assets: Res<AssetServer>,
    hud: Option<Res<HudData>>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    hover: Res<HoverCursor>,
    mut shown: Query<(&mut Node, &mut ImageNode, &mut Visibility), With<ContextCursor>>,
    mut current: Local<&'static str>,
) {
    let (Some((name, cursor)), Some(hud)) = (hover.0, hud) else {
        for (_, _, mut visibility) in &mut shown {
            visibility.set_if_neq(Visibility::Hidden);
        }
        return;
    };
    let s = window.height() / REF_H;
    let Some((img, size)) = image(&assets, &hud, name) else { return };
    let hot = offset(&hud, &format!("Offset_{name}"));
    let node = Node {
        position_type: PositionType::Absolute,
        left: Val::Px(cursor.x + hot.x * s),
        top: Val::Px(cursor.y + hot.y * s),
        width: Val::Px(size.x * s),
        height: Val::Px(size.y * s),
        ..default()
    };
    match shown.single_mut() {
        Ok((mut shown_node, mut shown_image, mut visibility)) => {
            *shown_node = node;
            if *current != name {
                *shown_image = img;
                *current = name;
            }
            *visibility = Visibility::Inherited;
        }
        Err(_) => {
            *current = "";
            commands.spawn((ContextCursor, crate::Scenario, GlobalZIndex(180), img, node));
        }
    }
}
