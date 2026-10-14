//! Mission one's in-engine opening; retail paths/voices, provisional camera interpolation and portrait expression.
use crate::{
    Scenario,
    campaign::{Mission, MissionShip, ObjectiveText},
    game_to_bevy,
    sprites::{HudData, inset, sprite_of},
};
use bevy::prelude::*;
use serde_json::Value;

pub struct Opening {
    pub elapsed: f32,
    speech: Option<usize>,
}
impl Default for Opening {
    fn default() -> Self {
        Self { elapsed: 0.0, speech: None }
    }
}
#[derive(Component)]
pub struct BriefingUi;
#[derive(Component)]
pub struct Subtitle;
#[derive(Component)]
pub struct Portrait;
#[derive(Component)]
pub struct Voice;

pub fn active(mission: Option<&Mission>) -> bool {
    mission.is_some_and(|m| m.opening.is_some())
}

// Opening NIS 2 queues the speech at world timer >10. Audio uses real time, independent of --speed.
fn cue_at(elapsed: f32, cues: &[Value]) -> Option<(usize, f32)> {
    let mut start = 10.0;
    for (i, cue) in cues.iter().enumerate() {
        let end = start + cue["duration"].as_f64().expect("speech duration") as f32;
        if elapsed >= start && elapsed < end {
            return Some((i, elapsed - start));
        }
        start = end;
    }
    None
}

/// Start time of the first cue (or the end of the last) after `elapsed`: where Space (Skip Speech Event) jumps to.
fn next_cue_start(elapsed: f32, cues: &[Value]) -> f32 {
    let mut t = 10.0;
    for cue in cues {
        if t > elapsed {
            break;
        }
        t += cue["duration"].as_f64().expect("speech duration") as f32;
    }
    t
}

/// PROVISIONAL: piecewise-linear path sampled by distance; native spline and acceleration remain unverified.
fn path_position(points: &[Value], mut distance: f32) -> Vec3 {
    assert!(!points.is_empty());
    for pair in points.windows(2) {
        let (a, b) = (game_to_bevy(&pair[0]), game_to_bevy(&pair[1]));
        let len = a.distance(b);
        if len > 0.0 && distance <= len {
            return a.lerp(b, (distance / len).clamp(0.0, 1.0));
        }
        distance -= len;
    }
    game_to_bevy(points.last().unwrap())
}

pub fn update(
    mut commands: Commands,
    mut mission: Option<ResMut<Mission>>,
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    assets: Res<AssetServer>,
    hud: Res<HudData>,
    cfg: Res<crate::Config>,
    mut camera: Single<&mut Transform, With<Camera3d>>,
    mut ships: Query<(&mut Transform, &MissionShip, &mut crate::Ship, Has<crate::PlayerShip>), Without<Camera3d>>,
    ui: Query<Entity, With<BriefingUi>>,
    voices: Query<Entity, With<Voice>>,
    binds: Res<crate::bindings::Bindings>,
    mut subtitles: Query<&mut Text, With<Subtitle>>,
    mut portraits: Query<&mut Visibility, With<Portrait>>,
    mut objectives: Query<&mut Visibility, (With<ObjectiveText>, Without<Portrait>)>,
) {
    let Some(ref mut mission) = mission else { return };
    let mission = &mut **mission;
    let Some(mut opening) = mission.opening.take() else { return };
    opening.elapsed += time.delta_secs();
    let cues = mission.data["opening_speech"].as_array().expect("opening speech export");
    let end = 10.0 + cues.iter().map(|c| c["duration"].as_f64().unwrap() as f32).sum::<f32>();
    if binds.just_pressed(&keys, crate::bindings::Act::SkipSpeech) {
        opening.elapsed = next_cue_start(opening.elapsed, cues);
    }
    let skip =
        cfg.script.as_deref().is_some_and(|s| matches!(s, "briefing-skip" | "mission" | "asteroid") || s.starts_with("dock") || s.starts_with("lifeboat")) && cfg.frame == 60;
    if skip || opening.elapsed >= end {
        for e in ui.iter().chain(voices.iter()) {
            commands.entity(e).despawn();
        }
        // Skipping must leave the player where the arrival path ends.
        for (mut tf, _, mut sh, primary) in &mut ships {
            if let (true, Some(&end)) = (primary, sh.waypoints.last()) {
                tf.translation = end;
                sh.waypoints.clear();
                sh.speed = 0.0;
                sh.order = 0;
            }
        }
        // Rules 'END Opening NIS' (music, command-ship focus at 60 / 10 degrees, throttle) run in rules.rs once these are marked played.
        mission.script.played.extend(cues.iter().map(|c| c["name"].as_str().unwrap().to_string()));
        for mut vis in &mut objectives {
            *vis = Visibility::Inherited;
        }
        info!("Mission 1 briefing complete{}", if skip { " (skipped)" } else { "" });
        return;
    }
    for mut vis in &mut objectives {
        *vis = Visibility::Hidden;
    }
    // The arrival path ends in a halt; without this the command ship coasts on at full order into the fleet before the NIS ends.
    for (_, _, mut sh, primary) in &mut ships {
        if primary && sh.waypoints.is_empty() {
            sh.order = 0;
        }
    }
    if ui.is_empty() {
        // Layout measured from the reference video, not decoded native NIS window placement.
        commands
            .spawn((
                Scenario,
                BriefingUi,
                GlobalZIndex(150),
                Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(14.0), ..default() },
                BackgroundColor(Color::BLACK),
            ))
            .with_children(|parent| {
                for (name, pos, size) in
                    [("TalkingHead_Character_Amelia_Happy", Vec2::new(26.0, 21.0), Vec2::splat(64.0)), ("TalkingHead_FloatingPortal_Open", Vec2::new(6.0, 1.0), Vec2::splat(104.0))]
                {
                    let (sheet, rect) = sprite_of(&hud, name).expect("briefing portrait sprite");
                    let mut image = ImageNode::new(assets.load(format!("hud/tex/{sheet}.png")));
                    image.rect = Some(inset(&sheet, rect));
                    parent.spawn((
                        Portrait,
                        image,
                        Visibility::Hidden,
                        Node { position_type: PositionType::Absolute, left: Val::Px(pos.x), top: Val::Px(pos.y), width: Val::Px(size.x), height: Val::Px(size.y), ..default() },
                    ));
                }
                parent.spawn((
                    Subtitle,
                    Text::new(""),
                    TextFont { font_size: FontSize::Px(20.0), ..default() },
                    TextColor(Color::srgb(1.0, 0.65, 0.1)),
                    Node { position_type: PositionType::Absolute, left: Val::Px(120.0), right: Val::Px(30.0), top: Val::Px(12.0), ..default() },
                ));
            });
        commands
            .spawn((
                Scenario,
                BriefingUi,
                GlobalZIndex(150),
                Node { position_type: PositionType::Absolute, bottom: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(14.0), ..default() },
                BackgroundColor(Color::BLACK),
            ))
            .with_children(|parent| {
                parent.spawn((
                    Text::new("Space: next line"),
                    TextFont { font_size: FontSize::Px(16.0), ..default() },
                    Node { position_type: PositionType::Absolute, right: Val::Px(20.0), bottom: Val::Px(12.0), ..default() },
                ));
            });
    }
    let cue = cue_at(opening.elapsed, cues);
    let index = cue.map(|c| c.0);
    if opening.speech != index {
        for e in &voices {
            commands.entity(e).despawn();
        }
        if let Some(i) = opening.speech {
            mission.script.played.insert(cues[i]["name"].as_str().unwrap().to_string());
        }
        if let Some(i) = index {
            commands.spawn((
                Scenario,
                Voice,
                AudioPlayer::new(assets.load(crate::audio::sound(cues[i]["name"].as_str().unwrap()))),
                PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(crate::menu::volume(crate::menu::Setting::Dialogue))),
            ));
            info!("Briefing speech {}", cues[i]["name"]);
        }
        opening.speech = index;
    }
    for mut text in &mut subtitles {
        **text = cue.map_or("", |(i, _)| cues[i]["text"].as_str().unwrap()).into();
    }
    for mut vis in &mut portraits {
        *vis = if cue.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    }
    let ship_pos = |id| ships.iter().find(|(_, s, ..)| s.id == id).expect("briefing ship").0.translation + Vec3::Y * 4.0;
    let (path, distance, focus) = match cue {
        None => ("Opening NIS", opening.elapsed * 115.0, game_to_bevy(&mission.data["landmarks"]["Montressor Archipelago"])),
        Some((0 | 1, _)) => ("Opening NIS", 10.0 * 115.0 + (opening.elapsed - 10.0) * 65.0, ship_pos(222)),
        // The rule says 'Opneing NIS 2c', the actual path is 'Opneing NIS 2c' (retail typo preserved).
        Some((2, local)) => ("Opneing NIS 2c", local * 125.0, (ship_pos(227) + ship_pos(228) + ship_pos(229)) / 3.0),
        Some((3, local)) => ("Opening NIS 2d", local * 70.0, if local < 3.0 { ship_pos(222) } else { ship_pos(231) }),
        _ => unreachable!(),
    };
    let points = mission.data["paths"][path].as_array().expect("briefing camera path");
    let desired = path_position(points, distance);
    // PROVISIONAL: soften focus/path switches; native transition controller is not decoded.
    camera.translation = if opening.elapsed <= time.delta_secs() { desired } else { camera.translation.lerp(desired, (time.delta_secs() * 3.0).min(1.0)) };
    if camera.translation.distance_squared(focus) > 0.01 {
        camera.look_at(focus, Vec3::Y);
    }
    mission.opening = Some(opening);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cue_boundaries_and_path_endpoints() {
        let cues = vec![serde_json::json!({"duration":2}), serde_json::json!({"duration":3})];
        assert_eq!(cue_at(9.9, &cues), None);
        assert_eq!(cue_at(10.0, &cues), Some((0, 0.0)));
        assert_eq!(cue_at(12.0, &cues), Some((1, 0.0)));
        assert_eq!(cue_at(15.0, &cues), None);
        assert_eq!((next_cue_start(0.0, &cues), next_cue_start(10.5, &cues), next_cue_start(12.5, &cues), next_cue_start(15.0, &cues)), (10.0, 12.0, 15.0, 15.0));
        let path = vec![serde_json::json!([0, 0, 0]), serde_json::json!([0, 0, 0]), serde_json::json!([10, 0, 0])];
        assert_eq!(path_position(&path, 0.0), Vec3::ZERO);
        assert_eq!(path_position(&path, 5.0), Vec3::new(5.0, 0.0, 0.0));
        assert_eq!(path_position(&path, 100.0), Vec3::new(10.0, 0.0, 0.0));
        let m = crate::campaign::load();
        for name in ["Opening NIS", "Opneing NIS 2c", "Opening NIS 2d"] {
            assert!(path_position(m.data["paths"][name].as_array().unwrap(), 0.0).is_finite());
        }
        assert_eq!(m.data["opening_speech"].as_array().unwrap().len(), 4);
    }
}
