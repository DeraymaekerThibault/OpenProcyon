//! Screenshot runs (`--shot`, `--shot-frame`, `--late-frame`) and the `Config` the command line fills for a battle.

use crate::{Group, exit, loading, smoke};
use bevy::prelude::*;

#[derive(Resource)]
pub struct Config {
    pub(crate) groups: Vec<Group>, // from the lobby; empty = derive from --ship/--allies/--enemies
    pub(crate) player: String,
    pub(crate) allies: Vec<String>,
    pub(crate) enemies: Vec<String>,
    pub(crate) map: Option<String>,
    pub(crate) shot: Option<String>,
    pub(crate) frame: u32,
    pub(crate) speed: f32,
    pub(crate) ended: Option<u32>,
    pub(crate) script: Option<smoke::Script>,
    /// `--autolaunch`: the lobby fills the fleet and starts the battle (dev/smoke hook).
    pub(crate) autolaunch: bool,
    /// `--shot-frame`: the last requested frame (scripts time themselves off it).
    pub(crate) shot_frame: u32,
    /// Every `--shot-frame` entry, ascending.
    pub(crate) shot_frames: Vec<u32>,
    pub(crate) late_frame: u32,
}

/// `--shot-frame 100,200`: the frames to save, ascending. Nothing parsable (or no flag) means frame 500.
pub(crate) fn parse_shot_frames(arg: Option<&str>) -> Vec<u32> {
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
/// A hidden window (`hide_window`) is never shown, never focused and has no taskbar entry; the game still renders into its surface.
pub(crate) fn shot_window(shooting: bool, hidden: bool) -> Window {
    let window = if shooting {
        Window { resolution: bevy::window::WindowResolution::new(1280, 720).with_scale_factor_override(1.0), resizable: false, ..default() }
    } else {
        Window::default()
    };
    if !hidden {
        return window;
    }
    // bevy_winit creates every window invisible and then applies `visible`, so a hidden window is never mapped (X11) or shown (Windows).
    // `focused: false` is what such a window is (it gets no focus events); `skip_taskbar` is the Windows taskbar entry.
    Window { visible: false, focused: false, skip_taskbar: true, ..window }
}

/// `--hidden` or `PROCYON_HIDDEN=1`: start with the window hidden and unfocused (tool runs that should not appear on the desktop);
/// `PROCYON_HIDDEN=0` switches the flag off. Like `wants_fixed_step`, any other value of the variable leaves the choice to the flag.
pub(crate) fn wants_hidden(args: &[String], env: Option<&str>) -> bool {
    match env {
        Some("0") => false,
        Some("1") => true,
        _ => args.iter().skip(1).any(|a| a == "--hidden"),
    }
}

/// Whether winit 0.30 picks its native Wayland backend: it does whenever `WAYLAND_DISPLAY` or `WAYLAND_SOCKET` is set and not empty,
/// before it looks at `DISPLAY`. Wayland has no hidden windows (winit's `set_visible` does nothing there).
fn native_wayland(wayland_display: Option<&str>, wayland_socket: Option<&str>) -> bool {
    [wayland_display, wayland_socket].into_iter().flatten().any(|v| !v.is_empty())
}

/// Whether this run's window is hidden: wanted (`wants_hidden`) and possible. On native Wayland it is not, so the run warns and opens
/// a normal window; clear `WAYLAND_DISPLAY` to go through X11 (XWayland), where the window can stay hidden.
pub(crate) fn hide_window(args: &[String]) -> bool {
    if !wants_hidden(args, std::env::var("PROCYON_HIDDEN").ok().as_deref()) {
        return false;
    }
    let var = |name| std::env::var(name).ok();
    if cfg!(all(unix, not(target_os = "macos"))) && native_wayland(var("WAYLAND_DISPLAY").as_deref(), var("WAYLAND_SOCKET").as_deref()) {
        eprintln!("warning: --hidden / PROCYON_HIDDEN: native Wayland cannot hide a window, so this run opens a normal one; clear WAYLAND_DISPLAY to use X11");
        return false;
    }
    true
}

/// Settings a hidden window needs to run at full speed. bevy_winit's default `WinitSettings::game()` updates at most 60 times a
/// second while no window has focus, and a hidden window never has it; `continuous()` updates as fast as the frame allows.
pub(crate) fn hidden_plugin(hidden: bool) -> impl Fn(&mut App) {
    move |app: &mut App| {
        if hidden {
            app.insert_resource(bevy::winit::WinitSettings::continuous());
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shot_frames_parse_sort_and_name_their_files() {
        assert_eq!(parse_shot_frames(None), vec![500]);
        assert_eq!(parse_shot_frames(Some("300,100,200,100")), vec![100, 200, 300]);
        assert_eq!(parse_shot_frames(Some("x")), vec![500]);
        assert_eq!(shot_paths("a/x.png", &[300]), vec![(300, "a/x.png".to_string())], "one frame keeps the plain name");
        assert_eq!(shot_paths("a/x.png", &[100, 200]), vec![(100, "a/x_100.png".to_string()), (200, "a/x_200.png".to_string())]);
    }
    #[test]
    fn hidden_follows_the_flag_unless_the_environment_decides() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(!wants_hidden(&args(&["game", "--shot", "f.png"]), None), "a screenshot run is not hidden by itself");
        assert!(wants_hidden(&args(&["game", "--hidden"]), None));
        assert!(!wants_hidden(&args(&["--hidden"]), None), "the program name is not a flag");
        assert!(wants_hidden(&args(&["game", "--mission", "11"]), Some("1")), "the launcher's variable hides any run");
        assert!(!wants_hidden(&args(&["game", "--hidden"]), Some("0")), "the variable can switch it off");
        assert!(wants_hidden(&args(&["game", "--hidden"]), Some("")), "an empty value is no choice");
        assert!(!wants_hidden(&args(&["game"]), Some("yes")));
    }
    #[test]
    fn only_native_wayland_cannot_hide() {
        assert!(!native_wayland(None, None), "X11 or Windows");
        assert!(!native_wayland(Some(""), None), "a cleared WAYLAND_DISPLAY goes to X11 (XWayland)");
        assert!(native_wayland(Some("wayland-0"), None));
        assert!(native_wayland(Some(""), Some("3")), "a Wayland socket alone is Wayland too");
    }
    #[test]
    fn a_hidden_window_is_invisible_unfocused_and_off_the_taskbar_and_keeps_its_size() {
        for shooting in [false, true] {
            let shown = shot_window(shooting, false);
            assert!(shown.visible && shown.focused && !shown.skip_taskbar, "a normal run keeps bevy's defaults");
            let hidden = shot_window(shooting, true);
            assert!(!hidden.visible && !hidden.focused && hidden.skip_taskbar);
            assert_eq!(hidden.resolution, shown.resolution, "pictures keep their size");
            assert_eq!(hidden.resizable, shown.resizable);
        }
        assert_eq!(shot_window(true, true).resolution.physical_size(), UVec2::new(1280, 720));
    }
    #[test]
    fn a_hidden_run_updates_continuously() {
        use bevy::winit::{UpdateMode, WinitSettings};
        let mut app = App::new();
        app.add_plugins(hidden_plugin(false));
        assert!(app.world().get_resource::<WinitSettings>().is_none(), "a normal run keeps bevy_winit's default");
        app.add_plugins(hidden_plugin(true));
        let settings = app.world().resource::<WinitSettings>();
        assert_eq!((settings.focused_mode, settings.unfocused_mode), (UpdateMode::Continuous, UpdateMode::Continuous));
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
}
