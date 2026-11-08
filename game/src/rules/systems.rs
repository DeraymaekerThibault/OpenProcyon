//! ECS side of the rule interpreter: dispatch of `Script` effects, mission AI, mission UI, music and NIS camera.
use crate::Nearest;
use bevy::audio::Volume;
use bevy::ecs::query::QueryData;
use bevy::prelude::*;
use serde_json::Value;

use super::*;
use crate::campaign::{Mission, MissionBase, MissionShip, ObjectiveText};
use crate::combat::{Asteroid, Destroyed, Fired, Health, Hit, Library, Target};
use crate::docking::DockMsg;
use crate::{Config, Orbit, PlayerShip, Scenario, Ship, Side, game_to_bevy};

// ---------------------------------------------------------------------------------------------------------------- ECS side

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
pub(super) struct EventFx(String);

/// 'Create/Release Event Effect': removes the effect's old copies and, when `on`, plays it at every point of the set.
pub(super) fn event_effect<'a>(
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
    /// Whether the dev autopilot last ordered its fleet to run (formation held, cease fire).
    fleet_fleeing: Local<'s, bool>,
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
#[derive(Component)]
struct MusicFade {
    cur: f32,
    to: f32,
    rate: f32,
    kill: bool,
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
            (update.in_set(crate::BattleSet::Rules), (mission_ai, leash_player).chain().in_set(crate::BattleSet::Ai))
                .run_if(in_state(Screen::Battle))
                .run_if(resource_exists::<Mission>),
        )
        .add_systems(
            Update,
            (
                capture_input.before(crate::briefing::update),
                crate::smoke::screenshot_hooks,
                crate::smoke::autoplay_end,
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

fn follow_path(
    ships: &mut Query<RuleShip>,
    animals: &mut Query<(Entity, &mut crate::environment::SpaceAnimal, &mut Transform, &mut Visibility), Without<Ship>>,
    units: &[Entity],
    points: &[Vec3],
    closest: bool,
    looping: bool,
) {
    for e in units {
        if let Ok((_, mut animal, tf, _)) = animals.get_mut(*e) {
            animal.follow(points, tf.translation, closest, looping);
            continue;
        }
        let Ok(RuleShipItem { ship: mut sh, transform: tf, mission: mut ms, .. }) = ships.get_mut(*e) else { continue };
        let full = points.to_vec(); // Continuous fleet followers supply leader-relative slots, not cloned path offsets.
        let from = if closest { (0..full.len()).nearest_to(tf.translation, |a| full[*a]).unwrap_or(0) } else { 0 };
        sh.throttle = None;
        sh.waypoints = full[from..].to_vec();
        sh.order = 2;
        sh.goal = None;
        ms.attack.clear();
        ms.stop_at_end = !looping;
        ms.looping = looping.then_some(full);
    }
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
    // Tutorials run the same interpreter but do not count toward the campaign.
    if mission.id.starts_with("mission") {
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
            Fx::Path { units, path, closest, looping } => follow_path(&mut ships, &mut animals, &units, &path_points(paths, &path), closest, looping),
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
            Fx::Event(set, name, on) => event_effect(&mut commands, extra.event_fx.iter(), extra.fxlib.as_deref(), &mission.script.point_sets, &set, &name, on),
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
            Fx::Throttle(u, v) => each_ship(&mut ships, u, |s| s.ship.cap = v),
            // A ship just handed to the human (Captain_Human earlier in the same rule; the primary handoff itself is queued) counts as
            // the player's: mission 9's "Dark Matter Ignited" gives back the Longboat, then sets the group's throttle to 0. As a cap
            // that would have left the Longboat unable to move for the rest of the mission.
            Fx::Speed(u, v) => each_ship(&mut ships, u, |s| {
                if !s.primary && !s.mission.human {
                    s.ship.cap = v;
                } else if v <= 0.0 {
                    s.ship.order = 0;
                    s.ship.waypoints.clear();
                }
            }),
            Fx::Destroy(u) => each_ship(&mut ships, u, |s| {
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
            Fx::Lifeboats(u, on) => each_ship(&mut ships, u, |s| s.mission.create_lifeboats = on),
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
                each_ship(&mut ships, u, |s| s.mission.stance = st);
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
            Fx::Invuln(u, v) => each_ship(&mut ships, u, |s| s.mission.invuln = v),
            Fx::Flag(u, tex) => each_ship(&mut ships, u, |s| s.mission.flag.clone_from(&tex)),
            Fx::Label(u, text) => {
                for e in u {
                    commands.entity(e).insert(crate::ShipName(text.clone()));
                }
            }
            Fx::Banner(u, bn) => each_ship(&mut ships, u, |s| s.mission.banner.clone_from(&bn)),
            Fx::Ram(u, g) => each_ship(&mut ships, u, |s| {
                s.mission.ram = true;
                add_once(&mut s.mission.attack, &g);
            }),
            Fx::Attack(u, g) => {
                each_dragon(&mut extra.dragons, &u, |d| add_once(&mut d.attack, &g));
                each_ship(&mut ships, u, |s| add_once(&mut s.mission.attack, &g));
            }
            // Only script-given orders are cleared: the human's own course and throttle stay.
            Fx::Clear(u) => {
                each_dragon(&mut extra.dragons, &u, |d| {
                    d.attack.clear();
                    d.hold = false;
                });
                each_ship(&mut ships, u, |s| {
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
            Fx::Damage(u, p) => each_ship(&mut ships, u, |s| s.health.scale(1.0 - p)),
            Fx::Hold(u, on) => {
                each_dragon(&mut extra.dragons, &u, |d| d.hold = on);
                if on {
                    each_ship(&mut ships, u, |s| {
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
                each_ship(&mut ships, u, |s| s.mission.border.clone_from(&zone));
            }
            Fx::ReturnPoint(u, p) => {
                each_dragon(&mut extra.dragons, &u, |d| d.home = Some(p));
                each_ship(&mut ships, u, |s| s.mission.return_point = Some(p));
            }
            Fx::Towable(u, on) => marker(&mut commands, u, !on, crate::docking::NoTow),
            Fx::Movable(u, on) => marker(&mut commands, u, !on, crate::collision::Pinned),
            Fx::Collidable(u, on) => marker(&mut commands, u, !on, crate::collision::Ghost),
            Fx::Captain(u, human) => each_ship(&mut ships, u, |s| s.mission.human = human),
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
            Fx::WarningShots(u, on) => each_ship(&mut ships, u, |s| s.mission.warning_shots = on),
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
        mission.script.crew_screen = false; // keep the default crew and arms (missions 3+ open the screen after their first NIS)
        // PROCYON_AUTOINVULN also keeps the player's fleet from being boarded: a pirate captured mission 4's flagship while the
        // invulnerable bot fought it, leaving the run on a stranded stand-in ship.
        if std::env::var_os("PROCYON_AUTOINVULN").is_some()
            && let Some(owner) = world.iter().find(|u| u.primary).map(|u| u.owner.clone())
        {
            for s in ships.iter().filter(|s| s.mission.owner == owner) {
                extra.board.write(crate::boarding::BoardMsg::Boardable(s.entity, false));
            }
        }
        autopilot_dock(&mission.script, &world, &mission.data, &mut extra);
        autopilot_tow(&mission.script, &world, &mut extra);
        // While running, the fleet follows in formation with cease fire, as a player would order from the fleet bar: left to their
        // own captains the escorts stay and fight a battle they cannot win (mission 3's ambush). Released when the run ends.
        let flee = fleeing(&mission.script);
        if flee != *extra.fleet_fleeing {
            *extra.fleet_fleeing = flee;
            commands.queue(move |w: &mut World| {
                let owner = w.resource::<crate::ship::Fleets>().local.clone();
                let Some(fleet) = w.resource::<crate::ship::Fleets>().groups.get(&owner).cloned() else { return };
                let kind = if fleet.formation == crate::ship::Formation::None { crate::ship::Formation::Column } else { fleet.formation };
                crate::ship::set_formation(w, &owner, Some(kind), flee);
                if fleet.cease_fire != flee {
                    crate::ship::fleet_help(w, &owner, false); // toggles cease fire
                }
            });
        }
        // A ship that is docking or sailing to a tow steers itself (docking.rs); steering it as well would fight the approach.
        if extra.docking_player.is_empty() && extra.towing_player.is_empty() {
            let from = world.iter().find(|u| u.primary).map_or(Vec3::ZERO, |u| u.pos);
            // Armed bases that still have a live mission section, with the world and base-local position of the one nearest the player.
            let base_ids: Vec<(Entity, Vec3, Vec3)> = extra
                .bases
                .iter()
                .filter_map(|(e, _, tf, armed)| {
                    let p = armed.and_then(|(a, h)| crate::combat::live_tower_point(&lib.classes[a.class], h, tf, from))?;
                    Some((e, p, tf.to_matrix().inverse().transform_point3(p)))
                })
                .collect();
            let boats: Vec<(Vec3, &str)> = extra.lifeboats.iter().map(|(tf, b)| (tf.translation, b.owner.as_str())).collect();
            // The player's mortar mounts (mission 9's StarHammer carries Star and Nova mortars on different arcs).
            let mortars: Vec<Mortar> = ships
                .iter()
                .find(|s| s.primary)
                .and_then(|s| {
                    let guns = extra.player_guns.get(s.entity).ok()?;
                    let mounts = lib.classes[s.ship.class].mounts.iter().enumerate().filter(|(i, _)| guns.gun(*i).contains("Mortar"));
                    Some(
                        mounts
                            .map(|(i, m)| Mortar { bank: m.bank, range: lib.guns.get(guns.gun(i)).map_or(0.0, |d| d.range[2]), rot: s.transform.rotation * m.rot, arc: m.arc })
                            .collect(),
                    )
                })
                .unwrap_or_default();
            autopilot(&mission.script, &world, &mission.data, &rocks, &base_ids, &boats, &mut ships, &mut target, &mortars);
        }
    }
}

/// How far ahead the dev autopilot sets its waypoint while leaving a volume (PROVISIONAL).
const LEAVE_STEP: f32 = 400.0;
/// One of the player's mortar mounts as the dev autopilot sees it: its bank, maximum range, world rotation and arc (radians).
struct Mortar {
    bank: usize,
    range: f32,
    rot: Quat,
    arc: f32,
}

/// Fraction of the mortar's maximum range the dev autopilot fires from (PROVISIONAL).
const LOB_REACH: f32 = 0.8;
/// Distance at which the dev autopilot counts the objective point as reached (PROVISIONAL).
const POINT_REACHED: f32 = 150.0;

/// How far from a defended objective point the dev autopilot looks for its attackers (PROVISIONAL).
const DEFEND_RADIUS: f32 = 600.0;

/// True for an objective text that tells the player to guard something (dev autopilot heuristic, PROVISIONAL: English task texts).
fn defend_task(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    ["defend", "protect", "keep the pirates from"].iter().any(|w| t.contains(w))
}

/// Where the objective marker is: a ship it follows ('Set Current Objective Point On Ship', mission 9's St Roch), else the point.
fn objective_at(sc: &Script, data: &Value) -> Option<Vec3> {
    sc.point_pos.or_else(|| sc.point.map(|i| game_to_bevy(&data["points"][i]["Position"])))
}

/// A task to dock, by its name or its text (mission 9's "M09E - Return To Ship" reads "Return to and dock with Command Ship").
fn is_dock_task(t: &Task) -> bool {
    t.name.to_ascii_lowercase().contains("dock") || t.text.to_ascii_lowercase().contains("dock")
}

/// The dev autopilot is running for the objective point instead of fighting (see `flee_task`).
fn fleeing(sc: &Script) -> bool {
    (sc.point.is_some() || sc.point_pos.is_some()) && sc.tasks.iter().any(|t| t.active && !t.complete && flee_task(&t.text))
}

/// True for an objective text that tells the player to get away rather than fight (dev autopilot heuristic, PROVISIONAL: English task texts).
fn flee_task(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    ["take cover", "avoid", "escape", "flee", "alive", "retreat"].iter().any(|w| t.contains(w))
}

/// `--script mission`: for any active task named `Tow<name>` (mission 2's `TowCoventry`), orders a tow of the ship whose name ends with
/// `<name>` (as the menu's Tow would) unless the player already tows or is on the way; once linked, the autopilot sails for the objective point.
/// The bot's progress check: where the ship was at the last check, and a detour it is on.
#[derive(Default)]
struct Stuck {
    since: f32,
    at: Vec3,
    detour: Option<(f32, Vec3)>,
    side: f32,
    /// Detours since the ship last got nearer its goal, and the nearest it got.
    tries: u32,
    best: f32,
}

static STUCK: std::sync::Mutex<Option<Stuck>> = std::sync::Mutex::new(None);

/// PROVISIONAL bot tuning: a ship heading for a goal farther than STUCK_GOAL that moved less than STUCK_MOVE in STUCK_SECS is
/// pressed against land (missions 2 and 3 wedge on islands). It backs off to one side, DETOUR_STEP further each time it wedges again
/// without getting PROGRESS nearer the goal, so it works along an island chain; after MAX_TRIES it tries the other side. A detour
/// lasts until reached or its length at DETOUR_SPEED units a second has passed.
const STUCK_SECS: f32 = 20.0;
const STUCK_MOVE: f32 = 40.0;
const STUCK_GOAL: f32 = 150.0;
const DETOUR_STEP: f32 = 250.0;
const DETOUR_SPEED: f32 = 4.0;
const PROGRESS: f32 = 200.0;
const MAX_TRIES: u32 = 4;
/// How long a stalled tower hold sails for its new hold point.
const DETOUR_SECS: f32 = 30.0;

/// PROVISIONAL bot tuning: a tower hold whose target's damage has not changed for HOLD_STALL_SECS moves TOWER_ROUND radians round it.
const HOLD_STALL_SECS: f32 = 30.0;
const TOWER_ROUND: f32 = 1.0;

/// (last call, since when, the damage then) for the tower hold.
static TOWER_STALL: std::sync::Mutex<Option<(f32, f32, f32)>> = std::sync::Mutex::new(None);

/// True once a hold has gone HOLD_STALL_SECS without the target's damage changing; a gap in the calls (the hold was left) starts over.
fn hold_stalled(time: f32, damage: f32, cell: &std::sync::Mutex<Option<(f32, f32, f32)>>) -> bool {
    let mut lock = cell.lock().unwrap_or_else(|e| e.into_inner());
    let (last, since, then) = lock.get_or_insert((time, time, damage));
    if time < *last || time - *last > 2.0 || (damage - *then).abs() > 1e-4 {
        (*since, *then) = (time, damage);
    }
    *last = time;
    if time - *since >= HOLD_STALL_SECS {
        *since = time;
        return true;
    }
    false
}

/// Sends the bot to `d` for DETOUR_SECS, whatever its goal.
fn start_detour(time: f32, d: Vec3, cell: &std::sync::Mutex<Option<Stuck>>) {
    let mut lock = cell.lock().unwrap_or_else(|e| e.into_inner());
    let st = lock.get_or_insert_with(|| Stuck { since: time, at: d, side: 1.0, best: f32::MAX, ..default() });
    st.detour = Some((time + DETOUR_SECS, d));
    info!("autopilot hold stalled; detour to {d:.0}");
}

/// A detour point while the ship is wedged on its way to `goal`, else None (the goal stands).
fn unstick(time: f32, pos: Vec3, goal: Option<Vec3>, cell: &std::sync::Mutex<Option<Stuck>>) -> Option<Vec3> {
    let mut lock = cell.lock().unwrap_or_else(|e| e.into_inner());
    let st = lock.get_or_insert_with(|| Stuck { since: time, at: pos, side: 1.0, best: f32::MAX, ..default() });
    // A new run (game time went back) starts over.
    if time < st.since {
        *st = Stuck { since: time, at: pos, side: 1.0, best: f32::MAX, ..default() };
    }
    if let Some((until, d)) = st.detour {
        if time < until && d.distance(pos) > 40.0 {
            return Some(d);
        }
        st.detour = None;
        st.since = time;
        st.at = pos;
    }
    let goal = goal.filter(|g| g.distance(pos) > STUCK_GOAL)?;
    if pos.distance(st.at) >= STUCK_MOVE {
        st.since = time;
        st.at = pos;
        return None;
    }
    if time - st.since < STUCK_SECS {
        return None;
    }
    let dist = goal.distance(pos);
    if dist < st.best - PROGRESS {
        st.tries = 0;
    }
    st.best = st.best.min(dist);
    st.tries += 1;
    if st.tries > MAX_TRIES {
        st.tries = 1;
        st.side = -st.side;
    }
    let to = (goal - pos).with_y(0.0).normalize_or_zero();
    let side = Vec3::new(-to.z, 0.0, to.x) * st.side;
    let step = DETOUR_STEP * st.tries as f32;
    let d = pos + (side - to * 0.5) * step;
    st.detour = Some((time + step / DETOUR_SPEED, d));
    info!("autopilot stuck at {pos:.0} heading for {goal:.0}; detour to {d:.0}");
    Some(d)
}

/// The ship a `Tow...` task asks the player to tow: the one the task name ends with, else the named ship its text mentions (mission 4's
/// "Tow tender to safety" reads "Tow the RLS Bachanalia to the Rally Point.", #351).
fn task_towee<'a>(t: &super::eval::Task, w: &'a [Unit], player: Entity) -> Option<&'a Unit> {
    let name = t.name.strip_prefix("Tow").filter(|n| !n.is_empty())?;
    let candidates = || w.iter().filter(|u| u.alive && u.e != player && !u.name.is_empty());
    candidates().find(|u| u.name.ends_with(name)).or_else(|| candidates().find(|u| t.text.contains(u.name.as_str())))
}

fn autopilot_tow(sc: &Script, w: &[Unit], extra: &mut Extra) {
    let Some(p) = w.iter().find(|u| u.primary && u.alive) else { return };
    if sc.nis || !extra.towing_player.is_empty() || !extra.docking_player.is_empty() || sc.tows().iter().any(|t| t.0 == p.e) {
        return;
    }
    if let Some(towee) = sc.tasks.iter().filter(|t| t.active && !t.complete).find_map(|t| task_towee(t, w, p.e)) {
        extra.dock.write(DockMsg::TowStart { tower: None, towee: towee.e });
        return;
    }
    // Otherwise a visible ship an unfired rule waits for the player to tow (mission 11's SS Harvester at the black hole).
    let towee = sc
        .wants(w)
        .into_iter()
        .filter_map(|x| match x {
            Want::Tow(e) => w.iter().find(|u| u.e == e && u.visible),
            _ => None,
        })
        .nearest_to(p.pos, |u| u.pos);
    if let Some(towee) = towee {
        extra.dock.write(DockMsg::TowStart { tower: None, towee: towee.e });
    }
}

/// `--script mission`: for any active task named `Dock...` (mission 2's camps), once the player is near the task's objective point
/// (the point carries the task's name), orders the dock with the base nearest that point, as a click on it would. Mission 1's
/// tender has its own hook above.
fn autopilot_dock(sc: &Script, w: &[Unit], data: &Value, extra: &mut Extra) {
    const NEAR: f32 = 600.0;
    const BASE_RANGE: f32 = 400.0;
    let Some(p) = w.iter().find(|u| u.primary && u.alive) else { return };
    if !extra.docking_player.is_empty() || sc.nis || sc.task_active("Dock With Tender") {
        return;
    }
    let dock_task = |t: &&Task| t.active && !t.complete && is_dock_task(t);
    for t in sc.tasks.iter().filter(dock_task) {
        // A task whose objective point carries its name (mission 2's camps): the base nearest that point.
        let Some(at) = data["points"].as_array().and_then(|a| a.iter().find(|x| x["Name"] == t.name.as_str())).map(|x| game_to_bevy(&x["Position"])) else { continue };
        if at.distance(p.pos) > NEAR {
            continue;
        }
        let base = extra.bases.iter().filter(|(_, _, tf, _)| tf.translation.distance(at) < BASE_RANGE).nearest_to(at, |b| b.2.translation);
        if let Some((dockee, ..)) = base {
            extra.dock.write(DockMsg::Dock { docker: None, dockee });
            return;
        }
    }
    // Otherwise whatever an unfired rule waits for the player to dock with, once the player is near it: while a task says to dock, or
    // when the target sits at the objective point (mission 5's "Deliver Supplies to Alamein" is a dock at Alamein).
    let point = objective_at(sc, data);
    if let Some((dockee, at)) = dock_target(sc, w, p.pos).filter(|(_, at)| at.distance(p.pos) < NEAR)
        && (sc.tasks.iter().any(|t| dock_task(&t)) || point.is_some_and(|q| q.distance(at) < BASE_RANGE))
    {
        extra.dock.write(DockMsg::Dock { docker: None, dockee });
    }
}

/// The nearest of the rules' dock targets (`Script::dock_targets`) and where it is.
fn dock_target(sc: &Script, w: &[Unit], from: Vec3) -> Option<(Entity, Vec3)> {
    sc.dock_targets(w).into_iter().filter_map(|e| w.iter().find(|u| u.e == e && u.alive).map(|u| (e, u.pos))).nearest_to(from, |t| t.1)
}

/// The points of a mission path (`paths.<name>`), empty when the mission has no such path.
fn path_points(paths: &Value, name: &str) -> Vec<Vec3> {
    paths[name].as_array().map(|a| a.iter().map(game_to_bevy).collect()).unwrap_or_default()
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

/// `--script mission`: follows the objective point, then closes on the nearest hostile and targets it (smoke test of the rules).
fn autopilot(
    sc: &Script,
    w: &[Unit],
    data: &Value,
    rocks: &Query<(Entity, &Transform, &Asteroid), (Without<Ship>, Without<crate::environment::SpaceAnimal>)>,
    bases: &[(Entity, Vec3, Vec3)],
    lifeboats: &[(Vec3, &str)],
    ships: &mut Query<RuleShip>,
    target: &mut Target,
    mortars: &[Mortar],
) {
    /// Distance the dev autopilot stops from a live gun tower, bow on it (PROVISIONAL; the retail test fires from 100 units).
    const TOWER_HOLD: f32 = 110.0;
    /// Closest the autopilot lets itself creep to a gun tower before backing out (PROVISIONAL).
    const TOWER_MIN: f32 = 70.0;
    if sc.nis || sc.ended.is_some() {
        return;
    }
    let Some(p) = w.iter().find(|u| u.primary && u.alive) else { return };
    // Ships only: whales count as hostile until the reprimand rule (PROVISIONAL default), and chasing them stalled the run after Objective 3.
    let foe = w.iter().filter(|u| u.alive && u.visible && sc.hostile(&u.owner, &p.owner) && ships.contains(u.e)).nearest_to(p.pos, |a| a.pos);
    // An enemy towing a friendly ship away (mission 10's Plunder takes the convoy's lead ship) comes first, wherever it is: the convoy
    // waits until the tow breaks. Only once it is visible: mission 1's hidden Pirate Schooner tows the tender in distress from the start,
    // and chasing it then pulls the player away from the fleet before the opening is over.
    let kidnapper = sc.tows().iter().find_map(|&(tower, towee)| {
        let (t, v) = (w.iter().find(|u| u.e == tower)?, w.iter().find(|u| u.e == towee)?);
        (t.alive && t.visible && sc.hostile(&t.owner, &p.owner) && !sc.hostile(&v.owner, &p.owner)).then_some(t)
    });
    // A task to defend what sits at the objective point (mission 12's "Defend Her Majesty Aboard Parliament"): the enemy closest to that
    // point comes before whatever attacks the player, or the boarders capture it while the bot duels submarines.
    let raider = objective_at(sc, data).filter(|_| sc.tasks.iter().any(|t| t.active && !t.complete && defend_task(&t.text))).and_then(|at| {
        w.iter().filter(|u| u.alive && u.visible && sc.hostile(&u.owner, &p.owner) && ships.contains(u.e)).nearest_to(at, |u| u.pos).filter(|u| u.pos.distance(at) < DEFEND_RADIUS)
    });
    // What the unfired rules wait for (`Script::wants`): a volume to enter or a unit to shoot (mission 5's blockaders) is where to sail
    // once the objective point is reached or there is none (mission 4's and 8's searches). It never outranks the point: optional
    // rules (mission 1's whales) would pull the bot off its course.
    let wants = sc.wants(w);
    let wanted_hit = wants
        .iter()
        .filter_map(|x| match x {
            Want::Hit(e) => w.iter().find(|u| u.e == *e),
            _ => None,
        })
        .nearest_to(p.pos, |u| u.pos);
    let wanted_at = wants
        .iter()
        .filter_map(|x| match x {
            Want::Reach(at) => Some(*at),
            _ => None,
        })
        .chain(wanted_hit.map(|u| u.pos))
        .nearest_to(p.pos, |a| *a);
    // A shell the rules want landed in a volume (mission 9's Dark Matter): close to within the mortar's reach, then fire it at the
    // centroid as a click with the lob cursor would. Gunnery drops the shot once fired; the next follows until the rule fires.
    // A mount whose arc covers the point fires; with none, the ship turns its bow on it (an immobile ship may never manage).
    let range = mortars.iter().map(|m| m.range).fold(0.0, f32::max);
    let covering = |at: Vec3| {
        mortars
            .iter()
            .find(|m| {
                let local = m.rot.inverse() * (at - p.pos).with_y(0.0);
                at.distance(p.pos) < m.range && (-local.x).atan2(-local.z).abs() <= m.arc / 2.0
            })
            .map(|m| m.bank)
    };
    let lob_points: Vec<Vec3> = wants
        .iter()
        .filter_map(|x| match x {
            Want::Lob(at) => Some(*at),
            _ => None,
        })
        .filter(|_| !fleeing(sc) && !mortars.is_empty())
        .collect();
    // The first point (the nearest is listed first) a mount covers, else the nearest one to sail or turn for.
    let lob = lob_points.iter().find_map(|at| covering(*at).map(|bank| (*at, range, Some(bank)))).or_else(|| lob_points.first().map(|at| (*at, range, None)));
    if let Some((at, _, Some(bank))) = lob
        && target.2.shot.is_none()
    {
        target.2.shot = Some((bank, at));
    }
    let kidnapper = kidnapper.or(raider);
    // Hostile bases (mission 2's island gun towers) must be shot down before they can be docked.
    let tower = w
        .iter()
        .filter(|u| u.alive && sc.hostile(&u.owner, &p.owner))
        .filter_map(|u| bases.iter().find(|b| b.0 == u.e).copied())
        .filter(|(_, at, _)| at.distance(p.pos) < 900.0)
        .min_by(|a, b| a.1.distance(p.pos).total_cmp(&b.1.distance(p.pos)));
    target.1 = None; // no click point unless a tower is the target (set below)
    let dock = sc.task_active("Dock With Tender");
    let shoot_rocks = sc.task_active("Destroy Asteroids");
    let rock = rocks.iter().filter(|(_, _, a)| a.group == "Asteroid Belt" && a.hp > 0.0).nearest_to(p.pos, |a| a.1.translation);
    let mut face: Option<f32> = None; // hold position with the bow on this heading (a gun tower in reach)
    // A task that says to run (mission 3's "Take Cover in the Asteroid Field", "Avoid the Pirate Trap"): sail for the objective point and
    // leave the enemy alone. The player cannot sink under PROCYON_AUTOINVULN, so fighting there never ends and the escorts die.
    let flee = fleeing(sc);
    let foe = foe.filter(|_| !flee);
    let tower = tower.filter(|_| !flee);
    let goal = if let (true, Some((e, tf, _))) = (shoot_rocks, rock.filter(|r| r.1.translation.distance(p.pos) < 400.0)) {
        target.0 = Some(e);
        Some(tf.translation)
    } else if let Some((at, range, covered)) = lob {
        // In reach: hold; with no mount covering the point, bow on it (gunnery drops a shot outside the mount's arc).
        let d = at.distance(p.pos);
        let stopped = ships.iter().find(|s| s.primary).is_some_and(|s| s.ship.speed.abs() < 0.1);
        if covered.is_none() && (d <= range * LOB_REACH || (d < range && stopped)) {
            face = Some((-(at.x - p.pos.x)).atan2(-(at.z - p.pos.z)));
        }
        Some(if face.is_some() || covered.is_some() { p.pos } else { at })
    } else if let Some(from) = wants.iter().find_map(|x| match x {
        Want::Leave(at) => Some(*at),
        _ => None,
    }) {
        // Out of the volume, straight away from its centre (mission 11: dragging the galleon clear of the black hole).
        Some(p.pos + (p.pos - from).normalize_or(Vec3::X) * LEAVE_STEP)
    } else if let Some(foe) = kidnapper.filter(|_| !flee).or(foe.filter(|f| f.pos.distance(p.pos) < 700.0)) {
        target.0 = Some(foe.e);
        Some(foe.pos)
    } else if let Some((e, at, local)) = tower {
        // Aim at the tower being approached, as a click on it would: without a click point the guns pick the tower nearest the
        // island centre, which can be the other one, out of the Lancer's arc or range.
        target.0 = Some(e);
        target.1 = Some(local);
        // Straight at the live tower, then hold with the bow on it: only the front gun can target towers, and the ship keeps its heading.
        // Holding creeps forward (order 1), so a ship that closes inside `TOWER_MIN` (roof too steep above, hull against the island)
        // sails back out to the hold distance before it turns bow-on again.
        let at = at.with_y(0.0);
        let off = p.pos - at;
        let d = off.length();
        let backing_out = ships.iter().find(|s| s.primary).is_some_and(|s| !s.ship.waypoints.is_empty());
        if (TOWER_MIN..=TOWER_HOLD).contains(&d) && (!backing_out || d >= TOWER_HOLD - 15.0) {
            face = Some((-(at.x - p.pos.x)).atan2(-(at.z - p.pos.z))); // heading 0 faces -Z, rotating about +Y
            // A hold that lands no hit (hull pinned on the island with the tower out of the gun's reach) moves round the tower.
            let damage = w.iter().find(|u| u.e == e).map_or(0.0, |u| u.damage);
            if hold_stalled(sc.time, damage, &TOWER_STALL) {
                let round = Quat::from_rotation_y(TOWER_ROUND) * off.normalize_or(Vec3::X);
                start_detour(sc.time, at + round * TOWER_HOLD * 1.3, &STUCK);
            }
        }
        Some(if face.is_some() { p.pos } else { at + off.normalize_or(Vec3::X) * TOWER_HOLD })
    } else if let Some((at, _)) = lifeboats.iter().filter(|b| sc.hostile(b.1, &p.owner) && b.0.distance(p.pos) < 1500.0).nearest_to(p.pos, |b| b.0) {
        Some(*at) // touching an enemy lifeboat collects it (mission 2's PickUpLifeboats counts it as a capture)
    } else if dock {
        w.iter().find(|u| u.name == "RLS Bounty").map(|u| u.pos)
    } else {
        // No objective point (mission 8's "Dock with Mining Camp"): sail for what the rules wait for the player to dock with.
        // An objective point already reached that nothing answers (mission 5's "Dock @ Elzrand" waits for a warning shot) gives way
        // to what the rules want.
        objective_at(sc, data)
            .filter(|at| at.distance(p.pos) > POINT_REACHED || wanted_at.is_none())
            .or_else(|| sc.tasks.iter().any(|t| t.active && !t.complete && is_dock_task(t)).then(|| dock_target(sc, w, p.pos).map(|t| t.1)).flatten())
            .or(wanted_at)
            // Still nowhere to go (mission 10's "Defend Merchant Fleet"): stay with the nearest friendly ship outside the player's fleet.
            // Only once the ship is idle: a course the rules gave (mission 1's "Player Ordered Around Archipelago" path) wins.
            .or_else(|| {
                let idle = ships.iter().find(|s| s.primary).is_some_and(|s| s.ship.waypoints.is_empty());
                w.iter()
                    .filter(|_| idle)
                    .filter(|u| u.alive && u.visible && u.owner != p.owner && !sc.hostile(&u.owner, &p.owner) && ships.contains(u.e))
                    .nearest_to(p.pos, |u| u.pos)
                    .filter(|u| u.pos.distance(p.pos) < 2000.0)
                    .map(|u| u.pos)
            })
    };
    // Sailing for a unit the rules want shot (the fallback below the objective point): it is the target as well.
    if let (Some(g), Some(u)) = (goal, wanted_hit)
        && g == u.pos
    {
        target.0 = Some(u.e);
    }
    // Once a game minute: what the bot is after, for reading a stalled autoplay log (#68).
    static LOGGED_MINUTE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let minute = (sc.time / 60.0) as u32;
    if LOGGED_MINUTE.swap(minute, std::sync::atomic::Ordering::Relaxed) != minute {
        let helm = ships
            .iter()
            .find(|s| s.primary)
            .map(|s| (s.ship.order, s.ship.waypoints.len(), s.ship.throttle, s.ship.speed, s.ship.cap, s.ship.heading, face, mortars.len(), target.2.shot));
        let target_damage = target.0.and_then(|e| w.iter().find(|u| u.e == e)).map(|u| u.damage);
        info!(
            "autopilot t={:.0} pos={:.0} goal={:.0?} target={:?} damage={:.2?} helm(order,wps,throttle,speed,cap,heading,face,mortar,shot)={:?} wants={:?}",
            sc.time,
            p.pos,
            goal,
            target.0,
            target_damage,
            helm,
            (wants.len(), &wants[..wants.len().min(6)])
        );
    }
    // Dev shortcut for end-to-end runs: PROCYON_AUTOKILL=1 sinks the pirate once the player is close.
    if std::env::var_os("PROCYON_AUTOKILL").is_some()
        && let Some(wr) =
            w.iter().find(|u| u.name == "Warrior" && u.alive && u.visible && u.pos.distance(p.pos) < 250.0 && sc.hits_by.get(&(PLAYER.to_string(), u.e)).is_some_and(|n| *n >= 3))
        && let Ok(mut x) = ships.get_mut(wr.e)
    {
        x.health.core = 0.0;
    }
    // Dev switch for unattended runs (tools/autoplay.py): PROCYON_AUTOINVULN=1 keeps the player's fleet (every ship the player owns) from
    // taking damage, so the bot survives mission 2's island towers and mission 3's escort outlives the chase, and the later rules get
    // exercised. Re-applied every tick, overriding the rules' own vulnerability changes for those ships; a "lost" outcome then only
    // comes from rules that do not need a player ship to sink.
    if std::env::var_os("PROCYON_AUTOINVULN").is_some() {
        for mut s in ships.iter_mut().filter(|s| s.mission.owner == p.owner) {
            s.mission.invuln = 1.0;
        }
    }
    let detour = unstick(sc.time, p.pos, goal.filter(|_| face.is_none()), &STUCK);
    if detour.is_some() {
        face = None;
    }
    let goal = detour.or(goal);
    if let Some(mut s) = ships.iter_mut().find(|s| s.primary)
        && let Some(g) = goal
    {
        if let Some(h) = face {
            s.ship.waypoints.clear();
            s.ship.goal = Some(h);
            s.ship.order = 1;
            s.ship.throttle = None;
        } else if g.distance(p.pos) > 60.0 || foe.is_none() {
            s.ship.waypoints = vec![g];
            s.ship.order = 2;
            s.ship.goal = None;
            s.ship.throttle = None; // a move order releases a manual throttle (the opening NIS leaves it at 0)
        } else {
            s.ship.waypoints.clear();
            s.ship.order = 1;
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
    holes: Query<(&Transform, &crate::blackhole::Pull), Without<Ship>>,
    storms: (Option<Res<crate::nebula::Storms>>, Option<Res<crate::nebula::StormShift>>),
    (guns, time, mut breakoffs): (Query<&crate::combat::GunStates>, Res<Time>, Local<std::collections::HashMap<Entity, crate::ship::attack::Breakoff>>),
) {
    let Some(mission) = mission else { return };
    let holes = crate::ship::hole_list(&holes, storms.0.as_deref(), storms.1.as_deref());
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
    // Where each ship's damage sections are, for the attack's aim point (`ship::attack::aim`).
    let hulls: std::collections::HashMap<Entity, (Transform, usize, Vec<f32>)> = ships.iter().map(|s| (s.0, (*s.2, s.1.class, s.3.sections.clone()))).collect();
    let player_owner = ships.iter().find(|s| s.5).map_or_else(|| sc.player().into(), |s| s.4.owner.clone());
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
        // zone's centre). PROVISIONAL: the original's border AI is unknown; this re-enters and resumes on the next frame inside.
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
                .nearest_to(tf.translation, |a| a.pos)
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
        // Movement: an attack order or an aggressive stance closes on the foe with the original's attack (`ship::attack`, as skirmish).
        // A human-captained ship (Captain_Human) only fires: it moves on the human fleet's orders (ship.rs fleet_follow).
        let chase = if ms.human || (ms.stance == Stance::Persistent && !ship.waypoints.is_empty()) {
            None // Persistent scripted routes take precedence over combat steering (Mission 3's swoop/retreat).
        } else {
            ordered.or(if ms.stance == Stance::Aggressive { hostile } else { None })
        };
        if ms.human {
            continue;
        }
        if let (Some((foe, fp)), false) = (chase, ms.stance == Stance::Dummy) {
            let (goal, order, throttle) = if ms.ram && ordered.is_some() {
                let d = fp - tf.translation;
                ((-d.x).atan2(-d.z), 3, None)
            } else if let Some(away) = crate::ship::flee(&holes, tf.translation) {
                (away, 3, None)
            } else {
                // PROVISIONAL: the target's radius is left out of the gap (rocks and ships alike), and the course is steered as a heading.
                let class = &lib.classes[ship.class];
                match crate::ship::attack::plan(&lib, class, h, guns.get(e).ok()) {
                    Some((routine, range)) => {
                        let gap = (fp - tf.translation).with_y(0.0).length() - class.radius;
                        let at = match hulls.get(&foe) {
                            Some((t, c, hp)) if routine != crate::ship::attack::Routine::Ram => crate::ship::attack::aim(&lib.classes[*c].sections, hp, t, tf.translation, false),
                            _ => fp, // Ram, or a rock
                        };
                        let course = crate::ship::attack::course(routine, tf.translation, ship.heading, at, gap, range[0], time.elapsed_secs(), breakoffs.entry(e).or_default());
                        let d = course[0] - tf.translation;
                        ((-d.x).atan2(-d.z), 3, crate::ship::attack::throttle(e, tf.translation, ship.heading, course[0]))
                    }
                    None => (ship.heading, 0, None),
                }
            };
            ms.stop_at_end = true;
            // Land in the way is left to the slow-and-steer near land (`collision::sail`); retail captains never pre-route round islands.
            ship.waypoints.clear();
            ship.goal = Some(goal);
            ship.order = order;
            ship.throttle = throttle;
        } else if ms.stop_at_end && ms.looping.is_none() && ship.waypoints.len() <= 1 {
            // Retail takes the last point of a 'to end' path within 2 x the ship's radius, then cuts throttle to 0.
            // PROVISIONAL: `class.radius` stands in for the original's radius field. A finished chase (remake AI) likewise waits for orders.
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
        vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
        if let Some(mut t) = text
            && **t != content
        {
            **t = content;
        }
    }
    let tasks = if crate::panels::cinematic(Some(&mission)) { String::new() } else { sc.tasks_text() };
    for mut t in &mut objective {
        if **t != tasks {
            **t = tasks.clone();
        }
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

#[cfg(test)]
mod ai_tests {
    use super::*;

    #[test]
    fn a_wedged_bot_backs_off_sideways_then_resumes_its_goal() {
        let cell = std::sync::Mutex::new(None);
        let goal = Some(Vec3::new(1000.0, 0.0, 0.0));
        let at = Vec3::ZERO;
        // Moving: no detour.
        assert_eq!(unstick(0.0, at, goal, &cell), None);
        assert_eq!(unstick(10.0, Vec3::new(100.0, 0.0, 0.0), goal, &cell), None);
        // Pinned in place past STUCK_SECS: a detour behind and to one side of the course.
        let pinned = Vec3::new(100.0, 0.0, 0.0);
        assert_eq!(unstick(25.0, pinned, goal, &cell), None);
        let d = unstick(31.0, pinned, goal, &cell).expect("detour");
        assert!(d.x < pinned.x && d.z.abs() > 100.0, "{d:?}");
        // Kept until it runs out, then the goal stands again.
        assert_eq!(unstick(35.0, pinned, goal, &cell), Some(d));
        let over = 31.0 + DETOUR_STEP / DETOUR_SPEED + 1.0;
        assert_eq!(unstick(over, pinned, goal, &cell), None);
        // Wedged again no nearer the goal: the same side, a longer step; after MAX_TRIES the other side.
        let mut t = over + STUCK_SECS + 1.0;
        let d2 = unstick(t, pinned, goal, &cell).expect("second detour");
        assert!(d2.z * d.z > 0.0 && d2.distance(pinned) > d.distance(pinned), "{d:?} {d2:?}");
        let mut last = d2;
        for _ in 2..=MAX_TRIES {
            t += 10_000.0 / DETOUR_SPEED;
            assert_eq!(unstick(t, pinned, goal, &cell), None, "the detour ran out");
            t += STUCK_SECS + 1.0;
            last = unstick(t, pinned, goal, &cell).expect("another detour");
        }
        assert!(last.z * d.z < 0.0 && (last.distance(pinned) - d.distance(pinned)).abs() < 1.0, "{d:?} {last:?}");
        // A near goal never counts as wedged.
        let cell = std::sync::Mutex::new(None);
        assert_eq!(unstick(0.0, at, Some(Vec3::new(100.0, 0.0, 0.0)), &cell), None);
        assert_eq!(unstick(60.0, at, Some(Vec3::new(100.0, 0.0, 0.0)), &cell), None);
        // A tower hold stalls only when the target's damage stays put for HOLD_STALL_SECS of continuous holding.
        let hold = std::sync::Mutex::new(None);
        let ticks = |from: f32, to: f32, damage: f32| (0..=((to - from) as i32)).any(|i| hold_stalled(from + i as f32, damage, &hold));
        assert!(!ticks(0.0, 20.0, 0.5));
        assert!(!ticks(21.0, 45.0, 0.6), "damage rising restarts the clock");
        assert!(ticks(46.0, 60.0, 0.6));
        assert!(!ticks(100.0, 120.0, 0.6), "leaving the hold starts over");
    }

    #[test]
    fn a_tow_task_names_its_ship_by_name_suffix_or_in_its_text() {
        let task = |name: &str, text: &str| super::super::eval::Task { name: name.into(), text: text.into(), active: true, complete: false, failed: false };
        let unit = |n: u32, name: &str| super::super::eval::Unit {
            e: Entity::from_raw_u32(n).unwrap(),
            id: n as i64,
            name: name.into(),
            owner: "Navy".into(),
            owner_is_named_group: false,
            groups: vec![],
            pos: Vec3::ZERO,
            alive: true,
            visible: true,
            primary: n == 1,
            damage: 0.0,
            flag: String::new(),
            attack: vec![],
        };
        let w = [unit(1, "RLS Sea Sparrow"), unit(2, ""), unit(3, "RLS Bachanalia"), unit(4, "Harvester")];
        let me = w[0].e;
        // Mission 4 (#351): the task name says "tender", the text names the ship.
        let m4 = task("Tow tender to safety", "Tow the RLS Bachanalia to the Rally Point.");
        assert_eq!(task_towee(&m4, &w, me).map(|u| u.e), Some(w[2].e));
        assert_eq!(task_towee(&task("TowHarvester", ""), &w, me).map(|u| u.e), Some(w[3].e), "a name suffix still wins");
        assert!(task_towee(&task("Tow the wreck", "Tow the wreck home."), &w, me).is_none(), "an unnamed ship never matches the text");
        assert!(task_towee(&task("Protect the RLS Bachanalia", "Protect the RLS Bachanalia."), &w, me).is_none(), "only Tow tasks");
    }

    #[test]
    fn autopilot_flees_on_cover_and_escape_objectives_but_fights_the_rest() {
        assert!(flee_task("Take Cover in the Asteroid Field."));
        assert!(flee_task("Avoid the Pirate Trap. Return to Crescentia Right Away."));
        assert!(!flee_task("Hold off Pirate Attack until Repairs are Complete."));
        assert!(!flee_task("Destroy Both Camp Cannon Towers."));
        assert!(defend_task("Defend Her Majesty Aboard Parliament."));
        assert!(defend_task("Keep the Pirates From Firing on Dorlinor."));
        assert!(!defend_task("Search Nearby Islands for Ships."));
    }

    #[test]
    fn rule_keys_latch_until_a_tick_and_opening_skip_is_not_queued_twice() {
        let data = serde_json::json!({"rules": []});
        let mission = Mission { id: "synthetic".into(), script: Script::new(&data, serde_json::json!({})), data, opening: None };
        let mut app = App::new();
        let binds = crate::bindings::Bindings::default();
        let skip = binds.key(crate::bindings::Act::SkipSpeech);
        app.insert_resource(mission).insert_resource(binds).init_resource::<ButtonInput<KeyCode>>().init_resource::<RuleInput>().add_systems(Update, capture_input);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(skip);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyD);
        app.update();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
        app.update();
        let pending = std::mem::take(&mut *app.world_mut().resource_mut::<RuleInput>());
        assert!(pending.skip_speech && pending.dock, "commands survive frames without a simulation tick");
        app.update();
        let pending = app.world().resource::<RuleInput>();
        assert!(!pending.skip_speech && !pending.dock, "consumed commands are not repeated");
        app.world_mut().resource_mut::<Mission>().opening = Some(default());
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset(skip);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(skip);
        app.update();
        assert!(!app.world().resource::<RuleInput>().skip_speech, "the opening briefing owns this skip");
    }

    /// The rules `update` system on its own, with the resources and messages it reads; `mission` is the script under test.
    fn rules_app(mission: Mission) -> App {
        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()))
            .init_asset::<Image>()
            .init_asset::<bevy::audio::AudioSource>()
            .init_resource::<Time>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<RuleInput>()
            .init_resource::<Target>()
            .init_resource::<crate::bindings::Bindings>()
            .init_resource::<crate::campaign::CampaignPoints>()
            .init_resource::<crate::nebula::StormShift>()
            .init_resource::<crate::environment::Sensors>()
            .init_resource::<crate::ship::Fleets>()
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
                map: Some(mission.id.clone()),
                shot: None,
                frame: 0,
                speed: 1.0,
                ended: None,
                script: None,
                autolaunch: false,
                shot_frame: 0,
                late_frame: 0,
            })
            .insert_resource(mission)
            .add_systems(Update, update);
        app
    }

    /// Mission 9's "Dark Matter Ignited" hands the Longboat back to the human, then sets the group's throttle to 0 in the same rule.
    /// The primary handoff is queued, so the Longboat is not the primary yet when the throttle applies: it must only stop, not keep
    /// a cap of 0 (it could never move again, and "Return To Ship" could not be done).
    #[test]
    #[ignore = "needs imported assets"]
    fn a_ship_handed_to_the_human_is_stopped_not_capped_by_a_group_throttle() {
        let mut mission = crate::campaign::load_id("mission02");
        let data = serde_json::json!({"rules": [{"name": "Hand back", "conditions": [{"Type": "World Initialize"}], "actions": [
            {"Type": "Set Group/Unit AI Captain", "Group/Unit Name": "Player0 Group,Longboat", "AI Stance": "Captain_Human"},
            {"Type": "Set Group Throttle Percent", "Group Name": "Player0 Group", "Throttle Percent": 0.0}]}]});
        mission.script = Script::new(&data, serde_json::json!({}));
        let owner = mission.script.player().to_string();
        let lib = Library::load(crate::assets_dir());
        let class = lib.index("Ship_Navy_Torpedo");
        let mut app = rules_app(mission);
        let ship = |order| Ship {
            class,
            side: Side::Player,
            heading: 0.0,
            speed: 0.0,
            ang_vel: 0.0,
            order,
            waypoints: vec![],
            goal: None,
            cap: 1.0,
            throttle: None,
            sail_energy: 1.0,
            crew_speed: 1.0,
            crew_turn: 1.0,
            net: 1.0,
            avoid: 1.0,
        };
        let ms = |name: &str| MissionShip { name: name.into(), owner: owner.clone(), visible: true, ..default() };
        app.world_mut().spawn((ship(0), Health::new(&lib.classes[class]), Transform::default(), Visibility::Inherited, PlayerShip, ms("RLS Comet")));
        let boat = app.world_mut().spawn((ship(2), Health::new(&lib.classes[class]), Transform::from_xyz(50.0, 0.0, 0.0), Visibility::Inherited, ms("Longboat"))).id();
        app.insert_resource(lib);
        app.update();
        let (sh, m) = (app.world().get::<Ship>(boat).unwrap(), app.world().get::<MissionShip>(boat).unwrap());
        assert!(m.human, "Captain_Human applied");
        assert_eq!((sh.cap, sh.order), (1.0, 0), "stopped, but free to sail on the human's orders");
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn mission_two_attack_volume_changes_base_owner_and_gunfire_advances_the_story() {
        let mut mission = crate::campaign::load_id("mission02");
        let mut data = mission.data.clone();
        data["rules"].as_array_mut().unwrap().retain(|r| matches!(r["name"].as_str(), Some("SmugglerCampFound2" | "SmugglersCampFound2c")));
        mission.script = Script::new(&data, serde_json::json!({}));
        let player_owner = mission.script.player().to_string();
        let lib = Library::load(crate::assets_dir());
        let class = lib.index("Ship_Navy_Torpedo");
        let mut app = rules_app(mission);
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
            net: 1.0,
            avoid: 1.0,
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
    #[ignore = "needs imported assets"]
    fn mission_ships_halt_when_their_script_route_runs_out() {
        let lib = Library::load(crate::assets_dir());
        let mut app = App::new();
        app.insert_resource(crate::campaign::load())
            .init_resource::<Time>()
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
            net: 1.0,
            avoid: 1.0,
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
        let en_route = app.world_mut().spawn((Ship { waypoints: vec![Vec3::new(0.0, 0.0, -500.0)], ..ship }, health(), Transform::default(), ms(true, None))).id();
        let looping = app.world_mut().spawn((ship.clone(), health(), Transform::default(), ms(true, Some(vec![Vec3::new(0.0, 0.0, -500.0)])))).id();
        let near = app.world_mut().spawn((Ship { waypoints: vec![Vec3::new(0.0, 0.0, -1.0)], ..ship }, health(), Transform::default(), ms(true, None))).id();
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

        use bevy::ecs::system::RunSystemOnce;
        app.world_mut().entity_mut(persistent).insert(Visibility::Inherited);
        let path = vec![Vec3::ZERO, Vec3::X * 290.0, Vec3::X * 500.0];
        let expected = path.clone();
        app.world_mut()
            .run_system_once(
                move |mut ships: Query<RuleShip>, mut animals: Query<(Entity, &mut crate::environment::SpaceAnimal, &mut Transform, &mut Visibility), Without<Ship>>| {
                    follow_path(&mut ships, &mut animals, &[persistent], &path, true, true);
                },
            )
            .unwrap();
        let ship = app.world().get::<Ship>(persistent).unwrap();
        let mission = app.world().get::<MissionShip>(persistent).unwrap();
        assert_eq!(ship.waypoints, expected[1..]);
        assert_eq!(mission.looping.as_ref(), Some(&expected));
        assert!(!mission.stop_at_end);
        assert!(mission.attack.is_empty());
    }
}
