//! The boarding status bar over a live fight (hud.hdt BOARDINGSTATUS_*): a gold barbell with the two sides' colour discs, a sword on the
//! attacker's and a shield on the defender's, and between them the relative strength (`boarding::friendly_share`). The original shows it
//! only within 'Max Boarding Info Visible World Distance' of the camera. PROVISIONAL: its screen position (above the target) and the colours.
use bevy::prelude::*;

use crate::boarding::{Boardable, Boarding, friendly_share};
use crate::campaign::Mission;
use crate::sprites::HudData;
use crate::{Ship, Side};

const BAR_VISIBLE: f32 = 250.0;
const BAR_SIZE: Vec2 = Vec2::new(171.0, 40.0);
const BAR_INNER: (Vec2, Vec2) = (Vec2::new(40.0, 13.0), Vec2::new(130.0, 29.0));

pub fn plugin(app: &mut App) {
    app.add_systems(Update, board_bars.after(crate::input).run_if(in_state(crate::menu::Screen::Battle)).run_if(crate::menu::overlay_closed));
}

#[derive(Component)]
struct BoardBar {
    boarder: Entity,
}
#[derive(Component)]
enum BarFill {
    Friendly,
    Foe,
}

fn board_bars(
    mut commands: Commands,
    hud: Option<Res<HudData>>,
    assets: Res<AssetServer>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    mission: Option<Res<Mission>>,
    fights: Query<(Entity, &Boarding)>,
    things: Query<(&Transform, &Boardable, Option<&Ship>)>,
    mut bars: Query<(Entity, &BoardBar, &mut Node, &mut Visibility, &Children)>,
    mut fills: Query<(&BarFill, &mut Node), Without<BoardBar>>,
) {
    let Some(hud) = hud else { return };
    for (entity, bar, ..) in &bars {
        if !fights.get(bar.boarder).is_ok_and(|fight| fight.1.live()) {
            commands.entity(entity).despawn();
        }
    }
    let scale = window.height() / crate::panels::REF_H;
    let (camera, camera_transform) = *camera;
    let friendly = |e: Entity| things.get(e).ok().and_then(|thing| thing.2).is_some_and(|ship| ship.side == Side::Player);
    for (boarder, fight) in fights.iter().filter(|fight| fight.1.live()) {
        let target = fight.target();
        let Ok([(_, boarder_crew, _), (target_transform, target_crew, _)]) = things.get_many([boarder, target]) else { continue };
        let screen = camera
            .world_to_viewport(camera_transform, target_transform.translation)
            .ok()
            .filter(|_| !crate::panels::cinematic(mission.as_deref()) && camera_transform.translation().distance(target_transform.translation) <= BAR_VISIBLE);
        let Some((_, _, mut node, mut visibility, children)) = bars.iter_mut().find(|bar| bar.1.boarder == boarder) else {
            if screen.is_some() {
                spawn_bar(&mut commands, &assets, &hud, boarder, scale, !friendly(target));
            }
            continue;
        };
        let Some(screen) = screen else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        node.left = Val::Px(screen.x - BAR_SIZE.x * 0.5 * scale);
        node.top = Val::Px(screen.y - BAR_SIZE.y * 1.6 * scale);
        let share = friendly_share(&boarder_crew.strength, &target_crew.strength, friendly(target));
        let width = (BAR_INNER.1.x - BAR_INNER.0.x) * scale;
        for child in children.iter() {
            if let Ok((part, mut fill)) = fills.get_mut(child) {
                let (left, fill_width) = match part {
                    BarFill::Friendly => (BAR_INNER.0.x * scale, width * share),
                    BarFill::Foe => (BAR_INNER.0.x * scale + width * share, width * (1.0 - share)),
                };
                fill.left = Val::Px(left);
                fill.width = Val::Px(fill_width);
            }
        }
    }
}

/// `boarder_left`: the boarder's disc is the left one (the friendly side sits on the left); the sword goes on the boarder, the shield on the target.
fn spawn_bar(commands: &mut Commands, assets: &AssetServer, hud: &HudData, boarder: Entity, scale: f32, boarder_left: bool) {
    use crate::sprites::image;
    let absolute = |at: Vec2, size: Vec2| Node {
        position_type: PositionType::Absolute,
        left: Val::Px(at.x * scale),
        top: Val::Px(at.y * scale),
        width: Val::Px(size.x * scale),
        height: Val::Px(size.y * scale),
        ..default()
    };
    let offset = |name: &str| crate::sprites::offset(hud, name);
    let (left, right) = (offset("Player_Color_Icon_Offset_From_BoardingHealthBar_TopLeft"), offset("Enemy_Color_Icon_Offset_From_BoardingHealthBar_TopLeft"));
    let (friendly, foe) = (Color::srgb(0.25, 0.55, 1.0), Color::srgb(0.9, 0.25, 0.2));
    let (sword, shield) = if boarder_left { (left, right) } else { (right, left) };
    commands.spawn((BoardBar { boarder }, crate::Scenario, GlobalZIndex(120), Node { position_type: PositionType::Absolute, ..default() })).with_children(|bar| {
        for (fill, colour) in [(BarFill::Friendly, friendly), (BarFill::Foe, foe)] {
            bar.spawn((fill, absolute(BAR_INNER.0, BAR_INNER.1 - BAR_INNER.0), BackgroundColor(colour)));
        }
        for (name, at, tint) in [
            ("BOARDINGSTATUS_HEALTHBAR", Vec2::ZERO, Color::WHITE),
            ("BOARDINGSTATUS_PLAYERCOLORICON", left, friendly),
            ("BOARDINGSTATUS_PLAYERCOLORICON", right, foe),
            ("BOARDINGSTATUS_ATTACKERICON", sword, Color::WHITE),
            ("BOARDINGSTATUS_DEFENDERICON", shield, Color::WHITE),
        ] {
            if let Some((mut image, size)) = image(assets, hud, name) {
                image.color = tint;
                bar.spawn((image, absolute(at, size)));
            }
        }
    });
}
