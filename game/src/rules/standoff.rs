//! Where the dev autopilot holds its ship against a target: from the flagship's own mounts, the relative bearing at which most gun
//! fire bears and a range inside those guns' reach but outside ramming distance. Pure geometry on a list of `Gun`s; the engine's own
//! engage test (`combat::mount_engages`, the one `aim_turret` fires on) decides which guns bear, so the two cannot disagree.
//!
//! Bearings follow the helm's convention: the angle of the target off the bow, positive to port (the direction a heading turns when it
//! grows), 0 dead ahead. Heading 0 faces -Z, so a direction at bearing `b` from the bow is (-sin b, 0, -cos b) in the ship's frame.

use bevy::prelude::*;

/// One gun that can engage the target: its mount's orientation (ship frame) and arc (radians), its range bands (effective, long,
/// maximum) and its weight (damage per second).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gun {
    pub rot: Quat,
    pub arc: f32,
    pub range: [f32; 3],
    pub weight: f32,
}

/// The place to hold: the target's bearing off the bow, the distance, and the weight of the guns that bear there (of `total`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Solution {
    pub bearing: f32,
    pub range: f32,
    pub weight: f32,
    pub total: f32,
}

/// Share of a gun's weight counted in each range band (effective, long, maximum): its deviation grows with the band
/// (docs/re/gunnery.md "Deviation"), so a hit is likelier the closer inside the effective range.
const BAND: [f32; 3] = [1.0, 0.5, 0.15];
/// Inside this range a shell's spread (the deviation angle times the range, in both axes) is smaller than a hull's height, so closing in
/// no longer helps; beyond it the share of shells that hit falls as 1 / range. Measured: at 55-120 units about a third of the bot's
/// cannon shells hit a sloop-sized hull (mission 4), their spread being larger than the hull is high. PROVISIONAL: the engine's accuracy
/// factor (0.5 for every ship) and a hull height of a few units.
const SHARP: f32 = 45.0;
const BEARING_STEP: f32 = std::f32::consts::PI / 36.0;
const RANGE_STEP: f32 = 10.0;

/// The weight of the guns that bear on a target at `bearing` and horizontal distance `range`, each by its range band.
pub fn bearing_weight(guns: &[Gun], bearing: f32, range: f32) -> f32 {
    let dir = Vec3::new(-bearing.sin(), 0.0, -bearing.cos());
    guns.iter()
        .filter(|g| crate::combat::mount_engages(g.arc, g.rot.inverse() * dir, range, g.range[2]))
        .map(|g| {
            g.weight
                * if range <= g.range[0] {
                    BAND[0]
                } else if range <= g.range[1] {
                    BAND[1]
                } else {
                    BAND[2]
                }
        })
        .sum::<f32>()
        * (SHARP / range.max(1.0)).min(1.0)
}

/// The best bearing and range, searched in 5 degree and 10 unit steps from `min_range` (ramming distance) out to the longest effective
/// range. Ties go to the shorter range, then to the bearing nearest `near` (the last choice, or where the target is now), so the pick
/// does not flip sides. None when no gun can bear anywhere.
pub fn solve(guns: &[Gun], min_range: f32, near: f32) -> Option<Solution> {
    let total: f32 = guns.iter().map(|g| g.weight).sum();
    let reach = guns.iter().map(|g| g.range[0]).fold(min_range, f32::max);
    let mut best: Option<Solution> = None;
    let mut range = min_range;
    while range <= reach + 1e-3 {
        for k in 0..72 {
            let bearing = -std::f32::consts::PI + k as f32 * BEARING_STEP;
            let weight = bearing_weight(guns, bearing, range);
            if weight <= 0.0 {
                continue;
            }
            let better = best.is_none_or(|b| {
                weight > b.weight + 1e-3
                    || ((weight - b.weight).abs() <= 1e-3 && (range < b.range - 1e-3 || (range == b.range && angle_gap(bearing, near) < angle_gap(b.bearing, near))))
            });
            if better {
                best = Some(Solution { bearing, range, weight, total });
            }
        }
        range += RANGE_STEP;
    }
    best
}

/// The smaller angle between two bearings.
pub fn angle_gap(a: f32, b: f32) -> f32 {
    ((a - b + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI).abs()
}

/// The heading that holds `solution` against a target `to_target` away (horizontal, world axes): the target is kept at the solution's
/// bearing off the bow, turned toward the target while farther than the solution's range (closing) and away from it while nearer
/// (opening), by up to 45 degrees at a range error of the whole range.
pub fn heading(solution: &Solution, to_target: Vec3) -> f32 {
    let direction = (-to_target.x).atan2(-to_target.z);
    let distance = Vec2::new(to_target.x, to_target.z).length();
    let error = ((distance - solution.range) / solution.range.max(1.0)).clamp(-1.0, 1.0);
    let side = if solution.bearing.abs() < BEARING_STEP { 0.0 } else { solution.bearing.signum() };
    direction - (solution.bearing - side * 0.8 * error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gun(yaw_deg: f32, arc_deg: f32, ranges: [f32; 3], weight: f32) -> Gun {
        Gun { rot: Quat::from_rotation_y(yaw_deg.to_radians()), arc: arc_deg.to_radians(), range: ranges, weight }
    }

    #[test]
    fn a_broadside_battery_wants_the_target_abeam_inside_its_effective_range() {
        // Cutter-like: two guns each side (arc 75), a heavy bow gun (arc 140), as in the data.
        let guns = [
            gun(90.0, 75.0, [110.0, 180.0, 270.0], 1.0),
            gun(90.0, 75.0, [110.0, 180.0, 270.0], 1.0),
            gun(-90.0, 75.0, [110.0, 180.0, 270.0], 1.0),
            gun(-90.0, 75.0, [110.0, 180.0, 270.0], 1.0),
            gun(0.0, 140.0, [150.0, 270.0, 350.0], 1.2),
        ];
        let s = solve(&guns, 30.0, 1.0).unwrap();
        assert!((s.bearing.abs().to_degrees() - 90.0).abs() <= 40.0 && s.bearing.abs() > 0.8, "abeam, not bow-on: {s:?}");
        assert!(s.range <= 50.0 && s.range >= 30.0, "close in, outside ramming: {s:?}");
        assert!(s.weight >= 2.0, "a whole side bears: {s:?}");
        // The side nearest the target now is kept.
        assert!(solve(&guns, 30.0, 1.0).unwrap().bearing > 0.0 && solve(&guns, 30.0, -1.0).unwrap().bearing < 0.0);
    }

    #[test]
    fn a_forward_battery_wants_the_target_ahead_and_a_gun_out_of_range_counts_for_nothing() {
        let guns = [gun(0.0, 90.0, [60.0, 90.0, 135.0], 1.0)];
        let s = solve(&guns, 20.0, 0.0).unwrap();
        assert!(s.bearing.abs() <= 45f32.to_radians() + 1e-3, "{s:?}");
        assert_eq!(bearing_weight(&guns, 0.0, 200.0), 0.0, "beyond the maximum range nothing bears");
        assert!(bearing_weight(&guns, std::f32::consts::PI, 50.0) == 0.0, "astern is outside the arc");
        assert!(solve(&[], 20.0, 0.0).is_none());
    }

    #[test]
    fn the_heading_closes_when_far_and_opens_when_near_and_keeps_the_bearing_at_the_range() {
        let s = Solution { bearing: std::f32::consts::FRAC_PI_2, range: 100.0, weight: 4.0, total: 5.0 };
        // Target 100 units dead east of the ship (+x): direction angle = atan2(-100, 0) = -90 degrees; abeam to port means heading -180.
        let east = Vec3::new(100.0, 0.0, 0.0);
        let at_range = heading(&s, east);
        let bearing_now = (-east.x).atan2(-east.z) - at_range;
        assert!((bearing_now - s.bearing).abs() < 1e-4, "at the range the target sits at the solution's bearing");
        assert!(heading(&s, east * 2.0) > at_range, "far: turn toward it (the bearing shrinks)");
        assert!(heading(&s, east * 0.5) < at_range, "near: turn away");
    }
}
