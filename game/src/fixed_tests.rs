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
        scene: None,
        mounts: vec![],
        sections: vec![],
        core_hp: 100.0,
        phys: data::Physics { mass: 10.0, thrust: 30.0, max_speed: 10.0, rot_friction: 1.0, max_ang_acc: 0.2 },
        centre: Vec3::ZERO,
        radius: 1.0,
        hull_bound: 1.0,
        engage: 0.0,
        flag: None,
        engines: vec![],
        wake: vec![],
        explosion_fx: None,
        engine_snd: [None, None, None],
        ring: 1.0,
        sight: 100.0,
        repair_fx: None,
        cloakable: false,
        cloak_points: None,
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
        net: 1.0,
        avoid: 1.0,
    };
    let mut app = clock();
    app.insert_resource(Library { classes: vec![class], guns: default(), bullets: default(), names: default(), clips: default() })
        .add_systems(FixedUpdate, sail.in_set(BattleSet::Simulate));
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

fn frame_micros(app: &mut App, us: u64) {
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_micros(us)));
    app.update();
}

/// What a system running in `FixedUpdate` saw of the ship's `Transform`, and the pose the tick left it in.
#[derive(Resource, Default)]
struct TickView {
    start: Vec<Transform>,
    end: Vec<Transform>,
}

fn see_start(q: Query<&Transform, With<Ship>>, mut view: ResMut<TickView>) {
    view.start.push(*q.single().unwrap());
}

fn see_end(q: Query<&Transform, With<Ship>>, mut view: ResMut<TickView>) {
    view.end.push(*q.single().unwrap());
}

fn interpolated_ship() -> (App, Entity) {
    let (mut app, e) = moving_ship();
    app.add_plugins(interpolate::plugin).init_resource::<TickView>().add_systems(FixedUpdate, (see_start.in_set(BattleSet::Commands), see_end.after(BattleSet::Simulate)));
    (app, e)
}

#[test]
fn two_frames_between_ticks_draw_the_pose_half_way() {
    let (mut app, e) = interpolated_ship();
    frame(&mut app, 33); // tick 1 lands exactly on the tick boundary
    let (p0, p1) = {
        let view = app.world().resource::<TickView>();
        (view.start[0], view.end[0])
    };
    assert!((p1.translation - p0.translation).length() > 0.01, "the ship must move during a tick");
    assert_eq!(app.world().get::<Transform>(e).unwrap().translation, p0.translation, "overstep 0 draws the pose the tick started from");
    frame_micros(&mut app, 8_250);
    frame_micros(&mut app, 8_250); // two frames, a quarter of a tick each
    let drawn = *app.world().get::<Transform>(e).unwrap();
    assert_eq!(app.world().resource::<TickView>().end.len(), 1, "no tick ran between the frames");
    let half = interpolate::blend(&p0, &p1, 0.5);
    assert!((drawn.translation - half.translation).length() < 1e-4, "{:?} vs {:?}", drawn.translation, half.translation);
    assert!(drawn.rotation.angle_between(half.rotation) < 1e-4);
    assert_ne!(drawn.translation, p1.translation);
    // The state under the drawn pose is the simulation's, untouched.
    let it = app.world().get::<interpolate::Interp>(e).unwrap();
    assert_eq!(it.sim_pose(&drawn), p1);
}

#[test]
fn fixed_systems_see_the_tick_pose_and_ticks_without_frames_in_between_are_unchanged() {
    let (mut app, e) = interpolated_ship();
    let (mut reference, r) = moving_ship();
    reference.init_resource::<TickView>().add_systems(FixedUpdate, (see_start.in_set(BattleSet::Commands), see_end.after(BattleSet::Simulate)));
    frame(&mut app, 33);
    frame_micros(&mut app, 16_500); // drawn half-way between two simulation poses
    frame(&mut app, 99); // 16.5 ms overstep + 99 ms: three ticks, none with a frame between them
    frame(&mut reference, 33);
    frame_micros(&mut reference, 16_500);
    frame(&mut reference, 99);
    let (view, base) = (app.world().resource::<TickView>(), reference.world().resource::<TickView>());
    assert_eq!(view.start.len(), 4);
    assert_eq!(view.start, base.start, "every tick starts from the simulation's pose, never the interpolated one");
    assert_eq!(view.end, base.end, "and ends in the pose the simulation alone would have produced");
    for i in 1..view.start.len() {
        assert_eq!(view.start[i], view.end[i - 1], "a tick starts where the previous one ended");
    }
    let drawn = *app.world().get::<Transform>(e).unwrap();
    let it = app.world().get::<interpolate::Interp>(e).unwrap();
    assert_eq!(it.sim_pose(&drawn), *reference.world().get::<Transform>(r).unwrap(), "the simulation state is bit-identical");
    assert!(app.world().get::<Ship>(e).unwrap() == reference.world().get::<Ship>(r).unwrap());
}

#[test]
fn a_paused_clock_holds_the_last_drawn_pose() {
    let (mut app, e) = interpolated_ship();
    frame(&mut app, 33);
    frame_micros(&mut app, 16_500);
    let held = *app.world().get::<Transform>(e).unwrap();
    app.world_mut().resource_mut::<Time<Virtual>>().pause();
    for _ in 0..5 {
        frame(&mut app, 16);
        assert_eq!(*app.world().get::<Transform>(e).unwrap(), held);
    }
}

#[test]
fn a_write_from_a_frame_system_is_a_teleport_not_a_glide() {
    let (mut app, e) = interpolated_ship();
    frame(&mut app, 33);
    frame_micros(&mut app, 16_500);
    let far = Vec3::new(500.0, 0.0, -500.0);
    app.world_mut().get_mut::<Transform>(e).unwrap().translation = far;
    frame_micros(&mut app, 1_000);
    let drawn = app.world().get::<Transform>(e).unwrap().translation;
    assert!(drawn.distance(far) < 1.0, "drawn {drawn:?}, expected near {far:?}");
}

/// A child of the ship (a gun muzzle) as seen on the tick: `GlobalTransform` is propagated per frame in `PostUpdate`, from the
/// drawn pose, so only the tick-start propagation keeps what gunnery reads on the simulation's pose.
#[derive(Component)]
struct Muzzle;
#[derive(Resource, Default)]
struct MuzzleView(Vec<Vec3>);

fn see_muzzle(q: Query<&GlobalTransform, With<Muzzle>>, mut view: ResMut<MuzzleView>) {
    view.0.push(q.single().unwrap().translation());
}

#[test]
fn a_childs_global_transform_on_the_tick_follows_the_simulation_pose() {
    let offset = Vec3::new(0.5, 1.5, -0.25);
    let (mut app, ship) = interpolated_ship();
    app.add_plugins(bevy::transform::TransformPlugin).init_resource::<MuzzleView>().add_systems(FixedUpdate, see_muzzle.in_set(BattleSet::Commands).after(see_start));
    app.world_mut().spawn((Muzzle, Transform::from_translation(offset), ChildOf(ship)));
    frame(&mut app, 33);
    frame_micros(&mut app, 16_500); // drawn half-way: PostUpdate propagates the drawn pose
    frame_micros(&mut app, 16_500); // tick
    frame(&mut app, 99); // three more ticks in one frame
    let (view, muzzles) = (app.world().resource::<TickView>(), app.world().resource::<MuzzleView>());
    assert_eq!(muzzles.0.len(), 5);
    assert_eq!(view.start.len(), 5);
    let moved = view.start.iter().map(|t| t.translation).collect::<Vec<_>>();
    assert!(moved.windows(2).all(|w| w[0] != w[1]), "the ship moves between ticks");
    for (i, (pose, seen)) in view.start.iter().zip(&muzzles.0).enumerate() {
        let want = pose.transform_point(offset);
        assert!(seen.distance(want) < 1e-4, "tick {i}: the muzzle was at {seen:?}, the simulation pose puts it at {want:?}");
    }
}
