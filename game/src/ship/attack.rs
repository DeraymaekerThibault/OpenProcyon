//! The original's attack manoeuvres for computer captains, read from its code (docs/re/movement.md, "Computer captains: attack"):
//! the primary gun bank picks Broadside (one side), Frontal, Rear or Ram; R is the guns' mean effective range (long range against
//! a base); the captain sails a course of aim points, re-planned every think, at full throttle when the next point is within
//! 11.25° of the bow and at its minimum throttle otherwise.
use bevy::prelude::*;

use crate::combat::{GunStates, Health, Library};
use crate::data::Class;

/// Retail follower: full throttle while the next course point is within this angle of the bow, else the minimum throttle.
const FULL_THROTTLE_ARC: f32 = 11.25 * std::f32::consts::PI / 180.0;
/// Frontal attack: longest leg sailed at right angles to the line of sight before charging again (s).
const FRONTAL_LEG: f32 = 15.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Routine {
    /// Keep the target on one beam: +1 port (the original's side A), -1 starboard.
    Broadside(f32),
    Frontal,
    Rear,
    /// No live gun: sail straight at it.
    Ram,
}

/// Quadrant of a mount angle (counter-clockwise from the bow): 0 front-port, 3 rear-port, 2 rear-starboard, 1 front-starboard.
fn quadrant(a: f32) -> u8 {
    let a = a.rem_euclid(std::f32::consts::TAU);
    [0, 3, 2, 1][((a / std::f32::consts::FRAC_PI_2) as usize).min(3)]
}

/// The attack routine and R against a ship and against a base, from the guns the ship has now. Primary bank: the largest sum of its
/// live guns' Victory Point Cost (first bank on a tie); its quadrants (each gun's mount direction and arc edges, all four for an
/// arc over 180°) pick the routine. R: the mean over banks with a live gun of the first live gun's effective (long) range.
/// None when no bank covers any quadrant.
pub fn plan(lib: &Library, class: &Class, health: &Health, guns: Option<&GunStates>) -> Option<(Routine, [f32; 2])> {
    // (bank, victory points, quadrant flags, first live gun's [effective, long] range)
    let mut banks: Vec<(usize, u32, u8, [f32; 2])> = vec![];
    for (i, m) in class.mounts.iter().enumerate() {
        if m.parent.is_some_and(|p| health.sections.get(p).is_some_and(|hp| *hp <= 0.0)) {
            continue;
        }
        let name = guns.map_or(m.gun.as_str(), |g| g.gun(i));
        let Some(gun) = lib.guns.get(name) else { continue };
        let yaw = m.rot.to_euler(EulerRot::YXZ).0;
        let flags = if m.arc > std::f32::consts::PI { 0b1111 } else { [yaw, yaw - m.arc / 2.0, yaw + m.arc / 2.0].iter().fold(0, |f, a| f | 1 << quadrant(*a)) };
        match banks.iter_mut().find(|b| b.0 == m.bank) {
            Some(b) => (b.1, b.2) = (b.1 + gun.points.unwrap_or(0), b.2 | flags),
            None => banks.push((m.bank, gun.points.unwrap_or(0), flags, [gun.range[0], gun.range[1]])),
        }
    }
    if banks.is_empty() {
        return Some((Routine::Ram, [0.0; 2]));
    }
    let n = banks.len() as f32;
    let range = [banks.iter().map(|b| b.3[0]).sum::<f32>() / n, banks.iter().map(|b| b.3[1]).sum::<f32>() / n];
    let best = banks.iter().map(|b| b.1).max().unwrap_or(0);
    let f = banks.iter().find(|b| b.1 == best)?.2;
    let has = |q: u8| f & (1 << q) != 0;
    let routine = if (has(0) || has(1)) && !has(2) && !has(3) {
        Routine::Frontal
    } else if has(2) && has(3) {
        Routine::Rear
    } else if !has(1) && !has(2) && (has(0) || has(3)) {
        Routine::Broadside(1.0)
    } else if f != 0 {
        Routine::Broadside(-1.0)
    } else {
        return None;
    };
    Some((routine, range))
}

/// A Frontal attacker's break-off: (game time it started, direction).
pub type Breakoff = Option<(f32, Vec3)>;

/// The course (aim points) of one think. `gap`: distance between the two hulls' spheres; `r`: the attack range; `now`: game seconds.
#[allow(clippy::too_many_arguments)]
pub fn course(routine: Routine, pos: Vec3, heading: f32, foe: Vec3, gap: f32, r: f32, now: f32, breakoff: &mut Breakoff) -> Vec<Vec3> {
    let d = (foe - pos).with_y(0.0).normalize_or_zero();
    let right = Vec3::new(-d.z, 0.0, d.x); // the line of sight turned 90° to starboard
    match routine {
        // Aim R abeam of the target, then R beyond it: the ship ends on its turning circle with the target on the chosen beam.
        Routine::Broadside(side) => vec![foe + right * side * r, foe + d * r],
        Routine::Frontal => {
            if let Some((since, v)) = *breakoff {
                if gap <= r && now - since < FRONTAL_LEG {
                    return vec![pos + v * r];
                }
                *breakoff = None;
            }
            if gap >= r {
                return vec![foe];
            }
            // PROVISIONAL: which way the original breaks off is not read; here away from the beam the target bears on.
            let port = crate::ship::angdiff((-d.x).atan2(-d.z), heading) >= 0.0;
            let v = if port { right } else { -right };
            *breakoff = Some((now, v));
            vec![pos + v * r]
        }
        Routine::Rear if gap < r => vec![pos - d * r],
        Routine::Rear | Routine::Ram => vec![foe],
    }
}

/// The follower's throttle toward `point` as a fraction of Ahead Full (`Ship::throttle`): full (Emergency) within 11.25° of the bow,
/// else the ship's minimum, 0.65-0.75 of full drawn once per ship.
pub fn throttle(e: Entity, pos: Vec3, heading: f32, point: Vec3) -> Option<f32> {
    let d = point - pos;
    if crate::ship::angdiff((-d.x).atan2(-d.z), heading).abs() <= FULL_THROTTLE_ARC {
        return None;
    }
    // ponytail: a fixed per-ship draw from the entity, not the original's random generator.
    let u = (e.to_bits().wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as f32 / (1u64 << 24) as f32;
    Some(2.0 * (0.65 + 0.1 * u))
}

#[cfg(test)]
mod tests;
