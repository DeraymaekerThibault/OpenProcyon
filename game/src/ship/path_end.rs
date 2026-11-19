//! Throttle of a computer or scripted ship following a path that ends in a stop ("stop at end", docs/re/movement.md, "Path end
//! behaviour"). States: steer (aligned, full throttle), turn (heading error above `TURN_ANGLE`: hold the current speed fraction),
//! arrive (the path is done, or the last point is inside the stopping distance: ramp down, then stop).
use std::f32::consts::PI;

/// Heading error to the current point above which the ship is in the turn state (pi/16 = 11.25 degrees).
pub const TURN_ANGLE: f32 = PI / 16.0;
/// Lowest throttle held while turning, and the arrive ramp's floor.
pub const MIN_THROTTLE: f32 = 0.1;

/// Distance to the last point at which arrive starts besides "path done". The original's helper builds terms that cancel (the result
/// is zero up to rounding), so arrive in practice starts only when the path ends; this returns that result, not an invented distance.
pub fn stopping_distance() -> f32 {
    0.0
}

/// Throttle as a share of Ahead Full, 0..1. `speed_fraction` is |speed| / max speed, `heading_error` the angle to the current point,
/// `to_last` the distance to the last point, `radius` the ship's radius R (PROVISIONAL stand-in: `class.radius`), `done` the path being
/// exhausted (last point inside 2R).
pub fn throttle(speed_fraction: f32, heading_error: f32, to_last: f32, radius: f32, done: bool) -> f32 {
    let t = if done || to_last <= stopping_distance() {
        arrive(to_last, radius, done)
    } else if heading_error.abs() > TURN_ANGLE {
        speed_fraction.max(MIN_THROTTLE)
    } else {
        1.0
    };
    t.clamp(0.0, 1.0)
}

/// The arrive state's throttle: 1.0 at 2R from the last point falling to 0.1 at R while the path is live, 0 once it is done.
pub fn arrive(to_last: f32, radius: f32, done: bool) -> f32 {
    if done || radius <= 0.0 { 0.0 } else { MIN_THROTTLE + (1.0 - MIN_THROTTLE) * ((to_last - radius) / radius).clamp(0.0, 1.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steers_at_full_throttle_within_the_turn_angle() {
        assert_eq!(throttle(0.3, TURN_ANGLE - 0.001, 500.0, 10.0, false), 1.0);
        assert_eq!(throttle(0.3, -(TURN_ANGLE - 0.001), 500.0, 10.0, false), 1.0);
    }

    #[test]
    fn turning_holds_the_speed_fraction_with_a_floor() {
        assert!((TURN_ANGLE - 11.25f32.to_radians()).abs() < 1e-6);
        assert_eq!(throttle(0.6, TURN_ANGLE + 0.01, 500.0, 10.0, false), 0.6);
        assert_eq!(throttle(0.6, -(TURN_ANGLE + 0.01), 500.0, 10.0, false), 0.6);
        assert_eq!(throttle(0.0, 1.0, 500.0, 10.0, false), MIN_THROTTLE);
        assert_eq!(throttle(1.7, 1.0, 500.0, 10.0, false), 1.0, "clamped to 1");
    }

    #[test]
    fn path_done_stops() {
        assert_eq!(throttle(1.0, 0.0, 15.0, 10.0, true), 0.0);
        assert_eq!(throttle(0.5, 3.0, 15.0, 10.0, true), 0.0);
    }

    #[test]
    fn arrive_ramp_runs_from_full_at_2r_to_a_tenth_at_r() {
        assert_eq!(arrive(20.0, 10.0, false), 1.0);
        assert!((arrive(15.0, 10.0, false) - 0.55).abs() < 1e-6);
        assert!((arrive(10.0, 10.0, false) - MIN_THROTTLE).abs() < 1e-6);
        assert!((arrive(3.0, 10.0, false) - MIN_THROTTLE).abs() < 1e-6, "inside R it floors at 0.1");
        assert_eq!(arrive(40.0, 10.0, false), 1.0, "clamped at 1");
        assert_eq!(arrive(15.0, 10.0, true), 0.0);
    }

    #[test]
    fn arrive_is_entered_only_when_the_path_ends() {
        // The stopping-distance helper cancels to zero: far from the end the throttle is the steer/turn one, never the ramp.
        assert_eq!(stopping_distance(), 0.0);
        assert_eq!(throttle(0.5, 0.0, 15.0, 10.0, false), 1.0);
    }
}
