//! Shared game library: simulation, rules, data and input. Skirmish slice: your ship vs enemy ships, original models/data, AI that fights back, damage effects, win/lose, restart.
//! Everything marked PROVISIONAL is a stand-in, not measured original behaviour (see AGENTS.md, rule 5).

pub mod alliance;
pub mod audio;
pub mod bindings;
pub mod blackhole;
pub mod boarding;
pub mod briefing;
pub mod campaign;
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
#[cfg(test)]
mod fixed_tests;
pub mod flag;
pub mod fx;
pub mod interpolate;
pub mod lifeboats;
pub mod logging;
pub mod menu;
pub mod modding;
pub mod nebula;
pub mod options_menu;
pub mod panels;
pub mod profiles;
pub mod render;
pub mod rules;
pub mod scenario;
pub mod setup;
pub mod ship;
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
pub use scenario::{Group, Namer, Outcome, Scenario, debug_kill, enter_battle, game_to_bevy, leave_battle, outcome, restart, resume_scenario, shot_target, spawn_ship};
// ship.rs: ship state, sides and AI.
pub use ship::{ORDERS, PlayerShip, Ship, ShipName, Side, ai, angdiff, approach_speed, step};
// view.rs: camera, input and HUD wiring.
pub use view::{Orbit, Sky, Slot, camera, hud, input, sky, toggle_debug};

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use collision::sail;
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
    shot_frame: u32,
    late_frame: u32,
}

static ASSETS: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
/// Converted assets: `PROCYON_ASSETS`, else `assets/` in the source checkout, else the per-user folder the importer
/// (`python -m tools.import_assets`, or the release download's `openprocyon-import`) fills. Nothing from the original game ships with the code.
fn base_assets() -> PathBuf {
    if let Some(dir) = std::env::var_os("PROCYON_ASSETS") {
        return PathBuf::from(dir);
    }
    let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets");
    if checkout.join("ships").is_dir() {
        return checkout;
    }
    user_assets().unwrap_or(checkout)
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
    content_dir_from(exe_dir, PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../content"))
}
/// The `mods/` that ships with the game: next to the executable in a download, the checkout's under `make run`.
pub fn bundled_mods_dir() -> Option<PathBuf> {
    let beside_binary = std::env::current_exe().ok().and_then(|path| path.parent().map(|dir| dir.join("mods")));
    beside_binary.filter(|dir| dir.is_dir()).or_else(|| Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../mods")).filter(|d| d.is_dir()))
}
fn content_dir_from(next_to_exe: Option<PathBuf>, checkout: PathBuf) -> PathBuf {
    next_to_exe.filter(|dir| dir.join("fonts").is_dir()).unwrap_or(checkout)
}
/// The asset root: the imported assets with `content/` and any `--mod` folders overlaid on top (see modding.rs).
pub fn assets_dir() -> &'static std::path::Path {
    ASSETS.get_or_init(|| modding::overlay(&base_assets(), &[content_dir()]))
}

/// Runs the game with one presentation (a view plugin such as the binary's `classic::plugin`); without imported assets it opens
/// the first-run import screen instead (`setup.rs`, drawn by `setup_view`), which starts the game again once the import is done.
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
                std::process::exit(2);
            }
        }
    }
}

pub fn run(view: fn(&mut App), setup_view: fn(&mut App)) {
    let args: Vec<String> = std::env::args().collect();
    let script = args.iter().position(|a| a == "--script").map(|i| {
        args.get(i + 1).ok_or_else(|| "--script requires a name".to_owned()).and_then(|name| smoke::Script::parse(name)).unwrap_or_else(|err| {
            eprintln!("{err}");
            std::process::exit(2);
        })
    });
    let mods: Vec<PathBuf> = args.windows(2).filter(|w| w[0] == "--mod").map(|w| PathBuf::from(&w[1])).collect();
    let base = base_assets();
    // A player's folder (the download's import) must come from this version and is replaced whole when imported again; a
    // developer's assets (PROCYON_ASSETS, the checkout's assets/) are used as they are.
    let player = player_assets(&base);
    let status = match player {
        true => setup::imported(&base, env!("CARGO_PKG_VERSION")),
        false if base.join("ships").is_dir() => setup::Imported::Ready,
        false => setup::Imported::Missing,
    };
    if status != setup::Imported::Ready || args.iter().any(|a| a == "--crashed") {
        eprintln!("Game assets in {}: {status:?}; opening the import screen.", base.display());
        return setup::run(base, content_dir(), setup_view, status, player);
    }
    if player && let Some(dir) = base.parent() {
        setup::report_crashes(dir.to_path_buf());
    }
    let installed = modding::installed(bundled_mods_dir().as_deref()).into_iter().filter(|m| m.enabled).map(|m| m.dir);
    let layers: Vec<PathBuf> = std::iter::once(content_dir()).chain(installed).chain(mods).collect();
    let _ = ASSETS.set(modding::overlay(&base, &layers));
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let shot = arg("--shot");
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
        shot_frame: arg("--shot-frame").and_then(|v| v.parse().ok()).unwrap_or(500),
        late_frame: arg("--late-frame").and_then(|v| v.parse().ok()).unwrap_or(9000), // 0: no _late shot (slow software rendering)
    };
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin { file_path: assets_dir().to_string_lossy().into_owned(), ..default() }).set(logging::plugin()))
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
                app.add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default()).add_systems(Last, profile_counts);
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
            _ if args.iter().any(|a| a == "--menu") || (shot.is_none() && arg("--map").is_none() && arg("--mission").is_none()) => menu::Screen::Main,
            _ => menu::Screen::Battle,
        })
        .insert_resource(cfg)
        .insert_resource(Outcome::Playing)
        .init_resource::<scenario::EndMessage>()
        .init_resource::<scenario::EndLine>()
        .init_resource::<scenario::MapLighting>()
        .add_systems(Update, scenario::apply_lighting.run_if(resource_changed::<scenario::MapLighting>))
        .insert_resource(bindings::Bindings::load())
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
            } else {
                Orbit { yaw: 0.6, pitch: 0.5, dist: 60.0, focus: None }
            }
        })
        .init_resource::<collision::Shapes>()
        .add_message::<collision::Ram>()
        .init_resource::<ship::Fleets>()
        .init_resource::<ship::CaptainTargets>()
        .init_resource::<course::Plot>()
        .add_systems(Startup, setup)
        .add_plugins((smoke::plugin, battle_schedule, interpolate::plugin))
        .add_systems(
            FixedUpdate,
            (
                ship::reconcile_fleets.in_set(BattleSet::Commands),
                ai.in_set(BattleSet::Ai),
                ship::fleet_follow.in_set(BattleSet::Fleet),
                (briefing::halt_arrival, blackhole::pull, sail).chain().in_set(BattleSet::Simulate),
                outcome.in_set(BattleSet::Outcome),
            )
                .run_if(in_state(menu::Screen::Battle))
                .run_if(menu::overlay_closed),
        )
        .add_systems(OnEnter(menu::Screen::Battle), enter_battle)
        .add_systems(OnExit(menu::Screen::Battle), scenario::reset_fleets)
        .add_systems(
            Update,
            (
                (
                    (shot_target, restart, debug_kill).chain(),
                    briefing::update,
                    view::fleet_keys,
                    input,
                    toggle_debug,
                    camera,
                    sky,
                    wake::update,
                    blackhole::update,
                    course::update,
                    hud,
                    leave_battle,
                )
                    .chain()
                    .run_if(in_state(menu::Screen::Battle))
                    .run_if(menu::overlay_closed),
                screenshot,
                view::hd_sky,
                view::screenshot_key,
                view::focus_projectile.run_if(in_state(menu::Screen::Battle)),
            ),
        )
        .run();
}

fn setup(mut commands: Commands, cfg: Res<Config>, mut vtime: ResMut<Time<Virtual>>, mut window: Single<&mut Window, With<bevy::window::PrimaryWindow>>) {
    // --speed N runs the simulation faster for testing; without vsync, so a hidden window is not held to the compositor's pace.
    set_game_speed(&mut vtime, &mut window, cfg.speed);
    commands.spawn((Camera3d::default(), Projection::Perspective(PerspectiveProjection { far: 20000.0, ..default() })));
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

/// Ordered shared simulation stages; input and presentation stay on the frame schedules.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BattleSet {
    Commands,
    Sense,
    Rules,
    Ai,
    Fleet,
    Orders,
    Forces,
    Simulate,
    Combat,
    Cleanup,
    Outcome,
}

fn battle_schedule(app: &mut App) {
    app.insert_resource(Time::<Fixed>::from_duration(BATTLE_TICK)).configure_sets(
        FixedUpdate,
        (
            BattleSet::Commands,
            BattleSet::Sense,
            BattleSet::Rules,
            BattleSet::Ai,
            BattleSet::Fleet,
            BattleSet::Orders,
            BattleSet::Forces,
            BattleSet::Simulate,
            BattleSet::Combat,
            BattleSet::Cleanup,
            BattleSet::Outcome,
        )
            .chain(),
    );
}

/// Preserve the virtual-time catch-up cap; fixed ticks now advance motion and timers together.
/// At low frame rates the effective speed drops below `speed` instead of taking unbounded catch-up steps.
fn set_time_scale(vtime: &mut Time<Virtual>, speed: f32) {
    let speed = speed.max(0.01);
    vtime.set_relative_speed(speed);
    vtime.set_max_delta(std::time::Duration::from_secs_f32(if speed > 1.0 { 0.1 } else { 0.25 }));
}

/// Fastest speed of the HUD speed buttons when `data/hud/speed.json` gives no `max`.
pub const MAX_GAME_SPEED: f32 = 16.0;

/// Freezes or resumes the simulation by pausing virtual time (the HUD pause button and non-retail Pause key). The speed setting is kept.
/// Systems on `Res<Time>` stop advancing; real-time UI (menus, star map, options) keeps working.
pub fn toggle_pause(vtime: &mut Time<Virtual>) {
    if vtime.is_paused() { vtime.unpause() } else { vtime.pause() }
}

/// Whether `PROCYON_PROFILE` is set (frame-time logging, and vsync off so the display does not cap the measurement).
fn profiling() -> bool {
    std::env::var_os("PROCYON_PROFILE").is_some()
}

/// Vsync is off while fast-forwarding (more capped steps fit in a second) and whenever profiling (a vsynced frame time only
/// measures the display period).
fn present_mode(profiling: bool, speed: f32) -> bevy::window::PresentMode {
    if profiling || speed > 1.0 { bevy::window::PresentMode::AutoNoVsync } else { bevy::window::PresentMode::AutoVsync }
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
}

fn screenshot(mut commands: Commands, mut cfg: ResMut<Config>) {
    let Some(path) = cfg.shot.clone() else { return };
    cfg.frame += 1;
    let mut shots = vec![(cfg.shot_frame, path.clone()), (cfg.late_frame, path.replace(".png", "_late.png"))];
    if cfg.script.as_deref() == Some("speedtest") {
        for (order, label) in ["stop", "slow", "full", "emergency"].iter().enumerate() {
            shots.push((45 + order as u32 * 90, path.replace(".png", &format!("_{label}.png"))));
        }
    }
    if let Some(end) = cfg.ended {
        shots.push((end + 90, path.replace(".png", "_end.png")));
    }
    for (frame, p) in shots {
        if cfg.frame == frame {
            commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window()).observe(bevy::render::view::screenshot::save_to_disk(p));
        }
    }
    if cfg.ended.is_some_and(|e| cfg.frame >= e + 150) || cfg.frame >= cfg.late_frame.max(cfg.shot_frame) + 100 || (cfg.script.as_deref() == Some("restart") && cfg.frame >= 560) {
        std::process::exit(0);
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
