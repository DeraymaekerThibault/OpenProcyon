//! ECS side of the developer console (rules/console.rs): the `~` key and typing, running submitted lines against the mission's
//! script and applying their effects, and the cheat mark. The drawing is the view's (`classic/console.rs`).

use bevy::input::keyboard::KeyboardInput;

use super::*;
use crate::menu::Screen;
use crate::rules::console::{Cheats, Command, Console, Edit, parse};

/// Whether the console is on without the Display option "Developer Console": debug builds, `--console` or `PROCYON_CONSOLE` in the
/// environment. A release build otherwise leaves it to the option (off by default), so a stray `~` never hands a player the cheats.
pub fn enabled() -> bool {
    cfg!(debug_assertions) || std::env::args().any(|a| a == "--console") || std::env::var_os("PROCYON_CONSOLE").is_some()
}

pub(super) fn plugin(app: &mut App) {
    app.insert_resource(Console::new(enabled()))
        .init_resource::<Cheats>()
        .add_systems(OnEnter(Screen::Battle), |mut cheats: ResMut<Cheats>| *cheats = Cheats::default())
        .add_systems(OnExit(Screen::Battle), close_on_leaving)
        // After the input systems and before anything reads the keys: while the console is open the game sees no keyboard or mouse.
        .add_systems(PreUpdate, keys.after(bevy::input::InputSystems).run_if(in_state(Screen::Battle)).run_if(resource_exists::<Mission>).run_if(crate::menu::overlay_closed))
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

/// `~` (or F12) opens the console and pauses the battle; Escape or `~` closes it. While it is open typing goes to its line and the
/// keyboard and mouse state the game reads is cleared.
fn keys(
    mut input: MessageReader<KeyboardInput>,
    mut console: ResMut<Console>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut vtime: ResMut<Time<Virtual>>,
) {
    if !console.available() {
        return;
    }
    for k in input.read().filter(|k| k.state.is_pressed()) {
        if !console.open {
            let tilde = matches!(&k.logical_key, bevy::input::keyboard::Key::Character(c) if c.as_str() == "~");
            if (tilde || k.key_code == KeyCode::F12) && !k.repeat {
                console.open = true;
                if !vtime.is_paused() {
                    vtime.pause();
                    console.paused_by_us = true;
                }
                if console.log.is_empty() {
                    console.say("Developer console. `help` lists the commands; ~ or Escape closes it.");
                }
            }
            continue;
        }
        match console.key(&k.logical_key) {
            Edit::Submit(line) => console.pending.push(line),
            Edit::Close => close(&mut console, &mut vtime),
            Edit::None => {}
        }
        if !console.open {
            break;
        }
    }
    if console.open {
        keyboard.reset_all();
        mouse.reset_all();
    }
}

/// Runs the lines submitted since the last frame: each becomes a rule action or condition on the mission's script, and its effects
/// are applied as a rule's would be.
fn run(
    mut console: ResMut<Console>,
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
    let (Some(mut mission), false) = (mission, console.pending.is_empty()) else { return };
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
        if cmd.cheats() {
            commands.queue(|w: &mut World| w.resource_mut::<Cheats>().used = true);
        }
        if let Command::Acts { god: Some(on), .. } = &cmd {
            god = *on;
            let on = *on;
            commands.queue(move |w: &mut World| w.resource_mut::<Cheats>().god = on);
        }
        let mut fx = vec![];
        let outcome = mission.script.console_run(&cmd, &mut world, target.0, &mut fx);
        for l in &outcome.lines {
            console.say(l);
        }
        each_ship(&mut ships, outcome.repair, |s| crate::docking::repair(&lib, &s.ship, &mut s.health, true));
        apply_fx(fx, mission, &mut commands, &mut ships, &mut animals, &mut extra, &mut orbit, &lib);
    }
}
