//! 'Teleport Group/Unit' applied to the world. The original puts the group on the point (docs/missions.md section 7); this engine
//! keeps the members' layout round it, nudges a hull that would land inside another one to the nearest free spot
//! (`collision::spot`, applied by `apply::place`), and leaves every ship at a full stop. Engine-added behaviour, see docs/re/collision.md "Teleport and overlap".
use super::*;

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
