//! 'Teleport Group/Unit' applied to the world. The original puts the group on the point (docs/missions.md section 7); this engine
//! keeps the members' layout round it, nudges a hull that would land inside another one to the nearest free spot
//! (`collision::spot`), and leaves every ship at a full stop. Engine-added behaviour, see docs/re/collision.md "Teleport and overlap".
use super::apply::FxEnv;
use super::*;
use crate::collision::{Drift, spot};

/// Where each member goes: the first point of the set plus the member's offset from the group's centre, so a single ship lands on
/// the point itself and a group keeps the shape it had.
pub(super) fn layout(members: &[Vec3], point: Vec3) -> Vec<Vec3> {
    if members.is_empty() {
        return vec![];
    }
    let centre = members.iter().copied().sum::<Vec3>() / members.len() as f32;
    // Height comes from the point set for everyone: the members' own heights are not part of the layout.
    members.iter().map(|m| point + (*m - centre).with_y(0.0)).collect()
}

/// Brings a teleported ship to rest. Speed and turn rate are zero for everyone, and the rudder with them. The human's ship also
/// drops its throttle, sail order and course, or it would speed up again by itself; a computer ship keeps its order (a tug told
/// to follow a path afterwards still goes). Its old waypoints led from the old place, so they go.
pub(super) fn stop(ship: &mut Ship, human: bool) {
    ship.speed = 0.0;
    ship.ang_vel = 0.0;
    ship.helm.steer = 0.0;
    ship.waypoints.clear();
    if human {
        ship.halt();
        ship.throttle = None;
    }
}

pub(super) fn apply(
    units: Vec<Entity>,
    point: Vec3,
    env: &mut FxEnv,
    commands: &mut Commands,
    ships: &mut Query<RuleShip>,
    animals: &mut Query<(Entity, &mut crate::environment::SpaceAnimal, &mut Transform, &mut Visibility), Without<Ship>>,
) {
    let members: Vec<(Entity, Vec3)> = units.iter().filter_map(|e| ships.get(*e).ok().map(|s| (*e, s.transform.translation))).collect();
    let targets = layout(&members.iter().map(|m| m.1).collect::<Vec<_>>(), point);
    // Hulls a teleported ship must not land in: every other ship that is in play, and each member once it has been placed.
    let mut placed: Vec<_> = ships
        .iter()
        .filter(|s| !units.contains(&s.entity) && !s.health.destroyed && *s.visibility != Visibility::Hidden)
        .map(|s| spot::hull(env.lib.classes[s.ship.class].collision_bounds, s.transform))
        .collect();
    for ((e, _), target) in members.iter().zip(targets) {
        let Ok(RuleShipItem { ship: mut sh, transform: mut tf, mission: ms, primary, .. }) = ships.get_mut(*e) else { continue };
        let bounds = env.lib.classes[sh.class].collision_bounds;
        tf.translation = spot::free_spot(bounds, tf.rotation, target, &placed);
        placed.push(spot::hull(bounds, &tf));
        let human = ms.human || primary;
        stop(&mut sh, human);
        let e = *e;
        commands.queue(move |w: &mut World| {
            if let Some(mut d) = w.get_mut::<Drift>(e) {
                d.0 = Vec3::ZERO;
            }
            if human && let Some(mut pilot) = w.get_mut::<crate::ship::FleetPilot>(e) {
                pilot.destination = None;
            }
        });
    }
    for e in &units {
        if let Ok((_, mut animal, mut tf, _)) = animals.get_mut(*e) {
            tf.translation = point;
            animal.velocity = Vec3::ZERO;
        } else if env.bases.contains(*e) {
            let e = *e;
            // Scenery (wrecks): move the model and its collision body together.
            commands.queue(move |w: &mut World| {
                let Some(old) = w.get::<Transform>(e).map(|t| t.translation) else { return };
                let delta = (point - old).with_y(0.0);
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
