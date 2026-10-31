//! Asset-free regressions for the production battle clock and schedule boundary.
use super::*;
use bevy::time::TimeUpdateStrategy;
use std::time::Duration;

fn clock() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(battle_schedule).insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    app.update();
    app
}

fn frame(app: &mut App, ms: u64) {
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(ms)));
    app.update();
}

#[derive(Resource, Default)]
struct Ticks(Vec<Duration>);

fn count_ticks(time: Res<Time>, mut ticks: ResMut<Ticks>) {
    ticks.0.push(time.delta());
}

#[test]
fn fixed_clock_preserves_pause_speed_and_catch_up_caps() {
    let mut app = clock();
    app.init_resource::<Ticks>().add_systems(FixedUpdate, count_ticks);
    frame(&mut app, 100);
    assert_eq!(app.world().resource::<Ticks>().0, vec![BATTLE_TICK; 3]);
    app.world_mut().resource_mut::<Time<Virtual>>().pause();
    frame(&mut app, 100);
    assert_eq!(app.world().resource::<Ticks>().0.len(), 3);
    app.world_mut().resource_mut::<Time<Virtual>>().unpause();
    set_time_scale(&mut app.world_mut().resource_mut::<Time<Virtual>>(), 8.0);
    frame(&mut app, 5); // 40 ms virtual, not eight input polls.
    assert_eq!(app.world().resource::<Ticks>().0.len(), 4);
    frame(&mut app, 1000); // Existing fast-forward catch-up limit: 100 ms virtual.
    let time = app.world().resource::<Time<Virtual>>();
    assert_eq!(time.delta(), time.max_delta());
    assert!((time.delta_secs() - 0.1).abs() < 1e-6);
    assert_eq!(app.world().resource::<Ticks>().0.len(), 7);
    assert!(app.world().resource::<Ticks>().0.iter().all(|dt| *dt == BATTLE_TICK));
}

#[derive(Message)]
struct InputCommand;
#[derive(Message)]
struct TickOutput(usize);
#[derive(Resource, Default)]
struct Boundary {
    send: bool,
    commands: usize,
    ticks: usize,
    presented: Vec<usize>,
}

fn capture(mut state: ResMut<Boundary>, mut commands: MessageWriter<InputCommand>) {
    if std::mem::take(&mut state.send) {
        commands.write(InputCommand);
    }
}

fn consume(mut state: ResMut<Boundary>, mut commands: MessageReader<InputCommand>, mut output: MessageWriter<TickOutput>) {
    state.commands += commands.read().count();
    state.ticks += 1;
    output.write(TickOutput(state.ticks));
}

fn present(mut state: ResMut<Boundary>, mut output: MessageReader<TickOutput>) {
    state.presented.extend(output.read().map(|m| m.0));
}

#[test]
fn messages_survive_zero_tick_frames_and_are_not_replayed_in_catch_up() {
    let mut app = clock();
    app.init_resource::<Boundary>()
        .add_message::<InputCommand>()
        .add_message::<TickOutput>()
        .add_systems(Update, (capture, present).chain())
        .add_systems(FixedUpdate, consume.in_set(BattleSet::Commands));
    app.world_mut().resource_mut::<Boundary>().send = true;
    for _ in 0..8 {
        frame(&mut app, 1);
    }
    assert_eq!(app.world().resource::<Boundary>().commands, 0);
    frame(&mut app, 100);
    let state = app.world().resource::<Boundary>();
    assert_eq!(state.commands, 1, "one frame command consumed once across three fixed ticks");
    assert_eq!(state.presented, [1, 2, 3], "every fixed output reaches the frame reader");
    for _ in 0..8 {
        frame(&mut app, 1);
    }
    frame(&mut app, 33);
    let state = app.world().resource::<Boundary>();
    assert_eq!(state.commands, 1, "old commands are never replayed");
    assert_eq!(state.presented, [1, 2, 3, 4], "old outputs are never replayed");
}

fn moving_ship() -> (App, Entity) {
    // Hand-authored test physics, not a copy of any imported ship.
    let class = data::Class {
        name: "Synthetic".into(),
        race: String::new(),
        skirmish: false,
        glb: String::new(),
        mounts: vec![],
        sections: vec![],
        core_hp: 100.0,
        phys: data::Physics { mass: 10.0, thrust: 30.0, max_speed: 10.0, rot_friction: 1.0, max_ang_acc: 0.2 },
        centre: Vec3::ZERO,
        radius: 1.0,
        hull_bound: 1.0,
        engage: 0.0,
        reach: 0.0,
        flag: None,
        engines: vec![],
        explosion_fx: None,
        engine_snd: [None, None, None],
        ring: 1.0,
        sight: 100.0,
        repair_fx: None,
        cloakable: false,
        points: 0,
        crew_posts: 0,
        display_name: None,
        icon: None,
        length: 2.0,
        collision_bounds: [Vec3::splat(-1.0), Vec3::splat(1.0)],
        mass_centre: Vec3::ZERO,
        lifeboats: 0,
        is_lifeboat: false,
        sphere: (Vec3::ZERO, 1.0),
        tow_points: [Vec3::ZERO; 2],
        homing_point: Vec3::ZERO,
    };
    let ship = Ship {
        class: 0,
        side: Side::Player,
        heading: 0.0,
        speed: 2.0,
        ang_vel: 0.0,
        order: 2,
        waypoints: vec![],
        goal: Some(0.5),
        cap: 1.0,
        throttle: None,
        sail_energy: 1.0,
        crew_speed: 1.0,
        crew_turn: 1.0,
        avoid: 1.0,
    };
    let mut app = clock();
    app.insert_resource(Library { classes: vec![class], guns: default(), bullets: default(), names: default() }).add_systems(FixedUpdate, sail.in_set(BattleSet::Simulate));
    let e = app.world_mut().spawn((ship, Health { sections: vec![], core: 100.0, destroyed: false }, Transform::default(), collision::Drift::default(), PlayerShip)).id();
    (app, e)
}

#[test]
fn ship_motion_is_identical_for_one_large_frame_and_ten_small_frames() {
    let (mut large, a) = moving_ship();
    let (mut small, b) = moving_ship();
    frame(&mut large, 100);
    for _ in 0..10 {
        frame(&mut small, 10);
    }
    let a_tf = large.world().get::<Transform>(a).unwrap();
    let b_tf = small.world().get::<Transform>(b).unwrap();
    assert!(a_tf.translation.length() > 0.1, "exercise real movement, not an idle ship");
    assert_eq!(a_tf, b_tf);
    assert!(large.world().get::<Ship>(a).unwrap() == small.world().get::<Ship>(b).unwrap());
    assert_eq!(large.world().resource::<Time<Fixed>>().elapsed(), BATTLE_TICK * 3);
}

/// A computer ship heading at 0 (towards -z) with a stopped ship of `side` 15 units ahead; returns the computer's goal heading
/// after one frame of `sail`.
fn goal_with_a_ship_ahead(side: Side) -> f32 {
    let (mut app, e) = moving_ship();
    let mut ship = app.world().get::<Ship>(e).unwrap().clone();
    app.world_mut().entity_mut(e).remove::<PlayerShip>().insert(Ship { side: Side(1), goal: Some(0.0), ..ship.clone() });
    ship.side = side;
    ship.speed = 0.0;
    ship.order = 0;
    ship.goal = None;
    app.world_mut().spawn((ship, Health { sections: vec![], core: 100.0, destroyed: false }, Transform::from_xyz(0.0, 0.0, -15.0), collision::Drift::default()));
    frame(&mut app, 34);
    app.world().get::<Ship>(e).unwrap().goal.unwrap()
}

#[test]
fn computer_ships_steer_around_each_other_but_not_around_the_humans_ships() {
    // Issue #191 / owner's play of the original: computer ships never bump into each other, but do bump into the human's ships.
    assert_ne!(goal_with_a_ship_ahead(Side(2)), 0.0, "a hostile computer ship ahead is steered around");
    assert_eq!(goal_with_a_ship_ahead(Side::Player), 0.0, "the human's ship is not");
}

#[test]
fn a_computer_ship_pressed_against_the_humans_ship_steers_off_after_three_seconds() {
    // Issue #191: an allied computer ship rammed the human's parked Tug and pushed against it for 230 s.
    let (mut app, e) = moving_ship();
    let mut ship = app.world().get::<Ship>(e).unwrap().clone();
    app.world_mut().entity_mut(e).remove::<PlayerShip>().insert(Ship { side: Side(1), goal: Some(0.0), ..ship.clone() });
    (ship.speed, ship.order, ship.goal) = (0.0, 0, None);
    // Hand-authored stand-in for a parked hull: pinned, as the Tug held its station against the push.
    app.world_mut().spawn((ship, Health { sections: vec![], core: 100.0, destroyed: false }, Transform::from_xyz(0.0, 0.0, -1.9), collision::Drift::default(), collision::Pinned));
    let goal = |app: &App| app.world().get::<Ship>(e).unwrap().goal.unwrap();
    for _ in 0..25 {
        frame(&mut app, 100);
        assert_eq!(goal(&app), 0.0, "bumps into the human's ship first");
        app.world_mut().get_mut::<Ship>(e).unwrap().goal = Some(0.0); // the captain keeps aiming through it
    }
    for _ in 0..10 {
        frame(&mut app, 100);
    }
    assert_ne!(goal(&app), 0.0, "then steers around it");
}
