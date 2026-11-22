use super::*;
use std::f32::consts::{FRAC_PI_2, PI};

const FOE: Vec3 = Vec3::ZERO;

#[test]
fn quadrants_run_counter_clockwise_from_the_bow() {
    assert_eq!([0.1, FRAC_PI_2 + 0.1, PI + 0.1, -0.1].map(quadrant), [0, 3, 2, 1]);
}

#[test]
fn a_broadside_aims_r_abeam_of_the_target_on_the_chosen_side_then_beyond_it() {
    // The ship 300 units south (+z) of the target: the line of sight points north (-z), so starboard of it is +x.
    let pos = Vec3::new(0.0, 0.0, 300.0);
    let port = course(Routine::Broadside(1.0), pos, 0.0, FOE, 250.0, 90.0, 0.0, &mut None);
    assert_eq!(port, vec![Vec3::new(90.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -90.0)], "aim right of the target: it stays to port");
    let starboard = course(Routine::Broadside(-1.0), pos, 0.0, FOE, 250.0, 90.0, 0.0, &mut None);
    assert_eq!(starboard[0], Vec3::new(-90.0, 0.0, 0.0));
}

#[test]
fn a_frontal_attacker_charges_to_r_then_breaks_off_square_for_up_to_fifteen_seconds() {
    let mut b = None;
    let far = Vec3::new(0.0, 0.0, 300.0);
    assert_eq!(course(Routine::Frontal, far, 0.0, FOE, 250.0, 130.0, 0.0, &mut b), vec![FOE], "outside R: straight at it");
    let near = Vec3::new(0.0, 0.0, 120.0);
    let leg = course(Routine::Frontal, near, 0.0, FOE, 100.0, 130.0, 1.0, &mut b);
    assert!(b.is_some() && leg[0].z == near.z && (leg[0].x - near.x).abs() == 130.0, "breaks off at right angles: {leg:?}");
    assert_eq!(course(Routine::Frontal, near, 0.0, FOE, 100.0, 130.0, 15.9, &mut b), leg, "keeps the same leg");
    assert_eq!(course(Routine::Frontal, near, 0.0, FOE, 140.0, 130.0, 16.0, &mut b), vec![FOE], "out of range: charges again");
    assert!(leg[0].x > near.x, "a target dead ahead: breaks off to starboard");
    let mut b = None;
    let port_bow = course(Routine::Frontal, near, 0.0, Vec3::new(-60.0, 0.0, 0.0), 100.0, 130.0, 1.0, &mut b);
    assert!(port_bow[0].x < near.x, "a target on the port bow: turns to port, toward it: {port_bow:?}");
    let mut b = Some((0.0, Vec3::X));
    course(Routine::Frontal, near, 0.0, FOE, 100.0, 130.0, 15.0, &mut b);
    assert!(b.is_some_and(|(since, _)| since == 15.0), "a new break-off after 15 s");
}

#[test]
fn a_rear_attacker_opens_to_r_and_closes_again() {
    let pos = Vec3::new(0.0, 0.0, 50.0);
    assert_eq!(course(Routine::Rear, pos, 0.0, FOE, 30.0, 80.0, 0.0, &mut None), vec![Vec3::new(0.0, 0.0, 130.0)], "inside R: straight away");
    assert_eq!(course(Routine::Rear, pos, 0.0, FOE, 90.0, 80.0, 0.0, &mut None), vec![FOE]);
}

#[test]
fn the_guns_astern_routine_measures_to_the_aim_point_without_the_targets_size() {
    // Attacker radius 10 at 100 units from the target's centre; the nearest section of the target sits 20 units toward it.
    let (pos, aim) = (Vec3::new(0.0, 0.0, 100.0), Vec3::new(0.0, 0.0, 20.0));
    for foe_radius in [15.0, 60.0] {
        assert_eq!(range_gap(Routine::Rear, pos, 10.0, FOE, foe_radius, aim), 70.0, "a bigger target does not push it out");
    }
    assert_eq!(range_gap(Routine::Frontal, pos, 10.0, FOE, 60.0, aim), 30.0, "frontal: the gap between the hulls");
}

#[test]
fn full_throttle_within_eleven_degrees_of_the_bow_else_the_ships_minimum() {
    let e = Entity::from_raw_u32(7).unwrap();
    let pos = Vec3::ZERO;
    assert_eq!(throttle(Routine::Frontal, e, pos, 0.0, Vec3::new(0.0, 0.0, -100.0)), None, "dead ahead: full");
    assert_eq!(throttle(Routine::Frontal, e, pos, 0.0, Vec3::new(19.0, 0.0, -100.0)), None, "10.8 degrees off: still full");
    let min = throttle(Routine::Frontal, e, pos, 0.0, Vec3::new(100.0, 0.0, 0.0)).unwrap();
    assert!((1.3..=1.5).contains(&min), "0.65-0.75 of Emergency: {min}");
    assert_eq!(throttle(Routine::Frontal, e, pos, 1.0, Vec3::new(100.0, 0.0, 0.0)), Some(min), "drawn once per ship");
}

#[test]
fn a_ram_goes_full_throttle_only_within_five_degrees_of_the_bow() {
    let e = Entity::from_raw_u32(7).unwrap();
    let pos = Vec3::ZERO;
    assert_eq!(throttle(Routine::Ram, e, pos, 0.0, Vec3::new(8.0, 0.0, -100.0)), None, "4.6 degrees off: full");
    let slow = throttle(Routine::Ram, e, pos, 0.0, Vec3::new(10.0, 0.0, -100.0));
    assert!(slow.is_some_and(|t| (1.3..=1.5).contains(&t)), "5.7 degrees off: the minimum, {slow:?}");
    assert_eq!(throttle(Routine::Broadside(1.0), e, pos, 0.0, Vec3::new(10.0, 0.0, -100.0)), None, "the other attacks still use 11.25");
}

/// The decoded choice for stock fits (procyon-re ai-captain attack_choice, same rule on the same import).
#[test]
#[ignore = "needs imported assets"]
fn stock_fits_pick_the_decoded_routine_and_range() {
    let lib = Library::load(crate::assets_dir());
    for (name, routine, r) in [
        ("Ship_Pirate_Barque", Routine::Broadside(1.0), 79.3),
        ("Ship_Navy_Frigate", Routine::Broadside(1.0), 95.6),
        ("Ship_Pirate_Carrack", Routine::Broadside(-1.0), 89.4),
        ("Ship_Procyon_Minelayer", Routine::Broadside(-1.0), 70.0),
        ("Ship_Procyon_Frigate", Routine::Frontal, 133.8),
        ("Ship_Navy_Assault_Cutter", Routine::Rear, 81.0),
    ] {
        let class = &lib.classes[lib.index(name)];
        let h = Health { sections: class.sections.iter().map(|s| s.hp).collect(), core: class.core_hp, destroyed: false };
        let (got, range) = plan(&lib, class, &h, None).unwrap();
        assert_eq!(got, routine, "{name}");
        assert!((range[0] - r).abs() < 0.1, "{name}: R {} against {r}", range[0]);
    }
}

#[test]
fn the_aim_is_the_nearest_damage_section_of_a_ship_dead_or_alive_and_a_bases_nearest_live_one() {
    let section = |x: f32, mission: bool| crate::data::Section { min: Vec3::new(x - 1.0, -1.0, -1.0), max: Vec3::new(x + 1.0, 1.0, 1.0), mission, ..Default::default() };
    let class = [section(-10.0, true), section(10.0, true)];
    let tf = Transform::from_xyz(0.0, 0.0, 100.0);
    let from = Vec3::new(50.0, 0.0, 100.0);
    assert_eq!(aim(&class, &[1.0, 0.0], &tf, from, false), Vec3::new(10.0, 0.0, 100.0), "a ship: the nearest, though dead");
    assert_eq!(aim(&class, &[1.0, 0.0], &tf, from, true), Vec3::new(-10.0, 0.0, 100.0), "a base: the nearest live one");
    assert_eq!(aim(&class, &[0.0, 0.0], &tf, from, true), tf.translation, "a base with none left: its centre");
}

#[test]
fn the_guns_aim_point_is_that_section_in_the_targets_own_frame_unflattened() {
    // A raised section (a sail at y 8) on the near side: the guns aim at its centre, height kept, and the point turns with the hull.
    let section = |x: f32, y: f32| crate::data::Section { min: Vec3::new(x - 1.0, y - 1.0, -1.0), max: Vec3::new(x + 1.0, y + 1.0, 1.0), ..Default::default() };
    let class = [section(-10.0, 0.0), section(10.0, 8.0)];
    let tf = Transform::from_xyz(0.0, 0.0, 100.0).with_rotation(Quat::from_rotation_y(PI));
    let from = Vec3::new(-50.0, 0.0, 100.0); // west of the target: the turned hull's +x section is the near one
    assert_eq!(aim_local(&class, &[1.0, 1.0], &tf, from, false), Vec3::new(10.0, 8.0, 0.0));
    assert_eq!(aim_local(&[], &[], &tf, from, false), Vec3::ZERO, "no sections: the target's origin");
}

#[test]
fn a_fully_submerged_submarine_charges_to_a_fifth_of_r_and_breaks_off_at_zero() {
    let r = 100.0;
    assert_eq!(frontal_ranges(r, false), (100.0, 100.0));
    let ranges = frontal_ranges(r, true);
    assert_eq!(ranges, (20.0, 0.0), "0.2 R to charge, 0 to break off");
    let near = Vec3::new(0.0, 0.0, 120.0);
    let mut b = None;
    // Gap 50 is inside R but outside 0.2 R: still charging.
    assert_eq!(course_to(Routine::Frontal, near, 0.0, FOE, 50.0, ranges, 0.0, &mut b), Some(vec![FOE]));
    // Gap 10 is inside 0.2 R but above 0: the break-off ends at once, no course is set and none is kept.
    assert_eq!(course_to(Routine::Frontal, near, 0.0, FOE, 10.0, ranges, 1.0, &mut b), None);
    assert!(b.is_none());
    // Touching hulls: the break-off is 0 x R from its own position.
    assert_eq!(course_to(Routine::Frontal, near, 0.0, FOE, 0.0, ranges, 2.0, &mut b), Some(vec![near]));
    // An ordinary ship at the same gaps breaks off at right angles, 100 out.
    let mut b = None;
    assert!(course_to(Routine::Frontal, near, 0.0, FOE, 50.0, frontal_ranges(r, false), 0.0, &mut b).is_some_and(|c| (c[0].x - near.x).abs() == r));
}

#[test]
fn a_standoff_attacker_keeps_the_target_dead_ahead_inside_r_and_breaks_off_only_when_it_is_too_close() {
    let (r, near) = (200.0, Vec3::new(0.0, 0.0, 150.0));
    let mut b = None;
    // Outside R and inside it down to 0.35 R: always the target itself, no break-off (a frontal ship would turn away at R).
    for gap in [400.0, 200.0, 150.0, 80.0, 71.0] {
        assert_eq!(course_to(Routine::Standoff, near, 0.0, FOE, gap, (r, r), 0.0, &mut b), Some(vec![FOE]), "gap {gap}");
        assert!(b.is_none());
    }
    // The retail frontal attack does break off at R: a ship without a beam plays as before.
    assert_ne!(course_to(Routine::Frontal, near, 0.0, FOE, 150.0, (r, r), 0.0, &mut None), Some(vec![FOE]));
    // Too close (inside 0.35 R): breaks off square, for up to 15 s or until it is back out at the hold distance (0.6 R).
    let leg = course_to(Routine::Standoff, near, 0.0, FOE, 60.0, (r, r), 1.0, &mut b).unwrap();
    assert!(b.is_some() && leg[0].z == near.z && (leg[0].x - near.x).abs() == r, "{leg:?}");
    assert_eq!(course_to(Routine::Standoff, near, 0.0, FOE, 100.0, (r, r), 8.0, &mut b), Some(leg.clone()), "keeps the leg");
    assert_eq!(course_to(Routine::Standoff, near, 0.0, FOE, 100.0, (r, r), 16.5, &mut b), Some(vec![FOE]), "15 s up: back at it");
    b = Some((1.0, Vec3::X));
    assert_eq!(course_to(Routine::Standoff, near, 0.0, FOE, 125.0, (r, r), 3.0, &mut b), Some(vec![FOE]), "far enough again: back at it");
    assert!(b.is_none());
}

#[test]
fn a_standoff_attacker_creeps_inside_0_9_r_and_crawls_at_the_hold_distance() {
    assert_eq!(standoff_throttle(300.0, 200.0), None, "closing: full");
    assert_eq!(standoff_throttle(179.0, 200.0), Some(0.25));
    assert_eq!(standoff_throttle(121.0, 200.0), Some(0.25));
    assert_eq!(standoff_throttle(100.0, 200.0), Some(0.12), "a crawl, not a stop: it must keep way on to turn");
}

#[test]
#[ignore = "needs imported assets"]
fn a_frontal_ship_is_a_standoff_attacker_only_when_its_main_gun_is_a_beam() {
    let mut lib = Library::load(crate::assets_dir());
    let class = lib.classes[lib.index("Ship_Procyon_Frigate")].mounts.iter().map(|m| m.gun.clone()).collect::<Vec<_>>();
    let health = |lib: &Library| {
        let c = &lib.classes[lib.index("Ship_Procyon_Frigate")];
        Health { sections: c.sections.iter().map(|s| s.hp).collect(), core: c.core_hp, destroyed: false }
    };
    let routine = |lib: &Library| plan(lib, &lib.classes[lib.index("Ship_Procyon_Frigate")], &health(lib), None).map(|p| p.0);
    assert_eq!(routine(&lib), Some(Routine::Frontal), "retail guns: the original's frontal attack");
    let beam = crate::data::BeamDef { charge: 2.0, duration: 6.0, tick: 0.25, ramp: 0.5, max_power: 4.0, width: 0.4, color: [1.0; 3] };
    for g in &class {
        lib.guns.get_mut(g).unwrap().beam = Some(beam.clone());
    }
    assert_eq!(routine(&lib), Some(Routine::Standoff), "the same ship with beam guns holds a stand-off");
}
