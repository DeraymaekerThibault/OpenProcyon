//! ECS side of the developer console (rules/console.rs): the `~` key and typing, running submitted lines against the mission's
//! script and applying their effects, and the cheat mark. The drawing is the view's (`classic/console.rs`).

use bevy::input::keyboard::KeyboardInput;

use super::*;
use crate::menu::Screen;
use crate::rules::console::{Cheats, Command, Console, DevView, Edit, Env, ShipInfo, marks_path, parse};

/// Whether the console is on without the Display option "Developer Console": debug builds, `--console` or `PROCYON_CONSOLE` in the
/// environment. A release build otherwise leaves it to the option (off by default), so a stray `~` never hands a player the cheats.
pub fn enabled() -> bool {
    cfg!(debug_assertions) || std::env::args().any(|a| a == "--console") || std::env::var_os("PROCYON_CONSOLE").is_some()
}

pub(super) fn plugin(app: &mut App) {
    app.insert_resource(Console::new(enabled()))
        .init_resource::<Cheats>()
        .init_resource::<DevView>()
        .init_resource::<crate::scenario::RestartRequest>()
        .add_systems(OnEnter(Screen::Battle), |mut cheats: ResMut<Cheats>| *cheats = Cheats::default())
        .add_systems(OnExit(Screen::Battle), (close_on_leaving, |mut dev: ResMut<DevView>| dev.reset()))
        // After the input systems and before anything reads the keys: while the console is open the game sees no keyboard or mouse.
        .add_systems(PreUpdate, keys.after(bevy::input::InputSystems).run_if(in_state(Screen::Battle)).run_if(resource_exists::<Mission>).run_if(crate::menu::overlay_closed))
        .add_systems(Update, free_camera.after(crate::camera).run_if(in_state(Screen::Battle)))
        .add_systems(Update, run.after(update).run_if(in_state(Screen::Battle)).run_if(resource_exists::<Mission>));
}

fn close_on_leaving(mut console: ResMut<Console>, mut vtime: ResMut<Time<Virtual>>) {
    close(&mut console, &mut vtime);
}

fn close(console: &mut Console, vtime: &mut Time<Virtual>) {
    console.open = false;
    console.line.clear();
    if std::mem::take(&mut console.paused_by_us) {
        vtime.unpause();
    }
}

/// The console key (`Act::Console`, the backquote key by default; F12 always works) opens the console and pauses the battle; Escape or the console key closes it. While it is open typing goes to its line and the
/// keyboard and mouse state the game reads is cleared.
fn keys(
    mut input: MessageReader<KeyboardInput>,
    mut console: ResMut<Console>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut vtime: ResMut<Time<Virtual>>,
    binds: Option<Res<crate::bindings::Bindings>>,
    mut wheel: ResMut<bevy::input::mouse::AccumulatedMouseScroll>,
    mut ctrl: Local<bool>,
) {
    if !console.available() {
        return;
    }
    let bound = binds.map_or(KeyCode::Backquote, |b| b.key(crate::bindings::Act::Console));
    for k in input.read() {
        if matches!(k.key_code, KeyCode::ControlLeft | KeyCode::ControlRight) {
            *ctrl = k.state.is_pressed();
        }
        if !k.state.is_pressed() {
            continue;
        }
        let toggle = (k.key_code == bound || k.key_code == KeyCode::F12) && !k.repeat;
        if !console.open {
            if toggle {
                console.open = true;
                if !vtime.is_paused() {
                    vtime.pause();
                    console.paused_by_us = true;
                }
                if console.log.is_empty() {
                    console.say("Developer console. `help` lists the commands; Escape or the console key closes it.");
                }
            }
            continue;
        }
        if toggle {
            close(&mut console, &mut vtime);
            break;
        }
        match console.key_with(&k.logical_key, *ctrl) {
            Edit::Submit(line) => console.pending.push(line),
            Edit::Close => close(&mut console, &mut vtime),
            Edit::None => {}
        }
        if !console.open {
            break;
        }
    }
    if console.open {
        // The wheel reads back through the output, and the camera does not zoom with it.
        if wheel.delta.y != 0.0 {
            let notches = if wheel.unit == bevy::input::mouse::MouseScrollUnit::Line { wheel.delta.y } else { wheel.delta.y / 20.0 };
            let lines = (notches * crate::rules::console::WHEEL_LINES as f32).round() as isize;
            console.scroll_by(if lines == 0 { wheel.delta.y.signum() as isize } else { lines });
            wheel.delta = Vec2::ZERO;
        }
        keyboard.reset_all();
        mouse.reset_all();
    }
}

/// Runs the lines submitted since the last frame: each becomes a rule action or condition on the mission's script, and its effects
/// are applied as a rule's would be.
fn run(
    mut console: ResMut<Console>,
    mut dev: ResMut<DevView>,
    mut restart: ResMut<crate::scenario::RestartRequest>,
    mission: Option<ResMut<Mission>>,
    target: Res<Target>,
    lib: Res<Library>,
    mut orbit: ResMut<Orbit>,
    rocks: Query<(Entity, &Transform, &Asteroid), (Without<Ship>, Without<crate::environment::SpaceAnimal>)>,
    mut animals: Query<(Entity, &mut crate::environment::SpaceAnimal, &mut Transform, &mut Visibility), Without<Ship>>,
    mut commands: Commands,
    mut ships: Query<RuleShip>,
    mut extra: Extra,
) {
    let Some(mut mission) = mission else { return };
    if console.pending.is_empty() && !console.open {
        return;
    }
    let mission = &mut *mission;
    let lines = std::mem::take(&mut console.pending);
    // `Extra` holds the read side of `Cheats` (the points gate in `update`), so changes go through the command queue.
    let mut god = extra.cheats.as_ref().is_some_and(|c| c.god);
    // The unit list as `update` builds it each frame.
    let mut world = units(&ships, &lib);
    world.extend(animals.iter().map(|(e, a, tf, _)| a.unit(e, tf)));
    world.extend(extra.bases.iter_mut().map(|(e, b, tf, armed)| b.unit(e, tf, armed.map_or(0.0, |(a, h)| crate::combat::base_mission_damage(&lib.classes[a.class], h)))));
    world.extend(rocks.iter().map(|(e, tf, a)| a.unit(e, tf)));
    mission.script.tag(&mut world);
    if console.open {
        console.names = mission.script.console_names(&world);
    }
    for line in lines {
        console.say(format!("> {line}"));
        let player = mission.script.player().to_string();
        let cmd = match parse(&line, &player, god) {
            Ok(c) => c,
            Err(e) => {
                console.say(e);
                continue;
            }
        };
        let mut fx = vec![];
        let env = Env {
            target: target.0,
            mission: mission.id.clone(),
            marks: marks_path(),
            ships: ships
                .iter()
                .map(|s| ShipInfo {
                    e: Some(s.entity),
                    stance: format!("{:?}", s.mission.stance),
                    order: s.ship.order,
                    waypoints: s.ship.waypoints.len(),
                    attack: s.mission.attack.clone(),
                    human: s.mission.human || s.primary,
                    ram: s.mission.ram,
                    speed: s.ship.speed,
                })
                .collect(),
        };
        let outcome = mission.script.console_run_with(&cmd, &mut world, &env, &mut fx);
        if outcome.rejected {
            for l in &outcome.lines {
                console.say(l);
            }
            continue;
        }
        if cmd.cheats() {
            commands.queue(|w: &mut World| w.resource_mut::<Cheats>().used = true);
        }
        if let Command::Acts { god: Some(on), .. } = &cmd {
            god = *on;
            let on = *on;
            commands.queue(move |w: &mut World| w.resource_mut::<Cheats>().god = on);
        }
        for l in &outcome.lines {
            console.say(l);
        }
        if outcome.skip_voice {
            for e in &extra.voices {
                commands.entity(e).despawn();
            }
        }
        restart.0 |= outcome.restart;
        for c in &outcome.view {
            dev.apply(c);
        }
        each_ship(&mut ships, outcome.repair, |s| crate::docking::repair(&lib, &s.ship, &mut s.health, true));
        apply_fx(fx, mission, &mut commands, &mut ships, &mut animals, &mut extra, &mut orbit, &lib);
    }
}

/// `cam free`: arrows fly the camera along its view, PageUp / PageDown rise and sink, dragging with the right mouse button looks,
/// Shift is five times faster. It runs on real time, so it also works with the battle paused.
fn free_camera(
    mut dev: ResMut<DevView>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<bevy::input::mouse::AccumulatedMouseMotion>,
    time: Res<Time<Real>>,
    mut cam: Single<&mut Transform, (With<Camera3d>, With<crate::MainCamera>)>,
    mut angles: Local<(f32, f32)>,
) {
    if !dev.free_cam {
        return;
    }
    if dev.free_fresh {
        dev.free_fresh = false;
        let (yaw, pitch, _) = cam.rotation.to_euler(EulerRot::YXZ);
        *angles = (yaw, pitch);
    }
    if mouse.pressed(MouseButton::Right) {
        angles.0 -= motion.delta.x * 0.003;
        angles.1 = (angles.1 - motion.delta.y * 0.003).clamp(-1.5, 1.5);
    }
    cam.rotation = Quat::from_euler(EulerRot::YXZ, angles.0, angles.1, 0.0);
    let axis = |plus: KeyCode, minus: KeyCode| f32::from(keys.pressed(plus)) - f32::from(keys.pressed(minus));
    let speed = 80.0 * if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) { 5.0 } else { 1.0 };
    let step = (*cam.forward() * axis(KeyCode::ArrowUp, KeyCode::ArrowDown)
        + *cam.right() * axis(KeyCode::ArrowRight, KeyCode::ArrowLeft)
        + Vec3::Y * axis(KeyCode::PageUp, KeyCode::PageDown))
        * speed
        * time.delta_secs();
    cam.translation += step;
}
