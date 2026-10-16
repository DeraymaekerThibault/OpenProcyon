//! Classic mission text and letterboxing: the objectives line, the cutscene speech banner (black strip with the speaker's portrait), the
//! gameplay subtitle, hint and end text, the cutscene bars and the opening briefing's letterbox. The shared `rules::ui` and
//! `briefing::update` fill in their text, art and visibility.
//! Layout measured from the reference video, not decoded native NIS window placement.
use bevy::prelude::*;

use crate::Scenario;
use crate::briefing::{BriefingUi, Portrait, Subtitle};
use crate::campaign::{Mission, ObjectiveText};
use crate::rules::Ui;
use crate::sprites::{HudData, inset, sprite_of};

const SPEECH_GOLD: Color = Color::srgb(1.0, 0.65, 0.1);

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (build_mission, build_briefing).before(crate::briefing::update).run_if(in_state(crate::menu::Screen::Battle)).run_if(resource_exists::<Mission>));
}

/// The top strip both the briefing and cutscene speech use.
fn speech_strip() -> Node {
    Node { position_type: PositionType::Absolute, top: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(14.0), ..default() }
}
/// The speech line beside the portrait.
fn speech_line() -> Node {
    Node { position_type: PositionType::Absolute, left: Val::Px(120.0), right: Val::Px(30.0), top: Val::Px(12.0), ..default() }
}
fn absolute(left: f32, top: f32, size: f32) -> Node {
    Node { position_type: PositionType::Absolute, left: Val::Px(left), top: Val::Px(top), width: Val::Px(size), height: Val::Px(size), ..default() }
}
fn text(size: f32, colour: Color) -> (Text, TextFont, TextColor) {
    (Text::new(""), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(colour))
}

/// Mission widgets, once per loaded mission (a restart despawns them with the rest of the `Scenario`).
fn build_mission(mut commands: Commands, existing: Query<(), With<Ui>>) {
    if !existing.is_empty() {
        return;
    }
    commands.spawn((
        Scenario,
        ObjectiveText,
        text(18.0, Color::srgb(1.0, 0.9, 0.5)),
        Node { position_type: PositionType::Absolute, top: Val::Px(12.0), left: Val::Px(180.0), right: Val::Px(180.0), ..default() },
    ));
    // Cutscene speech banner as in the opening briefing: black strip at the top with the speaker's portrait and the line.
    commands.spawn((Scenario, Ui::SpeechPanel, Visibility::Hidden, BackgroundColor(Color::BLACK), GlobalZIndex(150), speech_strip()));
    for (kind, left, top, size, z) in [(Ui::SpeechFrame, 6.0, 1.0, 104.0, 155), (Ui::SpeechHead, 26.0, 21.0, 64.0, 156)] {
        commands.spawn((Scenario, kind, ImageNode::default(), Visibility::Hidden, GlobalZIndex(z), absolute(left, top, size)));
    }
    commands.spawn((Scenario, Ui::SpeechTop, text(20.0, SPEECH_GOLD), Visibility::Hidden, GlobalZIndex(160), speech_line()));
    // Gameplay speech subtitle sits just above the main bar (bar is ~7.7% of the window height; PROVISIONAL margin).
    let at = |top: Option<f32>, bottom: Option<f32>, side: f32| Node {
        position_type: PositionType::Absolute,
        top: top.map_or(Val::Auto, Val::Percent),
        bottom: bottom.map_or(Val::Auto, Val::Percent),
        left: Val::Percent(side),
        right: Val::Percent(side),
        ..default()
    };
    commands.spawn((Scenario, Ui::Speech, text(20.0, Color::srgb(1.0, 0.75, 0.2)), Visibility::Hidden, GlobalZIndex(160), at(None, Some(9.5), 20.0)));
    commands.spawn((Scenario, Ui::Hint, text(18.0, Color::srgb(0.7, 0.9, 1.0)), Visibility::Hidden, GlobalZIndex(160), at(Some(30.0), None, 30.0)));
    commands.spawn((Scenario, Ui::End, text(34.0, Color::WHITE), Visibility::Hidden, GlobalZIndex(170), at(Some(35.0), None, 15.0)));
    for (bar, top) in [(Ui::BarTop, true), (Ui::BarBottom, false)] {
        let mut node = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(12.0), ..default() };
        if top {
            node.top = Val::Px(0.0)
        } else {
            node.bottom = Val::Px(0.0)
        }
        commands.spawn((Scenario, bar, node, BackgroundColor(Color::BLACK), Visibility::Hidden, GlobalZIndex(140)));
    }
}

/// The opening briefing's letterbox while `Mission::opening` runs: top strip with Amelia's portrait and the line, bottom strip with
/// the skip hint.
fn build_briefing(mut commands: Commands, mission: Res<Mission>, assets: Res<AssetServer>, hud: Res<HudData>, existing: Query<(), With<BriefingUi>>) {
    if mission.opening.is_none() || !existing.is_empty() {
        return;
    }
    commands.spawn((Scenario, BriefingUi, GlobalZIndex(150), speech_strip(), BackgroundColor(Color::BLACK))).with_children(|strip| {
        for (name, left, top, size) in [("TalkingHead_Character_Amelia_Happy", 26.0, 21.0, 64.0), ("TalkingHead_FloatingPortal_Open", 6.0, 1.0, 104.0)] {
            let (sheet, rect) = sprite_of(&hud, name).expect("briefing portrait sprite");
            let mut image = ImageNode::new(assets.load(format!("hud/tex/{sheet}.png")));
            image.rect = Some(inset(&sheet, rect));
            strip.spawn((Portrait, image, Visibility::Hidden, absolute(left, top, size)));
        }
        strip.spawn((Subtitle, text(20.0, SPEECH_GOLD), speech_line()));
    });
    commands
        .spawn((
            Scenario,
            BriefingUi,
            GlobalZIndex(150),
            Node { position_type: PositionType::Absolute, bottom: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(14.0), ..default() },
            BackgroundColor(Color::BLACK),
        ))
        .with_children(|strip| {
            strip.spawn((
                Text::new("Space: next line"),
                TextFont { font_size: FontSize::Px(16.0), ..default() },
                Node { position_type: PositionType::Absolute, right: Val::Px(20.0), bottom: Val::Px(12.0), ..default() },
            ));
        });
}
