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
pub(crate) fn shot_window(shooting: bool) -> Window {
    if !shooting {
        return Window::default();
    }
    Window { resolution: bevy::window::WindowResolution::new(1280, 720).with_scale_factor_override(1.0), resizable: false, ..default() }
}

/// What a screenshot run does on a frame (`run_end`).
#[derive(Debug, PartialEq)]
enum RunEnd {
    /// The simulation goes on.
    Play,
    /// The run's last simulated frame has passed: game time stands still until the pictures are on disk.
    Wait,
    /// Quit now.
    Exit,
}

/// The state of a screenshot run on `frame`. The simulation stops at a frame fixed by the arguments and the battle, never by
/// file-write timing, so one command gives a trace of one length on every run: the last picture's frame, then for a scripted
/// run `SCRIPT_TAIL` frames more, because smoke scripts assert after their picture (`pause` at frame 70, `fleet` up to 130).
/// After that the run waits, game time still, until every requested picture is on disk (`saved` counts finished writes). A
/// battle that ended stops 150 frames later and the `restart` script at frame 560, whatever the pictures.
fn run_end(frame: u32, last_shot: u32, saved: usize, wanted: usize, scripted: bool, ended: Option<u32>, restart: bool) -> RunEnd {
    const SCRIPT_TAIL: u32 = 100;
    if ended.is_some_and(|e| frame >= e + 150) || (restart && frame >= 560) {
        return RunEnd::Exit;
    }
    match (frame >= last_shot + if scripted { SCRIPT_TAIL } else { 0 }, saved >= wanted) {
        (false, _) => RunEnd::Play,
        (true, false) => RunEnd::Wait,
        (true, true) => RunEnd::Exit,
    }
}

static SHOTS_SAVED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub(crate) fn screenshot(
    mut commands: Commands,
    mut cfg: ResMut<Config>,
    loading: Option<Res<loading::Loading>>,
    mut vtime: ResMut<Time<Virtual>>,
    mut exit: MessageWriter<AppExit>,
) {
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
    let end = run_end(cfg.frame, last, saved, shots.len(), cfg.script.is_some(), cfg.ended, cfg.script.as_deref() == Some("restart"));
    if end != RunEnd::Play && !vtime.is_paused() {
        // This frame was the last one simulated (its time step is already taken); from the next one game time stands still.
        vtime.pause();
    }
    if end == RunEnd::Exit {
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
    fn a_run_ends_once_pictures_are_saved() {
        use RunEnd::{Exit, Play, Wait};
        assert_eq!(run_end(299, 300, 0, 1, false, None, false), Play);
        assert_eq!(run_end(300, 300, 0, 1, false, None, false), Wait, "the last picture's frame is the last simulated");
        assert_eq!(run_end(303, 300, 1, 1, false, None, false), Exit, "plain run stops once the picture is saved");
        assert_eq!(run_end(350, 300, 1, 1, true, None, false), Play, "scripts keep asserting after their picture");
        assert_eq!(run_end(400, 300, 1, 1, true, None, false), Exit);
        assert_eq!(run_end(450, 300, 1, 2, false, None, false), Wait, "second picture missing");
        assert_eq!(run_end(560, 300, 0, 1, true, None, true), Exit, "restart script");
        assert_eq!(run_end(250, 900, 0, 1, true, Some(100), false), Exit, "battle ended long ago");
    }
    /// Two runs of one command once ended a tick apart: the simulation ran until the picture's write finished. The frames
    /// simulated (up to and including the first frame that is not `Play`) must not depend on when the write lands.
    #[test]
    fn simulated_frames_do_not_depend_on_write_timing() {
        for scripted in [false, true] {
            let mut lengths = vec![];
            for write_takes in [1, 2, 3, 7, 150] {
                let saved = |frame: u32| usize::from(frame >= 300 + write_takes);
                let last_simulated = (1..).find(|&f| run_end(f, 300, saved(f), 1, scripted, None, false) != RunEnd::Play).unwrap();
                let exit = (1..).find(|&f| run_end(f, 300, saved(f), 1, scripted, None, false) == RunEnd::Exit).unwrap();
                assert!(saved(exit) == 1, "exits only once the picture is on disk");
                lengths.push(last_simulated);
            }
            assert!(lengths.iter().all(|&n| n == lengths[0]), "scripted {scripted}: last simulated frames {lengths:?}");
        }
    }
}
