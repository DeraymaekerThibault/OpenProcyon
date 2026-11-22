//! 'OP Escort Group' (engine-added, not retail: docs/modding.md "Mission rule extensions"): a computer ship keeps station on another
//! ship and matches its speed. The steering is the fleet followers' (`fleet_follow`: full speed to the station, the leader's speed in
//! it, a crawl while turned away from it), pointed at a station measured from the leader's own heading instead of a formation slot.

use bevy::prelude::*;

/// Where the escort sails relative to the leader: `distance` units from it, on this side of its heading.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Station {
    #[default]
    Astern,
    Ahead,
    Port,
    Starboard,
}

impl Station {
    /// `Astern`, `Ahead`, `Port` or `Starboard`, any case; `None` for anything else.
    pub fn parse(s: &str) -> Option<Station> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "astern" => Some(Station::Astern),
            "ahead" => Some(Station::Ahead),
            "port" => Some(Station::Port),
            "starboard" => Some(Station::Starboard),
            _ => None,
        }
    }
}

/// A standing escort order: the leader's group spec (resolved every frame, so it follows whichever ship it names now), the side
/// to keep and the distance (0: the two hulls' radii plus `GAP`).
#[derive(Clone, PartialEq, Debug)]
pub struct Escort {
    pub target: String,
    pub station: Station,
    pub distance: f32,
}

/// Clear water between two hulls when no distance is given (the formations' own spacing constant, `R + 30`).
pub const GAP: f32 = 30.0;

/// The leader as the escort sees it.
#[derive(Clone, Copy, Debug)]
pub struct Leader {
    pub pos: Vec3,
    pub heading: f32,
    pub speed: f32,
    pub radius: f32,
}

/// The station's point: `distance` from the leader along its heading (astern, ahead) or abeam of it (port, starboard).
/// A heading of 0 points to -Z, starboard is +X.
pub fn station_point(leader: &Leader, station: Station, distance: f32) -> Vec3 {
    let forward = Vec3::new(-leader.heading.sin(), 0.0, -leader.heading.cos());
    let starboard = Vec3::new(forward.z * -1.0, 0.0, forward.x);
    let offset = match station {
        Station::Astern => -forward,
        Station::Ahead => forward,
        Station::Port => -starboard,
        Station::Starboard => starboard,
    };
    leader.pos + offset * distance
}

/// The distance to keep: the order's, else the two hulls' radii and `GAP`.
pub fn keep_distance(order: &Escort, own_radius: f32, leader_radius: f32) -> f32 {
    if order.distance > 0.0 { order.distance } else { own_radius + leader_radius + GAP }
}

/// What the escort's helm is given this frame (`Ship::waypoints`, `goal`, `order`, `throttle`).
#[derive(Clone, PartialEq, Debug)]
pub struct Course {
    pub waypoints: Vec<Vec3>,
    pub goal: Option<f32>,
    pub order: usize,
    pub throttle: f32,
}

/// The course to the station from `here` (heading `heading`, top speed `max`). Far from it: full speed round the leader if the
/// line would cross it; in it: the leader's speed and heading.
pub fn course(here: Vec3, heading: f32, max: f32, own_radius: f32, leader: &Leader, order: &Escort) -> Course {
    let station = station_point(leader, order.station, keep_distance(order, own_radius, leader.radius)).with_y(here.y);
    let distance = here.distance(station);
    let to = station - here;
    let error = if to.length_squared() > 1.0 { crate::ship::angdiff((-to.x).atan2(-to.z), heading) } else { 0.0 };
    let wanted = super::follow_speed(true, distance, leader.speed, max, error);
    let aim = super::steer_round(here, station, leader.pos, own_radius + leader.radius);
    Course {
        waypoints: if distance > 3.0 { vec![aim] } else { vec![] },
        goal: Some(leader.heading),
        order: if wanted < 0.01 { 0 } else { 2 },
        throttle: if max > 0.001 { (wanted / max).clamp(0.0, 1.0) } else { 0.0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leader(heading: f32) -> Leader {
        Leader { pos: Vec3::new(100.0, 0.0, -50.0), heading, speed: 4.0, radius: 20.0 }
    }

    #[test]
    fn stations_are_measured_from_the_leaders_heading() {
        // Heading 0 sails towards -Z: astern is +Z, starboard +X.
        let l = leader(0.0);
        assert!(station_point(&l, Station::Astern, 50.0).abs_diff_eq(Vec3::new(100.0, 0.0, 0.0), 1e-3));
        assert!(station_point(&l, Station::Ahead, 50.0).abs_diff_eq(Vec3::new(100.0, 0.0, -100.0), 1e-3));
        assert!(station_point(&l, Station::Starboard, 50.0).abs_diff_eq(Vec3::new(150.0, 0.0, -50.0), 1e-3));
        assert!(station_point(&l, Station::Port, 50.0).abs_diff_eq(Vec3::new(50.0, 0.0, -50.0), 1e-3));
        // A quarter turn to port (+90 degrees about +Y) points the bow at -X: astern is +X.
        let l = leader(std::f32::consts::FRAC_PI_2);
        assert!(station_point(&l, Station::Astern, 50.0).abs_diff_eq(Vec3::new(150.0, 0.0, -50.0), 1e-3));
        assert!(station_point(&l, Station::Starboard, 50.0).abs_diff_eq(Vec3::new(100.0, 0.0, 0.0), 1e-3), "starboard of a ship heading -X is +Z");
    }

    #[test]
    fn stations_parse_in_any_case_and_an_unknown_one_is_refused() {
        assert_eq!(Station::parse("Starboard"), Some(Station::Starboard));
        assert_eq!(Station::parse("astern"), Some(Station::Astern));
        assert_eq!(Station::parse(""), Some(Station::Astern), "the default");
        assert_eq!(Station::parse("abeam"), None);
    }

    #[test]
    fn no_distance_keeps_the_hulls_and_a_gap_apart() {
        let order = Escort { target: "p".into(), station: Station::Astern, distance: 0.0 };
        assert_eq!(keep_distance(&order, 18.0, 20.0), 68.0);
        assert_eq!(keep_distance(&Escort { distance: 90.0, ..order }, 18.0, 20.0), 90.0);
    }

    #[test]
    fn an_escort_far_from_its_station_runs_to_it_and_in_it_matches_the_leaders_speed() {
        let order = Escort { target: "p".into(), station: Station::Astern, distance: 60.0 };
        let l = leader(0.0);
        // Station (100, 0, 10). Far behind it with the bow towards -Z (heading 0): full speed, the station as the aim point.
        let far = course(Vec3::new(100.0, 0.0, 400.0), 0.0, 8.0, 18.0, &l, &order);
        assert_eq!((far.order, far.waypoints.len()), (2, 1));
        assert!((far.throttle - 1.0).abs() < 1e-4, "{far:?}");
        // Turned the other way it crawls until the bow comes round.
        let away = course(Vec3::new(100.0, 0.0, 400.0), std::f32::consts::PI, 8.0, 18.0, &l, &order);
        assert!((away.throttle - 0.2).abs() < 1e-4, "{away:?}");
        // In the station: no course, the leader's speed (4 of 8) and heading.
        let on = course(Vec3::new(100.0, 0.0, 10.0), 0.0, 8.0, 18.0, &l, &order);
        assert!(on.waypoints.is_empty());
        assert!((on.throttle - 0.5).abs() < 1e-4, "{on:?}");
        assert_eq!(on.goal, Some(0.0));
    }
}
