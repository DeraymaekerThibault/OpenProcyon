//! Classic mission text and letterboxing: the cutscene speech banner (black strip with the speaker's portrait), the
//! gameplay subtitle, hint and end text, the cutscene bars and the opening briefing's letterbox. The shared `rules::ui` and
//! `briefing::update` fill in their text, art and visibility.
//! Layout measured from the reference video, not decoded native NIS window placement.
use bevy::prelude::*;

use crate::Scenario;
use crate::briefing::{BriefingUi, Portrait, Subtitle};
use crate::campaign::Mission;
use crate::rules::Ui;
use crate::sprites::{HudData, sprite};

const SPEECH_GOLD: Color = Color::srgb(1.0, 0.65, 0.1);

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (build_mission, build_briefing).before(crate::briefing::update).run_if(in_state(crate::menu::Screen::Battle)).run_if(resource_exists::<Mission>))
        .add_systems(Update, (speech_colour, fade_picture).run_if(in_state(crate::menu::Screen::Battle)).run_if(resource_exists::<Mission>))
        .add_systems(
            Update,
            (build_skirmish_speech, skirmish_subtitle).chain().run_if(in_state(crate::menu::Screen::Battle)).run_if(resource_exists::<crate::rules::skirmish::SkirmishScript>),
        );
}

/// Gameplay and cutscene subtitles take the speech event's own text colour (each speaker has one); gold when the event has none.
fn speech_colour(mission: Res<Mission>, mut texts: Query<(&Ui, &mut TextColor)>) {
    let colour = mission.script.speech_colour().map_or(SPEECH_GOLD, |[r, g, b, a]| Color::srgba(r, g, b, a));
    for (ui, mut c) in &mut texts {
        if matches!(ui, Ui::Speech | Ui::SpeechTop) && c.0 != colour {
            c.0 = colour;
        }
    }
}

/// Not retail: how black the picture is (`OP Fade`, `Script::fade_alpha`); the shared `rules::ui` shows the cover while it is above 0.
fn fade_picture(mission: Res<Mission>, mut covers: Query<(&Ui, &mut BackgroundColor)>) {
    let alpha = mission.script.fade_alpha();
    for (ui, mut c) in &mut covers {
        if matches!(ui, Ui::Fade) {
            c.set_if_neq(BackgroundColor(Color::srgba(0.0, 0.0, 0.0, alpha)));
        }
    }
}

/// The top strip both the briefing and cutscene speech use.
fn speech_strip() -> Node {
    Node { position_type: PositionType::Absolute, top: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(14.0), ..default() } // PROVISIONAL: the 14% strip height is eyeballed
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
    // Not retail: the cover `OP Fade` fades to black and back; above the speech banner, below the end text.
    let whole = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    commands.spawn((Scenario, Ui::Fade, whole, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)), Visibility::Hidden, GlobalZIndex(165)));
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
            let art = sprite(&assets, &hud, name).expect("briefing portrait sprite");
            let mut node = absolute(left, top, size);
            art.fit(Vec2::splat(size)).place(&mut node, 1.0, UiRect::DEFAULT);
            strip.spawn((Portrait, art.image_node(), Visibility::Hidden, node));
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

/// Not retail: a skirmish map script's speech lines (`maps/<id>/media.json`), the gameplay subtitle above the main bar, in the speech event's
/// own colour. The cutscene banner and the rest of the mission widgets stay with the campaign.
fn build_skirmish_speech(mut commands: Commands, existing: Query<(), With<Ui>>) {
    if !existing.is_empty() {
        return;
    }
    let node = Node { position_type: PositionType::Absolute, bottom: Val::Percent(9.5), left: Val::Percent(20.0), right: Val::Percent(20.0), ..default() };
    commands.spawn((Scenario, Ui::Speech, text(20.0, SPEECH_GOLD), Visibility::Hidden, GlobalZIndex(160), node));
}

fn skirmish_subtitle(sk: Res<crate::rules::skirmish::SkirmishScript>, mut texts: Query<(&Ui, &mut Visibility, &mut Text, &mut TextColor)>) {
    let colour = sk.script.speech_colour().map_or(SPEECH_GOLD, |[r, g, b, a]| Color::srgba(r, g, b, a));
    for (ui, mut vis, mut t, mut c) in &mut texts {
        if !matches!(ui, Ui::Speech) {
            continue;
        }
        vis.set_if_neq(if sk.script.subtitle.is_empty() { Visibility::Hidden } else { Visibility::Inherited });
        if **t != sk.script.subtitle {
            **t = sk.script.subtitle.clone();
        }
        c.set_if_neq(TextColor(colour));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Script;
    use serde_json::json;

    /// Not retail: `OP Fade` darkens the cover to black at the middle of its time and clears it at the end.
    #[test]
    fn the_cover_is_black_at_the_middle_of_a_fade_and_clear_again_at_its_end() {
        let data = json!({"rules": [{"name": "fade", "Run Once": true, "conditions": [{"Type": "World Initialize"}], "actions": [{"Type": "OP Fade", "Seconds": 4.0}]}]});
        let mission = Mission { id: "synthetic".into(), script: Script::new(&data, json!({})), data, opening: None };
        let mut app = App::new();
        app.insert_resource(mission).add_systems(Update, fade_picture);
        let cover = app.world_mut().spawn((Ui::Fade, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)))).id();
        let alpha = |app: &App| app.world().get::<BackgroundColor>(cover).unwrap().0.alpha();
        let at = |app: &mut App, dt: f32| {
            app.world_mut().resource_mut::<Mission>().script.step(&[], dt, false);
            app.update();
            alpha(app)
        };
        assert!(at(&mut app, 0.01) < 0.01, "just started");
        assert!((at(&mut app, 1.0) - 0.5).abs() < 0.01, "a quarter of the way: half black");
        assert!((at(&mut app, 1.0) - 1.0).abs() < 0.01, "the middle: black");
        assert!((at(&mut app, 1.0) - 0.5).abs() < 0.01, "on the way back");
        assert_eq!(at(&mut app, 1.0), 0.0, "over: the picture is clear and nothing keeps the cover up");
    }
}
