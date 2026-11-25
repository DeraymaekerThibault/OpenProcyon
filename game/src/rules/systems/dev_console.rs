//! ECS side of the developer console (rules/console.rs): the `~` key and typing, running submitted lines against the mission's
//! script and applying their effects, and the cheat mark. The drawing is the view's (`classic/console.rs`).

use bevy::input::keyboard::KeyboardInput;

use super::*;
use crate::menu::Screen;
use crate::rules::console::{Caps, Cheats, Command, Console, DevView, Edit, Env, FrameStats, Needs, ShipInfo, ViewChange, marks_path, parse};
use crate::rules::skirmish::SkirmishScript;

/// Whether the console is on without the Display option "Developer Console": debug builds, `--console` or `PROCYON_CONSOLE` in the
/// environment. A release build otherwise leaves it to the option (off by default), so a stray `~` never hands a player the cheats.
pub fn enabled() -> bool {
    cfg!(debug_assertions) || std::env::args().any(|a| a == "--console") || std::env::var_os("PROCYON_CONSOLE").is_some()
}

/// The rules script the console works on in a battle that runs none (a skirmish map without a `script.json`, a lobby battle): empty, so the
/// commands that only need the battle's ships still have an interpreter to run through. Made on first use, dropped when a battle starts.
#[derive(Resource, Default)]
pub(super) struct BareScript(Option<Script>);

pub(super) fn plugin(app: &mut App) {
    app.insert_resource(Console::new(enabled()))
        .init_resource::<Cheats>()
        .init_resource::<DevView>()
        .init_resource::<BareScript>()
        .init_resource::<crate::scenario::RestartRequest>()
        .add_systems(OnEnter(Screen::Battle), (|mut cheats: ResMut<Cheats>| *cheats = Cheats::default(), |mut bare: ResMut<BareScript>| bare.0 = None))
        .add_systems(OnExit(Screen::Battle), (close_on_leaving, |mut dev: ResMut<DevView>| dev.reset()))
        // After the input systems and before anything reads the keys: while the console is open the game sees no keyboard or mouse.
        .add_systems(PreUpdate, keys.after(bevy::input::InputSystems).run_if(in_state(Screen::Battle)).run_if(crate::menu::overlay_closed))
        .init_resource::<crate::rules::console::FrameStats>()
        .add_systems(Last, frame_stats)
        .add_systems(Update, free_camera.after(crate::camera).run_if(in_state(Screen::Battle)))
        // One dispatch for every singleplayer battle: what a command needs decides whether it runs (`Command::needs`), not which rules resource exists.
        .add_systems(Update, (attach_ship_state, run).chain().after(update).run_if(in_state(Screen::Battle)));
}

/// Feeds the frame-rate counter from the real-time clock while it is shown.
fn frame_stats(time: Res<Time<Real>>, mut stats: ResMut<FrameStats>) {
    if crate::menu::fps_setting() {
        stats.push(time.elapsed_secs_f64(), time.delta_secs_f64());
    } else {
        stats.clear();
    }
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
    let was_open = console.open;
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
    // The closing frame is cleared too: this Escape or console key must not reach the game (Escape would also open the in-game menu,
    // whose next click then closes it, so the first click after closing the console seemed to do nothing).
    if was_open && !console.open {
        keyboard.reset_all();
        mouse.reset_all();
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

/// A battle whose ships carry no `MissionShip` (a skirmish map without a script, a lobby battle) gets one on each ship the first time a
/// command needs them: the rule actions (`god`, `tp`, `kill`...) and the unit list read and change a ship through it. The ship is named
/// after its banner and owned by its fleet, as a scripted skirmish's are; nothing else about it changes. PROVISIONAL: some systems read
/// `MissionShip` when it exists (owners in the statistics, alliances), so a battle in which the console ran such a command counts its
/// ships by fleet owner from then on, which is already how the scripted skirmishes work.
pub(super) fn attach_ship_state(
    console: Res<Console>,
    ships: Query<(Entity, &Ship, Option<&crate::ship::FleetMember>, Option<&crate::ShipName>), (Without<MissionShip>, Without<crate::lifeboats::Lifeboat>)>,
    lib: Res<Library>,
    mut commands: Commands,
) {
    if console.pending.is_empty() || ships.is_empty() || !console.pending.iter().any(|l| parse(l, "", false).is_ok_and(|c| c.needs() >= Needs::World)) {
        return;
    }
    for (e, ship, member, name) in &ships {
        let owner = member.map_or_else(|| if ship.side == crate::Side::Player { "player".to_string() } else { "enemy".to_string() }, |m| m.0.to_string());
        let name = name.map_or_else(|| lib.classes[ship.class].name.clone(), |n| n.0.clone());
        commands.entity(e).insert(MissionShip { id: -1, name, owner: owner.as_str().into(), visible: true, ..default() });
    }
}

/// Runs the lines submitted since the last frame: each becomes a rule action or condition on the battle's script (the mission's, the
/// skirmish map's, or an empty one), and its effects are applied as a rule's would be. A command the battle cannot serve is answered
/// with what it needs (`Command::refusal`).
pub(super) fn run(
    mut console: ResMut<Console>,
    mut dev: ResMut<DevView>,
    mut restart: ResMut<crate::scenario::RestartRequest>,
    mut mission: Option<ResMut<Mission>>,
    mut skirmish: Option<ResMut<SkirmishScript>>,
    mut bare: ResMut<BareScript>,
    fleets: Option<Res<crate::ship::Fleets>>,
    target: Res<Target>,
    lib: Res<Library>,
    mut orbit: ResMut<Orbit>,
    rocks: Query<(Entity, &Transform, &Asteroid), (Without<Ship>, Without<crate::environment::SpaceAnimal>)>,
    mut animals: Query<(Entity, &mut crate::environment::SpaceAnimal, &mut Transform, &mut Visibility), Without<Ship>>,
    mut commands: Commands,
    mut ships: Query<RuleShip>,
    mut extra: Extra,
    mut settings: Option<ResMut<crate::menu::Settings>>,
) {
    if console.pending.is_empty() && !console.open {
        return;
    }
    // The script the battle runs, with what applying its effects needs: a campaign mission, a skirmish map's script, or neither.
    let in_mission = mission.is_some();
    let (null, no_sides) = (serde_json::Value::Null, std::collections::HashMap::new());
    let (script, paths, sides, caps, mission_id): (&mut Script, &serde_json::Value, _, _, _) = if let Some(m) = mission.as_deref_mut() {
        let Mission { script, data, id, .. } = m;
        (script, &data["paths"], None, Caps::MISSION, id.clone())
    } else if let Some(sk) = skirmish.as_deref_mut() {
        let SkirmishScript { script, paths, sides, .. } = sk;
        (script, &*paths, Some(&*sides), Caps::SKIRMISH_SCRIPT, String::new())
    } else {
        (bare.0.get_or_insert_with(|| Script::new(&serde_json::json!({"rules": []}), serde_json::Value::Null)), &null, Some(&no_sides), Caps::BARE, String::new())
    };
    let lines = std::mem::take(&mut console.pending);
    // `Extra` holds the read side of `Cheats` (the points gate in `update`), so changes go through the command queue.
    let mut god = extra.cheats.as_ref().is_some_and(|c| c.god);
    // The unit list as `update` builds it each frame.
    let mut world = units(&ships, &lib);
    world.extend(animals.iter().map(|(e, a, tf, _)| a.unit(e, tf)));
    world.extend(extra.bases.iter_mut().map(|(e, b, tf, armed)| super::base_unit(&b, e, tf, armed, &lib)));
    world.extend(rocks.iter().map(|(e, tf, a)| a.unit(e, tf)));
    script.tag(&mut world);
    if console.open {
        console.names = script.console_names(&world);
    }
    // The rules address the human's fleet by its owner name: the script's player in a mission, else the local fleet's owner.
    let player = match fleets.as_deref().map(|f| f.local.to_string()).filter(|o| !o.is_empty() && !in_mission) {
        Some(owner) => owner,
        None => script.player().to_string(),
    };
    for line in lines {
        console.say(format!("> {line}"));
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
            mission: mission_id.clone(),
            marks: marks_path(),
            word: line.split_whitespace().next().unwrap_or_default().to_lowercase(),
            caps,
            ships: ships
                .iter()
                .map(|s| ShipInfo {
                    e: Some(s.entity),
                    stance: format!("{:?}", s.mission.stance),
                    order: s.ship.order.index(),
                    waypoints: s.ship.waypoints.len(),
                    attack: s.mission.attack.clone(),
                    human: s.mission.human || s.primary,
                    ram: s.mission.ram,
                    speed: s.ship.speed,
                })
                .collect(),
        };
        let outcome = script.console_run_with(&cmd, &mut world, &env, &mut fx);
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
        apply_view(&outcome.view, &mut console, &mut dev, settings.as_deref_mut());
        each_ship(&mut ships, outcome.repair, |s| crate::docking::repair(&lib, &s.ship, &mut s.health, true));
        apply_fx(fx, script, paths, sides, &mut commands, &mut ships, &mut animals, &mut extra, &mut orbit, &lib);
    }
}

/// Applies the developer-view changes of a command; `fps` is saved with the options.
fn apply_view(changes: &[ViewChange], console: &mut Console, dev: &mut DevView, mut settings: Option<&mut crate::menu::Settings>) {
    for c in changes {
        dev.apply(c);
        if let (ViewChange::Fps(want), Some(settings)) = (c, settings.as_deref_mut()) {
            settings.fps = want.unwrap_or(!settings.fps);
            settings.save();
            console.say(format!("fps counter {}", if settings.fps { "on" } else { "off" }));
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::keyboard::Key;

    /// The console's key handling on its own (the battle's commands are tested in `rules::systems::tests`).
    fn skirmish() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<KeyboardInput>()
            .insert_resource(Console::new(true))
            .init_resource::<DevView>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<bevy::input::mouse::AccumulatedMouseScroll>()
            .add_systems(Update, keys);
        app
    }
    fn press(app: &mut App, code: KeyCode) {
        app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
            state: bevy::input::ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
    }

    #[test]
    fn the_console_opens_without_mission_rules() {
        let mut app = skirmish();
        assert!(app.world().get_resource::<Mission>().is_none());
        press(&mut app, KeyCode::Backquote);
        app.update();
        assert!(app.world().resource::<Console>().open, "the console key opens it in a skirmish");
        press(&mut app, KeyCode::Backquote);
        app.update();
        assert!(!app.world().resource::<Console>().open);
    }

    #[test]
    fn closing_with_escape_hides_the_escape_from_the_game() {
        let mut app = skirmish();
        press(&mut app, KeyCode::Backquote);
        app.update();
        assert!(app.world().resource::<Console>().open);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Escape,
            logical_key: Key::Escape,
            state: bevy::input::ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        assert!(!app.world().resource::<Console>().open);
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Escape), "the game must not also open the in-game menu");
    }
}
