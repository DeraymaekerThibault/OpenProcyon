//! Mission AI, the rules' scripted layer over the computer captain (`ship::ai`): sides, hold fire, attack orders, tows, border
//! leashes, follow orders and path following.

use super::*;
use crate::EngineOrder;

/// Owners -> sides, hold fire, the script's attack orders and scripted movement for every unit but the player's; towing. Runs after
/// the captain (`ship::ai`), which picks targets and attacks.
pub(super) fn mission_ai(
    mission: Option<Res<Mission>>,
    lib: Res<Library>,
    rocks: Query<(Entity, &Transform, &Asteroid), Without<Ship>>,
    mut ships: Query<(
        Entity,
        &mut Ship,
        &mut Transform,
        &Health,
        &mut MissionShip,
        Has<crate::commands::Commander>,
        Option<&crate::ship::FleetPilot>,
        Has<crate::lifeboats::Abandoned>,
    )>,
    busy: Query<(), crate::ship::CaptainBusy>,
    boats: Query<(), With<crate::lifeboats::Lifeboat>>,
    held: Query<(), With<crate::ship::HoldPosition>>,
    (spawn_order, table, captains): (Query<&crate::scenario::SpawnIndex>, crate::alliance::Hostility, Res<crate::ship::CaptainTargets>),
) {
    let key = |e: Entity| crate::scenario::SpawnIndex::of(&spawn_order, e);
    let Some(mission) = mission else { return };
    let sc = &mission.script;
    // Targets for orders and hostility: ships, then rocks (rocks only answer attack orders, never hostility); tagged with TWT groups.
    let unit = |e, id, name: &str, owner: &str, pos, alive| Unit { name: name.into(), owner: owner.into(), alive, visible: alive, ..Unit::new(e, id, pos) };
    // Lifeboats (mission 4's pre-placed ones) are never an enemy or a target, whoever owns them.
    let mut snap: Vec<Unit> =
        ships.iter().filter(|s| !boats.contains(s.0)).map(|(e, _, tf, h, m, ..)| unit(e, m.id, &m.name, &m.owner, tf.translation, !h.destroyed && m.visible)).collect();
    let n_ships = snap.len();
    snap.extend(rocks.iter().map(|(e, tf, a)| unit(e, a.id, &a.kind, &a.group, tf.translation, a.hp > 0.0)));
    sc.tag(&mut snap);
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
    for (e, mut ship, tf, h, mut ms, primary, pilot, abandoned) in &mut ships {
        let is_boat = boats.contains(e);
        // Each ship fights for its owner's side in the alliance table (an owner change moves it); a lifeboat for the human's.
        let side = if is_boat { Side::Player } else { table.side_or_neutral(&ms.owner) };
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
            ms.ordered = None;
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
        if primary || h.destroyed || busy.contains(e) || held.contains(e) || pilot.is_some_and(crate::ship::FleetPilot::is_steering) {
            continue;
        }
        // The captain's attack throttle stands between its thinks.
        if captains.of(e).is_none() {
            ship.throttle = None;
        }
        // loops: restart an exhausted path
        if ship.waypoints.is_empty()
            && let Some(p) = ms.looping.clone()
        {
            ship.waypoints = p;
            ship.order = EngineOrder::Full;
        }
        let partner = |x: Entity| sc.tows().iter().any(|t| (t.0 == e && t.1 == x) || (t.1 == e && t.0 == x));
        let near = |pool: &[Unit], pred: &dyn Fn(&Unit) -> bool| {
            pool.iter().filter(|s| s.e != e && s.alive && pred(s) && !partner(s.e)).nearest_to(tf.translation, |a| a.pos).map(|s| s.e)
        };
        // 'GroupA to attack GroupB': the order names the nearest ship of the target, else the nearest of its rocks ('attack Asteroid
        // Belt'); the captain (ship::ai) attacks it.
        let attack = |s: &Unit| ms.attack.iter().any(|g| matches(s, g));
        let ordered = near(&snap[..n_ships], &attack).or_else(|| near(&snap[n_ships..], &attack));
        if ms.ordered != ordered {
            ms.ordered = ordered;
        }
        // Not retail: a standing `OP Follow Group` order. The ship keeps its station beside, ahead of or astern of a ship of the group
        // (the one it first chose, until that one is gone) as a fleet's follower holds its formation slot (`ship::follow_speed`); it still
        // fires at its target. The human's own ship never follows. PROVISIONAL: the station is the leader's position plus an offset in its
        // frame, with no allowance for the leader's turn rate; ships that share a station stand a hull and 10 units further out each.
        if !ms.human
            && let Some(mut order) = ms.follow.clone()
        {
            let leader = order.leader.filter(|l| snap[..n_ships].iter().any(|u| u.e == *l && u.alive)).or_else(|| near(&snap[..n_ships], &|s| matches(s, &order.group)));
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
                let error = crate::ship::follow_error(to, ship.heading);
                let aim = crate::ship::steer_round(here, slot, lpos, class.radius + lib.classes[lclass].radius);
                let wanted = crate::ship::follow_speed(true, to.length(), lspeed, top, error);
                let helm = crate::ship::FollowHelm { top, max_share: 1.0, emergency_over_top: false };
                crate::ship::follow_station(&mut ship, aim, to.length(), Some(lheading), wanted, helm);
                ms.stop_at_end = false;
            } else {
                // Nothing to follow (the group is gone): stand still until a ship of it shows up.
                ship.halt();
                ship.throttle = None;
            }
            continue;
        }
        if ms.human {
            continue; // a human-captained ship (Captain_Human) moves on the human fleet's orders (ship.rs fleet_follow)
        }
        // A ship its captain sends after a target is not at a path end.
        if ms.stop_at_end && ms.looping.is_none() && captains.of(e).is_none() {
            // Retail path end (docs/re/movement.md): throttle holds the speed fraction while turned off the point, else full; the last point
            // of the path counts as reached inside 2 x the ship's radius, then the throttle is 0.
            // PROVISIONAL: `class.radius` stands in for the original's radius field. A finished chase (remake AI) likewise waits for orders.
            let class = &lib.classes[ship.class];
            if let Some(&wp) = ship.waypoints.first() {
                let last = ship.waypoints.len() == 1;
                let to_last = ship.waypoints.last().map_or(0.0, |p| p.distance(tf.translation));
                if !(last && to_last < 2.0 * class.radius) {
                    let top = crate::ship::max_speed(&ship, class, h);
                    let fraction = crate::ship::speed_fraction(ship.speed, top);
                    let d = wp - tf.translation;
                    let error = crate::ship::angdiff(crate::geom::heading_of(d), ship.heading);
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
pub(super) fn leash_player(mut mission: ResMut<Mission>, mut ships: Query<(&mut Ship, &Transform, &MissionShip, &Health), With<crate::commands::Commander>>, mut out: Local<bool>) {
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
