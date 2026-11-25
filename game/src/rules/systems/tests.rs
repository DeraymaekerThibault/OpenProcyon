use super::*;
use crate::EngineOrder;

#[test]
fn rule_keys_latch_until_a_tick_and_opening_skip_is_not_queued_twice() {
    let data = serde_json::json!({"rules": []});
    let mission = Mission { id: "synthetic".into(), script: Script::new(&data, serde_json::json!({})), data, opening: None };
    let mut app = App::new();
    let binds = crate::bindings::Bindings::default();
    let skip = binds.key(crate::bindings::Act::SkipSpeech);
    app.insert_resource(mission).insert_resource(binds).init_resource::<ButtonInput<KeyCode>>().init_resource::<RuleInput>().add_systems(Update, capture_input);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(skip);
    app.update();
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
    app.update();
    let pending = std::mem::take(&mut *app.world_mut().resource_mut::<RuleInput>());
    assert!(pending.skip_speech, "commands survive frames without a simulation tick");
    app.update();
    let pending = app.world().resource::<RuleInput>();
    assert!(!pending.skip_speech, "consumed commands are not repeated");
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
        .init_resource::<RuleWorld>()
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
    let ms = |name: &str| MissionShip { name: name.into(), owner: owner.as_str().into(), visible: true, ..default() };
    app.world_mut().spawn((ship(EngineOrder::Stop), Health::new(&lib.classes[class]), Transform::default(), Visibility::Inherited, PlayerShip, ms("RLS Comet")));
    let boat = app.world_mut().spawn((ship(EngineOrder::Full), Health::new(&lib.classes[class]), Transform::from_xyz(50.0, 0.0, 0.0), Visibility::Inherited, ms("Longboat"))).id();
    app.insert_resource(lib);
    app.update();
    let (sh, m) = (app.world().get::<Ship>(boat).unwrap(), app.world().get::<MissionShip>(boat).unwrap());
    assert!(m.human, "Captain_Human applied");
    assert_eq!((sh.cap, sh.order), (1.0, EngineOrder::Stop), "stopped, but free to sail on the human's orders");
}

/// Mission 4 stops 'Player0 Group' at every cutscene ("Nelson Speech", the sightings) and no rule sets it back to full. The human's
/// escort (the RLS Beans) must be stopped like the flagship, not capped at 0: a cap left it unable to move for the whole mission.
/// An allied computer ship named in the same group keeps the cap.
#[test]
#[ignore = "needs imported assets"]
fn the_humans_escort_is_stopped_not_capped_by_a_group_throttle() {
    let mut mission = crate::campaign::load_id("mission02");
    let data = serde_json::json!({"rules": [{"name": "Cutscene", "conditions": [{"Type": "World Initialize"}], "actions": [
        {"Type": "Set Group Throttle Percent", "Group Name": "Player0 Group", "Throttle Percent": 0.0},
        {"Type": "Set Group Throttle Percent", "Group Name": "navy", "Throttle Percent": 0.0}]}]});
    mission.script = Script::new(&data, serde_json::json!({}));
    let owner = mission.script.player().to_string();
    let lib = Library::load(crate::assets_dir());
    let class = lib.index("Ship_Navy_Torpedo");
    let mut app = rules_app(mission);
    let ship = |order| Ship { heading: 0.0, order, ..Ship::new(class, Side::Player) };
    let ms = |name: &str, owner: &str| MissionShip { name: name.into(), owner: owner.into(), visible: true, ..default() };
    app.world_mut().spawn((ship(EngineOrder::Full), Health::new(&lib.classes[class]), Transform::default(), Visibility::Inherited, PlayerShip, ms("RLS Frank", &owner)));
    let escort = app
        .world_mut()
        .spawn((ship(EngineOrder::Full), Health::new(&lib.classes[class]), Transform::from_xyz(20.0, 0.0, 0.0), Visibility::Inherited, ms("RLS Beans", &owner)))
        .id();
    let ally = app
        .world_mut()
        .spawn((ship(EngineOrder::Full), Health::new(&lib.classes[class]), Transform::from_xyz(90.0, 0.0, 0.0), Visibility::Inherited, ms("RLS Sleekshot", "navy")))
        .id();
    app.insert_resource(lib);
    app.update();
    let escort = app.world().get::<Ship>(escort).unwrap();
    assert_eq!((escort.cap, escort.order), (1.0, EngineOrder::Stop), "stopped, but free to sail on the human's orders");
    assert_eq!(app.world().get::<Ship>(ally).unwrap().cap, 0.0, "a computer ship's group throttle is its cap");
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
            MissionShip { id: 249, name: "RLS Comet".into(), owner: player_owner.as_str().into(), visible: true, ..default() },
        ))
        .id();
    let base = app.world_mut().spawn((MissionBase { id: 24, owner: "NO PLAYER".into() }, Transform::default())).id();
    app.insert_resource(lib);
    app.update();
    let owner = &app.world().get::<MissionBase>(base).unwrap().owner;
    assert_eq!(owner, "SmugglerCampEnemy");
    assert!(app.world().resource::<Mission>().script.hostile(owner, &player_owner));
    app.world_mut().resource_mut::<Messages<Fired>>().write(Fired { shooter: base, target: Some(player), bullet: "Bullet_CannonBall_Light".into(), torpedo: false, mine: false });
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
    let ship = Ship { heading: 0.0, speed: 5.0, order: EngineOrder::Full, ..Ship::new(0, Side::Player) };
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
    assert_eq!(order(done), EngineOrder::Stop, "finished route stops");
    assert!(!app.world().get::<MissionShip>(done).unwrap().stop_at_end);
    assert_eq!(order(en_route), EngineOrder::Full, "still on its path");
    assert_eq!(app.world().get::<Ship>(en_route).unwrap().throttle, Some(1.0), "aligned with its point: full throttle");
    let held = app.world().get::<Ship>(turning).unwrap().throttle.unwrap();
    assert!(held > 0.0 && held < 1.0, "turned off its point: the speed fraction is held, not full ({held})");
    assert_eq!(order(near), EngineOrder::Stop, "last point within 2 x radius counts as reached");
    assert!(app.world().get::<Ship>(near).unwrap().waypoints.is_empty());
    assert_eq!(order(looping), EngineOrder::Full, "loops restart instead");
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

/// The mission AI on its own, with one synthetic class and a script that has no rules: what `OP Follow Group` and an abandoned hull make
/// the captains do.
fn ai_world() -> App {
    let data = serde_json::json!({"rules": []});
    let mission = Mission { id: "synthetic".into(), script: Script::new(&data, serde_json::json!({})), data, opening: None };
    let mut app = App::new();
    app.insert_resource(mission)
        .insert_resource(Library { classes: vec![crate::fixed_tests::synthetic_class()], guns: default(), bullets: default(), names: default(), clips: default() })
        .init_resource::<Time>()
        .init_resource::<crate::environment::Sensors>()
        .add_systems(PreUpdate, crate::environment::refresh_sensors)
        .add_systems(Update, mission_ai);
    app
}

fn ai_ship(app: &mut App, name: &str, owner: &str, at: Vec3, follow: Option<FollowOrder>, speed: f32) -> Entity {
    let ship = Ship { heading: 0.0, speed, order: EngineOrder::Full, ..Ship::new(0, Side::Player) };
    let ms = MissionShip { name: name.into(), owner: owner.into(), visible: true, follow, ..default() };
    app.world_mut().spawn((ship, Health { sections: vec![], core: 100.0, destroyed: false }, Transform::from_translation(at), ms)).id()
}

#[test]
fn a_follower_steers_for_its_station_in_the_leaders_frame_and_keeps_its_leader() {
    for (station, slot) in [
        (Station::Astern, Vec3::new(0.0, 0.0, 40.0)),
        (Station::Ahead, Vec3::new(0.0, 0.0, -40.0)),
        (Station::AbeamStarboard, Vec3::new(40.0, 0.0, 0.0)),
        (Station::AbeamPort, Vec3::new(-40.0, 0.0, 0.0)),
    ] {
        let mut app = ai_world();
        // The leader at the origin heading "north" (-Z at heading 0); the escort far off, clear of the line to every station.
        let leader = ai_ship(&mut app, "Leader", "Convoy", Vec3::ZERO, None, 5.0);
        let other = ai_ship(&mut app, "Other", "Convoy", Vec3::new(500.0, 0.0, 0.0), None, 5.0);
        let order = FollowOrder { group: "Convoy".into(), station, distance: 40.0, leader: None };
        let escort = ai_ship(&mut app, "Escort", "Escort", Vec3::new(30.0, 0.0, 400.0), Some(order), 0.0);
        app.update();
        let (ship, ms) = (app.world().get::<Ship>(escort).unwrap(), app.world().get::<MissionShip>(escort).unwrap());
        assert_eq!(ms.follow.as_ref().and_then(|f| f.leader), Some(leader), "the nearest ship of the group, not {other:?}");
        assert!(ship.waypoints.first().is_some_and(|w| w.distance(slot) < 1e-3), "{station:?}: {:?}", ship.waypoints);
        assert_eq!(ship.order, EngineOrder::Full, "far from the station: full speed");
        assert!(!ms.stop_at_end);
        // Leader gone: the escort stands still and keeps its order.
        app.world_mut().get_mut::<Health>(leader).unwrap().destroyed = true;
        app.world_mut().get_mut::<Health>(other).unwrap().destroyed = true;
        app.update();
        let ship = app.world().get::<Ship>(escort).unwrap();
        assert_eq!((ship.order, ship.waypoints.len()), (EngineOrder::Stop, 0));
        assert!(app.world().get::<MissionShip>(escort).unwrap().follow.is_some());
    }
}

#[test]
fn a_follower_in_its_station_matches_the_leaders_speed() {
    let mut app = ai_world();
    ai_ship(&mut app, "Leader", "Convoy", Vec3::ZERO, None, 3.0);
    let order = FollowOrder { group: "Convoy".into(), station: Station::Astern, distance: 40.0, leader: None };
    let escort = ai_ship(&mut app, "Escort", "Escort", Vec3::new(0.0, 0.0, 40.5), Some(order), 3.0);
    app.update();
    let ship = app.world().get::<Ship>(escort).unwrap();
    assert!(ship.waypoints.is_empty(), "within 3 units of the slot there is nothing to steer for");
    let top = crate::ship::max_speed(ship, &app.world().resource::<Library>().classes[0], app.world().get::<Health>(escort).unwrap());
    assert!((ship.throttle.unwrap() * top - 3.0).abs() < 1e-3, "the leader's speed as a share of its own top speed: {:?}", ship.throttle);
}

#[test]
fn an_abandoned_hull_takes_no_orders_and_is_held_at_zero_speed_whatever_a_rule_sets() {
    let mut app = ai_world();
    let hull = ai_ship(&mut app, "Hulk", "Navy", Vec3::ZERO, None, 4.0);
    app.world_mut().get_mut::<Ship>(hull).unwrap().waypoints = vec![Vec3::new(0.0, 0.0, -500.0)];
    app.world_mut().entity_mut(hull).insert(crate::lifeboats::Abandoned);
    app.update();
    let (ship, ms) = (app.world().get::<Ship>(hull).unwrap(), app.world().get::<MissionShip>(hull).unwrap());
    assert_eq!((ship.cap, ship.order, ship.waypoints.len()), (0.0, EngineOrder::Stop, 0));
    assert!(ms.hold_fire && ms.target.is_none());
    app.world_mut().get_mut::<Ship>(hull).unwrap().cap = 1.0; // a later `Set Group Throttle Percent`
    app.update();
    assert_eq!(app.world().get::<Ship>(hull).unwrap().cap, 0.0, "held again");
}

/// The rules `update` system with the queries a custom mission adds (placed mines are rule units, a ship's cloak and abandoned state):
/// it starts without a query conflict and a mine is addressed by its world-object id like any unit. Asset-free.
#[test]
fn a_custom_missions_mines_are_units_the_rules_can_count_and_hand_over() {
    let data = serde_json::json!({
        "players": ["Player0 Group", "Pirates"],
        "groups": {"Field": [5, 6]},
        "rules": [
            {"name": "count", "Run Once": true, "conditions": [{"Type": "Group has X members", "Group Name": "Field", "Equivalence": "Equal To", "number": 2}],
             "actions": [{"Type": "Set Flag Action", "Flag Name": "two mines", "Boolean Value": "TRUE"}, {"Type": "Set Group/Unit Owner", "Group/Unit Name": "Field", "New Owner": "Pirates"}]},
        ],
    });
    let mission = Mission { id: "synthetic".into(), script: Script::new(&data, serde_json::json!({})), data, opening: None };
    let mut app = rules_app(mission);
    let class = crate::fixed_tests::synthetic_class();
    app.insert_resource(Library { classes: vec![class], guns: default(), bullets: default(), names: default(), clips: default() });
    app.world_mut().spawn((
        Ship::new(0, Side::Player),
        Health { sections: vec![], core: 100.0, destroyed: false },
        Transform::default(),
        Visibility::Inherited,
        PlayerShip,
        MissionShip { id: 1, name: "RLS Hawk".into(), owner: "Player0 Group".into(), visible: true, ..default() },
    ));
    let mines: Vec<Entity> = [5, 6]
        .map(|id| app.world_mut().spawn((crate::combat::MapMine { id, bullet: "Bullet_StarMine_Med".into(), owner: String::new(), side: None }, Transform::default())).id())
        .into();
    app.update();
    app.update();
    assert!(app.world().resource::<Mission>().script.flag("two mines"), "both mines are in the group the rules see");
    for e in mines {
        let mine = app.world().get::<crate::combat::MapMine>(e).unwrap();
        assert_eq!(mine.owner, "Pirates", "Set Group/Unit Owner hands a mission's mine to a player");
        assert_eq!(mine.side, Some(Side::Enemy), "and it fights for the side that player has in the mission: hostile to the human's");
    }
}

// ---- The appliers of the engine-added orders, through the real rules `update` system (asset-free: one synthetic class).

fn order_app(data: serde_json::Value) -> App {
    let mission = Mission { id: "one".into(), script: Script::new(&data, serde_json::json!({})), data, opening: None };
    let mut app = rules_app(mission);
    app.insert_resource(Library { classes: vec![crate::fixed_tests::synthetic_class()], guns: default(), bullets: default(), names: default(), clips: default() });
    app.init_resource::<crate::campaign::CampaignValues>();
    app
}
fn order_ship(app: &mut App, id: i64, owner: &str, at: Vec3) -> Entity {
    let ship = Ship { heading: 0.0, speed: 4.0, ang_vel: 0.3, order: EngineOrder::Full, waypoints: vec![Vec3::NEG_Z * 900.0], ..Ship::new(0, Side::Player) };
    let ms = MissionShip { id, name: format!("Ship {id}"), owner: owner.into(), visible: true, ..default() };
    let health = Health { sections: vec![], core: 100.0, destroyed: false };
    app.world_mut().spawn((ship, health, Transform::from_translation(at), Visibility::Inherited, ms, crate::collision::Drift(Vec3::X * 3.0))).id()
}
fn once(name: &str, conds: serde_json::Value, acts: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"name": name, "Run Once": true, "conditions": conds, "actions": acts})
}
fn init() -> serde_json::Value {
    serde_json::json!([{"Type": "World Initialize"}])
}
fn when(flag: &str) -> serde_json::Value {
    serde_json::json!([{"Type": "Flag Condition", "Flag Name": flag, "Boolean Value": "TRUE"}])
}

#[test]
fn op_teleport_places_a_mixed_group_in_its_layout_turned_to_the_heading_and_at_rest() {
    let mut app = order_app(serde_json::json!({
        "players": ["Player0 Group"],
        "groups": {"Fleet": [1, 2, 3]},
        "environment": {"point_sets": {"harbour": [{"pos": [0.0, 0.0, 0.0]}, {"pos": [100.0, 200.0, 0.0]}]}},
        "rules": [once("move", init(), serde_json::json!([{"Type": "OP Teleport", "Group/Unit Name": "Fleet", "Point Set Name": "harbour", "Point Index": 1, "Heading": 90}]))],
    }));
    let (a, b) = (order_ship(&mut app, 1, "Navy", Vec3::ZERO), order_ship(&mut app, 2, "Navy", Vec3::new(20.0, 0.0, 0.0)));
    let wreck = app.world_mut().spawn((MissionBase { id: 3, owner: String::new() }, Transform::from_xyz(10.0, 0.0, -30.0))).id();
    app.update();
    // The centre (10, 0, -10) goes to game (100, 200) = Bevy (100, 0, -200); bearing 90 is heading -pi/2, and the first ship had 0.
    let (dest, turn) = (Vec3::new(100.0, 0.0, -200.0), Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2));
    for (e, was) in [(a, Vec3::ZERO), (b, Vec3::new(20.0, 0.0, 0.0))] {
        let (ship, tf) = (app.world().get::<Ship>(e).unwrap(), app.world().get::<Transform>(e).unwrap());
        assert!(tf.translation.distance(dest + turn * (was - Vec3::new(10.0, 0.0, -10.0))) < 1e-3, "{:?}", tf.translation);
        assert_eq!((ship.heading, ship.speed, ship.ang_vel, ship.waypoints.len()), (-std::f32::consts::FRAC_PI_2, 0.0, 0.0, 0), "turned to the heading, at rest, course dropped");
        assert!(tf.rotation.dot(turn).abs() > 0.99999, "the hull is drawn on the new heading at once");
        assert_eq!(app.world().get::<crate::collision::Drift>(e).unwrap().0, Vec3::ZERO, "no push left over");
        assert_eq!(ship.order, EngineOrder::Full, "the engine order is not touched");
    }
    let tf = app.world().get::<Transform>(wreck).unwrap();
    assert!(tf.translation.distance(dest + turn * Vec3::new(0.0, 0.0, -20.0)) < 1e-3, "scenery of the group keeps its place in the layout: {:?}", tf.translation);
    assert!(tf.rotation.dot(Quat::IDENTITY).abs() > 0.99999, "and is not turned");
}

#[test]
fn op_abandon_ship_disables_the_hull_once_and_leaves_it_alive() {
    let mut app = order_app(serde_json::json!({
        "players": ["Player0 Group"],
        "rules": [
            once("leave", init(), serde_json::json!([{"Type": "OP Abandon Ship", "Group Name": "Navy"}])),
            once("lost", serde_json::json!([{"Type": "Group Destroyed", "Group Name": "Navy"}]), serde_json::json!([{"Type": "Set Flag Action", "Flag Name": "lost", "Boolean Value": "TRUE"}])),
        ],
    }));
    let hull = order_ship(&mut app, 1, "Navy", Vec3::ZERO);
    app.update();
    app.update();
    let (ship, ms) = (app.world().get::<Ship>(hull).unwrap(), app.world().get::<MissionShip>(hull).unwrap());
    assert!(app.world().get::<crate::lifeboats::Abandoned>(hull).is_some());
    assert_eq!((ship.cap, ship.order, ship.waypoints.len()), (0.0, EngineOrder::Stop, 0));
    assert!(ms.hold_fire && !app.world().get::<Health>(hull).unwrap().destroyed);
    assert!(!app.world().resource::<Mission>().script.flag("lost"), "an abandoned hull is not a destroyed group");
}

#[test]
fn a_follow_order_is_ended_by_clear_all_ai_commands_and_by_hold() {
    for (flag, action) in [
        ("clear", serde_json::json!({"Type": "Clear all AI commands", "Group/Unit Name": "Escort"})),
        ("hold", serde_json::json!({"Type": "Set Group/Unit Hold Position", "Group/Unit Name": "Escort", "Boolean": "TRUE"})),
    ] {
        let mut app = order_app(serde_json::json!({
            "players": ["Player0 Group"],
            "rules": [
                once("follow", init(), serde_json::json!([{"Type": "OP Follow Group", "Group Name": "Escort", "Target Group": "Convoy", "Station": "Abeam Port", "Distance": 60}])),
                once("end", when("end"), serde_json::json!([action])),
            ],
        }));
        order_ship(&mut app, 1, "Convoy", Vec3::ZERO);
        let escort = order_ship(&mut app, 2, "Escort", Vec3::new(200.0, 0.0, 0.0));
        app.update();
        let order = app.world().get::<MissionShip>(escort).unwrap().follow.clone().expect("the order is stored on the ship");
        assert_eq!((order.group.as_str(), order.station, order.distance), ("Convoy", Station::AbeamPort, 60.0));
        app.world_mut().resource_mut::<Mission>().script.flags.insert("end".into(), true);
        app.update();
        assert!(app.world().get::<MissionShip>(escort).unwrap().follow.is_none(), "{flag} ends the follow order");
    }
}

#[test]
fn followers_with_the_same_station_line_up_instead_of_sharing_one_slot() {
    let mut app = ai_world();
    ai_ship(&mut app, "Leader", "Convoy", Vec3::ZERO, None, 0.0);
    let order = || Some(FollowOrder { group: "Convoy".into(), station: Station::Astern, distance: 40.0, leader: None });
    let first = ai_ship(&mut app, "One", "Escort", Vec3::new(30.0, 0.0, 400.0), order(), 0.0);
    let second = ai_ship(&mut app, "Two", "Escort", Vec3::new(-30.0, 0.0, 400.0), order(), 0.0);
    let abeam = ai_ship(
        &mut app,
        "Three",
        "Escort",
        Vec3::new(400.0, 0.0, 0.0),
        Some(FollowOrder { group: "Convoy".into(), station: Station::AbeamStarboard, distance: 40.0, leader: None }),
        0.0,
    );
    app.update();
    let aim = |e: Entity| app.world().get::<Ship>(e).unwrap().waypoints[0];
    // The synthetic class has radius 1: each further ship stands a hull (2) and 10 units further out.
    let (mut near, mut far) = (aim(first), aim(second));
    if near.z > far.z {
        std::mem::swap(&mut near, &mut far);
    }
    assert!(near.distance(Vec3::new(0.0, 0.0, 40.0)) < 1e-3 && far.distance(Vec3::new(0.0, 0.0, 52.0)) < 1e-3, "{near:?} {far:?}");
    assert!(aim(abeam).distance(Vec3::new(40.0, 0.0, 0.0)) < 1e-3, "another station is its own line");
}

#[test]
fn a_won_mission_hands_its_campaign_counters_on_through_the_rules_system_and_a_lost_one_does_not() {
    let mission = |end: serde_json::Value| serde_json::json!({"players": ["Player0 Group"], "rules": [once("end", init(), serde_json::json!([{"Type": "OP Set Campaign Counter", "Counter Name": "rescued", "Value": 3}, end]))]});
    let m = |id: &str| crate::campaign::CustomMission { id: id.into(), name: id.into(), description: String::new(), campaign: "Saga".into() };
    let list = [m("one"), m("two")];
    let mut app = order_app(mission(serde_json::json!({"Type": "Goto Next Level", "World": "two"})));
    app.update();
    assert_eq!(app.world().resource::<crate::campaign::CampaignValues>().incoming_in(&list, "two").get("rescued"), Some(&3.0), "mission 'one' was won");
    let mut app = order_app(mission(serde_json::json!({"Type": "End Game"})));
    app.update();
    assert!(app.world().resource::<Mission>().script.ended.as_ref().is_some_and(|e| !e.1));
    assert!(app.world().resource::<crate::campaign::CampaignValues>().incoming_in(&list, "two").is_empty(), "a lost mission hands on nothing");
}

#[test]
fn a_missions_mines_do_not_answer_to_their_owners_name_only_to_their_group() {
    let data = serde_json::json!({
        "players": ["Player0 Group", "Pirates"],
        "groups": {"Field": [5, 6]},
        "rules": [
            once("give", init(), serde_json::json!([{"Type": "Set Group/Unit Owner", "Group/Unit Name": "Field", "New Owner": "Pirates"}])),
            once("gone", serde_json::json!([{"Type": "Group Destroyed", "Group Name": "Pirates"}]), serde_json::json!([{"Type": "Set Flag Action", "Flag Name": "pirates destroyed", "Boolean Value": "TRUE"}])),
            once("one ship", serde_json::json!([{"Type": "Group has X members", "Group Name": "Pirates", "Equivalence": "Equal To", "number": 1}]), serde_json::json!([{"Type": "Set Flag Action", "Flag Name": "one pirate", "Boolean Value": "TRUE"}])),
            once("field", serde_json::json!([{"Type": "Group has X members", "Group Name": "Field", "Equivalence": "Equal To", "number": 2}]), serde_json::json!([{"Type": "Set Flag Action", "Flag Name": "two mines", "Boolean Value": "TRUE"}])),
            once("sweep", when("sweep"), serde_json::json!([{"Type": "Set Group/Unit Owner", "Group/Unit Name": "Pirates", "New Owner": "Player0 Group"}])),
        ],
    });
    let mut app = order_app(data);
    let pirate = order_ship(&mut app, 1, "Pirates", Vec3::ZERO);
    let mines: Vec<Entity> = [5, 6]
        .map(|id| app.world_mut().spawn((crate::combat::MapMine { id, bullet: "Bullet_StarMine_Med".into(), owner: String::new(), side: None }, Transform::default())).id())
        .into();
    app.update();
    app.update();
    let flag = |app: &App, f: &str| app.world().resource::<Mission>().script.flag(f);
    assert!(flag(&app, "one pirate") && flag(&app, "two mines") && !flag(&app, "pirates destroyed"), "the owner's name counts ships only; the group still names the mines");
    // An owner-wide order does not reach the mines.
    app.world_mut().resource_mut::<Mission>().script.flags.insert("sweep".into(), true);
    app.update();
    assert_eq!(app.world().get::<MissionShip>(pirate).unwrap().owner.as_str(), "Player0 Group");
    assert!(mines.iter().all(|e| app.world().get::<crate::combat::MapMine>(*e).unwrap().owner == "Pirates"), "the mines keep their owner");
    // With every ship of the faction dead and its mines resting, the faction is destroyed.
    let mut app = order_app(serde_json::json!({
        "players": ["Player0 Group", "Pirates"],
        "groups": {"Field": [5]},
        "rules": [
            once("give", init(), serde_json::json!([{"Type": "Set Group/Unit Owner", "Group/Unit Name": "Field", "New Owner": "Pirates"}])),
            once("gone", serde_json::json!([{"Type": "Group Destroyed", "Group Name": "Pirates"}]), serde_json::json!([{"Type": "Set Flag Action", "Flag Name": "pirates destroyed", "Boolean Value": "TRUE"}])),
        ],
    }));
    let pirate = order_ship(&mut app, 1, "Pirates", Vec3::ZERO);
    app.world_mut().spawn((crate::combat::MapMine { id: 5, bullet: "Bullet_StarMine_Med".into(), owner: String::new(), side: None }, Transform::default()));
    app.update();
    app.world_mut().get_mut::<Health>(pirate).unwrap().destroyed = true;
    app.update();
    app.update();
    assert!(flag(&app, "pirates destroyed"));
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
        order: crate::EngineOrder::Emergency,
        throttle: Some(0.8),
        waypoints: vec![Vec3::X],
        goal: Some(1.0),
        helm: crate::ship::Helm { steer: 0.7, boost: 2.0, age: 0.0 },
        ..Ship::new(0, Side::Player)
    };
    let mut tug = moving();
    teleport::stop(&mut tug, false);
    assert_eq!((tug.speed, tug.ang_vel, tug.helm.steer), (0.0, 0.0, 0.0));
    assert_eq!((tug.order, tug.throttle), (crate::EngineOrder::Emergency, Some(0.8)), "a computer ship keeps its order: told to follow a path it still goes");
    let mut own = moving();
    teleport::stop(&mut own, true);
    assert_eq!((own.speed, own.ang_vel, own.order, own.throttle, own.goal), (0.0, 0.0, crate::EngineOrder::Stop, None, None), "the player's ship stops for good");
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
    let moving = Ship { heading: 0.0, speed: 9.0, ang_vel: 0.2, order: crate::EngineOrder::Emergency, ..Ship::new(class, Side::Player) };
    let ms = |name: &str| MissionShip { name: name.into(), owner: owner.as_str().into(), visible: true, ..default() };
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
    assert_eq!((sa.speed, sa.ang_vel, sa.order), (0.0, 0.0, crate::EngineOrder::Stop), "the player's ship is at all stop");
    assert_eq!((sb.speed, sb.ang_vel, sb.order), (0.0, 0.0, crate::EngineOrder::Emergency), "a computer ship keeps its order");
}

// ---- The developer console in a battle that is not a campaign mission (asset-free: one synthetic class).

use crate::rules::console::{Cheats, Console, DevView};
use crate::rules::skirmish::SkirmishScript;

/// A singleplayer battle as the lobby makes it: ships with no `MissionShip`, no `Mission`, no script unless the test adds one.
fn console_battle() -> App {
    let mut app = App::new();
    app.init_resource::<Time>()
        .init_resource::<Target>()
        .init_resource::<RuleWorld>()
        .init_resource::<crate::campaign::CampaignPoints>()
        .init_resource::<crate::nebula::StormShift>()
        .init_resource::<crate::environment::Sensors>()
        .init_resource::<crate::ship::Fleets>()
        .init_resource::<Cheats>()
        .init_resource::<DevView>()
        .init_resource::<dev_console::BareScript>()
        .init_resource::<crate::scenario::RestartRequest>()
        .add_message::<DockMsg>()
        .add_message::<crate::boarding::BoardMsg>()
        .add_message::<crate::boarding::Captured>()
        .add_message::<crate::combat::AsteroidSetup>()
        .add_message::<crate::nebula::MeteorRoute>()
        .add_message::<crate::combat::Detonate>()
        .insert_resource(Console::new(true))
        .insert_resource(Orbit { yaw: 0.0, pitch: 0.4, dist: 90.0, focus: None })
        .insert_resource(Library { classes: vec![crate::fixed_tests::synthetic_class()], guns: default(), bullets: default(), names: default(), clips: default() })
        .add_systems(Update, (dev_console::attach_ship_state, dev_console::run).chain());
    app
}
/// A ship with the components of a lobby battle's: no `MissionShip`.
fn lobby_ship(app: &mut App, owner: &str, at: Vec3, primary: bool) -> Entity {
    let ship = Ship { heading: 0.0, ..Ship::new(0, Side::Player) };
    let mut e = app.world_mut().spawn((
        ship,
        Health { sections: vec![], core: 100.0, destroyed: false },
        Transform::from_translation(at),
        Visibility::Inherited,
        crate::ship::FleetMember(owner.into(), 0),
        crate::ShipName("RLS Legacy".into()),
    ));
    if primary {
        e.insert(PlayerShip);
    }
    e.id()
}
fn say(app: &mut App, lines: &[&str]) -> String {
    for l in lines {
        app.world_mut().resource_mut::<Console>().submit(l);
    }
    app.update();
    app.world().resource::<Console>().log.join("\n")
}
fn skirmish_script(data: serde_json::Value) -> SkirmishScript {
    SkirmishScript {
        script: Script::new(&data, serde_json::json!({})),
        sides: [("Player0".to_string(), Side::Player)].into(),
        paths: data["paths"].clone(),
        environment: serde_json::Value::Null,
        map_text: serde_json::Value::Null,
    }
}

#[test]
fn ship_commands_run_in_a_battle_with_no_mission_and_no_script() {
    let mut app = console_battle();
    app.world_mut().resource_mut::<crate::ship::Fleets>().local = "player0".into();
    let hero = lobby_ship(&mut app, "player0", Vec3::ZERO, true);
    let raider = lobby_ship(&mut app, "player1", Vec3::new(50.0, 0.0, 0.0), false);
    assert!(app.world().get_resource::<Mission>().is_none() && app.world().get_resource::<SkirmishScript>().is_none());
    // Queries run, mark nothing, and give the ships the state the commands read.
    let log = say(&mut app, &["where", "ships"]);
    assert!(log.contains("RLS Legacy at x 0.0, y"), "{log}");
    assert!(log.contains("2 unit(s)"), "{log}");
    assert!(!log.contains("needs"), "{log}");
    assert!(!app.world().resource::<Cheats>().used, "queries are not cheats");
    // tp: x east, y north; the Bevy position is (x, 0, -y).
    let log = say(&mut app, &["tp 100 -40"]);
    assert!(log.contains("teleported to 100, -40"), "{log}");
    app.update();
    let at = app.world().get::<Transform>(hero).unwrap().translation;
    assert!(at.distance(Vec3::new(100.0, 0.0, 40.0)) < 1.0, "{at:?}");
    assert!(app.world().resource::<Cheats>().used, "a command that changes the game marks the session, here as in a mission");
    // god, damage, kill on the target.
    let log = say(&mut app, &["god on"]);
    assert!(log.contains("ran Set Group/Unit Vulnerability"), "{log}");
    app.update();
    assert!(app.world().resource::<Cheats>().god);
    assert_eq!(app.world().get::<MissionShip>(hero).unwrap().invuln, 1.0);
    assert_eq!(app.world().get::<MissionShip>(raider).unwrap().owner.as_str(), "player1");
    app.world_mut().resource_mut::<Target>().0 = Some(raider);
    say(&mut app, &["kill"]);
    app.update();
    assert!(app.world().get::<Health>(raider).unwrap().destroyed, "kill reaches the target");
    assert!(!app.world().get::<Health>(hero).unwrap().destroyed);
    // View commands and restart.
    let log = say(&mut app, &["hud off", "restart"]);
    assert!(app.world().resource::<DevView>().hud_hidden, "{log}");
    assert!(app.world().resource::<crate::scenario::RestartRequest>().0);
}

#[test]
fn rule_and_mission_commands_say_what_they_need_with_no_script() {
    let mut app = console_battle();
    lobby_ship(&mut app, "player0", Vec3::ZERO, true);
    let log = say(&mut app, &["flags", "rules", "set key", "tp Harbour", "win", "nis end", "cam follow x"]);
    for word in ["flags", "rules", "set", "tp"] {
        assert!(log.contains(&format!("{word}: needs rules (this battle runs no rules script")), "{word}: {log}");
    }
    for word in ["win", "nis", "cam"] {
        assert!(log.contains(&format!("{word}: needs a campaign mission")), "{word}: {log}");
    }
    assert!(!app.world().resource::<Cheats>().used, "a refused command changes nothing");
}

#[test]
fn a_skirmish_script_serves_the_rule_commands_of_the_console() {
    let mut app = console_battle();
    let gate = once("Open the gate", serde_json::json!([]), serde_json::json!([{"Type": "Set Flag Action", "Flag Name": "gate", "Boolean Value": "TRUE"}]));
    app.insert_resource(skirmish_script(serde_json::json!({"rules": [gate], "environment": {"point_sets": {"Harbour": [{"pos": [10.0, 20.0, 0.0]}]}}})));
    let hero = lobby_ship(&mut app, "player0", Vec3::ZERO, true);
    app.world_mut().resource_mut::<crate::ship::Fleets>().local = "player0".into();
    let log = say(&mut app, &["rules", "fire gate", "flags", "tp Harbour", "win"]);
    assert!(log.contains("never fired") && log.contains("Open the gate"), "{log}");
    assert!(log.contains("fired 'Open the gate'"), "{log}");
    assert!(app.world().resource::<SkirmishScript>().script.flag("gate"), "the skirmish map's own script ran the rule");
    app.update();
    let at = app.world().get::<Transform>(hero).unwrap().translation;
    assert!(at.distance(Vec3::new(10.0, 0.0, -20.0)) < 1.0, "tp to a point set of the map's script: {at:?}");
    assert!(log.contains("win: needs a campaign mission"), "{log}");
}

#[test]
fn the_console_dispatch_is_the_same_in_a_campaign_mission() {
    let data = serde_json::json!({"rules": [], "player_start": {"name": "Hero"}});
    let mission = Mission { id: "synthetic".into(), script: Script::new(&data, serde_json::json!({})), data, opening: None };
    let mut app = console_battle();
    app.insert_resource(mission);
    let hero = app.world_mut().spawn((
        Ship::new(0, Side::Player),
        Health { sections: vec![], core: 100.0, destroyed: false },
        Transform::default(),
        Visibility::Inherited,
        PlayerShip,
        MissionShip { id: 1, name: "RLS Hawk".into(), owner: "Hero".into(), visible: true, ..default() },
    ));
    let hero = hero.id();
    let log = say(&mut app, &["tp 30 10", "flags", "cam follow Hero"]);
    assert!(!log.contains("needs"), "a mission serves every need: {log}");
    app.update();
    assert!(app.world().get::<Transform>(hero).unwrap().translation.distance(Vec3::new(30.0, 0.0, -10.0)) < 1.0);

/// 'Player Killed A Object' of kind Ship holds when a ship of another player sinks under the player's guns (`combat::deaths` sends
/// `Destroyed`); a hull of the player's own does not count. Asset-free.
#[test]
fn a_sunk_ship_is_a_kill_for_the_player_who_sank_it() {
    let rule = |flag: &str, kind: &str| {
        once(
            flag,
            serde_json::json!([{"Type": "Player Killed A Object", "Player Name": "Navy", "World Object Type": kind}]),
            serde_json::json!([{"Type": "Set Flag Action", "Flag Name": flag, "Boolean Value": "TRUE"}]),
        )
    };
    let mut app = order_app(serde_json::json!({"players": ["Player0 Group"], "rules": [rule("ship kill", "Ship"), rule("rock kill", "Asteroid")]}));
    let (gun, own, raider) = (order_ship(&mut app, 1, "Navy", Vec3::ZERO), order_ship(&mut app, 2, "Navy", Vec3::X * 20.0), order_ship(&mut app, 3, "Raiders", Vec3::X * 40.0));
    app.update();
    app.world_mut().write_message(Destroyed { by: gun, victim: own });
    app.update();
    app.update();
    assert!(!app.world().resource::<Mission>().script.flag("ship kill"), "sinking the player's own hull is no kill");
    app.world_mut().write_message(Destroyed { by: gun, victim: raider });
    app.update();
    app.update();
    let script = &app.world().resource::<Mission>().script;
    assert!(script.flag("ship kill"), "a raider sunk by the player's ship is a ship kill");
    assert!(!script.flag("rock kill"), "and not a rock kill");
}
