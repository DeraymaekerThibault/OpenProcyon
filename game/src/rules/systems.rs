//! ECS side of the rule interpreter: dispatch of `Script` effects, mission AI, mission UI, music and NIS camera.
use bevy::audio::Volume;
use bevy::prelude::*;
use serde_json::Value;

use super::*;
use crate::campaign::{Mission, MissionBase, MissionShip, ObjectiveText};
use crate::combat::{Asteroid, Destroyed, Fired, Health, Hit, Library, Target};
use crate::docking::DockMsg;
use crate::{Config, Orbit, PlayerShip, Scenario, Ship, Side, game_to_bevy};

// ---------------------------------------------------------------------------------------------------------------- ECS side

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
/// PROCYON_END_SHOT and PROCYON_NIS_SHOT, read once instead of every frame.
fn dev_env() -> &'static (Option<std::ffi::OsString>, Option<std::ffi::OsString>) {
    static ENV: std::sync::OnceLock<(Option<std::ffi::OsString>, Option<std::ffi::OsString>)> = std::sync::OnceLock::new();
    ENV.get_or_init(|| (std::env::var_os("PROCYON_END_SHOT"), std::env::var_os("PROCYON_NIS_SHOT")))
}
/// A looping effect created by 'Create/Release Event Effect', keyed by point set + effect name.
#[derive(Component)]
struct EventFx(String);
/// Parameters of `update` beyond Bevy's 16-argument limit.
#[derive(bevy::ecs::system::SystemParam)]
struct Extra<'w, 's> {
    bases: Query<
        'w,
        's,
        (Entity, &'static mut MissionBase, &'static Transform, Option<(&'static crate::combat::ArmedBase, &'static Health)>),
        (Without<Ship>, Without<crate::environment::SpaceAnimal>),
    >,
    dock: MessageWriter<'w, DockMsg>,
    board: MessageWriter<'w, crate::boarding::BoardMsg>,
    captured: MessageReader<'w, 's, crate::boarding::Captured>,
    asteroids: MessageWriter<'w, crate::combat::AsteroidSetup>,
    meteors: MessageWriter<'w, crate::nebula::MeteorRoute>,
    storm_shift: Res<'w, crate::nebula::StormShift>,
    campaign_points: ResMut<'w, crate::campaign::CampaignPoints>,
    binds: Res<'w, crate::bindings::Bindings>,
    sensors: Res<'w, crate::environment::Sensors>,
    voices: Query<'w, 's, Entity, With<crate::briefing::Voice>>,
    fxlib: Option<Res<'w, crate::fx::FxLib>>,
    event_fx: Query<'w, 's, (Entity, &'static EventFx)>,
    sfx: Option<ResMut<'w, Messages<crate::audio::Sfx>>>,
    toggles: Option<ResMut<'w, crate::panels::Toggles>>,
    detonate: MessageWriter<'w, crate::combat::Detonate>,
    dragons: Query<'w, 's, &'static mut crate::dragons::Dragon>,
    dev: Local<'s, Dev>,
}
/// State of the PROCYON_*_SHOT dev hooks.
#[derive(Default)]
struct Dev {
    nis_age: f32,
    shot_taken: bool,
    end_age: f32,
    end_shot: bool,
    skipped: bool,
}
#[derive(Component)]
struct MusicFade {
    cur: f32,
    to: f32,
    rate: f32,
    kill: bool,
}

pub fn spawn_ui(commands: &mut Commands) {
    let text = |size: f32| (Text::new(""), TextFont { font_size: FontSize::Px(size), ..default() }, Visibility::Hidden);
    // Cutscene speech banner as in the opening briefing: black strip at the top with the speaker's portrait and the line.
    commands.spawn((
        Scenario,
        Ui::SpeechPanel,
        Visibility::Hidden,
        BackgroundColor(Color::BLACK),
        GlobalZIndex(150),
        Node { position_type: PositionType::Absolute, top: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(14.0), ..default() },
    ));
    for (kind, left, top, size, z) in [(Ui::SpeechFrame, 6.0, 1.0, 104.0, 155), (Ui::SpeechHead, 26.0, 21.0, 64.0, 156)] {
        commands.spawn((
            Scenario,
            kind,
            ImageNode::default(),
            Visibility::Hidden,
            GlobalZIndex(z),
            Node { position_type: PositionType::Absolute, left: Val::Px(left), top: Val::Px(top), width: Val::Px(size), height: Val::Px(size), ..default() },
        ));
    }
    commands.spawn((
        Scenario,
        Ui::SpeechTop,
        text(20.0),
        TextColor(Color::srgb(1.0, 0.65, 0.1)),
        GlobalZIndex(160),
        Node { position_type: PositionType::Absolute, top: Val::Px(12.0), left: Val::Px(120.0), right: Val::Px(30.0), ..default() },
    ));
    // Gameplay speech subtitle sits just above the main bar (bar is ~7.7% of the window height; PROVISIONAL margin).
    commands.spawn((
        Scenario,
        Ui::Speech,
        crate::campaign::Subtitle,
        text(20.0),
        TextColor(Color::srgb(1.0, 0.75, 0.2)),
        GlobalZIndex(160),
        Node { position_type: PositionType::Absolute, bottom: Val::Percent(9.5), left: Val::Percent(20.0), right: Val::Percent(20.0), ..default() },
    ));
    commands.spawn((
        Scenario,
        Ui::Hint,
        crate::campaign::HintText,
        text(18.0),
        TextColor(Color::srgb(0.7, 0.9, 1.0)),
        GlobalZIndex(160),
        Node { position_type: PositionType::Absolute, top: Val::Percent(30.0), left: Val::Percent(30.0), right: Val::Percent(30.0), ..default() },
    ));
    commands.spawn((
        Scenario,
        Ui::End,
        crate::campaign::EndText,
        text(34.0),
        TextColor(Color::WHITE),
        GlobalZIndex(170),
        Node { position_type: PositionType::Absolute, top: Val::Percent(35.0), left: Val::Percent(15.0), right: Val::Percent(15.0), ..default() },
    ));
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

pub fn plugin(app: &mut App) {
    use crate::menu::Screen;
    app.insert_resource(crate::campaign::CampaignPoints::load()).add_systems(
        Update,
        (
            update.after(crate::briefing::update).before(crate::input),
            mission_ai.after(update).before(crate::ship::fleet_follow),
            leash_player.after(update).before(crate::sail),
            presentation.after(nis_camera),
            (ui, music, nis_camera.after(crate::camera)).after(update),
        )
            .run_if(in_state(Screen::Battle))
            .run_if(resource_exists::<Mission>),
    );
}

fn units(ships: &Query<(Entity, &mut Ship, &mut Transform, &mut Health, &mut MissionShip, &mut Visibility, Has<PlayerShip>)>, lib: &Library) -> Vec<Unit> {
    ships
        .iter()
        .map(|(e, ship, tf, h, m, _, primary)| {
            let class = &lib.classes[ship.class];
            let secs =
                if h.sections.is_empty() { 0.0 } else { h.sections.iter().zip(&class.sections).map(|(x, c)| 1.0 - x / c.hp.max(1.0)).sum::<f32>() / h.sections.len() as f32 };
            Unit {
                e,
                id: m.id,
                name: m.name.clone(),
                owner: m.owner.clone(),
                owner_is_named_group: false,
                groups: vec![],
                pos: tf.translation,
                alive: !h.destroyed,
                visible: m.visible,
                primary,
                damage: (1.0 - h.core / class.core_hp).max(secs).clamp(0.0, 1.0),
                flag: m.flag.clone(),
                attack: m.attack.clone(),
            }
        })
        .collect()
}

fn update(
    mission: Option<ResMut<Mission>>,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    cfg: Res<Config>,
    lib: Res<Library>,
    mut orbit: ResMut<Orbit>,
    mut target: ResMut<Target>,
    mut hits: MessageReader<Hit>,
    mut fired: MessageReader<Fired>,
    mut killed: MessageReader<Destroyed>,
    rocks: Query<(Entity, &Transform, &Asteroid), (Without<Ship>, Without<crate::environment::SpaceAnimal>)>,
    mut animals: Query<(Entity, &mut crate::environment::SpaceAnimal, &mut Transform, &mut Visibility), Without<Ship>>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut ships: Query<(Entity, &mut Ship, &mut Transform, &mut Health, &mut MissionShip, &mut Visibility, Has<PlayerShip>)>,
    mut extra: Extra,
) {
    let Some(mut mission) = mission else { return };
    let mission = &mut *mission;
    // Tutorials run the same interpreter but do not count toward the campaign.
    if mission.id.starts_with("mission") {
        extra.campaign_points.record(&mission.id, mission.script.score);
    }
    let opening = mission.opening.is_some();
    let dt = time.delta_secs();
    // Dev hook `--script dock|dock-click|dock-menu`: jump to the tender (Script::skip_to_tender) and put the player (half damaged, to
    // see the repair) and the small ships inside the tender volume. `dock` then docks like the D key; the others click (options_menu.rs).
    if cfg.script.as_deref().is_some_and(|s| s.starts_with("dock")) && !opening && !extra.dev.skipped {
        extra.dev.skipped = true;
        mission.script.skip_to_tender();
        let tender = ships.iter().find(|s| s.4.name == "RLS Bounty").map(|s| s.2.translation).expect("mission 1 tender");
        let mut k = 0.0;
        for (_, mut sh, mut tf, mut h, ms, _, primary) in &mut ships {
            if primary {
                tf.translation = tender + Vec3::new(-100.0, 0.0, 0.0);
                h.core *= 0.5;
                h.sections.iter_mut().for_each(|x| *x *= 0.5);
            } else if ms.owner == "Small Ships (Red Team)" {
                k += 1.0;
                tf.translation = tender + Vec3::new(-170.0, 0.0, 45.0 * k - 110.0);
            } else {
                continue;
            }
            sh.waypoints.clear();
            sh.order = 0;
            sh.speed = 0.0;
        }
    }
    let mut w = units(&ships, &lib);
    w.extend(animals.iter().map(|(e, a, tf, _)| a.unit(e, tf)));
    w.extend(extra.bases.iter_mut().map(|(e, b, tf, armed)| b.unit(e, tf, armed.map_or(0.0, |(a, h)| crate::combat::base_mission_damage(&lib.classes[a.class], h)))));
    w.extend(rocks.iter().map(|(e, tf, a)| a.unit(e, tf)));
    mission.script.tag(&mut w);
    let owner_of = |e: Entity| w.iter().find(|u| u.e == e).map(|u| u.owner.clone());
    for h in hits.read() {
        mission.script.record_hit(owner_of(h.shooter).as_deref(), h.victim);
    }
    for fr in fired.read() {
        mission.script.record_fire(fr.shooter, fr.target);
    }
    for c in extra.captured.read() {
        mission.script.record_capture(&c.by, &c.victim, c.unit, extra.bases.contains(c.unit));
    }
    let sc = &mut mission.script;
    for u in w.iter().filter(|u| rocks.contains(u.e)) {
        sc.belt.entry(u.e).or_insert_with(|| std::iter::once(u.owner.clone()).chain(u.groups.iter().cloned()).collect());
    }
    for d in killed.read() {
        if let Some(o) = owner_of(d.by) {
            *sc.kills.entry(o).or_default() += 1;
        }
    }
    // Space (Skip Speech Event) ends the current line in every cutscene/gameplay speech, as in the opening NIS.
    if !opening && extra.binds.just_pressed(&keys, crate::bindings::Act::SkipSpeech) && sc.skip_speech() {
        for e in &extra.voices {
            commands.entity(e).despawn();
        }
    }
    sc.nebulae = crate::nebula::polygons(&mission.data, &extra.storm_shift);
    let (fxs, voice) = sc.step(&w, dt, opening);
    if let Some(file) = voice {
        commands.spawn((
            Scenario,
            crate::briefing::Voice,
            AudioPlayer::new(assets.load(crate::audio::sound(&file))),
            PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(crate::menu::volume(crate::menu::Setting::Dialogue))),
        ));
    }
    let paths = &mission.data["paths"];
    for fx in fxs {
        match fx {
            Fx::Formation(owner, kind, hold) => {
                commands.queue(move |w: &mut World| crate::ship::set_formation(w, &owner, kind, hold));
            }
            Fx::Primary(units) => {
                if let Some(e) = units.first().copied() {
                    commands.queue(move |w: &mut World| {
                        crate::ship::promote(w, e, true);
                    });
                }
            }
            Fx::Asteroids { units, path, closest, mode, speed, tumble } => {
                let path = paths[path.as_str()].as_array().map(|a| a.iter().map(game_to_bevy).collect()).unwrap_or_default();
                extra.asteroids.write(crate::combat::AsteroidSetup { units, path, closest, mode, speed, tumble });
            }
            Fx::Path { units, path, closest, looping } => {
                let pts: Vec<Vec3> = paths[path.as_str()].as_array().map(|a| a.iter().map(game_to_bevy).collect()).unwrap_or_default();
                for e in &units {
                    if let Ok((_, mut animal, tf, _)) = animals.get_mut(*e) {
                        animal.follow(&pts, tf.translation, closest, looping);
                        continue;
                    }
                    let Ok((_, mut sh, tf, _, mut ms, _, _)) = ships.get_mut(*e) else { continue };
                    let full = pts.clone(); // Continuous fleet followers supply leader-relative slots, not cloned path offsets.
                    let from =
                        if closest { (0..full.len()).min_by(|a, bb| full[*a].distance(tf.translation).total_cmp(&full[*bb].distance(tf.translation))).unwrap_or(0) } else { 0 };
                    sh.throttle = None;
                    sh.waypoints = full[from..].to_vec();
                    sh.order = 2;
                    sh.goal = None;
                    ms.attack.clear();
                    ms.stop_at_end = !looping;
                    ms.looping = looping.then_some(full);
                }
            }
            Fx::MeteorPath { group, path, closest, mode } => {
                let path = paths[path.as_str()].as_array().map(|a| a.iter().map(game_to_bevy).collect()).unwrap_or_default();
                extra.meteors.write(crate::nebula::MeteorRoute::Path { group, path, closest, mode });
            }
            Fx::MeteorSpeed(group, speed) => {
                extra.meteors.write(crate::nebula::MeteorRoute::Speed { group, speed });
            }
            Fx::Velocity(u, v) => {
                for e in u {
                    if let Ok((_, mut animal, ..)) = animals.get_mut(e) {
                        animal.speed = v;
                    }
                }
            }
            Fx::Event(set, name, on) => {
                let key = format!("{set}/{name}");
                for (e, f) in &extra.event_fx {
                    if f.0 == key {
                        commands.entity(e).despawn();
                    }
                }
                if let Some(lib) = extra.fxlib.as_deref().filter(|_| on) {
                    for p in mission.script.point_sets[set.as_str()].as_array().into_iter().flatten() {
                        let at = Transform::from_translation(game_to_bevy(&p["pos"]));
                        if let Some(e) = crate::fx::spawn(&mut commands, lib, &name, None, at, at, false) {
                            commands.entity(e).insert((EventFx(key.clone()), Scenario));
                        }
                    }
                }
            }
            Fx::WeaponBar(open) => {
                if let Some(t) = extra.toggles.as_mut() {
                    t.weapons = open;
                }
            }
            Fx::Sound(name) => {
                if let Some(sfx) = extra.sfx.as_mut() {
                    sfx.write(crate::audio::Sfx::ui(&name));
                }
            }
            Fx::Throttle(u, v) => {
                for e in u {
                    if let Ok((_, mut sh, ..)) = ships.get_mut(e) {
                        sh.cap = v;
                    }
                }
            }
            Fx::Speed(u, v) => {
                for e in u {
                    if let Ok((_, mut sh, .., primary)) = ships.get_mut(e) {
                        if primary {
                            if v <= 0.0 {
                                sh.order = 0;
                                sh.waypoints.clear();
                            }
                        } else {
                            sh.cap = v;
                        }
                    }
                }
            }
            Fx::Destroy(u) => {
                for e in u {
                    if let Ok((_, _, _, mut h, _, mut vis, _)) = ships.get_mut(e) {
                        h.core = 0.0;
                        h.destroyed = true;
                        *vis = Visibility::Hidden;
                    }
                }
            }
            Fx::Teleport(u, p) => {
                for e in u {
                    if let Ok((_, mut sh, mut tf, ..)) = ships.get_mut(e) {
                        tf.translation = p;
                        sh.waypoints.clear();
                    } else if let Ok((_, mut animal, mut tf, _)) = animals.get_mut(e) {
                        tf.translation = p;
                        animal.velocity = Vec3::ZERO;
                    } else if extra.bases.contains(e) {
                        // Scenery (wrecks): move the model and its collision body together.
                        commands.queue(move |w: &mut World| {
                            let Some(old) = w.get::<Transform>(e).map(|t| t.translation) else { return };
                            let delta = (p - old).with_y(0.0);
                            if let Some(mut t) = w.get_mut::<Transform>(e) {
                                t.translation += delta;
                            }
                            if let Some(mut o) = w.get_mut::<crate::collision::Obstacle>(e) {
                                o.translate(delta);
                            }
                        });
                    }
                }
            }
            Fx::Dock(dockers, dockees) => {
                for d in dockers {
                    // docking.rs steers and stops the docker itself
                    if let Ok((.., mut ms, _, _)) = ships.get_mut(d) {
                        ms.stop_at_end = false;
                    }
                    if let Some(t) = dockees.first() {
                        extra.dock.write(DockMsg::Dock { docker: Some(d), dockee: *t });
                    }
                }
            }
            Fx::Dockable(u, on) => {
                for e in u {
                    extra.dock.write(DockMsg::Enabled(e, on));
                }
            }
            Fx::Boardable(u, on) => {
                for e in u {
                    extra.board.write(crate::boarding::BoardMsg::Boardable(e, on));
                }
            }
            Fx::Lifeboats(u, on) => {
                for e in u {
                    if let Ok(mut ship) = ships.get_mut(e) {
                        ship.4.create_lifeboats = on; // ships only: bases, rocks and animals have no flag
                    }
                }
            }
            Fx::IslandRepair(u, on) => {
                for e in u {
                    extra.dock.write(DockMsg::Repairs(e, on));
                }
            }
            Fx::DockTime(u, secs) => {
                for e in u {
                    mission.script.dock_times.insert(e, secs);
                }
            }
            Fx::Stance(u, st) => {
                for e in u {
                    if let Ok((.., mut ms, _, _)) = ships.get_mut(e) {
                        ms.stance = st;
                    }
                    if let Ok(mut d) = extra.dragons.get_mut(e) {
                        d.stance = st;
                    }
                }
            }
            Fx::Owner(u, o) => {
                for e in u {
                    if let Ok((_, mut base, ..)) = extra.bases.get_mut(e) {
                        base.owner = o.clone();
                    }
                    if let Ok((_, mut animal, ..)) = animals.get_mut(e) {
                        animal.owner = o.clone();
                    }
                    if let Ok((.., mut ms, _, _)) = ships.get_mut(e) {
                        ms.owner = o.clone();
                    }
                }
            }
            Fx::Visible(u, v) => {
                for e in u {
                    if let Ok((_, mut animal, _, mut vis)) = animals.get_mut(e) {
                        animal.visible = v;
                        *vis = if v { Visibility::Inherited } else { Visibility::Hidden };
                    }
                    if let Ok((.., mut ms, mut vis, _)) = ships.get_mut(e) {
                        ms.visible = v;
                        *vis = if v { Visibility::Inherited } else { Visibility::Hidden };
                    }
                }
            }
            Fx::Invuln(u, v) => {
                for e in u {
                    if let Ok((.., mut ms, _, _)) = ships.get_mut(e) {
                        ms.invuln = v;
                    }
                }
            }
            Fx::Flag(u, tex) => {
                for e in u {
                    if let Ok((.., mut ms, _, _)) = ships.get_mut(e) {
                        ms.flag = tex.clone();
                    }
                }
            }
            Fx::Label(u, text) => {
                for e in u {
                    commands.entity(e).insert(crate::ShipName(text.clone()));
                }
            }
            Fx::Banner(u, bn) => {
                for e in u {
                    if let Ok((.., mut ms, _, _)) = ships.get_mut(e) {
                        ms.banner = bn.clone();
                    }
                }
            }
            Fx::Ram(u, g) => {
                for e in u {
                    if let Ok((.., mut ms, _, _)) = ships.get_mut(e) {
                        ms.ram = true;
                        if !ms.attack.contains(&g) {
                            ms.attack.push(g.clone());
                        }
                    }
                }
            }
            Fx::Attack(u, g) => {
                for e in u.iter().copied() {
                    if let Ok(mut d) = extra.dragons.get_mut(e)
                        && !d.attack.contains(&g)
                    {
                        d.attack.push(g.clone());
                    }
                }
                for e in u {
                    if let Ok((.., mut ms, _, _)) = ships.get_mut(e)
                        && !ms.attack.contains(&g)
                    {
                        ms.attack.push(g.clone());
                    }
                }
            }
            // Only script-given orders are cleared: the human's own course and throttle stay.
            Fx::Clear(u) => {
                for e in u.iter().copied() {
                    if let Ok(mut d) = extra.dragons.get_mut(e) {
                        d.attack.clear();
                        d.hold = false;
                    }
                }
                for e in u {
                    if let Ok((_, mut sh, .., mut ms, _, primary)) = ships.get_mut(e) {
                        ms.attack.clear();
                        ms.ram = false;
                        ms.looping = None;
                        if !primary {
                            sh.waypoints.clear();
                            sh.order = 0;
                            sh.goal = None;
                        }
                    }
                }
            }
            Fx::Damage(u, p) => {
                for e in u {
                    if let Ok((_, _, _, mut h, ..)) = ships.get_mut(e) {
                        for x in h.sections.iter_mut() {
                            *x *= 1.0 - p;
                        }
                        h.core *= 1.0 - p;
                    }
                }
            }
            Fx::Hold(u, on) => {
                for e in u.iter().copied() {
                    if let Ok(mut d) = extra.dragons.get_mut(e) {
                        d.hold = on;
                    }
                }
                for e in u.into_iter().filter(|_| on) {
                    if let Ok((_, mut sh, ..)) = ships.get_mut(e) {
                        sh.waypoints.clear();
                        sh.order = 0;
                    }
                }
            }
            Fx::ResetHits(u) => {
                for e in u {
                    mission.script.hits.remove(&e);
                    mission.script.hits_by.retain(|k, _| k.1 != e);
                }
            }
            Fx::Border(u, zone) => {
                for e in u {
                    if let Ok((.., mut ms, _, _)) = ships.get_mut(e) {
                        ms.border = zone.clone();
                    }
                    if let Ok(mut d) = extra.dragons.get_mut(e) {
                        d.border = zone.clone();
                    }
                }
            }
            Fx::ReturnPoint(u, p) => {
                for e in u {
                    if let Ok((.., mut ms, _, _)) = ships.get_mut(e) {
                        ms.return_point = Some(p);
                    }
                    if let Ok(mut d) = extra.dragons.get_mut(e) {
                        d.home = Some(p);
                    }
                }
            }
            Fx::Towable(u, on) => {
                for e in u {
                    if on {
                        commands.entity(e).remove::<crate::docking::NoTow>();
                    } else {
                        commands.entity(e).insert(crate::docking::NoTow);
                    }
                }
            }
            Fx::Movable(u, on) => {
                for e in u {
                    if on {
                        commands.entity(e).remove::<crate::collision::Pinned>();
                    } else {
                        commands.entity(e).insert(crate::collision::Pinned);
                    }
                }
            }
            Fx::Collidable(u, on) => {
                for e in u {
                    if on {
                        commands.entity(e).remove::<crate::collision::Ghost>();
                    } else {
                        commands.entity(e).insert(crate::collision::Ghost);
                    }
                }
            }
            Fx::Captain(u, human) => {
                for e in u {
                    if let Ok((.., mut ms, _, _)) = ships.get_mut(e) {
                        ms.human = human;
                    }
                }
            }
            Fx::Cloak(u, on, ai_override) => {
                for e in u {
                    if ships.contains(e) {
                        // A ship already fitted fades; a fresh one (world init) starts in the ordered state.
                        commands.queue(move |w: &mut World| match w.get_mut::<crate::cloak::Cloak>(e) {
                            Some(mut c) => (c.on, c.ai_override) = (on, ai_override),
                            None => {
                                if let Ok(mut x) = w.get_entity_mut(e) {
                                    x.insert(crate::cloak::Cloak::new(on, ai_override));
                                }
                            }
                        });
                    }
                }
            }
            Fx::WarningShots(u, on) => {
                for e in u {
                    if let Ok((.., mut ms, _, _)) = ships.get_mut(e) {
                        ms.warning_shots = on;
                    }
                }
            }
            Fx::Current(path) => {
                commands.spawn((Scenario, crate::environment::Current::new(path)));
            }
            Fx::DarkMatter(volume) => {
                // The blast starts at the volume's centre (game x, y) at the ships' height.
                if let Some(c) = mission.script.volume(&volume).centroid() {
                    extra.detonate.write(crate::combat::Detonate { bullet: "Bullet_DarkMatterExplosion".into(), at: Vec3::new(c.x, 0.0, -c.y), shooter: None });
                }
            }
            Fx::Longboat(u) => {
                // Launched from the human's command ship: alongside it, same heading.
                let from = ships.iter().find(|s| s.6).map(|s| (s.2.translation, s.1.heading, lib.classes[s.1.class].radius));
                if let Some((p, heading, r)) = from {
                    for (k, e) in u.into_iter().enumerate() {
                        if let Ok((_, mut sh, mut tf, ..)) = ships.get_mut(e) {
                            let side = Vec3::new(heading.cos(), 0.0, -heading.sin());
                            tf.translation = p + side * (r + 8.0 + 6.0 * k as f32);
                            sh.heading = heading;
                            sh.waypoints.clear();
                            sh.speed = 0.0;
                        }
                    }
                }
            }
            Fx::DragonStance(u, st) => {
                for e in u {
                    if let Ok(mut d) = extra.dragons.get_mut(e) {
                        d.stance = st;
                    }
                }
            }
            Fx::DragonThreshold(u, t) => {
                for e in u {
                    if let Ok(mut d) = extra.dragons.get_mut(e) {
                        d.threshold = t;
                    }
                }
            }
            Fx::Orbit { unit, dist, angle } => {
                orbit.focus = unit;
                orbit.dist = dist;
                let heading = unit.and_then(|e| ships.get(e).ok()).or_else(|| ships.iter().find(|s| s.6)).map_or(0.0, |s| s.1.heading);
                orbit.yaw = heading + angle.to_radians();
            }
        }
    }
    // Mission 1's refit task: a hint while the weapons tender is in range (docking itself is docking.rs; D is a keyboard shortcut).
    // `w` is last frame-start's snapshot: good enough for a hint and the dev autopilot.
    mission.script.hint.clear();
    if let (Some(p), Some(t)) = (w.iter().find(|u| u.primary && u.alive), w.iter().find(|u| u.name == "RLS Bounty")) {
        let wants = mission.script.task_active("Dock With Tender");
        let near = extra.sensors.in_sight(p.e, t.e, Some(mission)) && !mission.script.docked.contains(&(p.e, t.e));
        if wants && near {
            mission.script.hint = "Left-click RLS Bounty (or press D) to dock".into();
        }
        if near && wants && (keys.just_pressed(KeyCode::KeyD) || matches!(cfg.script.as_deref(), Some("mission" | "dock"))) {
            extra.dock.write(DockMsg::Dock { docker: None, dockee: t.e });
        }
    }
    // Dev hook: PROCYON_END_SHOT=<png> saves a screenshot 3 s after the mission ends and quits.
    if let (Some(path), Some(_)) = (dev_env().0.clone(), mission.script.ended.as_ref()) {
        extra.dev.end_age += dt;
        if extra.dev.end_age > 3.0 && !extra.dev.end_shot {
            extra.dev.end_shot = true;
            commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window()).observe(bevy::render::view::screenshot::save_to_disk(std::path::PathBuf::from(path)));
        } else if extra.dev.end_age > 8.0 {
            std::process::exit(0);
        }
    }
    // Dev hook: PROCYON_NIS_SHOT=<png> saves a screenshot 4 s into the third script NIS (rule starts) and quits shortly after.
    if let Some(path) = dev_env().1.clone() {
        if mission.script.nis && !mission.opening.is_some() {
            extra.dev.nis_age += dt;
        } else {
            extra.dev.nis_age = 0.0;
        }
        if extra.dev.nis_age > 4.0 && !extra.dev.shot_taken && mission.script.time > 400.0 {
            extra.dev.shot_taken = true;
            commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window()).observe(bevy::render::view::screenshot::save_to_disk(std::path::PathBuf::from(path)));
        } else if extra.dev.shot_taken {
            extra.dev.nis_age += dt;
            if extra.dev.nis_age > 12.0 {
                std::process::exit(0);
            }
        }
    }
    if cfg.script.as_deref() == Some("mission") {
        autopilot(&mission.script, &w, &mission.data, &rocks, &mut ships, &mut target);
    }
}

/// `--script mission`: follows the objective point, then closes on the nearest hostile and targets it (smoke test of the rules).
fn autopilot(
    sc: &Script,
    w: &[Unit],
    data: &Value,
    rocks: &Query<(Entity, &Transform, &Asteroid), (Without<Ship>, Without<crate::environment::SpaceAnimal>)>,
    ships: &mut Query<(Entity, &mut Ship, &mut Transform, &mut Health, &mut MissionShip, &mut Visibility, Has<PlayerShip>)>,
    target: &mut Target,
) {
    if sc.nis || sc.ended.is_some() {
        return;
    }
    let Some(p) = w.iter().find(|u| u.primary && u.alive) else { return };
    // Ships only: whales count as hostile until the reprimand rule (PROVISIONAL default), and chasing them stalled the run after Objective 3.
    let foe =
        w.iter().filter(|u| u.alive && u.visible && sc.hostile(&u.owner, &p.owner) && ships.contains(u.e)).min_by(|a, bb| a.pos.distance(p.pos).total_cmp(&bb.pos.distance(p.pos)));
    let dock = sc.task_active("Dock With Tender");
    let shoot_rocks = sc.task_active("Destroy Asteroids");
    let rock =
        rocks.iter().filter(|(_, _, a)| a.group == "Asteroid Belt" && a.hp > 0.0).min_by(|a, bb| a.1.translation.distance(p.pos).total_cmp(&bb.1.translation.distance(p.pos)));
    let goal = if let (true, Some((e, tf, _))) = (shoot_rocks, rock.filter(|r| r.1.translation.distance(p.pos) < 400.0)) {
        target.0 = Some(e);
        Some(tf.translation)
    } else if let Some(foe) = foe.filter(|f| f.pos.distance(p.pos) < 700.0) {
        target.0 = Some(foe.e);
        Some(foe.pos)
    } else if dock {
        w.iter().find(|u| u.name == "RLS Bounty").map(|u| u.pos)
    } else {
        sc.point.map(|i| game_to_bevy(&data["points"][i]["Position"]))
    };
    // Dev shortcut for end-to-end runs: PROCYON_AUTOKILL=1 sinks the pirate once the player is close.
    if std::env::var_os("PROCYON_AUTOKILL").is_some()
        && let Some(wr) =
            w.iter().find(|u| u.name == "Warrior" && u.alive && u.visible && u.pos.distance(p.pos) < 250.0 && sc.hits_by.get(&(PLAYER.to_string(), u.e)).is_some_and(|n| *n >= 3))
        && let Ok(mut x) = ships.get_mut(wr.e)
    {
        x.3.core = 0.0;
    }
    if let Some(mut s) = ships.iter_mut().find(|s| s.6)
        && let Some(g) = goal
    {
        if g.distance(p.pos) > 60.0 || foe.is_none() {
            s.1.waypoints = vec![g];
            s.1.order = 2;
            s.1.goal = None;
        } else {
            s.1.waypoints.clear();
            s.1.order = 1;
        }
    }
}

/// Alliances -> sides, hold fire, target choice and movement for every unit but the player's; towing.
fn mission_ai(
    mission: Option<Res<Mission>>,
    lib: Res<Library>,
    rocks: Query<(Entity, &Transform, &Asteroid), Without<Ship>>,
    mut ships: Query<(Entity, &mut Ship, &mut Transform, &Health, &mut MissionShip, Has<PlayerShip>, Option<&crate::ship::FleetPilot>)>,
    busy: Query<(), crate::ship::CaptainBusy>,
    boats: Query<(), With<crate::lifeboats::Lifeboat>>,
    sensors: Res<crate::environment::Sensors>,
    held: Query<(), With<crate::ship::HoldPosition>>,
) {
    let Some(mission) = mission else { return };
    let sc = &mission.script;
    // Targets for orders and hostility: ships, then rocks (rocks only answer attack orders, never hostility); tagged with TWT groups.
    let unit = |e, id, name: &str, owner: &str, pos, alive| Unit {
        e,
        id,
        name: name.into(),
        owner: owner.into(),
        owner_is_named_group: false,
        groups: vec![],
        pos,
        alive,
        visible: alive,
        primary: false,
        damage: 0.0,
        flag: String::new(),
        attack: vec![],
    };
    // Lifeboats (mission 4's pre-placed ones) are never an enemy or a target, whoever owns them.
    let mut snap: Vec<Unit> =
        ships.iter().filter(|s| !boats.contains(s.0)).map(|(e, _, tf, h, m, _, _)| unit(e, m.id, &m.name, &m.owner, tf.translation, !h.destroyed && m.visible)).collect();
    let n_ships = snap.len();
    snap.extend(rocks.iter().map(|(e, tf, a)| unit(e, a.id, &a.kind, &a.group, tf.translation, a.hp > 0.0)));
    sc.tag(&mut snap);
    let player_owner = ships.iter().find(|s| s.5).map(|s| s.4.owner.clone()).unwrap_or_else(|| sc.player().into());
    for (e, mut ship, tf, h, mut ms, primary, pilot) in &mut ships {
        let is_boat = boats.contains(e);
        let side = if !is_boat && sc.hostile(&ms.owner, &player_owner) { Side::Enemy } else { Side::Player };
        if ship.side != side {
            ship.side = side;
        }
        if is_boat {
            continue; // drifts or sits where it was placed; no captain AI
        }
        let hold = ms.stance == Stance::Dummy || sc.holds_fire(&ms.owner);
        if ms.hold_fire != hold {
            ms.hold_fire = hold;
        }
        if sc.tows().iter().any(|link| link.1 == e) {
            ship.waypoints.clear();
            ship.order = 0;
            ship.goal = None;
            continue; // Movement/turning come from the shared tow constraint, not a rigid teleport.
        }
        // Border zone: a ship outside its leash volume drops what it was doing and heads back (to its return point, else the
        // zone's centre). PROVISIONAL: the exe's border AI is not decoded; this re-enters and resumes on the next frame inside.
        if !primary
            && !h.destroyed
            && !busy.contains(e)
            && let Some(back) = ms.border.as_deref().and_then(|z| leash(sc, z, ms.return_point, tf.translation))
        {
            ship.waypoints = vec![back];
            ship.goal = None;
            ship.order = 2;
            ship.throttle = None;
            ms.stop_at_end = true;
            continue;
        }
        if primary || h.destroyed || busy.contains(e) || held.contains(e) || pilot.is_some_and(|p| p.hold || p.destination.is_some()) {
            continue;
        }
        ship.throttle = None;
        // loops: restart an exhausted path
        if ship.waypoints.is_empty()
            && let Some(p) = ms.looping.clone()
        {
            ship.waypoints = p;
            ship.order = 2;
        }
        let range = lib.classes[ship.class].engage * 1.3;
        let partner = |x: Entity| sc.tows().iter().any(|t| (t.0 == e && t.1 == x) || (t.1 == e && t.0 == x));
        let near = |pool: &[Unit], pred: &dyn Fn(&Unit) -> bool, acquire: bool| {
            pool.iter()
                .filter(|s| s.e != e && s.alive && pred(s) && !partner(s.e) && (!acquire || sensors.in_sight(e, s.e, Some(&mission))))
                .min_by(|a, b| a.pos.distance(tf.translation).total_cmp(&b.pos.distance(tf.translation)))
                .map(|s| (s.e, s.pos))
        };
        // 'GroupA to attack GroupB': the nearest ship of the target, else the nearest of its rocks ('attack Asteroid Belt').
        let explicit = pilot.and_then(|p| p.target).and_then(|e| snap.iter().find(|u| u.e == e && u.alive).map(|u| (u.e, u.pos)));
        let ordered = explicit.or_else(|| {
            near(&snap[..n_ships], &|s| ms.attack.iter().any(|g| matches(s, g)), false).or_else(|| near(&snap[n_ships..], &|s| ms.attack.iter().any(|g| matches(s, g)), false))
        });
        let hostile = near(&snap[..n_ships], &|s| sc.hostile(&ms.owner, &s.owner), true);
        let want = if hold {
            None
        } else if let Some(o) = ordered {
            Some(o.0)
        } else {
            hostile.filter(|(_, p)| p.distance(tf.translation) < range).map(|x| x.0)
        };
        if ms.target != want {
            ms.target = want;
        }
        // Movement: an attack order or an aggressive stance closes on the foe (same broadside logic as the skirmish AI).
        // A human-captained ship (Captain_Human) only fires: it moves on the human fleet's orders (ship.rs fleet_follow).
        let chase = if ms.human || (ms.stance == Stance::Persistent && !ship.waypoints.is_empty()) {
            None // Persistent scripted routes take precedence over combat steering (Mission 3's swoop/retreat).
        } else {
            ordered.or(if ms.stance == Stance::Aggressive { hostile } else { None })
        };
        if ms.human {
            continue;
        }
        if let (Some((_, fp)), false) = (chase, ms.stance == Stance::Dummy) {
            let d = fp - tf.translation;
            let dist = Vec2::new(d.x, d.z).length();
            let bearing = (-d.x).atan2(-d.z);
            let engage = lib.classes[ship.class].engage;
            let side = [bearing + std::f32::consts::FRAC_PI_2, bearing - std::f32::consts::FRAC_PI_2]
                .into_iter()
                .min_by(|a, b| crate::angdiff(*a, ship.heading).abs().total_cmp(&crate::angdiff(*b, ship.heading).abs()))
                .unwrap();
            let (goal, order) = if ms.ram && ordered.is_some() {
                (bearing, 3)
            } else if dist > engage * 1.1 {
                (bearing, 2)
            } else if dist < engage * 0.5 {
                (side - 0.5 * crate::angdiff(bearing, side).signum(), 1)
            } else {
                (side, 1)
            };
            ship.waypoints.clear();
            ship.goal = Some(goal);
            ship.order = order;
            ms.stop_at_end = true;
        } else if ms.stop_at_end && ms.looping.is_none() && ship.waypoints.len() <= 1 {
            // Retail takes the last point of a 'to end' path within 2 x the ship's radius, then cuts throttle to 0.
            // PROVISIONAL: `class.radius` stands in for the exe's radius field. A finished chase (remake AI) likewise waits for orders.
            if ship.waypoints.first().is_some_and(|p| p.distance(tf.translation) >= 2.0 * lib.classes[ship.class].radius) {
                continue;
            }
            ship.waypoints.clear();
            ms.stop_at_end = false;
            ship.order = 0;
            ship.goal = None;
        }
    }
}

/// Where a unit outside its border zone `zone` should head: its return point, else the zone's centre. None while inside
/// (or when the zone is unknown).
pub fn leash(sc: &Script, zone: &str, home: Option<Vec3>, pos: Vec3) -> Option<Vec3> {
    let vol = sc.volume(zone);
    if vol.len() < 3 || in_volume(vol, pos) {
        return None;
    }
    home.or_else(|| vol.centroid().map(|c| Vec3::new(c.x, pos.y, -c.y)))
}

/// The human's command ship on a border zone (missions 3, 9): steered back inside with the helmsman's off-course line.
/// PROVISIONAL: the retail helm takeover is not decoded; the ship turns for the zone and resumes the player's orders inside.
fn leash_player(mut mission: ResMut<Mission>, mut ships: Query<(&mut Ship, &Transform, &MissionShip, &Health), With<PlayerShip>>, mut out: Local<bool>) {
    let Ok((mut ship, tf, ms, h)) = ships.single_mut() else { return };
    let back = ms.border.as_deref().filter(|_| !h.destroyed).and_then(|z| leash(&mission.script, z, ms.return_point, tf.translation));
    match back {
        Some(p) => {
            if !*out {
                mission.script.helm_off_course = true;
            }
            ship.waypoints = vec![p];
            ship.goal = None;
            if ship.order == 0 {
                ship.order = 1;
            }
        }
        None if *out => {
            ship.waypoints.clear();
        }
        None => {}
    }
    *out = back.is_some();
}

/// Portrait sprite for a speech line by speaker prefix. PROVISIONAL: one fixed expression per speaker.
fn speaker_portrait(line: &str) -> Option<&'static str> {
    let who = line.strip_prefix("M01").unwrap_or(line);
    [("AMELIA", "Amelia_Happy"), ("JIM", "Jim_Stoic"), ("ONYX", "Onyx_Stoic")].into_iter().find(|(p, _)| who.starts_with(p)).map(|x| x.1)
}

fn ui(
    mut talk: ResMut<crate::panels::Talk>,
    assets: Res<AssetServer>,
    hud: Res<crate::sprites::HudData>,
    mut images: Query<(&Ui, &mut ImageNode)>,
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
    for (kind, mut image) in &mut images {
        let name = match kind {
            Ui::SpeechHead => portrait.clone(),
            Ui::SpeechFrame => Some("TalkingHead_FloatingPortal_Open".to_string()),
            _ => continue,
        };
        if let Some((sheet, rect)) = name.and_then(|n| crate::sprites::sprite_of(&hud, &n)) {
            image.image = assets.load(format!("hud/tex/{sheet}.png"));
            image.rect = Some(crate::sprites::inset(&sheet, rect));
        }
    }
    for (kind, mut vis, text) in &mut q {
        let (show, content): (bool, String) = match kind {
            Ui::Speech => (speaking && !sc.nis, sc.subtitle.clone()),
            Ui::SpeechTop => (cutscene, sc.subtitle.clone()),
            Ui::SpeechPanel => (cutscene, String::new()),
            Ui::SpeechFrame | Ui::SpeechHead => (cutscene && portrait.is_some(), String::new()),
            Ui::Hint => (!crate::panels::cinematic(Some(&mission)) && !sc.hint.is_empty(), sc.hint.clone()),
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
        *vis = if show { Visibility::Inherited } else { Visibility::Hidden };
        if let Some(mut t) = text
            && **t != content
        {
            **t = content;
        }
    }
    for mut t in &mut objective {
        **t = if crate::panels::cinematic(Some(&mission)) { String::new() } else { sc.tasks_text() };
    }
}

fn music(
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
fn presentation(
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
fn nis_camera(
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
                let pts: Vec<Vec3> = sc.paths[name.as_str()].as_array().map(|a| a.iter().map(game_to_bevy).collect()).unwrap_or_default();
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

#[cfg(test)]
mod ai_tests {
    use super::*;
    #[test]
    fn mission_two_attack_volume_changes_base_owner_and_gunfire_advances_the_story() {
        let mut mission = crate::campaign::load_id("mission02");
        let mut data = mission.data.clone();
        data["rules"].as_array_mut().unwrap().retain(|r| matches!(r["name"].as_str(), Some("SmugglerCampFound2" | "SmugglersCampFound2c")));
        mission.script = Script::new(&data, serde_json::json!({}));
        let player_owner = mission.script.player().to_string();
        let lib = Library::load(&crate::assets_dir());
        let class = lib.index("Ship_Navy_Torpedo");
        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()))
            .init_asset::<Image>()
            .init_asset::<bevy::audio::AudioSource>()
            .init_resource::<Time>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Target>()
            .init_resource::<crate::bindings::Bindings>()
            .init_resource::<crate::campaign::CampaignPoints>()
            .init_resource::<crate::nebula::StormShift>()
            .init_resource::<crate::environment::Sensors>()
            .add_systems(PreUpdate, crate::environment::refresh_sensors)
            .add_message::<Hit>()
            .add_message::<Fired>()
            .add_message::<Destroyed>()
            .add_message::<DockMsg>()
            .add_message::<crate::boarding::BoardMsg>()
            .add_message::<crate::boarding::Captured>()
            .add_message::<crate::combat::AsteroidSetup>()
            .add_message::<crate::nebula::MeteorRoute>()
            .add_message::<crate::combat::Detonate>()
            .insert_resource(Orbit { yaw: 0.0, pitch: 0.4, dist: 90.0, focus: None })
            .insert_resource(Config {
                groups: vec![],
                player: "Ship_Navy_Torpedo".into(),
                allies: vec![],
                enemies: vec![],
                map: Some("mission02".into()),
                shot: None,
                frame: 0,
                speed: 1.0,
                ended: None,
                script: None,
                shot_frame: 0,
                late_frame: 0,
            })
            .insert_resource(mission)
            .add_systems(Update, update);
        let ship = Ship {
            class,
            side: Side::Player,
            heading: 0.0,
            speed: 0.0,
            ang_vel: 0.0,
            order: 0,
            waypoints: vec![],
            goal: None,
            cap: 1.0,
            throttle: None,
            sail_energy: 1.0,
            crew_speed: 1.0,
            crew_turn: 1.0,
        };
        let player = app
            .world_mut()
            .spawn((
                ship,
                Health::new(&lib.classes[class]),
                Transform::from_xyz(-970.0, 0.0, 450.0),
                Visibility::Inherited,
                PlayerShip,
                MissionShip { id: 249, name: "RLS Comet".into(), owner: player_owner.clone(), visible: true, ..default() },
            ))
            .id();
        let base = app.world_mut().spawn((MissionBase { id: 24, owner: "NO PLAYER".into() }, Transform::default())).id();
        app.insert_resource(lib);
        app.update();
        let owner = &app.world().get::<MissionBase>(base).unwrap().owner;
        assert_eq!(owner, "SmugglerCampEnemy");
        assert!(app.world().resource::<Mission>().script.hostile(owner, &player_owner));
        app.world_mut().resource_mut::<Messages<Fired>>().write(Fired { shooter: base, target: Some(player), bullet: "Bullet_CannonBall_Light".into() });
        app.update();
        let script = &app.world().resource::<Mission>().script;
        assert!(script.timers.contains_key("OnyxFirePause"), "the first tower shot unlocks the next dialogue");
        assert!(script.timers.contains_key("SmugglerCampSwitchTwo"));
    }

    #[test]
    fn mission_ships_halt_when_their_script_route_runs_out() {
        let lib = Library::load(&crate::assets_dir());
        let mut app = App::new();
        app.insert_resource(crate::campaign::load())
            .init_resource::<crate::environment::Sensors>()
            .add_systems(PreUpdate, crate::environment::refresh_sensors)
            .add_systems(Update, mission_ai);
        let ship = Ship {
            class: 0,
            side: Side::Player,
            heading: 0.0,
            speed: 5.0,
            ang_vel: 0.0,
            order: 2,
            waypoints: vec![],
            goal: None,
            cap: 1.0,
            throttle: None,
            sail_energy: 1.0,
            crew_speed: 1.0,
            crew_turn: 1.0,
        };
        let n = lib.classes[0].sections.len();
        let health = || Health { sections: vec![1.0; n], core: 1.0, destroyed: false };
        app.insert_resource(lib);
        let ms = |stop, looping: Option<Vec<Vec3>>| MissionShip {
            id: -1,
            name: "Test".into(),
            owner: "Navy Tenders".into(),
            hold_fire: true,
            invuln: 0.0,
            visible: true,
            stance: Stance::Dummy,
            target: None,
            attack: vec![],
            flag: String::new(),
            banner: String::new(),
            looping,
            stop_at_end: stop,
            create_lifeboats: false,
            ..Default::default()
        };
        let done = app.world_mut().spawn((ship.clone(), health(), Transform::default(), ms(true, None))).id();
        let en_route = app.world_mut().spawn((Ship { waypoints: vec![Vec3::new(0.0, 0.0, -500.0)], ..ship.clone() }, health(), Transform::default(), ms(true, None))).id();
        let looping = app.world_mut().spawn((ship.clone(), health(), Transform::default(), ms(true, Some(vec![Vec3::new(0.0, 0.0, -500.0)])))).id();
        let near = app.world_mut().spawn((Ship { waypoints: vec![Vec3::new(0.0, 0.0, -1.0)], ..ship.clone() }, health(), Transform::default(), ms(true, None))).id();
        let foe = app
            .world_mut()
            .spawn((
                ship.clone(),
                health(),
                Transform::from_xyz(1000.0, 0.0, 0.0),
                MissionShip { name: "Foe".into(), owner: "Target".into(), visible: true, stance: Stance::Dummy, ..default() },
            ))
            .id();
        let route = vec![Vec3::NEG_Z * 500.0];
        let persistent = app
            .world_mut()
            .spawn((
                Ship { waypoints: route.clone(), ..ship },
                health(),
                Transform::from_xyz(300.0, 0.0, 0.0),
                MissionShip { attack: vec!["Target".into()], stance: Stance::Persistent, ..ms(true, None) },
            ))
            .id();
        app.update();
        assert_eq!(app.world().get::<MissionShip>(persistent).unwrap().target, Some(foe));
        assert_eq!(app.world().get::<Ship>(persistent).unwrap().waypoints, route, "persistent attack order must not cancel the scripted swoop");
        let order = |e| app.world().get::<Ship>(e).unwrap().order;
        assert_eq!(order(done), 0, "finished route stops");
        assert!(!app.world().get::<MissionShip>(done).unwrap().stop_at_end);
        assert_eq!(order(en_route), 2, "still on its path");
        assert_eq!(order(near), 0, "last point within 2 x radius counts as reached");
        assert!(app.world().get::<Ship>(near).unwrap().waypoints.is_empty());
        assert_eq!(order(looping), 2, "loops restart instead");
        app.world_mut().get_mut::<MissionShip>(persistent).unwrap().stance = Stance::Aggressive;
        app.update();
        assert!(app.world().get::<Ship>(persistent).unwrap().waypoints.is_empty(), "aggressive stance switches to combat steering");
    }
}
