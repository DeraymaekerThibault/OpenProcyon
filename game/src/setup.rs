//! First run: with no imported assets the game opens an "Import your game" screen instead of the menu. It finds the install
//! (Steam libraries, disc installs, or a typed/pasted/dropped folder), runs the importer as a child process (read-only on the install) and shows
//! its `[n/total] step` lines as progress. On success the game starts again: plugins and `Startup` systems read the assets while
//! the app is built and the asset root is fixed for the process (`ASSETS`, `AssetPlugin`), and winit cannot open a second event
//! loop in one process, so the full game cannot be built in place of this small app; a fresh process with the same arguments is
//! the clean way to continue to the main menu. State and input live here; the view (`classic/setup.rs`) only draws.
#![allow(deprecated)] // shortcut: bevy 0.20 Interaction (deprecated for picking Hovered/Pressed); port when it is removed

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::window::FileDragAndDrop;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};

/// The install folder's name inside a Steam library.
pub const STEAM_DIR: &str = "Treasure Planet Battle at Procyon";
/// Log lines kept for the error view.
const LOG_LINES: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// Picking the install folder.
    Choose,
    /// The importer runs; `step` of `total` (0 before its first line) is `name`; `percent` of the whole import is done when the
    /// importer says (older importers do not).
    Importing { step: u32, total: u32, name: String, percent: Option<u32> },
    /// The importer failed (or could not start): the failing step when it said which, and why.
    Failed { step: Option<String>, reason: String },
    /// Import done: the game starts again with the assets.
    Starting,
    /// The game crashed (`--crashed <log>`): what it said, and where the report is.
    Crashed { message: String, log: PathBuf },
}

/// Written into the asset folder after a successful import from this screen: the game version that imported it and the install
/// it came from. A player's folder (`%APPDATA%`/XDG) is only used when it matches this build (`imported`).
pub const STAMP: &str = "import.json";

/// Why the import screen opened instead of the game.
#[derive(Clone, Debug, PartialEq)]
pub enum Imported {
    Ready,
    /// No converted assets yet.
    Missing,
    /// Converted by another version (`None`: before imports were stamped); `game` is the install it came from.
    Outdated {
        version: Option<String>,
        game: Option<PathBuf>,
    },
}

/// The state of a player's asset folder for game version `version`: every release converts the game again.
pub fn imported(dir: &Path, version: &str) -> Imported {
    if !dir.join("ships").is_dir() {
        return Imported::Missing;
    }
    let stamp: serde_json::Value = std::fs::read_to_string(dir.join(STAMP)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let found = stamp["version"].as_str().map(String::from);
    if found.as_deref() == Some(version) {
        return Imported::Ready;
    }
    Imported::Outdated { version: found, game: stamp["game"].as_str().map(PathBuf::from) }
}

pub fn write_stamp(dir: &Path, version: &str, game: &Path) -> std::io::Result<()> {
    let stamp = serde_json::json!({ "version": version, "game": game.display().to_string() });
    std::fs::write(dir.join(STAMP), format!("{stamp:#}\n"))
}

/// The folder a player's import is written to before it replaces `out`, so a failed or cancelled import keeps the old one.
pub fn staging(out: &Path) -> PathBuf {
    out.with_file_name(format!("{}.importing", out.file_name().map_or("assets".into(), |n| n.to_string_lossy())))
}

/// Puts the finished import `staged` in place of `out` (the old folder is removed only once the new one is in place).
pub fn swap_in(staged: &Path, out: &Path) -> std::io::Result<()> {
    let old = out.with_file_name(format!("{}.replaced", out.file_name().map_or("assets".into(), |n| n.to_string_lossy())));
    if old.exists() {
        std::fs::remove_dir_all(&old)?;
    }
    if out.exists() {
        retry(|| std::fs::rename(out, &old))?;
    }
    if let Err(err) = retry(|| std::fs::rename(staged, out)) {
        let _ = std::fs::rename(&old, out);
        return Err(err);
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Windows refuses to rename a folder while a virus scanner or the indexer briefly holds a file in it.
fn retry(mut f: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    let mut tries = 0;
    loop {
        match f() {
            Err(_) if tries < 20 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            done => return done,
        }
    }
}

/// The setup screen's input and importer polling; views draw after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SetupInput;

/// What a setup button does (the view tags its buttons with this).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub enum SetupAction {
    /// Use the detected install with this index.
    Pick(usize),
    Paste,
    Import,
    /// Back to the folder field after a failure or crash.
    Change,
    /// Show the crash report in the system's text viewer.
    OpenLog,
    Quit,
}

enum Msg {
    Line(String),
    Exit(Result<(), String>),
    Paste(String),
}

#[derive(Resource)]
pub struct Setup {
    /// The install folder field.
    pub path: String,
    /// Installs found on this machine.
    pub found: Vec<PathBuf>,
    /// `path` checked: the install folder, or what is wrong with it.
    pub check: Result<PathBuf, String>,
    /// Where the assets go (the folder the game reads, `base_assets()`).
    pub out: PathBuf,
    pub phase: Phase,
    /// The importer's output, newest last.
    pub log: Vec<String>,
    /// Which importer runs (frozen next to the game, or Python from the checkout).
    pub importer: String,
    /// Why the screen opened when there are assets already (an older import); shown above the folder field.
    pub reason: Option<String>,
    /// A player's folder: the import goes to `staging(out)` and replaces `out` only when it succeeded.
    pub staged: bool,
    /// The install being imported (recorded in the stamp).
    from: Option<PathBuf>,
    tx: mpsc::Sender<Msg>,
    rx: Mutex<mpsc::Receiver<Msg>>,
    child: Option<Arc<Mutex<Child>>>,
}

impl Setup {
    pub fn new(out: PathBuf, path: Option<String>) -> Self {
        let found = find_installs();
        let path = path.or_else(|| found.first().map(|p| p.display().to_string())).unwrap_or_default();
        let (tx, rx) = mpsc::channel();
        Setup {
            check: check_install(&path),
            path,
            found,
            out,
            phase: Phase::Choose,
            log: vec![],
            importer: String::new(),
            reason: None,
            staged: false,
            from: None,
            tx,
            rx: Mutex::new(rx),
            child: None,
        }
    }

    /// Where this import writes.
    fn target(&self) -> PathBuf {
        if self.staged { staging(&self.out) } else { self.out.clone() }
    }

    /// After the importer exited cleanly: stamp the import and, for a player's folder, put it in place of the old one.
    fn finish(&self) -> Result<(), String> {
        let target = self.target();
        if !target.join("ships").is_dir() {
            return Err(format!("The importer finished but wrote no ships/ to {}.", target.display()));
        }
        write_stamp(&target, env!("CARGO_PKG_VERSION"), self.from.as_deref().unwrap_or(Path::new("")))
            .map_err(|err| format!("Could not write {} ({err}).", target.join(STAMP).display()))?;
        if self.staged {
            swap_in(&target, &self.out).map_err(|err| {
                format!("The import finished in {}, but it could not replace {} ({err}). Close other programs using that folder and retry.", target.display(), self.out.display())
            })?;
        }
        Ok(())
    }

    pub fn set_path(&mut self, path: String) {
        self.check = check_install(&path);
        self.path = path;
    }

    /// Starts the importer on the checked folder (no-op while it runs or when the folder is wrong).
    pub fn start(&mut self) {
        let Ok(game) = self.check.clone() else { return };
        if matches!(self.phase, Phase::Importing { .. } | Phase::Starting) {
            return;
        }
        self.log.clear();
        let target = self.target();
        if self.staged
            && target.exists()
            && let Err(err) = std::fs::remove_dir_all(&target)
        {
            // A stopped earlier import; it never replaced the assets in use.
            self.phase = Phase::Failed { step: None, reason: format!("Could not clear the unfinished import in {} ({err}).", target.display()) };
            return;
        }
        match spawn_import(&game, &target, self.tx.clone()) {
            Ok((label, child)) => {
                self.importer = label;
                self.child = Some(child);
                self.from = Some(game);
                self.phase = Phase::Importing { step: 0, total: 0, name: "starting the importer".into(), percent: None };
            }
            Err(reason) => self.phase = Phase::Failed { step: None, reason },
        }
    }

    /// Applies the importer's output and exit; true when anything changed.
    fn poll(&mut self) -> bool {
        let msgs: Vec<Msg> = self.rx.lock().map(|rx| rx.try_iter().collect()).unwrap_or_default();
        let changed = !msgs.is_empty();
        for msg in msgs {
            match msg {
                Msg::Paste(text) => {
                    if self.phase == Phase::Choose {
                        self.set_path(text.lines().next().unwrap_or("").trim().to_owned());
                    }
                }
                Msg::Line(line) => {
                    if let (Some((step, total, name, percent)), Phase::Importing { .. }) = (parse_progress(&line), &self.phase) {
                        self.phase = Phase::Importing { step, total, name: name.to_owned(), percent };
                    }
                    self.log.push(line);
                    if self.log.len() > LOG_LINES {
                        self.log.remove(0);
                    }
                }
                Msg::Exit(result) => {
                    self.child = None;
                    let last = match &self.phase {
                        Phase::Importing { step, name, .. } if *step > 0 => Some(name.clone()),
                        _ => None,
                    };
                    self.phase = match result {
                        Ok(()) => match self.finish() {
                            Ok(()) => Phase::Starting,
                            Err(reason) => Phase::Failed { step: last, reason },
                        },
                        Err(reason) => Phase::Failed { step: self.log.iter().rev().find_map(|l| failed_step(l)).or(last), reason },
                    };
                }
            }
        }
        changed
    }
}

impl Drop for Setup {
    /// Closing the window stops a running import.
    fn drop(&mut self) {
        if let Some(child) = self.child.take()
            && let Ok(mut child) = child.lock()
        {
            let _ = child.kill();
        }
    }
}

/// `[n/total] step` from the importer, optionally `[n/total] step: i/k (p%)` (a part of the step; p is the whole import).
pub fn parse_progress(line: &str) -> Option<(u32, u32, &str, Option<u32>)> {
    let (count, name) = line.trim().strip_prefix('[')?.split_once("] ")?;
    let (n, total) = count.split_once('/')?;
    let (n, total) = (n.parse().ok()?, total.parse().ok()?);
    let name = name.trim();
    let percent = name.strip_suffix("%)").and_then(|s| s.rsplit_once(" (")).and_then(|(rest, p)| Some((rest.trim_end(), p.parse::<u32>().ok()?.min(100))));
    let (name, percent) = percent.map_or((name, None), |(rest, p)| (rest, Some(p)));
    (n >= 1 && n <= total).then_some((n, total, name, percent))
}

/// The step name of the importer's `import failed at step 'name': ...` line.
pub fn failed_step(line: &str) -> Option<String> {
    let rest = line.split_once("import failed at step ")?.1;
    let quote = rest.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    Some(rest[1..].split(quote).next()?.to_owned())
}

/// The game folder in a typed/pasted/dropped path: the folder with `WorldObjects/`. A dropped file (the game's program) means its
/// folder; a Steam library or its `steamapps/common` finds the game inside.
pub fn check_install(input: &str) -> Result<PathBuf, String> {
    let text = input.trim().trim_matches('"').trim();
    if text.is_empty() {
        return Err("Type or paste the folder you installed the game in, or drop it on this window.".into());
    }
    let mut path = match text.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map_or_else(|| PathBuf::from(text), |h| PathBuf::from(h).join(rest)),
        None => PathBuf::from(text),
    };
    if path.is_file() {
        path.pop();
    }
    if !path.is_dir() {
        return Err(format!("There is no folder {}.", path.display()));
    }
    for candidate in [path.clone(), path.join(STEAM_DIR), path.join("common").join(STEAM_DIR), path.join("steamapps").join("common").join(STEAM_DIR)] {
        if is_install(&candidate) {
            return Ok(candidate);
        }
    }
    Err(format!("{} is not the game folder: it has no WorldObjects folder. Pick the folder that holds the game's WorldObjects folder and program.", path.display()))
}

pub fn is_install(dir: &Path) -> bool {
    dir.join("WorldObjects").is_dir()
}

/// Library folders listed in Steam's `steamapps/libraryfolders.vdf`: `"path" "D:\\Games"` entries (current format) or
/// `"1" "D:\\Games"` (older format).
pub fn library_folders(vdf: &str) -> Vec<PathBuf> {
    let mut out = vec![];
    for line in vdf.lines() {
        let words = quoted(line);
        if let [key, value] = words.as_slice() {
            let old_format = key.parse::<u32>().is_ok() && (value.contains('/') || value.contains('\\'));
            if key.eq_ignore_ascii_case("path") || old_format {
                out.push(PathBuf::from(value));
            }
        }
    }
    out
}

/// The quoted strings of one VDF line (`\\` and `\"` unescaped).
fn quoted(line: &str) -> Vec<String> {
    let (mut words, mut chars) = (vec![], line.chars());
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut word = String::new();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => word.extend(chars.next()),
                c => word.push(c),
            }
        }
        words.push(word);
    }
    words
}

/// `SteamPath` from `reg query HKCU\Software\Valve\Steam /v SteamPath` output.
pub fn reg_value(output: &str) -> Option<PathBuf> {
    let line = output.lines().find(|l| l.trim_start().starts_with("SteamPath"))?;
    Some(PathBuf::from(line.split_once("REG_SZ")?.1.trim()))
}

/// Steam installs on this machine (their libraries are listed inside).
fn steam_roots() -> Vec<PathBuf> {
    let mut roots = vec![];
    #[cfg(windows)]
    {
        // `reg` ships with Windows; reading the key needs no extra crate.
        let mut reg = Command::new("reg");
        reg.args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"]);
        if let Ok(out) = no_window(&mut reg).output() {
            roots.extend(reg_value(&String::from_utf8_lossy(&out.stdout)));
        }
        for var in ["ProgramFiles(x86)", "ProgramFiles"] {
            roots.extend(std::env::var_os(var).map(|p| PathBuf::from(p).join("Steam")));
        }
    }
    #[cfg(not(windows))]
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        roots.extend([".steam/steam", ".local/share/Steam", ".var/app/com.valvesoftware.Steam/.local/share/Steam"].map(|p| home.join(p)));
    }
    roots
}

/// `(name, data)` of one value line in `reg query` output: `    Name    REG_SZ    data`.
fn reg_line(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.trim_start().splitn(3, "    ");
    let (name, kind) = (parts.next()?, parts.next()?);
    kind.starts_with("REG_").then(|| (name, parts.next().unwrap_or("").trim_end()))
}

/// Folders named by installed-program entries whose `DisplayName` mentions Treasure Planet, from `reg query <Uninstall key> /s`
/// output: `InstallLocation`, and every quoted path (or the unquoted path) in `DisplayIcon` / `UninstallString` with its folder.
/// Disc installers chose their own folders, so this finds them wherever they went; callers keep only real installs.
pub fn uninstall_dirs(output: &str) -> Vec<PathBuf> {
    let mut out = vec![];
    for entry in output.split("HKEY_") {
        let values: Vec<_> = entry.lines().filter_map(reg_line).collect();
        let get = |name: &str| values.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| *v);
        if !get("DisplayName").is_some_and(|n| n.to_ascii_lowercase().contains("treasure planet")) {
            continue;
        }
        out.extend(get("InstallLocation").filter(|v| !v.is_empty()).map(|v| PathBuf::from(v.trim_matches('"'))));
        for value in ["DisplayIcon", "UninstallString"].into_iter().filter_map(get) {
            let quoted: Vec<&str> = value.split('"').skip(1).step_by(2).collect();
            let paths = if quoted.is_empty() { vec![value.split(',').next().unwrap_or("").trim()] } else { quoted };
            for path in paths.into_iter().filter(|p| !p.is_empty()) {
                // Registry paths use '\'; split by hand so the parent is also found when this runs (or is tested) off Windows.
                out.extend(path.rsplit_once(['\\', '/']).map(|(dir, _)| PathBuf::from(dir)));
                out.push(PathBuf::from(path));
            }
        }
    }
    out
}

/// Folders up to two levels below `dirs` whose name mentions Treasure Planet, with their subfolders (a publisher folder may
/// hold the game folder): disc installs in Program Files or a Wine prefix that left no installed-program entry.
pub fn scan(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let subdirs = |dir: &Path| -> Vec<PathBuf> { std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect() };
    let (mut out, mut level) = (vec![], dirs.to_vec());
    for _ in 0..2 {
        let mut next = vec![];
        for path in level.iter().flat_map(|d| subdirs(d)) {
            if path.file_name().is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().contains("treasure planet")) {
                out.extend(subdirs(&path));
                out.push(path);
            } else {
                next.push(path);
            }
        }
        level = next;
    }
    out
}

/// Places a disc (or other non-Steam) install may be: installed-program entries and Program Files on Windows, the default
/// Wine prefix elsewhere.
fn disc_candidates() -> Vec<PathBuf> {
    let mut found = vec![];
    #[cfg(windows)]
    {
        // 64-bit, 32-bit (a 2002 installer is 32-bit) and per-user entries.
        for key in [
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall",
        ] {
            let mut reg = Command::new("reg");
            reg.args(["query", key, "/s"]);
            if let Ok(out) = no_window(&mut reg).output() {
                found.extend(uninstall_dirs(&String::from_utf8_lossy(&out.stdout)));
            }
        }
        let program_files: Vec<PathBuf> = ["ProgramFiles(x86)", "ProgramFiles"].iter().filter_map(|v| std::env::var_os(v).map(PathBuf::from)).collect();
        found.extend(scan(&program_files));
    }
    #[cfg(not(windows))]
    {
        let prefix = std::env::var_os("WINEPREFIX").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".wine")));
        if let Some(c) = prefix.map(|p| p.join("drive_c")) {
            found.extend(scan(&[c.join("Program Files (x86)"), c.join("Program Files")]));
        }
    }
    found
}

/// Game installs in every Steam library, then disc installs, without duplicates.
pub fn find_installs() -> Vec<PathBuf> {
    let mut candidates = vec![];
    for root in steam_roots() {
        let vdf = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")).unwrap_or_default();
        for library in std::iter::once(root).chain(library_folders(&vdf)) {
            candidates.push(library.join("steamapps").join("common").join(STEAM_DIR));
        }
    }
    candidates.extend(disc_candidates());
    let key = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let mut found: Vec<PathBuf> = vec![];
    for dir in candidates {
        if is_install(&dir) && !found.iter().any(|f| key(f) == key(&dir)) {
            found.push(dir);
        }
    }
    found
}

fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashes up for the child
    }
    cmd
}

/// The importers to try, in order: `openprocyon-import` next to the game (the release download), else Python on the checkout's
/// `tools.import_assets` (its `.venv` first).
fn importers(game: &Path, out: &Path) -> Vec<(String, Command)> {
    let args = |cmd: &mut Command| {
        cmd.arg("--game").arg(game).arg("--out").arg(out);
    };
    let mut list = vec![];
    let name = if cfg!(windows) { "openprocyon-import.exe" } else { "openprocyon-import" };
    if let Some(frozen) = std::env::current_exe().ok().and_then(|p| Some(p.parent()?.join(name))).filter(|p| p.is_file()) {
        let mut cmd = Command::new(&frozen);
        args(&mut cmd);
        list.push((frozen.display().to_string(), cmd));
    }
    let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    if checkout.join("tools/import_assets.py").is_file() {
        let venv = [".venv/Scripts/python.exe", ".venv/bin/python"].map(|p| checkout.join(p));
        let pythons: Vec<(String, Vec<&str>)> = venv
            .iter()
            .filter(|p| p.is_file())
            .map(|p| (p.display().to_string(), vec![]))
            .chain(if cfg!(windows) {
                vec![("py".into(), vec!["-3"]), ("python".into(), vec![]), ("python3".into(), vec![])]
            } else {
                vec![("python3".into(), vec![]), ("python".into(), vec![])]
            })
            .collect();
        for (python, pre) in pythons {
            let mut cmd = Command::new(&python);
            cmd.args(pre).args(["-m", "tools.import_assets"]).current_dir(&checkout);
            args(&mut cmd);
            list.push((format!("{python} -m tools.import_assets"), cmd));
        }
    }
    list
}

/// Starts the first importer that runs; its output and exit arrive on `tx`.
fn spawn_import(game: &Path, out: &Path, tx: mpsc::Sender<Msg>) -> Result<(String, Arc<Mutex<Child>>), String> {
    let mut tried = vec![];
    for (label, mut cmd) in importers(game, out) {
        no_window(&mut cmd).env("PYTHONUNBUFFERED", "1").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(err) => {
                tried.push(format!("{label}: {err}"));
                continue;
            }
        };
        let readers = [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)]
            .into_iter()
            .flatten()
            .map(|stream| {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    for line in BufReader::new(stream).split(b'\n').map_while(Result::ok) {
                        let _ = tx.send(Msg::Line(String::from_utf8_lossy(&line).trim_end().to_owned()));
                    }
                })
            })
            .collect::<Vec<_>>();
        let child = Arc::new(Mutex::new(child));
        let watched = child.clone();
        std::thread::spawn(move || {
            for reader in readers {
                let _ = reader.join();
            }
            let status = loop {
                match watched.lock().map(|mut c| c.try_wait()) {
                    Ok(Ok(Some(status))) => break Ok(status),
                    Ok(Ok(None)) => std::thread::sleep(std::time::Duration::from_millis(50)),
                    Ok(Err(err)) => break Err(err.to_string()),
                    Err(_) => break Err("lost the importer".into()),
                }
            };
            let _ = tx.send(Msg::Exit(match status {
                Ok(s) if s.success() => Ok(()),
                Ok(s) => Err(format!("The importer stopped ({s}).")),
                Err(err) => Err(err),
            }));
        });
        return Ok((label, child));
    }
    Err(format!("Could not start the importer. The download has openprocyon-import next to the game; from source, Python 3 with requirements.txt is needed.\n{}", tried.join("\n")))
}

/// Opens a file with the system's default program (the crash report in a text viewer).
fn open_file(path: &Path) {
    let opener = if cfg!(windows) {
        Command::new("explorer").arg(path).spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(path).spawn()
    } else {
        Command::new("xdg-open").arg(path).spawn()
    };
    if let Err(err) = opener {
        error!("could not open {} ({err})", path.display());
    }
}

/// Reads the clipboard as text with the system's own tool (no clipboard crate), off the main thread.
fn paste(tx: mpsc::Sender<Msg>) {
    std::thread::spawn(move || {
        let tools: &[(&str, &[&str])] = if cfg!(windows) {
            &[("powershell", &["-NoProfile", "-NonInteractive", "-Command", "Get-Clipboard"])]
        } else if cfg!(target_os = "macos") {
            &[("pbpaste", &[])]
        } else {
            &[("wl-paste", &["--no-newline"]), ("xclip", &["-selection", "clipboard", "-o"]), ("xsel", &["--clipboard", "--output"])]
        };
        for (tool, args) in tools {
            if let Ok(out) = no_window(Command::new(tool).args(*args).stdin(Stdio::null())).output()
                && out.status.success()
            {
                let _ = tx.send(Msg::Paste(String::from_utf8_lossy(&out.stdout).into_owned()));
                return;
            }
        }
    });
}

fn buttons(q: Query<(&Interaction, &SetupAction), Changed<Interaction>>, mut setup: ResMut<Setup>, mut exit: MessageWriter<AppExit>) {
    for (i, action) in &q {
        if *i != Interaction::Pressed {
            continue;
        }
        match *action {
            SetupAction::Pick(n) => {
                if let Some(p) = setup.found.get(n).map(|p| p.display().to_string()) {
                    setup.set_path(p);
                }
            }
            SetupAction::Paste => paste(setup.tx.clone()),
            SetupAction::Import => setup.start(),
            SetupAction::Change => setup.phase = Phase::Choose,
            SetupAction::OpenLog => {
                if let Phase::Crashed { log, .. } = &setup.phase {
                    open_file(log);
                }
            }
            SetupAction::Quit => {
                exit.write(AppExit::Success);
            }
        }
    }
}

/// Typing into the folder field: characters, Backspace, Ctrl+V / Shift+Insert paste, Enter imports, Escape clears.
fn keys(mut input: MessageReader<KeyboardInput>, held: Res<ButtonInput<KeyCode>>, mut setup: ResMut<Setup>) {
    let ctrl = held.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    for key in input.read() {
        if key.state != ButtonState::Pressed {
            continue;
        }
        if matches!(setup.phase, Phase::Failed { .. }) && key.logical_key == Key::Enter {
            setup.start(); // Enter retries
            continue;
        }
        if setup.phase != Phase::Choose {
            continue;
        }
        let shift = held.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        match &key.logical_key {
            Key::Enter => setup.start(),
            Key::Escape => setup.set_path(String::new()),
            Key::Backspace => {
                let mut path = setup.path.clone();
                if ctrl {
                    path.clear();
                } else {
                    path.pop();
                }
                setup.set_path(path);
            }
            _ if ctrl && key.key_code == KeyCode::KeyV => paste(setup.tx.clone()),
            Key::Insert if shift => paste(setup.tx.clone()),
            _ if ctrl => {}
            _ => {
                if let Some(text) = key.text.as_ref().filter(|t| t.chars().all(|c| !c.is_control())) {
                    let path = format!("{}{text}", setup.path);
                    setup.set_path(path);
                }
            }
        }
    }
}

/// Dropping the game folder (or its program) on the window fills the field.
fn dropped(mut drops: MessageReader<FileDragAndDrop>, mut setup: ResMut<Setup>) {
    for drop in drops.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = drop
            && setup.phase == Phase::Choose
        {
            setup.set_path(path_buf.display().to_string());
        }
    }
}

fn poll(mut setup: ResMut<Setup>) {
    if setup.bypass_change_detection().poll() {
        setup.set_changed();
    }
}

/// After a successful import: the same command line again, now with assets, then this app closes.
fn relaunch(setup: Res<Setup>, mut exit: MessageWriter<AppExit>, mut done: Local<bool>) {
    if setup.phase != Phase::Starting || *done {
        return;
    }
    *done = true;
    let args = game_args(std::env::args().skip(1));
    match std::env::current_exe().and_then(|binary| Command::new(binary).args(args).spawn()) {
        Ok(_) => info!("assets imported into {}; starting the game", setup.out.display()),
        Err(err) => error!("assets imported into {}, but the game did not start again ({err}): start it again by hand", setup.out.display()),
    }
    exit.write(AppExit::Success);
}

/// The command line for the game: without the setup screen's own `--import`, `--setup-shot <path>` and `--crashed <log>`.
fn game_args(mut args: impl Iterator<Item = String>) -> Vec<String> {
    let mut out = vec![];
    while let Some(a) = args.next() {
        match a.as_str() {
            "--import" => {}
            "--setup-shot" | "--crashed" => {
                args.next();
            }
            _ => out.push(a),
        }
    }
    out
}

/// `--setup-shot path` (`--shot-frame N`, default 60) saves one screenshot of the setup screen and quits; for smoke runs. Not
/// `--shot`: that one is passed on to the game started after the import.
fn shot(mut commands: Commands, mut frame: Local<u32>, mut exit: MessageWriter<AppExit>) {
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let Some(path) = arg("--setup-shot") else { return };
    let at = arg("--shot-frame").and_then(|v| v.parse().ok()).unwrap_or(60u32);
    *frame += 1;
    if *frame == at {
        commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window()).observe(bevy::render::view::screenshot::save_to_disk(path));
    }
    if *frame >= at + 30 {
        exit.write(AppExit::Success);
    }
}

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
    let args: Vec<String> = std::env::args().skip(1).collect();
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
fn outdated_reason(version: Option<&str>) -> String {
    let now = env!("CARGO_PKG_VERSION");
    let by = version.map_or("an older version of OpenProcyon".to_owned(), |v| format!("OpenProcyon {v}"));
    format!("Your game was converted by {by}. OpenProcyon {now} needs it converted again: click Import (a few minutes). Only the converted files are replaced.")
}

/// The crash screen's message: the report's panic line (`message` in `crash::report`).
fn crash_message(log: &Path) -> String {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    text.lines().find_map(|l| l.strip_prefix("message: ")).unwrap_or("unknown error").to_owned()
}

/// The setup app: `out` is the folder the game reads its assets from, `fonts` the `content/` folder (the screen's only art),
/// `view` draws it. `status` says why it opened; `staged` (a player's folder) imports beside `out` and swaps it in on success.
/// `--game DIR` fills the folder field (else the install the old import came from, else a detected one); `--import` also starts
/// importing it; `--crashed LOG` opens the crash screen for the game's report.
pub fn run(out: PathBuf, fonts: PathBuf, view: fn(&mut App), status: Imported, staged: bool) {
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let previous = match &status {
        Imported::Outdated { game: Some(game), .. } if is_install(game) => Some(game.display().to_string()),
        _ => None,
    };
    let mut setup = Setup::new(out, arg("--game").or(previous));
    setup.staged = staged;
    if let Imported::Outdated { version, .. } = &status {
        setup.reason = Some(outdated_reason(version.as_deref()));
    }
    let crashed = arg("--crashed").map(PathBuf::from);
    if let Some(log) = crashed.clone() {
        setup.phase = Phase::Crashed { message: crash_message(&log), log };
    } else if args.iter().any(|a| a == "--import") {
        setup.start();
    }
    let title = if crashed.is_some() { "OpenProcyon: something went wrong" } else { "OpenProcyon: import your game" };
    let window = Window { title: title.into(), ..default() };
    App::new()
        .add_plugins(
            DefaultPlugins.set(AssetPlugin { file_path: fonts.to_string_lossy().into_owned(), ..default() }).set(WindowPlugin { primary_window: Some(window), ..default() }),
        )
        .insert_resource(setup)
        .insert_resource(ClearColor(Color::srgb(0.03, 0.04, 0.09)))
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Camera2d);
        })
        .add_systems(Update, ((poll, keys, dropped, buttons, relaunch).chain().in_set(SetupInput), shot))
        .add_plugins(view)
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_library_folders_both_formats() {
        let vdf = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
		"apps"
		{
			"228980"		"360059062"
		}
	}
	"1"
	{
		"path"		"D:\\Games"
	}
}"#;
        assert_eq!(library_folders(vdf), [PathBuf::from(r"C:\Program Files (x86)\Steam"), PathBuf::from(r"D:\Games")]);
        let old = "\"LibraryFolders\"\n{\n\t\"TimeNextStatsReport\"\t\t\"1234\"\n\t\"1\"\t\t\"/home/me/games/Steam\"\n}";
        assert_eq!(library_folders(old), [PathBuf::from("/home/me/games/Steam")]);
        assert!(library_folders("").is_empty());
        assert_eq!(quoted(r#"  "a\"b"  "c""#), ["a\"b", "c"]);
    }

    #[test]
    fn steam_path_from_the_registry_query() {
        let out = "\r\nHKEY_CURRENT_USER\\Software\\Valve\\Steam\r\n    SteamPath    REG_SZ    c:/program files (x86)/steam\r\n\r\n";
        assert_eq!(reg_value(out), Some(PathBuf::from("c:/program files (x86)/steam")));
        assert_eq!(reg_value("ERROR: The system was unable to find the specified registry key or value."), None);
    }

    #[test]
    fn disc_installs_from_installed_program_entries() {
        let out = concat!(
            "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Other\r\n",
            "    DisplayName    REG_SZ    Some Other Game\r\n",
            "    InstallLocation    REG_SZ    C:\\Games\\Other\r\n",
            "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{0A1B}\r\n",
            "    DisplayName    REG_SZ    Treasure Planet: Battle at Procyon\r\n",
            "    InstallLocation    REG_SZ    \r\n",
            "    UninstallString    REG_SZ    C:\\WINDOWS\\IsUninst.prog -f\"C:\\Program Files\\Disney\\TP\\Uninst.isu\"\r\n",
            "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\TP\r\n",
            "    DisplayName    REG_SZ    TREASURE PLANET\r\n",
            "    InstallLocation    REG_SZ    \"E:\\Old Games\\Procyon\"\r\n",
            "    DisplayIcon    REG_SZ    E:\\Old Games\\Procyon\\Procyon.ico,0\r\n",
        );
        let dirs = uninstall_dirs(out);
        for want in [r"C:\Program Files\Disney\TP", r"E:\Old Games\Procyon"] {
            assert!(dirs.contains(&PathBuf::from(want)), "{want} in {dirs:?}");
        }
        assert!(!dirs.contains(&PathBuf::from(r"C:\Games\Other")));
        assert_eq!(reg_line("    DisplayName    REG_SZ    A  B"), Some(("DisplayName", "A  B")));
        assert_eq!(reg_line("HKEY_CURRENT_USER\\Software"), None);
    }

    #[test]
    fn disc_installs_by_folder_name() {
        let dir = std::env::temp_dir().join(format!("procyon-scan-test-{}", std::process::id()));
        let direct = dir.join("Program Files (x86)").join("Treasure Planet Battle at Procyon");
        let nested = dir.join("Program Files").join("Disney Interactive").join("TREASURE PLANET").join("Procyon");
        let too_deep = dir.join("Program Files").join("a").join("b").join("Treasure Planet");
        for d in [&direct, &nested, &too_deep] {
            std::fs::create_dir_all(d).unwrap();
        }
        let found = scan(&[dir.join("Program Files (x86)"), dir.join("Program Files"), dir.join("missing")]);
        assert!(found.contains(&direct) && found.contains(&nested), "{found:?}");
        assert!(!found.contains(&too_deep));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn install_validation() {
        let dir = std::env::temp_dir().join(format!("procyon-setup-test-{}", std::process::id()));
        let game = dir.join("steamapps").join("common").join(STEAM_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(check_install("").is_err());
        assert!(check_install(&dir.join("missing").display().to_string()).unwrap_err().contains("no folder"));
        assert!(check_install(&dir.display().to_string()).unwrap_err().contains("no WorldObjects"));
        std::fs::create_dir_all(game.join("WorldObjects")).unwrap();
        std::fs::write(game.join("Procyon.exe"), b"").unwrap();
        for input in [game.display().to_string(), format!("  \"{}\" ", game.display()), game.join("Procyon.exe").display().to_string(), dir.display().to_string()] {
            assert_eq!(check_install(&input), Ok(game.clone()), "{input}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn progress_lines() {
        assert_eq!(parse_progress("[3/27] hud"), Some((3, 27, "hud", None)));
        assert_eq!(parse_progress("[1/28] ships: 36/71 (36%)\r"), Some((1, 28, "ships: 36/71", Some(36))));
        assert_eq!(parse_progress("[2/28] models (74%)"), Some((2, 28, "models", Some(74))));
        assert_eq!(parse_progress("[2/28] odd (name%)"), Some((2, 28, "odd (name%)", None)));
        assert_eq!(parse_progress("[12/27] waypoint art\r"), Some((12, 27, "waypoint art", None)));
        assert_eq!(parse_progress("skip Ship_Wreck: no mesh"), None);
        assert_eq!(parse_progress("[0/27] hud"), None);
        assert_eq!(parse_progress("[x/27] hud"), None);
        assert_eq!(failed_step("import failed at step 'models': tools.mdb_to_glb"), Some("models".into()));
        assert_eq!(failed_step("done: C:/assets"), None);
    }

    #[test]
    fn the_game_gets_the_command_line_without_setup_flags() {
        let args = ["--game", "D:/x", "--import", "--setup-shot", "a.png", "--menu", "--crashed", "c.log", "--shot", "b.png"].map(String::from);
        assert_eq!(game_args(args.into_iter()), ["--game", "D:/x", "--menu", "--shot", "b.png"]);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("procyon-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_players_import_must_come_from_this_version() {
        let dir = scratch("stamp");
        let assets = dir.join("assets");
        assert_eq!(imported(&assets, "0.1.4"), Imported::Missing);
        std::fs::create_dir_all(assets.join("ships")).unwrap();
        assert_eq!(imported(&assets, "0.1.4"), Imported::Outdated { version: None, game: None }, "imports from before the stamp");
        write_stamp(&assets, "0.1.3", Path::new("D:/Games/TP")).unwrap();
        assert_eq!(imported(&assets, "0.1.4"), Imported::Outdated { version: Some("0.1.3".into()), game: Some(PathBuf::from("D:/Games/TP")) });
        write_stamp(&assets, "0.1.4", Path::new("D:/Games/TP")).unwrap();
        assert_eq!(imported(&assets, "0.1.4"), Imported::Ready);
        std::fs::write(assets.join(STAMP), "not json").unwrap();
        assert!(matches!(imported(&assets, "0.1.4"), Imported::Outdated { version: None, .. }));
        assert!(outdated_reason(Some("0.1.3")).contains("OpenProcyon 0.1.3"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_finished_import_replaces_the_old_folder_whole() {
        let dir = scratch("swap");
        let (out, staged) = (dir.join("assets"), dir.join("assets.importing"));
        assert_eq!(staging(&out), staged);
        std::fs::create_dir_all(out.join("ships")).unwrap();
        std::fs::write(out.join("ships/leftover.json"), "old").unwrap();
        std::fs::create_dir_all(staged.join("ships")).unwrap();
        std::fs::write(staged.join("ships/new.json"), "new").unwrap();
        swap_in(&staged, &out).unwrap();
        assert!(out.join("ships/new.json").is_file() && !out.join("ships/leftover.json").exists(), "no files left from the old import");
        assert!(!staged.exists() && !dir.join("assets.replaced").exists());
        // First import: nothing to replace.
        std::fs::remove_dir_all(&out).unwrap();
        std::fs::create_dir_all(staged.join("ships")).unwrap();
        swap_in(&staged, &out).unwrap();
        assert!(out.join("ships").is_dir());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_staged_import_stamps_and_swaps_only_when_it_succeeds() {
        let dir = scratch("staged");
        let out = dir.join("assets");
        std::fs::create_dir_all(out.join("ships")).unwrap();
        let mut setup = Setup::new(out.clone(), Some(String::new()));
        setup.staged = true;
        setup.from = Some(PathBuf::from("D:/Games/TP"));
        // The importer failed: the old import stays in use.
        std::fs::create_dir_all(staging(&out).join("ships")).unwrap();
        setup.phase = Phase::Importing { step: 3, total: 3, name: "bases".into(), percent: None };
        setup.tx.send(Msg::Exit(Err("exit 1".into()))).unwrap();
        setup.poll();
        assert!(matches!(setup.phase, Phase::Failed { .. }));
        assert!(out.join("ships").is_dir() && !out.join(STAMP).exists());
        // It succeeded: the new import, stamped with this version, replaces the old one.
        setup.phase = Phase::Importing { step: 3, total: 3, name: "bases".into(), percent: None };
        setup.tx.send(Msg::Exit(Ok(()))).unwrap();
        setup.poll();
        assert_eq!(setup.phase, Phase::Starting);
        assert_eq!(imported(&out, env!("CARGO_PKG_VERSION")), Imported::Ready);
        assert!(!staging(&out).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_crash_screen_shows_the_reports_message() {
        let dir = scratch("crash");
        let log = dir.join("crash.log");
        std::fs::write(&log, "OpenProcyon 0.1.4 crashed.\nmessage: missing field `point`\nat: src/data/ships.rs:157\n").unwrap();
        assert_eq!(crash_message(&log), "missing field `point`");
        assert_eq!(crash_message(&dir.join("missing.log")), "unknown error");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn importer_output_drives_the_phase() {
        let dir = std::env::temp_dir().join(format!("procyon-setup-poll-{}", std::process::id()));
        let mut setup = Setup::new(dir.clone(), Some(String::new()));
        setup.phase = Phase::Importing { step: 0, total: 0, name: String::new(), percent: None };
        for line in ["[1/3] ships (0%)", "skip Ship_Wreck", "[2/3] models (40%)", "import failed at step 'models': tools.mdb_to_glb"] {
            setup.tx.send(Msg::Line(line.into())).unwrap();
        }
        assert!(setup.poll());
        assert_eq!(setup.phase, Phase::Importing { step: 2, total: 3, name: "models".into(), percent: Some(40) });
        setup.tx.send(Msg::Exit(Err("exit 1".into()))).unwrap();
        setup.poll();
        assert_eq!(setup.phase, Phase::Failed { step: Some("models".into()), reason: "exit 1".into() });
        assert_eq!(setup.log.len(), 4);
        // A clean exit without ships/ is still a failure; with it the game starts.
        setup.phase = Phase::Importing { step: 3, total: 3, name: "bases".into(), percent: None };
        setup.tx.send(Msg::Exit(Ok(()))).unwrap();
        setup.poll();
        assert!(matches!(setup.phase, Phase::Failed { .. }));
        std::fs::create_dir_all(dir.join("ships")).unwrap();
        setup.phase = Phase::Importing { step: 3, total: 3, name: "bases".into(), percent: None };
        setup.tx.send(Msg::Exit(Ok(()))).unwrap();
        setup.poll();
        assert_eq!(setup.phase, Phase::Starting);
        assert!(!setup.poll(), "nothing new");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
