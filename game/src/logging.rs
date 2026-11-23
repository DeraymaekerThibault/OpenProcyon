//! Log file for bug reports: everything the console shows also goes to `<user dir>/logs/openprocyon.log` (the run before is kept as
//! `openprocyon.previous.log`), starting with a header (version, system, arguments) and ending with any panic.
use bevy::log::{
    BoxedLayer, DEFAULT_FILTER, LogPlugin,
    tracing_subscriber::{self, Layer},
};
use bevy::prelude::*;
use std::{fs, path::PathBuf, sync::Mutex};

/// Empty dynamic meshes (course line, wake, bursts, ...) are normal before anything is drawn into them: one warning per mesh per frame.
const QUIET: &str = "bevy_render::mesh::allocator=error";

pub fn plugin() -> LogPlugin {
    LogPlugin { filter: format!("{DEFAULT_FILTER}{QUIET}"), custom_layer: file_layer, ..default() }
}

/// Where testers find the log (also named in the docs and the crash screen).
pub fn log_path() -> Option<PathBuf> {
    crate::campaign::user_dir().map(|d| d.join("logs").join("openprocyon.log"))
}

/// A file that, once `CAP` bytes are written, becomes `openprocyon.previous.log` and starts again: at most about twice `CAP` on disk,
/// and the newest lines (a crash) are always kept.
const CAP: u64 = 20 << 20;

struct Rotating {
    path: PathBuf,
    file: fs::File,
    written: u64,
}

impl Rotating {
    fn open(path: PathBuf) -> std::io::Result<Self> {
        let _ = fs::rename(&path, path.with_file_name("openprocyon.previous.log"));
        Ok(Self { file: fs::File::create(&path)?, path, written: 0 })
    }
}

impl std::io::Write for Rotating {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.written >= CAP
            && let Ok(next) = Self::open(self.path.clone())
        {
            *self = next;
        }
        self.written += buf.len() as u64;
        self.file.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// The command line for a log or crash report: flags and plain values stay, anything that looks like a path (it can hold a
/// user name) becomes `<path>`, also after `--flag=`.
pub(crate) fn redacted_args() -> Vec<String> {
    std::env::args().skip(1).map(|a| redact_arg(&a)).collect()
}
/// A panic location without the build machine's folders (a dependency sits under the user's home): from `src/` on.
pub(crate) fn trim_source_path(file: &str) -> &str {
    file.rfind("/src/").or_else(|| file.rfind("\\src\\")).map_or(file, |i| &file[i + 1..])
}
fn redact_arg(arg: &str) -> String {
    let hide = |v: &str| if v.contains('/') || v.contains('\\') { "<path>".to_owned() } else { v.to_owned() };
    match arg.split_once('=') {
        Some((flag, v)) => format!("{flag}={}", hide(v)),
        None => hide(arg),
    }
}

fn file_layer(_: &mut App) -> Option<BoxedLayer> {
    let path = log_path()?;
    fs::create_dir_all(path.parent()?).ok()?;
    let mut file = Rotating::open(path).ok()?;
    let args = redacted_args();
    use std::io::Write;
    let _ = writeln!(file, "OpenProcyon {} on {} {}\narguments: {}\n", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH, args.join(" "));
    // Panics bypass tracing: log them too, then hand on to the previous hook (the crash screen's, if installed).
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        error!("panic: {info}");
        previous(info);
    }));
    Some(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(Mutex::new(file)).boxed())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn paths_in_arguments_are_hidden() {
        let hidden: Vec<_> = ["--mission", "3", "/home/me/g", "--mod=C:\\Users\\me\\m", "--map=red_rover"].map(redact_arg).into();
        assert_eq!(hidden, ["--mission", "3", "<path>", "--mod=<path>", "--map=red_rover"]);
    }

    #[test]
    fn panic_locations_start_at_src() {
        assert_eq!(trim_source_path("/home/me/.cargo/registry/x-1.0/src/lib.rs"), "src/lib.rs");
        assert_eq!(trim_source_path("game/src/ship.rs"), "src/ship.rs");
        assert_eq!(trim_source_path("C:\\Users\\me\\x\\src\\a.rs"), "src\\a.rs");
    }

    #[test]
    fn full_log_becomes_previous_and_newest_lines_are_kept() {
        let dir = std::env::temp_dir().join(format!("procyon-log-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut log = Rotating::open(dir.join("openprocyon.log")).unwrap();
        log.write_all(b"old\n").unwrap();
        log.written = CAP;
        log.write_all(b"new\n").unwrap();
        assert_eq!(fs::read_to_string(dir.join("openprocyon.previous.log")).unwrap(), "old\n");
        assert_eq!(fs::read_to_string(dir.join("openprocyon.log")).unwrap(), "new\n");
        let _ = fs::remove_dir_all(dir);
    }
}
