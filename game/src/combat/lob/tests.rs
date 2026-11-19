#[test]
fn a_ram_factor_follows_who_owns_the_struck_ship() {
    // (victim human, rammer human): Easy 0.25 in / 3 out, Normal 0.5 / 2, Hard 1 / 1; computer against computer is x1.
    for (d, inc, out) in [(0, 0.25, 3.0), (1, 0.5, 2.0), (2, 1.0, 1.0)] {
        assert_eq!(ram_scale(true, false, d), inc);
        assert_eq!(ram_scale(false, true, d), out);
        assert_eq!(ram_scale(false, false, d), 1.0, "an allied computer ship or a rock rams like any computer");
    }
}

#[test]
fn a_gravity_well_pulls_hardest_at_half_its_radius_and_not_at_the_centre_or_rim() {
    let w = GravWell { radius: 40.0, magnitude: 40000.0, left: 17.0 };
    assert_eq!((w.force(0.0), w.force(40.0), w.force(55.0)), (0.0, 0.0, 0.0));
    assert!((w.force(20.0) - 40000.0).abs() < 1.0);
    assert!((w.force(10.0) - 10000.0).abs() < 1.0 && (w.force(30.0) - 10000.0).abs() < 1.0, "a quarter at 10 and 30 units");
}

use super::*;

#[test]
fn a_nova_splits_into_one_straight_and_four_turned_secondaries() {
    let v = Vec3::new(30.0, 0.0, -40.0);
    let out = nova_split(v);
    assert_eq!(out[0], v);
    for w in &out[1..] {
        assert!((w.length() - 50.0).abs() < 1e-3);
        // Turned 0.1 rad about two perpendicular axes: about 0.141 rad off the straight one.
        assert!((w.angle_between(v) - 0.1414).abs() < 2e-3, "{}", w.angle_between(v));
    }
    assert!(out[1].y < 0.0 && out[4].y > 0.0, "two dip and two rise");
}

#[test]
#[ignore = "needs imported assets"]
fn a_star_blast_strikes_every_section_once_as_its_ring_passes_and_spares_nobody() {
    let lib = Library::load(crate::assets_dir());
    let class = lib.index("Ship_Navy_Cutter");
    let ship = Ship { heading: 0.0, ..Ship::new(class, Side::Enemy) };
    let data = Library::load(crate::assets_dir());
    let c = &data.classes[class];
    let bullet = "Bullet_StarMortar";
    let Payload::Blast { radius, .. } = lib.bullets[bullet].payload else { panic!("star mortar is a blast") };
    let damage = lib.bullets[bullet].damage.clone();
    let mut app = App::new();
    app.insert_resource(lib)
        .insert_resource(Rng(5))
        .init_resource::<Time>()
        .init_resource::<FxLib>()
        .add_message::<Hit>()
        .add_message::<super::Splashed>()
        .add_message::<Destroyed>()
        .add_message::<Sfx>()
        .add_systems(Update, blasts);
    let e = app.world_mut().spawn((Transform::default(), ship, Health::new(c))).id();
    // Fired by the ship's own side: it is struck all the same (same side: no difficulty factor).
    let at = Vec3::new(20.0, 0.0, 0.0);
    let blast = Blast { bullet: bullet.into(), shooter: None, owner: Some((Side::Enemy, false)), radius, duration: 3.0, age: 0.0 };
    app.world_mut().spawn((blast, Transform::from_translation(at)));
    for _ in 0..100 {
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(1.0 / 30.0));
        app.update();
    }
    let hp = &app.world().get::<Health>(e).unwrap().sections;
    for (k, sec) in c.sections.iter().enumerate() {
        let r = ((sec.min + sec.max) / 2.0).distance(at);
        let [lo, hi, ..] = damage[material_key(&sec.material)];
        let d = (1.0 - r / radius).max(0.1);
        let lost = sec.hp - hp[k];
        if r < radius {
            // The roll is rounded to whole hit points after scaling (shells.rs damage_roll), so allow half a point either way.
            assert!(lost >= (lo * d).round() - 1e-3 && lost <= (hi * d).round() + 1e-3, "{}: lost {lost} at {r}, want {lo}-{hi} x {d}", sec.label);
        } else {
            assert_eq!(lost, 0.0, "{} is out of reach", sec.label);
        }
    }
}

#[test]
#[ignore = "needs imported assets"]
fn storm_wind_uses_weather_damage_and_respects_mission_protection() {
    let lib = Library::load(crate::assets_dir());
    let class = lib.index("Ship_Navy_Sloop");
    let k = lib.classes[class].sections.iter().position(|s| s.material.contains("Cloth")).unwrap();
    let hp = Health::new(&lib.classes[class]);
    let before = hp.sections[k];
    let core = hp.core;
    let ship = Ship { heading: 0.0, ..Ship::new(class, crate::Side::Enemy) };
    let tables: std::collections::HashMap<String, std::collections::HashMap<String, [f32; 5]>> =
        serde_json::from_value(crate::data::read_json(&crate::assets_dir().join("data/nebula.json"))).unwrap();
    assert_eq!(&tables["wind"]["Cloth"][..2], &[5.0, 7.0]);
    let mut app = App::new();
    app.insert_resource(lib).insert_resource(Rng(123)).add_message::<Strike>().add_systems(Update, strikes);
    let e = app.world_mut().spawn((Transform::default(), ship, hp, crate::campaign::MissionShip::default())).id();
    for _ in 0..3 {
        app.world_mut().write_message(Strike { victim: e, damage: tables["wind"].clone(), section: k });
    }
    app.world_mut().write_message(Strike { victim: e, damage: tables["meteor"].clone(), section: usize::MAX });
    app.update();
    let hp = app.world().get::<Health>(e).unwrap();
    assert!((15.0..=21.0).contains(&(before - hp.sections[k])));
    assert_eq!(hp.core, core);
    let after = hp.sections[k];
    app.world_mut().get_mut::<crate::campaign::MissionShip>(e).unwrap().invuln = 1.0;
    app.world_mut().write_message(Strike { victim: e, damage: tables["meteor"].clone(), section: k });
    app.update();
    assert_eq!(app.world().get::<Health>(e).unwrap().sections[k], after);
    // The hit carries no attacker: the human's own ship takes the difficulty factor, a computer ship does not.
    let mine = app.world().get::<Ship>(e).unwrap().clone();
    let mine = Ship { side: crate::Side::Player, ..mine };
    let h2 = Health::new(&app.world().resource::<Library>().classes[mine.class]);
    let before2 = h2.sections[k];
    let p = app.world_mut().spawn((Transform::default(), mine, h2)).id();
    for _ in 0..3 {
        app.world_mut().write_message(Strike { victim: p, damage: tables["wind"].clone(), section: k });
    }
    app.update();
    let f = super::super::shells::side_damage_scale(crate::Side::Enemy, crate::Side::Player, crate::menu::difficulty(), false);
    let lost = before2 - app.world().get::<Health>(p).unwrap().sections[k];
    assert!(f < 1.0 && lost <= 21.0 * f + 3.0, "the human's ship takes the difficulty factor ({lost} lost, factor {f})");
}

#[test]
#[ignore = "needs imported assets"]
fn a_ram_hits_only_the_section_nearest_the_contact() {
    let lib = Library::load(crate::assets_dir());
    let mut app = App::new();
    let ship = |class, side| Ship { heading: 0.0, ..Ship::new(class, side) };
    let [cutter, mow] = ["Ship_Navy_Cutter", "Ship_Navy_ManOWar"].map(|n| lib.index(n));
    // Two computer sides: no difficulty factor.
    let a = app.world_mut().spawn((ship(cutter, crate::Side(1)), Transform::default(), Health::new(&lib.classes[cutter]))).id();
    let b = app.world_mut().spawn((ship(mow, crate::Side(2)), Transform::default(), Health::new(&lib.classes[mow]))).id();
    let full = Health::new(&lib.classes[cutter]).sections;
    let s = &lib.classes[cutter].sections[0];
    let at = (s.min + s.max) * 0.5;
    // A Cutter rammed by an enemy Man-O-War at 9 units/s: 0.0004 x 40000 x 9 = 144 hit points.
    let damage = 0.0004 * lib.classes[mow].phys.mass * 9.0;
    app.insert_resource(lib).insert_resource(Rng(7)).add_message::<crate::collision::Ram>().add_message::<crate::audio::Sfx>().add_systems(Update, ram_damage);
    app.world_mut().write_message(crate::collision::Ram { a, b, damage, at });
    app.update();
    let sounds: Vec<String> = app.world_mut().resource_mut::<Messages<crate::audio::Sfx>>().drain().map(|s| s.name).collect();
    assert!(sounds.len() == 1 && sounds[0].starts_with("Damage_"), "the struck section's own damage sound: {sounds:?}");
    let hp = &app.world().get::<Health>(a).unwrap().sections;
    assert_eq!(hp[0], (full[0] - damage).max(0.0), "the nearest section takes it all");
    // It kills the 120-point bow, which spills 0.4 of the hit to its neighbours only.
    let lib = app.world().resource::<Library>();
    let adjacent = &lib.classes[cutter].sections[0].adjacent;
    for n in 1..hp.len() {
        let want = if adjacent.contains(&n) { (full[n] - 0.4 * damage).max(0.0) } else { full[n] };
        assert!((hp[n] - want).abs() < 1e-3, "section {n}: {} vs {want}", hp[n]);
    }
}

#[test]
#[ignore = "needs imported assets"]
fn a_hit_on_a_dead_section_spills_four_tenths_to_its_neighbours() {
    let lib = Library::load(crate::assets_dir());
    let c = lib.index("Ship_Navy_Cutter");
    let class = &lib.classes[c];
    let k = class.sections.iter().position(|s| s.label == "bow_wood").unwrap();
    let mut hp = Health::new(class);
    hp.sections[k] = 0.0;
    let full = hp.sections.clone();
    apply_ship_hit(class, &mut hp, k, [10.0, 10.0, 0.0, 0.0, 0.0], 0.0, false, &mut Rng(7));
    for (n, (now, was)) in hp.sections.iter().zip(&full).enumerate() {
        let want = if class.sections[k].adjacent.contains(&n) { was - 4.0 } else { *was };
        assert!((now - want).abs() < 1e-4, "{}: {now} vs {want}", class.sections[n].label);
    }
    assert!(!class.sections[k].adjacent.is_empty(), "the bow has neighbours");
}

#[test]
fn lob_arc_lands_on_the_aim_point() {
    let d = Vec3::new(300.0, -2.0, -150.0);
    const G: f32 = 1.263; // retail star mortar
    let v = super::super::lob_velocity(d, std::f32::consts::FRAC_PI_4, G).unwrap();
    // Fly the ballistic arc and find where it comes back down to the target height.
    let (mut p, mut vel, dt) = (Vec3::ZERO, v, 0.001);
    while !(vel.y < 0.0 && p.y <= d.y) {
        vel.y -= G * dt;
        p += vel * dt;
    }
    assert!(p.distance(d) < 1.5, "landed {p} for {d}");
    assert!(super::super::lob_velocity(Vec3::new(10.0, 50.0, 0.0), 0.3, G).is_none(), "point above the arc");
}
