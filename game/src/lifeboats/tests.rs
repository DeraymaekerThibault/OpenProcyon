use super::*;
use crate::combat::Rng;

fn app(mission: &str) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<WorldAsset>()
        .init_resource::<crate::fx::FxLib>()
        .insert_resource(crate::testutil::lib())
        .insert_resource(crate::combat::SimRng::new(5))
        .init_resource::<crate::scenario::Difficulty>()
        .insert_resource(crate::campaign::load_id(mission))
        .add_message::<ShipLost>()
        .add_message::<crate::audio::Sfx>()
        .add_systems(Update, (crate::tick_start, lifeboats).chain());
    app
}
fn ship(app: &mut App, class: &str, owner: &str, at: Vec3, flag: bool) -> Entity {
    let lib = app.world().resource::<Library>();
    let ci = lib.index(class);
    let health = Health::new(&lib.classes[ci]);
    let s = Ship { heading: 0.0, ..Ship::new(ci, Side::Player) };
    let ms = MissionShip {
        id: -1,
        name: class.into(),
        owner: owner.into(),
        hold_fire: false,
        invuln: 0.0,
        visible: true,
        stance: crate::rules::Stance::Neutral,
        ordered: None,
        attack: vec![],
        flag: String::new(),
        banner: String::new(),
        looping: None,
        stop_at_end: false,
        create_lifeboats: flag,
        ..Default::default()
    };
    app.world_mut().spawn((s, health, Transform::from_translation(at), ms)).id()
}
fn boats(app: &mut App) -> Vec<(crate::names::Owner, String, Vec3)> {
    let mut q = app.world_mut().query::<(&Lifeboat, &Ship, &Transform)>();
    let lib = app.world().resource::<Library>();
    q.iter(app.world()).map(|(l, s, t)| (l.owner.clone(), lib.classes[s.class].name.clone(), t.translation)).collect()
}
fn lose(app: &mut App, e: Entity) {
    app.world_mut().write_message(ShipLost(e));
    app.update();
}

#[test]
fn drift_is_uniform_in_the_exe_ranges_and_deterministic() {
    let (mut a, mut b) = (Rng::new(9), Rng::new(9));
    for _ in 0..200 {
        let (o, t) = drift(&mut a);
        assert!(o.abs().max_element() <= 100.0 && (0.2..=1.0).contains(&t));
        assert_eq!((o, t), drift(&mut b));
    }
    assert_eq!([class_for("Navy"), class_for("Procyon"), class_for("Weird")], ["Ship_Navy_Lifeboat", "Ship_Procyon_Lifeboat", "Ship_Civilian_Lifeboat"]);
    assert!(surface_distance((Vec3::ZERO, 2.0), (Vec3::X * 3.0, 1.0)) <= 0.0 && surface_distance((Vec3::ZERO, 2.0), (Vec3::X * 3.1, 1.0)) > 0.0);
}

#[test]
#[ignore = "needs imported assets"]
fn creation_follows_flag_count_race_and_current_owner() {
    let mut app = app("mission02");
    let at = Vec3::new(10.0, 0.0, 20.0);
    let plain = ship(&mut app, "Ship_Pirate_Schooner", "Pirates", at, false);
    lose(&mut app, plain);
    assert!(boats(&mut app).is_empty(), "a ship no rule named leaves nothing");
    for (class, owner, n, boat) in [
        ("Ship_Pirate_Schooner", "Pirates", 2, "Ship_Pirate_Lifeboat"),
        ("Ship_Pirate_Sloop", "Pirates", 1, "Ship_Pirate_Lifeboat"),
        ("Ship_Civilian_Merchant", "Schooner1", 6, "Ship_Civilian_Lifeboat"),
        (
            "Ship_Navy_Tender",
            "Pirates",
            3.min(app.world().resource::<Library>().classes[app.world().resource::<Library>().index("Ship_Navy_Tender")].lifeboats as usize),
            "Ship_Navy_Lifeboat",
        ),
        ("Ship_Procyon_Dreadnought", "Pirates", 0, "Ship_Procyon_Lifeboat"),
    ] {
        let e = ship(&mut app, class, owner, at, true);
        lose(&mut app, e);
        let got = boats(&mut app);
        assert_eq!(got.len(), n, "{class}");
        assert!(got.iter().all(|b| b.0 == owner && b.1 == boat && b.2 == at), "{class}: owner of the moment, race class, at the wreck");
        let all: Vec<_> = app.world_mut().query_filtered::<Entity, With<Lifeboat>>().iter(app.world()).collect();
        all.into_iter().for_each(|b| {
            app.world_mut().despawn(b);
        });
    }
}

#[test]
#[ignore = "needs imported assets"]
fn boats_sail_straight_at_a_fraction_of_class_speed_and_never_collide_with_ships() {
    let mut app = app("mission02");
    let parent = ship(&mut app, "Ship_Pirate_Schooner", "Pirates", Vec3::ZERO, true);
    lose(&mut app, parent);
    let mut q = app.world_mut().query_filtered::<&Ship, With<Lifeboat>>();
    for s in q.iter(app.world()) {
        assert_eq!((s.order, s.waypoints.len(), s.side), (EngineOrder::Full, 1, Side::Player));
        assert!((0.2..=1.0).contains(&s.throttle.unwrap()) && s.waypoints[0].length() > 1.0e3);
        let to = s.waypoints[0].normalize();
        assert!((Vec3::new(-s.heading.sin(), 0.0, -s.heading.cos()) - to).length() < 1e-3, "heading faces its goal");
    }
    // a ship on top of a boat is not pushed (a ship-lifeboat contact has no response), whichever way round
    let lib = app.world().resource::<Library>();
    let (cutter, boat) = (lib.index("Ship_Navy_Cutter"), lib.index("Ship_Navy_Lifeboat"));
    let mk = |class| Ship { heading: 0.0, ..Ship::new(class, Side::Player) };
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs_f32(0.1));
    let mut w = App::new();
    w.insert_resource(time).insert_resource(crate::testutil::lib()).add_systems(Update, crate::collision::sail);
    let health = |w: &App, c: usize| Health::new(&w.world().resource::<Library>().classes[c]);
    let (h1, h2) = (health(&w, cutter), health(&w, boat));
    let a = w.world_mut().spawn((mk(cutter), h1, Transform::from_xyz(0.0, 0.0, 0.0), crate::collision::Drift::default())).id();
    let b = w.world_mut().spawn((mk(boat), h2, Transform::from_xyz(0.5, 0.0, 0.5), crate::collision::Drift::default(), Lifeboat { owner: "x".into() })).id();
    for _ in 0..10 {
        w.update();
    }
    assert_eq!(w.world().get::<Transform>(a).unwrap().translation, Vec3::ZERO);
    assert_eq!(w.world().get::<Transform>(b).unwrap().translation, Vec3::new(0.5, 0.0, 0.5));
}

#[test]
#[ignore = "needs imported assets"]
fn pickup_by_nearest_live_non_lifeboat_ship_in_sphere_contact() {
    let mut app = app("mission02");
    let boat = |app: &mut App, at| {
        let e = ship(app, "Ship_Pirate_Lifeboat", "Pirates", at, false);
        app.world_mut().entity_mut(e).insert(Lifeboat { owner: "Pirates".into() }).remove::<MissionShip>();
        e
    };
    let b = boat(&mut app, Vec3::ZERO);
    let far = ship(&mut app, "Ship_Navy_Cutter", "PlayerStart", Vec3::new(0.0, 0.0, 15.5), false); // just outside the 12 + boat sphere
    app.update();
    assert!(app.world().get_entity(b).is_ok(), "just outside contact: not collected");
    assert!(!app.world().resource::<Mission>().script.lifeboat_owners().is_empty());
    let dead = ship(&mut app, "Ship_Navy_Cutter", "Schooner2", Vec3::ZERO, false);
    app.world_mut().get_mut::<Health>(dead).unwrap().destroyed = true;
    let other_boat = boat(&mut app, Vec3::ZERO);
    app.update();
    assert!(app.world().get_entity(b).is_ok() && app.world().get_entity(other_boat).is_ok(), "dead ships and other lifeboats never collect");
    app.world_mut().get_mut::<Transform>(far).unwrap().translation.z = 6.0; // inside contact
    let nearer = ship(&mut app, "Ship_Navy_Cutter", "Schooner2", Vec3::new(0.0, 0.0, 3.0), false);
    app.update();
    app.update();
    let m = &app.world().resource::<Mission>().script;
    assert_eq!((m.lifeboats_saved_by("Schooner2", "Pirates"), m.lifeboats_saved_by("PlayerStart", "Pirates")), (2, 0), "an AI ally collects both, only the nearest collects");
    assert!(app.world().get_entity(b).is_err() && app.world().get_entity(other_boat).is_err() && app.world().get_entity(nearer).is_ok());
    assert_eq!(m.score, 0, "AI collection scores nothing");
    assert!(m.lifeboat_owners().is_empty());
    assert_eq!(app.world().resource::<Messages<crate::audio::Sfx>>().len(), 2, "one pickup sound per boat");
}

#[test]
#[ignore = "needs imported assets"]
fn preplaced_mission4_boats_are_lifeboats_by_flag_and_mission9_longboat_is_not() {
    let lib = crate::testutil::lib();
    let flagged = |mission: &str, id: i64| {
        let d = crate::campaign::load_id(mission).data;
        let s = d["ships"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap().clone();
        lib.classes.iter().any(|c| c.is_lifeboat && Some(c.name.as_str()) == s["type"].as_str())
    };
    assert!([220, 221, 222].iter().all(|id| flagged("mission04", *id)));
    assert!(!flagged("mission09", 303), "Ship_Navy_Lifeboat_Mission9 is a combat story ship");
}

// ---- Not retail: `OP Abandon Ship` (the `Abandoned` marker), with synthetic classes (no imported assets).

/// A world with a hull class that carries two boats and its race's lifeboat class.
fn abandon_app() -> App {
    let hull = crate::data::Class { name: "Hull".into(), race: "Navy".into(), lifeboats: 2, ..crate::fixed_tests::synthetic_class() };
    let boat = crate::data::Class { name: class_for("Navy"), is_lifeboat: true, ..crate::fixed_tests::synthetic_class() };
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<WorldAsset>()
        .init_resource::<crate::fx::FxLib>()
        .insert_resource(Library { classes: vec![hull, boat], guns: default(), bullets: default(), names: default(), clips: default() })
        .insert_resource(crate::combat::SimRng::new(5))
        .add_message::<ShipLost>()
        .add_message::<crate::audio::Sfx>()
        .add_systems(Update, (crate::tick_start, lifeboats).chain());
    app
}
fn hull(app: &mut App, at: Vec3) -> Entity {
    let health = Health::new(&app.world().resource::<Library>().classes[0]);
    let ms = MissionShip { name: "Hull".into(), owner: "Navy".into(), visible: true, create_lifeboats: true, ..default() };
    app.world_mut().spawn((Ship::new(0, Side::Player), health, Transform::from_translation(at), ms)).id()
}
fn boat_count(app: &mut App) -> usize {
    app.world_mut().query::<&Lifeboat>().iter(app.world()).count()
}

#[test]
fn an_abandoned_hull_launches_its_boats_once_and_does_not_collect_them() {
    let mut app = abandon_app();
    let ship = hull(&mut app, Vec3::ZERO);
    app.update();
    assert_eq!(boat_count(&mut app), 0);
    app.world_mut().entity_mut(ship).insert(Abandoned);
    app.update();
    assert_eq!(boat_count(&mut app), 2, "the crew takes to the boats, by the same flag and class count as a death");
    // The boats start at the hull, inside its bounding sphere: the hull they left must not take them back in.
    for _ in 0..5 {
        app.update();
    }
    assert_eq!(boat_count(&mut app), 2, "nobody is aboard the abandoned hull to collect a boat");
    // A later death of the hull leaves no second set.
    app.world_mut().get_mut::<Health>(ship).unwrap().destroyed = true;
    lose(&mut app, ship);
    app.update();
    assert_eq!(boat_count(&mut app), 2, "the boats left when the ship was abandoned");
}

#[test]
fn an_abandoned_hull_collects_nobody_elses_boat_and_a_crewed_ship_still_does() {
    let mut app = abandon_app();
    let (wreck, other) = (hull(&mut app, Vec3::ZERO), hull(&mut app, Vec3::new(500.0, 0.0, 0.0)));
    app.world_mut().entity_mut(wreck).insert(Abandoned);
    app.world_mut().get_mut::<MissionShip>(wreck).unwrap().create_lifeboats = false;
    app.update();
    // The other ship sinks beside the abandoned hull: its boats rest on the wreck and stay.
    app.world_mut().get_mut::<Transform>(other).unwrap().translation = Vec3::new(0.5, 0.0, 0.0);
    app.world_mut().get_mut::<Health>(other).unwrap().destroyed = true;
    lose(&mut app, other);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(boat_count(&mut app), 2, "the wreck has no crew to take them in");
    // A crewed ship in the same place collects them, as before.
    app.world_mut().entity_mut(wreck).remove::<Abandoned>();
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(boat_count(&mut app), 0, "a live, crewed ship touching a boat collects it");
}
