//! The original's attack manoeuvres for computer captains, read from its code (docs/re/movement.md, "Computer captains: attack"):
//! the primary gun bank picks Broadside (one side), Frontal, Rear or Ram; R is the guns' mean effective range (long range against
//! a base); the captain sails a course of aim points, re-planned every think, at full throttle when the next point is within
//! 11.25° of the bow and at its minimum throttle otherwise.
use bevy::prelude::*;

use crate::combat::{GunStates, Health, Library};
use crate::data::Class;

/// Retail follower: full throttle while the next course point is within this angle of the bow, else the minimum throttle.
const FULL_THROTTLE_ARC: f32 = 11.25 * std::f32::consts::PI / 180.0;
/// A ram's own full-throttle window: the target's centre within this angle of the bow (5 degrees), narrower than the other attacks'.
const RAM_FULL_THROTTLE_ARC: f32 = 5.0 * std::f32::consts::PI / 180.0;
/// Standoff attack: the share of R inside which the ship creeps, the share it holds at, and the share inside which the target is
/// too close and it breaks off like a frontal attacker (PROVISIONAL, not read from the original: it has no such routine).
const STANDOFF_CREEP: f32 = 0.9;
const STANDOFF_HOLD: f32 = 0.6;
const STANDOFF_CLOSE: f32 = 0.35;
/// Standoff attack: speed fractions of Ahead Full while creeping in and while at the hold distance (PROVISIONAL). A ship needs way on
/// to turn, so the hold is a crawl, not a stop.
const STANDOFF_CREEP_SPEED: f32 = 0.25;
const STANDOFF_HOLD_SPEED: f32 = 0.12;

/// Frontal attack: longest leg sailed at right angles to the line of sight before charging again (s).
const FRONTAL_LEG: f32 = 15.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Routine {
    /// Keep the target on one beam: +1 port (the original's side A), -1 starboard.
    Broadside(f32),
    Frontal,
    /// Not in the original. A frontal ship whose main gun is a beam: sails in to R, then holds a stand-off inside R bow-on to the
    /// target, turning to track it, so the charge and the held beam get their time on target (see `course_to`, `standoff_throttle`).
    Standoff,
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
    let mut banks: Vec<(usize, u32, u8, [f32; 2], bool)> = vec![];
    for (i, m) in class.mounts.iter().enumerate() {
        if m.parent.is_some_and(|p| health.sections.get(p).is_some_and(|hp| *hp <= 0.0)) {
            continue;
        }
        let name = guns.map_or(m.gun.as_str(), |g| g.gun(i));
        let Some(gun) = lib.guns.get(name) else { continue };
        let yaw = m.rot.to_euler(EulerRot::YXZ).0;
        let flags = if m.arc > std::f32::consts::PI { 0b1111 } else { [yaw, yaw - m.arc / 2.0, yaw + m.arc / 2.0].iter().fold(0, |f, a| f | 1 << quadrant(*a)) };
        match banks.iter_mut().find(|b| b.0 == m.bank) {
            Some(b) => (b.1, b.2, b.4) = (b.1 + gun.points.unwrap_or(0), b.2 | flags, b.4 | gun.beam.is_some()),
            None => banks.push((m.bank, gun.points.unwrap_or(0), flags, [gun.range[0], gun.range[1]], gun.beam.is_some())),
        }
    }
    if banks.is_empty() {
        return Some((Routine::Ram, [0.0; 2]));
    }
    let n = banks.len() as f32;
    let range = [banks.iter().map(|b| b.3[0]).sum::<f32>() / n, banks.iter().map(|b| b.3[1]).sum::<f32>() / n];
    let best = banks.iter().map(|b| b.1).max().unwrap_or(0);
    let primary = banks.iter().find(|b| b.1 == best)?;
    let (f, beam) = (primary.2, primary.4);
    let has = |q: u8| f & (1 << q) != 0;
    let routine = if (has(0) || has(1)) && !has(2) && !has(3) {
        // A beam gun needs time on target: only a mod's gun has one, so every retail ship keeps the original's frontal attack.
        if beam { Routine::Standoff } else { Routine::Frontal }
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

/// The point a Broadside, Frontal or Rear attack steers by (Ram steers at the target's centre): `aim_local` in the world, at the
/// target's height.
pub fn aim(sections: &[crate::data::Section], hp: &[f32], tf: &Transform, from: Vec3, base: bool) -> Vec3 {
    tf.transform_point(aim_local(sections, hp, tf, from, base)).with_y(tf.translation.y)
}

/// The centre of the target's damage section nearest to `from`, in the target's own frame: the first section on a tie, a ship's dead
/// sections included, a base's only live ones; with none, the target's origin. The original picks it for the steering point every
/// think and, once when a computer captain chooses its target, as the point its guns aim at. PROVISIONAL: the original's
/// per-section filter on bases is read as our mission flag.
pub fn aim_local(sections: &[crate::data::Section], hp: &[f32], tf: &Transform, from: Vec3, base: bool) -> Vec3 {
    let live = |k: usize| hp.get(k).is_some_and(|hp| *hp > 0.0);
    sections
        .iter()
        .enumerate()
        .filter(|(k, s)| !base || (s.mission && live(*k)))
        .map(|(_, s)| (s.min + s.max) * 0.5)
        .min_by(|a, b| tf.transform_point(*a).distance_squared(from).total_cmp(&tf.transform_point(*b).distance_squared(from)))
        .unwrap_or(Vec3::ZERO)
}

/// A Frontal attacker's break-off: (game time it started, direction).
pub type Breakoff = Option<(f32, Vec3)>;

/// The distance a routine compares with the attack range R. The guns-astern routine measures from its own hull (centre minus its own
/// radius) to the aim point on the target, the target's size not counted; the others use the gap between the two hulls' spheres.
pub fn range_gap(routine: Routine, pos: Vec3, own_radius: f32, foe_pos: Vec3, foe_radius: f32, aim: Vec3) -> f32 {
    match routine {
        Routine::Rear => (aim - pos).with_y(0.0).length() - own_radius,
        _ => (foe_pos - pos).with_y(0.0).length() - own_radius - foe_radius,
    }
}

/// What a fully submerged submarine's frontal attack uses in place of R: the charge goes in to this share of R (docs/re/movement.md).
pub const SUBMERGED_CHARGE: f32 = 0.2;

/// The frontal attack's two ranges `(charge, break-off)`: both R, except for a fully submerged submarine, which charges in to 0.2 R and
/// breaks off at 0.
pub fn frontal_ranges(r: f32, submerged: bool) -> (f32, f32) {
    if submerged { (SUBMERGED_CHARGE * r, 0.0) } else { (r, r) }
}

/// The course (aim points) of one think. `gap`: distance between the two hulls' spheres; `r`: the attack range; `now`: game seconds.
#[allow(clippy::too_many_arguments)]
pub fn course(routine: Routine, pos: Vec3, heading: f32, foe: Vec3, gap: f32, r: f32, now: f32, breakoff: &mut Breakoff) -> Vec<Vec3> {
    course_to(routine, pos, heading, foe, gap, (r, r), now, breakoff).unwrap_or_else(|| vec![foe])
}

/// `course` with the frontal attack's two ranges (`frontal_ranges`). None: no course is set this think and the one already held stands.
#[allow(clippy::too_many_arguments)]
pub fn course_to(routine: Routine, pos: Vec3, heading: f32, foe: Vec3, gap: f32, (r, leg_r): (f32, f32), now: f32, breakoff: &mut Breakoff) -> Option<Vec<Vec3>> {
    let d = (foe - pos).with_y(0.0).normalize_or_zero();
    let right = Vec3::new(-d.z, 0.0, d.x); // the line of sight turned 90° to starboard
    match routine {
        // Aim R abeam of the target, then R beyond it: the ship ends on its turning circle with the target on the chosen beam.
        Routine::Broadside(side) => Some(vec![foe + right * side * r, foe + d * r]),
        Routine::Frontal => {
            if let Some((since, v)) = *breakoff {
                if gap <= leg_r && now - since < FRONTAL_LEG {
                    return Some(vec![pos + v * leg_r]);
                }
                *breakoff = None;
            }
            if gap >= r {
                return Some(vec![foe]);
            }
            // The line of sight turned 90° toward the side the target bears on (to port when it is on the port bow, else to starboard):
            // the ship turns toward it and circles it on that side. Fixed in the world for the whole leg.
            let port = crate::ship::angdiff((-d.x).atan2(-d.z), heading) > 0.0;
            let v = if port { -right } else { right };
            if gap > leg_r {
                return None; // a break-off ends at once above its range (a submerged submarine's: 0)
            }
            *breakoff = Some((now, v));
            Some(vec![pos + v * leg_r])
        }
        // Straight at the target all the way (the bow follows it, so a fixed gun keeps bearing), unless it is too close.
        Routine::Standoff => {
            if let Some((since, v)) = *breakoff {
                if gap < STANDOFF_HOLD * r && now - since < FRONTAL_LEG {
                    return Some(vec![pos + v * r]);
                }
                *breakoff = None;
            }
            if gap < STANDOFF_CLOSE * r {
                let port = crate::ship::angdiff((-d.x).atan2(-d.z), heading) > 0.0;
                let v = if port { -right } else { right };
                *breakoff = Some((now, v));
                return Some(vec![pos + v * r]);
            }
            Some(vec![foe])
        }
        Routine::Rear if gap < r => Some(vec![pos - d * r]),
        Routine::Rear | Routine::Ram => Some(vec![foe]),
    }
}

/// A standoff attacker's throttle (fraction of Ahead Full) while it closes on its target: None (full) outside `STANDOFF_CREEP` R, a creep
/// inside it and a crawl inside `STANDOFF_HOLD` R.
pub fn standoff_throttle(gap: f32, r: f32) -> Option<f32> {
    if gap >= STANDOFF_CREEP * r {
        None
    } else if gap >= STANDOFF_HOLD * r {
        Some(STANDOFF_CREEP_SPEED)
    } else {
        Some(STANDOFF_HOLD_SPEED)
    }
}

/// The follower's throttle toward `point` as a fraction of Ahead Full (`Ship::throttle`): full (Emergency) within 11.25° of the bow,
/// else the ship's minimum, 0.65-0.75 of full drawn once per ship. A `Ram` has a narrower window: 5° of the bow.
pub fn throttle(routine: Routine, e: Entity, pos: Vec3, heading: f32, point: Vec3) -> Option<f32> {
    let d = point - pos;
    let arc = if routine == Routine::Ram { RAM_FULL_THROTTLE_ARC } else { FULL_THROTTLE_ARC };
    if crate::ship::angdiff((-d.x).atan2(-d.z), heading).abs() <= arc {
        return None;
    }
    // ponytail: a fixed per-ship draw from the entity, not the original's random generator.
    let u = (e.to_bits().wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as f32 / (1u64 << 24) as f32;
    Some(2.0 * (0.65 + 0.1 * u))
}

#[cfg(test)]
mod tests;
