//! The ways out of the process, so exit codes live in one place.
//!
//! Before the app exists (command-line checks, a refused `--script`, missing assets) `quit` ends the process after flushing the
//! output. Once it runs, a system asks to stop with an `AppExit` (`app_exit`, `request`): the frame ends, `Last` systems and the
//! log writers run, and `status` turns the result back into the process's exit code.

use bevy::app::AppExit;
use bevy::prelude::*;

/// Ends the process at once with `code`, after flushing stdout and stderr. For use before the app runs.
pub fn quit(code: i32) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(code)
}

/// The `AppExit` for a status code: 0 is success, anything else an error carrying the code.
pub fn app_exit(code: u8) -> AppExit {
    AppExit::from_code(code)
}

/// Asks a running app to stop with `code` (for exclusive systems; other systems write `app_exit(code)` to a `MessageWriter<AppExit>`).
pub fn request(world: &mut World, code: u8) {
    world.write_message(app_exit(code));
}

/// Logs `run end: frame N` (the frames the app ran, loading included) once, on the frame a system asks to stop. The tools read
/// it for the frame rate of a run (tools/rungame.py).
pub fn plugin(app: &mut App) {
    app.add_systems(Last, log_run_end);
}

fn log_run_end(mut exits: MessageReader<AppExit>, frames: Option<Res<bevy::diagnostic::FrameCount>>, mut logged: Local<bool>) {
    if exits.read().next().is_some() && !std::mem::replace(&mut *logged, true) {
        info!("run end: frame {}", frames.map_or(0, |f| f.0));
    }
}

/// The process exit code of a finished app.
pub fn status(result: &AppExit) -> i32 {
    match result {
        AppExit::Success => 0,
        AppExit::Error(code) => i32::from(code.get()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_survive_the_round_trip_through_app_exit() {
        for code in [0u8, 1, 2, 255] {
            assert_eq!(status(&app_exit(code)), i32::from(code));
        }
        assert_eq!(app_exit(0), AppExit::Success);
    }

    #[test]
    fn a_request_reaches_the_app() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_systems(Update, |world: &mut World| request(world, 2));
        let result = app.run();
        assert_eq!(status(&result), 2);
    }
}
