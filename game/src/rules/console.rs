//! Not retail: the developer console, a one-shot runner for the mission rule vocabulary, opened with `~`.
//!
//! A typed command is not a second command system. It is turned into a rule action (or a condition, for a query) and run through
//! `Script::act` / `Script::cond`, so every action a mission can use is available, and `act {json}` takes any of them as raw JSON.
//! The short commands (`god`, `kill`, `reveal`, `tp`, ...) only build the action for the common cases. `repair` has no rule
//! action, so it is the one command that touches the ships directly (systems/console.rs). This file is the pure part: parsing,
//! the mapping to actions and the mission-debug queries; the ECS side and the keyboard live in `systems/console.rs`, the drawing in
//! the view. Using any command that changes the game marks the session as cheated (`Cheats`): campaign points are not recorded then.
use bevy::input::keyboard::Key;
use bevy::prelude::*;
use serde_json::{Value, json};

use super::schema::{self, Role};
use super::*;

mod complete;
mod fps;
mod list;
mod more;
pub use complete::{Completion, Names, completions, mean, nearest_name, tokens};
pub use fps::FrameStats;
pub use more::{CamMode, DevView, Env, Shape, ShapeKind, ShipInfo, ViewChange, free_cam, game_pos, hud_hidden, marks_path};

/// Point set the `tp x y` command writes its destination to before running 'Teleport Group/Unit'.
const TP_SET: &str = "(console)";
/// Lines of output kept (older ones scroll off; PageUp / the wheel / Ctrl+Home read back through them).
const LOG_LINES: usize = 2000;
/// Output lines the panel shows at once (`classic/console.rs` draws this many), and what PageUp / PageDown move by.
pub const SHOWN: usize = 16;
/// Lines one wheel notch moves.
pub const WHEEL_LINES: usize = 3;
/// Longest command line.
const MAX_LINE: usize = 400;

/// The console's state: shared, so views only draw it.
#[derive(Resource, Default)]
pub struct Console {
    /// On without the option: debug builds, `--console` or `PROCYON_CONSOLE` (see `systems::dev_console::enabled`).
    pub enabled: bool,
    pub open: bool,
    /// The line being typed.
    pub line: String,
    pub log: Vec<String>,
    /// Lines the view is scrolled up from the newest (0: following the end).
    pub scroll: usize,
    history: Vec<String>,
    /// Index into `history` while Up / Down recall is in use.
    recall: Option<usize>,
    /// The console paused the simulation when it opened (and resumes it on closing).
    pub(in crate::rules) paused_by_us: bool,
    /// Lines submitted and not yet run (systems/console.rs runs them once per frame).
    pub(in crate::rules) pending: Vec<String>,
    /// The mission's names, for Tab (refreshed each frame while open).
    pub names: Names,
    tab: Option<TabState>,
}

/// Repeated Tab on the same line cycles through the candidates.
struct TabState {
    start: usize,
    items: Vec<(String, String)>,
    next: usize,
    /// The line as Tab left it; any other edit ends the cycle.
    last: String,
}

/// What a key press did to the line.
#[derive(Debug, PartialEq)]
pub enum Edit {
    None,
    /// Enter: a finished line (never empty).
    Submit(String),
    Close,
}

impl Console {
    pub fn new(enabled: bool) -> Self {
        Console { enabled, ..default() }
    }
    /// Whether `~` opens it: the forced-on cases above, or the "Developer Console" option.
    pub fn available(&self) -> bool {
        self.enabled || crate::menu::console_setting()
    }
    /// Adds a line (or several, split at newlines) to the output.
    pub fn say(&mut self, text: impl AsRef<str>) {
        let before = self.log.len();
        self.log.extend(text.as_ref().lines().map(String::from));
        // New output keeps a view that was scrolled back where it was; one at the end follows the output.
        if self.scroll > 0 {
            self.scroll += self.log.len() - before;
        }
        let over = self.log.len().saturating_sub(LOG_LINES);
        self.log.drain(..over);
        self.scroll = self.scroll.min(self.max_scroll());
    }
    /// How far back the view can go: the first line stays at the top of the panel.
    pub fn max_scroll(&self) -> usize {
        self.log.len().saturating_sub(SHOWN)
    }
    /// Moves the view back (`lines` > 0) or forward (< 0) through the output.
    pub fn scroll_by(&mut self, lines: isize) {
        self.scroll = self.scroll.saturating_add_signed(lines).min(self.max_scroll());
    }
    /// The panel's text: the visible window of the output with its "more above / below" hints, then the line being typed.
    pub fn display(&self) -> String {
        let end = self.log.len().saturating_sub(self.scroll);
        let start = end.saturating_sub(SHOWN);
        let mut shown = String::new();
        if start > 0 {
            shown.push_str(&format!("^ {start} more above (PageUp, Ctrl+Home)\n"));
        }
        shown.push_str(&self.log[start..end].join("\n"));
        if self.scroll > 0 {
            shown.push_str(&format!("\nv {} more below (PageDown, Ctrl+End)", self.scroll));
        }
        shown.push_str(&format!("\n> {}_", self.line));
        shown
    }
    pub fn scroll_top(&mut self) {
        self.scroll = self.max_scroll();
    }
    pub fn scroll_bottom(&mut self) {
        self.scroll = 0;
    }
    /// Tab: completes the last word of the line (`completions`). One candidate is put in; several are listed in the output and the line
    /// gets what they share; Tab again steps through them.
    fn complete(&mut self) {
        if let Some(t) = self.tab.as_mut().filter(|t| t.last == self.line && !t.items.is_empty()) {
            let item = &t.items[t.next % t.items.len()].1;
            self.line = format!("{}{item}", &self.line[..t.start]);
            t.next += 1;
            t.last.clone_from(&self.line);
            return;
        }
        self.tab = None;
        let c = completions(&self.line, &self.names);
        match c.items.len() {
            0 => {}
            1 => {
                let word = !self.line.contains(char::is_whitespace);
                self.line = format!("{}{}", &self.line[..c.start], c.items[0].1);
                if word || !self.line.ends_with('"') && !self.line.ends_with(' ') && !self.line.ends_with(": ") && !self.line.contains('{') {
                    self.line.push(' ');
                }
            }
            n => {
                let shown: Vec<&str> = c.items.iter().take(40).map(|i| i.0.as_str()).collect();
                self.say(format!("{}{}", shown.join("  "), if n > 40 { format!("  ... {} more", n - 40) } else { String::new() }));
                let common = common_prefix(c.items.iter().map(|i| i.1.as_str()));
                if common.chars().count() > self.line[c.start..].chars().count() {
                    self.line = format!("{}{common}", &self.line[..c.start]);
                }
                self.tab = Some(TabState { start: c.start, items: c.items, next: 0, last: self.line.clone() });
            }
        }
    }
    /// Queues a line to run as if it had been typed and submitted (the smoke script's way in).
    pub fn submit(&mut self, line: &str) {
        self.pending.push(line.to_string());
    }
    /// One key press on the open console: typing, Backspace, Up/Down for earlier lines, Enter, Escape to close (the system also closes it on the console key).
    pub fn key(&mut self, key: &Key) -> Edit {
        self.key_with(key, false)
    }
    /// `key`, knowing whether Ctrl is held (Ctrl+Home / Ctrl+End jump to the top and bottom of the output).
    pub fn key_with(&mut self, key: &Key, ctrl: bool) -> Edit {
        match key {
            Key::Escape => return Edit::Close,
            Key::PageUp => self.scroll_by(SHOWN as isize - 1),
            Key::PageDown => self.scroll_by(1 - SHOWN as isize),
            Key::Home if ctrl => self.scroll_top(),
            Key::End if ctrl => self.scroll_bottom(),
            Key::Character(c) => {
                let room = MAX_LINE.saturating_sub(self.line.chars().count());
                self.line.extend(c.chars().filter(|c| !c.is_control()).take(room));
            }
            Key::Space if self.line.chars().count() < MAX_LINE => self.line.push(' '),
            Key::Backspace => {
                self.line.pop();
            }
            Key::Tab => self.complete(),
            Key::ArrowUp if !self.history.is_empty() => {
                let i = self.recall.map_or(self.history.len() - 1, |i| i.saturating_sub(1));
                self.recall = Some(i);
                self.line.clone_from(&self.history[i]);
            }
            Key::ArrowDown => match self.recall {
                Some(i) if i + 1 < self.history.len() => {
                    self.recall = Some(i + 1);
                    self.line.clone_from(&self.history[i + 1]);
                }
                _ => {
                    self.recall = None;
                    self.line.clear();
                }
            },
            Key::Enter => {
                let line = std::mem::take(&mut self.line).trim().to_string();
                self.recall = None;
                self.scroll = 0;
                if line.is_empty() {
                    return Edit::None;
                }
                if self.history.last() != Some(&line) {
                    self.history.push(line.clone());
                }
                return Edit::Submit(line);
            }
            _ => {}
        }
        Edit::None
    }
}

/// The longest start the strings share, ignoring case (taken from the first).
fn common_prefix<'a>(mut items: impl Iterator<Item = &'a str>) -> String {
    let Some(first) = items.next() else { return String::new() };
    let mut len = first.chars().count();
    for s in items {
        len = len.min(first.chars().zip(s.chars()).take_while(|(a, b)| a.to_lowercase().eq(b.to_lowercase())).count());
    }
    first.chars().take(len).collect()
}

/// Whether cheats were used in this battle, and the state of the toggles.
#[derive(Resource, Default)]
pub struct Cheats {
    /// A command that changes the game ran: scores and campaign points of this session do not count.
    pub used: bool,
    /// `god` is on (the player's fleet takes no damage).
    pub god: bool,
}

/// A parsed console line.
#[derive(Debug, PartialEq)]
pub enum Command {
    Nothing,
    Help(Option<String>),
    /// Rule actions, each run through `Script::act`. `god` carries the new state of the god toggle.
    Acts {
        acts: Vec<Value>,
        god: Option<bool>,
    },
    /// A rule condition, evaluated and answered with true / false.
    Cond(Value),
    /// 'Set Group/Unit Visibility' for every owner on the map.
    Reveal,
    /// 'Destroy Group/Unit' on the current target.
    Kill,
    /// 'Teleport Group/Unit' for the player's ship to a point (game axes), or to the first point of a named point set.
    Teleport(Dest),
    /// Full repair of a group (not a rule action).
    Repair(String),
    /// Run a rule's actions now, as `--fire-rule` does at the start.
    Fire(String),
    /// Which conditions of a rule hold now.
    Why(String),
    Flags,
    Timers,
    Tasks,
    /// Every counter set so far (engine-added `OP` counters).
    Counters,
    /// Rule actions that only change what is shown or heard (`cam follow`, `music`): run like `Acts`, not a cheat.
    Soft(Vec<Value>),
    Rules(Option<String>),
    Skip,
    Win,
    Lose,
    Restart,
    Ships(Option<String>),
    /// `list <kind> [text]`: the names and ids of one kind (see `list::KINDS`).
    List(&'static str, String),
    /// `find <text>`: every kind at once.
    Find(String),
    Inspect,
    Where,
    Mark(String),
    Show(ShapeKind, Option<String>),
    Hide(Option<ShapeKind>, Option<String>),
    Cam(CamMode),
    Hud(bool),
    /// `fps [on|off]`: show or hide the frame-rate counter (no argument toggles).
    Fps(Option<bool>),
    /// The rule schema's entry for an action or condition Type (or the Types whose name contains the text): its fields.
    Schema(String),
}

#[derive(Debug, PartialEq)]
pub enum Dest {
    Xy(f32, f32, f32),
    Set(String),
}

impl Command {
    /// Whether running it changes the game (it then marks the session as cheated). Queries do not.
    pub fn cheats(&self) -> bool {
        matches!(
            self,
            Command::Acts { .. } | Command::Reveal | Command::Kill | Command::Teleport(_) | Command::Repair(_) | Command::Fire(_) | Command::Skip | Command::Win | Command::Lose
        )
    }
}

/// What a command needs from the battle it runs in. Commands depend on this, not on which resource (`Mission`, `SkirmishScript`) the
/// battle happens to hold: one dispatch (`Script::console_run_with`) refuses a command whose need the battle cannot meet and says why.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Needs {
    /// The view and the help only: any screen of a battle.
    Nothing,
    /// The battle's ships and bases (the player's ship, the target): any singleplayer battle, with or without a script.
    World,
    /// A rules script: a campaign mission or a skirmish map with a `script.json` (flags, timers, tasks, counters, rule actions, shapes).
    Rules,
    /// A campaign mission: cutscenes, level changes, the human's ship handoff, mission music.
    Mission,
}

impl Needs {
    /// How `help <command>` words it.
    pub fn describe(self) -> &'static str {
        match self {
            Needs::Nothing => "any battle",
            Needs::World => "any singleplayer battle",
            Needs::Rules => "a battle that runs rules: a campaign mission, or a skirmish map with a script.json",
            Needs::Mission => "a campaign mission",
        }
    }
}

/// What the running battle offers (`Needs` it can meet). The default is a campaign mission, which offers everything.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Caps {
    /// A rules script runs (`Mission` or `SkirmishScript`).
    pub rules: bool,
    /// It is a campaign mission.
    pub mission: bool,
}
impl Caps {
    pub const MISSION: Caps = Caps { rules: true, mission: true };
    pub const SKIRMISH_SCRIPT: Caps = Caps { rules: true, mission: false };
    pub const BARE: Caps = Caps { rules: false, mission: false };
    pub fn meets(self, n: Needs) -> bool {
        match n {
            Needs::Nothing | Needs::World => true,
            Needs::Rules => self.rules,
            Needs::Mission => self.mission,
        }
    }
}
impl Default for Caps {
    fn default() -> Self {
        Caps::MISSION
    }
}

/// The action Types that only change the ships and the world; every other Type records into the rules' state or steers the rules' AI.
const WORLD_ACTS: &[&str] =
    &["Set Group/Unit Vulnerability", "Damage Group/Unit by X percent", "Destroy Group/Unit", "Set Group/Unit Visibility", "Teleport Group/Unit", "Set Group Throttle Percent"];

/// What one raw rule action needs: the schema marks the campaign-only Types; `Play Music Track` is read by the campaign's music system.
fn act_needs(a: &Value) -> Needs {
    let ty = s(a, "Type");
    if ty == "Play Music Track" || schema::lookup(Role::Action, ty).is_some_and(|d| d.campaign_only) {
        Needs::Mission
    } else if WORLD_ACTS.contains(&ty) {
        Needs::World
    } else {
        Needs::Rules
    }
}

impl Command {
    /// What it needs of the battle (see `Needs`). `tp` to a coordinate only moves the ship; to a point set it reads the script's.
    pub fn needs(&self) -> Needs {
        match self {
            Command::Nothing | Command::Help(_) | Command::Fps(_) | Command::Hud(_) | Command::Cam(CamMode::Free) | Command::Schema(_) => Needs::Nothing,
            Command::Reveal
            | Command::Kill
            | Command::Repair(_)
            | Command::Restart
            | Command::Ships(_)
            | Command::Inspect
            | Command::Where
            | Command::Mark(_)
            | Command::Cam(CamMode::Player)
            | Command::Teleport(Dest::Xy(..)) => Needs::World,
            Command::Cond(c) => {
                if schema::lookup(Role::Condition, s(c, "Type")).is_some_and(|d| d.campaign_only) {
                    Needs::Mission
                } else {
                    Needs::World
                }
            }
            Command::Acts { acts, .. } | Command::Soft(acts) => acts.iter().map(act_needs).max().unwrap_or(Needs::World),
            Command::Teleport(Dest::Set(_))
            | Command::Fire(_)
            | Command::Why(_)
            | Command::Flags
            | Command::Timers
            | Command::Tasks
            | Command::Counters
            | Command::Rules(_)
            | Command::Skip
            | Command::List(..)
            | Command::Find(_)
            | Command::Show(..)
            | Command::Hide(..) => Needs::Rules,
            Command::Win | Command::Lose | Command::Cam(CamMode::Follow(_)) => Needs::Mission,
        }
    }

    /// Why this battle cannot run the command, or nothing when it can. `word` is what the player typed first.
    pub fn refusal(&self, word: &str, caps: Caps) -> Option<String> {
        let need = self.needs();
        if caps.meets(need) {
            return None;
        }
        Some(match need {
            Needs::Mission => format!("{word}: needs a campaign mission (this battle is a skirmish: its own rules do not hand over levels, cutscenes or the ship)"),
            _ => format!("{word}: needs rules (this battle runs no rules script: its map has no script.json)"),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Cheats,
    State,
    Flow,
    Ships,
    Authoring,
    Camera,
    Raw,
    Console,
}
impl Group {
    fn title(self) -> &'static str {
        match self {
            Group::Cheats => "Cheats",
            Group::State => "Mission state",
            Group::Flow => "Mission flow",
            Group::Ships => "Ships and inspection",
            Group::Authoring => "Authoring helpers",
            Group::Camera => "Camera and sound",
            Group::Raw => "Raw rule access",
            Group::Console => "Console",
        }
    }
}

/// One console command. `TABLE` is the only list of commands: `help`, Tab completion and the "did you mean" answer read it, and the
/// tests parse every example and check `cheats` against `Command::cheats`, so a command cannot be added to the parser without a
/// row (or the reverse) unnoticed.
pub struct Entry {
    pub name: &'static str,
    pub alias: &'static [&'static str],
    pub group: Group,
    pub usage: &'static str,
    pub about: &'static str,
    pub example: &'static str,
    /// Running it marks the session as cheated.
    pub cheats: bool,
}
const fn entry(name: &'static str, group: Group, usage: &'static str, about: &'static str, example: &'static str, cheats: bool) -> Entry {
    Entry { name, alias: &[], group, usage, about, example, cheats }
}
pub static TABLE: &[Entry] = &[
    entry("god", Group::Cheats, "god [on|off]", "the player's fleet takes no damage (Set Group/Unit Vulnerability); no argument toggles", "god on", true),
    entry("repair", Group::Cheats, "repair [group]", "full repair of the player's fleet, or of a group (no rule action exists for it)", "repair", true),
    entry("kill", Group::Cheats, "kill", "destroy the current target (Destroy Group/Unit)", "kill", true),
    entry("reveal", Group::Cheats, "reveal", "show every hidden unit (Set Group/Unit Visibility)", "reveal", true),
    entry(
        "tp",
        Group::Cheats,
        "tp <x> <y> [z] | tp <point set>",
        "move the player's ship to game coordinates (any battle) or to a point set of the map's script",
        "tp 120 -40",
        true,
    ),
    entry("fire", Group::State, "fire <rule>", "run a rule's actions now, as --fire-rule does; a part of the name is enough", "fire \"Open the gate\"", true),
    entry("set", Group::State, "set <flag> [on|off]", "Set Flag Action", "set key off", true),
    entry("timer", Group::State, "timer <name> start|stop", "Start Timer / Stop Timer", "timer t start", true),
    entry("task", Group::State, "task <name> active|complete|failed [on|off]", "Set Objective Task ... State", "task Escape complete", true),
    entry("counter", Group::State, "counter <name> <value>", "OP Set Counter", "counter kills 3", true),
    entry("why", Group::State, "why <rule>", "which conditions of a rule hold now, and whether it is inactive or waiting in an OP Wait", "why gate", false),
    entry("flags", Group::State, "flags", "every flag that has been set", "flags", false),
    entry("timers", Group::State, "timers", "every timer, with its time", "timers", false),
    entry("tasks", Group::State, "tasks", "every objective task and its state", "tasks", false),
    entry("counters", Group::State, "counters", "every counter that has been set", "counters", false),
    entry("rules", Group::Flow, "rules [text]", "every rule with its state: fired, ran (repeating), never fired, inactive or waiting in an OP Wait", "rules gate", false),
    entry("skip", Group::Flow, "skip", "end the speech line playing now; it counts as played for Speech Event Played Once", "skip", true),
    entry("nis", Group::Flow, "nis end", "NIS End: leave a cutscene", "nis end", true),
    entry("win", Group::Flow, "win", "win the mission (Goto Next Level): test the victory screen and the hand-off", "win", true),
    entry("lose", Group::Flow, "lose", "lose the mission (End Game): test the defeat screen", "lose", true),
    entry("restart", Group::Flow, "restart", "restart the mission (the R key)", "restart", false),
    entry(
        "ships",
        Group::Ships,
        "ships [text]",
        "id, name, owner, groups, damage, position, visibility and stance of the units whose name, owner or group contains the text",
        "ships raider",
        false,
    ),
    entry(
        "list",
        Group::Authoring,
        "list groups|paths|volumes|points|tasks|timers|flags|counters|players|speech|objects [text]",
        "the names and ids a mission defines, filtered by text: group members (id and name), objects (id, type, position), speech (event id and the start of its subtitle)",
        "list groups raid",
        false,
    ),
    entry(
        "find",
        Group::Authoring,
        "find <text>",
        "search every kind at once (groups, paths, volumes, points, tasks, timers, flags, counters, players, speech, objects, ships, rules)",
        "find gull",
        false,
    ),
    entry("inspect", Group::Ships, "inspect", "the current target: the same, plus its orders and tow", "inspect", false),
    entry("primary", Group::Ships, "primary <group>", "Set Fleet Primary Ship: take command of that ship (a unit is written Group,Unit)", "primary Hero,Escort", true),
    entry(
        "damage",
        Group::Ships,
        "damage <0..1> [group]",
        "Damage Group/Unit by X percent: that share of every section's hit points (default: the player's fleet)",
        "damage 0.3 Raiders",
        true,
    ),
    entry("attack", Group::Ships, "attack <group> <target group>", "GroupA to attack GroupB", "attack Pirates Hero", true),
    entry("stance", Group::Ships, "stance <group> <stance>", "Set Group/Unit AI Stance: dummy, neutral, defensive, aggressive or persistent", "stance Pirates aggressive", true),
    entry("ally", Group::Ships, "ally <player> <player>", "Set alliance between two players to TRUE", "ally Hero Pirates", true),
    entry("hostile", Group::Ships, "hostile <player> <player>", "Set alliance between two players to FALSE", "hostile Hero Pirates", true),
    entry("tow", Group::Ships, "tow <tower> <ship>", "Tow Ship: the first group takes the second in tow", "tow Hero Raiders", true),
    entry("untow", Group::Ships, "untow <group> <group>", "Break Tow between two groups", "untow Hero Raiders", true),
    entry("where", Group::Authoring, "where", "the player's ship position in game units (x east, y north), as mission JSON writes them", "where", false),
    entry(
        "mark",
        Group::Authoring,
        "mark <name>",
        "append the position to console_marks.txt in the user data folder, as a point line to paste into a mission",
        "mark Harbour",
        false,
    ),
    entry(
        "show",
        Group::Authoring,
        "show volumes|paths|points [name]",
        "draw the mission's volumes, paths or points (point sets and objective points) in the world, labelled",
        "show volumes",
        false,
    ),
    entry("hide", Group::Authoring, "hide volumes|paths|points|all [name]", "stop drawing them", "hide all", false),
    entry("cam", Group::Camera, "cam free | player | follow <group>", "a free-flying camera, the orbit camera on the player, or on a group", "cam follow Raiders", false),
    entry("hud", Group::Camera, "hud on|off", "hide or show the HUD, for clean screenshots", "hud off", false),
    entry(
        "fps",
        Group::Camera,
        "fps [on|off]",
        "show or hide a frame-rate counter (smoothed rate, frame time, worst frame of the last second); no argument toggles; remembered between runs",
        "fps on",
        false,
    ),
    entry("say", Group::Camera, "say <speech event>", "Play speech event: a line from the mission's media, without reaching its rule (it counts as played)", "say Opening", true),
    entry("music", Group::Camera, "music <track>", "Play Music Track: a music file, without reaching its rule", "music Battle", false),
    Entry {
        alias: &["do"],
        ..entry(
            "act",
            Group::Raw,
            "act <json>",
            "run any rule action by its Type and fields, checked against the rule schema (JSON true / false are accepted)",
            "act {\"Type\": \"Set Flag Action\", \"Flag Name\": \"x\", \"Boolean Value\": true}",
            true,
        )
    },
    entry(
        "cond",
        Group::Raw,
        "cond <json>",
        "evaluate any rule condition, checked against the schema",
        "cond {\"Type\": \"Flag Condition\", \"Flag Name\": \"x\", \"Boolean Value\": true}",
        false,
    ),
    entry("schema", Group::Raw, "schema <text>", "the fields of an action or condition Type (the rule schema)", "schema Teleport", false),
    Entry { alias: &["?"], ..entry("help", Group::Console, "help [command]", "this list, or a command's usage and an example", "help god", false) },
];

/// The keys of the open console, listed by `help` (kept next to `Console::key_with` and the wheel in `systems/dev_console.rs`).
pub static KEYS: &[(&str, &str)] = &[
    ("Tab", "complete the last word; repeat Tab to cycle, several matches are listed"),
    ("Up / Down", "earlier / later lines of this session"),
    ("PageUp / PageDown, mouse wheel", "scroll the output"),
    ("Ctrl+Home / Ctrl+End", "jump to the top / bottom of the output"),
    ("Escape or the console key", "close the console (the battle is paused while it is open)"),
];

/// Commands the parser answers with a refusal, not an action: they have no table row on purpose.
#[cfg(test)]
const PLANNED: &[&str] = &["spawn"];

fn entry_named(word: &str) -> Option<&'static Entry> {
    TABLE.iter().find(|e| e.name == word || e.alias.contains(&word))
}

/// The command whose name is nearest to `word` (edit distance of at most 2, or a shared prefix), for "unknown command".
fn nearest_command(word: &str) -> Option<&'static str> {
    let word = word.to_lowercase();
    let distance = |a: &str, b: &str| {
        let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
        let mut row: Vec<usize> = (0..=b.len()).collect();
        for (i, ca) in a.iter().enumerate() {
            let mut prev = std::mem::replace(&mut row[0], i + 1);
            for (j, cb) in b.iter().enumerate() {
                let cost = usize::from(ca != cb);
                let here = (row[j] + 1).min(row[j + 1] + 1).min(prev + cost);
                prev = std::mem::replace(&mut row[j + 1], here);
            }
        }
        row[b.len()]
    };
    TABLE.iter().map(|e| e.name).filter(|n| n.starts_with(&word) || word.starts_with(n) || distance(n, &word) <= 2).min_by_key(|n| distance(n, &word))
}

/// Splits a `help` row into its command (or key) and its description. The console font is proportional, so the view draws the two
/// pieces in separate columns instead of padding with spaces.
pub const HELP_COLUMN: char = '\t';

/// What a table row's command needs: that of its example line (the tests parse every example).
pub fn needs_of(e: &Entry) -> Needs {
    parse(e.example, "Player", false).map_or(Needs::Rules, |c| c.needs())
}

/// `help` (every command, grouped) or `help <command>` (its usage and an example), as output lines.
pub fn help_lines(topic: Option<&str>) -> Vec<String> {
    if let Some(word) = topic {
        let word = word.to_lowercase();
        return match entry_named(&word) {
            Some(e) => vec![
                format!("{}: {}", e.name, e.about),
                format!("  usage:   {}", e.usage),
                format!("  example: {}", e.example),
                format!("  runs in: {}", needs_of(e).describe()),
                if e.cheats { "  changes the game: marks the session as cheated".into() } else { "  a query: does not mark the session".into() },
            ],
            None => vec![format!("no command '{word}'{}", nearest_command(&word).map_or(String::new(), |n| format!("; did you mean {n}?")))],
        };
    }
    let mut lines = vec![];
    for group in [Group::Cheats, Group::State, Group::Flow, Group::Ships, Group::Authoring, Group::Camera, Group::Raw, Group::Console] {
        lines.push(format!("{}:", group.title()));
        lines.extend(TABLE.iter().filter(|e| e.group == group).map(|e| {
            let alias = if e.alias.is_empty() { String::new() } else { format!(" (also: {})", e.alias.join(", ")) };
            format!("  {}{}{HELP_COLUMN}{}{alias}", if e.cheats { "*" } else { " " }, e.usage, e.about)
        }));
    }
    lines.push("Keys:".into());
    lines.extend(KEYS.iter().map(|(key, what)| format!("   {key}{HELP_COLUMN}{what}")));
    lines.push("* marks the session as cheated (campaign points are not recorded). `help <command>` shows an example; Tab completes a command.".into());
    lines
}

/// Turns a line into a command. `player` is the human's group name (rules address the fleet by it); `god` the current god state.
pub fn parse(line: &str, player: &str, god: bool) -> Result<Command, String> {
    let line = line.trim();
    let (word, rest) = line.split_once(char::is_whitespace).map_or((line, ""), |(w, r)| (w, r.trim()));
    let owned: Vec<String> = tokens(rest, false).into_iter().map(|t| t.text).collect();
    let args: Vec<&str> = owned.iter().map(String::as_str).collect();
    let act = |a: Value| Command::Acts { acts: vec![a], god: None };
    Ok(match word.to_lowercase().as_str() {
        "" => Command::Nothing,
        "help" | "?" => Command::Help((!rest.is_empty()).then(|| rest.to_string())),
        "god" => {
            let on = match args.as_slice() {
                [] => !god,
                [v] => flag_word(v)?,
                _ => return Err("usage: god [on|off]".into()),
            };
            Command::Acts { acts: vec![vulnerability(player, if on { 1.0 } else { 0.0 })], god: Some(on) }
        }
        "repair" => Command::Repair(if rest.is_empty() { player.to_string() } else { unquote(rest).to_string() }),
        "kill" => Command::Kill,
        "reveal" => Command::Reveal,
        "tp" => match args.as_slice() {
            [x, y] | [x, y, _] if x.parse::<f32>().is_ok() => {
                let n = |s: &str| s.parse::<f32>().map_err(|_| format!("'{s}' is not a number"));
                Command::Teleport(Dest::Xy(n(x)?, n(y)?, args.get(2).map_or(Ok(0.0), |z| n(z))?))
            }
            [] => return Err("usage: tp <x> <y> [z]  or  tp <point set name>".into()),
            _ => Command::Teleport(Dest::Set(unquote(rest).to_string())),
        },
        "spawn" => return Err("spawn: no rule action creates ships yet, so there is nothing to run".into()),
        "fire" if !rest.is_empty() => Command::Fire(unquote(rest).into()),
        "why" if !rest.is_empty() => Command::Why(unquote(rest).into()),
        "fire" | "why" => return Err(format!("usage: {word} <rule name>")),
        "flags" => Command::Flags,
        "timers" => Command::Timers,
        "tasks" => Command::Tasks,
        "rules" => Command::Rules((!rest.is_empty()).then(|| unquote(rest).to_string())),
        "skip" => Command::Skip,
        "nis" => match args.as_slice() {
            ["end"] => act(json!({"Type": "NIS End"})),
            _ => return Err("usage: nis end".into()),
        },
        "win" => Command::Win,
        "lose" => Command::Lose,
        "restart" => Command::Restart,
        "ships" => Command::Ships((!rest.is_empty()).then(|| unquote(rest).to_string())),
        "inspect" => Command::Inspect,
        "list" => match args.split_first() {
            Some((k, text)) => match list::parse_kind(k) {
                Some(kind) => Command::List(kind, text.join(" ")),
                None => return Err(format!("'{k}': list takes {}", list::KINDS.join(", "))),
            },
            None => return Err(format!("usage: list {} [text]", list::KINDS.join("|"))),
        },
        "find" if !rest.is_empty() => Command::Find(unquote(rest).to_string()),
        "find" => return Err("usage: find <text>".into()),
        "primary" if !rest.is_empty() => act(json!({"Type": "Set Fleet Primary Ship", "Group/Unit Name": unquote(rest)})),
        "primary" => return Err("usage: primary <group>".into()),
        "damage" => match args.as_slice() {
            [f] | [f, _] => match f.parse::<f32>() {
                Ok(p) if (0.0..=1.0).contains(&p) => act(json!({"Type": "Damage Group/Unit by X percent", "Group Name": args.get(1).copied().unwrap_or(player), "Percent": p})),
                _ => return Err(format!("'{f}': the damage is a share between 0 and 1")),
            },
            _ => return Err("usage: damage <0..1> [group]".into()),
        },
        "attack" => match args.as_slice() {
            [a, b] => act(json!({"Type": "GroupA to attack GroupB", "Attack Group": a, "Target Group": b})),
            _ => return Err("usage: attack <group> <target group> (put a name with spaces in \"quotes\")".into()),
        },
        "stance" => match args.as_slice() {
            [g, st] => {
                let name = match st.to_lowercase().as_str() {
                    "dummy" => "Stance_Dummy",
                    "neutral" => "Stance_Neutral",
                    "defensive" => "Stance_Defensive",
                    "aggressive" => "Stance_Aggressive",
                    "persistent" => "Stance Persistent",
                    other => return Err(format!("'{other}': expected dummy, neutral, defensive, aggressive or persistent")),
                };
                act(json!({"Type": "Set Group/Unit AI Stance", "Group/Unit Name": g, "AI Stance": name}))
            }
            _ => return Err("usage: stance <group> <stance>".into()),
        },
        "ally" | "hostile" => match args.as_slice() {
            [a, b] => act(
                json!({"Type": "Set alliance between PlayerA and PlayerB to TRUE/FALSE", "PLAYER A": a, "PLAYER B": b, "SET ALLIANCE TO": wot(word.eq_ignore_ascii_case("ally"))}),
            ),
            _ => return Err(format!("usage: {word} <player> <player>")),
        },
        "tow" => match args.as_slice() {
            [a, b] => act(json!({"Type": "Tow Ship", "Docking Group": a, "Target to Tow": b})),
            _ => return Err("usage: tow <tower group> <ship group>".into()),
        },
        "untow" => match args.as_slice() {
            [a, b] => act(json!({"Type": "Break Tow", "TowerGroupA": a, "TowerGroupB": b})),
            _ => return Err("usage: untow <group> <group>".into()),
        },
        "where" => Command::Where,
        "mark" if !rest.is_empty() => Command::Mark(unquote(rest).to_string()),
        "mark" => return Err("usage: mark <name>".into()),
        "show" | "hide" => {
            let kind = args.first().and_then(|k| ShapeKind::parse(k));
            let name = (args.len() > 1).then(|| args[1..].join(" "));
            match (word.eq_ignore_ascii_case("show"), kind, args.first()) {
                (true, Some(k), _) => Command::Show(k, name),
                (false, Some(k), _) => Command::Hide(Some(k), name),
                (false, None, Some(a)) if a.eq_ignore_ascii_case("all") => Command::Hide(None, None),
                _ => return Err(format!("usage: {word} volumes|paths|points{} [name]", if word.eq_ignore_ascii_case("hide") { "|all" } else { "" })),
            }
        }
        "cam" => match args.as_slice() {
            ["free"] => Command::Cam(CamMode::Free),
            ["player" | "off"] => Command::Cam(CamMode::Player),
            ["follow", g] => Command::Cam(CamMode::Follow(g.to_string())),
            _ => return Err("usage: cam free | player | follow <group>".into()),
        },
        "hud" => match args.as_slice() {
            [v] => Command::Hud(flag_word(v)?),
            _ => return Err("usage: hud on|off".into()),
        },
        "fps" => match args.as_slice() {
            [] => Command::Fps(None),
            [v] => Command::Fps(Some(flag_word(v)?)),
            _ => return Err("usage: fps [on|off]".into()),
        },
        "say" if !rest.is_empty() => act(json!({"Type": "Play speech event", "Speech Event Name": unquote(rest)})),
        "say" => return Err("usage: say <speech event>".into()),
        "music" if !rest.is_empty() => Command::Soft(vec![
            json!({"Type": "Play Music Track", "File Name": unquote(rest), "Fade In Time ( secs )": 0.0, "Fade Out Time ( secs )": 0.0, "New Volume ( 0 to 1 )": 1.0}),
        ]),
        "music" => return Err("usage: music <track>".into()),
        "counters" => Command::Counters,
        "counter" => match args.as_slice() {
            [name, value] => match value.parse::<f32>() {
                Ok(v) => act(json!({"Type": "OP Set Counter", "Counter Name": name, "Value": v})),
                Err(_) => return Err(format!("'{value}' is not a number")),
            },
            _ => return Err("usage: counter <name> <value>".into()),
        },
        "schema" if !rest.is_empty() => Command::Schema(unquote(rest).into()),
        "schema" => return Err("usage: schema <text>, e.g. schema Teleport".into()),
        "set" => match args.as_slice() {
            [] => return Err("usage: set <flag> [on|off]".into()),
            [name] => act(set_flag(name, true)),
            [name, v] => act(set_flag(name, flag_word(v)?)),
            _ => return Err("usage: set <flag> [on|off] (put a name with spaces in \"quotes\")".into()),
        },
        "timer" => match args.as_slice() {
            [name, "start"] => act(json!({"Type": "Start Timer", "Timer Name": name})),
            [name, "stop"] => act(json!({"Type": "Stop Timer", "Timer Name": name})),
            _ => return Err("usage: timer <name> start|stop".into()),
        },
        "task" => {
            let state_at = args.iter().rposition(|a| matches!(*a, "active" | "complete" | "failed"));
            let Some(k) = state_at.filter(|k| *k > 0 && args.len() <= k + 2) else {
                return Err("usage: task <name> active|complete|failed [on|off] (put a name with spaces in \"quotes\")".into());
            };
            let on = args.get(k + 1).map_or(Ok(true), |v| flag_word(v))?;
            let (kind, key) = match args[k] {
                "active" => ("Set Objective Task Active State", "Active State"),
                "complete" => ("Set Objective Task Complete State", "Complete State"),
                _ => ("Set Objective Task Failed State", "Failed State"),
            };
            act(json!({"Type": kind, "Objective Task": args[..k].join(" "), key: wot(on)}))
        }
        "act" | "do" => match serde_json::from_str::<Value>(rest) {
            Ok(v) if v["Type"].is_string() => {
                validate(Role::Action, &v)?;
                Command::Acts { acts: vec![strings(v)], god: None }
            }
            Ok(_) => return Err("an action needs a \"Type\"".into()),
            Err(e) => return Err(format!("not JSON: {e}")),
        },
        "cond" => match serde_json::from_str::<Value>(rest) {
            Ok(v) if v["Type"].is_string() => {
                validate(Role::Condition, &v)?;
                Command::Cond(strings(v))
            }
            Ok(_) => return Err("a condition needs a \"Type\"".into()),
            Err(e) => return Err(format!("not JSON: {e}")),
        },
        other => return Err(format!("unknown command '{other}'{}; `help` lists the commands", nearest_command(other).map_or(String::new(), |n| format!(" (did you mean {n}?)")))),
    })
}

fn flag_word(v: &str) -> Result<bool, String> {
    match v.to_lowercase().as_str() {
        "on" | "true" | "1" | "yes" => Ok(true),
        "off" | "false" | "0" | "no" => Ok(false),
        _ => Err(format!("'{v}': expected on or off")),
    }
}
fn wot(on: bool) -> &'static str {
    if on { "TRUE" } else { "FALSE" }
}
fn unquote(s: &str) -> &str {
    s.trim().trim_matches('"')
}
/// The rules' booleans are the strings TRUE / FALSE; JSON true / false typed by hand mean the same.
fn strings(v: Value) -> Value {
    match v {
        Value::Bool(b) => Value::String(wot(b).into()),
        Value::Array(a) => Value::Array(a.into_iter().map(strings).collect()),
        Value::Object(m) => Value::Object(m.into_iter().map(|(k, v)| (k, strings(v))).collect()),
        other => other,
    }
}

/// 'Set Group/Unit Vulnerability': 1.0 takes no damage, 0.0 is normal.
pub fn vulnerability(group: &str, percent: f32) -> Value {
    json!({"Type": "Set Group/Unit Vulnerability", "Group Name": group, "Invulnerable Percent": percent})
}
fn set_flag(name: &str, on: bool) -> Value {
    json!({"Type": "Set Flag Action", "Flag Name": name, "Boolean Value": wot(on)})
}
/// 'Destroy Group/Unit' for one unit, by its 'Group,Unit' spec.
pub fn destroy(spec: &str) -> Value {
    json!({"Type": "Destroy Group/Unit", "Group Name": spec})
}
pub fn visibility(group: &str, visible: bool) -> Value {
    json!({"Type": "Set Group/Unit Visibility", "Group/Unit Name": group, "Boolean": wot(visible)})
}
pub fn teleport(spec: &str, point_set: &str) -> Value {
    json!({"Type": "Teleport Group/Unit", "Group/Unit Name": spec, "Point Set Name": point_set})
}
/// The spec of the human's command ship.
pub fn flagship(player: &str) -> String {
    format!("{player},HUMAN CONTROLLED COMMAND SHIP")
}

/// What a command did besides the lines it prints.
#[derive(Default)]
pub struct Outcome {
    pub lines: Vec<String>,
    /// Units to repair fully (`repair`).
    pub repair: Vec<Entity>,
    /// A name in the command is not in the mission: nothing ran.
    pub rejected: bool,
    /// End the speech line that is playing (the ECS side stops its sound).
    pub skip_voice: bool,
    /// Restart the mission.
    pub restart: bool,
    /// Changes to the developer view (`DevView`).
    pub view: Vec<ViewChange>,
}

impl Script {
    /// Runs a command against the unit list `w` (what `systems::units` builds each frame). ECS effects go to `out`, as in `step`.
    /// `target`: the player's current target.
    pub fn console_run(&mut self, cmd: &Command, w: &mut [Unit], target: Option<Entity>, out: &mut Vec<Fx>) -> Outcome {
        self.console_run_with(cmd, w, &Env { target, ..Env::default() }, out)
    }
    /// `console_run` with what the ECS side knows (`Env`).
    pub fn console_run_with(&mut self, cmd: &Command, w: &mut [Unit], env: &Env, out: &mut Vec<Fx>) -> Outcome {
        let target = env.target;
        let mut o = Outcome::default();
        if let Some(why) = cmd.refusal(&env.word, env.caps) {
            o.lines.push(why);
            o.rejected = true;
            return o;
        }
        if let Err(e) = self.check_names(cmd, w) {
            o.lines.push(e);
            o.rejected = true;
            return o;
        }
        let player = self.player.clone();
        match cmd {
            Command::Nothing => {}
            Command::Help(topic) => o.lines = help_lines(topic.as_deref()),
            Command::Acts { acts, .. } | Command::Soft(acts) => {
                for a in acts {
                    self.console_act(a, w, out);
                }
                o.lines.push(format!("ran {}", acts.iter().map(|a| s(a, "Type")).collect::<Vec<_>>().join(", ")));
            }
            Command::Cond(c) => {
                let held = self.console_cond(c, w);
                o.lines.push(held.to_string());
            }
            Command::Reveal => {
                let mut owners: Vec<&str> = w.iter().filter(|u| !u.owner.is_empty()).map(|u| u.owner.as_str()).collect();
                owners.sort_unstable();
                owners.dedup();
                let hidden = w.iter().filter(|u| !u.visible).count();
                for owner in owners.into_iter().map(String::from).collect::<Vec<_>>() {
                    self.console_act(&visibility(&owner, true), w, out);
                }
                o.lines.push(format!("{hidden} hidden unit(s) shown"));
            }
            Command::Kill => match target.and_then(|t| w.iter().find(|u| u.e == t)) {
                Some(u) if !u.name.is_empty() => {
                    let (spec, name) = (format!("{},{}", u.owner, u.name), u.name.clone());
                    let (from, hit) = (out.len(), u.e);
                    self.console_act(&destroy(&spec), w, out);
                    // A 'Group,Unit' spec selects by the unit's name alone, so another unit of the same name would go too: keep the target.
                    for fx in &mut out[from..] {
                        if let Fx::Destroy(ids) = fx {
                            ids.retain(|e| *e == hit);
                        }
                    }
                    o.lines.push(format!("destroyed {name}"));
                }
                Some(_) => o.lines.push("the target has no unit name for a rule to address".into()),
                None => o.lines.push("no target: click a ship first".into()),
            },
            Command::Teleport(dest) => {
                let set = match dest {
                    Dest::Xy(x, y, z) => {
                        self.point_sets[TP_SET] = json!([{"pos": [x, y, z]}]);
                        TP_SET.to_string()
                    }
                    Dest::Set(name) if self.point_sets[name.as_str()].get(0).is_some() => name.clone(),
                    Dest::Set(name) => {
                        let mut names: Vec<&str> = self.point_sets.as_object().into_iter().flatten().map(|(k, _)| k.as_str()).filter(|k| *k != TP_SET).collect();
                        names.sort_unstable();
                        o.lines.push(format!("no point set '{name}'{}; the mission has {}", mean(name, names.iter().copied()), names.join(", ")));
                        return o;
                    }
                };
                self.console_act(&teleport(&flagship(&player), &set), w, out);
                o.lines.push(format!(
                    "teleported to {}",
                    match dest {
                        Dest::Xy(x, y, _) => format!("{x}, {y}"),
                        Dest::Set(n) => n.clone(),
                    }
                ));
            }
            Command::Repair(spec) => {
                o.repair = ids(w, spec);
                o.lines.push(format!("repairing {} unit(s) of {spec}", o.repair.len()));
            }
            Command::Fire(name) => match self.rule_named(name) {
                Ok(i) => {
                    let r = &self.rules[i];
                    let rule = Rule { name: r.name.clone(), once: r.once, conds: vec![], acts: r.acts.clone() };
                    if rule.once {
                        self.fired[i] = true;
                    }
                    self.mark_ran(i);
                    info!("t={:.1} rule #{i} fired from the console: {}", self.time, rule.name.trim());
                    self.run_from(i, &rule, 0, w, out);
                    o.lines.push(format!("fired '{}' ({} action(s))", rule.name.trim(), rule.acts.len()));
                }
                Err(e) => o.lines.push(e),
            },
            Command::Why(name) => match self.rule_named(name) {
                Ok(i) => o.lines = self.why(i, w),
                Err(e) => o.lines.push(e),
            },
            Command::Flags => {
                o.lines = self.flag_lines();
                if o.lines.is_empty() {
                    o.lines.push("no flag has been set".into());
                }
            }
            Command::Timers => {
                o.lines = self.timer_lines();
                if o.lines.is_empty() {
                    o.lines.push("no timer has been started".into());
                }
            }
            Command::Schema(query) => o.lines = schema_lines(query),
            Command::Rules(_)
            | Command::Skip
            | Command::Win
            | Command::Lose
            | Command::Restart
            | Command::Ships(_)
            | Command::List(..)
            | Command::Find(_)
            | Command::Inspect
            | Command::Where
            | Command::Mark(_)
            | Command::Show(..)
            | Command::Hide(..)
            | Command::Cam(_)
            | Command::Fps(_)
            | Command::Hud(_) => self.run_more(cmd, w, env, out, &mut o),
            Command::Counters => {
                o.lines = self.counter_lines();
                if o.lines.is_empty() {
                    o.lines.push("no counter has been set".into());
                }
            }
            Command::Tasks => {
                o.lines = self.task_lines();
                if o.lines.is_empty() {
                    o.lines.push("the mission has no tasks".into());
                }
            }
        }
        o
    }

    /// One action, with a visibility change shown to the actions after it (as `run_acts` does for a rule).
    fn console_act(&mut self, a: &Value, w: &mut [Unit], out: &mut Vec<Fx>) {
        let from = out.len();
        self.act(a, w, out, "(console)");
        for fx in &out[from..] {
            if let Fx::Visible(ids, visible) = fx {
                for u in w.iter_mut().filter(|u| ids.contains(&u.e)) {
                    u.visible = *visible;
                }
            }
        }
    }

    /// A condition asked once from outside the rule loop: the edge state the rules' own evaluation depends on is put back after it.
    fn console_cond(&mut self, c: &Value, w: &[Unit]) -> bool {
        let saved = (self.prev_inside.clone(), self.entered.clone(), self.events.clone(), self.mortar_seen.clone());
        let held = self.cond(c, w, false);
        (self.prev_inside, self.entered, self.events, self.mortar_seen) = saved;
        self.consume.clear();
        held
    }

    /// The rule a (part of a) name stands for: the exact name, else the only rule whose name contains it (any case).
    fn rule_named(&self, name: &str) -> Result<usize, String> {
        let name = name.trim();
        if let Some(i) = self.rules.iter().position(|r| r.name.trim() == name) {
            return Ok(i);
        }
        let found: Vec<usize> = (0..self.rules.len()).filter(|&i| self.rules[i].name.to_lowercase().contains(&name.to_lowercase())).collect();
        match found.as_slice() {
            [i] => Ok(*i),
            [] => Err(format!("no rule named '{name}'{}", mean(name, self.rules.iter().map(|r| r.name.trim())))),
            many => Err(format!("'{name}' matches {} rules: {}", many.len(), many.iter().take(8).map(|&i| self.rules[i].name.trim()).collect::<Vec<_>>().join("; "))),
        }
    }

    /// Each condition of rule `i` with whether it holds now, and whether the rule already ran.
    fn why(&mut self, i: usize, w: &[Unit]) -> Vec<String> {
        let (name, once, conds) = (self.rules[i].name.trim().to_string(), self.rules[i].once, self.rules[i].conds.clone());
        let mut lines = vec![format!(
            "{name}: {}{}",
            if self.runs[i].count > 0 { "has run" } else { "has not run" },
            if once && self.fired[i] { " (run-once: it will not run again)" } else { "" }
        )];
        if !self.ext.is_active(i) {
            lines.push("  inactive (\"Is Active\" is false or OP Set Rule Active turned it off): its conditions are not evaluated; OP Set Rule Active turns it on".into());
        }
        if self.ext.busy(i) {
            lines.push(format!("  {}", self.ext.busy_note(i)));
        }
        let mut all = true;
        for c in &conds {
            let held = self.console_cond(c, w);
            all &= held;
            lines.push(format!("  {} {}", if held { "yes" } else { "NO " }, self.explain(c, w)));
        }
        if conds.is_empty() {
            lines.push("  (no conditions)".into());
        } else if all && !(once && self.fired[i]) && self.ext.is_active(i) && !self.ext.busy(i) {
            lines.push("every condition holds now: the rule runs on the next step".into());
        }
        lines
    }
}

/// Checks a raw action or condition against the rule schema: a known Type, no field it does not have, none of its required fields
/// missing. The interpreter itself ignores what it does not know; at the console that would be a silent no-op.
pub fn validate(role: Role, v: &Value) -> Result<(), String> {
    let ty = s(v, "Type");
    let what = if role == Role::Action { "action" } else { "condition" };
    let Some(def) = schema::lookup(role, ty) else {
        let near: Vec<&str> = schema::RULE_TYPES
            .iter()
            .filter(|t| t.role == role && (t.name.to_lowercase().contains(&ty.to_lowercase()) || ty.to_lowercase().contains(&t.name.to_lowercase())))
            .map(|t| t.name)
            .take(5)
            .collect();
        let hint = if near.is_empty() { "`schema <text>` searches the Types".to_string() } else { format!("did you mean: {}", near.join("; ")) };
        return Err(format!("no {what} Type '{ty}' ({hint})"));
    };
    let unknown: Vec<&str> = v.as_object().into_iter().flatten().map(|(k, _)| k.as_str()).filter(|k| *k != "Type" && def.field(k).is_none()).collect();
    if !unknown.is_empty() {
        return Err(format!("'{ty}' has no field {}; its fields are: {}", unknown.join(", "), def.fields.iter().map(|f| f.name).collect::<Vec<_>>().join(", ")));
    }
    let missing: Vec<&str> = def.fields.iter().filter(|f| f.required && v[f.name].is_null()).map(|f| f.name).collect();
    if !missing.is_empty() {
        return Err(format!("'{ty}' needs {}", missing.join(", ")));
    }
    Ok(())
}

/// The schema entries (actions and conditions) whose Type is `query` or contains it (any case), one line each, then their fields.
pub fn schema_lines(query: &str) -> Vec<String> {
    let q = query.trim().to_lowercase();
    let mut types: Vec<&schema::TypeDef> = schema::RULE_TYPES.iter().filter(|t| t.name.to_lowercase() == q).collect();
    if types.is_empty() {
        types = schema::RULE_TYPES.iter().filter(|t| t.name.to_lowercase().contains(&q)).collect();
    }
    let mut lines = vec![];
    match types.len() {
        0 => lines.push(format!("no Type contains '{query}'")),
        1..=3 => {
            for t in types {
                lines.push(format!(
                    "{} {}{}",
                    if t.role == Role::Action { "action   " } else { "condition" },
                    t.name,
                    if t.origin == schema::Origin::Engine { "  (engine-added)" } else { "" }
                ));
                for f in t.fields.iter().filter(|f| f.used) {
                    lines.push(format!(
                        "  {} ({}){} default {}",
                        f.name,
                        f.kind.name(),
                        if f.required { " required" } else { "" },
                        if f.default.is_empty() { "\"\"" } else { f.default }
                    ));
                }
            }
        }
        n => {
            lines.push(format!("{n} Types match; a longer text narrows it:"));
            lines.extend(types.iter().take(30).map(|t| format!("  {} {}", if t.role == Role::Action { "action   " } else { "condition" }, t.name)));
        }
    }
    lines
}

#[cfg(test)]
mod tests;
