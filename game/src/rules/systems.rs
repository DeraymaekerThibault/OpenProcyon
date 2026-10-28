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
            Fx::Throttle(u, v) => each_ship(&mut ships, u, |s| s.ship.cap = v),
            Fx::Speed(u, v) => each_ship(&mut ships, u, |s| {
                if !s.primary {
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
            Fx::WarningShots(u, on) => each_ship(&mut ships, u, |s| s.mission.warning_shots = on),
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
    if cfg.script.as_deref() == Some("mission") {
        mission.script.crew_screen = false; // keep the default crew and arms (missions 3+ open the screen after their first NIS)
        autopilot(&mission.script, &world, &mission.data, &rocks, &mut ships, &mut target);
    }
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
    ships: &mut Query<RuleShip>,
    target: &mut Target,
) {
    if sc.nis || sc.ended.is_some() {
        return;
    }
    let Some(p) = w.iter().find(|u| u.primary && u.alive) else { return };
    // Ships only: whales count as hostile until the reprimand rule (PROVISIONAL default), and chasing them stalled the run after Objective 3.
    let foe = w.iter().filter(|u| u.alive && u.visible && sc.hostile(&u.owner, &p.owner) && ships.contains(u.e)).nearest_to(p.pos, |a| a.pos);
    let dock = sc.task_active("Dock With Tender");
    let shoot_rocks = sc.task_active("Destroy Asteroids");
    let rock = rocks.iter().filter(|(_, _, a)| a.group == "Asteroid Belt" && a.hp > 0.0).nearest_to(p.pos, |a| a.1.translation);
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
        x.health.core = 0.0;
    }
    if let Some(mut s) = ships.iter_mut().find(|s| s.primary)
        && let Some(g) = goal
    {
        if g.distance(p.pos) > 60.0 || foe.is_none() {
            s.ship.waypoints = vec![g];
            s.ship.order = 2;
            s.ship.goal = None;
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
        // Movement: an attack order or an aggressive stance closes on the foe (`ship::captain_steer`, shared with the skirmish AI).
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
            let (goal, order, throttle) = if ms.ram && ordered.is_some() {
                let d = fp - tf.translation;
                ((-d.x).atan2(-d.z), 3, None)
            } else {
                crate::ship::captain_steer(ship.heading, tf.translation, fp, lib.classes[ship.class].reach, &holes)
            };
            ship.waypoints.clear();
            ship.goal = Some(goal);
            ship.order = order;
            ship.throttle = throttle;
            ms.stop_at_end = true;
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
        // Only on a change: writing the node every frame would re-render it every frame.
        if let Some((img, _)) = name.and_then(|n| crate::sprites::image(&assets, &hud, &n))
            && (image.image != img.image || image.rect != img.rect)
        {
            (image.image, image.rect) = (img.image, img.rect);
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
                autolaunch: false,
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
    #[ignore = "needs imported assets"]
    fn mission_ships_halt_when_their_script_route_runs_out() {
        let lib = Library::load(crate::assets_dir());
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
