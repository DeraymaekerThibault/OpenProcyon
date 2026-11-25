use super::*;
use crate::Side;
use crate::scenario::SpawnIndex;
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
    let me = w.spawn((Ship::new(0, Side::Player), SpawnIndex(0))).id();
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

#[test]
fn a_command_keeps_its_meaning_through_the_file() {
    let mut app = battle();
    let (me, _, enemy) = fleet(&mut app);
    let kinds = [
        Kind::Engine { ship: me, engine: Engine::Faster },
        Kind::Engine { ship: me, engine: Engine::To(EngineOrder::Emergency) },
        Kind::Target { ship: me, target: Some(enemy), spot: Some(Vec3::new(0.25, -1.5, 3.0)) },
        Kind::Target { ship: me, target: None, spot: None },
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
    assert_eq!(replay.commands, vec![(At::Frame(2), Saved::Engine { ship: NetId { kind: NetKind::Ship, index: 0 }, engine: SavedEngine::Slower })]);
    assert!(Replay::read(header.replace("\"format\":1", "\"format\":2").as_bytes()).is_err(), "another format is refused");
}

#[test]
fn the_recorded_arguments_are_the_ones_that_decide_the_battle() {
    let args: Vec<String> =
        ["game", "--lobby", "map=x", "--speed", "16", "--script", "commands", "--fixed-step", "--seed", "3", "--replay", "f.jsonl"].iter().map(|s| s.to_string()).collect();
    assert_eq!(scenario_args(&args), ["--lobby", "map=x", "--seed", "3"]);
    let header = Header { format: FORMAT, version: "x".into(), seed: 3, args: scenario_args(&args), mods: vec![], from_menu: false };
    assert_eq!(mismatch(&header, &args, 3, &[]), None);
    assert!(mismatch(&header, &args, 4, &[]).unwrap().contains("--seed 3"));
    assert!(mismatch(&header, &["game".to_string(), "--map".to_string(), "y".to_string()], 3, &[]).is_some());
    assert!(mismatch(&header, &args, 3, &["remaster".to_string()]).is_some());
    let menu = Header { args: vec![], from_menu: true, ..header };
    assert!(mismatch(&menu, &["game".to_string()], 3, &[]).unwrap().contains("menus"), "a battle set up in the menus is refused, not replayed wrong");
    assert!(sets_up_battle(&scenario_args(&args)) && !sets_up_battle(&[]));
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
    let mut rec = Recorder { out: std::io::BufWriter::new(std::fs::File::create(&path).unwrap()), frames: vec![], warned: false };
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
    let mut rec = Recorder { out: std::io::BufWriter::new(file), frames: vec![], warned: false };
    let header = Header { format: FORMAT, version: "test".into(), seed: 1, args: vec![], mods: vec![], from_menu: false };
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
