use super::*;

#[test]
fn rule_keys_latch_until_a_tick_and_opening_skip_is_not_queued_twice() {
    let data = serde_json::json!({"rules": []});
    let mission = Mission { id: "synthetic".into(), script: Script::new(&data, serde_json::json!({})), data, opening: None };
    let mut app = App::new();
    let binds = crate::bindings::Bindings::default();
    let skip = binds.key(crate::bindings::Act::SkipSpeech);
    app.insert_resource(mission).insert_resource(binds).init_resource::<ButtonInput<KeyCode>>().init_resource::<RuleInput>().add_systems(Update, capture_input);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(skip);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyD);
    app.update();
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
    app.update();
    let pending = std::mem::take(&mut *app.world_mut().resource_mut::<RuleInput>());
    assert!(pending.skip_speech && pending.dock, "commands survive frames without a simulation tick");
    app.update();
    let pending = app.world().resource::<RuleInput>();
    assert!(!pending.skip_speech && !pending.dock, "consumed commands are not repeated");
    app.world_mut().resource_mut::<Mission>().opening = Some(default());
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset(skip);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(skip);
    app.update();
    assert!(!app.world().resource::<RuleInput>().skip_speech, "the opening briefing owns this skip");
}

/// The rules `update` system on its own, with the resources and messages it reads; `mission` is the script under test.
fn rules_app(mission: Mission) -> App {
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()))
        .init_asset::<Image>()
        .init_asset::<bevy::audio::AudioSource>()
        .init_resource::<Time>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<RuleInput>()
        .init_resource::<Target>()
        .init_resource::<crate::bindings::Bindings>()
        .init_resource::<crate::campaign::CampaignPoints>()
        .init_resource::<crate::nebula::StormShift>()
        .init_resource::<crate::environment::Sensors>()
        .init_resource::<crate::ship::Fleets>()
        .add_systems(PreUpdate, crate::environment::refresh_sensors)
        .add_message::<Hit>()
        .add_message::<Fired>()
        .add_message::<Destroyed>()
        .add_message::<DockMsg>()
        .add_message::<crate::boarding::BoardMsg>()
        .add_message::<crate::boarding::Captured>()
        .add_message::<crate::combat::AsteroidSetup>()
        .add_message::<crate::nebula::MeteorRoute>()
        .add_message::<crate::combat::Detonate>()
        .insert_resource(Orbit { yaw: 0.0, pitch: 0.4, dist: 90.0, focus: None })
        .insert_resource(Config {
            groups: vec![],
            player: "Ship_Navy_Torpedo".into(),
            allies: vec![],
            enemies: vec![],
            map: Some(mission.id.clone()),
            shot: None,
            frame: 0,
            speed: 1.0,
            ended: None,
            script: None,
            autolaunch: false,
            shot_frame: 0,
            shot_frames: vec![],
            late_frame: 0,
        })
        .insert_resource(mission)
        .add_systems(Update, update);
    app
}

/// Mission 9's "Dark Matter Ignited" hands the Longboat back to the human, then sets the group's throttle to 0 in the same rule.
/// The primary handoff is queued, so the Longboat is not the primary yet when the throttle applies: it must only stop, not keep
/// a cap of 0 (it could never move again, and "Return To Ship" could not be done).
#[test]
#[ignore = "needs imported assets"]
fn a_ship_handed_to_the_human_is_stopped_not_capped_by_a_group_throttle() {
    let mut mission = crate::campaign::load_id("mission02");
    let data = serde_json::json!({"rules": [{"name": "Hand back", "conditions": [{"Type": "World Initialize"}], "actions": [
        {"Type": "Set Group/Unit AI Captain", "Group/Unit Name": "Player0 Group,Longboat", "AI Stance": "Captain_Human"},
        {"Type": "Set Group Throttle Percent", "Group Name": "Player0 Group", "Throttle Percent": 0.0}]}]});
    mission.script = Script::new(&data, serde_json::json!({}));
    let owner = mission.script.player().to_string();
    let lib = Library::load(crate::assets_dir());
    let class = lib.index("Ship_Navy_Torpedo");
    let mut app = rules_app(mission);
    let ship = |order| Ship { heading: 0.0, order, ..Ship::new(class, Side::Player) };
    let ms = |name: &str| MissionShip { name: name.into(), owner: owner.clone(), visible: true, ..default() };
    app.world_mut().spawn((ship(0), Health::new(&lib.classes[class]), Transform::default(), Visibility::Inherited, PlayerShip, ms("RLS Comet")));
    let boat = app.world_mut().spawn((ship(2), Health::new(&lib.classes[class]), Transform::from_xyz(50.0, 0.0, 0.0), Visibility::Inherited, ms("Longboat"))).id();
    app.insert_resource(lib);
    app.update();
    let (sh, m) = (app.world().get::<Ship>(boat).unwrap(), app.world().get::<MissionShip>(boat).unwrap());
    assert!(m.human, "Captain_Human applied");
    assert_eq!((sh.cap, sh.order), (1.0, 0), "stopped, but free to sail on the human's orders");
}

#[test]
#[ignore = "needs imported assets"]
fn mission_two_attack_volume_changes_base_owner_and_gunfire_advances_the_story() {
    let mut mission = crate::campaign::load_id("mission02");
    let mut data = mission.data.clone();
    data["rules"].as_array_mut().unwrap().retain(|r| matches!(r["name"].as_str(), Some("SmugglerCampFound2" | "SmugglersCampFound2c")));
    mission.script = Script::new(&data, serde_json::json!({}));
    let player_owner = mission.script.player().to_string();
    let lib = Library::load(crate::assets_dir());
    let class = lib.index("Ship_Navy_Torpedo");
    let mut app = rules_app(mission);
    let ship = Ship { heading: 0.0, ..Ship::new(class, Side::Player) };
    let player = app
        .world_mut()
        .spawn((
            ship,
            Health::new(&lib.classes[class]),
            Transform::from_xyz(-970.0, 0.0, 450.0),
            Visibility::Inherited,
            PlayerShip,
            MissionShip { id: 249, name: "RLS Comet".into(), owner: player_owner.clone(), visible: true, ..default() },
        ))
        .id();
    let base = app.world_mut().spawn((MissionBase { id: 24, owner: "NO PLAYER".into() }, Transform::default())).id();
    app.insert_resource(lib);
    app.update();
    let owner = &app.world().get::<MissionBase>(base).unwrap().owner;
    assert_eq!(owner, "SmugglerCampEnemy");
    assert!(app.world().resource::<Mission>().script.hostile(owner, &player_owner));
    app.world_mut().resource_mut::<Messages<Fired>>().write(Fired { shooter: base, target: Some(player), bullet: "Bullet_CannonBall_Light".into() });
    app.update();
    let script = &app.world().resource::<Mission>().script;
    assert!(script.timers.contains_key("OnyxFirePause"), "the first tower shot unlocks the next dialogue");
    assert!(script.timers.contains_key("SmugglerCampSwitchTwo"));
}

#[test]
#[ignore = "needs imported assets"]
fn mission_ships_halt_when_their_script_route_runs_out() {
    let lib = Library::load(crate::assets_dir());
    let mut app = App::new();
    app.insert_resource(crate::campaign::load())
        .init_resource::<Time>()
        .init_resource::<crate::environment::Sensors>()
        .add_systems(PreUpdate, crate::environment::refresh_sensors)
        .add_systems(Update, mission_ai);
    let ship = Ship { heading: 0.0, speed: 5.0, order: 2, ..Ship::new(0, Side::Player) };
    let n = lib.classes[0].sections.len();
    let health = || Health { sections: vec![1.0; n], core: 1.0, destroyed: false };
    app.insert_resource(lib);
    let ms = |stop, looping: Option<Vec<Vec3>>| MissionShip {
        id: -1,
        name: "Test".into(),
        owner: "Navy Tenders".into(),
        hold_fire: true,
        invuln: 0.0,
        visible: true,
        stance: Stance::Dummy,
        target: None,
        attack: vec![],
        flag: String::new(),
        banner: String::new(),
        looping,
        stop_at_end: stop,
        create_lifeboats: false,
        ..Default::default()
    };
    let done = app.world_mut().spawn((ship.clone(), health(), Transform::default(), ms(true, None))).id();
    let en_route = app.world_mut().spawn((Ship { waypoints: vec![Vec3::new(0.0, 0.0, -500.0)], ..ship }, health(), Transform::default(), ms(true, None))).id();
    let turning = app.world_mut().spawn((Ship { waypoints: vec![Vec3::new(0.0, 0.0, 500.0)], ..ship }, health(), Transform::default(), ms(true, None))).id();
    let looping = app.world_mut().spawn((ship.clone(), health(), Transform::default(), ms(true, Some(vec![Vec3::new(0.0, 0.0, -500.0)])))).id();
    let near = app.world_mut().spawn((Ship { waypoints: vec![Vec3::new(0.0, 0.0, -1.0)], ..ship }, health(), Transform::default(), ms(true, None))).id();
    let foe = app
        .world_mut()
        .spawn((
            ship.clone(),
            health(),
            Transform::from_xyz(1000.0, 0.0, 0.0),
            MissionShip { name: "Foe".into(), owner: "Target".into(), visible: true, stance: Stance::Dummy, ..default() },
        ))
        .id();
    let route = vec![Vec3::NEG_Z * 500.0];
    let persistent = app
        .world_mut()
        .spawn((
            Ship { waypoints: route.clone(), ..ship },
            health(),
            Transform::from_xyz(300.0, 0.0, 0.0),
            MissionShip { attack: vec!["Target".into()], stance: Stance::Persistent, ..ms(true, None) },
        ))
        .id();
    app.update();
    assert_eq!(app.world().get::<MissionShip>(persistent).unwrap().target, Some(foe));
    assert_eq!(app.world().get::<Ship>(persistent).unwrap().waypoints, route, "persistent attack order must not cancel the scripted swoop");
    let order = |e| app.world().get::<Ship>(e).unwrap().order;
    assert_eq!(order(done), 0, "finished route stops");
    assert!(!app.world().get::<MissionShip>(done).unwrap().stop_at_end);
    assert_eq!(order(en_route), 2, "still on its path");
    assert_eq!(app.world().get::<Ship>(en_route).unwrap().throttle, Some(1.0), "aligned with its point: full throttle");
    let held = app.world().get::<Ship>(turning).unwrap().throttle.unwrap();
    assert!(held > 0.0 && held < 1.0, "turned off its point: the speed fraction is held, not full ({held})");
    assert_eq!(order(near), 0, "last point within 2 x radius counts as reached");
    assert!(app.world().get::<Ship>(near).unwrap().waypoints.is_empty());
    assert_eq!(order(looping), 2, "loops restart instead");
    app.world_mut().get_mut::<MissionShip>(persistent).unwrap().stance = Stance::Aggressive;
    app.update();
    assert!(app.world().get::<Ship>(persistent).unwrap().waypoints.is_empty(), "aggressive stance switches to combat steering");

    use bevy::ecs::system::RunSystemOnce;
    app.world_mut().entity_mut(persistent).insert(Visibility::Inherited);
    let path = vec![Vec3::ZERO, Vec3::X * 290.0, Vec3::X * 500.0];
    let expected = path.clone();
    app.world_mut()
        .run_system_once(move |mut ships: Query<RuleShip>, mut animals: Query<(Entity, &mut crate::environment::SpaceAnimal, &mut Transform, &mut Visibility), Without<Ship>>| {
            follow_path(&mut ships, &mut animals, &[persistent], &path, true, true);
        })
        .unwrap();
    let ship = app.world().get::<Ship>(persistent).unwrap();
    let mission = app.world().get::<MissionShip>(persistent).unwrap();
    assert_eq!(ship.waypoints, expected[1..]);
    assert_eq!(mission.looping.as_ref(), Some(&expected));
    assert!(!mission.stop_at_end);
    assert!(mission.attack.is_empty());
}

#[test]
fn a_teleported_group_keeps_its_layout_round_the_point_and_a_single_ship_lands_on_it() {
    let point = Vec3::new(800.0, 5.0, -800.0);
    let at = teleport::layout(&[Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 40.0)], point);
    assert_eq!(at, [point + Vec3::new(0.0, 0.0, -20.0), point + Vec3::new(0.0, 0.0, 20.0)]);
    assert_eq!(teleport::layout(&[Vec3::new(3.0, 9.0, 4.0)], point), [point], "one ship: on the point, at the point's height");
    assert!(teleport::layout(&[], point).is_empty());
}

#[test]
fn a_teleported_ship_is_at_rest_and_only_the_humans_forgets_its_orders() {
    let moving = || Ship {
        speed: 9.0,
        ang_vel: 0.3,
        order: 3,
        throttle: Some(0.8),
        waypoints: vec![Vec3::X],
        goal: Some(1.0),
        helm: crate::ship::Helm { steer: 0.7, boost: 2.0, age: 0.0 },
        ..Ship::new(0, Side::Player)
    };
    let mut tug = moving();
    teleport::stop(&mut tug, false);
    assert_eq!((tug.speed, tug.ang_vel, tug.helm.steer), (0.0, 0.0, 0.0));
    assert_eq!((tug.order, tug.throttle), (3, Some(0.8)), "a computer ship keeps its order: told to follow a path it still goes");
    let mut own = moving();
    teleport::stop(&mut own, true);
    assert_eq!((own.speed, own.ang_vel, own.order, own.throttle, own.goal), (0.0, 0.0, 0, None, None), "the player's ship stops for good");
    assert!(own.waypoints.is_empty());
}

#[test]
#[ignore = "needs imported assets"]
fn teleporting_ships_to_one_point_keeps_them_apart_and_at_rest() {
    let lib = Library::load(crate::assets_dir());
    let class = lib.index("Ship_Navy_Torpedo");
    let mut mission = crate::campaign::load_id("mission02");
    let data = serde_json::json!({
        "environment": {"point_sets": {"Spot": [{"pos": [100.0, 100.0, 0.0]}]}},
        "rules": [{"name": "Cut", "conditions": [{"Type": "World Initialize"}], "actions": [
            {"Type": "Teleport Group/Unit", "Group/Unit Name": "Player0 Group,Own", "Point Set Name": "Spot"},
            {"Type": "Teleport Group/Unit", "Group/Unit Name": "Player0 Group,Other", "Point Set Name": "Spot"}]}]});
    mission.script = Script::new(&data, serde_json::json!({}));
    let owner = mission.script.player().to_string();
    let mut app = rules_app(mission);
    let moving = Ship { heading: 0.0, speed: 9.0, ang_vel: 0.2, order: 3, ..Ship::new(class, Side::Player) };
    let ms = |name: &str| MissionShip { name: name.into(), owner: owner.clone(), visible: true, ..default() };
    let own = app.world_mut().spawn((moving.clone(), Health::new(&lib.classes[class]), Transform::from_xyz(-500.0, 0.0, 0.0), Visibility::Inherited, PlayerShip, ms("Own"))).id();
    let other = app.world_mut().spawn((moving, Health::new(&lib.classes[class]), Transform::from_xyz(500.0, 0.0, 0.0), Visibility::Inherited, ms("Other"))).id();
    let bounds = lib.classes[class].collision_bounds;
    app.insert_resource(lib);
    app.update();
    let world = app.world();
    let (a, b) = (world.get::<Transform>(own).unwrap(), world.get::<Transform>(other).unwrap());
    assert!(crate::collision::tree::overlap(&crate::collision::spot::hull(bounds, a), &crate::collision::spot::hull(bounds, b)).is_none(), "{a:?} {b:?}");
    assert!(a.translation.distance(b.translation) < 40.0, "side by side, not far apart");
    let (sa, sb) = (world.get::<Ship>(own).unwrap(), world.get::<Ship>(other).unwrap());
    assert_eq!((sa.speed, sa.ang_vel, sa.order), (0.0, 0.0, 0), "the player's ship is at all stop");
    assert_eq!((sb.speed, sb.ang_vel, sb.order), (0.0, 0.0, 3), "a computer ship keeps its order");
}
