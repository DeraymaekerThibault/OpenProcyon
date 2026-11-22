//! Mission presentation: speech and HUD widgets, event effects, music and the NIS camera.

use super::*;

/// Mission UI widgets a view spawns (`classic/mission_ui.rs`); `ui` sets their text, visibility and portrait art.
#[derive(Component)]
pub enum Ui {
    Speech,
    SpeechTop,
    SpeechPanel,
    SpeechFrame,
    SpeechHead,
    Hint,
    End,
    BarTop,
    BarBottom,
}

/// A looping effect created by 'Create/Release Event Effect', keyed by point set + effect name.
#[derive(Component)]
pub(in crate::rules) struct EventFx(String);

/// 'Create/Release Event Effect': removes the effect's old copies and, when `on`, plays it at every point of the set.
pub(in crate::rules) fn event_effect<'a>(
    commands: &mut Commands,
    existing: impl Iterator<Item = (Entity, &'a EventFx)>,
    lib: Option<&crate::fx::FxLib>,
    points: &serde_json::Value,
    set: &str,
    name: &str,
    on: bool,
) {
    let key = format!("{set}/{name}");
    for (e, f) in existing {
        if f.0 == key {
            commands.entity(e).despawn();
        }
    }
    if let Some(lib) = lib.filter(|_| on) {
        for p in points[set].as_array().into_iter().flatten() {
            let at = Transform::from_translation(game_to_bevy(&p["pos"]));
            if let Some(e) = crate::fx::spawn(commands, lib, name, None, at, at, false) {
                commands.entity(e).insert((EventFx(key.clone()), Scenario));
            }
        }
    }
}

#[derive(Component)]
pub(super) struct MusicFade {
    cur: f32,
    to: f32,
    rate: f32,
    kill: bool,
}

/// Portrait sprite for a speech line by speaker prefix. PROVISIONAL: one fixed expression per speaker.
fn speaker_portrait(line: &str) -> Option<&'static str> {
    let who = line.strip_prefix("M01").unwrap_or(line);
    [("AMELIA", "Amelia_Happy"), ("JIM", "Jim_Stoic"), ("ONYX", "Onyx_Stoic")].into_iter().find(|(p, _)| who.starts_with(p)).map(|x| x.1)
}

pub(super) fn ui(
    mut talk: ResMut<crate::panels::Talk>,
    assets: Res<AssetServer>,
    hud: Res<crate::sprites::HudData>,
    mut images: Query<(&Ui, &mut ImageNode, &mut Node)>,
    mission: Res<Mission>,
    campaign_points: Res<crate::campaign::CampaignPoints>,
    stats: Option<Res<crate::stats::GameStats>>,
    mut q: Query<(&Ui, &mut Visibility, Option<&mut Text>), Without<ObjectiveText>>,
    mut objective: Query<&mut Text, With<ObjectiveText>>,
) {
    let sc = &mission.script;
    let opening = mission.opening.is_some();
    let speaking = !opening && !sc.subtitle.is_empty();
    let cutscene = speaking && sc.nis;
    let portrait = sc.face().map(String::from).or_else(|| sc.speaking.as_ref().and_then(|s| speaker_portrait(&s.0)).map(|p| format!("TalkingHead_Character_{p}")));
    // The bottom-right talking head (panels.rs) takes over gameplay speech; cutscenes use the top banner.
    talk.0 = (speaking && !sc.nis).then(|| portrait.clone()).flatten();
    for (kind, mut image, mut node) in &mut images {
        let name = match kind {
            Ui::SpeechHead => portrait.clone(),
            Ui::SpeechFrame => Some("TalkingHead_FloatingPortal_Open".to_string()),
            _ => continue,
        };
        // Only on a change: writing the node every frame would re-render it every frame. The banner is laid out in screen px.
        if let Some(sprite) = name.and_then(|n| crate::sprites::sprite(&assets, &hud, &n)) {
            sprite.show(&mut image, &mut node, 1.0);
        }
    }
    for (kind, mut vis, text) in &mut q {
        let (show, content): (bool, String) = match kind {
            Ui::Speech => (speaking && !sc.nis, sc.subtitle.clone()),
            Ui::SpeechTop => (cutscene, sc.subtitle.clone()),
            Ui::SpeechPanel => (cutscene, String::new()),
            Ui::SpeechFrame | Ui::SpeechHead => (cutscene && portrait.is_some(), String::new()),
            Ui::Hint => (!crate::panels::cinematic(Some(&mission)) && !super::console::hud_hidden() && !sc.hint.is_empty(), sc.hint.clone()),
            Ui::End => (
                sc.ended.is_some(),
                sc.ended
                    .as_ref()
                    .map(|e| {
                        format!(
                            "{}\n\n{}\nVictory points earned: {}\nCampaign total: {}\n{}R restart   F10 menu",
                            e.0,
                            stats.as_ref().map(|s| s.table()).unwrap_or_default(),
                            sc.score,
                            campaign_points.total,
                            if e.1 && crate::campaign::next(&mission.id).is_some() { "N next mission   " } else { "" }
                        )
                    })
                    .unwrap_or_default(),
            ),
            Ui::BarTop | Ui::BarBottom => (sc.nis && !opening, String::new()),
        };
        vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
        if let Some(mut t) = text
            && **t != content
        {
            **t = content;
        }
    }
    let tasks = if crate::panels::cinematic(Some(&mission)) || super::console::hud_hidden() { String::new() } else { sc.tasks_text() };
    for mut t in &mut objective {
        if **t != tasks {
            **t = tasks.clone();
        }
    }
}

pub(super) fn music(
    mut commands: Commands,
    assets: Res<AssetServer>,
    time: Res<Time>,
    mission: Option<ResMut<Mission>>,
    mut playing: Query<(Entity, &mut MusicFade, Option<&mut AudioSink>), With<crate::audio::Music>>,
) {
    let Some(mut mission) = mission else { return };
    let dt = time.delta_secs();
    for c in std::mem::take(&mut mission.script.music) {
        for (e, mut fade, _) in &mut playing {
            if c.fade_out <= 0.0 {
                commands.entity(e).try_despawn();
            } else {
                fade.to = 0.0;
                fade.rate = fade.cur.max(0.05) / c.fade_out;
                fade.kill = true;
            }
        }
        if c.file == "None" {
            continue;
        }
        let vol = c.vol * 0.5; // PROVISIONAL scale of the script's 0..1 volume (the skirmish uses 0.35)
        let start = if c.fade_in > 0.0 { 0.0 } else { vol };
        let looped = !c.file.starts_with("TRANS_");
        let settings = if looped { PlaybackSettings::LOOP } else { PlaybackSettings::DESPAWN };
        commands.spawn((
            Scenario,
            crate::audio::Music,
            MusicFade { cur: start, to: vol, rate: if c.fade_in > 0.0 { vol / c.fade_in } else { 1.0 }, kill: false },
            AudioPlayer::new(assets.load(crate::audio::sound(&c.file))),
            settings.with_volume(Volume::Linear(start * crate::menu::volume(crate::menu::Setting::Music))),
        ));
    }
    for (e, mut fade, sink) in &mut playing {
        let step = fade.rate * dt;
        fade.cur = if fade.cur < fade.to { (fade.cur + step).min(fade.to) } else { (fade.cur - step).max(fade.to) };
        if let Some(mut sink) = sink {
            sink.set_volume(Volume::Linear(fade.cur * crate::menu::volume(crate::menu::Setting::Music)));
        }
        if fade.kill && fade.cur <= 0.001 {
            commands.entity(e).despawn();
        }
    }
}

/// Script presentation switches: 'NIS Zoom' field of view, the star map's open state for the rules and the tutorial's
/// 'Pause when starmap opens'. PROVISIONAL: zoom eases linearly at the action's Speed (radians per second).
pub(super) fn presentation(
    mut mission: ResMut<Mission>,
    real: Res<Time<Real>>,
    mut vtime: ResMut<Time<Virtual>>,
    map: Query<(), With<crate::starmap::Starmap>>,
    mut proj: Query<&mut Projection, With<Camera3d>>,
    mut paused_by_map: Local<bool>,
) {
    const DEFAULT_FOV: f32 = std::f32::consts::FRAC_PI_4;
    let sc = &mut mission.script;
    sc.starmap_open = !map.is_empty();
    if sc.starmap_open && sc.starmap_pause && !vtime.is_paused() {
        vtime.pause();
        *paused_by_map = true;
    } else if !sc.starmap_open && *paused_by_map {
        vtime.unpause();
        *paused_by_map = false;
    }
    let (want, rate) = sc.zoom.filter(|_| sc.nis).unwrap_or((DEFAULT_FOV, 0.5));
    for mut p in &mut proj {
        if let Projection::Perspective(pp) = &mut *p {
            let step = rate * real.delta_secs();
            if (pp.fov - want).abs() > 1e-4 {
                pp.fov = if pp.fov < want { (pp.fov + step).min(want) } else { (pp.fov - step).max(want) };
            }
        }
    }
}

/// Camera during a script NIS: attached to a unit, flying a path, or orbiting the focus; smoothed (PROVISIONAL, native controller not decoded).
pub(super) fn nis_camera(
    mission: Res<Mission>,
    time: Res<Time>,
    orbit: Res<Orbit>,
    units: Query<(&Transform, Option<&Ship>), Without<Camera3d>>,
    mut cam: Single<&mut Transform, With<Camera3d>>,
    mut travelled: Local<f32>,
) {
    let sc = &mission.script;
    if !sc.nis || mission.opening.is_some() {
        *travelled = 0.0;
        return;
    }
    let dt = time.delta_secs();
    // Script::step resolves the camera's unit specs (ships, whales, bases) to entities.
    let unit = |e: Option<Entity>| units.get(e?).ok().map(|(t, s)| (t.translation, s.map_or(0.0, |s| s.heading)));
    let c = &sc.cam;
    let (want, look) = if let Some((_, d, yz, xy)) = &c.attach {
        let Some((p, hd)) = unit(c.attach_e) else { return };
        let off = Quat::from_rotation_y(hd + xy.to_radians()) * Quat::from_rotation_x(-yz.to_radians()) * Vec3::new(0.0, 0.0, *d);
        (p + Vec3::Y * 3.0 + off, p + Vec3::Y * 3.0)
    } else {
        // 'NIS Focus camera on Point' names a point set (its first point), as Teleport does.
        let focus = unit(c.focus_e).map(|u| u.0).or_else(|| Some(game_to_bevy(&sc.point_sets[c.focus_point.as_deref()?].get(0)?["pos"])));
        let Some(focus) = focus else { return };
        let pos = match &c.path {
            Some((name, _, vel)) => {
                *travelled += vel * dt;
                let pts = path_points(&sc.paths, name);
                let mut left = *travelled;
                let mut at = pts.first().copied().unwrap_or(cam.translation);
                for pair in pts.windows(2) {
                    let len = pair[0].distance(pair[1]);
                    if left <= len {
                        at = pair[0].lerp(pair[1], left / len.max(0.001));
                        break;
                    }
                    left -= len;
                    at = pair[1];
                }
                at
            }
            None => focus + Quat::from_rotation_y(orbit.yaw) * Quat::from_rotation_x(-orbit.pitch) * Vec3::new(0.0, 0.0, orbit.dist.max(45.0)),
        };
        (pos, focus)
    };
    cam.translation = cam.translation.lerp(want, 1.0 - (-2.5 * dt).exp());
    if cam.translation.distance_squared(look) > 0.01 {
        cam.look_at(look, Vec3::Y);
    }
}
