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
    LogPlugin { filter: format!("{DEFAULT_FILTER}{QUIET}"), custom_layer: file_layer, fmt_layer: console_layer, ..default() }
}

/// Bevy's console layer, with repeated lines collapsed (a failing audio device logs one `rodio::stream` error per poll, #561).
fn console_layer(_: &mut App) -> Option<bevy::log::BoxedFmtLayer> {
    Some(Box::new(tracing_subscriber::fmt::Layer::default().with_writer(Mutex::new(Dedup::new(std::io::stderr())))))
}

/// How often a run of identical lines is summed up while it goes on.
const REPEAT_NOTE: std::time::Duration = std::time::Duration::from_secs(5);

/// Writes a line only when it differs from the line before (the timestamp, up to the first space, is ignored); a run of
/// repeats becomes one `previous line repeated N times` when it ends, and every `REPEAT_NOTE` while it lasts.
/// One `write` is one formatted event (the fmt layer writes each event in one buffer).
struct Dedup<W> {
    out: W,
    last: Vec<u8>,
    repeats: u64,
    since: std::time::Instant,
}

impl<W: std::io::Write> Dedup<W> {
    fn new(out: W) -> Self {
        Self { out, last: Vec::new(), repeats: 0, since: std::time::Instant::now() }
    }
    fn note(&mut self) -> std::io::Result<()> {
        if self.repeats > 0 {
            writeln!(self.out, "previous line repeated {} times", self.repeats)?;
        }
        self.repeats = 0;
        self.since = std::time::Instant::now();
        Ok(())
    }
}

impl<W: std::io::Write> std::io::Write for Dedup<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let key = buf.iter().position(|&b| b == b' ').map_or(buf, |i| &buf[i..]);
        if key == self.last.as_slice() {
            self.repeats += 1;
            if self.since.elapsed() >= REPEAT_NOTE {
                self.note()?;
            }
            return Ok(buf.len());
        }
        self.note()?;
        self.last = key.to_vec();
        self.out.write_all(buf)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
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
        if self.written >= 2 * CAP {
            return Ok(buf.len()); // rotation keeps failing: drop lines rather than fill the disk
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
    Some(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(Mutex::new(Dedup::new(file))).boxed())
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
    fn a_flood_of_one_line_is_collapsed() {
        let mut log = Dedup::new(Vec::new());
        log.write_all(b"t0 INFO start\n").unwrap();
        for i in 0..100_000 {
            log.write_all(format!("t{i} ERROR rodio::stream: audio stream error: POLLERR\n").as_bytes()).unwrap();
        }
        log.write_all(b"t9 INFO next\n").unwrap();
        let text = String::from_utf8(log.out).unwrap();
        assert_eq!(text, "t0 INFO start\nt0 ERROR rodio::stream: audio stream error: POLLERR\nprevious line repeated 99999 times\nt9 INFO next\n");
    }

    /// #561 end to end: the rodio error once per audio poll, through the real fmt layer (one write per event, timestamps differ).
    #[test]
    fn a_failing_audio_device_keeps_the_log_small() {
        #[derive(Clone, Default)]
        struct Shared(std::sync::Arc<Mutex<Vec<u8>>>);
        impl Write for Shared {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        use tracing_subscriber::layer::SubscriberExt;
        let out = Shared::default();
        let layer = tracing_subscriber::fmt::layer().with_ansi(false).with_writer(Mutex::new(Dedup::new(out.clone())));
        bevy::log::tracing::subscriber::with_default(tracing_subscriber::registry().with(layer), || {
            for _ in 0..200_000 {
                error!(target: "rodio::stream", "audio stream error: alsa::poll() returned POLLERR");
            }
            info!("next");
        });
        let text = String::from_utf8(out.0.lock().unwrap().clone()).unwrap();
        assert!(text.len() < 400, "{} bytes: {text}", text.len());
        assert!(text.contains("previous line repeated 199999 times"), "{text}");
    }

    #[test]
    fn a_long_flood_is_summed_up_while_it_lasts() {
        let mut log = Dedup::new(Vec::new());
        log.write_all(b"t0 x\n").unwrap();
        log.write_all(b"t1 x\n").unwrap();
        log.since -= REPEAT_NOTE;
        log.write_all(b"t2 x\n").unwrap();
        log.write_all(b"t3 x\n").unwrap();
        assert_eq!(String::from_utf8(log.out).unwrap(), "t0 x\nprevious line repeated 2 times\n");
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

    #[test]
    fn nothing_is_written_past_twice_the_cap_when_rotation_fails() {
        let dir = std::env::temp_dir().join(format!("procyon-cap-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let kept = dir.join("kept.log");
        let mut log = Rotating { path: dir.join("missing/openprocyon.log"), file: fs::File::create(&kept).unwrap(), written: 2 * CAP };
        log.write_all(b"dropped\n").unwrap();
        assert_eq!(fs::read_to_string(&kept).unwrap(), "");
        let _ = fs::remove_dir_all(dir);
    }
}
