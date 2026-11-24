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
pub mod pointer;
pub mod profiles;
pub mod render;
pub mod retail_tex;
pub mod rules;
pub mod scenario;
pub mod setup;
pub mod ship;
pub mod shipart;
pub mod smoke;
pub mod sprites;
pub mod starmap;
pub mod stats;
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
// view.rs: camera, input and HUD wiring.
pub use view::{Orbit, Sky, Slot, camera, hud, input, sky, toggle_debug};

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
pub use combat::{GunStates, Health, Library, Target, Transient};
use std::f32::consts::{PI, TAU};
use std::path::PathBuf;

#[derive(Resource)]
pub struct Config {
    groups: Vec<Group>, // from the lobby; empty = derive from --ship/--allies/--enemies
    player: String,
    allies: Vec<String>,
    enemies: Vec<String>,
    map: Option<String>,
    shot: Option<String>,
    frame: u32,
    speed: f32,
    ended: Option<u32>,
    script: Option<smoke::Script>,
    /// `--autolaunch`: the lobby fills the fleet and starts the battle (dev/smoke hook).
    autolaunch: bool,
    /// `--shot-frame`: the last requested frame (scripts time themselves off it).
    shot_frame: u32,
    /// Every `--shot-frame` entry, ascending.
    shot_frames: Vec<u32>,
    late_frame: u32,
}

static ASSETS: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// `--fixed-step` / `PROCYON_FIXED_STEP=1` (#535): every frame advances exactly one battle tick of game time at 1x, whatever the
/// machine's frame rate, and the clock-seeded cosmetic draws (ship names, stars) use a fixed seed. With the battle seed (`--seed`)
/// and frames counted after loading, the same scripted run gives the same pictures and trace on every run. The crew stays silent
/// (its lines follow audio playback, not the tick).
static FIXED_STEP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn fixed_step() -> bool {
    FIXED_STEP.load(std::sync::atomic::Ordering::Relaxed)
}
/// A seed from the clock, or a constant in a fixed-step run.
pub fn clock_seed() -> u64 {
    if fixed_step() {
        return 0x9E37_79B9;
    }
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64)
}
/// Converted assets: `PROCYON_ASSETS`, else `assets/` in the source checkout, else the per-user folder the importer
/// (`python -m tools.import_assets`, or the release download's `openprocyon-import`) fills. Nothing from the original game ships with the code.
pub(crate) fn base_assets() -> PathBuf {
    find_base_assets().unwrap_or_else(|| PathBuf::from("assets"))
}
/// The lookup behind `base_assets`, with no fallback: `None` when there is no env var, no checkout and no per-user folder to name.
/// Touches nothing on disk.
pub(crate) fn find_base_assets() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("PROCYON_ASSETS") {
        return Some(PathBuf::from(dir));
    }
    let checkout = checkout_dir().map(|dir| dir.join("assets"));
    if let Some(checkout) = checkout.as_ref().filter(|c| c.join("ships").is_dir()) {
        return Some(checkout.clone());
    }
    user_assets().or(checkout)
}

/// The source checkout, found at run time (never baked into the binary, so a release build carries no build-machine path): the
/// nearest folder holding `game/Cargo.toml` and `content/`, looking up from the working directory, then from the executable
/// (`make run` and the check scripts start inside the checkout or build into it).
pub(crate) fn checkout_dir() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok();
    let binary = std::env::current_exe().ok();
    [cwd, binary]
        .into_iter()
        .flatten()
        .find_map(|start| start.ancestors().find(|dir| dir.join("game/Cargo.toml").is_file() && dir.join("content").is_dir()).map(|dir| dir.to_path_buf()))
}

/// Where the importer puts converted assets by default (see `campaign::user_dir`).
fn user_assets() -> Option<PathBuf> {
    Some(campaign::user_dir()?.join("assets"))
}
/// Whether `base` is the per-user folder a player's download imports into (not PROCYON_ASSETS or the checkout's assets/).
fn player_assets(base: &std::path::Path) -> bool {
    std::env::var_os("PROCYON_ASSETS").is_none() && user_assets().is_some_and(|u| u == base)
}
/// Files that ship with the code (fonts, hand-written data), layered over the imported assets: `content/` next to the
/// executable (the release download), else `content/` in the checkout.
fn content_dir() -> PathBuf {
    let exe_dir = std::env::current_exe().ok().and_then(|path| path.parent().map(|dir| dir.join("content")));
    content_dir_from(exe_dir, checkout_dir().map_or_else(|| PathBuf::from("content"), |dir| dir.join("content")))
}
/// The `mods/` that ships with the game: next to the executable in a download, the checkout's under `make run`.
pub fn bundled_mods_dir() -> Option<PathBuf> {
    let beside_binary = std::env::current_exe().ok().and_then(|path| path.parent().map(|dir| dir.join("mods")));
    beside_binary.filter(|dir| dir.is_dir()).or_else(|| checkout_dir().map(|dir| dir.join("mods")).filter(|d| d.is_dir()))
}
fn content_dir_from(next_to_exe: Option<PathBuf>, checkout: PathBuf) -> PathBuf {
    next_to_exe.filter(|dir| dir.join("fonts").is_dir()).unwrap_or(checkout)
}
/// PROCYON_OVERLAY: where this launch builds its overlay (modding::overlay).
fn own_overlay() -> Option<PathBuf> {
    std::env::var_os("PROCYON_OVERLAY").map(PathBuf::from)
}
/// The asset root: the imported assets with `content/` and any `--mod` folders overlaid on top (see modding.rs).
pub fn assets_dir() -> &'static std::path::Path {
    ASSETS.get_or_init(|| modding::overlay(&base_assets(), &[content_dir()], own_overlay()))
}

/// --lobby <spec>: play a skirmish setup through the lobby model instead of the --ship/--allies/--enemies shortcut. A named function,
/// not a closure: Bevy names every non-capturing closure plugin alike, so a second one panics at startup ("already added").
fn lobby_scenario(app: &mut App) {
    if let Some(spec) = std::env::args().skip_while(|a| a != "--lobby").nth(1) {
        match menu::LobbyScenario::parse(&spec) {
            Ok(scenario) => {
                app.insert_resource(scenario);
            }
            Err(err) => {
                eprintln!("{err}");
                exit::quit(2);
            }
        }
    }
}

/// The command line starts a battle, a menu screen or a screenshot directly (no player is there to use the import screen).
fn is_scripted(args: &[String]) -> bool {
    args.iter().any(|a| matches!(a.as_str(), "--shot" | "--script" | "--menu" | "--map" | "--mission"))
}

/// Runs the game with one presentation (a view plugin such as the binary's `classic::plugin`); without imported assets it opens
/// the first-run import screen instead (`setup.rs`, drawn by `setup_view`), which starts the game again once the import is done.
pub fn run(view: fn(&mut App), setup_view: fn(&mut App)) {
    let args: Vec<String> = std::env::args().collect();
    // Mission authoring (docs/modding.md): the rule schema needs no assets.
    if let Some(code) = rules::cli::dump_schema(&args) {
        exit::quit(code);
    }
    // `--version` and `--dump-schemas <dir>` need neither a window nor assets.
    if let Some(code) = cli::quick(&args) {
        exit::quit(code);
    }
    let script = args.iter().position(|a| a == "--script").map(|i| {
        args.get(i + 1).ok_or_else(|| "--script requires a name".to_owned()).and_then(|name| smoke::Script::parse(name)).unwrap_or_else(|err| {
            eprintln!("{err}");
            exit::quit(2);
        })
    });
    let mods = modding::mod_args(&args).unwrap_or_else(|error| {
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
    if status != setup::Imported::Ready && is_scripted(&args) {
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
    let all_mods = modding::installed(bundled_mods_dir().as_deref());
    modding::log_warnings(&all_mods, &mods);
    let installed = all_mods.into_iter().filter(|m| m.enabled).map(|m| m.dir);
    modding::set_active(installed.chain(mods).collect());
    let layers: Vec<PathBuf> = std::iter::once(content_dir()).chain(modding::active().iter().cloned()).collect();
    let (root, report) = modding::overlay_report(&base, &layers, own_overlay());
    report.without(&content_dir()).log();
    let _ = ASSETS.set(root);
    if let Some(code) = rules::cli::apply(&args) {
        exit::quit(code);
    }
    // Bad ship or weapon data (a mod's ship.json) ends the launch here with the message, not with a panic while the app is built.
    if let Err(error) = combat::preload() {
        eprintln!("error: {error}");
        exit::quit(1);
    }
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let shot = arg("--shot");
    FIXED_STEP.store(args.iter().any(|a| a == "--fixed-step") || std::env::var_os("PROCYON_FIXED_STEP").is_some_and(|v| v == "1"), std::sync::atomic::Ordering::Relaxed);
    // Debug aids for mission authors: fire named rules at the start (repeatable) and place the primary ship (x,y in game units).
    let at: Option<Vec<f32>> = arg("--start-at").map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect());
    campaign::set_start(args.windows(2).filter(|w| w[0] == "--fire-rule").map(|w| w[1].clone()).collect(), at.and_then(|c| (c.len() == 2).then(|| (c[0], c[1]))));
    let shot_frames = parse_shot_frames(arg("--shot-frame").as_deref());
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
                .set(WindowPlugin { primary_window: Some(shot_window(shot.is_some())), ..default() }),
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
            rules::skirmish::plugin,
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
            _ if arg("--lobby").is_some() => menu::Screen::Lobby,
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
        .add_plugins((smoke::plugin, battle_schedule, interpolate::plugin, loading::plugin))
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

/// Retail tick documented in docs/re/gunnery.md and docs/re/effects.md.
pub const BATTLE_TICK: std::time::Duration = std::time::Duration::from_millis(33);
/// The same tick in seconds, for rules the original writes per tick (rates, pulls, chances): one tick is 0.033 s, not 1/30 s.
pub const TICK_SECS: f32 = 0.033;

/// Queued for removal: the object is destroyed at the start of the next tick and, until then, still takes part in the logic of
/// the tick it was queued in (the original's deferred removal, docs/re/tick.md).
#[derive(Component)]
pub struct Removed;

/// Queues `e` for removal at the start of the next tick. Safe to call twice, or for an entity already gone.
pub fn queue_removal(commands: &mut Commands, e: Entity) {
    commands.entity(e).try_insert(Removed);
}

/// Created during a tick: the object joins at the start of the next tick and is neither moved nor tested before then.
/// Movement systems of such objects (shells, blasts, wells) leave `With<Joining>` out.
#[derive(Component)]
pub struct Joining;

/// Height below which a moving object has left the map (docs/re/tick.md).
pub const MAP_FLOOR: f32 = -130.0;

/// Whether `p` (Bevy axes, y up) is below the map's floor.
pub fn below_floor(p: Vec3) -> bool {
    p.y < MAP_FLOOR
}

/// After the movement of a tick: moving objects that ended below the floor are queued for removal. Shells test their own step.
pub(crate) fn out_of_map(mut commands: Commands, movers: Query<(Entity, &Transform), (With<combat::Asteroid>, Without<Removed>)>) {
    for (e, tf) in &movers {
        if below_floor(tf.translation) {
            queue_removal(&mut commands, e);
        }
    }
}

/// Start of a tick: destroy what was queued for removal last tick, and let what was created last tick join.
pub(crate) fn tick_start(mut commands: Commands, removed: Query<Entity, With<Removed>>, joining: Query<Entity, With<Joining>>) {
    for e in &removed {
        commands.entity(e).try_despawn();
    }
    for e in &joining {
        commands.entity(e).try_remove::<Joining>();
    }
}

/// Single-player rules (mission and skirmish scripts): evaluated once per frame, after the last tick of that frame and also while the
/// game is paused (docs/re/tick.md). At 2x and 5x they run a half or a fifth as often as ticks. Their `Res<Time>` is the frame's
/// game-time advance, so a script timer still adds up to the ticks' time.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameRules;

/// Ordered shared simulation stages; input and presentation stay on the frame schedules.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BattleSet {
    /// Removals queued last tick are applied and new objects join, before anything else of the tick.
    Tick,
    Commands,
    Rules,
    /// Step 6 of the original's tick, the mission tick: boarding fights are resolved and links (grapples, tows) test whether they break
    /// and reel in. Skipped while the mission is frozen. No position is written here.
    MissionTick,
    Forces,
    Simulate,
    /// Sensor snapshot of the positions after movement and collision, for the decisions below.
    Sense,
    /// Per-object decisions (captains, mission AI) run after movement and collision; what they decide acts in the next tick's integration.
    Ai,
    Fleet,
    /// Ship upkeep (fire, deaths, repair) and shells, after the object's decision.
    Combat,
    Cleanup,
    Outcome,
}

fn battle_schedule(app: &mut App) {
    app.insert_resource(Time::<Fixed>::from_duration(BATTLE_TICK)).add_systems(FixedUpdate, tick_start.in_set(BattleSet::Tick)).configure_sets(
        FixedUpdate,
        (
            BattleSet::Tick,
            BattleSet::Commands,
            BattleSet::Rules,
            BattleSet::MissionTick,
            BattleSet::Forces,
            BattleSet::Simulate,
            BattleSet::Sense,
            BattleSet::Ai,
            BattleSet::Fleet,
            BattleSet::Combat,
            BattleSet::Cleanup,
            BattleSet::Outcome,
        )
            .chain()
            .run_if(loading::done),
    );
}

/// Preserve the virtual-time catch-up cap; fixed ticks now advance motion and timers together.
/// At low frame rates the effective speed drops below `speed` instead of taking unbounded catch-up steps.
fn set_time_scale(vtime: &mut Time<Virtual>, speed: f32) {
    let speed = speed.max(0.01);
    vtime.set_relative_speed(speed);
    vtime.set_max_delta(std::time::Duration::from_secs_f32(max_frame_step(speed)));
}

/// Most game time one call may advance. At the original's speeds (up to 5x): 10 frames of `speed` ticks each, real time beyond
/// that is dropped (docs/re/tick.md). Above 5x, which the original does not have: 0.1 s (about 3 ticks) so a 16x battle stays
/// smooth instead of turning one slow frame into a long burst. An unattended autoplay run (`PROCYON_AUTOPLAY`) only wants game
/// time per second, so there the step grows with the speed (60 fps worth of it).
fn max_frame_step(speed: f32) -> f32 {
    match speed > MAX_RETAIL_SPEED {
        true if std::env::var_os("PROCYON_AUTOPLAY").is_some() => (speed / 60.0).max(0.1),
        true => 0.1,
        false => MAX_CATCH_UP_FRAMES as f32 * BATTLE_TICK.as_secs_f32() * speed,
    }
}

/// Fastest speed setting the original has.
const MAX_RETAIL_SPEED: f32 = 5.0;

/// Frames the simulation may run in one call before the backlog is dropped.
const MAX_CATCH_UP_FRAMES: u32 = 10;

/// Fastest speed of the HUD speed buttons when `data/hud/speed.json` gives no `max`.
pub const MAX_GAME_SPEED: f32 = 16.0;

/// Freezes or resumes the simulation by pausing virtual time (the HUD pause button and the Pause key, `view.rs`). The speed setting is kept.
/// Systems on `Res<Time>` stop advancing; real-time UI (menus, star map, options) keeps working.
pub fn toggle_pause(vtime: &mut Time<Virtual>) {
    if vtime.is_paused() { vtime.unpause() } else { vtime.pause() }
}

/// Whether `PROCYON_PROFILE` is set (frame-time logging, and vsync off so the display does not cap the measurement).
pub(crate) fn profiling() -> bool {
    std::env::var_os("PROCYON_PROFILE").is_some()
}

/// Vsync is off when the player turns it off, while fast-forwarding (more capped steps fit in a second) and whenever profiling
/// (a vsynced frame time only measures the display period).
pub(crate) fn present_mode(profiling: bool, speed: f32) -> bevy::window::PresentMode {
    if profiling || speed > 1.0 || !menu::vsync() { bevy::window::PresentMode::AutoNoVsync } else { bevy::window::PresentMode::AutoVsync }
}

/// Testing fast-forward; see [`present_mode`] for the vsync rule.
pub fn set_game_speed(vtime: &mut Time<Virtual>, window: &mut Window, speed: f32) {
    set_time_scale(vtime, speed);
    window.present_mode = present_mode(profiling(), speed);
}

/// 99th-percentile (nearest rank) of per-frame times; 0 for no samples. Sorts the slice.
fn p99(samples: &mut [f64]) -> f64 {
    samples.sort_by(f64::total_cmp);
    match samples.len() {
        0 => 0.0,
        n => samples[(n * 99).div_ceil(100) - 1],
    }
}

fn profile_counts(
    time: Res<Time<Real>>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    mission: Option<Res<campaign::Mission>>,
    entities: Query<Entity>,
    ships: Query<&Ship>,
    rocks: Query<&combat::Asteroid>,
    effects: Query<&fx::Fx>,
    meshes: Res<Assets<Mesh>>,
    audio: Query<&AudioPlayer>,
    point_lights: Query<&PointLight>,
    spot_lights: Query<&SpotLight>,
    draws: Query<&Mesh3d>,
    standard_materials: Res<Assets<StandardMaterial>>,
    effect_materials: Res<Assets<fx::EffectMaterial>>,
    mut last: Local<(f32, bool)>,
    mut frame_samples: Local<Vec<f64>>,
) {
    // One sample per rendered frame; the p99 covers the frames since the previous log line.
    if let Some(sample) = diagnostics.get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FRAME_TIME).and_then(|d| d.value()) {
        frame_samples.push(sample);
    }
    let (mission_time, objective, ended) = mission.as_ref().map_or((0.0, None, false), |m| (m.script.time, m.script.point, m.script.ended.is_some()));
    if time.elapsed_secs() - last.0 < 5.0 && ended == last.1 {
        return;
    }
    *last = (time.elapsed_secs(), ended);
    let frame_ms = diagnostics.get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FRAME_TIME).and_then(|d| d.average()).unwrap_or(0.0);
    let p99_ms = p99(&mut frame_samples);
    frame_samples.clear();
    let particles: usize = effects.iter().map(fx::Fx::particle_count).sum();
    info!(
        "profile real={:.1} mission={mission_time:.1} objective={objective:?} ended={ended} frame_ms={frame_ms:.2} entities={} ships={} rocks={} effects={} particles={particles} mesh_assets={} audio={} p99_ms={p99_ms:.2} lights={} draw_entities={} materials={}",
        time.elapsed_secs(),
        entities.iter().count(),
        ships.iter().count(),
        rocks.iter().count(),
        effects.iter().count(),
        meshes.len(),
        audio.iter().count(),
        point_lights.iter().count() + spot_lights.iter().count(),
        draws.iter().count(),
        standard_materials.len() + effect_materials.len()
    );
    // GPU milliseconds per render pass (rolling average), the passes above 0.2 ms, slowest first: where a slow frame goes.
    let mut gpu: Vec<(f64, String)> = diagnostics
        .iter()
        .filter_map(|d| Some((d.average()?, d.path().as_str().strip_prefix("render/")?.strip_suffix("/elapsed_gpu")?.to_string())))
        .filter(|(ms, _)| *ms > 0.2)
        .collect();
    gpu.sort_by(|a, b| b.0.total_cmp(&a.0));
    info!("profile gpu_ms {}", gpu.iter().map(|(ms, pass)| format!("{pass}={ms:.2}")).collect::<Vec<_>>().join(" "));
}

/// `--shot-frame 100,200`: the frames to save, ascending. Nothing parsable (or no flag) means frame 500.
fn parse_shot_frames(arg: Option<&str>) -> Vec<u32> {
    let mut frames: Vec<u32> = arg.unwrap_or("").split(',').filter_map(|v| v.trim().parse().ok()).collect();
    frames.sort_unstable();
    frames.dedup();
    if frames.is_empty() { vec![500] } else { frames }
}

/// File for each requested frame: the `--shot` path itself for one frame, `<stem>_<frame>.png` for a list.
fn shot_paths(path: &str, frames: &[u32]) -> Vec<(u32, String)> {
    let several = frames.len() > 1;
    frames.iter().map(|&f| (f, if several { path.replace(".png", &format!("_{f}.png")) } else { path.to_string() })).collect()
}

/// Screenshot runs render at 1280x720 physical pixels on every backend (scale factor 1), so pictures compare; interactive runs keep the default window.
fn shot_window(shooting: bool) -> Window {
    if !shooting {
        return Window::default();
    }
    Window { resolution: bevy::window::WindowResolution::new(1280, 720).with_scale_factor_override(1.0), resizable: false, ..default() }
}

/// Whether a screenshot run is over. Every requested picture must be on disk (`saved` counts finished writes). A scripted run
/// then goes on `SCRIPT_TAIL` frames, because smoke scripts assert after their picture (`pause` at frame 70, `fleet` up to 130).
/// A battle that ended stops 150 frames later and the `restart` script at frame 560.
fn run_is_over(frame: u32, last_shot: u32, saved: usize, wanted: usize, scripted: bool, ended: Option<u32>, restart: bool) -> bool {
    const SCRIPT_TAIL: u32 = 100;
    let pictures_done = saved >= wanted && frame >= last_shot + if scripted { SCRIPT_TAIL } else { 0 };
    ended.is_some_and(|e| frame >= e + 150) || pictures_done || (restart && frame >= 560)
}

static SHOTS_SAVED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub(crate) fn screenshot(mut commands: Commands, mut cfg: ResMut<Config>, loading: Option<Res<loading::Loading>>, mut exit: MessageWriter<AppExit>) {
    let Some(path) = cfg.shot.clone() else { return };
    // scripts and pictures count frames of the battle: the count waits while it loads
    if loading.is_some() {
        return;
    }
    cfg.frame += 1;
    let mut shots = shot_paths(&path, &cfg.shot_frames);
    if cfg.late_frame > 0 {
        shots.push((cfg.late_frame, path.replace(".png", "_late.png")));
    }
    if cfg.script.as_deref() == Some("speedtest") {
        for (order, label) in ["stop", "slow", "full", "emergency"].iter().enumerate() {
            shots.push((45 + order as u32 * 90, path.replace(".png", &format!("_{label}.png"))));
        }
    }
    if let Some(end) = cfg.ended {
        shots.push((end + 90, path.replace(".png", "_end.png")));
    }
    let last = shots.iter().map(|s| s.0).max().unwrap_or(0);
    for (frame, p) in &shots {
        if cfg.frame == *frame {
            let mut save = bevy::render::view::screenshot::save_to_disk(p.clone());
            commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window()).observe(move |event: On<bevy::render::view::screenshot::ScreenshotCaptured>| {
                save(event);
                SHOTS_SAVED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            });
        }
    }
    let saved = SHOTS_SAVED.load(std::sync::atomic::Ordering::SeqCst);
    if run_is_over(cfg.frame, last, saved, shots.len(), cfg.script.is_some(), cfg.ended, cfg.script.as_deref() == Some("restart")) {
        exit.write(exit::app_exit(0));
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
    #[test]
    fn shot_frames_parse_sort_and_name_their_files() {
        assert_eq!(parse_shot_frames(None), vec![500]);
        assert_eq!(parse_shot_frames(Some("300,100,200,100")), vec![100, 200, 300]);
        assert_eq!(parse_shot_frames(Some("x")), vec![500]);
        assert_eq!(shot_paths("a/x.png", &[300]), vec![(300, "a/x.png".to_string())], "one frame keeps the plain name");
        assert_eq!(shot_paths("a/x.png", &[100, 200]), vec![(100, "a/x_100.png".to_string()), (200, "a/x_200.png".to_string())]);
    }
    #[test]
    fn a_run_ends_once_pictures_are_saved() {
        assert!(!run_is_over(300, 300, 0, 1, false, None, false), "picture not written yet");
        assert!(run_is_over(303, 300, 1, 1, false, None, false), "plain run stops at the save");
        assert!(!run_is_over(350, 300, 1, 1, true, None, false), "scripts keep asserting after their picture");
        assert!(run_is_over(400, 300, 1, 1, true, None, false));
        assert!(!run_is_over(450, 300, 1, 2, false, None, false), "second picture missing");
        assert!(run_is_over(560, 300, 0, 1, true, None, true), "restart script");
        assert!(run_is_over(250, 900, 0, 1, true, Some(100), false), "battle ended long ago");
    }
    #[test]
    fn p99_is_the_nearest_rank_sample() {
        assert_eq!(p99(&mut []), 0.0);
        assert_eq!(p99(&mut [7.0]), 7.0);
        let mut hundred: Vec<f64> = (1..=100).rev().map(f64::from).collect();
        assert_eq!(p99(&mut hundred), 99.0, "one outlier in a hundred frames is excluded");
        let mut stutter = vec![16.0; 99];
        stutter.push(250.0);
        assert_eq!(p99(&mut stutter), 16.0);
        stutter.push(250.0);
        assert_eq!(p99(&mut stutter), 250.0, "two slow frames in 101 are the tail");
    }
    #[test]
    fn profiling_turns_vsync_off_at_every_speed() {
        use bevy::window::PresentMode::{AutoNoVsync, AutoVsync};
        assert_eq!(present_mode(false, 1.0), AutoVsync);
        assert_eq!(present_mode(false, 4.0), AutoNoVsync);
        assert_eq!(present_mode(true, 1.0), AutoNoVsync);
        assert_eq!(present_mode(true, 4.0), AutoNoVsync);
    }
    #[test]
    fn content_next_to_the_executable_wins_over_the_checkout() {
        let dir = std::env::temp_dir().join(format!("procyon-content-test-{}", std::process::id()));
        let (unpacked, checkout) = (dir.join("download/content"), dir.join("checkout/content"));
        assert_eq!(content_dir_from(Some(unpacked.clone()), checkout.clone()), checkout, "no content/ next to a cargo-built binary");
        std::fs::create_dir_all(unpacked.join("fonts")).unwrap();
        assert_eq!(content_dir_from(Some(unpacked.clone()), checkout), unpacked);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
