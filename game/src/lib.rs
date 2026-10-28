//! Shared game library: simulation, rules, data and input. Skirmish slice: your ship vs enemy ships, original models/data, AI that fights back, damage effects, win/lose, restart.
//! Everything marked PROVISIONAL is a stand-in, not measured original behaviour (see AGENTS.md, rule 5).

pub mod audio;
pub mod bindings;
pub mod blackhole;
pub mod boarding;
pub mod briefing;
pub mod campaign;
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
pub mod flag;
pub mod fx;
pub mod lifeboats;
pub mod menu;
pub mod modding;
pub mod nebula;
pub mod options_menu;
pub mod panels;
pub mod rules;
pub mod scenario;
pub mod setup;
pub mod ship;
pub mod smoke;
pub mod sprites;
pub mod starmap;
pub mod stats;
pub mod victory;
pub mod view;
pub mod wake;

pub use scenario::*;
pub use ship::*;
pub use view::*;

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
/// Files that ship with the code (fonts, hand-written data), layered over the imported assets: `content/` next to the
/// executable (the release download), else `content/` in the checkout.
fn content_dir() -> PathBuf {
    let exe_dir = std::env::current_exe().ok().and_then(|path| path.parent().map(|dir| dir.join("content")));
    content_dir_from(exe_dir, PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../content"))
}
fn content_dir_from(next_to_exe: Option<PathBuf>, checkout: PathBuf) -> PathBuf {
    next_to_exe.filter(|dir| dir.join("fonts").is_dir()).unwrap_or(checkout)
}
/// The asset root: the imported assets with `content/` and any `--mod` folders overlaid on top (see modding.rs).
pub fn assets_dir() -> PathBuf {
    ASSETS.get_or_init(|| modding::overlay(&base_assets(), &[content_dir()])).clone()
}

/// Runs the game with one presentation (a view plugin such as the binary's `classic::plugin`); without imported assets it opens
/// the first-run import screen instead (`setup.rs`, drawn by `setup_view`), which starts the game again once the import is done.
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
    if !base.join("ships").is_dir() {
        eprintln!("No converted game assets in {}: opening the import screen.", base.display());
        return setup::run(base, content_dir(), setup_view);
    }
    let layers: Vec<PathBuf> = std::iter::once(content_dir()).chain(mods).collect();
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
        shot_frame: arg("--shot-frame").and_then(|v| v.parse().ok()).unwrap_or(500),
        late_frame: arg("--late-frame").and_then(|v| v.parse().ok()).unwrap_or(9000), // 0: no _late shot (slow software rendering)
    };
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin { file_path: assets_dir().to_string_lossy().into_owned(), ..default() }))
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
        .add_plugins((lifeboats::plugin, dragons::plugin, cloak::plugin, decals::plugin, stats::plugin, crew_arms::plugin))
        // The presentation (docs/development.md "Layers"): views live outside this library, so shared code cannot reach into them.
        .add_plugins(view)
        .add_plugins(|app: &mut App| {
            if std::env::var_os("PROCYON_PROFILE").is_some() {
                app.add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default()).add_systems(Last, profile_counts);
            }
        })
        // Interactive runs open the front end; scripted/screenshot runs and --map go straight to battle (--menu forces the menu).
        .insert_state(match arg("--menu").as_deref() {
            Some("single") => menu::Screen::SinglePlayer,
            Some("skirmish") => menu::Screen::CreateSkirmish,
            Some("lobby") => menu::Screen::Lobby,
            Some("options") => menu::Screen::Options,
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
        .add_plugins(smoke::plugin)
        .add_systems(OnEnter(menu::Screen::Battle), enter_battle)
        .add_systems(OnExit(menu::Screen::Battle), scenario::reset_fleets)
        .add_systems(
            Update,
            (
                (
                    (shot_target, restart, debug_kill).chain(),
                    briefing::update,
                    ship::reconcile_fleets,
                    view::fleet_keys,
                    input,
                    ai,
                    ship::fleet_follow,
                    blackhole::pull,
                    sail,
                    toggle_debug,
                    camera,
                    sky,
                    wake::update,
                    blackhole::update,
                    course::update,
                    outcome,
                    hud,
                    leave_battle,
                )
                    .chain()
                    .run_if(in_state(menu::Screen::Battle))
                    .run_if(menu::overlay_closed),
                screenshot,
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

/// Largest simulated step per frame: `sail`/`environment` clamp dt to this, so bigger steps would desync motion from rule timers.
const MAX_SIM_STEP: f32 = 0.1;

/// Fast-forward for testing: scale virtual time; Bevy's max_delta clamps the scaled step so it never exceeds MAX_SIM_STEP.
/// At low frame rates the effective speed drops below `speed` instead of taking unsafe steps.
fn set_time_scale(vtime: &mut Time<Virtual>, speed: f32) {
    let speed = speed.max(0.01);
    vtime.set_relative_speed(speed);
    vtime.set_max_delta(std::time::Duration::from_secs_f32(if speed > 1.0 { MAX_SIM_STEP } else { 0.25 }));
}

pub const MAX_GAME_SPEED: f32 = 16.0;

/// Freezes or resumes the simulation by pausing virtual time (the HUD pause button and non-retail Pause key). The speed setting is kept.
/// Systems on `Res<Time>` stop advancing; real-time UI (menus, star map, options) keeps working.
pub fn toggle_pause(vtime: &mut Time<Virtual>) {
    if vtime.is_paused() { vtime.unpause() } else { vtime.pause() }
}

/// Testing fast-forward; vsync is off while fast so more capped steps fit in a second.
pub fn set_game_speed(vtime: &mut Time<Virtual>, window: &mut Window, speed: f32) {
    set_time_scale(vtime, speed);
    window.present_mode = if speed > 1.0 { bevy::window::PresentMode::AutoNoVsync } else { bevy::window::PresentMode::AutoVsync };
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
    mut last: Local<(f32, bool)>,
) {
    let (mission_time, objective, ended) = mission.as_ref().map_or((0.0, None, false), |m| (m.script.time, m.script.point, m.script.ended.is_some()));
    if time.elapsed_secs() - last.0 < 5.0 && ended == last.1 {
        return;
    }
    *last = (time.elapsed_secs(), ended);
    let frame_ms = diagnostics.get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FRAME_TIME).and_then(|d| d.average()).unwrap_or(0.0);
    let particles: usize = effects.iter().map(fx::Fx::particle_count).sum();
    info!(
        "profile real={:.1} mission={mission_time:.1} objective={objective:?} ended={ended} frame_ms={frame_ms:.2} entities={} ships={} rocks={} effects={} particles={particles} mesh_assets={} audio={}",
        time.elapsed_secs(),
        entities.iter().count(),
        ships.iter().count(),
        rocks.iter().count(),
        effects.iter().count(),
        meshes.len(),
        audio.iter().count()
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_next_to_the_executable_wins_over_the_checkout() {
        let dir = std::env::temp_dir().join(format!("procyon-content-test-{}", std::process::id()));
        let (unpacked, checkout) = (dir.join("download/content"), dir.join("checkout/content"));
        assert_eq!(content_dir_from(Some(unpacked.clone()), checkout.clone()), checkout, "no content/ next to a cargo-built binary");
        std::fs::create_dir_all(unpacked.join("fonts")).unwrap();
        assert_eq!(content_dir_from(Some(unpacked.clone()), checkout.clone()), unpacked);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
