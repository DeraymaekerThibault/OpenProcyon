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
fn full_throttle_within_eleven_degrees_of_the_bow_else_the_ships_minimum() {
    let e = Entity::from_raw_u32(7).unwrap();
    let pos = Vec3::ZERO;
    assert_eq!(throttle(e, pos, 0.0, Vec3::new(0.0, 0.0, -100.0)), None, "dead ahead: full");
    assert_eq!(throttle(e, pos, 0.0, Vec3::new(19.0, 0.0, -100.0)), None, "10.8 degrees off: still full");
    let min = throttle(e, pos, 0.0, Vec3::new(100.0, 0.0, 0.0)).unwrap();
    assert!((1.3..=1.5).contains(&min), "0.65-0.75 of Emergency: {min}");
    assert_eq!(throttle(e, pos, 1.0, Vec3::new(100.0, 0.0, 0.0)), Some(min), "drawn once per ship");
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
