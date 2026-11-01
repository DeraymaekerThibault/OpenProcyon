use super::*;
use std::time::Duration;

#[test]
#[ignore = "needs imported assets"]
fn skirmish_dock_lights_show_on_friendly_bases_only() {
    let mut app = App::new();
    app.init_resource::<FxLib>().add_systems(Update, markers);
    let point = DockPoint { pos: Vec3::Z * 3.0, fwd: Vec3::NEG_Z };
    let mut base = |side| {
        let base = MissionBase { id: 1, owner: "b".into() };
        app.world_mut().spawn((Transform::default(), Dockable::new(vec![point], true), base, crate::combat::BaseSide(side))).id()
    };
    let (own, enemy) = (base(Side::Player), base(Side::Enemy));
    app.update();
    assert!(!app.world().get::<Dockable>(own).unwrap().markers.is_empty(), "our base shows its dock lights");
    assert!(app.world().get::<Dockable>(enemy).unwrap().markers.is_empty(), "the enemy base shows none");
}

#[test]
#[ignore = "needs imported assets"]
fn mission_two_base_docking_from_the_far_side_reaches_the_dock_round_the_island() {
    let (mut app, lib) = app();
    app.init_resource::<crate::collision::Shapes>().add_systems(Update, crate::collision::sail.after(routine));
    let map: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("maps/mission02/map.json")).unwrap()).unwrap();
    let docks = load_docks();
    let mut dockee = None;
    let mut start = Vec3::ZERO;
    for object in map["objects"].as_array().unwrap() {
        let tf = crate::scenario::scenery_transform(object, crate::scenario::game_rotation(&object["rot"]), &docks);
        let kind = object["type"].as_str().unwrap();
        let body = app.world().resource::<crate::collision::Shapes>().obstacle(kind, &tf);
        let e = app.world_mut().spawn(tf).id();
        if let Some(body) = body {
            app.world_mut().entity_mut(e).insert(body);
        }
        if kind == "Base_AbandonedCamp" {
            let dk = dockable_for(&docks, kind).unwrap();
            let p = dk.world(&tf, 0).0;
            start = (tf.translation - (p - tf.translation).with_y(0.0).normalize() * 280.0).with_y(0.0);
            app.world_mut().entity_mut(e).insert(dk);
            dockee = Some(e);
        }
    }
    let dockee = dockee.unwrap();
    let (s, h) = ship(&lib, "Ship_Navy_Torpedo", Side::Player);
    let me = app.world_mut().spawn((Transform::from_translation(start), s, h, PlayerShip, crate::collision::Drift::default())).id();
    app.world_mut().resource_mut::<Messages<DockMsg>>().write(DockMsg::Dock { docker: None, dockee });
    tick(&mut app, 0.1);
    tick(&mut app, 0.1);
    // The original plans no route: one approach point on the dock axis, and the collision avoider bends the course round the island.
    let d = app.world().get::<Docker>(me).unwrap();
    assert!(!d.ignores_collision(dockee), "the island stays solid on the approach");
    for _ in 0..6000 {
        tick(&mut app, 0.1);
        if app.world().get::<Docker>(me).unwrap().is_docked() {
            return;
        }
    }
    panic!("far-side approach did not reach the dock: {:?}", app.world().get::<Transform>(me).unwrap().translation);
}

/// A computer ship docking at a dockable island of a map: every scenery object placed as the scenario does; returns whether it docked
/// within `secs` from `start` (Bevy frame).
fn docks_at_island(map: &str, island: &str, class: &str, start: Vec3, secs: u32) -> bool {
    let (mut app, lib) = app();
    app.init_resource::<crate::collision::Shapes>().add_systems(Update, crate::collision::sail.after(routine));
    let map: Value = serde_json::from_slice(&std::fs::read(assets_dir().join(format!("maps/{map}/map.json"))).unwrap()).unwrap();
    let docks = load_docks();
    let mut dockee = None;
    for object in map["objects"].as_array().unwrap() {
        let tf = crate::scenario::scenery_transform(object, crate::scenario::game_rotation(&object["rot"]), &docks);
        let kind = object["type"].as_str().unwrap();
        let body = app.world().resource::<crate::collision::Shapes>().obstacle(kind, &tf);
        let e = app.world_mut().spawn(tf).id();
        if let Some(body) = body {
            app.world_mut().entity_mut(e).insert(body);
        }
        if kind == island {
            app.world_mut().entity_mut(e).insert(dockable_for(&docks, kind).unwrap());
            dockee = Some(e);
        }
    }
    let (s, h) = ship(&lib, class, Side::Enemy);
    let me = app.world_mut().spawn((Transform::from_translation(start), s, h, crate::collision::Drift::default())).id();
    app.world_mut().resource_mut::<Messages<DockMsg>>().write(DockMsg::Dock { docker: Some(me), dockee: dockee.unwrap() });
    (0..secs * 10).any(|_| {
        tick(&mut app, 0.1);
        app.world().get::<Docker>(me).is_some_and(Docker::is_docked)
    })
}

#[test]
#[ignore = "needs imported assets"]
fn a_computer_ship_docks_at_border_disputes_centre_island_from_the_south_east() {
    // Issue #192: from here the remake's approach point fell on the island's land and the ship parked for good. The original puts it
    // on the dock axis on the ship's side, unchecked, and lets the collision avoider take the ship there.
    assert!(docks_at_island("border_dispute", "Island_Large01", "Ship_Procyon_Sloop", Vec3::new(141.0, 0.0, -173.0), 300));
}

#[test]
#[ignore = "needs imported assets"]
fn coventry_docks_and_repairs_at_norville_on_the_flight_plane() {
    let (mut app, lib) = app();
    app.init_resource::<crate::collision::Shapes>().add_systems(Update, crate::collision::sail.after(routine));
    let map: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("maps/mission02/map.json")).unwrap()).unwrap();
    let mission: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("maps/mission02/mission.json")).unwrap()).unwrap();
    let kind = "Base_Safetytown";
    let object = map["objects"].as_array().unwrap().iter().find(|o| o["type"] == kind).unwrap();
    let docks = load_docks();
    let tf = crate::scenario::scenery_transform(object, crate::scenario::game_rotation(&object["rot"]), &docks);
    let dk = dockable_for(&docks, kind).unwrap();
    let (point, axis) = dk.world(&tf, 0);
    assert!(point.y.abs() < 0.01);
    let body = app.world().resource::<crate::collision::Shapes>().obstacle(kind, &tf).unwrap();
    let dockee = app.world_mut().spawn((tf, dk, body)).id();
    let class = mission["ships"].as_array().unwrap().iter().find(|s| s["name"] == "SS Coventry").unwrap()["type"].as_str().unwrap();
    let (s, mut h) = ship(&lib, class, Side::Player);
    let full = h.core;
    h.core *= 0.25;
    let start = point + axis * lib.classes[s.class].length * 2.0;
    let me = app.world_mut().spawn((Transform::from_translation(start), s, h, crate::collision::Drift::default())).id();
    app.world_mut().resource_mut::<Messages<DockMsg>>().write(DockMsg::Dock { docker: Some(me), dockee });
    for _ in 0..6000 {
        tick(&mut app, 0.1);
        if app.world().get::<Docker>(me).is_some_and(Docker::is_docked) {
            assert!(app.world().get::<Transform>(me).unwrap().translation.y.abs() < 0.01);
            tick(&mut app, 6.0);
            assert_eq!(app.world().get::<Health>(me).unwrap().core, full);
            return;
        }
    }
    panic!("Coventry could not reach Norville: {:?}", app.world().get::<Transform>(me).unwrap().translation);
}

#[test]
#[ignore = "needs imported assets"]
fn towing_detaches_only_its_formation_follower() {
    let (mut w, ids) = crate::ship::movement_tests::fleet_world();
    crate::ship::set_formation(&mut w, "Player", Some(crate::ship::Formation::Column), true);
    w.entity_mut(ids[1]).insert(Tower { towee: ids[4] });
    crate::ship::fleet_follow(&mut w);
    assert!(!w.get::<crate::ship::FleetPilot>(ids[1]).unwrap().hold);
    assert!(w.get::<crate::ship::FleetPilot>(ids[2]).unwrap().hold);
}

/// A computer ship docks for repair once two sections are down to half or less (a destroyed one does not count), at the
/// nearest friendly dock; the human's ships and healthy ships do not.
#[test]
#[ignore = "needs imported assets"]
fn computer_ship_with_two_half_sections_sails_for_the_nearest_dock() {
    let (mut app, lib) = app();
    app.add_systems(Update, ai_repair_dock.before(handle_messages));
    let docks = load_docks();
    let base = |app: &mut App, x: f32| app.world_mut().spawn((Transform::from_xyz(x, 0.0, 0.0), dockable_for(&docks, "Base_Safetytown").unwrap())).id();
    base(&mut app, 900.0);
    let near = base(&mut app, -300.0);
    let hurt = |app: &mut App, side: Side, halves: &[usize], dead: &[usize]| {
        let (s, mut h) = ship(&lib, "Ship_Pirate_Barque", side);
        for &i in halves {
            h.sections[i] *= 0.5;
        }
        for &i in dead {
            h.sections[i] = 0.0;
        }
        app.world_mut().spawn((Transform::default(), s, h, crate::collision::Drift::default())).id()
    };
    let two = hurt(&mut app, Side::Enemy, &[0, 1], &[]);
    let one = hurt(&mut app, Side::Enemy, &[0], &[1]);
    let ours = hurt(&mut app, Side::Player, &[0, 1], &[]);
    tick(&mut app, 0.1);
    tick(&mut app, 0.1);
    assert_eq!(app.world().get::<Docker>(two).map(|d| d.dockee), Some(near));
    assert!(app.world().get::<Docker>(one).is_none() && app.world().get::<Docker>(ours).is_none());
    assert!(!needs_repair(&lib, &ship(&lib, "Ship_Pirate_Barque", Side::Enemy).0, &ship(&lib, "Ship_Pirate_Barque", Side::Enemy).1));
}

fn app() -> (App, Library) {
    let lib = Library::load(assets_dir());
    let mut app = App::new();
    app.add_message::<DockMsg>()
        .add_message::<Sfx>()
        .insert_resource(Time::<()>::default())
        .init_resource::<ButtonInput<KeyCode>>()
        .insert_resource(Library::load(assets_dir()))
        .add_systems(Update, (handle_messages, routine).chain());
    (app, lib)
}
fn tick(app: &mut App, secs: f32) {
    app.world_mut().resource_mut::<Time>().advance_by(Duration::from_secs_f32(secs));
    app.update();
}
fn ship(lib: &Library, name: &str, side: Side) -> (Ship, Health) {
    let class = lib.index(name);
    let c = &lib.classes[class];
    (
        Ship {
            class,
            side,
            heading: 0.0,
            speed: 3.0,
            ang_vel: 0.0,
            order: 2,
            waypoints: vec![],
            goal: None,
            cap: 1.0,
            throttle: None,
            sail_energy: 1.0,
            crew_speed: 1.0,
            crew_turn: 1.0,
            net: 1.0,
            avoid: 1.0,
        },
        Health::new(c),
    )
}
fn point(x: f32) -> DockPoint {
    DockPoint { pos: Vec3::new(x, 0.0, 0.0), fwd: Vec3::NEG_Z }
}

#[test]
#[ignore = "needs imported assets"]
fn player_docks_repairs_and_leaves_its_slot_free() {
    let (mut app, lib) = app();
    app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .add_systems(Update, crate::course::update.after(routine));
    let line = app.world_mut().resource_mut::<Assets<Mesh>>().add(Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default()));
    let marks = app.world_mut().resource_mut::<Assets<Mesh>>().add(Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default()));
    for mesh in [&line, &marks] {
        app.world_mut().resource_mut::<Assets<Mesh>>().get_mut(mesh).unwrap().insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 0.0, 0.0]; 3]);
    }
    app.world_mut().spawn((crate::course::Course, Mesh3d(line.clone())));
    app.world_mut().spawn((crate::course::Marks::test(), Mesh3d(marks.clone())));
    let (s, full) = ship(&lib, "Ship_Navy_Cutter", Side::Player);
    let mut hurt = Health { sections: full.sections.iter().map(|h| h * 0.2).collect(), core: full.core * 0.2, destroyed: false };
    hurt.sections[0] = 0.0;
    let dockee = app.world_mut().spawn((Transform::from_xyz(100.0, 0.0, 0.0), Dockable::new(vec![point(-20.0)], true))).id();
    let me = app.world_mut().spawn((Transform::from_xyz(60.0, 0.0, 90.0), s, hurt, PlayerShip)).id();
    app.world_mut().resource_mut::<Messages<DockMsg>>().write(DockMsg::Dock { docker: None, dockee });
    tick(&mut app, 0.1);
    tick(&mut app, 0.1);
    let d = app.world().get::<Docker>(me).unwrap();
    assert_eq!((d.phase, d.point), (Phase::Approach, Some(0)), "order accepted, point reserved");
    assert!(d.shows_status(), "approach shows the bottom-bar docking animation");
    // A base dockee: the approach leg ends one docker length out along the point's axis, on the docker's side (+z here).
    let wp = app.world().get::<Ship>(me).unwrap().waypoints.clone();
    assert_eq!(wp.len(), 1, "one approach point, no route");
    for mesh in [&line, &marks] {
        assert_eq!(app.world().resource::<Assets<Mesh>>().get(mesh).unwrap().count_vertices(), 0, "automatic docking paths are hidden");
    }
    assert!((wp[0] - Vec3::new(80.0, 0.0, lib.classes[lib.index("Ship_Navy_Cutter")].length)).length() < 0.01, "{wp:?}");
    // the movement system is not part of this test: put the ship on the dock point
    app.world_mut().get_mut::<Transform>(me).unwrap().translation = Vec3::new(80.0, 0.0, 2.0);
    tick(&mut app, 0.1);
    let docker = app.world().get::<Docker>(me).unwrap();
    assert!(docker.is_docked());
    assert!(docker.shows_status(), "animation remains during repair");
    assert!(docker.ignores_collision(dockee));
    assert!(!docker.ignores_collision(me));
    tick(&mut app, 2.0);
    let mid = app.world().get::<Health>(me).unwrap().core;
    assert_eq!(mid, full.core * 0.2, "no heal before Repair Time: it lands in one step");
    tick(&mut app, 2.5); // past the 4 s Repair Time (Repair_Small)
    assert_eq!(app.world().get::<Health>(me).unwrap().sections, full.sections, "base dock: full refit revives the destroyed section");
    assert!(app.world().get::<Docker>(me).unwrap().is_docked(), "still docked until the 7 s Repair Effect Time");
    tick(&mut app, 3.0);
    assert_eq!(app.world().get::<Docker>(me).unwrap().phase, Phase::Clear, "released, collision stays off while inside the hull");
    app.world_mut().get_mut::<Transform>(me).unwrap().translation = Vec3::new(80.0, 0.0, 200.0);
    tick(&mut app, 0.1);
    assert!(app.world().get::<Docker>(me).is_none(), "human ship is free once clear");
    let h = app.world().get::<Health>(me).unwrap();
    assert_eq!(h.core, full.core);
    assert_eq!(h.sections, full.sections);
    assert!(app.world().get::<Dockable>(dockee).unwrap().nearest_free(&Transform::default(), Vec3::ZERO).is_some(), "slot freed");
    let position = app.world().get::<Transform>(me).unwrap().translation;
    {
        let mut ship = app.world_mut().get_mut::<Ship>(me).unwrap();
        ship.order = 2;
        ship.waypoints = vec![position + Vec3::NEG_Z * 100.0];
    }
    tick(&mut app, 0.1);
    for mesh in [&line, &marks] {
        assert!(app.world().resource::<Assets<Mesh>>().get(mesh).unwrap().count_vertices() > 0, "ordinary routes return after docking");
    }
}

#[test]
#[ignore = "needs imported assets"]
fn closer_ship_takes_over_a_point_and_ai_ship_returns_home() {
    let (mut app, lib) = app();
    let tender = ship(&lib, "Ship_Navy_Tender", Side::Player);
    let dockee = app.world_mut().spawn((Transform::default(), Dockable::new(vec![point(10.0)], true), tender.0, tender.1)).id();
    let (a, ha) = ship(&lib, "Ship_Navy_Cutter", Side::Player);
    let (b, hb) = ship(&lib, "Ship_Navy_Cutter", Side::Player);
    let far = app.world_mut().spawn((Transform::from_xyz(10.0, 0.0, 300.0), a, ha)).id();
    let near = app.world_mut().spawn((Transform::from_xyz(10.0, 0.0, 120.0), b, hb)).id();
    app.world_mut().resource_mut::<Messages<DockMsg>>().write(DockMsg::Dock { docker: Some(far), dockee });
    tick(&mut app, 0.1);
    tick(&mut app, 0.1);
    assert_eq!(app.world().get::<Docker>(far).unwrap().point, Some(0));
    app.world_mut().resource_mut::<Messages<DockMsg>>().write(DockMsg::Dock { docker: Some(near), dockee });
    tick(&mut app, 0.1);
    tick(&mut app, 0.1);
    tick(&mut app, 0.1);
    assert_eq!(app.world().get::<Docker>(near).unwrap().point, Some(0), "the nearer ship takes the point over");
    assert_eq!(app.world().get::<Docker>(far).unwrap().phase, Phase::Wait, "the other one waits");
    // Dock `near`, then let its time run out: an AI ship glides away, turns in place and sails one 100-unit leg toward where it came from.
    app.world_mut().get_mut::<Transform>(near).unwrap().translation = Vec3::new(10.0, 0.0, 1.0);
    tick(&mut app, 0.1);
    assert_eq!(app.world().get::<Docker>(near).unwrap().phase, Phase::Docked);
    tick(&mut app, 10.5);
    assert_eq!(app.world().get::<Docker>(near).unwrap().phase, Phase::Away);
    tick(&mut app, 10.5);
    assert_eq!(app.world().get::<Docker>(near).unwrap().phase, Phase::Turn);
    assert_eq!(app.world().get::<Docker>(far).unwrap().phase, Phase::Approach, "the freed point goes to the waiting ship");
    tick(&mut app, 10.5);
    tick(&mut app, 0.1);
    assert_eq!(app.world().get::<Docker>(near).unwrap().phase, Phase::Return);
    let here = app.world().get::<Transform>(near).unwrap().translation;
    let leg = app.world().get::<Ship>(near).unwrap().waypoints.clone();
    assert_eq!(leg.len(), 1);
    assert!(((leg[0] - here).length() - 100.0).abs() < 0.01, "{leg:?} from {here}");
    assert!((leg[0] - here).normalize().dot((Vec3::new(10.0, 0.0, 120.0) - here).normalize()) > 0.99, "toward the start");
}

#[test]
#[ignore = "needs imported assets"]
fn disabled_dockee_refuses_and_exported_points_load() {
    let (mut app, lib) = app();
    let (s, h) = ship(&lib, "Ship_Navy_Cutter", Side::Player);
    let dockee = app.world_mut().spawn((Transform::default(), Dockable::new(vec![point(0.0)], false))).id();
    let me = app.world_mut().spawn((Transform::from_xyz(0.0, 0.0, 30.0), s, h, PlayerShip)).id();
    app.world_mut().resource_mut::<Messages<DockMsg>>().write(DockMsg::Dock { docker: None, dockee });
    tick(&mut app, 0.1);
    assert!(app.world().get::<Docker>(me).is_none());
    let docks = load_docks();
    let tender = dockable_for(&docks, "Ship_Navy_Tender").expect("tender dock points exported");
    assert_eq!(tender.points.len(), 2);
    assert!(tender.points[0].pos.x < 0.0 && tender.points[1].pos.x > 0.0, "port and starboard");
    assert!(tender.enabled && tender.repairs, "tenders can be docked with");
    assert!(!dockable_for(&docks, "Ship_Navy_Frigate").unwrap().enabled, "other ships only after 'Set Group/Unit Dockable'");
    assert!(dockable_for(&docks, "Base_Grant").unwrap().enabled, "bases always");
    assert_eq!(repair_secs(lib.classes[lib.index("Ship_Navy_Tender")].repair_fx.as_deref()), (10.0, 15.0));
}

#[test]
fn tow_registers_inside_fifty_units_of_surface_distance() {
    // Centres 120 apart with 30 + 30 of hull: 60 units of surface gap, out of range; 90 apart: 30, in range.
    assert!(!tow_in_range(Vec3::ZERO, Vec3::new(120.0, 0.0, 0.0), 30.0, 30.0));
    assert!(tow_in_range(Vec3::ZERO, Vec3::new(90.0, 0.0, 0.0), 30.0, 30.0));
    assert!(tow_in_range(Vec3::new(0.0, 500.0, 0.0), Vec3::new(90.0, -500.0, 0.0), 30.0, 30.0), "height is ignored");
}

#[test]
fn approach_point_is_on_the_dockers_side_of_the_axis() {
    let p = Vec3::new(10.0, 0.0, 0.0);
    assert_eq!(approach_point(p, Vec3::NEG_Z, Vec3::new(50.0, 0.0, -80.0), 30.0), Vec3::new(10.0, 0.0, -30.0));
    assert_eq!(approach_point(p, Vec3::NEG_Z, Vec3::new(50.0, 0.0, 80.0), 30.0), Vec3::new(10.0, 0.0, 30.0));
}

#[test]
fn cranes_run_while_a_ship_is_docked_and_stop_on_their_end_pose() {
    let mut app = App::new();
    let clips = std::collections::HashMap::from([("BaseX".to_string(), vec![("CRANES01".to_string(), crate::clips::Clip::slide("Crane", 10.0, 5.0))])]);
    app.insert_resource(Library { classes: vec![], guns: default(), bullets: default(), names: default(), clips }).init_resource::<Time>().add_systems(Update, play_cranes);
    let dock = |phase| Docker {
        dockee: Entity::PLACEHOLDER,
        point: Some(0),
        phase,
        final_leg: false,
        timer: 0.0,
        duration: 0.0,
        repair_secs: 0.0,
        healed: false,
        full: false,
        start: Vec3::ZERO,
        offset: Vec3::ZERO,
    };
    let ship = app.world_mut().spawn(dock(Phase::Docked)).id();
    let mut dockable = Dockable::new(vec![point(0.0), point(1.0)], true);
    dockable.numbers = vec![1, 2];
    dockable.taken[1] = Some(ship);
    let crane = app.world_mut().spawn((Name::new("Crane"), Transform::default())).id();
    let base = app.world_mut().spawn((crate::clips::SceneLoop("BaseX".into()), dockable)).add_child(crane).id();
    let x = |app: &App| app.world().get::<Transform>(crane).unwrap().translation.x;
    let step = |app: &mut App, secs: f32| {
        app.world_mut().resource_mut::<Time>().advance_by(Duration::from_secs_f32(secs));
        app.update();
    };
    for _ in 0..3 {
        step(&mut app, 1.0);
    }
    assert_eq!(x(&app), 0.0, "point 2 is docked, so crane set 1 stays at rest");
    app.world_mut().get_mut::<Dockable>(base).unwrap().taken = vec![Some(ship), None];
    for _ in 0..8 {
        step(&mut app, 1.0);
    }
    assert!(x(&app) > 0.0 && x(&app) < 5.0, "running: {}", x(&app));
    app.world_mut().get_mut::<Dockable>(base).unwrap().taken = vec![None, None];
    step(&mut app, 1.0);
    step(&mut app, 1.0);
    assert!((x(&app) - 5.0).abs() < 1e-4, "stopped on the end pose: {}", x(&app));
}
