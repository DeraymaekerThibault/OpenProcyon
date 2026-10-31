use super::*;
use crate::collision::{Bounds, BoxBody, Triangle};

// A Frigate-sized hull: 6 wide, 30 long, sphere radius 15 (hand-authored, not imported data).
const BOUNDS: [Vec3; 2] = [Vec3::new(-3.0, -2.0, -15.0), Vec3::new(3.0, 2.0, 15.0)];
fn me(speed: f32) -> Me {
    Me { pos: Vec3::ZERO, heading: 0.0, speed, bounds: BOUNDS, radius: 15.0, priority: speed, lifeboat: false }
}
/// A wall across the course `z` units ahead (heading 0 sails toward -z), `half` units either side of the line, offset by `x`.
fn wall(z: f32, x: f32, half: f32) -> Obstacle {
    let t = [Vec3::new(x - half, -2.0, -z), Vec3::new(x + half, -2.0, -z), Vec3::new(x, 2.0, -z)];
    Obstacle { body: BoxBody { points: t.to_vec(), bounds: Bounds::points(&t) }, triangles: vec![Triangle { points: t, bounds: Bounds::points(&t) }] }
}
fn island(o: &Obstacle) -> Other<'_> {
    let (centre, radius) = o.sphere();
    Other { key: Key::Rock(0), centre, radius, priority: 0.0, lifeboat: false, shape: Shape::Fixed { obstacle: o, island: true } }
}
fn ship(at: Vec3, heading: f32, speed: f32, priority: f32) -> Other<'static> {
    let velocity = Vec3::new(-heading.sin(), 0.0, -heading.cos()) * speed;
    Other { key: Key::Rock(1), centre: at, radius: 15.0, priority, lifeboat: false, shape: Shape::Ship { bounds: BOUNDS, heading, velocity } }
}
const SELF: Key = Key::Rock(99);
const FAR: Vec3 = Vec3::new(0.0, 0.0, -2000.0);

#[test]
fn land_on_the_line_caps_speed_by_the_distance_to_contact_and_sets_a_detour() {
    let cap = |coast: f32| {
        let w = wall(coast, 0.0, 40.0);
        let mut a = Avoid::new(0.0);
        a.think(&me(6.6), FAR, &[island(&w)], SELF);
        (a.cap, a.detour)
    };
    let (far, detour) = cap(70.0);
    assert!(detour.is_some(), "turns away from land on the line");
    // Contact is found once per hull length of travel (30 units here), so the gap moves in those steps.
    assert!(far > cap(30.0).0 && cap(30.0).0 > cap(10.0).0, "slower the closer the contact");
    assert_eq!(cap(200.0), (1.0, None), "land beyond 10 s of travel does not count");
    assert!((cap(1.0).0 - 0.1).abs() < 1e-3, "never below 0.1 for land");
}

#[test]
fn land_beside_the_line_is_passed_at_full_speed() {
    // Retail Frigate passed an island 27 units off its line at full speed.
    let w = wall(60.0, 27.0 + 3.0 + 40.0, 40.0);
    let mut a = Avoid::new(0.0);
    a.think(&me(6.6), FAR, &[island(&w)], SELF);
    assert_eq!((a.cap, a.detour), (1.0, None));
}

#[test]
fn the_detour_is_the_smallest_clear_turn_and_goes_round_the_short_side() {
    // A wall reaching far to the left of the line and only a little to the right: the turn goes right (negative x).
    let w = wall(60.0, -30.0, 40.0);
    let mut a = Avoid::new(0.0);
    a.think(&me(6.6), FAR, &[island(&w)], SELF);
    let d = a.detour.expect("detour");
    assert!(d.x > 0.0, "round the short end of the wall, got {d}");
    assert!(d.length() >= 60.0 - 1e-3, "at least 4 radii ahead");
}

#[test]
fn after_three_seconds_on_the_same_ship_the_lower_priority_ship_stops_for_four() {
    // A ship 50 ahead on the line (held in place so the right-of-way rule is tested, not the prediction): the faster of the two keeps
    // detouring, the slower one waits.
    let mut fast = Avoid::new(0.0);
    let mut slow = Avoid::new(0.0);
    let other_slow = ship(Vec3::new(0.0, 0.0, -50.0), PI, 0.0, 2.0);
    let other_fast = ship(Vec3::new(0.0, 0.0, -50.0), PI, 0.0, 6.6);
    for _ in 0..6 {
        fast.think(&me(6.6), FAR, std::slice::from_ref(&other_slow), SELF);
        slow.think(&Me { priority: 3.0, ..me(3.0) }, FAR, std::slice::from_ref(&other_fast), SELF);
    }
    assert!(fast.cap > 0.0 && slow.cap > 0.0, "five repeats are not yet a reason to stop");
    fast.think(&me(6.6), FAR, std::slice::from_ref(&other_slow), SELF);
    slow.think(&Me { priority: 3.0, ..me(3.0) }, FAR, std::slice::from_ref(&other_fast), SELF);
    assert!(fast.cap > 0.0, "the faster ship has right of way");
    assert_eq!(slow.cap, 0.0, "the slower ship stops");
    let thinks = (0..20).filter(|_| slow.due(THINK)).count();
    assert!((12..=13).contains(&thinks) && slow.cap == 1.0, "waits 4 s (8 thinks skipped), then thinks again ({thinks} thinks in 10 s)");
}

#[test]
fn a_computer_ship_gives_way_to_the_humans_moving_ship_but_never_stops_for_land() {
    let human = ship(Vec3::new(0.0, 0.0, -50.0), PI, 0.0, 1.0 + HUMAN_PRIORITY);
    let mut a = Avoid::new(0.0);
    for _ in 0..7 {
        a.think(&me(6.6), FAR, std::slice::from_ref(&human), SELF);
    }
    assert_eq!(a.cap, 0.0, "stops for the human's ship");
    let w = wall(40.0, 0.0, 400.0);
    let mut b = Avoid::new(0.0);
    for _ in 0..12 {
        b.think(&me(6.6), FAR, &[island(&w)], SELF);
    }
    assert!(b.cap > 0.0, "a moving ship outranks land");
}

#[test]
fn a_ship_wedged_against_land_turns_in_place_after_eleven_thinks_toward_the_smaller_clear_turn() {
    // The bow 1 unit into a wide wall dead ahead, creeping at 0.5 units/s.
    let w = wall(14.0, 0.0, 400.0);
    let mut a = Avoid::new(0.0);
    for _ in 0..11 {
        a.think(&me(0.5), FAR, &[island(&w)], SELF);
    }
    assert_eq!(a.stuck, None, "not before the twelfth think on the same obstacle");
    a.think(&me(0.5), FAR, &[island(&w)], SELF);
    assert_eq!(a.stuck, Some(-1.0), "both sides clear at the same turn: the minus side, as the original");
    let free = Me { heading: -PI / 2.0, ..me(0.5) };
    a.think(&free, FAR, &[island(&w)], SELF);
    assert_eq!(a.stuck, None, "released once the bow is clear");
    let mut fast = Avoid::new(0.0);
    for _ in 0..20 {
        fast.think(&me(6.6), FAR, &[island(&w)], SELF);
    }
    assert_eq!(fast.stuck, None, "a ship still under way is never stuck");
}
