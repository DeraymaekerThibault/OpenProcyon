use super::*;
use crate::Side;
use crate::scenario::SpawnIndex;
use bevy::ecs::system::RunSystemOnce;
use bevy::time::TimeUpdateStrategy;

/// The production battle clock with the command path, in a battle that is still loading until `loaded` (as `loading::wait` lifts it:
/// in a frame's `Update`, so the frame's `Last` already sees it gone).
fn battle() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins((crate::battle_schedule, plugin))
        .insert_resource(State::new(crate::menu::Screen::Battle))
        .insert_resource(crate::loading::Loading::default())
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
        .init_resource::<Lift>()
        .add_systems(Update, |mut lift: ResMut<Lift>, mut commands: Commands| {
            if std::mem::take(&mut lift.0) {
                commands.remove_resource::<crate::loading::Loading>();
            }
        });
    app.update();
    app
}
#[derive(Resource, Default)]
struct Lift(bool);

fn loaded(app: &mut App) {
    app.world_mut().resource_mut::<Lift>().0 = true;
    app.update();
}

/// One frame of `ms` real time (a replay's clock overrides it).
fn frame(app: &mut App, ms: u64) {
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(ms)));
    app.update();
}

fn send(app: &mut App, kind: Kind) {
    app.world_mut().resource_mut::<Outbox>().0.push(PlayerCommand { player: PlayerId(0), kind });
}

fn order(app: &App, e: Entity) -> EngineOrder {
    app.world().get::<Ship>(e).unwrap().order
}

/// The player's command ship (index 0 of the local fleet) and a fleet captain beside it, numbered as at battle start.
fn fleet(app: &mut App) -> (Entity, Entity, Entity) {
    let w = app.world_mut();
    let me = w.spawn((Ship::new(0, Side::Player), SpawnIndex(0), Commander(PlayerId(0)))).id();
    let captain = w.spawn((Ship::new(0, Side::Player), crate::ship::FleetPilot::default(), SpawnIndex(1))).id();
    let enemy = w.spawn((Ship::new(0, Side::Enemy), SpawnIndex(2))).id();
    let mut fleets = crate::ship::Fleets::default();
    fleets.groups.insert(fleets.local.clone(), crate::ship::Fleet { members: vec![me, captain], ..default() });
    w.insert_resource(fleets);
    w.init_resource::<crate::combat::Target>();
    (me, captain, enemy)
}

#[test]
fn an_order_lands_on_the_next_tick_and_one_given_while_paused_at_the_end_of_its_frame() {
    let mut app = battle();
    let (me, ..) = fleet(&mut app);
    loaded(&mut app);
    send(&mut app, Kind::Engine { ship: me, engine: Engine::To(EngineOrder::Full) });
    frame(&mut app, 10);
    assert_eq!(order(&app, me), EngineOrder::Stop, "no tick ran in this frame: the order waits");
    frame(&mut app, 30);
    assert_eq!(order(&app, me), EngineOrder::Full, "the next tick applies it");
    app.world_mut().resource_mut::<Time<Virtual>>().pause();
    send(&mut app, Kind::Engine { ship: me, engine: Engine::Faster });
    frame(&mut app, 100);
    assert_eq!(order(&app, me), EngineOrder::Emergency, "paused: applied at the end of the frame, as the dial answered before");
    send(&mut app, Kind::Engine { ship: me, engine: Engine::Slower });
    send(&mut app, Kind::Engine { ship: me, engine: Engine::Slower });
    frame(&mut app, 100);
    assert_eq!(order(&app, me), EngineOrder::Slow, "two steps in one paused frame both count");
}

#[test]
fn a_target_is_the_players_own_on_the_command_ship_and_an_attack_order_on_a_captain() {
    let mut app = battle();
    let (me, captain, enemy) = fleet(&mut app);
    loaded(&mut app);
    app.world_mut().resource_mut::<crate::combat::Target>().2.panic = true;
    send(&mut app, Kind::Target { ship: me, target: Some(enemy), spot: Some(Vec3::X) });
    send(&mut app, Kind::Target { ship: captain, target: Some(enemy), spot: None });
    frame(&mut app, 40);
    let t = app.world().resource::<crate::combat::Target>();
    assert_eq!((t.0, t.1, t.2.panic), (Some(enemy), Some(Vec3::X), false), "the player's target and spot; a retarget ends panic fire");
    assert_eq!(app.world().get::<crate::ship::FleetPilot>(captain).unwrap().target, Some(enemy));
    send(&mut app, Kind::Target { ship: me, target: None, spot: None });
    send(&mut app, Kind::Target { ship: captain, target: None, spot: None });
    frame(&mut app, 40);
    assert_eq!(app.world().resource::<crate::combat::Target>().0, None, "C clears the command ship's target");
    assert_eq!(app.world().get::<crate::ship::FleetPilot>(captain).unwrap().target, Some(enemy), "a captain keeps its attack order");
}

/// A click on water: the command ship sails there (Shift appends, a stopped ship goes to light-green Full), a captain is ordered
/// there; both end a hold and a dock routine. Backspace drops the course, and the armed shot or the captain's orders.
#[test]
fn move_course_and_cancel_orders_do_what_the_clicks_and_keys_did() {
    let mut app = battle();
    app.add_message::<crate::docking::DockMsg>();
    let (me, captain, enemy) = fleet(&mut app);
    loaded(&mut app);
    app.world_mut().entity_mut(me).insert(crate::ship::HoldPosition);
    app.world_mut().entity_mut(captain).insert(crate::ship::HoldPosition);
    let (a, b, c) = (Vec3::X, Vec3::Z, Vec3::NEG_X);
    send(&mut app, Kind::Move { ship: me, to: a, append: false });
    send(&mut app, Kind::Move { ship: me, to: b, append: true });
    send(&mut app, Kind::Move { ship: captain, to: c, append: false });
    frame(&mut app, 40);
    let ship = |app: &App, e| app.world().get::<Ship>(e).unwrap().clone();
    assert_eq!((ship(&app, me).waypoints, ship(&app, me).order), (vec![a, b], EngineOrder::Full));
    assert_eq!(app.world().get::<crate::ship::FleetPilot>(captain).unwrap().destination, Some(c));
    assert!(ship(&app, captain).waypoints.is_empty(), "a captain's move is an order, not the command ship's course");
    for e in [me, captain] {
        assert!(app.world().get::<crate::ship::HoldPosition>(e).is_none(), "a move ends a hold");
    }
    let cancels: Vec<_> = app.world_mut().resource_mut::<Messages<crate::docking::DockMsg>>().drain().collect();
    assert_eq!(cancels.iter().filter(|m| matches!(m, crate::docking::DockMsg::CancelShip(_))).count(), 3, "each move ends a dock routine");
    send(&mut app, Kind::Move { ship: me, to: c, append: false });
    frame(&mut app, 40);
    assert_eq!(ship(&app, me).waypoints, vec![c], "without Shift the course starts again");
    // A plotted course replaces the course; only the click that closes it ends a hold.
    app.world_mut().entity_mut(me).insert(crate::ship::HoldPosition);
    app.world_mut().get_mut::<Ship>(me).unwrap().order = EngineOrder::Stop;
    send(&mut app, Kind::Course { ship: me, points: vec![b, a], release: false });
    frame(&mut app, 40);
    assert_eq!((ship(&app, me).waypoints, ship(&app, me).order), (vec![b, a], EngineOrder::Full));
    assert!(app.world().get::<crate::ship::HoldPosition>(me).is_some(), "Enter keeps the hold");
    send(&mut app, Kind::Course { ship: me, points: vec![a], release: true });
    frame(&mut app, 40);
    assert!(app.world().get::<crate::ship::HoldPosition>(me).is_none(), "the closing click ends it");
    // Backspace: the command ship keeps its engines and loses the armed shot; a captain stops and forgets its orders.
    {
        let mut t = app.world_mut().resource_mut::<crate::combat::Target>();
        t.2.armed = Some(1);
        t.2.shot = Some((1, Vec3::ONE));
    }
    app.world_mut().get_mut::<crate::ship::FleetPilot>(captain).unwrap().attack(enemy);
    app.world_mut().get_mut::<crate::ship::FleetPilot>(captain).unwrap().hold = true;
    app.world_mut().get_mut::<Ship>(captain).unwrap().order = EngineOrder::Full;
    send(&mut app, Kind::Cancel { ship: me });
    send(&mut app, Kind::Cancel { ship: captain });
    frame(&mut app, 40);
    let t = app.world().resource::<crate::combat::Target>();
    assert_eq!((t.2.armed, t.2.shot), (None, None));
    assert_eq!((ship(&app, me).waypoints.len(), ship(&app, me).order), (0, EngineOrder::Full));
    let p = app.world().get::<crate::ship::FleetPilot>(captain).unwrap();
    assert_eq!((p.destination, p.target, p.hold, ship(&app, captain).order), (None, None, false, EngineOrder::Stop));
}

/// The right-click menu's Stop, Hold and Cloak.
#[test]
fn stop_hold_and_cloak_do_what_the_menu_did() {
    let mut app = battle();
    app.add_message::<crate::docking::DockMsg>();
    let (me, captain, enemy) = fleet(&mut app);
    loaded(&mut app);
    {
        let w = app.world_mut();
        let mut s = w.get_mut::<Ship>(me).unwrap();
        (s.waypoints, s.order) = (vec![Vec3::X], EngineOrder::Full);
        let mut p = w.get_mut::<crate::ship::FleetPilot>(captain).unwrap();
        p.move_to(Vec3::Z);
        p.hold = true;
        w.entity_mut(me).insert(crate::cloak::Cloak::new(false));
    }
    let ship = |app: &App, e| app.world().get::<Ship>(e).unwrap().clone();
    send(&mut app, Kind::Stop { ship: me });
    send(&mut app, Kind::Stop { ship: captain });
    frame(&mut app, 40);
    assert_eq!((ship(&app, me).waypoints.len(), ship(&app, me).order), (0, EngineOrder::Stop));
    let p = app.world().get::<crate::ship::FleetPilot>(captain).unwrap();
    assert_eq!((p.destination, p.hold), (None, true), "Stop leaves the formation hold");
    let msgs: Vec<_> = app.world_mut().resource_mut::<Messages<crate::docking::DockMsg>>().drain().collect();
    assert_eq!(msgs.len(), 2, "Stop also ends a dock routine");
    app.world_mut().get_mut::<crate::ship::FleetPilot>(captain).unwrap().attack(enemy);
    app.world_mut().get_mut::<crate::ship::FleetPilot>(captain).unwrap().move_to(Vec3::X);
    app.world_mut().get_mut::<Ship>(captain).unwrap().order = EngineOrder::Full;
    send(&mut app, Kind::Hold { ship: captain, on: true });
    frame(&mut app, 40);
    assert!(app.world().get::<crate::ship::HoldPosition>(captain).is_some());
    assert_eq!((ship(&app, captain).order, app.world().get::<crate::ship::FleetPilot>(captain).unwrap().destination), (EngineOrder::Stop, None));
    send(&mut app, Kind::Hold { ship: captain, on: false });
    frame(&mut app, 40);
    assert!(app.world().get::<crate::ship::HoldPosition>(captain).is_none());
    send(&mut app, Kind::Cloak { ship: me, on: true });
    send(&mut app, Kind::Cloak { ship: enemy, on: true }); // no cloak: nothing happens
    frame(&mut app, 40);
    assert!(app.world().get::<crate::cloak::Cloak>(me).unwrap().stop_shown(), "Cloak Start: cloaking");
    send(&mut app, Kind::Cloak { ship: me, on: false });
    frame(&mut app, 40);
    assert!(!app.world().get::<crate::cloak::Cloak>(me).unwrap().stop_shown(), "Cloak Stop: decloaking");
}

/// Dock, tow, board and grapple orders reach docking and boarding as the messages the menu and the click sent; the menu's Dock,
/// Tow Start and Board Start end a hold, the click's dock does not.
#[test]
fn dock_tow_board_and_grapple_orders_send_what_the_menu_sent() {
    use crate::boarding::BoardMsg;
    use crate::docking::DockMsg;
    let mut app = battle();
    app.add_message::<DockMsg>().add_message::<BoardMsg>();
    let (me, captain, enemy) = fleet(&mut app);
    loaded(&mut app);
    let held = |app: &mut App| {
        let held = app.world().get::<crate::ship::HoldPosition>(me).is_some();
        app.world_mut().entity_mut(me).insert(crate::ship::HoldPosition);
        held
    };
    held(&mut app);
    let docks = |app: &mut App| app.world_mut().resource_mut::<Messages<DockMsg>>().drain().map(|m| format!("{m:?}")).collect::<Vec<_>>();
    let boards = |app: &mut App| app.world_mut().resource_mut::<Messages<BoardMsg>>().drain().map(|m| format!("{m:?}")).collect::<Vec<_>>();
    let cases: Vec<(Kind, bool, String, bool)> = vec![
        (Kind::Dock { ship: me, dockee: captain, release: false }, true, format!("{:?}", DockMsg::Dock { docker: Some(me), dockee: captain }), true),
        (Kind::Dock { ship: me, dockee: captain, release: true }, false, format!("{:?}", DockMsg::Dock { docker: Some(me), dockee: captain }), true),
        (Kind::Tow { ship: me, towee: Some(captain) }, false, format!("{:?}", DockMsg::TowStart { tower: Some(me), towee: captain }), true),
        (Kind::Tow { ship: me, towee: None }, true, format!("{:?}", DockMsg::TowBreak { tower: Some(me) }), true),
        (Kind::CancelDock { ship: me }, true, format!("{:?}", DockMsg::CancelShip(me)), true),
        (Kind::Board { ship: me, target: Some(enemy) }, false, format!("{:?}", BoardMsg::Board { boarder: Some(me), target: enemy }), false),
        (Kind::Board { ship: me, target: None }, true, format!("{:?}", BoardMsg::Unboard { boarder: Some(me) }), false),
        (Kind::Grapple { ship: me, target: Some(enemy) }, true, format!("{:?}", BoardMsg::Grapple { grappler: Some(me), target: enemy }), false),
        (Kind::Grapple { ship: me, target: None }, true, format!("{:?}", BoardMsg::Ungrapple { grappler: Some(me) }), false),
    ];
    for (kind, keeps_hold, msg, dock) in cases {
        send(&mut app, kind.clone());
        frame(&mut app, 40);
        assert_eq!(held(&mut app), keeps_hold, "{kind:?}");
        let (d, b) = (docks(&mut app), boards(&mut app));
        assert_eq!(if dock { (d, b) } else { (b, d) }, (vec![msg], vec![]), "{kind:?}");
    }
    app.world_mut().entity_mut(me).insert(crate::boarding::GrappleOrder(enemy));
    send(&mut app, Kind::DropGrapple { ship: me });
    frame(&mut app, 40);
    assert!(app.world().get::<crate::boarding::GrappleOrder>(me).is_none());
}

/// Panic fire and indirect shots act on the command ship's own gunnery state only.
#[test]
fn panic_fire_and_lob_shots_are_the_command_ships() {
    let mut app = battle();
    let (me, captain, _) = fleet(&mut app);
    loaded(&mut app);
    send(&mut app, Kind::PanicFire { ship: captain });
    send(&mut app, Kind::Lob { ship: captain, shot: Some((2, Vec3::X)) });
    frame(&mut app, 40);
    let t = app.world().resource::<crate::combat::Target>();
    assert_eq!((t.2.panic, t.2.shot), (false, None), "a captain has no panic button or armed bank");
    send(&mut app, Kind::PanicFire { ship: me });
    send(&mut app, Kind::Lob { ship: me, shot: Some((2, Vec3::X)) });
    frame(&mut app, 40);
    let t = app.world().resource::<crate::combat::Target>();
    assert_eq!((t.2.panic, t.2.shot), (true, Some((2, Vec3::X))));
    send(&mut app, Kind::Lob { ship: me, shot: None });
    frame(&mut app, 40);
    assert_eq!(app.world().resource::<crate::combat::Target>().2.shot, None, "a gun-bank click drops a shot not fired yet");
}

/// Everything a player can click has an id: ships, armed bases and rocks by creation number, space animals too, and map scenery
/// without one (a base without guns, an island) by its place in the map's object list. Numbers of different kinds do not mix.
#[test]
fn space_animals_and_bases_without_guns_have_ids() {
    use crate::scenario::SceneryIndex;
    let mut app = battle();
    let (me, _, enemy) = fleet(&mut app);
    let w = app.world_mut();
    let whale = w.spawn((crate::environment::SpaceAnimal::new("Whales", 5.0, Vec3::ZERO, 1, true), SpawnIndex(0))).id();
    let stockade = w.spawn((Transform::default(), SceneryIndex(4))).id();
    let island = w.spawn((Transform::default(), SceneryIndex(0))).id();
    let armed = w.spawn((crate::combat::ArmedBase { class: 0 }, SpawnIndex(0), SceneryIndex(5))).id();
    let shell = w.spawn(SpawnIndex(0)).id();
    let id = |w: &World, e| NetId::of(w, e);
    let w = app.world_mut();
    assert_eq!(id(w, whale), Some(NetId { kind: NetKind::Animal, index: 0 }));
    assert_eq!(id(w, stockade), Some(NetId { kind: NetKind::Scenery, index: 4 }));
    assert_eq!(id(w, armed), Some(NetId { kind: NetKind::ArmedBase, index: 0 }), "a numbered kind keeps its creation number");
    assert_eq!(id(w, shell), None, "nothing to click");
    for e in [me, enemy, whale, stockade, island, armed] {
        let found = id(w, e).unwrap().find(w);
        assert_eq!(found, Some(e), "{:?}", id(w, e));
    }
    let saved = Saved::save(w, &Kind::Dock { ship: me, dockee: stockade, release: true }).unwrap();
    assert_eq!(serde_json::to_string(&saved).unwrap(), r#"{"dock":{"ship":{"kind":"ship","index":0},"dockee":{"kind":"scenery","index":4},"release":true}}"#);
    assert_eq!(saved.load(w), Some(Kind::Dock { ship: me, dockee: stockade, release: true }));
    assert!(Saved::save(w, &Kind::Target { ship: me, target: Some(whale), spot: None }).is_ok(), "a whale is a target with an id");
}

/// `Commander` follows `PlayerShip` (battle start, promotion, loss), and "direct" is decided by the command's player: another
/// player's order to this command ship is not its commander's.
#[test]
fn the_command_ship_is_the_commanders_and_only_its_players_orders_are_direct() {
    let mut app = battle();
    let (me, captain, enemy) = fleet(&mut app);
    loaded(&mut app);
    let w = app.world_mut();
    w.entity_mut(me).remove::<Commander>();
    w.entity_mut(captain).insert(crate::PlayerShip);
    w.flush();
    assert_eq!(w.get::<Commander>(captain), Some(&Commander(PlayerId(0))), "the player ship is player 0's command ship");
    w.entity_mut(captain).remove::<crate::PlayerShip>();
    w.entity_mut(me).insert(crate::PlayerShip);
    w.flush();
    assert_eq!((w.get::<Commander>(captain), w.get::<Commander>(me)), (None, Some(&Commander(PlayerId(0)))), "promotion moves it");
    app.world_mut().resource_mut::<crate::combat::Target>().2.panic = true;
    app.world_mut().resource_mut::<Outbox>().0.push(PlayerCommand { player: PlayerId(1), kind: Kind::Target { ship: me, target: Some(enemy), spot: None } });
    app.world_mut().resource_mut::<Outbox>().0.push(PlayerCommand { player: PlayerId(1), kind: Kind::PanicFire { ship: me } });
    frame(&mut app, 40);
    let t = app.world().resource::<crate::combat::Target>();
    assert_eq!((t.0, t.2.panic), (None, true), "player 1 does not command player 0's ship");
    send(&mut app, Kind::Target { ship: me, target: Some(enemy), spot: None });
    frame(&mut app, 40);
    assert_eq!(app.world().resource::<crate::combat::Target>().0, Some(enemy));
}

/// A replay gives each command back to the player who gave it.
#[test]
fn a_replay_keeps_the_player_of_each_command() {
    let header = r#"{"format":2,"version":"0.0.0","seed":5,"args":[],"mods":[]}"#;
    let line = |p: u8| format!(r#"{{"command":{{"at":{{"tick":0}},"player":{p},"order":{{"cease_fire":null}}}}}}"#);
    let text = format!("{header}\n{}\n{}\n", line(0), line(3));
    let (_, replay, _) = Replay::read(text.as_bytes()).unwrap();
    assert_eq!(replay.commands.iter().map(|c| c.1).collect::<Vec<_>>(), [PlayerId(0), PlayerId(3)]);
    let mut app = battle();
    fleet(&mut app);
    app.insert_resource(replay);
    loaded(&mut app);
    let got = replayed(app.world_mut(), At::Tick(0)).unwrap();
    assert_eq!(got.iter().map(|c| c.player).collect::<Vec<_>>(), [PlayerId(0), PlayerId(3)]);
}

#[test]
fn a_command_keeps_its_meaning_through_the_file() {
    let mut app = battle();
    let (me, _, enemy) = fleet(&mut app);
    let kinds = [
        Kind::Engine { ship: me, engine: Engine::Faster },
        Kind::Engine { ship: me, engine: Engine::To(EngineOrder::Emergency) },
        Kind::Target { ship: me, target: Some(enemy), spot: Some(Vec3::new(0.25, -1.5, 3.0)) },
        Kind::Target { ship: me, target: None, spot: None },
        Kind::Move { ship: me, to: Vec3::new(10.0, 0.0, -20.5), append: true },
        Kind::Course { ship: me, points: vec![Vec3::new(1.0, 0.0, 2.0), Vec3::new(-3.0, 0.0, 4.0)], release: false },
        Kind::Cancel { ship: me },
        Kind::Stop { ship: me },
        Kind::Hold { ship: me, on: true },
        Kind::Cloak { ship: me, on: false },
        Kind::Dock { ship: me, dockee: enemy, release: true },
        Kind::Tow { ship: me, towee: None },
        Kind::Board { ship: me, target: Some(enemy) },
        Kind::Grapple { ship: me, target: None },
        Kind::CancelDock { ship: me },
        Kind::DropGrapple { ship: me },
        Kind::Formation { formation: crate::ship::Formation::EchelonLeft, hold: true },
        Kind::FleetAttack,
        Kind::CeaseFire,
        Kind::Promote { ship: me },
        Kind::PanicFire { ship: me },
        Kind::Lob { ship: me, shot: Some((3, Vec3::new(5.0, 0.0, -7.0))) },
        Kind::Lob { ship: me, shot: None },
    ];
    for kind in kinds {
        let saved = Saved::save(app.world(), &kind).unwrap();
        let text = serde_json::to_string(&Line::Command { at: At::Tick(7), player: PlayerId(0), order: saved }).unwrap();
        assert!(!text.contains("Ship_") && !text.contains("index\":0,\"kind"), "ids only: {text}");
        let Line::Command { order, .. } = serde_json::from_str(&text).unwrap() else { panic!("{text}") };
        assert_eq!(order.load(app.world_mut()), Some(kind), "{text}");
    }
    let unnumbered = app.world_mut().spawn(Transform::default()).id();
    assert!(Saved::save(app.world(), &Kind::Target { ship: me, target: Some(unnumbered), spot: None }).is_err(), "no id, no line");
    let json = serde_json::to_string(&Saved::save(app.world(), &Kind::Engine { ship: me, engine: Engine::To(EngineOrder::Full) }).unwrap()).unwrap();
    assert_eq!(json, r#"{"engine":{"ship":{"kind":"ship","index":0},"engine":{"to":2}}}"#);
}

#[test]
fn a_recording_reads_back_with_its_frames_unpacked() {
    let header = r#"{"format":1,"version":"0.0.0","seed":5,"args":["--map","red_rover"],"mods":[]}"#;
    let text = format!(
        "{header}\n{}\n{}\n{}\n",
        r#"{"frames":[[0,0],[16000000,0,3],[33000000,1]]}"#,
        r#"{"command":{"at":{"frame":2},"player":0,"order":{"engine":{"ship":{"kind":"ship","index":0},"engine":"slower"}}}}"#,
        r#"{"unrecorded":{"at":{"tick":4},"what":"an object without an id"}}"#
    );
    let (h, replay, unrecorded) = Replay::read(text.as_bytes()).unwrap();
    assert_eq!((h.seed, h.args.as_slice(), unrecorded), (5, ["--map".to_string(), "red_rover".to_string()].as_slice(), 1));
    let ms = |m: u64| Duration::from_millis(m);
    assert_eq!(replay.frames, vec![(ms(0), 0), (ms(16), 0), (ms(16), 0), (ms(16), 0), (ms(33), 1)]);
    assert_eq!(replay.commands, vec![(At::Frame(2), PlayerId(0), Saved::Engine { ship: NetId { kind: NetKind::Ship, index: 0 }, engine: SavedEngine::Slower })]);
    assert!(Replay::read(header.replace("\"format\":1", "\"format\":3").as_bytes()).is_err(), "a newer format is refused");
    assert!(Replay::read(header.replace("\"format\":1", "\"format\":0").as_bytes()).is_err());
}

#[test]
fn the_recorded_arguments_are_the_ones_that_decide_the_battle() {
    let args: Vec<String> =
        ["game", "--lobby", "map=x", "--speed", "16", "--script", "commands", "--fixed-step", "--seed", "3", "--replay", "f.jsonl"].iter().map(|s| s.to_string()).collect();
    assert_eq!(scenario_args(&args), ["--lobby", "map=x", "--seed", "3"]);
    let header = Header { format: FORMAT, version: "x".into(), seed: 3, args: scenario_args(&args), mods: vec![], from_menu: false, skirmish: None };
    assert_eq!(mismatch(&header, &args, 3, &[]), None);
    assert!(mismatch(&header, &args, 4, &[]).unwrap().contains("--seed 3"));
    assert!(mismatch(&header, &["game".to_string(), "--map".to_string(), "y".to_string()], 3, &[]).is_some());
    assert!(mismatch(&header, &args, 3, &["remaster".to_string()]).is_some());
    let menu = Header { args: vec![], from_menu: true, ..header };
    assert!(mismatch(&menu, &["game".to_string()], 3, &[]).unwrap().contains("menus"), "a battle set up in the menus is refused, not replayed wrong");
    assert!(sets_up_battle(&scenario_args(&args)) && !sets_up_battle(&[]));
}

/// A battle launched from the lobby keeps the lobby's setup instead of `--lobby`, and replays from `--replay <file>` alone.
#[test]
fn a_lobby_battle_keeps_its_setup_and_replays_without_arguments() {
    let setup = crate::menu::SkirmishSetup::parse(r#"{"map":"m","points":300,"limit":5,"slots":[{"kind":"human","race":"navy","team":0,"flag":0}]}"#).unwrap();
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let header = Header::new(0, &a(&["game", "--lobby", "map=m;slots=human,computer", "--speed", "4"]), Some(setup.clone()));
    assert_eq!((header.format, header.args.as_slice(), header.from_menu), (2, [].as_slice(), false));
    assert_eq!(mismatch(&header, &a(&["game", "--replay", "f"]), 0, &mod_names()), None);
    assert!(mismatch(&header, &a(&["game", "--replay", "f", "--lobby", "map=m"]), 0, &mod_names()).unwrap().contains("leave out --lobby"));
    let menu = Header::new(0, &a(&["game"]), Some(setup.clone()));
    assert!(!menu.from_menu, "a skirmish set up by hand in the menus replays");
    assert!(Header::new(0, &a(&["game"]), None).from_menu, "a campaign mission from the menus is still refused");
    assert_eq!(Header::new(0, &a(&["game", "--map", "x"]), None).args, ["--map", "x"]);
    // The header line round-trips, and `recorded_setup` finds the setup for the launch.
    let path = std::env::temp_dir().join(format!("openprocyon-commands-setup-{}.jsonl", std::process::id()));
    std::fs::write(&path, format!("{}\n{{\"frames\":[]}}\n", serde_json::to_string(&header).unwrap())).unwrap();
    let found = recorded_setup(&a(&["game", "--replay", &path.to_string_lossy()]));
    let read = Replay::read(std::io::BufReader::new(std::fs::File::open(&path).unwrap())).map(|r| r.0);
    let _ = std::fs::remove_file(&path);
    assert_eq!(found, Some(setup));
    assert_eq!(read.unwrap(), header);
    assert_eq!(recorded_setup(&a(&["game", "--replay", "/no/such/file"])), None);
}

#[test]
fn a_recording_keeps_no_paths() {
    let args: Vec<String> =
        ["game", "--map", "red_rover", "--mod", "/home/someone/mods/hunter", "--rule-report", "C:/Users/someone/r.json"].iter().map(|s| s.to_string()).collect();
    assert_eq!(scenario_args(&args), ["--map", "red_rover"], "mods are compared by folder name; no folder of the player's goes in the file");
}

#[test]
fn a_damaged_recording_cannot_ask_for_unbounded_frames() {
    let header = r#"{"format":1,"version":"0.0.0","seed":5,"args":[],"mods":[]}"#;
    let text = format!(
        "{header}
{{\"frames\":[[16000000,0,18446744073709551615]]}}
"
    );
    assert!(Replay::read(text.as_bytes()).unwrap_err().contains("frames"));
}

#[test]
fn frames_held_back_are_written_when_the_recorder_goes() {
    let path = std::env::temp_dir().join(format!("openprocyon-commands-drop-{}.jsonl", std::process::id()));
    let mut rec = Recorder { out: std::io::BufWriter::new(std::fs::File::create(&path).unwrap()), pending: None, frames: vec![], warned: false };
    rec.frame(Duration::from_millis(16), 0);
    drop(rec);
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(text.trim(), r#"{"frames":[[16000000,0]]}"#);
}

/// A restart or a new battle starts the clock again: orders answer at once while paused in every battle, not only the first, and
/// the recording keeps only the first battle.
#[test]
fn orders_work_the_same_in_a_second_battle() {
    let mut app = battle();
    let (me, ..) = fleet(&mut app);
    loaded(&mut app);
    frame(&mut app, 40);
    app.world_mut().insert_resource(crate::loading::Loading::default()); // R: the battle loads again
    frame(&mut app, 40);
    loaded(&mut app);
    frame(&mut app, 40);
    assert_eq!(app.world().resource::<Clock>().battle, 1);
    app.world_mut().resource_mut::<Time<Virtual>>().pause();
    send(&mut app, Kind::Engine { ship: me, engine: Engine::To(EngineOrder::Full) });
    frame(&mut app, 100);
    assert_eq!(order(&app, me), EngineOrder::Full, "paused in a second battle: still applied at the end of the frame");
    app.world_mut().resource_mut::<Time<Virtual>>().unpause();
    send(&mut app, Kind::Engine { ship: me, engine: Engine::Slower });
    frame(&mut app, 40);
    assert_eq!(order(&app, me), EngineOrder::Slow);
}

/// Past the end of its recording a replay hands the battle back: live orders work again.
#[test]
fn live_orders_work_again_after_the_end_of_a_replay() {
    let mut app = battle();
    let (me, ..) = fleet(&mut app);
    app.insert_resource(Replay { frames: vec![(Duration::from_millis(40), 1); 2], commands: vec![], finished: false }).add_systems(Last, replay_clock);
    loaded(&mut app);
    for _ in 0..4 {
        app.update();
    }
    assert!(app.world().resource::<Replay>().finished);
    send(&mut app, Kind::Engine { ship: me, engine: Engine::To(EngineOrder::Full) });
    frame(&mut app, 40);
    assert_eq!(order(&app, me), EngineOrder::Full);
}

/// The point of the log: a session on a jittery clock, with a pause and orders given at odd moments, is recorded, and the replay
/// on a different frame rate runs the same ticks in the same frames and applies every order at the same moment.
#[test]
fn a_recorded_session_replays_tick_for_tick_on_another_frame_rate() {
    let path = std::env::temp_dir().join(format!("openprocyon-commands-test-{}.jsonl", std::process::id()));
    // Record: frames of varying length, orders on zero-tick frames, two during a pause.
    let mut app = battle();
    let (me, _, enemy) = fleet(&mut app);
    let file = std::fs::File::create(&path).unwrap();
    let mut rec = Recorder { out: std::io::BufWriter::new(file), pending: None, frames: vec![], warned: false };
    let header = Header { format: FORMAT, version: "test".into(), seed: 1, args: vec![], mods: vec![], from_menu: false, skirmish: None };
    serde_json::to_writer(&mut rec.out, &header).unwrap();
    rec.out.write_all(b"\n").unwrap();
    app.insert_resource(rec);
    loaded(&mut app);
    let script = [17u64, 9, 45, 16, 70, 16, 3, 120, 33, 34, 10];
    let mut seen = vec![];
    for (i, ms) in script.iter().enumerate() {
        match i {
            1 => send(&mut app, Kind::Engine { ship: me, engine: Engine::Faster }),
            3 => send(&mut app, Kind::Target { ship: me, target: Some(enemy), spot: Some(Vec3::Y) }),
            5 => app.world_mut().resource_mut::<Time<Virtual>>().pause(),
            6 => send(&mut app, Kind::Engine { ship: me, engine: Engine::Faster }),
            7 => app.world_mut().resource_mut::<Time<Virtual>>().unpause(),
            8 => send(&mut app, Kind::Engine { ship: me, engine: Engine::Slower }),
            _ => {}
        }
        frame(&mut app, *ms);
        let c = app.world().resource::<Clock>();
        seen.push((c.frame, c.tick, order(&app, me), app.world().resource::<crate::combat::Target>().0.is_some()));
    }
    app.world_mut().write_message(AppExit::Success);
    app.update();
    drop(app);
    // Replay: the clock the test would give (16 ms frames, as `battle` leaves it) is replaced by the recording's.
    let (_, replay, unrecorded) = Replay::read(std::io::BufReader::new(std::fs::File::open(&path).unwrap())).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(unrecorded, 0);
    let mut app = battle();
    let (me, ..) = fleet(&mut app);
    app.insert_resource(replay).add_systems(Last, replay_clock);
    loaded(&mut app);
    let mut again = vec![];
    for _ in &script {
        send(&mut app, Kind::Engine { ship: me, engine: Engine::To(EngineOrder::Stop) }); // live input is ignored in a replay
        app.update(); // the replay sets each frame's clock
        let c = app.world().resource::<Clock>();
        again.push((c.frame, c.tick, order(&app, me), app.world().resource::<crate::combat::Target>().0.is_some()));
    }
    assert_eq!(again, seen);
}

/// An attack order against a ship of no player (allied with every player) is dropped; one against a hostile stands.
#[test]
fn an_attack_order_on_a_thing_of_no_player_is_dropped() {
    let mut app = battle();
    let (_, captain, enemy) = fleet(&mut app);
    let nobody = app.world_mut().spawn((Ship::new(0, crate::alliance::NEUTRAL), SpawnIndex(3))).id();
    loaded(&mut app);
    send(&mut app, Kind::Target { ship: captain, target: Some(nobody), spot: None });
    frame(&mut app, 40);
    assert_eq!(app.world().get::<crate::ship::FleetPilot>(captain).unwrap().target, None, "dropped");
    send(&mut app, Kind::Target { ship: captain, target: Some(enemy), spot: None });
    frame(&mut app, 40);
    assert_eq!(app.world().get::<crate::ship::FleetPilot>(captain).unwrap().target, Some(enemy));
}

/// A player's Attack, and "fire when possible", make the ship Aggressive (PROVISIONAL).
#[test]
fn an_attack_or_fire_when_possible_order_makes_the_ship_aggressive() {
    use crate::rules::Stance;
    let mut app = battle();
    let (_, captain, enemy) = fleet(&mut app);
    let stance = |app: &App| app.world().get::<crate::campaign::MissionShip>(captain).unwrap().stance;
    app.world_mut().entity_mut(captain).insert(crate::campaign::MissionShip { stance: Stance::Neutral, ..default() });
    loaded(&mut app);
    send(&mut app, Kind::Target { ship: captain, target: Some(enemy), spot: None });
    frame(&mut app, 40);
    assert_eq!(stance(&app), Stance::Aggressive);
    app.world_mut().get_mut::<crate::campaign::MissionShip>(captain).unwrap().stance = Stance::Defensive;
    let owner = app.world().resource::<crate::ship::Fleets>().local.clone();
    app.world_mut().resource_mut::<crate::ship::Fleets>().groups.get_mut(&owner).unwrap().cease_fire = true;
    send(&mut app, Kind::CeaseFire);
    frame(&mut app, 40);
    assert_eq!(stance(&app), Stance::Aggressive, "fire when possible");
}

/// `fleet_world` with a second human: player 1 commands the "Ally" fleet (`ids[3]`, `ids[4]`; `ids[4]` its command ship), player 0
/// (local) the "Player" fleet (`ids[0]` its command ship, `ids[1]`, `ids[2]`).
fn two_humans() -> (World, [Entity; 6]) {
    let (mut w, ids) = crate::ship::movement_tests::fleet_world();
    let player = |owner: &str| Player { owner: owner.into(), side: Side::Player, human: true };
    w.insert_resource(Players { list: [(PlayerId(0), player("Player")), (PlayerId(1), player("Ally"))].into(), local: PlayerId(0) });
    w.entity_mut(ids[3]).insert(crate::ship::FleetMember("Ally".into(), 1));
    w.entity_mut(ids[4]).insert(crate::ship::FleetMember("Ally".into(), 0));
    crate::ship::reconcile_fleets(&mut w);
    (w, ids)
}

fn commander(w: &mut World, player: u8) -> Vec<Entity> {
    w.query::<(Entity, &Commander)>().iter(w).filter(|(_, c)| c.0 == PlayerId(player)).map(|(e, _)| e).collect()
}

#[test]
#[ignore = "needs imported assets"]
fn each_human_has_a_command_ship_and_the_view_marks_only_the_local_ones() {
    let (mut w, ids) = two_humans();
    assert_eq!(commander(&mut w, 0), [ids[0]]);
    assert_eq!(commander(&mut w, 1), [ids[4]], "player 1's fleet gets its own command ship");
    let marked: Vec<Entity> = w.query_filtered::<Entity, With<crate::PlayerShip>>().iter(&w).collect();
    assert_eq!(marked, [ids[0]], "the view's marker stays on the local player's");
    assert_eq!(w.resource::<crate::ship::Fleets>().groups["Ally"].members, [ids[4], ids[3]]);
    // Player 1's command ship sinks: the next ship of its fleet takes over, player 0's stays.
    w.get_mut::<crate::combat::Health>(ids[4]).unwrap().destroyed = true;
    crate::ship::reconcile_fleets(&mut w);
    assert_eq!(commander(&mut w, 1), [ids[3]]);
    assert_eq!(commander(&mut w, 0), [ids[0]]);
    assert!(w.get::<crate::PlayerShip>(ids[3]).is_none());
}

#[test]
#[ignore = "needs imported assets"]
fn another_players_fleet_orders_and_p_leave_the_local_fleet_and_selection_alone() {
    let (mut w, ids) = two_humans();
    let selected = w.resource::<crate::ship::Fleets>().selected;
    apply(&mut w, PlayerId(1), &Kind::Promote { ship: ids[3] });
    assert_eq!(commander(&mut w, 1), [ids[3]], "P moves player 1's command ship");
    assert_eq!(commander(&mut w, 0), [ids[0]]);
    assert_eq!(w.resource::<crate::ship::Fleets>().selected, selected, "only the local player's selection follows a P");
    assert_eq!(w.resource::<crate::ship::Fleets>().roster(), &[ids[0], ids[1], ids[2]]);
    assert!(w.get::<crate::PlayerShip>(ids[3]).is_none() && w.get::<crate::PlayerShip>(ids[0]).is_some());
    apply(&mut w, PlayerId(1), &Kind::Promote { ship: ids[1] });
    assert_eq!(commander(&mut w, 0), [ids[0]], "a P naming another player's ship does nothing");
    apply(&mut w, PlayerId(1), &Kind::Formation { formation: crate::ship::Formation::Diamond, hold: true });
    let fleets = w.resource::<crate::ship::Fleets>();
    assert_eq!(fleets.groups["Ally"].formation, crate::ship::Formation::Diamond);
    assert_eq!(fleets.groups["Player"].formation, crate::ship::Formation::None, "the sender's fleet takes the formation");
    assert!(w.get::<crate::ship::FleetPilot>(ids[4]).unwrap().hold);
    assert!(!w.get::<crate::ship::FleetPilot>(ids[1]).unwrap().hold);
    apply(&mut w, PlayerId(1), &Kind::CeaseFire);
    assert!(w.resource::<crate::ship::Fleets>().groups["Ally"].cease_fire);
    assert!(!w.resource::<crate::ship::Fleets>().groups["Player"].cease_fire);
    // A player with no fleet orders nothing.
    apply(&mut w, PlayerId(7), &Kind::CeaseFire);
    assert!(!w.resource::<crate::ship::Fleets>().groups["Player"].cease_fire);
}

#[test]
#[ignore = "needs imported assets"]
fn in_single_player_the_command_ship_carries_both_marks() {
    let (mut w, ids) = crate::ship::movement_tests::fleet_world();
    let both = |w: &mut World| {
        let a: Vec<Entity> = w.query_filtered::<Entity, With<Commander>>().iter(w).collect();
        let b: Vec<Entity> = w.query_filtered::<Entity, With<crate::PlayerShip>>().iter(w).collect();
        assert_eq!(a, b);
        assert_eq!(a.len(), 1);
        a[0]
    };
    assert_eq!(both(&mut w), ids[0]);
    assert!(crate::ship::promote(&mut w, PlayerId(0), ids[2], false));
    assert_eq!(both(&mut w), ids[2]);
    w.get_mut::<crate::combat::Health>(ids[2]).unwrap().destroyed = true;
    crate::ship::reconcile_fleets(&mut w);
    assert_eq!(both(&mut w), ids[0], "the hand-over after a loss moves both");
    w.entity_mut(ids[0]).insert(crate::ship::FleetMember("Enemy".into(), 1));
    crate::ship::reconcile_fleets(&mut w);
    assert_eq!(both(&mut w), ids[1], "and so does the one after a capture");
}

#[test]
fn orders_are_the_local_players_and_a_tick_applies_them_by_player() {
    let mut app = battle();
    let (me, ..) = fleet(&mut app);
    let them = app.world_mut().spawn((Ship::new(0, Side::Enemy), SpawnIndex(3), Commander(PlayerId(1)))).id();
    app.world_mut().resource_mut::<Players>().local = PlayerId(1);
    app.world_mut().run_system_once(|mut orders: Orders| orders.send(Kind::Engine { ship: Entity::PLACEHOLDER, engine: Engine::Faster })).unwrap();
    assert_eq!(app.world().resource::<Outbox>().0[0].player, PlayerId(1), "an order is stamped with the local player");
    app.world_mut().resource_mut::<Outbox>().0.clear();
    loaded(&mut app);
    // Player 1 sent first, but player 0 goes first: Slow, then one step up.
    app.world_mut().resource_mut::<Outbox>().0.push(PlayerCommand { player: PlayerId(1), kind: Kind::Engine { ship: them, engine: Engine::Faster } });
    app.world_mut().resource_mut::<Outbox>().0.push(PlayerCommand { player: PlayerId(0), kind: Kind::Engine { ship: them, engine: Engine::To(EngineOrder::Slow) } });
    frame(&mut app, 40);
    assert_eq!(order(&app, them), EngineOrder::Full, "player 0's Slow, then player 1's step up");
    assert_eq!(order(&app, me), EngineOrder::Stop);
}
