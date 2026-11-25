//! Game time: the fixed battle tick, the speed setting, pause, and the fixed-step mode screenshots and traces run in.

use crate::*;

/// `--fixed-step` / `PROCYON_FIXED_STEP=1` (#535): every frame advances exactly one battle tick of game time at 1x, whatever the
/// machine's frame rate, and the clock-seeded cosmetic draws (ship names, stars) use a fixed seed. With the battle seed (`--seed`)
/// and frames counted after loading, the same scripted run gives the same pictures and trace on every run. The crew stays silent
/// (its lines follow audio playback, not the tick).
static FIXED_STEP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(crate) fn set_fixed_step(on: bool) {
    FIXED_STEP.store(on, std::sync::atomic::Ordering::Relaxed);
}
pub fn fixed_step() -> bool {
    FIXED_STEP.load(std::sync::atomic::Ordering::Relaxed)
}
/// Whether this launch runs fixed-step: `--fixed-step` or `PROCYON_FIXED_STEP=1` ask for it, and so does `--shot` (a picture must not
/// depend on the machine's frame times: with the wall-clock step two runs of one command gave different pictures). `PROCYON_FIXED_STEP=0`
/// switches it off, also for `--shot`. The battle seed needs no such rule: it is `SimRng::DEFAULT_SEED` unless `--seed` / `PROCYON_SEED`
/// say otherwise, which stay in force under `--shot`.
pub fn wants_fixed_step(args: &[String], env: Option<&str>) -> bool {
    match env {
        Some("0") => false,
        Some("1") => true,
        _ => args.iter().skip(1).any(|a| a == "--fixed-step" || a == "--shot"),
    }
}
/// A seed from the clock, or a constant in a fixed-step run.
pub fn clock_seed() -> u64 {
    if fixed_step() {
        return 0x9E37_79B9;
    }
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64)
}
/// Retail tick documented in docs/re/gunnery.md and docs/re/effects.md.
pub const BATTLE_TICK: std::time::Duration = std::time::Duration::from_millis(33);
/// The same tick in seconds, for rules the original writes per tick (rates, pulls, chances): one tick is 0.033 s, not 1/30 s.
pub const TICK_SECS: f32 = 0.033;

/// Preserve the virtual-time catch-up cap; fixed ticks now advance motion and timers together.
/// At low frame rates the effective speed drops below `speed` instead of taking unbounded catch-up steps.
pub(crate) fn set_time_scale(vtime: &mut Time<Virtual>, speed: f32) {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiling_turns_vsync_off_at_every_speed() {
        use bevy::window::PresentMode::{AutoNoVsync, AutoVsync};
        assert_eq!(present_mode(false, 1.0), AutoVsync);
        assert_eq!(present_mode(false, 4.0), AutoNoVsync);
        assert_eq!(present_mode(true, 1.0), AutoNoVsync);
        assert_eq!(present_mode(true, 4.0), AutoNoVsync);
    }
}

#[cfg(test)]
mod fixed_step_tests {
    use super::wants_fixed_step;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_shot_runs_fixed_step_unless_switched_off() {
        assert!(!wants_fixed_step(&args(&["game", "--map", "red_rover"]), None), "an interactive run follows the clock");
        assert!(wants_fixed_step(&args(&["game", "--map", "red_rover", "--shot", "f.png"]), None));
        assert!(wants_fixed_step(&args(&["game", "--fixed-step"]), None));
        assert!(wants_fixed_step(&args(&["game"]), Some("1")));
        assert!(!wants_fixed_step(&args(&["game", "--shot", "f.png"]), Some("0")), "the environment can switch it off");
        assert!(!wants_fixed_step(&args(&["game", "--fixed-step"]), Some("0")));
        assert!(wants_fixed_step(&args(&["game", "--shot", "f.png"]), Some("")), "an empty value is no choice");
    }
}
