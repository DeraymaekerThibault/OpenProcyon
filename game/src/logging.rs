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

fn file_layer(_: &mut App) -> Option<BoxedLayer> {
    let path = log_path()?;
    fs::create_dir_all(path.parent()?).ok()?;
    let mut file = Rotating::open(path).ok()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
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
