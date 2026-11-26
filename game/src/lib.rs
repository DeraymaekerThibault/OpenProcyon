//! Shared game library: simulation, rules, data and input. Skirmish slice: your ship vs enemy ships, original models/data, AI that fights back, damage effects, win/lose, restart.
//! Everything marked PROVISIONAL is a stand-in, not measured original behaviour (see AGENTS.md, rule 5).

pub mod alliance;
pub mod audio;
pub mod authoring;
pub mod battle;
pub mod bindings;
pub mod blackhole;
pub mod boarding;
pub mod briefing;
pub mod campaign;
pub mod cli;
pub mod clips;
pub mod cloak;
pub mod collision;
pub mod combat;
pub mod commands;
pub mod course;
pub mod crew;
pub mod crew_arms;
pub mod data;
pub mod decals;
pub mod docking;
pub mod dragons;
pub mod environment;
pub mod exit;
#[cfg(test)]
mod fixed_tests;
pub mod flag;
pub mod fx;
mod gametime;
pub mod geom;
pub mod interpolate;
pub mod lifeboats;
pub mod line_of_fire;
pub mod loading;
pub mod logging;
pub mod menu;
pub mod modding;
pub mod names;
pub mod nebula;
pub mod options_menu;
pub mod panels;
mod paths;
pub mod pointer;
mod profile;
pub mod profiles;
pub mod render;
pub mod retail_tex;
pub mod rules;
pub mod scenario;
pub mod setup;
pub mod ship;
pub mod shipart;
mod shot;
pub mod smoke;
pub mod sprites;
pub mod starmap;
pub mod stats;
#[cfg(test)]
mod testutil;
pub mod theme;
pub mod victory;
pub mod view;
pub mod wake;

// Explicit crate-root re-exports, grouped by owning module. No glob re-exports here (docs/development.md).
// scenario.rs: battle lifecycle and map objects.
pub use scenario::{
    Group, Namer, Outcome, Scenario, debug_kill, enter_battle, game_to_bevy, leave_battle, outcome, restart, resume_scenario, scripted_kills, scripted_steps, spawn_ship,
};
// ship.rs: ship state, sides and AI.
pub use ship::{EngineOrder, PlayerShip, Ship, ShipName, Side, ai, angdiff, approach_speed, step};
// battle.rs: the tick's sets and the removal and joining of objects.
pub use battle::{BattleSet, FrameRules, Joining, MAP_FLOOR, Removed, below_floor, queue_removal};
pub(crate) use battle::{battle_schedule, out_of_map, tick_start};
// gametime.rs: the battle tick, speed, pause and fixed-step mode.
#[cfg(test)]
pub(crate) use gametime::set_time_scale;
pub use gametime::{BATTLE_TICK, MAX_GAME_SPEED, TICK_SECS, clock_seed, fixed_step, set_game_speed, toggle_pause, wants_fixed_step};
pub(crate) use gametime::{present_mode, set_fixed_step};
// paths.rs: where assets, content and mods are found.
pub use paths::{assets_dir, bundled_mods_dir};
pub(crate) use paths::{base_assets, checkout_dir, content_dir, find_base_assets, own_overlay, player_assets, set_assets};
// profile.rs: PROCYON_PROFILE logging.
pub(crate) use profile::{profile_counts, profiling};
// shot.rs: the run's `Config` and screenshots.
pub use shot::Config;
pub(crate) use shot::screenshot;
// view.rs: camera, input and HUD wiring.
pub use view::{Orbit, Sky, Slot, camera, hud, input, sky, toggle_debug};

use bevy::prelude::*;
pub use combat::{GunStates, Health, Library, Target, Transient};
use std::f32::consts::PI;
use std::path::PathBuf;

/// What a tool launch sets the lobby up with: `--lobby <spec>`, or the skirmish a `--replay` recording holds (a battle set up in the
/// lobby, `commands::recorded_setup`). A named function, not a closure: Bevy names every non-capturing closure plugin alike, so a
/// second one panics at startup ("already added").
fn lobby_scenario(app: &mut App) {
    if let Some(launch) = lobby_launch(cli::args()) {
        app.insert_resource(launch);
    }
}

fn lobby_launch(args: &[String]) -> Option<menu::LobbyLaunch> {
    if let Some(spec) = cli::value_of(args, "--lobby") {
        return match menu::LobbyLaunch::parse(&spec) {
            Ok(launch) => Some(launch),
            Err(err) => {
                eprintln!("{err}");
                exit::quit(2);
            }
        };
    }
    commands::recorded_setup(args).map(menu::LobbyLaunch::Setup)
}

/// The command line starts a battle, a menu screen or a screenshot directly (no player is there to use the import screen).
fn is_scripted(args: &[String]) -> bool {
    args.iter().any(|a| cli::flag_named(a).is_some_and(|f| f.scripted))
}

/// Runs the game with one presentation (a view plugin such as the binary's `classic::plugin`); without imported assets it opens
/// the first-run import screen instead (`setup.rs`, drawn by `setup_view`), which starts the game again once the import is done.
pub fn run(view: fn(&mut App), setup_view: fn(&mut App)) {
    let args = cli::args();
    // Mission authoring (docs/modding.md): the rule schema needs no assets.
    if let Some(code) = rules::cli::dump_schema(args) {
        exit::quit(code);
    }
    // `--version` and `--dump-schemas <dir>` need neither a window nor assets.
    if let Some(code) = cli::quick(args) {
        exit::quit(code);
    }
    let script = args.iter().position(|a| a == "--script").map(|i| {
        args.get(i + 1).ok_or_else(|| "--script requires a name".to_owned()).and_then(|name| smoke::Script::parse(name)).unwrap_or_else(|err| {
            eprintln!("{err}");
            exit::quit(2);
        })
    });
    let mods = modding::mod_args(args).unwrap_or_else(|error| {
        eprintln!("error: {error}");
        exit::quit(2);
    });
    let base = base_assets();
    // A player's folder (the download's import) must come from this version and is replaced whole when imported again; a
    // developer's assets (PROCYON_ASSETS, the checkout's assets/) are used as they are.
    let player = player_assets(&base);
    let status = match player {
        true => setup::imported(&base, env!("CARGO_PKG_VERSION")),
        false if base.join("ships").is_dir() => setup::Imported::Ready,
        false => setup::Imported::Missing,
    };
    if status != setup::Imported::Ready && is_scripted(args) {
        // A scripted run (tools/smokes.sh, shot.sh) has nobody to answer the import screen: fail at once with the hint.
        eprintln!("No imported assets in {} ({status:?}). Import your copy of the game first: make assets ARGS=\"--game <install dir>\" (or set PROCYON_ASSETS).", base.display());
        exit::quit(1);
    }
    if status != setup::Imported::Ready || args.iter().any(|a| a == "--crashed") {
        eprintln!("Game assets in {}: {status:?}; opening the import screen.", base.display());
        return setup::run(base, content_dir(), setup_view, status, player);
    }
    if player && let Some(dir) = base.parent() {
        setup::report_crashes(dir.to_path_buf());
    }
    // `--only-mod` never even lists the player's installed mods; `mods.json` is read by the normal path only and written by neither.
    let installed = if args.iter().skip(1).any(|a| a == modding::ONLY_MOD) { vec![] } else { modding::installed(bundled_mods_dir().as_deref()) };
    let (all_mods, dirs) = modding::pick_mods(args, installed, mods.clone()).unwrap_or_else(|error| {
        eprintln!("error: {error}");
        exit::quit(2)
    });
    modding::log_warnings(&all_mods, &mods);
    modding::set_active(dirs);
    let layers: Vec<PathBuf> = std::iter::once(content_dir()).chain(modding::active().iter().cloned()).collect();
    let (root, report) = modding::overlay_report(&base, &layers, own_overlay());
    report.without(&content_dir()).log();
    set_assets(root);
    if let Some(code) = rules::cli::apply(args) {
        exit::quit(code);
    }
    // Bad ship or weapon data (a mod's ship.json) ends the launch here with the message, not with a panic while the app is built.
    if let Err(error) = combat::preload() {
        eprintln!("error: {error}");
        exit::quit(1);
    }
    let arg = |k: &str| cli::value_of(args, k);
    let shot = arg("--shot");
    set_fixed_step(wants_fixed_step(args, std::env::var("PROCYON_FIXED_STEP").ok().as_deref()));
    // Debug aids for mission authors: fire named rules at the start (repeatable) and place the primary ship (x,y in game units).
    let at: Option<Vec<f32>> = arg("--start-at").map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect());
    campaign::set_start(args.windows(2).filter(|w| w[0] == "--fire-rule").map(|w| w[1].clone()).collect(), at.and_then(|c| (c.len() == 2).then(|| (c[0], c[1]))));
    let shot_frames = shot::parse_shot_frames(arg("--shot-frame").as_deref());
    let cfg = Config {
        groups: vec![],
        player: arg("--ship").unwrap_or(ship::DEFAULT_SHIP.into()),
        allies: arg("--allies").map(|v| v.split(',').map(String::from).collect()).unwrap_or_default(),
        map: arg("--mission").map(|id| format!("mission{:02}", id.parse::<u32>().expect("--mission takes a number"))).or_else(|| arg("--map")),
        enemies: arg("--enemies").unwrap_or("Ship_Pirate_Schooner,Ship_Pirate_Sloop".into()).split(',').filter(|n| *n != "none").map(String::from).collect(),
        shot: shot.clone(),
        frame: 0,
        speed: arg("--speed").and_then(|v| v.parse().ok()).unwrap_or(1.0),
        ended: None,
        script,
        autolaunch: args.iter().any(|a| a == "--autolaunch"),
        shot_frame: shot_frames.last().copied().unwrap_or(500),
        shot_frames,
        late_frame: arg("--late-frame").and_then(|v| v.parse().ok()).unwrap_or(0), // 0: no _late shot
    };
    let result = App::new()
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin { file_path: assets_dir().to_string_lossy().into_owned(), ..default() })
                .set(logging::plugin())
                .set(WindowPlugin { primary_window: Some(shot::shot_window(shot.is_some())), ..default() }),
        )
        .add_plugins((clips::plugin, render::plugin))
        .add_plugins((
            fx::plugin,
            combat::plugin,
            sprites::plugin,
            panels::plugin,
            environment::plugin,
            nebula::plugin,
            flag::plugin,
            audio::plugin,
            menu::plugin,
            rules::plugin,
            docking::plugin,
            boarding::plugin,
            options_menu::plugin,
            starmap::plugin,
        ))
        .add_plugins((alliance::plugin, lifeboats::plugin, dragons::plugin, cloak::plugin, decals::plugin, stats::plugin, crew_arms::plugin))
        // The presentation (docs/development.md "Layers"): views live outside this library, so shared code cannot reach into them.
        .add_plugins(view)
        .add_plugins(|app: &mut App| {
            if profiling() {
                app.add_plugins((bevy::diagnostic::FrameTimeDiagnosticsPlugin::default(), bevy::render::diagnostic::RenderDiagnosticsPlugin)).add_systems(Last, profile_counts);
            }
            if let Some(free) = view::FreeCamera::load(assets_dir()) {
                app.insert_resource(free);
            }
        })
        .add_plugins(lobby_scenario)
        // Interactive runs open the front end; scripted/screenshot runs and --map go straight to battle (--menu forces the menu).
        .insert_state(match arg("--menu").as_deref() {
            _ if arg("--lobby").is_some() || commands::recorded_setup(args).is_some() => menu::Screen::Lobby,
            Some("single") => menu::Screen::SinglePlayer,
            Some("skirmish") => menu::Screen::CreateSkirmish,
            Some("lobby") => menu::Screen::Lobby,
            Some("options") => menu::Screen::Options,
            Some("mods") => menu::Screen::Mods,
            Some("controls") => menu::Screen::Controls,
            Some("fleet") => menu::Screen::Fleet,
            Some("load") => menu::Screen::Load,
            Some("audio") => menu::Screen::Audio,
            Some("display") => menu::Screen::Display,
            Some("tutorial") => menu::Screen::Tutorial,
            Some("missions") => menu::Screen::Missions,
            _ if args.iter().any(|a| a == "--menu") || (shot.is_none() && arg("--map").is_none() && arg("--mission").is_none()) => menu::Screen::Main,
            _ => menu::Screen::Battle,
        })
        .insert_resource(cfg)
        .insert_resource(match fixed_step() {
            true => {
                info!("fixed step: {} ms of game time per frame", BATTLE_TICK.as_millis());
                bevy::time::TimeUpdateStrategy::ManualDuration(BATTLE_TICK)
            }
            false => default(),
        })
        .add_plugins((scenario::plugin, bindings::plugin))
        .insert_resource({
            let c: Vec<f32> = arg("--cam").map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
            if c.len() == 3 {
                Orbit { yaw: c[0], pitch: c[1], dist: c[2], focus: None }
            } else if shot.is_some() {
                Orbit {
                    yaw: arg("--yaw").and_then(|v| v.parse().ok()).unwrap_or(0.9),
                    pitch: arg("--pitch").and_then(|v| v.parse().ok()).unwrap_or(0.9),
                    dist: arg("--dist").and_then(|v| v.parse().ok()).unwrap_or(300.0),
                    focus: None,
                }
            } else if view::FreeCamera::load(assets_dir()).is_some() {
                Orbit { yaw: 0.6, pitch: 0.5, dist: 60.0, focus: None }
            } else {
                // Retail: the pitch follows the distance (`view::camera`); the zoom limits raise it to the focus's nearest.
                Orbit { yaw: 0.6, pitch: 0.5, dist: view::RETAIL_START_DIST, focus: None }
            }
        })
        .add_systems(Startup, setup)
        .add_plugins((smoke::plugin, battle_schedule, interpolate::plugin, loading::plugin, commands::plugin))
        .add_plugins((battle::plugin, course::plugin, view::plugin))
        .run();
    // A system that stopped the run with an error code (`exit::app_exit`) sets the process's status.
    if let AppExit::Error(_) = result {
        exit::quit(exit::status(&result));
    }
}

/// The camera the player looks through. Other 3D cameras exist (the Fleet screen's ship preview renders to a texture), so systems
/// that need the one view camera filter on this, not on `Camera3d` alone: a `Single` over two cameras silently skips the system.
#[derive(Component)]
pub struct MainCamera;

fn setup(mut commands: Commands, cfg: Res<Config>, mut vtime: ResMut<Time<Virtual>>, mut window: Single<&mut Window, With<bevy::window::PrimaryWindow>>) {
    // --speed N runs the simulation faster for testing; without vsync, so a hidden window is not held to the compositor's pace.
    set_game_speed(&mut vtime, &mut window, cfg.speed);
    commands.spawn((MainCamera, Camera3d::default(), Projection::Perspective(PerspectiveProjection { far: 20000.0, ..default() })));
    commands.spawn((DirectionalLight { illuminance: 9000.0, shadow_maps_enabled: false, ..default() }, Transform::from_xyz(30.0, 60.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y)));
    commands.insert_resource(GlobalAmbientLight { color: Color::WHITE, brightness: 450.0, ..default() });
    commands.insert_resource(ClearColor(Color::srgb(0.03, 0.04, 0.09)));
    for (slot, node) in [
        (Slot::Left, Node { position_type: PositionType::Absolute, top: Val::Px(8.0), left: Val::Px(8.0), ..default() }),
        (Slot::Right, Node { position_type: PositionType::Absolute, top: Val::Px(8.0), right: Val::Px(8.0), ..default() }),
        (Slot::Centre, Node { position_type: PositionType::Absolute, top: Val::Px(120.0), left: Val::Percent(38.0), ..default() }),
    ] {
        commands.spawn((slot, Text::new(""), node, Visibility::Hidden)); // F1 toggles the debug text
    }
}

/// The item whose position is closest to `to`.
/// `iter.nearest_to(point, pos)`: the item whose `pos` is closest to `point` (see `nearest`).
pub trait Nearest: Iterator + Sized {
    fn nearest_to(self, to: Vec3, pos: impl Fn(&Self::Item) -> Vec3) -> Option<Self::Item> {
        nearest(self, to, pos)
    }
}
impl<I: Iterator> Nearest for I {}

pub fn nearest<T>(items: impl IntoIterator<Item = T>, to: Vec3, pos: impl Fn(&T) -> Vec3) -> Option<T> {
    items.into_iter().min_by(|a, b| pos(a).distance_squared(to).total_cmp(&pos(b).distance_squared(to)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nearest_picks_the_closest_position() {
        let pts = [Vec3::new(5.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0), Vec3::new(-9.0, 0.0, 0.0)];
        assert_eq!(nearest(pts, Vec3::ZERO, |p| *p), Some(pts[1]));
        assert_eq!(nearest(Vec::<Vec3>::new(), Vec3::ZERO, |p| *p), None);
    }
    #[test]
    fn scripted_runs_never_get_the_import_screen() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        for flag in ["--shot", "--script", "--menu", "--map", "--mission"] {
            assert!(is_scripted(&args(&["game", flag, "x"])), "{flag}");
        }
        assert!(!is_scripted(&args(&["game"])));
        assert!(!is_scripted(&args(&["game", "--crashed", "log.txt"])));
    }
}
