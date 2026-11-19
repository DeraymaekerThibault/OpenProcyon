//! ECS side of the rule interpreter: dispatch of `Script` effects, mission AI, mission UI, music and NIS camera.
mod autopilot;
mod mission_ai;
mod presentation;

use crate::Nearest;

use bevy::audio::Volume;

use bevy::ecs::query::QueryData;

use bevy::prelude::*;

use serde_json::Value;

use super::*;
pub use mission_ai::leash;
use mission_ai::{follow_path, leash_player, mission_ai, path_points};
pub use presentation::Ui;
pub(super) use presentation::{EventFx, event_effect};
use presentation::{music, nis_camera, presentation, ui};

use crate::campaign::{Mission, MissionBase, MissionShip, ObjectiveText};

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
    dock: MessageWriter<'w, DockMsg>,
    /// Non-empty while the player's ship is running a dock sequence.
    docking_player: Query<'w, 's, (), (With<PlayerShip>, With<crate::docking::Docker>)>,
    /// Non-empty while the player's ship is sailing to pick up a tow.
    towing_player: Query<'w, 's, (), (With<PlayerShip>, With<crate::docking::Tower>)>,
    /// Spawned lifeboats (pre-placed ones carry a `MissionShip` and are rule units).
    lifeboats: Query<'w, 's, (&'static Transform, &'static crate::lifeboats::Lifeboat), (With<Ship>, Without<MissionShip>)>,
    /// The player's guns (the dev autopilot fires mission 9's Star Mortar).
    player_guns: Query<'w, 's, &'static crate::combat::GunStates, With<PlayerShip>>,
    /// Island and base collision: the dev autopilot's line-of-sight test for a firing position.
    land: Query<'w, 's, (&'static crate::collision::Obstacle, Option<&'static Visibility>), (Without<Ship>, Without<Asteroid>, Without<crate::environment::SpaceAnimal>)>,
    /// Whether the dev autopilot last ordered its fleet to run (formation held, cease fire).
    fleet_fleeing: Local<'s, bool>,
    /// The dev autopilot's memory between frames.
    pilot: Local<'s, autopilot::State>,
    board: MessageWriter<'w, crate::boarding::BoardMsg>,
    captured: MessageReader<'w, 's, crate::boarding::Captured>,
    asteroids: MessageWriter<'w, crate::combat::AsteroidSetup>,
    meteors: MessageWriter<'w, crate::nebula::MeteorRoute>,
    storm_shift: Res<'w, crate::nebula::StormShift>,
    campaign_points: ResMut<'w, crate::campaign::CampaignPoints>,
    sensors: Res<'w, crate::environment::Sensors>,
    voices: Query<'w, 's, Entity, With<crate::briefing::Voice>>,
    fxlib: Option<Res<'w, crate::fx::FxLib>>,
    event_fx: Query<'w, 's, (Entity, &'static EventFx)>,
    sfx: Option<ResMut<'w, Messages<crate::audio::Sfx>>>,
    toggles: Option<ResMut<'w, crate::panels::Toggles>>,
    detonate: MessageWriter<'w, crate::combat::Detonate>,
    dragons: Query<'w, 's, &'static mut crate::dragons::Dragon>,
    /// `--script dock` skipped the briefing already.
    dock_smoke_skipped: Local<'s, bool>,
}

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
    app.insert_resource(crate::campaign::CampaignPoints::load())
        .init_resource::<RuleInput>()
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
}

fn units(ships: &Query<RuleShip>, lib: &Library) -> Vec<Unit> {
    ships
        .iter()
        .map(|RuleShipReadOnlyItem { entity: e, ship, transform: tf, health: h, mission: m, primary, .. }| {
            let class = &lib.classes[ship.class];
            let secs =
                if h.sections.is_empty() { 0.0 } else { h.sections.iter().zip(&class.sections).map(|(x, c)| 1.0 - x / c.hp.max(1.0)).sum::<f32>() / h.sections.len() as f32 };
            Unit {
                name: m.name.clone(),
                owner: m.owner.clone(),
                alive: !h.destroyed,
                visible: m.visible,
                primary,
                damage: (1.0 - h.core / class.core_hp).max(secs).clamp(0.0, 1.0),
                flag: m.flag.clone(),
                attack: m.attack.clone(),
                ..Unit::new(e, m.id, tf.translation)
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
    mut target: ResMut<Target>,
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
    if crate::campaign::is_campaign(&mission.id) {
        extra.campaign_points.record(&mission.id, mission.script.score);
    }
    let opening = mission.opening.is_some();
    let dt = time.delta_secs();
    crate::smoke::dock(&cfg, mission, &mut extra.dock_smoke_skipped, &mut ships);
    let mut world = units(&ships, &lib);
    world.extend(animals.iter().map(|(e, a, tf, _)| a.unit(e, tf)));
    world.extend(extra.bases.iter_mut().map(|(e, b, tf, armed)| b.unit(e, tf, armed.map_or(0.0, |(a, h)| crate::combat::base_mission_damage(&lib.classes[a.class], h)))));
    world.extend(rocks.iter().map(|(e, tf, a)| a.unit(e, tf)));
    mission.script.tag(&mut world);
    let owner_of = |e: Entity| world.iter().find(|u| u.e == e).map(|u| u.owner.clone());
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
        script.belt.entry(u.e).or_insert_with(|| std::iter::once(u.owner.clone()).chain(u.groups.iter().cloned()).collect());
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
    // Not during mission 1's scripted opening (briefing.rs): the player sails the rules' arrival path into the fleet meanwhile.
    if cfg.script.as_deref() == Some("mission") && mission.opening.is_none() {
        autopilot::drive(&mut commands, mission, &world, &mut extra, &mut ships, &mut target, &rocks, &lib);
    }
}

/// Applies the effects a `Script::step` returned to the world: ship orders, flags, docking and tow messages, effects, the orbit camera.
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
                let path = path_points(paths, &path);
                extra.asteroids.write(crate::combat::AsteroidSetup { units, path, closest, mode, speed, tumble });
            }
            Fx::Path { units, path, closest, looping } => follow_path(ships, animals, &units, &path_points(paths, &path), closest, looping),
            Fx::MeteorPath { group, path, closest, mode } => {
                let path = path_points(paths, &path);
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
            Fx::Event(set, name, on) => event_effect(commands, extra.event_fx.iter(), extra.fxlib.as_deref(), &mission.script.point_sets, &set, &name, on),
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
            Fx::Throttle(u, v) => each_ship(ships, u, |s| s.ship.cap = v),
            // A ship just handed to the human (Captain_Human earlier in the same rule; the primary handoff itself is queued) counts as
            // the player's: mission 9's "Dark Matter Ignited" gives back the Longboat, then sets the group's throttle to 0. As a cap
            // that would have left the Longboat unable to move for the rest of the mission.
            Fx::Speed(u, v) => each_ship(ships, u, |s| {
                if !s.primary && !s.mission.human {
                    s.ship.cap = v;
                } else if v <= 0.0 {
                    s.ship.order = 0;
                    s.ship.waypoints.clear();
                }
            }),
            Fx::Destroy(u) => each_ship(ships, u, |s| {
                s.health.core = 0.0;
                s.health.destroyed = true;
                *s.visibility = Visibility::Hidden;
            }),
            Fx::Teleport(u, p) => {
                for e in u {
                    if let Ok(RuleShipItem { ship: mut sh, transform: mut tf, .. }) = ships.get_mut(e) {
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
                    if let Ok(RuleShipItem { mission: mut ms, .. }) = ships.get_mut(d) {
                        ms.stop_at_end = false;
                    }
                    if let Some(t) = dockees.first() {
                        extra.dock.write(DockMsg::Dock { docker: Some(d), dockee: *t });
                    }
                }
            }
            Fx::Tow(tower, towee) => {
                let at = |e| ships.get(e).ok().map(|s| (s.transform.translation, lib.classes[s.ship.class].radius));
                if let (Some((p, rp)), Some((q, rq))) = (at(tower), at(towee))
                    && crate::docking::tow_in_range(p, q, rp, rq)
                {
                    mission.script.set_tow(tower, towee, true); // tugs placed alongside their tow (missions 1, 4, 5, 13)
                } else {
                    // Mission 10's Plunder starts ~700 units from the merchant it tows. PROVISIONAL: the original's approach is not traced.
                    extra.dock.write(DockMsg::TowStart { tower: Some(tower), towee });
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
            // Ships only: bases, rocks and animals have no flag.
            Fx::Lifeboats(u, on) => each_ship(ships, u, |s| s.mission.create_lifeboats = on),
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
                each_dragon(&mut extra.dragons, &u, |d| d.stance = st);
                each_ship(ships, u, |s| s.mission.stance = st);
            }
            Fx::Owner(u, o) => {
                for e in u {
                    if let Ok((_, mut base, ..)) = extra.bases.get_mut(e) {
                        base.owner = o.clone();
                    }
                    if let Ok((_, mut animal, ..)) = animals.get_mut(e) {
                        animal.owner = o.clone();
                    }
                    if let Ok(RuleShipItem { mission: mut ms, .. }) = ships.get_mut(e) {
                        ms.owner = o.clone();
                    }
                }
            }
            Fx::Visible(u, v) => {
                for e in u {
                    if extra.bases.contains(e) {
                        commands.entity(e).insert(if v { Visibility::Inherited } else { Visibility::Hidden });
                    }
                    if let Ok((_, mut animal, _, mut vis)) = animals.get_mut(e) {
                        animal.visible = v;
                        *vis = if v { Visibility::Inherited } else { Visibility::Hidden };
                    }
                    if let Ok(RuleShipItem { mission: mut ms, visibility: mut vis, .. }) = ships.get_mut(e) {
                        ms.visible = v;
                        *vis = if v { Visibility::Inherited } else { Visibility::Hidden };
                    }
                }
            }
            Fx::Invuln(u, v) => each_ship(ships, u, |s| s.mission.invuln = v),
            Fx::Flag(u, tex) => each_ship(ships, u, |s| s.mission.flag.clone_from(&tex)),
            Fx::Label(u, text) => {
                for e in u {
                    commands.entity(e).insert(crate::ShipName(text.clone()));
                }
            }
            Fx::Banner(u, bn) => each_ship(ships, u, |s| s.mission.banner.clone_from(&bn)),
            Fx::Ram(u, g) => each_ship(ships, u, |s| {
                s.mission.ram = true;
                add_once(&mut s.mission.attack, &g);
            }),
            Fx::Attack(u, g) => {
                each_dragon(&mut extra.dragons, &u, |d| add_once(&mut d.attack, &g));
                each_ship(ships, u, |s| add_once(&mut s.mission.attack, &g));
            }
            // Only script-given orders are cleared: the human's own course and throttle stay.
            Fx::Clear(u) => {
                each_dragon(&mut extra.dragons, &u, |d| {
                    d.attack.clear();
                    d.hold = false;
                });
                each_ship(ships, u, |s| {
                    s.mission.attack.clear();
                    s.mission.ram = false;
                    s.mission.looping = None;
                    if !s.primary {
                        s.ship.waypoints.clear();
                        s.ship.order = 0;
                        s.ship.goal = None;
                    }
                });
            }
            Fx::Damage(u, p) => each_ship(ships, u, |s| crate::combat::fraction_hit(&lib.classes[s.ship.class], &mut s.health, p, s.ship.side, s.mission.invuln)),
            Fx::Hold(u, on) => {
                each_dragon(&mut extra.dragons, &u, |d| d.hold = on);
                if on {
                    each_ship(ships, u, |s| {
                        s.ship.waypoints.clear();
                        s.ship.order = 0;
                    });
                }
            }
            Fx::ResetHits(u) => {
                for e in u {
                    mission.script.hits.remove(&e);
                    mission.script.hits_by.retain(|k, _| k.1 != e);
                }
            }
            Fx::Border(u, zone) => {
                each_dragon(&mut extra.dragons, &u, |d| d.border.clone_from(&zone));
                each_ship(ships, u, |s| s.mission.border.clone_from(&zone));
            }
            Fx::ReturnPoint(u, p) => {
                each_dragon(&mut extra.dragons, &u, |d| d.home = Some(p));
                each_ship(ships, u, |s| s.mission.return_point = Some(p));
            }
            Fx::Towable(u, on) => marker(commands, u, !on, crate::docking::NoTow),
            Fx::Movable(u, on) => marker(commands, u, !on, crate::collision::Pinned),
            Fx::Collidable(u, on) => marker(commands, u, !on, crate::collision::Ghost),
            Fx::Captain(u, human) => each_ship(ships, u, |s| s.mission.human = human),
            Fx::Cloak(u, on, ai_override) => {
                for e in u {
                    if ships.contains(e) {
                        // 'AI Can Override' is always stored. TRUE starts a cloak when one is allowed (never within 5 s of the ship's
                        // creation: re-fire it later); FALSE stops one that is closing or closed. A ship without a keeper yet (world init)
                        // starts in the ordered state. PROVISIONAL: that last case is the remake's.
                        commands.queue(move |w: &mut World| {
                            let side = w.get::<Ship>(e).map(|s| s.side);
                            match (w.get_mut::<crate::cloak::Cloak>(e), side) {
                                (Some(mut c), Some(side)) => {
                                    c.ai_modify = ai_override;
                                    if on {
                                        c.start(side);
                                    } else {
                                        c.stop();
                                    }
                                }
                                _ => {
                                    if let Ok(mut x) = w.get_entity_mut(e) {
                                        let mut c = crate::cloak::Cloak::new(on);
                                        c.ai_modify = ai_override;
                                        x.insert(c);
                                    }
                                }
                            }
                        });
                    }
                }
            }
            Fx::WarningShots(u, on) => each_ship(ships, u, |s| s.mission.warning_shots = on),
            Fx::Current(path) => {
                commands.spawn((Scenario, crate::environment::Current::new(path)));
            }
            Fx::DarkMatter(volume) => {
                // The blast starts at the volume's centre (game x, y) at the ships' height.
                if let Some(c) = mission.script.volume(&volume).centroid() {
                    extra.detonate.write(crate::combat::Detonate { bullet: "Bullet_DarkMatterExplosion".into(), at: Vec3::new(c.x, 0.0, -c.y), shooter: None, owner: None });
                }
            }
            Fx::Longboat(u) => {
                // Launched from the human's command ship: alongside it, same heading.
                let from = ships.iter().find(|s| s.primary).map(|s| (s.transform.translation, s.ship.heading, lib.classes[s.ship.class].radius));
                if let Some((p, heading, r)) = from {
                    for (k, e) in u.into_iter().enumerate() {
                        if let Ok(RuleShipItem { ship: mut sh, transform: mut tf, .. }) = ships.get_mut(e) {
                            let side = Vec3::new(heading.cos(), 0.0, -heading.sin());
                            tf.translation = p + side * (r + 8.0 + 6.0 * k as f32);
                            sh.heading = heading;
                            sh.waypoints.clear();
                            sh.speed = 0.0;
                        }
                    }
                }
            }
            Fx::DragonStance(u, st) => each_dragon(&mut extra.dragons, &u, |d| d.stance = st),
            Fx::DragonThreshold(u, t) => each_dragon(&mut extra.dragons, &u, |d| d.threshold = t),
            Fx::Orbit { unit, dist, angle } => {
                orbit.focus = unit;
                orbit.dist = dist;
                let heading = unit.and_then(|e| ships.get(e).ok()).or_else(|| ships.iter().find(|s| s.primary)).map_or(0.0, |s| s.ship.heading);
                orbit.yaw = heading + angle.to_radians();
            }
        }
    }
}

/// Runs `f` on each of `units` that is a mission ship (bases, rocks and animals are skipped).
fn each_ship(ships: &mut Query<RuleShip>, units: Vec<Entity>, mut f: impl FnMut(&mut RuleShipItem)) {
    for e in units {
        if let Ok(mut s) = ships.get_mut(e) {
            f(&mut s);
        }
    }
}

/// Runs `f` on each of `units` that is a dragon.
fn each_dragon(dragons: &mut Query<&mut crate::dragons::Dragon>, units: &[Entity], mut f: impl FnMut(&mut crate::dragons::Dragon)) {
    for &e in units {
        if let Ok(mut d) = dragons.get_mut(e) {
            f(&mut d);
        }
    }
}

/// Adds or removes a marker component on each of `units`.
fn marker<C: Component + Clone>(commands: &mut Commands, units: Vec<Entity>, on: bool, c: C) {
    for e in units {
        if on {
            commands.entity(e).insert(c.clone());
        } else {
            commands.entity(e).remove::<C>();
        }
    }
}

fn add_once(list: &mut Vec<String>, g: &str) {
    if !list.iter().any(|x| x == g) {
        list.push(g.to_string());
    }
}

#[cfg(test)]
mod tests;
