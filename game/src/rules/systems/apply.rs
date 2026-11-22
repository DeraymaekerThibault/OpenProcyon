//! Applying the effects a `Script::step` returned to the world: ship orders, flags, docking and tow messages, effects. One
//! implementation for both callers: the campaign (`systems::update`) and a skirmish map's script (`skirmish::run`), so a rule
//! action does the same thing wherever the rule runs. What differs is in `FxEnv`: a skirmish passes its map player -> side table
//! (an owner change then moves the ship to the new owner's side) and has no orbit camera.

use super::*;
use std::collections::HashMap;

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
    /// The orbit camera (the campaign's; a skirmish has none).
    pub orbit: Option<&'a mut Orbit>,
    /// A skirmish's map players and the side each fights for; none in the campaign.
    pub sides: Option<&'a HashMap<String, Side>>,
    /// The map's placed mines (skirmish).
    pub mines: Option<&'a mut Mines<'w, 's>>,
}

/// Applies `fxs` to the world.
pub(in crate::rules) fn apply(
    fxs: Vec<Fx>,
    env: &mut FxEnv,
    commands: &mut Commands,
    ships: &mut Query<RuleShip>,
    animals: &mut Query<(Entity, &mut crate::environment::SpaceAnimal, &mut Transform, &mut Visibility), Without<Ship>>,
) {
    let skirmish = env.sides.is_some();
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
            // that would have left the Longboat unable to move for the rest of the mission. A skirmish's human ship keeps its throttle.
            Fx::Speed(u, v) => each_ship(ships, u, |s| {
                if !s.primary && !s.mission.human {
                    s.ship.cap = v;
                } else if v <= 0.0 && !skirmish {
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
            }
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
                let at = |e| ships.get(e).ok().map(|s| (s.transform.translation, env.lib.classes[s.ship.class].radius));
                if let (Some((p, rp)), Some((q, rq))) = (at(tower), at(towee))
                    && crate::docking::tow_in_range(p, q, rp, rq)
                {
                    env.script.set_tow(tower, towee, true); // tugs placed alongside their tow (missions 1, 4, 5, 13)
                } else {
                    // Mission 10's Plunder starts ~700 units from the merchant it tows. PROVISIONAL: the original's approach is not traced.
                    env.dock.write(DockMsg::TowStart { tower: Some(tower), towee });
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
            Fx::Board(boarders, targets) => {
                for b in boarders {
                    let Ok(from) = ships.get(b).map(|s| s.transform.translation) else { continue };
                    let near = targets.iter().copied().filter(|t| *t != b).min_by(|x, y| {
                        let d = |e: &Entity| ships.get(*e).map_or(f32::MAX, |s| s.transform.translation.distance(from));
                        d(x).total_cmp(&d(y))
                    });
                    if let Some(target) = near {
                        env.board.write(crate::boarding::BoardMsg::Scripted { boarder: b, target });
                    }
                }
            }
            // Ships only: bases, rocks and animals have no flag.
            Fx::Lifeboats(u, on) => each_ship(ships, u, |s| s.mission.create_lifeboats = on),
            // Crewless (Dummy stance: no guns, no chase, no boarding party) and adrift (course cleared, the hull coasts); health, owner,
            // Boardable and the rest are left alone. The human's own ship is never abandoned.
            Fx::Abandon(u) => each_ship(ships, u, |s| {
                if s.primary {
                    warn!("OP Abandon Ship: '{}' is the player's ship; left alone", s.mission.name);
                    return;
                }
                s.mission.stance = crate::rules::Stance::Dummy;
                s.mission.target = None;
                s.mission.looping = None;
                s.ship.waypoints.clear();
                s.ship.goal = None;
                s.ship.order = 0;
                let e = s.entity;
                commands.queue(move |w: &mut World| {
                    w.write_message(crate::lifeboats::ShipAbandoned(e));
                });
            }),
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
                let side = env.sides.and_then(|s| s.get(&o)).copied();
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
                        mine.side = side;
                        if let (Some(mut shell), Some(side)) = (shell, side) {
                            shell.side = side;
                        }
                    }
                    if let Ok(RuleShipItem { ship: mut sh, mission: mut ms, .. }) = ships.get_mut(e) {
                        if let Some(side) = side {
                            sh.side = side;
                        }
                        ms.owner = o.clone();
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
                s.mission.ram = true;
                add_once(&mut s.mission.attack, &g);
            }),
            Fx::Attack(u, g) => {
                each_dragon(env.dragons, &u, |d| add_once(&mut d.attack, &g));
                each_ship(ships, u, |s| add_once(&mut s.mission.attack, &g));
            }
            // Only script-given orders are cleared: the human's own course and throttle stay.
            Fx::Clear(u) => {
                each_dragon(env.dragons, &u, |d| {
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
            Fx::Damage(u, p) => each_ship(ships, u, |s| crate::combat::fraction_hit(&env.lib.classes[s.ship.class], &mut s.health, p, s.ship.side, s.mission.invuln)),
            // A skirmish's 'Hold position' is the captain's lock (the right-click menu's Hold): the ship stays where it is and still
            // fires; releasing hands it back to its captain or to the next order the script gives.
            Fx::Hold(u, on) if skirmish => {
                each_dragon(env.dragons, &u, |d| d.hold = on);
                for e in u {
                    let Ok(RuleShipItem { ship: mut sh, .. }) = ships.get_mut(e) else { continue };
                    if on {
                        sh.waypoints.clear();
                        sh.order = 0;
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
                        s.ship.waypoints.clear();
                        s.ship.order = 0;
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
                if let Some(c) = env.script.volume(&volume).centroid() {
                    env.detonate.write(crate::combat::Detonate { bullet: "Bullet_DarkMatterExplosion".into(), at: Vec3::new(c.x, 0.0, -c.y), shooter: None, owner: None });
                }
            }
            Fx::Longboat(u) => {
                // Launched from the human's command ship: alongside it, same heading.
                let from = ships.iter().find(|s| s.primary).map(|s| (s.transform.translation, s.ship.heading, env.lib.classes[s.ship.class].radius));
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
