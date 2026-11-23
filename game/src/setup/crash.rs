//! The crash report a panic writes and the crash screen's messages.
use super::*;

/// For a player's game: a panic writes `dir/crash.log` and opens the crash screen (this app with `--crashed`) in a new process, so
/// the error is seen instead of the window just closing. Only the game installs it, never the setup app, so it cannot loop.
pub fn report_crashes(dir: PathBuf) {
    let default = std::panic::take_hook();
    let reported = std::sync::atomic::AtomicBool::new(false);
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        if reported.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return; // one report: the first panic, not the threads that fail after it
        }
        let log = dir.join("crash.log");
        if std::fs::write(&log, crash_report(info, &std::backtrace::Backtrace::force_capture().to_string())).is_err() {
            return;
        }
        let args = game_args(std::env::args().skip(1));
        if let Ok(binary) = std::env::current_exe() {
            let _ = Command::new(binary).args(args).arg("--crashed").arg(&log).spawn();
        }
    }));
}

fn crash_report(info: &std::panic::PanicHookInfo, backtrace: &str) -> String {
    let message = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
    let place = info.location().map_or(String::new(), |l| format!("{}:{}", l.file(), l.line()));
    let args = crate::logging::redacted_args();
    format!(
        "OpenProcyon {} crashed. Please attach this file when you report the problem.\nmessage: {}\nat: {place}\nsystem: {} {}\narguments: {}\n\n{backtrace}\n",
        env!("CARGO_PKG_VERSION"),
        message.replace('\n', " "),
        std::env::consts::OS,
        std::env::consts::ARCH,
        args.join(" ")
    )
}

/// Why an older import has to be converted again, for the screen.
pub(super) fn outdated_reason(version: Option<&str>) -> String {
    let now = env!("CARGO_PKG_VERSION");
    let by = version.map_or("an older version of OpenProcyon".to_owned(), |v| format!("OpenProcyon {v}"));
    format!("Your game was converted by {by}. OpenProcyon {now} needs it converted again: click Import (a few minutes). Only the converted files are replaced.")
}

/// The crash screen's message: the report's panic line (`message` in `crash::report`).
pub(super) fn crash_message(log: &Path) -> String {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    text.lines().find_map(|l| l.strip_prefix("message: ")).unwrap_or("unknown error").to_owned()
}
