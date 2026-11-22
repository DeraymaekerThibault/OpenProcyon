//! Classic drawing of the developer console (`rules::console`): a dark panel over the top of the screen with the output and the
//! line being typed, and a red "CHEATS" tag once a command that changes the game has run. State and input are the shared module's.
use bevy::prelude::*;

use crate::rules::console::{Cheats, Console};

/// Output lines shown above the prompt.
const SHOWN: usize = 16;

#[derive(Component)]
struct Panel;
#[derive(Component)]
struct Tag;

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (build, draw).chain().run_if(in_state(crate::menu::Screen::Battle)).run_if(resource_exists::<Console>));
}

fn build(mut commands: Commands, existing: Query<(), With<Panel>>) {
    if !existing.is_empty() {
        return;
    }
    let font = |size| TextFont { font_size: FontSize::Px(size), ..default() };
    commands.spawn((
        Panel,
        Visibility::Hidden,
        GlobalZIndex(400),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.05, 0.88)),
        Node { position_type: PositionType::Absolute, top: Val::Px(0.0), left: Val::Px(0.0), width: Val::Percent(100.0), padding: UiRect::all(Val::Px(8.0)), ..default() },
        children![(Text::new(""), font(15.0), TextColor(Color::srgb(0.85, 0.95, 0.85)))],
    ));
    commands.spawn((
        Tag,
        Visibility::Hidden,
        GlobalZIndex(401),
        Text::new("CHEATS USED: this battle's points are not recorded"),
        font(14.0),
        TextColor(Color::srgb(1.0, 0.3, 0.25)),
        Node { position_type: PositionType::Absolute, bottom: Val::Px(6.0), left: Val::Px(8.0), ..default() },
    ));
}

fn draw(
    console: Res<Console>,
    cheats: Res<Cheats>,
    mut panel: Query<(&mut Visibility, &Children), (With<Panel>, Without<Tag>)>,
    mut tag: Query<&mut Visibility, (With<Tag>, Without<Panel>)>,
    mut texts: Query<&mut Text>,
) {
    for mut v in &mut tag {
        v.set_if_neq(if cheats.used { Visibility::Inherited } else { Visibility::Hidden });
    }
    for (mut v, children) in &mut panel {
        v.set_if_neq(if console.open { Visibility::Inherited } else { Visibility::Hidden });
        if !console.open {
            continue;
        }
        let Some(mut text) = children.first().and_then(|c| texts.get_mut(*c).ok()) else { continue };
        let tail = console.log.len().saturating_sub(SHOWN);
        let mut shown = console.log[tail..].join("\n");
        shown.push_str(&format!("\n> {}_", console.line));
        if text.0 != shown {
            text.0 = shown;
        }
    }
}
