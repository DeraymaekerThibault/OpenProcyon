//! Applying the effects a `Script::step` returned to the world: ship orders, flags, docking and tow messages, effects. One
//! implementation for both callers: the campaign (`systems::update`) and a skirmish map's script (`skirmish::run`), so a rule
//! action does the same thing wherever the rule runs. What differs is in `FxEnv`: whether a mission is loaded and the orbit camera
//! (a skirmish has none). An owner change moves a ship to its new owner's side in the alliance table the script holds while it runs.

use super::*;
use crate::EngineOrder;

/// Base queries as the rule dispatcher reads them (bases carry no `Ship`).
pub(in crate::rules) type Bases<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static mut MissionBase, &'static Transform, Option<(&'static crate::combat::ArmedBase, &'static Health)>),
    (Without<Ship>, Without<crate::environment::SpaceAnimal>),
>;
pub(in crate::rules) type Mines<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static Transform, &'static mut crate::combat::MapMine, Option<&'static mut crate::combat::Shell>),
    (Without<Ship>, Without<crate::environment::SpaceAnimal>),
>;

/// Everything the effects write to besides the ships and animals.
pub(in crate::rules) struct FxEnv<'a, 'w, 's> {
    /// The script the effects came from (tows, dock times and hit counts live there).
    pub script: &'a mut Script,
    /// The map's or mission's `paths`.
    pub paths: &'a Value,
    pub lib: &'a Library,
    pub bases: &'a mut Bases<'w, 's>,
    pub dragons: &'a mut Query<'w, 's, &'static mut crate::dragons::Dragon>,
    pub dock: &'a mut MessageWriter<'w, DockMsg>,
    pub board: &'a mut MessageWriter<'w, crate::boarding::BoardMsg>,
    pub asteroids: &'a mut MessageWriter<'w, crate::combat::AsteroidSetup>,
    pub meteors: &'a mut MessageWriter<'w, crate::nebula::MeteorRoute>,
    pub detonate: &'a mut MessageWriter<'w, crate::combat::Detonate>,
    pub event_fx: &'a Query<'w, 's, (Entity, &'static EventFx)>,
    pub fxlib: Option<&'a crate::fx::FxLib>,
    pub sfx: Option<&'a mut Messages<crate::audio::Sfx>>,
    pub toggles: Option<&'a mut crate::panels::Toggles>,
    /// The battle's difficulty (`scenario::Difficulty`), for damage the rules deal.
    pub difficulty: u8,
    /// The orbit camera (the campaign's; a skirmish has none).
    pub orbit: Option<&'a mut Orbit>,
    /// A mission is loaded (the campaign, a tutorial or a mod's mission); false in a skirmish.
    pub campaign: bool,
    /// The map's placed mines (skirmish).
    pub mines: Option<&'a mut Mines<'w, 's>>,
}

/// The ECS access `apply` needs besides the ships and animals: one `SystemParam` for the campaign's `update`, the skirmish
/// script's `run` and the dev console, which build their `FxEnv` from it.
#[derive(bevy::ecs::system::SystemParam)]
pub(in crate::rules) struct FxParam<'w, 's> {
    pub bases: Bases<'w, 's>,
    /// Not retail: the mines a custom mission's (or a skirmish map's) map places (`mines`), which its rules hand to players and address by group.
    pub mines: Mines<'w, 's>,
    pub dragons: Query<'w, 's, &'static mut crate::dragons::Dragon>,
    pub difficulty: Option<Res<'w, crate::scenario::Difficulty>>,
    pub dock: MessageWriter<'w, DockMsg>,
    pub board: MessageWriter<'w, crate::boarding::BoardMsg>,
    pub asteroids: MessageWriter<'w, crate::combat::AsteroidSetup>,
    pub meteors: MessageWriter<'w, crate::nebula::MeteorRoute>,
    pub detonate: MessageWriter<'w, crate::combat::Detonate>,
    pub event_fx: Query<'w, 's, (Entity, &'static EventFx)>,
    pub fxlib: Option<Res<'w, crate::fx::FxLib>>,
    pub sfx: Option<ResMut<'w, Messages<crate::audio::Sfx>>>,
    pub toggles: Option<ResMut<'w, crate::panels::Toggles>>,
}

impl<'w, 's> FxParam<'w, 's> {
    /// The effect environment for `script`'s effects. The campaign passes its orbit camera; a skirmish map's script none.
    pub fn env<'a>(&'a mut self, script: &'a mut Script, paths: &'a Value, lib: &'a Library, orbit: Option<&'a mut Orbit>, campaign: bool) -> FxEnv<'a, 'w, 's> {
        FxEnv {
            script,
            paths,
            lib,
            bases: &mut self.bases,
            dragons: &mut self.dragons,
            dock: &mut self.dock,
            board: &mut self.board,
            asteroids: &mut self.asteroids,
            meteors: &mut self.meteors,
            detonate: &mut self.detonate,
            event_fx: &self.event_fx,
            fxlib: self.fxlib.as_deref(),
            sfx: self.sfx.as_deref_mut(),
            toggles: self.toggles.as_deref_mut(),
            difficulty: self.difficulty.as_deref().copied().unwrap_or_default().0,
            orbit,
            campaign,
            mines: Some(&mut self.mines),
        }
    }
}

/// Starts the speech line a `Script::step` returned (a sound file name) as a one-shot voice at the dialogue volume.
pub(in crate::rules) fn spawn_voice(commands: &mut Commands, assets: &AssetServer, file: &str) {
    commands.spawn((
        Scenario,
        crate::briefing::Voice,
        AudioPlayer::new(assets.load(crate::audio::sound(file))),
        PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(crate::menu::volume(crate::menu::Setting::Dialogue))),
    ));
}

/// Applies `fxs` to the world.
pub(in crate::rules) fn apply(
    fxs: Vec<Fx>,
    env: &mut FxEnv,
    commands: &mut Commands,
    ships: &mut Query<RuleShip>,
    animals: &mut Query<(Entity, &mut crate::environment::SpaceAnimal, &mut Transform, &mut Visibility), Without<Ship>>,
) {
    let skirmish = !env.campaign;
    for fx in fxs {
        match fx {
            Fx::Formation(owner, kind, hold) => {
                commands.queue(move |w: &mut World| crate::ship::set_formation(w, &owner, kind, hold));
            }
            // The human's own ship is the campaign's player-ship model (a skirmish fleet keeps its flagship): these four are
            // schema `campaign_only` Types, warned about when the map loads.
            Fx::Primary(_) | Fx::Captain(..) | Fx::Longboat(_) if skirmish => {}
            Fx::Primary(units) => {
                if let Some(e) = units.first().copied() {
                    commands.queue(move |w: &mut World| {
                        crate::ship::promote(w, e, true);
                    });
                }
            }
            Fx::Asteroids { units, path, closest, mode, speed, tumble } => {
                let path = path_points(env.paths, &path);
                env.asteroids.write(crate::combat::AsteroidSetup { units, path, closest, mode, speed, tumble });
            }
            Fx::Path { units, path, closest, looping } => {
                if skirmish {
                    info!("map script: {} unit(s) take route '{path}'", units.len());
                }
                follow_path(ships, animals, &units, &path_points(env.paths, &path), closest, looping);
            }
            Fx::MeteorPath { group, path, closest, mode } => {
                let path = path_points(env.paths, &path);
                env.meteors.write(crate::nebula::MeteorRoute::Path { group, path, closest, mode });
            }
            Fx::MeteorSpeed(group, speed) => {
                env.meteors.write(crate::nebula::MeteorRoute::Speed { group, speed });
            }
            Fx::Velocity(u, v) => {
                for e in u {
                    if let Ok((_, mut animal, ..)) = animals.get_mut(e) {
                        animal.speed = v;
                    }
                }
            }
            Fx::Event(set, name, on) => event_effect(commands, env.event_fx.iter(), env.fxlib, &env.script.point_sets, &set, &name, on),
            Fx::WeaponBar(open) => {
                if let Some(t) = env.toggles.as_mut() {
                    t.weapons = open;
                }
            }
            Fx::Sound(name) => {
                if let Some(sfx) = env.sfx.as_mut() {
                    sfx.write(crate::audio::Sfx::ui(&name));
                }
            }
            Fx::Throttle(u, v) => each_ship(ships, u, |s| s.ship.cap = v),
            // A ship just handed to the human (Captain_Human earlier in the same rule; the primary handoff itself is queued) counts as
            // the player's: mission 9's "Dark Matter Ignited" gives back the Longboat, then sets the group's throttle to 0. As a cap
            // that would have left the Longboat unable to move for the rest of the mission. The human's other fleet ships are the
            // same: mission 4 stops 'Player0 Group' for each cutscene and never raises it again, and as a cap that froze the RLS Beans
            // (and the RLS Swiftsure once it joins) for good. A skirmish's human ship keeps its throttle.
            Fx::Speed(u, v) => each_ship(ships, u, |s| {
                let own = s.ship.side == Side::Player && s.mission.owner == env.script.player();
                if !s.primary && !s.mission.human && !own {
                    s.ship.cap = v;
                } else if v <= 0.0 && !skirmish {
                    s.ship.order = EngineOrder::Stop;
                    s.ship.waypoints.clear();
                }
            }),
            Fx::Destroy(u) => each_ship(ships, u, |s| {
                s.health.core = 0.0;
                s.health.destroyed = true;
                *s.visibility = Visibility::Hidden;
            }),
            // Retail `Teleport Group/Unit`: every member goes to the first point. This engine keeps the group's layout round it, moves a hull
            // that would land in another to the nearest free spot and leaves the ships at rest (docs/re/collision.md "Teleport and overlap").
            Fx::Teleport(u, p) => {
                let members: Vec<_> = u.iter().filter_map(|e| ships.get(*e).ok().map(|s| (*e, s.transform.translation))).collect();
                let targets = super::teleport::layout(&members.iter().map(|m| m.1).collect::<Vec<_>>(), p);
                // Hulls a teleported ship must not land in: every other ship in play, and each member once it has been placed.
                let mut hulls: Vec<_> = ships
                    .iter()
                    .filter(|s| !u.contains(&s.entity) && !s.health.destroyed && *s.visibility != Visibility::Hidden)
                    .map(|s| crate::collision::spot::hull(env.lib.classes[s.ship.class].collision_bounds, s.transform))
                    .collect();
                for ((e, _), at) in members.iter().zip(targets) {
                    place(*e, at, None, true, Some(&mut hulls), env, commands, ships, animals);
                }
                for e in u.into_iter().filter(|e| !members.iter().any(|m| m.0 == *e)) {
                    place(e, p, None, false, None, env, commands, ships, animals); // animals and scenery go to the point itself
                }
            }
            // Not retail: `OP Teleport`, the retail teleport's way of placing a unit, plus the heading and the speed.
            Fx::Place { units, heading } => {
                for (e, p) in units {
                    place(e, p, heading, true, None, env, commands, ships, animals);
                }
            }
            // Not retail: `OP Follow Group`. A later order of any kind (path, attack, ram, hold, clear) replaces it; the mission AI keeps the
            // station (rules/systems/mission_ai.rs).
            Fx::Follow { units, group, station, distance } => each_ship(ships, units, |s| {
                s.mission.follow = Some(crate::rules::FollowOrder { group: group.clone(), station, distance, leader: None });
                s.mission.attack.clear();
                s.mission.ram = false;
                s.mission.looping = None;
                s.mission.stop_at_end = false;
            }),
            Fx::Dock(dockers, dockees) => {
                for d in dockers {
                    // docking.rs steers and stops the docker itself
                    if let Ok(RuleShipItem { mission: mut ms, .. }) = ships.get_mut(d) {
                        ms.stop_at_end = false;
                    }
                    if let Some(t) = dockees.first() {
                        env.dock.write(DockMsg::Dock { docker: Some(d), dockee: *t });
                    }
                }
            }
            Fx::Tow(tower, towee) => {
                let at = |e| ships.get(e).ok().map(|s| (super::sim_pos(s.transform, s.interp), env.lib.classes[s.ship.class].radius));
                if let (Some((p, rp)), Some((q, rq))) = (at(tower), at(towee))
                    && crate::docking::tow_in_range(p, q, rp, rq)
                {
                    env.script.set_tow(tower, towee, true); // tugs placed alongside their tow (missions 1, 4, 5, 13)
                } else {
                    // Mission 10's Plunder starts ~700 units from the merchant it tows. PROVISIONAL: the original's approach is not traced.
                    env.dock.write(DockMsg::TowStart { tower: Some(tower), towee });
                }
            }
            // Each boarder takes the nearest live ship of the target group through the order a player's menu gives (boarding.rs): a
            // grapple, then the boarding once the hook has bitten.
            Fx::Board(boarders, targets) => {
                let at = |e| ships.get(e).ok().filter(|s| !s.health.destroyed).map(|s| super::sim_pos(s.transform, s.interp));
                for b in boarders {
                    let Some(from) = at(b) else { continue };
                    let target = targets.iter().copied().filter(|t| *t != b).filter_map(|t| Some((t, at(t)?))).min_by(|x, y| x.1.distance(from).total_cmp(&y.1.distance(from)));
                    match target {
                        Some((target, _)) => {
                            env.board.write(crate::boarding::BoardMsg::Order { boarder: b, target });
                        }
                        None => debug!("OP Board Ship: no live ship in the target group for {b}"),
                    }
                }
            }
            // Not retail: the crews of the ships take to the boats and the hulls are left disabled (lifeboats.rs launches the boats when it
            // sees `Abandoned` added, by the ship's Create Lifeboats flag). A hull that is already abandoned or destroyed is skipped.
            Fx::Abandon(u) => each_ship(ships, u, |s| {
                if s.health.destroyed || s.abandoned {
                    return;
                }
                s.ship.halt();
                s.ship.cap = 0.0;
                s.mission.hold_fire = true;
                s.mission.attack.clear();
                s.mission.ordered = None;
                commands.entity(s.entity).insert(crate::lifeboats::Abandoned);
            }),
            // Not retail: `OP Place Mines`. The mines are the map's own kind (combat/minefield.rs arms them); one of an unknown bullet is skipped.
            Fx::Mines { set, bullet, owner } => {
                if !env.lib.bullets.contains_key(&bullet) {
                    warn!("OP Place Mines: no bullet '{bullet}'");
                    continue;
                }
                let side = (!owner.is_empty()).then(|| env.script.alliances.side_or_neutral(&owner));
                for p in env.script.point_sets[&set].as_array().into_iter().flatten() {
                    let mine = crate::combat::MapMine { id: -1, bullet: bullet.clone(), owner: owner.clone(), side };
                    commands.spawn((Scenario, mine, Transform::from_translation(game_to_bevy(&p["pos"]))));
                }
            }
            Fx::Dockable(u, on) => {
                for e in u {
                    env.dock.write(DockMsg::Enabled(e, on));
                }
            }
            Fx::Boardable(u, on) => {
                for e in u {
                    env.board.write(crate::boarding::BoardMsg::Boardable(e, on));
                }
            }
            // Ships only: bases, rocks and animals have no flag.
            Fx::Lifeboats(u, on) => each_ship(ships, u, |s| s.mission.create_lifeboats = on),
            Fx::IslandRepair(u, on) => {
                for e in u {
                    env.dock.write(DockMsg::Repairs(e, on));
                }
            }
            Fx::DockTime(u, secs) => {
                for e in u {
                    env.script.dock_times.insert(e, secs);
                }
            }
            Fx::Stance(u, st) => {
                each_dragon(env.dragons, &u, |d| d.stance = st);
                each_ship(ships, u, |s| s.mission.stance = st);
            }
            Fx::Owner(u, o) => {
                if skirmish {
                    info!("map script: {} ship(s) now belong to '{o}'", u.len());
                }
                // Not retail: a placed mine fights for the side its new owner has in the alliance table.
                let side = env.script.alliances.side_of(&o);
                let mine_side = side.or_else(|| env.campaign.then_some(crate::alliance::NEUTRAL));
                for e in u {
                    if let Ok((_, mut base, ..)) = env.bases.get_mut(e) {
                        base.owner = o.clone();
                    }
                    if let Ok((_, mut animal, ..)) = animals.get_mut(e) {
                        animal.owner = o.clone();
                    }
                    // A placed mine fights for its new owner's side.
                    if let Some(Ok((.., mut mine, shell))) = env.mines.as_mut().map(|m| m.get_mut(e)) {
                        mine.owner.clone_from(&o);
                        mine.side = mine_side;
                        if let (Some(mut shell), Some(side)) = (shell, mine_side) {
                            shell.side = side;
                        }
                    }
                    if let Ok(RuleShipItem { ship: mut sh, mission: mut ms, .. }) = ships.get_mut(e) {
                        if let Some(side) = side {
                            sh.side = side;
                        }
                        ms.owner = o.as_str().into();
                    }
                }
            }
            Fx::Visible(u, v) => {
                for e in u {
                    if env.bases.contains(e) {
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
                s.mission.follow = None;
                s.mission.ram = true;
                add_once(&mut s.mission.attack, &g);
            }),
            Fx::Attack(u, g) => {
                each_dragon(env.dragons, &u, |d| add_once(&mut d.attack, &g));
                each_ship(ships, u, |s| {
                    s.mission.follow = None;
                    add_once(&mut s.mission.attack, &g);
                });
            }
            // Only script-given orders are cleared: the human's own course and throttle stay.
            Fx::Clear(u) => {
                each_dragon(env.dragons, &u, |d| {
                    d.attack.clear();
                    d.hold = false;
                });
                each_ship(ships, u, |s| {
                    s.mission.follow = None;
                    s.mission.attack.clear();
                    s.mission.ram = false;
                    s.mission.looping = None;
                    if !s.primary {
                        s.ship.waypoints.clear();
                        s.ship.order = EngineOrder::Stop;
                        s.ship.goal = None;
                    }
                });
            }
            Fx::Damage(u, p) => {
                each_ship(ships, u, |s| {
                    // only the human's own ships take the difficulty factor: an allied computer ship (mission 2's SS Coventry) takes it whole
                    let human = s.ship.side == Side::Player && s.mission.owner == env.script.player();
                    crate::combat::fraction_hit(&env.lib.classes[s.ship.class], &mut s.health, p, human, s.mission.invuln, env.difficulty)
                })
            }
            // A skirmish's 'Hold position' is the captain's lock (the right-click menu's Hold): the ship stays where it is and still
            // fires; releasing hands it back to its captain or to the next order the script gives.
            Fx::Hold(u, on) if skirmish => {
                each_dragon(env.dragons, &u, |d| d.hold = on);
                for e in u {
                    let Ok(RuleShipItem { ship: mut sh, .. }) = ships.get_mut(e) else { continue };
                    if on {
                        sh.waypoints.clear();
                        sh.order = EngineOrder::Stop;
                        commands.entity(e).insert(crate::ship::HoldPosition);
                    } else {
                        commands.entity(e).remove::<crate::ship::HoldPosition>();
                    }
                }
            }
            Fx::Hold(u, on) => {
                each_dragon(env.dragons, &u, |d| d.hold = on);
                if on {
                    each_ship(ships, u, |s| {
                        s.mission.follow = None;
                        s.ship.waypoints.clear();
                        s.ship.order = EngineOrder::Stop;
                    });
                }
            }
            Fx::ResetHits(u) => {
                for e in u {
                    env.script.hits.remove(&e);
                    env.script.hits_by.retain(|k, _| k.1 != e);
                }
            }
            Fx::Border(u, zone) => {
                each_dragon(env.dragons, &u, |d| d.border.clone_from(&zone));
                each_ship(ships, u, |s| s.mission.border.clone_from(&zone));
            }
            Fx::ReturnPoint(u, p) => {
                each_dragon(env.dragons, &u, |d| d.home = Some(p));
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
                            let table = w.get_resource::<crate::alliance::Alliances>().cloned().unwrap_or_default();
                            match (w.get_mut::<crate::cloak::Cloak>(e), side) {
                                (Some(mut c), Some(side)) => {
                                    c.ai_modify = ai_override;
                                    if on {
                                        c.start(side, &table);
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
            Fx::Current(name, path) => {
                commands.spawn((Scenario, crate::environment::Current::new(path).named(&name)));
            }
            // Not retail: `OP Etherium Current` changes the current of that path (retail or made by this action), or makes one.
            Fx::CurrentSet { name, path, strength, width, on } => commands.queue(move |w: &mut World| {
                let found = w.query::<(Entity, &crate::environment::Current)>().iter(w).find(|(_, c)| c.name() == name).map(|(e, _)| e);
                match found {
                    Some(e) => {
                        if let Some(mut c) = w.get_mut::<crate::environment::Current>(e) {
                            c.set(strength, width, on);
                        }
                    }
                    None => {
                        let mut c = crate::environment::Current::new(path).named(&name);
                        c.set(strength, width, on);
                        w.spawn((Scenario, c));
                    }
                }
            }),
            Fx::DarkMatter(volume) => {
                // The blast starts at the volume's centre (game x, y) at the ships' height.
                if let Some(c) = env.script.volume(&volume).centroid() {
                    env.detonate.write(crate::combat::Detonate { bullet: "Bullet_DarkMatterExplosion".into(), at: Vec3::new(c.x, 0.0, -c.y), shooter: None, owner: None });
                }
            }
            Fx::Longboat(u) => {
                // Launched from the human's command ship: alongside it, same heading.
                let from = ships.iter().find(|s| s.primary).map(|s| (super::sim_pos(s.transform, s.interp), s.ship.heading, env.lib.classes[s.ship.class].radius));
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
            Fx::DragonStance(u, st) => each_dragon(env.dragons, &u, |d| d.stance = st),
            Fx::DragonThreshold(u, t) => each_dragon(env.dragons, &u, |d| d.threshold = t),
            Fx::Orbit { unit, dist, angle } => {
                let Some(orbit) = env.orbit.as_mut() else { continue };
                orbit.focus = unit;
                orbit.dist = dist;
                let heading = unit.and_then(|e| ships.get(e).ok()).or_else(|| ships.iter().find(|s| s.primary)).map_or(0.0, |s| s.ship.heading);
                orbit.yaw = heading + angle.to_radians();
            }
        }
    }
}

/// Moves one unit to `p` (the retail `Teleport Group/Unit`, with `heading` and `clear_speed` off): a ship loses its waypoints, an animal its
/// velocity, scenery moves with its collision body. Not retail: `heading` (radians about +Y; ships only) and `clear_speed` (a ship's speed and
/// turn rate, and the push of a contact or a current) are what `OP Teleport` adds. Not retail: `retail_fix` is the retail teleport's engine-added
/// handling of a ship; it lands on the nearest spot clear of `retail_fix`'s hulls (and joins them), and the
/// human's ship also drops its throttle, sail order and course (`teleport::stop`).
#[allow(clippy::too_many_arguments)]
fn place(
    e: Entity,
    p: Vec3,
    heading: Option<f32>,
    clear_speed: bool,
    retail_fix: Option<&mut Vec<crate::collision::tree::Box3>>,
    env: &FxEnv,
    commands: &mut Commands,
    ships: &mut Query<RuleShip>,
    animals: &mut Query<(Entity, &mut crate::environment::SpaceAnimal, &mut Transform, &mut Visibility), Without<Ship>>,
) {
    if let Ok(RuleShipItem { ship: mut sh, transform: mut tf, mission, primary, .. }) = ships.get_mut(e) {
        tf.translation = p;
        sh.waypoints.clear();
        let human = mission.human || primary;
        if let Some(hulls) = retail_fix {
            let bounds = env.lib.classes[sh.class].collision_bounds;
            let rotation = heading.map_or(tf.rotation, Quat::from_rotation_y);
            tf.translation = crate::collision::spot::free_spot(bounds, rotation, p, hulls);
            hulls.push(crate::collision::spot::hull(bounds, &Transform { rotation, ..*tf }));
            super::teleport::stop(&mut sh, human);
            commands.queue(move |w: &mut World| {
                if human && let Some(mut pilot) = w.get_mut::<crate::ship::FleetPilot>(e) {
                    pilot.destination = None;
                }
            });
        }
        if clear_speed {
            sh.speed = 0.0;
            sh.ang_vel = 0.0;
            commands.queue(move |w: &mut World| {
                if let Some(mut d) = w.get_mut::<crate::collision::Drift>(e) {
                    d.0 = Vec3::ZERO;
                }
            });
        }
        if let Some(h) = heading {
            sh.heading = h;
            tf.rotation = Quat::from_rotation_y(h);
        }
    } else if let Ok((_, mut animal, mut tf, _)) = animals.get_mut(e) {
        tf.translation = p;
        animal.velocity = Vec3::ZERO;
    } else if env.bases.contains(e) {
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

/// Runs `f` on each of `units` that is a mission ship (bases, rocks and animals are skipped).
pub(in crate::rules) fn each_ship(ships: &mut Query<RuleShip>, units: Vec<Entity>, mut f: impl FnMut(&mut RuleShipItem)) {
    for e in units {
        if let Ok(mut s) = ships.get_mut(e) {
            f(&mut s);
        }
    }
}

/// Runs `f` on each of `units` that is a dragon.
pub(in crate::rules) fn each_dragon(dragons: &mut Query<&mut crate::dragons::Dragon>, units: &[Entity], mut f: impl FnMut(&mut crate::dragons::Dragon)) {
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
