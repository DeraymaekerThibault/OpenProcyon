//! ECS side of the rule interpreter: dispatch of `Script` effects, mission AI, mission UI, music and NIS camera.
pub(super) mod apply;
mod dev_console;
mod mission_ai;
mod presentation;
mod teleport;

use crate::Nearest;

use bevy::audio::Volume;

use bevy::ecs::query::QueryData;

use bevy::prelude::*;

use serde_json::Value;

use super::*;
use apply::each_ship;
pub use dev_console::enabled as console_enabled;
pub use mission_ai::leash;
use mission_ai::{follow_path, leash_player, mission_ai, path_points};
pub use presentation::Ui;
pub(super) use presentation::{EventFx, event_effect};
use presentation::{music, nis_camera, presentation, ui};

use crate::campaign::{Mission, MissionBase, MissionShip};

use crate::combat::{Asteroid, Destroyed, Fired, Health, Hit, Library, Target};

use crate::docking::DockMsg;

use crate::{Config, Orbit, PlayerShip, Scenario, Ship, Side, game_to_bevy};

// ---------------------------------------------------------------------------------------------------------------- ECS side

/// Parameters of `update` beyond Bevy's 16-argument limit.
#[derive(bevy::ecs::system::SystemParam)]
struct Extra<'w, 's> {
    bases: Query<
        'w,
        's,
        (Entity, &'static mut MissionBase, &'static Transform, Option<(&'static crate::combat::ArmedBase, &'static Health)>),
        (Without<Ship>, Without<crate::environment::SpaceAnimal>),
    >,
    /// Not retail: the mines a custom mission's map places (`mines`), which its rules hand to players and address by group.
    mines: apply::Mines<'w, 's>,
    dock: MessageWriter<'w, DockMsg>,
    difficulty: Option<Res<'w, crate::scenario::Difficulty>>,
    /// Where the frame's units are left for the dev autopilot.
    snapshot: ResMut<'w, RuleWorld>,
    board: MessageWriter<'w, crate::boarding::BoardMsg>,
    captured: MessageReader<'w, 's, crate::boarding::Captured>,
    asteroids: MessageWriter<'w, crate::combat::AsteroidSetup>,
    meteors: MessageWriter<'w, crate::nebula::MeteorRoute>,
    storm_shift: Res<'w, crate::nebula::StormShift>,
    campaign_points: ResMut<'w, crate::campaign::CampaignPoints>,
    /// Not retail: what a won custom mission hands on to the next one (`OP Set Campaign Counter`).
    campaign_values: Option<ResMut<'w, crate::campaign::CampaignValues>>,
    sensors: Res<'w, crate::environment::Sensors>,
    voices: Query<'w, 's, Entity, With<crate::briefing::Voice>>,
    fxlib: Option<Res<'w, crate::fx::FxLib>>,
    event_fx: Query<'w, 's, (Entity, &'static EventFx)>,
    sfx: Option<ResMut<'w, Messages<crate::audio::Sfx>>>,
    toggles: Option<ResMut<'w, crate::panels::Toggles>>,
    detonate: MessageWriter<'w, crate::combat::Detonate>,
    dragons: Query<'w, 's, &'static mut crate::dragons::Dragon>,
    /// Cheats used in this battle: its victory points are not recorded (`console::Cheats`).
    cheats: Option<Res<'w, console::Cheats>>,
    /// `--script dock` skipped the briefing already.
    dock_smoke_skipped: Local<'s, bool>,
}

/// The units `update` saw this frame (before its effects applied), left for the dev autopilot.
#[derive(Resource, Default)]
pub struct RuleWorld(pub Vec<Unit>);

#[derive(Resource, Default)]
struct RuleInput {
    skip_speech: bool,
    dock: bool,
}

fn capture_input(keys: Res<ButtonInput<KeyCode>>, binds: Res<crate::bindings::Bindings>, mission: Res<Mission>, mut input: ResMut<RuleInput>) {
    input.skip_speech |= mission.opening.is_none() && binds.just_pressed(&keys, crate::bindings::Act::SkipSpeech);
    input.dock |= keys.just_pressed(KeyCode::KeyD);
}

pub fn plugin(app: &mut App) {
    use crate::menu::Screen;
    app.add_plugins(dev_console::plugin)
        .insert_resource(crate::campaign::CampaignPoints::load())
        .init_resource::<crate::campaign::CampaignValues>()
        .init_resource::<RuleInput>()
        .init_resource::<RuleWorld>()
        .add_systems(
            FixedUpdate,
            (
                crate::briefing::autoplay_skip.in_set(crate::BattleSet::Commands),
                crate::smoke::autoplay_end.in_set(crate::BattleSet::Outcome),
                (mission_ai, leash_player).chain().in_set(crate::BattleSet::Ai),
            )
                .run_if(in_state(Screen::Battle))
                .run_if(resource_exists::<Mission>),
        )
        .add_systems(
            Update,
            (
                capture_input.before(crate::briefing::update),
                update.after(capture_input).in_set(crate::FrameRules).run_if(crate::menu::overlay_closed),
                crate::smoke::autopilot::drive.after(update).in_set(crate::FrameRules).run_if(crate::menu::overlay_closed),
                crate::smoke::screenshot_hooks,
                presentation.after(nis_camera),
                (ui, music, nis_camera.after(crate::camera)),
            )
                .run_if(in_state(Screen::Battle))
                .run_if(resource_exists::<Mission>),
        );
}

/// A mission ship as the rule dispatcher reads and changes it; named fields instead of tuple positions.
#[derive(QueryData)]
#[query_data(mutable)]
pub struct RuleShip {
    pub entity: Entity,
    pub ship: &'static mut Ship,
    pub transform: &'static mut Transform,
    pub health: &'static mut Health,
    pub mission: &'static mut MissionShip,
    pub visibility: &'static mut Visibility,
    /// The human's own ship.
    pub primary: Has<PlayerShip>,
    /// Its interpolation: rules see the simulation pose (`RuleShip::sim_pos`), not the drawn one.
    pub interp: Option<&'static crate::interpolate::Interp>,
    /// Fully cloaked ships answer `OP Group Is Cloaked`.
    pub cloak: Option<&'static crate::cloak::Cloak>,
    /// Not retail: the crew was ordered off by `OP Abandon Ship` (lifeboats.rs).
    pub abandoned: Has<crate::lifeboats::Abandoned>,
}

/// The simulation position of a ship whose drawn pose is `tf` (interpolated between ticks when it has an `Interp`).
pub fn sim_pos(tf: &Transform, interp: Option<&crate::interpolate::Interp>) -> Vec3 {
    interp.map_or(tf.translation, |i| i.sim_pose(tf).translation)
}

fn units(ships: &Query<RuleShip>, lib: &Library) -> Vec<Unit> {
    ships
        .iter()
        .map(|RuleShipReadOnlyItem { entity: e, ship, transform: tf, health: h, mission: m, primary, interp, cloak, .. }| {
            let class = &lib.classes[ship.class];
            let secs =
                if h.sections.is_empty() { 0.0 } else { h.sections.iter().zip(&class.sections).map(|(x, c)| 1.0 - x / c.hp.max(1.0)).sum::<f32>() / h.sections.len() as f32 };
            Unit {
                name: m.name.as_str().into(),
                owner: m.owner.as_str().into(),
                alive: !h.destroyed,
                visible: m.visible,
                primary,
                damage: (1.0 - h.core / class.core_hp).max(secs).clamp(0.0, 1.0),
                flag: m.flag.clone(),
                attack: m.attack.clone(),
                heading: ship.heading,
                speed: Some(ship.speed.abs()),
                cloaked: cloak.is_some_and(|c| c.state == crate::cloak::State::Full),
                ..Unit::new(e, m.id, sim_pos(tf, interp))
            }
        })
        .collect()
}

fn update(
    mission: Option<ResMut<Mission>>,
    time: Res<Time>,
    mut input: ResMut<RuleInput>,
    cfg: Res<Config>,
    lib: Res<Library>,
    mut orbit: ResMut<Orbit>,
    mut hits: MessageReader<Hit>,
    mut fired: MessageReader<Fired>,
    mut killed: MessageReader<Destroyed>,
    rocks: Query<(Entity, &Transform, &Asteroid), (Without<Ship>, Without<crate::environment::SpaceAnimal>)>,
    mut animals: Query<(Entity, &mut crate::environment::SpaceAnimal, &mut Transform, &mut Visibility), Without<Ship>>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut ships: Query<RuleShip>,
    mut extra: Extra,
) {
    let Some(mut mission) = mission else { return };
    let mission = &mut *mission;
    let input = std::mem::take(&mut *input);
    // Tutorials and mods' custom missions run the same interpreter but do not count toward the retail campaign.
    if crate::campaign::is_campaign(&mission.id) && !extra.cheats.as_ref().is_some_and(|c| c.used) {
        extra.campaign_points.record(&mission.id, mission.script.score);
    }
    let opening = mission.opening.is_some();
    let dt = time.delta_secs();
    crate::smoke::dock(&cfg, mission, &mut extra.dock_smoke_skipped, &mut ships);
    let mut world = units(&ships, &lib);
    mission.script.keep_sunk(&mut world);
    world.extend(animals.iter().map(|(e, a, tf, _)| a.unit(e, tf)));
    world.extend(extra.bases.iter_mut().map(|(e, b, tf, armed)| b.unit(e, tf, armed.map_or(0.0, |(a, h)| crate::combat::base_mission_damage(&lib.classes[a.class], h)))));
    world.extend(rocks.iter().map(|(e, tf, a)| a.unit(e, tf)));
    // Not retail: a mission's mines answer to an explicit group (their world-object ids) only, never to their owner's name: a faction
    // whose ships are all gone is destroyed although its mines rest, and owner-wide orders do not reach them.
    world.extend(extra.mines.iter().map(|(e, tf, m, _)| Unit { owner: GroupName::default(), ..m.unit(e, tf) }));
    mission.script.tag(&mut world);
    let owner_of = |e: Entity| world.iter().find(|u| u.e == e).map(|u| u.owner.to_string());
    for h in hits.read() {
        mission.script.record_hit(owner_of(h.shooter).as_deref(), h.victim);
    }
    for fr in fired.read() {
        mission.script.record_fire(fr.shooter, fr.target);
    }
    for c in extra.captured.read() {
        mission.script.record_capture(&c.by, &c.victim, c.unit, extra.bases.contains(c.unit));
    }
    let script = &mut mission.script;
    for u in world.iter().filter(|u| rocks.contains(u.e)) {
        script.belt.entry(u.e).or_insert_with(|| std::iter::once(u.owner.to_string()).chain(u.groups.iter().map(ToString::to_string)).collect());
    }
    for d in killed.read() {
        if let Some(o) = owner_of(d.by) {
            // A destroyed rock is one the belt remembers (it may already be gone from the unit list).
            let rock = script.belt.contains_key(&d.victim);
            *script.kills.entry((o.clone(), rock)).or_default() += 1;
            script.record_destroyed(&o, d.victim);
        }
    }
    // Space (Skip Speech Event) ends the current line in every cutscene/gameplay speech, as in the opening NIS.
    if !opening && input.skip_speech && script.skip_speech() {
        for e in &extra.voices {
            commands.entity(e).despawn();
        }
    }
    script.nebulae = crate::nebula::polygons(&mission.data, &extra.storm_shift);
    let (fxs, voice) = script.step(&world, dt, opening);
    if let Some(file) = voice {
        commands.spawn((
            Scenario,
            crate::briefing::Voice,
            AudioPlayer::new(assets.load(crate::audio::sound(&file))),
            PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(crate::menu::volume(crate::menu::Setting::Dialogue))),
        ));
    }
    apply_fx(fxs, mission, &mut commands, &mut ships, &mut animals, &mut extra, &mut orbit, &lib);
    if let Some(values) = mission.script.campaign_commit()
        && let Some(handed_on) = extra.campaign_values.as_mut()
    {
        handed_on.commit(&mission.id, values);
    }
    // Mission 1's refit task: a hint while the weapons tender is in range (docking itself is docking.rs; D is a keyboard shortcut).
    // `world` is last frame-start's snapshot: good enough for a hint and the dev autopilot.
    mission.script.hint.clear();
    if let (Some(p), Some(t)) = (world.iter().find(|u| u.primary && u.alive), world.iter().find(|u| u.name == "RLS Bounty")) {
        let wants = mission.script.task_active("Dock With Tender");
        let near = extra.sensors.in_sight(p.e, t.e, Some(mission)) && !mission.script.docked.contains(&(p.e, t.e));
        if wants && near {
            mission.script.hint = "Left-click RLS Bounty (or press D) to dock".into();
        }
        if near && wants && (input.dock || matches!(cfg.script.as_deref(), Some("mission" | "dock"))) {
            extra.dock.write(DockMsg::Dock { docker: None, dockee: t.e });
        }
    }
    // The frame's snapshot, for the dev autopilot (smoke/autopilot.rs), which runs right after this system.
    extra.snapshot.0 = world;
}

/// Applies the effects a `Script::step` returned to the world: ship orders, flags, docking and tow messages, effects, the orbit camera.
/// The application itself is `apply::apply`, shared with the skirmish maps' scripts.
fn apply_fx(
    fxs: Vec<Fx>,
    mission: &mut Mission,
    commands: &mut Commands,
    ships: &mut Query<RuleShip>,
    animals: &mut Query<(Entity, &mut crate::environment::SpaceAnimal, &mut Transform, &mut Visibility), Without<Ship>>,
    extra: &mut Extra,
    orbit: &mut Orbit,
    lib: &Library,
) {
    let mut env = apply::FxEnv {
        script: &mut mission.script,
        paths: &mission.data["paths"],
        lib,
        bases: &mut extra.bases,
        dragons: &mut extra.dragons,
        dock: &mut extra.dock,
        board: &mut extra.board,
        asteroids: &mut extra.asteroids,
        meteors: &mut extra.meteors,
        detonate: &mut extra.detonate,
        event_fx: &extra.event_fx,
        fxlib: extra.fxlib.as_deref(),
        sfx: extra.sfx.as_deref_mut(),
        toggles: extra.toggles.as_deref_mut(),
        difficulty: extra.difficulty.as_deref().copied().unwrap_or_default().0,
        orbit: Some(orbit),
        sides: None,
        mines: Some(&mut extra.mines),
    };
    apply::apply(fxs, &mut env, commands, ships, animals);
}

#[cfg(test)]
mod tests;
