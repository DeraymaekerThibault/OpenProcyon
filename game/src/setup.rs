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

mod crash;
mod import;
mod install;

pub use crash::report_crashes;
use crash::{crash_message, outdated_reason};
use import::no_window;
pub use import::{Imported, failed_step, imported, parse_progress, spawn_tool, staging, swap_in, write_stamp};
pub use install::{check_install, find_installs, is_install, library_folders, reg_value, scan, uninstall_dirs};
#[cfg(test)]
use install::{quoted, reg_line};

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

/// What a child started by `spawn_tool` says: an output line, then how it ended.
pub enum ToolMsg {
    Line(String),
    Exit(Result<(), String>),
}

enum Msg {
    Tool(ToolMsg),
    Paste(String),
}

impl From<ToolMsg> for Msg {
    fn from(m: ToolMsg) -> Self {
        Msg::Tool(m)
    }
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
        let args = ["--game".into(), game.clone().into_os_string(), "--out".into(), target.into_os_string()];
        match spawn_tool(&args, false, self.tx.clone()) {
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
                Msg::Tool(ToolMsg::Line(line)) => {
                    if let (Some((step, total, name, percent)), Phase::Importing { .. }) = (parse_progress(&line), &self.phase) {
                        self.phase = Phase::Importing { step, total, name: name.to_owned(), percent };
                    }
                    self.log.push(line);
                    if self.log.len() > LOG_LINES {
                        self.log.remove(0);
                    }
                }
                Msg::Tool(ToolMsg::Exit(result)) => {
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
mod tests;
