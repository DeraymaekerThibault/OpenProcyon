//! Classic right-click options menu art: the variant's four retail background corners and the retail icons at their slot offsets
//! (hud.hdt). Opening, hit testing and actions are in the shared `options_menu.rs`.
use bevy::prelude::*;

use crate::options_menu::OptionsMenuArt;
use crate::panels::REF_H;
use crate::sprites::HudData;
use crate::sprites::image;

pub fn plugin(app: &mut App) {
    app.add_systems(PostUpdate, draw.before(bevy::ui::UiSystems::Prepare));
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
