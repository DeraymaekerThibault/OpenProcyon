//! ECS side of the rule interpreter: dispatch of `Script` effects, mission AI, mission UI, music and NIS camera.
pub(super) mod apply;
mod dev_console;
mod mission_ai;
mod presentation;
mod report;
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
pub use report::{path_arg as report_path_arg, set_path as set_report_path, write as write_report, write_world as write_report_world};

use crate::campaign::{Mission, MissionBase, MissionShip};

use crate::combat::{Asteroid, Destroyed, Fired, Health, Hit, Library, Target};

use crate::docking::DockMsg;

use crate::{Config, Orbit, Scenario, Ship, Side, game_to_bevy};

// ---------------------------------------------------------------------------------------------------------------- ECS side

/// Parameters of `update` beyond Bevy's 16-argument limit.
#[derive(bevy::ecs::system::SystemParam)]
struct Extra<'w, 's> {
    fx: apply::FxParam<'w, 's>,
    /// Where the frame's units are left for the dev autopilot.
    snapshot: ResMut<'w, RuleWorld>,
    captured: MessageReader<'w, 's, crate::boarding::Captured>,
    storm_shift: Res<'w, crate::nebula::StormShift>,
    in_nebula: Option<Res<'w, crate::nebula::InNebula>>,
    campaign_points: ResMut<'w, crate::campaign::CampaignPoints>,
    /// Not retail: what a won custom mission hands on to the next one (`OP Set Campaign Counter`).
    campaign_values: Option<ResMut<'w, crate::campaign::CampaignValues>>,
    sensors: Res<'w, crate::environment::Sensors>,
    voices: Query<'w, 's, Entity, With<crate::briefing::Voice>>,
    /// Cheats used in this battle: its victory points are not recorded (`console::Cheats`).
    cheats: Option<Res<'w, console::Cheats>>,
    /// The battle's alliance table, lent to the script while it runs.
    alliances: Option<ResMut<'w, crate::alliance::Alliances>>,
    /// `--script dock` skipped the briefing already.
    dock_smoke_skipped: Local<'s, bool>,
}

/// The units `update` saw this frame (before its effects applied), left for the dev autopilot.
#[derive(Resource, Default)]
pub struct RuleWorld(pub Vec<Unit>);

#[derive(Resource, Default)]
struct RuleInput {
    skip_speech: bool,
}

fn capture_input(keys: Res<ButtonInput<KeyCode>>, binds: Res<crate::bindings::Bindings>, mission: Res<Mission>, mut input: ResMut<RuleInput>) {
    input.skip_speech |= mission.opening.is_none() && binds.just_pressed(&keys, crate::bindings::Act::SkipSpeech);
}

pub fn plugin(app: &mut App) {
    use crate::menu::Screen;
    app.add_plugins((dev_console::plugin, report::plugin))
        .insert_resource(crate::campaign::CampaignPoints::load())
        .init_resource::<crate::campaign::CampaignValues>()
        .init_resource::<RuleInput>()
        .init_resource::<RuleWorld>()
        .add_systems(
            FixedUpdate,
            (
                crate::briefing::autoplay_skip.in_set(crate::BattleSet::Commands),
                crate::smoke::autoplay_end.in_set(crate::BattleSet::Outcome),
                (mission_ai, leash_player).chain().after(crate::ai).in_set(crate::BattleSet::Ai),
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
    pub primary: Has<crate::commands::Commander>,
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

/// A mission base as a rule unit: its damage and its per-kind section damage (none without a class of sections).
pub(crate) fn base_unit(b: &crate::campaign::MissionBase, e: Entity, tf: &Transform, armed: Option<(&crate::combat::ArmedBase, &Health)>, lib: &Library) -> Unit {
    let mut u = b.unit(e, tf, armed.map_or(0.0, |(a, h)| crate::combat::base_mission_damage(&lib.classes[a.class], h)));
    u.vital = Some(armed.map_or([None; 4], |(a, h)| crate::combat::vital_damage(&lib.classes[a.class].sections, &h.sections)));
    u
}

/// Which battle a rule world is built for. The three builders were once three copies; what each leaves out is kept as it was.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum WorldKind {
    /// A campaign mission: every ship with its damage and vital sections, the bases, belts, animals and its custom mines.
    Mission,
    /// A skirmish map's script: ships without damage/vital sections, no bases (PROVISIONAL: a thinner unit list than the mission's,
    /// kept as is), the mines under their own owner.
    Skirmish,
    /// The dev console's one-off list: a mission's without sunk-ship memory and without mines.
    Console,
}

/// The unit list a script sees: ships, remembered sunk ships, animals, bases, rocks, mines, then the script's group tags.
pub(super) fn build_world(
    script: &mut Script,
    kind: WorldKind,
    ships: &Query<RuleShip>,
    animals: &Query<(Entity, &mut crate::environment::SpaceAnimal, &mut Transform, &mut Visibility), Without<Ship>>,
    rocks: &Query<(Entity, &Transform, &Asteroid), (Without<Ship>, Without<crate::environment::SpaceAnimal>)>,
    fx: &apply::FxParam,
    lib: &Library,
) -> Vec<Unit> {
    let mut world = units(ships, lib, kind != WorldKind::Skirmish);
    if kind != WorldKind::Console {
        script.keep_sunk(&mut world);
    }
    world.extend(animals.iter().map(|(e, a, tf, _)| a.unit(e, tf)));
    if kind != WorldKind::Skirmish {
        world.extend(fx.bases.iter().map(|(e, b, tf, armed)| base_unit(b, e, tf, armed, lib)));
    }
    world.extend(rocks.iter().map(|(e, tf, a)| a.unit(e, tf)));
    match kind {
        // Not retail: a mission's mines answer to an explicit group (their world-object ids) only, never to their owner's name: a faction
        // whose ships are all gone is destroyed although its mines rest, and owner-wide orders do not reach them.
        WorldKind::Mission => world.extend(fx.mines.iter().map(|(e, tf, m, _)| Unit { owner: GroupName::default(), ..m.unit(e, tf) })),
        WorldKind::Skirmish => world.extend(fx.mines.iter().map(|(e, tf, m, _)| m.unit(e, tf))),
        WorldKind::Console => {}
    }
    script.tag(&mut world);
    world
}

/// The ships as rule units; `rich`: with their damage and vital sections (a skirmish script's units carry neither).
fn units(ships: &Query<RuleShip>, lib: &Library, rich: bool) -> Vec<Unit> {
    ships
        .iter()
        .map(|RuleShipReadOnlyItem { entity: e, ship, transform: tf, health: h, mission: m, primary, interp, cloak, .. }| {
            let mut u = Unit {
                name: m.name.as_str().into(),
                owner: m.owner.as_str().into(),
                alive: !h.destroyed,
                visible: m.visible,
                primary,
                flag: m.flag.clone(),
                attack: m.attack.clone(),
                heading: ship.heading,
                speed: Some(ship.speed.abs()),
                cloaked: cloak.is_some_and(|c| c.state == crate::cloak::State::Full),
                ..Unit::new(e, m.id, sim_pos(tf, interp))
            };
            if rich {
                let class = &lib.classes[ship.class];
                let secs =
                    if h.sections.is_empty() { 0.0 } else { h.sections.iter().zip(&class.sections).map(|(x, c)| 1.0 - x / c.hp.max(1.0)).sum::<f32>() / h.sections.len() as f32 };
                u.damage = (1.0 - h.core / class.core_hp).max(secs).clamp(0.0, 1.0);
                u.vital = Some(crate::combat::vital_damage(&class.sections, &h.sections));
            }
            u
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
    // The script holds the battle's alliance table while it runs: its captures, 'Set alliance' and effects read the same table.
    if let Some(t) = extra.alliances.as_deref_mut() {
        mission.script.lend(t);
    }
    let input = std::mem::take(&mut *input);
    // Tutorials and mods' custom missions run the same interpreter but do not count toward the retail campaign.
    if crate::campaign::is_campaign(&mission.id) && !extra.cheats.as_ref().is_some_and(|c| c.used) {
        extra.campaign_points.record(&mission.id, mission.script.score);
    }
    let opening = mission.opening.is_some();
    let dt = time.delta_secs();
    crate::smoke::dock(&cfg, mission, &mut extra.dock_smoke_skipped, &mut ships);
    let world = build_world(&mut mission.script, WorldKind::Mission, &ships, &animals, &rocks, &extra.fx, &lib);
    let owner_of = |e: Entity| world.iter().find(|u| u.e == e).map(|u| u.owner.to_string());
    for h in hits.read() {
        mission.script.record_hit(owner_of(h.shooter).as_deref(), h.victim);
    }
    for fr in fired.read() {
        mission.script.record_fire(fr.shooter, fr.target);
    }
    for c in extra.captured.read() {
        mission.script.record_capture(&c.by, &c.victim, c.unit, extra.fx.bases.contains(c.unit));
    }
    let script = &mut mission.script;
    for u in world.iter().filter(|u| rocks.contains(u.e)) {
        script.belt.entry(u.e).or_insert_with(|| std::iter::once(u.owner.to_string()).chain(u.groups.iter().map(ToString::to_string)).collect());
    }
    for d in killed.read() {
        if let Some(o) = owner_of(d.by) {
            // A destroyed rock is one the belt remembers (it may already be gone from the unit list).
            let rock = script.belt.contains_key(&d.victim);
            // A sunk ship is a kill for the player who dealt the last damage (`combat::deaths`); a hull of the shooter's own player is not.
            // PROVISIONAL: retail's treatment of friendly fire is not measured (the Game Summary does not credit it either).
            if !rock && owner_of(d.victim).is_some_and(|v| v == o) {
                continue;
            }
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
    script.in_nebula = extra.in_nebula.as_ref().map(|n| n.0.clone()).unwrap_or_default();
    let (fxs, voice) = script.step(&world, dt, opening);
    if let Some(file) = voice {
        apply::spawn_voice(&mut commands, &assets, &file);
    }
    let mut env = extra.fx.env(&mut mission.script, &mission.data["paths"], &lib, Some(&mut orbit), true);
    apply::apply(fxs, &mut env, &mut commands, &mut ships, &mut animals);
    if let Some(t) = extra.alliances.as_deref_mut() {
        mission.script.lend(t);
    }
    if let Some(values) = mission.script.campaign_commit()
        && let Some(handed_on) = extra.campaign_values.as_mut()
    {
        handed_on.commit(&mission.id, values);
    }
    // Mission 1's refit task: the scripted smokes dock with the weapons tender once it is in range (docking itself is docking.rs).
    // `world` is last frame-start's snapshot: good enough for this and the dev autopilot.
    mission.script.hint.clear();
    if let (Some(p), Some(t)) = (world.iter().find(|u| u.primary && u.alive), world.iter().find(|u| u.name == "RLS Bounty")) {
        let wants = mission.script.task_active("Dock With Tender");
        let near = extra.sensors.in_sight(p.e, t.e, Some(mission)) && !mission.script.docked.contains(&(p.e, t.e));
        if near && wants && matches!(cfg.script.as_deref(), Some("mission" | "dock")) {
            extra.fx.dock.write(DockMsg::Dock { docker: None, dockee: t.e });
        }
    }
    // The frame's snapshot, for the dev autopilot (smoke/autopilot.rs), which runs right after this system.
    extra.snapshot.0 = world;
}

#[cfg(test)]
mod tests;
