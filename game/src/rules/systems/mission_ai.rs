//! Mission AI: alliances, targets, movement, border leashes and path following.

use super::*;
use crate::EngineOrder;

/// Alliances -> sides, hold fire, target choice and movement for every unit but the player's; towing.
pub(super) fn mission_ai(
    mission: Option<Res<Mission>>,
    lib: Res<Library>,
    rocks: Query<(Entity, &Transform, &Asteroid), Without<Ship>>,
    mut ships: Query<(Entity, &mut Ship, &mut Transform, &Health, &mut MissionShip, Has<PlayerShip>, Option<&crate::ship::FleetPilot>, Has<crate::lifeboats::Abandoned>)>,
    busy: Query<(), crate::ship::CaptainBusy>,
    boats: Query<(), With<crate::lifeboats::Lifeboat>>,
    sensors: Res<crate::environment::Sensors>,
    held: Query<(), With<crate::ship::HoldPosition>>,
    holes: Query<(&Transform, &crate::blackhole::Pull), Without<Ship>>,
    storms: (Option<Res<crate::nebula::Storms>>, Option<Res<crate::nebula::StormShift>>),
    (guns, time, mut breakoffs): (Query<&crate::combat::GunStates>, Res<Time>, Local<std::collections::HashMap<Entity, crate::ship::attack::Breakoff>>),
    (spawn_order, mut start): (Query<&crate::scenario::SpawnIndex>, crate::scenario::BattleStart),
) {
    if start.is_new() {
        breakoffs.clear();
    }
    let key = |e: Entity| crate::scenario::SpawnIndex::of(&spawn_order, e);
    let Some(mission) = mission else { return };
    let holes = crate::ship::hole_list(&holes, storms.0.as_deref(), storms.1.as_deref());
    let sc = &mission.script;
    // Targets for orders and hostility: ships, then rocks (rocks only answer attack orders, never hostility); tagged with TWT groups.
    let unit = |e, id, name: &str, owner: &str, pos, alive| Unit { name: name.into(), owner: owner.into(), alive, visible: alive, ..Unit::new(e, id, pos) };
    // Lifeboats (mission 4's pre-placed ones) are never an enemy or a target, whoever owns them.
    let mut snap: Vec<Unit> =
        ships.iter().filter(|s| !boats.contains(s.0)).map(|(e, _, tf, h, m, ..)| unit(e, m.id, &m.name, &m.owner, tf.translation, !h.destroyed && m.visible)).collect();
    let n_ships = snap.len();
    snap.extend(rocks.iter().map(|(e, tf, a)| unit(e, a.id, &a.kind, &a.group, tf.translation, a.hp > 0.0)));
    sc.tag(&mut snap);
    // Where each ship's damage sections are, for the attack's aim point (`ship::attack::aim`).
    let hulls: std::collections::HashMap<Entity, (Transform, usize, Vec<f32>)> = ships.iter().map(|s| (s.0, (*s.2, s.1.class, s.3.sections.clone()))).collect();
    // Where each ship is, which way it points and how fast it goes: what `OP Follow Group` keeps station on.
    let kin: std::collections::HashMap<Entity, (Vec3, f32, f32, usize)> = ships.iter().map(|s| (s.0, (s.2.translation, s.1.heading, s.1.speed, s.1.class))).collect();
    // Followers given the same station on the same group line up one behind the other instead of fighting for one slot: each ship's
    // place in that line, in spawn order.
    let mut line: Vec<(crate::scenario::SpawnIndex, Entity, String, Station, u32)> =
        ships.iter().filter(|s| !s.3.destroyed).filter_map(|s| s.4.follow.as_ref().map(|f| (key(s.0), s.0, f.group.clone(), f.station, f.distance.to_bits()))).collect();
    line.sort_by_key(|l| (l.0, l.1));
    let place_in_line = |e: Entity| {
        let Some(me) = line.iter().find(|l| l.1 == e) else { return 0 };
        line.iter().take_while(|l| l.1 != e).filter(|l| (&l.2, l.3, l.4) == (&me.2, me.3, me.4)).count()
    };
    let player_owner = ships.iter().find(|s| s.5).map_or_else(|| sc.player().into(), |s| s.4.owner.clone());
    for (e, mut ship, tf, h, mut ms, primary, pilot, abandoned) in &mut ships {
        let is_boat = boats.contains(e);
        let side = if !is_boat && sc.hostile(&ms.owner, &player_owner) { Side::Enemy } else { Side::Player };
        if ship.side != side {
            ship.side = side;
        }
        if is_boat {
            continue; // drifts or sits where it was placed; no captain AI
        }
        let hold = ms.stance == Stance::Dummy || sc.holds_fire(&ms.owner) || abandoned;
        if ms.hold_fire != hold {
            ms.hold_fire = hold;
        }
        // Not retail: an abandoned hull (`OP Abandon Ship`) takes no orders from its captain and cannot be given speed by a later rule.
        if abandoned {
            if ship.cap != 0.0 {
                ship.cap = 0.0;
            }
            ship.halt();
            ship.throttle = None;
            ms.target = None;
            continue;
        }
        if sc.tows().iter().any(|link| link.1 == e) {
            // PROVISIONAL: the original's towee after release is not traced; it resumes its course (mission 10's convoy leader).
            if ms.towed_course.is_none() && ship.order == EngineOrder::Full && !ship.waypoints.is_empty() {
                ms.towed_course = Some(std::mem::take(&mut ship.waypoints));
            }
            ship.waypoints.clear();
            ship.order = EngineOrder::Stop;
            ship.goal = None;
            continue; // Movement/turning come from the shared tow constraint, not a rigid teleport.
        }
        if ms.towed_course.is_some() && !h.destroyed {
            ship.waypoints = ms.towed_course.take().unwrap_or_default();
            ship.order = EngineOrder::Full;
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
            ship.order = EngineOrder::Full;
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
            ship.order = EngineOrder::Full;
        }
        let range = lib.classes[ship.class].engage * 1.3; // PROVISIONAL: the 1.3 factor on engage range is a guess
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
        // Not retail: a standing `OP Follow Group` order. The ship keeps its station beside, ahead of or astern of a ship of the group
        // (the one it first chose, until that one is gone) as a fleet's follower holds its formation slot (`ship::follow_speed`); it still
        // fires at its target. The human's own ship never follows. PROVISIONAL: the station is the leader's position plus an offset in its
        // frame, with no allowance for the leader's turn rate; ships that share a station stand a hull and 10 units further out each.
        if !ms.human
            && let Some(mut order) = ms.follow.clone()
        {
            let leader =
                order.leader.filter(|l| snap[..n_ships].iter().any(|u| u.e == *l && u.alive)).or_else(|| near(&snap[..n_ships], &|s| matches(s, &order.group), false).map(|x| x.0));
            if order.leader != leader {
                order.leader = leader;
                ms.follow = Some(order.clone());
            }
            if let Some(&(lpos, lheading, lspeed, lclass)) = leader.and_then(|l| kin.get(&l)) {
                let class = &lib.classes[ship.class];
                let here = tf.translation;
                let out = order.distance + place_in_line(e) as f32 * (2.0 * class.radius + 10.0);
                let slot = (lpos + Quat::from_rotation_y(lheading) * order.station.offset(out)).with_y(here.y);
                let top = crate::ship::max_speed(&ship, class, h);
                let to = slot - here;
                let error = if to.length_squared() > 1.0 { crate::ship::angdiff((-to.x).atan2(-to.z), ship.heading) } else { 0.0 };
                let aim = crate::ship::steer_round(here, slot, lpos, class.radius + lib.classes[lclass].radius);
                let wanted = crate::ship::follow_speed(true, to.length(), lspeed, top, error);
                ship.waypoints = if to.length() > 3.0 { vec![aim] } else { vec![] };
                ship.goal = Some(lheading);
                ship.order = if wanted < 0.01 { EngineOrder::Stop } else { EngineOrder::Full };
                ship.throttle = Some(if top > 0.001 { (wanted / top).clamp(0.0, 1.0) } else { 0.0 });
                ms.stop_at_end = false;
            } else {
                // Nothing to follow (the group is gone): stand still until a ship of it shows up.
                ship.halt();
                ship.throttle = None;
            }
            continue;
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
                ((-d.x).atan2(-d.z), EngineOrder::Emergency, crate::ship::attack::throttle(crate::ship::attack::Routine::Ram, key(e), tf.translation, ship.heading, fp))
            } else if let Some(away) = crate::ship::flee(&holes, tf.translation) {
                (away, EngineOrder::Emergency, None)
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
                        ((-d.x).atan2(-d.z), EngineOrder::Emergency, crate::ship::attack::throttle(routine, key(e), tf.translation, ship.heading, course[0]))
                    }
                    None => (ship.heading, EngineOrder::Stop, None),
                }
            };
            ms.stop_at_end = true;
            // Land in the way is left to the slow-and-steer near land (`collision::sail`); retail captains never pre-route round islands.
            ship.waypoints.clear();
            ship.goal = Some(goal);
            ship.order = order;
            ship.throttle = throttle;
        } else if ms.stop_at_end && ms.looping.is_none() {
            // Retail path end (docs/re/movement.md): throttle holds the speed fraction while turned off the point, else full; the last point
            // of the path counts as reached inside 2 x the ship's radius, then the throttle is 0.
            // PROVISIONAL: `class.radius` stands in for the original's radius field. A finished chase (remake AI) likewise waits for orders.
            let class = &lib.classes[ship.class];
            if let Some(&wp) = ship.waypoints.first() {
                let last = ship.waypoints.len() == 1;
                let to_last = ship.waypoints.last().map_or(0.0, |p| p.distance(tf.translation));
                if !(last && to_last < 2.0 * class.radius) {
                    let top = crate::ship::max_speed(&ship, class, h);
                    let fraction = if top > 0.001 { (ship.speed.abs() / top).clamp(0.0, 1.0) } else { 0.0 };
                    let d = wp - tf.translation;
                    let error = crate::ship::angdiff((-d.x).atan2(-d.z), ship.heading);
                    ship.throttle = Some(crate::ship::path_end::throttle(fraction, error, to_last, class.radius, false));
                    continue;
                }
            }
            ship.throttle = None;
            ship.waypoints.clear();
            ms.stop_at_end = false;
            ship.order = EngineOrder::Stop;
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
pub(super) fn leash_player(mut mission: ResMut<Mission>, mut ships: Query<(&mut Ship, &Transform, &MissionShip, &Health), With<PlayerShip>>, mut out: Local<bool>) {
    let Ok((mut ship, tf, ms, h)) = ships.single_mut() else { return };
    let back = ms.border.as_deref().filter(|_| !h.destroyed).and_then(|z| leash(&mission.script, z, ms.return_point, tf.translation));
    match back {
        Some(p) => {
            if !*out {
                mission.script.helm_off_course = true;
            }
            ship.waypoints = vec![p];
            ship.goal = None;
            if ship.order == EngineOrder::Stop {
                ship.order = EngineOrder::Slow;
            }
        }
        None if *out => {
            ship.waypoints.clear();
        }
        None => {}
    }
    *out = back.is_some();
}

pub(super) fn follow_path(
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
        ms.follow = None;
        sh.waypoints = full[from..].to_vec();
        sh.order = EngineOrder::Full;
        sh.goal = None;
        ms.attack.clear();
        ms.stop_at_end = !looping;
        ms.looping = looping.then_some(full);
    }
}

/// The points of a mission path (`paths.<name>`), empty when the mission has no such path.
pub(super) fn path_points(paths: &Value, name: &str) -> Vec<Vec3> {
    paths[name].as_array().map(|a| a.iter().map(game_to_bevy).collect()).unwrap_or_default()
}
