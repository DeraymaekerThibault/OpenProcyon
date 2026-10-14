//! Front-end model shared by both presentations: Main Menu -> Single Player -> Create Skirmish Game -> Skirmish Game Lobby -> battle.
//! Owns the screen state, the lobby/fleet choices (`Skirmish`), saves, the in-game Esc menu and the `Action`s buttons carry.
//! Views (`classic/menu.rs` retail screens) draw the current screen in `BuildMenu` from `MenuModel`
//! and tag their buttons with an `Action`; `buttons` / `save_actions` here run them. Maps: assets/maps/<id> (tools/export_maps.py).

#![allow(deprecated)] // shortcut: bevy 0.20 Interaction (deprecated for picking Hovered/Pressed); port when it is removed
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde_json::Value;

use crate::bindings::Bindings;
use crate::campaign::{CampaignPoints, CampaignSave};
use crate::{assets_dir, combat::Library};

#[derive(States, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Screen {
    #[default]
    Main,
    SinglePlayer,
    CreateSkirmish,
    Lobby,
    Fleet,
    Load,
    Options,
    Controls,
    /// Main menu > Tutorial: Basic Training / Full Training (`Tutorial Screen.tst`).
    Tutorial,
    /// Options > Sound Options (`Audio_Options.tst`) and Display Options (`Display Options Screen.tst`).
    Audio,
    Display,
    Battle,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Human,
    Computer,
    Closed,
}

/// One lobby row = one playable player of the map (docs/re/skirmish-setup.md).
#[derive(Clone, PartialEq, Debug)]
pub struct Slot {
    pub player: usize, // index in the map's player list
    pub name: String,
    pub kind: Kind,
    pub race: u8,  // fleet type as in the TWT: 0 Navy, 2 Pirate, 3 Procyon, 4 none chosen yet
    pub team: i32, // -1: on its own
    pub fleet: Vec<String>,
    pub weapons: Vec<std::collections::BTreeMap<usize, String>>,
}

/// What the front end has chosen; read by the battle setup.
#[derive(Resource)]
pub struct Skirmish {
    pub map: Option<String>,
    pub slots: Vec<Slot>,
    pub maps: Vec<(String, String, bool, usize)>, // id, name, open (MustAssembleFleet), playable players
    pub selected: usize,
    pub historical: bool,
    pub fleet_size: usize,
    /// Victory points a fleet may cost (the setup's Victory Limit; ships spend their 'Victory Point Cost').
    pub victory_limit: u32,
    pub map_json: Value,
    pub error: Option<&'static str>, // string id (or plain text) shown under the player list
    pub ship_pick: usize,
    pub fleet_pick: usize,
    pub arms_open: bool,
    /// The lobby's Map Description dialog ('Map Info') is open.
    pub map_info: bool,
    /// First row of the classic fixed-height ship list.
    pub scroll: usize,
}

/// Ordering handle for the system that runs `Action`s on pressed buttons.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub struct RunButtons;
/// Views draw the current screen here: after key capture, before `RunButtons`, only while a menu is shown.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub struct BuildMenu;
#[derive(Component)]
pub struct MenuRoot;
#[derive(Component, Clone)]
pub enum Action {
    Goto(Screen),
    Quit,
    PickMap(usize),
    Historical(bool),
    FleetSize,
    /// Step the victory point limit (open maps).
    VictoryLimit,
    Claim(usize),
    ToggleClosed(usize),
    CycleRace(usize),
    CycleTeam(usize),
    PickShip(usize),
    PickFleet(usize),
    AddShip,
    /// Fill the fleet within the victory points.
    AutoAssign,
    DropShip,
    OpenArms,
    Launch,
    Campaign,
    /// Start a tutorial map ("basic_tutorial" or "tutorial"), run by the same rule interpreter as the campaign.
    Tutorial(&'static str),
    OpenLoad,
    Ingame(Option<Page>),
    PickSave(usize),
    DoLoad,
    DoSave,
    Remove,
    /// Open or close the lobby's Map Description dialog.
    MapInfo(bool),
    QuitToMenu,
    QuickSave,
    QuickLoad,
    Rebind(usize),
    RestoreKeys,
    /// Step one Sound / Display option (`Settings`).
    Setting(Setting),
}

impl Action {
    /// In game, "go to screen" means "show this in-game page" instead.
    pub fn with_page(self, page: Option<Page>) -> Self {
        match page {
            Some(p) => Action::Ingame(Some(p)),
            None => self,
        }
    }
}

/// Pages of the retail in-game menu (Esc): the menu itself and the Load / Save lists (InGameMenu_Default / _Load_Campaign.tst).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Menu,
    Load,
    Save,
    Options,
    Controls,
    Audio,
    Display,
}
/// The in-game menu is an overlay on the running battle (not a `Screen`), so the battle stays loaded; it pauses the simulation.
#[derive(Resource, Default)]
pub struct InGame {
    page: Option<Page>,
    paused_by_us: bool,
}
impl InGame {
    pub fn page(&self) -> Option<Page> {
        self.page
    }
}
/// Run condition: no in-game menu is open (battle input systems use it so the menu does not click through).
pub fn overlay_closed(ig: Res<InGame>) -> bool {
    ig.page.is_none()
}
/// The front end, or the in-game menu over a battle.
pub fn menu_active(state: Res<State<Screen>>, ig: Res<InGame>) -> bool {
    *state.get() != Screen::Battle || ig.page.is_some()
}

/// One adjustable option of the Sound / Display screens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Setting {
    /// Volume channels (retail Volume Overall / Music / Effect / Crew Dialogue), stepped by 10% and wrapping.
    Master,
    Music,
    Effects,
    Dialogue,
    /// Video mode (windowed / fullscreen), effect detail (low / medium / high), render background, flag waving.
    Fullscreen,
    Detail,
    Background,
    Flags,
    RestoreDisplay,
}

/// Sound and display options, saved as `settings.json` in the user folder. PROVISIONAL: the retail sound channel/detail options
/// and texture/model detail have no remake equivalent; effect detail scales particle emission (Low 50%, Medium 75%, High 100%).
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Settings {
    pub master: u8,
    pub music: u8,
    pub effects: u8,
    pub dialogue: u8,
    pub fullscreen: bool,
    pub detail: u8,
    pub background: bool,
    pub flags: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Settings { master: 10, music: 10, effects: 10, dialogue: 10, fullscreen: false, detail: 2, background: true, flags: true }
    }
}
static CHANNELS: [std::sync::atomic::AtomicU8; 4] =
    [std::sync::atomic::AtomicU8::new(10), std::sync::atomic::AtomicU8::new(10), std::sync::atomic::AtomicU8::new(10), std::sync::atomic::AtomicU8::new(10)];
static DETAIL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);
/// Volume factor 0..1 of a channel (`Setting::Music`, `Effects` or `Dialogue`), for code that spawns sounds; the master volume is
/// Bevy's global volume.
pub fn volume(c: Setting) -> f32 {
    let i = match c {
        Setting::Music => 1,
        Setting::Effects => 2,
        Setting::Dialogue => 3,
        _ => 0,
    };
    CHANNELS[i].load(std::sync::atomic::Ordering::Relaxed) as f32 / 10.0
}
/// Particle emission factor of the effect detail option.
pub fn detail_scale() -> f32 {
    [0.5, 0.75, 1.0][DETAIL.load(std::sync::atomic::Ordering::Relaxed).min(2) as usize]
}
impl Settings {
    fn path() -> Option<std::path::PathBuf> {
        Some(crate::campaign::user_dir()?.join("settings.json"))
    }
    pub fn load() -> Self {
        let v: Value = Self::path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let d = Self::default();
        let n = |k: &str, def: u8, max: u8| v[k].as_u64().map_or(def, |x| (x as u8).min(max));
        let b = |k: &str, def: bool| v[k].as_bool().unwrap_or(def);
        Settings {
            master: n("master", d.master, 10),
            music: n("music", d.music, 10),
            effects: n("effects", d.effects, 10),
            dialogue: n("dialogue", d.dialogue, 10),
            fullscreen: b("fullscreen", d.fullscreen),
            detail: n("detail", d.detail, 2),
            background: b("background", d.background),
            flags: b("flags", d.flags),
        }
    }
    fn save(&self) {
        let Some(p) = Self::path() else { return };
        let _ = p.parent().map(std::fs::create_dir_all);
        let v = serde_json::json!({"master": self.master, "music": self.music, "effects": self.effects, "dialogue": self.dialogue,
            "fullscreen": self.fullscreen, "detail": self.detail, "background": self.background, "flags": self.flags});
        if let Err(e) = std::fs::write(p, serde_json::to_string_pretty(&v).unwrap_or_default()) {
            warn!("could not save settings: {e}");
        }
    }
    /// One click on an option: volumes step +10% (wrapping to 0), detail cycles, toggles flip.
    pub fn step(&mut self, s: Setting) {
        let up = |v: &mut u8| *v = if *v >= 10 { 0 } else { *v + 1 };
        match s {
            Setting::Master => up(&mut self.master),
            Setting::Music => up(&mut self.music),
            Setting::Effects => up(&mut self.effects),
            Setting::Dialogue => up(&mut self.dialogue),
            Setting::Fullscreen => self.fullscreen = !self.fullscreen,
            Setting::Detail => self.detail = (self.detail + 1) % 3,
            Setting::Background => self.background = !self.background,
            Setting::Flags => self.flags = !self.flags,
            Setting::RestoreDisplay => {
                let d = Self::default();
                (self.fullscreen, self.detail, self.background, self.flags) = (d.fullscreen, d.detail, d.background, d.flags);
            }
        }
    }
    /// (label, value) rows a view shows for the Sound or Display screen.
    pub fn rows(&self, display: bool) -> Vec<(Setting, &'static str, String)> {
        let pct = |v: u8| format!("{}%", v as u32 * 10);
        let on = |b: bool| if b { "On" } else { "Off" }.to_string();
        if display {
            vec![
                (Setting::Fullscreen, "Video Mode", if self.fullscreen { "Fullscreen" } else { "Windowed" }.into()),
                (Setting::Detail, "Effect Detail", ["Low", "Medium", "High"][self.detail.min(2) as usize].into()),
                (Setting::Background, "Render Background", on(self.background)),
                (Setting::Flags, "Flag Waving", on(self.flags)),
            ]
        } else {
            vec![
                (Setting::Master, "Volume Overall", pct(self.master)),
                (Setting::Music, "Volume Music", pct(self.music)),
                (Setting::Effects, "Volume Effect", pct(self.effects)),
                (Setting::Dialogue, "Volume Crew Dialogue", pct(self.dialogue)),
            ]
        }
    }
}

/// Pushes the options into the running app: global volume, channel factors, window mode, detail.
fn apply_settings(settings: Res<Settings>, mut global: ResMut<GlobalVolume>, mut window: Query<&mut Window, With<bevy::window::PrimaryWindow>>) {
    if !settings.is_changed() {
        return;
    }
    global.volume = bevy::audio::Volume::Linear(settings.master as f32 / 10.0);
    for (i, v) in [settings.master, settings.music, settings.effects, settings.dialogue].into_iter().enumerate() {
        CHANNELS[i].store(v, std::sync::atomic::Ordering::Relaxed);
    }
    DETAIL.store(settings.detail, std::sync::atomic::Ordering::Relaxed);
    let mode = if settings.fullscreen { bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current) } else { bevy::window::WindowMode::Windowed };
    for mut w in &mut window {
        if w.mode != mode {
            w.mode = mode;
        }
    }
}

/// Control Options screen: first visible row and the row waiting for a key press.
#[derive(Resource, Default)]
pub struct CtrlUi {
    /// First row of the classic fixed-height list.
    pub scroll: usize,
    pub capture: Option<usize>,
}

/// State of the Load / Save lists (front-end Load Campaign screen and the in-game pages).
#[derive(Resource, Default)]
pub struct SaveUi {
    pub list: Vec<(CampaignSave, bool)>,
    pub selected: Option<usize>,
    /// First row of the classic fixed-height list.
    pub scroll: usize,
    pub name: String,
    pub status: Option<&'static str>, // string id (or plain text) shown above the list
}
impl SaveUi {
    fn open(&mut self) {
        *self = Self { list: crate::campaign::list_saves(), ..default() };
    }
}

pub const RACES: [(u8, &str, &str); 3] =
    [(0, "Navy", "IDGS_TPFRONTENDTEXT03_FLEET_NAVY"), (2, "Pirate", "IDGS_TPFRONTENDTEXT03_FLEET_PIRATE"), (3, "Procyon", "IDGS_TPFRONTENDTEXT03_FLEET_PROCYON")];
/// Hulls the retail fleet screen is not expected to offer: story/NIS/tutorial variants, lifeboats, wrecks, targets.
/// PROVISIONAL: the exe lists every ship type of the chosen race; this name filter stands in for whatever hides these.
/// The original's Navy list includes the StarHammer and the Heavy Scout (Legacy hull), so those are not filtered.
const NOT_OFFERED: [&str; 10] = ["NIS", "Tutorial", "Feather", "Jim", "Amelia", "Wrecked", "Lifeboat", "Target", "FastTurn", "WeaponBarge"];

fn next_race(r: u8) -> u8 {
    match r {
        0 => 2,
        2 => 3,
        _ => 0,
    }
}

/// Ship classes the fleet screen offers for a race (mods can opt hulls in with `"skirmish": true`).
pub fn offered(lib: &Library, race: u8) -> Vec<usize> {
    let Some((_, name, _)) = RACES.iter().find(|r| r.0 == race) else { return vec![] };
    let mut list: Vec<usize> =
        (0..lib.classes.len()).filter(|&i| lib.classes[i].race == *name && (lib.classes[i].skirmish || !NOT_OFFERED.iter().any(|n| lib.classes[i].name.contains(n)))).collect();
    // As the original's 'Available Ships': most expensive first (StarHammer, Man-O-War, Frigate, ...).
    list.sort_by(|&a, &b| lib.classes[b].points.cmp(&lib.classes[a].points).then_with(|| lib.classes[a].name.cmp(&lib.classes[b].name)));
    list
}

/// Computer fleet: `n` armed hulls of the race, spread over the offered list. PROVISIONAL (the exe's AI fleet choice and victory-point budget are not decoded).
fn auto_fleet(lib: &Library, race: u8, n: usize, salt: usize) -> Vec<String> {
    let pool: Vec<usize> = offered(lib, race).into_iter().filter(|&i| !lib.classes[i].mounts.is_empty()).collect();
    if pool.is_empty() {
        return vec![];
    }
    (0..n).map(|k| lib.classes[pool[(k * 5 + salt) % pool.len()]].name.clone()).collect()
}

pub fn pretty(name: &str) -> String {
    name.trim_start_matches("Ship_").replace('_', " ")
}

fn ships_of(map: &Value, player: usize) -> Vec<String> {
    map["ships"].as_array().into_iter().flatten().filter(|s| s["player"].as_u64() == Some(player as u64)).filter_map(|s| s["type"].as_str().map(String::from)).collect()
}

/// Lobby rows of a map. Historical maps (MustAssembleFleet false) keep their pre-placed fleets and fixed fleet types: the only
/// choice is which side you take. Open maps start with you, one opponent and the other slots closed; fleet types come from the
/// map's team list (locked) or are chosen by the player (maps without teams start with no fleet type).
fn new_slots(map: &Value) -> Vec<Slot> {
    let open = map["assemble"].as_bool().unwrap_or(false);
    let teams = map["teams"].as_array().cloned().unwrap_or_default();
    let mut slots: Vec<Slot> = map["players"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, p)| p["playable"].as_bool().unwrap_or(false))
        .map(|(i, p)| {
            let team = p["team"].as_i64().unwrap_or(-1) as i32;
            let race = if open {
                usize::try_from(team).ok().and_then(|t| teams.get(t)).map_or(4, |t| t["race"].as_u64().unwrap_or(4) as u8)
            } else {
                p["race"].as_u64().unwrap_or(4) as u8
            };
            let fleet = if open { vec![] } else { ships_of(map, i) };
            Slot { player: i, name: p["name"].as_str().unwrap_or("").trim().into(), kind: Kind::Computer, race, team, fleet, weapons: vec![] }
        })
        .collect();
    if let Some(h) = slots.first_mut() {
        h.kind = Kind::Human;
    }
    if open && !slots.is_empty() {
        let ht = slots[0].team;
        let foe = slots.iter().skip(1).position(|s| ht < 0 || s.team != ht).map(|i| i + 1);
        for (i, s) in slots.iter_mut().enumerate().skip(1) {
            if Some(i) != foe {
                s.kind = Kind::Closed;
            } else if s.race == 4 {
                s.race = 2;
            }
        }
    }
    slots
}

impl Skirmish {
    fn load_map(&mut self) {
        self.map_json = self
            .maps
            .get(self.selected)
            .and_then(|m| std::fs::read_to_string(assets_dir().join(format!("maps/{}/map.json", m.0))).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null);
        self.slots = new_slots(&self.map_json);
        self.error = None;
        (self.ship_pick, self.fleet_pick, self.scroll) = (0, 0, 0);
        self.arms_open = false;
    }
    pub fn open(&self) -> bool {
        self.map_json["assemble"].as_bool().unwrap_or(false)
    }
    pub fn human(&self) -> Option<&Slot> {
        self.slots.iter().find(|s| s.kind == Kind::Human)
    }
    /// Why the game cannot launch yet (a string id, or plain text), if so.
    /// Victory points the human player has left to spend on ships ('Current Victory Points').
    pub fn points_left(&self, lib: &Library) -> i64 {
        let spent: i64 = self.human().map_or(0, |h| h.fleet.iter().map(|n| lib.classes.iter().find(|c| &c.name == n).map_or(0, |c| i64::from(c.points))).sum());
        i64::from(self.victory_limit) - spent
    }
    pub fn launch_error(&self) -> Option<&'static str> {
        let h = self.human()?;
        if self.open() {
            if h.race == 4 {
                return Some("IDGS_TPFRONTENDTEXT01_PLEASE_SELECT_A_RACE");
            }
            if h.fleet.is_empty() {
                return Some("IDGS_TPFRONTENDTEXT_SCREENS_ERROR_FLEET");
            }
        }
        if !self.slots.iter().any(|s| s.kind != Kind::Closed && self.hostile(h, s)) {
            return Some("No opponent: open another slot on a different team.");
        }
        None
    }
    fn hostile(&self, a: &Slot, b: &Slot) -> bool {
        b.kind != Kind::Closed && !(a.team >= 0 && a.team == b.team) && a.player != b.player
    }
    /// The battle: one group per open slot. Allies (same team as you) fight on our side; everyone else is hostile.
    /// Historical maps use their pre-placed ships and positions; open maps use the assembled (or auto-built computer) fleets.
    pub fn plan(&self, lib: &Library) -> Vec<crate::Group> {
        let Some(h) = self.human() else { return vec![] };
        let open = self.open();
        self.slots
            .iter()
            .filter(|s| s.kind != Kind::Closed)
            .map(|s| {
                let ships = if !open {
                    self.map_json["ships"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|o| o["player"].as_u64() == Some(s.player as u64))
                        .filter_map(|o| {
                            Some((o["type"].as_str()?.to_string(), Some([0, 1, 2].map(|i| o["pos"][i].as_f64().unwrap_or(0.0) as f32)), o["name"].as_str().map(String::from)))
                        })
                        .filter(|(n, ..)| lib.classes.iter().any(|c| c.name == *n))
                        .collect()
                } else if s.fleet.is_empty() {
                    auto_fleet(lib, s.race, self.fleet_size, s.player).into_iter().map(|n| (n, None, None)).collect()
                } else {
                    s.fleet.iter().map(|n| (n.clone(), None, None)).collect()
                };
                let side = if s.kind == Kind::Human || !self.hostile(h, s) { crate::Side::Player } else { crate::Side::Enemy };
                crate::Group { player: s.player, side, human: s.kind == Kind::Human, ships, weapons: if open { s.weapons.clone() } else { vec![] } }
            })
            .collect()
    }
}

/// The skirmish list: every `maps/<id>/map.json` flagged `"skirmish": true` (campaign maps are not offered), sorted by id, with
/// whether it is open (fleets assembled) and its playable player count (the original's 'Players' column; ports and towns excluded).
pub fn skirmish_maps() -> Vec<(String, String, bool, usize)> {
    let mut maps: Vec<(String, String, bool, usize)> = std::fs::read_dir(assets_dir().join("maps"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let m: Value = serde_json::from_str(&std::fs::read_to_string(e.path().join("map.json")).ok()?).ok()?;
            let players = m["players"].as_array().map_or(0, |p| p.iter().filter(|p| p["playable"].as_bool().unwrap_or(false)).count());
            m["skirmish"].as_bool()?.then(|| (e.file_name().to_string_lossy().into(), m["name"].as_str().unwrap_or("").into(), m["assemble"].as_bool().unwrap_or(false), players))
        })
        .collect();
    maps.sort();
    maps
}

/// The fleet screen's ship information, as the original lists it.
#[derive(Debug, PartialEq)]
pub struct ShipStats {
    pub hit_points: u32,
    pub max_speed_kmh: u32,
    /// String id of the turning grade ('IDGS_TPFRONTENDTEXT_TURNINGRATE_*').
    pub turning: &'static str,
    pub crew: u32,
    pub heavy: usize,
    pub medium: usize,
    pub light: usize,
    pub points: u32,
}

/// Stats for one class: hit points = core plus every damage section, speed = WOT MaxSpeed (m/s) in km/h, crew = all crew posts,
/// guns counted by the mount type's size word (dual and non-selectable mounts included), point cost = 'Victory Point Cost'.
/// Matches the original's Navy Frigate panel (3150, 21 km/h, Average, 23, 6/6/9, 131). PROVISIONAL: the turning grade bands on
/// MaxAngularAcceleration are anchored on that one panel (frigate 0.609 = Average).
pub fn ship_stats(lib: &Library, ci: usize) -> ShipStats {
    let c = &lib.classes[ci];
    let size = |word: &str| c.mounts.iter().filter(|m| lib.guns.get(&m.gun).and_then(|g| g.mount_type.as_deref()).is_some_and(|t| t.contains(word))).count();
    let a = c.phys.max_ang_acc;
    let turning = match a {
        _ if a < 0.35 => "IDGS_TPFRONTENDTEXT_TURNINGRATE_VERYPOOR",
        _ if a < 0.55 => "IDGS_TPFRONTENDTEXT_TURNINGRATE_POOR",
        _ if a < 1.5 => "IDGS_TPFRONTENDTEXT_TURNINGRATE_AVERAGE",
        _ if a < 4.0 => "IDGS_TPFRONTENDTEXT_TURNINGRATE_GOOD",
        _ => "IDGS_TPFRONTENDTEXT_TURNINGRATE_VERYGOOD",
    };
    ShipStats {
        hit_points: (c.core_hp + c.sections.iter().map(|s| s.hp).sum::<f32>()).round() as u32,
        max_speed_kmh: (c.phys.max_speed * 3.6).round() as u32,
        turning,
        crew: c.crew_posts,
        heavy: size("Heavy"),
        medium: size("Medium"),
        light: size("Light"),
        points: c.points,
    }
}

/// A back/forward arrow button (views mark them; the `Action` alone does not say which way it points).
#[derive(Component)]
pub struct ArrowButton {
    pub forward: bool,
}

/// The retail front-end sound for pointing at (`select == false`) or pressing a widget, chosen by what it does; the names come
/// from the retail `Sound/GUI` files.
pub fn button_sound(action: &Action, arrow: Option<&ArrowButton>, select: bool) -> &'static str {
    let list = matches!(action, Action::PickMap(_) | Action::PickShip(_) | Action::PickFleet(_) | Action::PickSave(_) | Action::Rebind(_));
    if !select {
        return if list { "FE_List_Rollover" } else { "FE_Button_Rollover" };
    }
    match (action, arrow) {
        (_, Some(a)) => {
            if a.forward {
                "FE_Button_Forward_Select"
            } else {
                "FE_Button_Backward_Select"
            }
        }
        _ if list => "FE_List_Select",
        (Action::FleetSize | Action::Setting(_), _) => "SpinButton",
        (Action::Launch | Action::Campaign | Action::Tutorial(_), _) => "StartButton",
        (Action::AddShip, _) => "FE_Add",
        (Action::DropShip | Action::Remove, _) => "FE_Drop",
        _ => "FE_Button_Select",
    }
}

/// Rollover when the pointer first reaches a widget, the select sound when it is pressed (not again when the press is released).
fn button_sounds(
    q: Query<(Entity, &Interaction, &Action, Option<&ArrowButton>), Changed<Interaction>>,
    mut pointed: Local<std::collections::HashSet<Entity>>,
    mut sfx: MessageWriter<crate::audio::Sfx>,
) {
    for (e, i, action, arrow) in &q {
        if *i == Interaction::None {
            pointed.remove(&e);
            continue;
        }
        if pointed.insert(e) {
            sfx.write(crate::audio::Sfx::ui(button_sound(action, arrow, false)));
        }
        if *i == Interaction::Pressed {
            sfx.write(crate::audio::Sfx::ui(button_sound(action, arrow, true)));
        }
    }
}

pub fn plugin(app: &mut App) {
    let maps = skirmish_maps();
    // `--menu lobby --map <id>` opens the lobby for that map.
    let args: Vec<String> = std::env::args().collect();
    let asked = args.windows(2).find(|w| w[0] == "--map").and_then(|w| maps.iter().position(|m| m.0 == w[1]));
    // The original opens on the historical maps (the 'Historical Maps' box ticked).
    let selected = asked.or_else(|| maps.iter().position(|m| !m.2)).unwrap_or(0);
    let historical = !maps.get(selected).is_some_and(|m| m.2);
    let mut sk = Skirmish {
        map: None,
        slots: vec![],
        maps,
        selected,
        historical,
        fleet_size: 2,
        victory_limit: 300,
        map_json: Value::Null,
        error: None,
        ship_pick: 0,
        fleet_pick: 0,
        scroll: 0,
        arms_open: false,
        map_info: false,
    };
    sk.load_map();
    app.insert_resource(sk)
        .add_systems(OnEnter(Screen::Battle), (clear_menu, stop_music))
        .add_systems(OnEnter(Screen::Main), start_music)
        .insert_resource(SaveUi { list: crate::campaign::list_saves(), ..default() })
        .init_resource::<InGame>()
        .init_resource::<CtrlUi>()
        .insert_resource(Settings::load())
        .add_systems(Update, apply_settings)
        .configure_sets(Update, BuildMenu.after(capture_key).before(RunButtons).run_if(menu_active))
        .add_systems(
            Update,
            (capture_key, buttons.in_set(RunButtons).run_if(menu_active), button_sounds.before(RunButtons).run_if(menu_active), save_actions, save_text.run_if(menu_active)),
        )
        .add_systems(PostUpdate, esc_menu.run_if(in_state(Screen::Battle)))
        .add_systems(Update, autolaunch.run_if(in_state(Screen::Lobby)));
}

#[derive(Component)]
struct MenuMusic;

/// Retail front-end track (the exe loads "FrontEndMusic"); plays from the main menu until a battle starts.
fn start_music(mut commands: Commands, assets: Res<AssetServer>, playing: Query<(), With<MenuMusic>>) {
    if playing.is_empty() {
        let e = crate::audio::music(&mut commands, &assets, "FrontEndMusic");
        commands.entity(e).remove::<crate::audio::Music>().insert(MenuMusic);
    }
}

fn stop_music(mut commands: Commands, music: Query<Entity, With<MenuMusic>>) {
    for e in &music {
        commands.entity(e).despawn();
    }
}

// The menu builder does not run in battle, so it cannot remove the previous screen there.
fn clear_menu(mut commands: Commands, roots: Query<Entity, With<MenuRoot>>) {
    for e in &roots {
        commands.entity(e).despawn();
    }
}

pub fn text_of(strings: &Value, id: &str) -> String {
    strings[id].as_str().unwrap_or("").to_string()
}

/// The retail string table (`menu/strings.json`).
pub fn strings() -> Value {
    std::fs::read_to_string(assets_dir().join("menu/strings.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)
}

/// Everything a front-end screen is drawn from.
#[derive(SystemParam)]
pub struct MenuModel<'w> {
    pub sk: Res<'w, Skirmish>,
    pub ig: Res<'w, InGame>,
    pub saves: Res<'w, SaveUi>,
    pub binds: Res<'w, Bindings>,
    pub ctrl: Res<'w, CtrlUi>,
    pub lib: Res<'w, Library>,
    pub state: Res<'w, State<Screen>>,
    pub settings: Res<'w, Settings>,
}
/// Changes whenever the drawn screen would: views rebuild when it differs from the last one they drew.
pub type MenuKey = (Screen, usize, bool, usize, Vec<Slot>, Option<&'static str>, [usize; 3], u32, u32, String);
impl MenuModel<'_> {
    pub fn screen(&self) -> Screen {
        *self.state.get()
    }
    pub fn key(&self, window: &Window) -> MenuKey {
        let (sk, ui, cui) = (&self.sk, &self.saves, &self.ctrl);
        (
            self.screen(),
            sk.selected,
            sk.historical,
            sk.fleet_size,
            sk.slots.clone(),
            sk.error,
            [sk.ship_pick, sk.fleet_pick, sk.scroll],
            window.width() as u32,
            window.height() as u32,
            format!(
                "{:?}|{:?}|{}|{:?}|{}|{}|{}|{:?}|{:?}",
                self.ig.page,
                ui.selected,
                ui.name,
                ui.status,
                ui.scroll,
                ui.list.len(),
                cui.scroll,
                (cui.capture, &self.binds.rows),
                self.settings
            ),
        )
    }
}

/// Dev/smoke hook (`--menu lobby --autolaunch`): fills the human fleet like the fleet screen would and launches.
fn autolaunch(mut sk: ResMut<Skirmish>, lib: Res<Library>, mut next: ResMut<NextState<Screen>>) {
    // Screenshot hook (`--menu lobby --script map-info`): open the Map Description dialog.
    if !sk.map_info && std::env::args().collect::<Vec<_>>().windows(2).any(|w| w[0] == "--script" && w[1] == "map-info") {
        sk.map_info = true;
    }
    if !std::env::args().any(|a| a == "--autolaunch") {
        return;
    }
    if sk.open() {
        if sk.slots[0].race == 4 {
            apply(&mut sk, &lib, &Action::CycleRace(0));
        }
        for _ in 0..sk.fleet_size {
            apply(&mut sk, &lib, &Action::AddShip);
        }
    }
    if sk.launch_error().is_none() {
        sk.map = sk.maps.get(sk.selected).map(|m| m.0.clone());
        next.set(Screen::Battle);
    }
}

/// Esc: open the in-game menu (pausing the simulation), or step back / resume. Runs in PostUpdate so systems that used this Esc
/// (star map, right-click menu) can swallow it first. `--script ingame|ingame-load|ingame-save` opens a page at frame 60 for screenshots.
fn esc_menu(
    mission: Option<Res<crate::campaign::Mission>>,
    keys: Res<ButtonInput<KeyCode>>,
    cfg: Res<crate::Config>,
    mut ig: ResMut<InGame>,
    mut ui: ResMut<SaveUi>,
    mut vtime: ResMut<Time<Virtual>>,
    mut commands: Commands,
    roots: Query<Entity, With<MenuRoot>>,
) {
    if mission.as_ref().is_some_and(|m| m.script.crew_screen) {
        return;
    }
    let scripted = (cfg.frame == 60).then(|| match cfg.script.as_deref() {
        Some("ingame") => Some(Page::Menu),
        Some("ingame-load" | "ingame-do-load") => Some(Page::Load),
        Some("ingame-save") => Some(Page::Save),
        Some("ingame-options") => Some(Page::Options),
        Some("ingame-controls") => Some(Page::Controls),
        _ => None,
    });
    if let Some(Some(page)) = scripted {
        ui.open();
        (ig.paused_by_us, ig.page) = (!vtime.is_paused(), Some(page));
        vtime.pause();
    } else if keys.just_pressed(KeyCode::Escape) {
        match ig.page {
            None => (ig.paused_by_us, ig.page) = (!vtime.is_paused(), Some(Page::Menu)),
            Some(Page::Menu) => close_overlay(&mut commands, &roots, &mut ig, &mut vtime),
            Some(Page::Controls | Page::Audio | Page::Display) => ig.page = Some(Page::Options),
            Some(_) => ig.page = Some(Page::Menu),
        }
        if ig.page.is_some() {
            vtime.pause();
        }
    }
}

fn close_overlay(commands: &mut Commands, roots: &Query<Entity, With<MenuRoot>>, ig: &mut InGame, vtime: &mut Time<Virtual>) {
    for e in roots {
        commands.entity(e).despawn();
    }
    if ig.paused_by_us {
        vtime.unpause();
    }
    *ig = InGame::default();
}

const QUICK: &str = "QuickSave";

/// Load / Save / Remove / Quit buttons of the Load Campaign screen and the in-game menu.
/// PROVISIONAL: a save is a mission-start checkpoint plus the campaign counters; loading restarts that mission. No delete or overwrite
/// confirmation dialogs (the retail screens have them).
#[allow(clippy::too_many_arguments)]
fn save_actions(
    q: Query<(&Interaction, &Action), Changed<Interaction>>,
    mut commands: Commands,
    (mut ui, mut ig, mut points, mut sk): (ResMut<SaveUi>, ResMut<InGame>, ResMut<CampaignPoints>, ResMut<Skirmish>),
    mut cfg: ResMut<crate::Config>,
    mission: Option<Res<crate::campaign::Mission>>,
    old: Query<Entity, Or<(With<crate::Scenario>, With<crate::Transient>)>>,
    roots: Query<Entity, With<MenuRoot>>,
    mut vtime: ResMut<Time<Virtual>>,
    mut next: ResMut<NextState<Screen>>,
    (keys, state): (Res<ButtonInput<KeyCode>>, Res<State<Screen>>),
) {
    let mut acts: Vec<Action> = q.iter().filter(|(i, _)| **i == Interaction::Pressed).map(|(_, a)| a.clone()).collect();
    // Dev hook: `--script ingame-do-load` loads the mission 5 start save from the Load page at frame 120.
    if cfg.script.as_deref() == Some("ingame-do-load") && cfg.frame == 120 {
        ui.selected = ui.list.iter().position(|s| s.0.mission == "mission05");
        acts.push(Action::DoLoad);
    }
    // Dev hook: `--script quick` quick-saves at frame 60 and quick-loads at frame 120.
    acts.extend(match (cfg.script.as_deref(), cfg.frame) {
        (Some("quick"), 60) => Some(Action::QuickSave),
        (Some("quick"), 120) => Some(Action::QuickLoad),
        _ => None,
    });
    // Retail default bindings (UserProfiles/default.pft, DirectInput scancodes 0x3d / 0x3e): F3 Quick Save, F4 Quick Load.
    if *state.get() == Screen::Battle && ig.page.is_none() {
        acts.extend([(KeyCode::F3, Action::QuickSave), (KeyCode::F4, Action::QuickLoad)].into_iter().filter(|(k, _)| keys.just_pressed(*k)).map(|k| k.1));
    }
    acts.reverse();
    while let Some(a) = acts.pop() {
        match a {
            Action::QuickSave => {
                if let Some(m) = &mission {
                    match crate::campaign::write_save(&points.snapshot(QUICK, &m.id)) {
                        Ok(()) => info!("quick saved {}", m.id),
                        Err(e) => warn!("quick save failed: {e}"),
                    }
                }
            }
            Action::QuickLoad => {
                ui.open();
                if let Some(n) = ui.list.iter().position(|s| !s.1 && s.0.name == QUICK) {
                    info!("quick loading {}", ui.list[n].0.mission);
                    ui.selected = Some(n);
                    acts.push(Action::DoLoad);
                }
            }
            Action::OpenLoad => {
                ui.open();
                next.set(Screen::Load);
            }
            Action::Ingame(None) => close_overlay(&mut commands, &roots, &mut ig, &mut vtime),
            Action::Ingame(Some(p)) => {
                if p != Page::Menu {
                    ui.open();
                }
                ig.page = Some(p);
            }
            Action::PickSave(n) => {
                ui.selected = Some(n);
                ui.status = None;
                if let (Some(Page::Save), Some((s, false))) = (ig.page, ui.list.get(n)) {
                    ui.name = s.name.clone();
                }
            }
            Action::DoLoad => {
                let Some(save) = ui.selected.and_then(|n| ui.list.get(n)).map(|s| s.0.clone()) else { continue };
                points.restore(&save);
                (sk.map, cfg.map, cfg.ended) = (None, Some(save.mission), None);
                if *state.get() == Screen::Battle {
                    for e in &old {
                        commands.entity(e).despawn();
                    }
                    commands.remove_resource::<crate::campaign::Mission>();
                    close_overlay(&mut commands, &roots, &mut ig, &mut vtime);
                }
                next.set(Screen::Battle); // from the battle too: re-entering the state spawns the mission again
            }
            Action::DoSave => {
                let Some(m) = &mission else {
                    ui.status = Some("Only campaign missions can be saved.");
                    continue;
                };
                let name = if ui.name.trim().is_empty() { format!("Mission {}", m.id.trim_start_matches("mission")) } else { ui.name.trim().to_string() };
                match crate::campaign::write_save(&points.snapshot(&name, &m.id)) {
                    Ok(()) => {
                        ui.list = crate::campaign::list_saves();
                        ui.selected = ui.list.iter().position(|s| !s.1 && s.0.name == name);
                        ui.status = Some("IDGS_TPFRONTENDTEXT02_FILECHANGE_SAVED");
                    }
                    Err(e) => {
                        warn!("could not save: {e}");
                        ui.status = Some("Could not write the save file.");
                    }
                }
            }
            Action::Remove => {
                if let Some((s, false)) = ui.selected.and_then(|n| ui.list.get(n)) {
                    crate::campaign::delete_save(&s.name);
                    let status = ui.status;
                    ui.open();
                    ui.status = status;
                }
            }
            Action::QuitToMenu => {
                for e in &old {
                    commands.entity(e).despawn();
                }
                commands.remove_resource::<crate::campaign::Mission>();
                close_overlay(&mut commands, &roots, &mut ig, &mut vtime);
                next.set(Screen::Main);
            }
            _ => {}
        }
    }
}

/// While a Control Options row waits for a key: the next bindable key press rebinds it (Esc cancels, and is not seen by the in-game menu).
pub fn capture_key(mut keys: ResMut<ButtonInput<KeyCode>>, mut cui: ResMut<CtrlUi>, mut binds: ResMut<Bindings>) {
    let Some(row) = cui.capture else { return };
    let pressed = keys.get_just_pressed().copied().find(|k| crate::bindings::bindable(*k));
    if keys.just_pressed(KeyCode::Escape) {
        keys.clear_just_pressed(KeyCode::Escape);
        cui.capture = None;
    } else if let Some(k) = pressed {
        keys.clear_just_pressed(k);
        binds.rebind(row, k);
        binds.save();
        cui.capture = None;
    }
}

/// Typing into the Save page's file name entry.
fn save_text(mut keys: MessageReader<bevy::input::keyboard::KeyboardInput>, ig: Res<InGame>, mut ui: ResMut<SaveUi>) {
    use bevy::input::keyboard::Key;
    for k in keys.read().filter(|k| k.state.is_pressed() && ig.page == Some(Page::Save)) {
        match &k.logical_key {
            Key::Character(c) if ui.name.chars().count() < 32 => ui.name.extend(c.chars().filter(|c| !c.is_control())),
            Key::Space if ui.name.chars().count() < 32 => ui.name.push(' '),
            Key::Backspace => {
                ui.name.pop();
            }
            _ => {}
        }
    }
}

/// The lobby / fleet-screen choices (everything that only edits the `Skirmish`).
fn apply(sk: &mut Skirmish, lib: &Library, action: &Action) {
    match action {
        Action::PickMap(m) => {
            sk.selected = *m;
            sk.load_map();
        }
        Action::Historical(h) => {
            sk.historical = *h;
            if let Some(p) = sk.maps.iter().position(|m| m.2 != *h) {
                sk.selected = p;
                sk.load_map();
            }
        }
        // PROVISIONAL steps: the original's scrollbar values are not recovered; 300 is the default seen on screenshots.
        Action::VictoryLimit => {
            const STEPS: [u32; 7] = [200, 300, 400, 500, 750, 1000, 1500];
            let next = STEPS.iter().position(|&v| v == sk.victory_limit).map_or(1, |i| (i + 1) % STEPS.len());
            sk.victory_limit = STEPS[next];
        }
        Action::FleetSize => {
            sk.fleet_size = sk.fleet_size % 8 + 1;
            let n = sk.fleet_size;
            for s in &mut sk.slots {
                s.fleet.truncate(n);
                s.weapons.truncate(n);
            }
        }
        Action::Claim(i) => {
            if sk.slots[*i].kind != Kind::Human {
                for s in &mut sk.slots {
                    if s.kind == Kind::Human {
                        s.kind = Kind::Computer;
                    }
                }
                sk.slots[*i].kind = Kind::Human;
                sk.error = None;
                (sk.ship_pick, sk.fleet_pick, sk.scroll) = (0, 0, 0);
            }
        }
        Action::ToggleClosed(i) => {
            let s = &mut sk.slots[*i];
            s.kind = if s.kind == Kind::Closed { Kind::Computer } else { Kind::Closed };
            sk.error = None;
        }
        Action::CycleRace(i) => {
            let s = &mut sk.slots[*i];
            s.race = next_race(s.race);
            s.fleet.clear(); // a fleet only holds hulls of its fleet type
            s.weapons.clear();
            sk.error = None;
        }
        Action::CycleTeam(i) => {
            // Team maps: step through the map's teams (the fleet type follows, as the lobby locks it); maps without teams: on its own, then team 1..4.
            let teams = sk.map_json["teams"].as_array().cloned().unwrap_or_default();
            let s = &mut sk.slots[*i];
            if teams.is_empty() {
                s.team = if s.team >= 3 { -1 } else { s.team + 1 };
            } else {
                s.team = (s.team + 1).rem_euclid(teams.len() as i32);
                let race = teams[s.team as usize]["race"].as_u64().unwrap_or(4) as u8;
                if s.race != race {
                    s.race = race;
                    s.fleet.clear();
                    s.weapons.clear();
                }
            }
            sk.error = None;
        }
        Action::OpenArms => sk.arms_open = sk.open() && sk.human().is_some_and(|h| !h.fleet.is_empty()),
        Action::PickShip(row) => sk.ship_pick = *row,
        Action::MapInfo(open) => sk.map_info = *open,
        Action::PickFleet(row) => sk.fleet_pick = *row,
        Action::AddShip => {
            let (limit, pick, left) = (sk.fleet_size, sk.ship_pick, sk.points_left(lib));
            if let Some(h) = sk.slots.iter_mut().find(|s| s.kind == Kind::Human).filter(|h| h.fleet.len() < limit)
                && let Some(&ci) = offered(lib, h.race).get(pick)
                && i64::from(lib.classes[ci].points) <= left
            {
                h.weapons.resize_with(h.fleet.len(), std::collections::BTreeMap::new);
                h.fleet.push(lib.classes[ci].name.clone());
                h.weapons.push(default());
                sk.fleet_pick = h.fleet.len() - 1;
            }
            sk.error = None;
        }
        // PROVISIONAL: the original's auto-assign rule is not recovered; this adds the most expensive ship that still fits until the
        // fleet is full or nothing fits.
        Action::AutoAssign => {
            while let Some(h) = sk.human().filter(|h| h.fleet.len() < sk.fleet_size) {
                let left = sk.points_left(lib);
                let Some(pick) = offered(lib, h.race).iter().position(|&c| i64::from(lib.classes[c].points) <= left) else { break };
                sk.ship_pick = pick;
                apply(sk, lib, &Action::AddShip);
            }
        }
        Action::DropShip => {
            let pick = sk.fleet_pick;
            if let Some(h) = sk.slots.iter_mut().find(|s| s.kind == Kind::Human).filter(|h| pick < h.fleet.len()) {
                h.fleet.remove(pick);
                if pick < h.weapons.len() {
                    h.weapons.remove(pick);
                }
                sk.fleet_pick = pick.saturating_sub(1);
            }
        }
        _ => {}
    }
}

/// Runs the `Action` of a pressed button (whichever view drew it).
fn buttons(
    q: Query<(&Interaction, &Action), Changed<Interaction>>,
    mut next: ResMut<NextState<Screen>>,
    mut sk: ResMut<Skirmish>,
    lib: Res<Library>,
    mut exit: MessageWriter<AppExit>,
    mut cfg: ResMut<crate::Config>,
    (mut binds, mut cui, mut settings): (ResMut<Bindings>, ResMut<CtrlUi>, ResMut<Settings>),
) {
    for (i, action) in &q {
        if *i != Interaction::Pressed {
            continue;
        }
        if sk.arms_open {
            continue;
        }
        apply(&mut sk, &lib, action);
        match action {
            Action::Goto(s) => next.set(*s),
            Action::Rebind(n) => cui.capture = Some(*n),
            Action::Setting(s) => {
                settings.step(*s);
                settings.save();
            }
            Action::RestoreKeys => {
                *binds = Bindings::default();
                binds.save();
                cui.capture = None;
            }
            Action::Quit => {
                exit.write(AppExit::Success);
            }
            Action::Campaign => {
                sk.map = None;
                cfg.map = Some("mission01".into());
                cfg.ended = None;
                next.set(Screen::Battle);
            }
            Action::Tutorial(id) => {
                sk.map = None;
                cfg.map = Some((*id).into());
                cfg.ended = None;
                next.set(Screen::Battle);
            }
            Action::Launch => {
                sk.error = sk.launch_error();
                if sk.error.is_none() {
                    sk.map = sk.maps.get(sk.selected).map(|m| m.0.clone());
                    next.set(Screen::Battle);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_assign_fills_the_fleet_within_the_points() {
        let lib = Library::load(&assets_dir());
        let mut sk = skirmish("red_rover");
        sk.slots[0].race = 0;
        sk.fleet_size = 5;
        apply(&mut sk, &lib, &Action::AutoAssign);
        let fleet = &sk.human().unwrap().fleet;
        assert!(!fleet.is_empty() && fleet.len() <= 5);
        assert!(sk.points_left(&lib) >= 0, "never overspends");
        let navy = offered(&lib, 0);
        assert_eq!(lib.classes[navy[0]].name, "Ship_Navy_StarHammer", "the original lists the StarHammer first");
    }

    #[test]
    fn frigate_stats_match_the_original_fleet_screen() {
        let lib = Library::load(&assets_dir());
        let st = ship_stats(&lib, lib.index("Ship_Navy_Frigate"));
        assert_eq!(st, ShipStats { hit_points: 3150, max_speed_kmh: 21, turning: "IDGS_TPFRONTENDTEXT_TURNINGRATE_AVERAGE", crew: 23, heavy: 6, medium: 6, light: 9, points: 131 });
    }

    #[test]
    fn fleet_spends_victory_points() {
        let lib = Library::load(&assets_dir());
        let mut sk = skirmish("red_rover");
        sk.slots[0].race = 0;
        sk.ship_pick = offered(&lib, 0).iter().position(|&c| lib.classes[c].name == "Ship_Navy_Frigate").unwrap();
        sk.fleet_size = 5;
        assert_eq!(sk.points_left(&lib), 300);
        apply(&mut sk, &lib, &Action::AddShip);
        assert_eq!(sk.points_left(&lib), 169, "the original shows 169 after adding a frigate");
        apply(&mut sk, &lib, &Action::AddShip);
        apply(&mut sk, &lib, &Action::AddShip);
        assert_eq!(sk.human().unwrap().fleet.len(), 2, "a third frigate would overspend 300 points");
    }

    #[test]
    fn map_info_opens_and_closes_the_lobby_dialog() {
        let lib = Library::load(&assets_dir());
        let mut sk = skirmish("ambush");
        apply(&mut sk, &lib, &Action::MapInfo(true));
        assert!(sk.map_info);
        apply(&mut sk, &lib, &Action::MapInfo(false));
        assert!(!sk.map_info);
    }

    #[test]
    fn skirmish_list_counts_playable_players_like_the_original() {
        let maps = skirmish_maps();
        let get = |id: &str| maps.iter().find(|m| m.0 == id).unwrap_or_else(|| panic!("{id}"));
        // Original 'Historical Maps' list: The Ambush 4, Bayle's Stand 2, Iron War 3, Locusts 4, Storm Front 2, The Maw 3.
        for (id, n) in [("ambush", 4), ("bayles_stand", 2), ("iron_war", 3), ("locusts", 4), ("storm_front", 2), ("the_maw", 3)] {
            assert_eq!((get(id).2, get(id).3), (false, n), "{id}");
        }
    }

    #[test]
    fn menu_widgets_play_rollover_once_and_their_own_select_sound() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_message::<crate::audio::Sfx>().add_systems(Update, button_sounds);
        let row = app.world_mut().spawn((Action::PickMap(0), Interaction::None)).id();
        let next = app.world_mut().spawn((Action::Goto(Screen::Lobby), ArrowButton { forward: true }, Interaction::None)).id();
        let heard = |app: &mut App, e: Entity, i: Interaction| -> Vec<String> {
            *app.world_mut().get_mut::<Interaction>(e).unwrap() = i;
            app.update();
            app.world_mut().resource_mut::<Messages<crate::audio::Sfx>>().drain().map(|s| s.name).collect()
        };
        assert_eq!(heard(&mut app, row, Interaction::Hovered), ["FE_List_Rollover"]);
        assert_eq!(heard(&mut app, row, Interaction::Pressed), ["FE_List_Select"]);
        assert!(heard(&mut app, row, Interaction::Hovered).is_empty(), "releasing the press is silent");
        assert_eq!(heard(&mut app, next, Interaction::Pressed), ["FE_Button_Rollover", "FE_Button_Forward_Select"]);
    }

    #[test]
    fn every_menu_sound_is_a_retail_file() {
        let back = ArrowButton { forward: false };
        let actions = [Action::Quit, Action::PickMap(0), Action::FleetSize, Action::Launch, Action::AddShip, Action::DropShip];
        for a in &actions {
            for (arrow, select) in [(None, false), (None, true), (Some(&back), true)] {
                let name = button_sound(a, arrow, select);
                assert!(crate::assets_dir().join(crate::audio::sound(name)).is_file(), "{name}");
            }
        }
    }

    fn map(id: &str) -> Value {
        serde_json::from_str(&std::fs::read_to_string(assets_dir().join(format!("maps/{id}/map.json"))).unwrap()).unwrap()
    }
    fn skirmish(id: &str) -> Skirmish {
        let mut sk = Skirmish {
            map: None,
            slots: vec![],
            maps: vec![(id.into(), id.into(), false, 0)],
            selected: 0,
            historical: false,
            fleet_size: 2,
            victory_limit: 300,
            map_json: Value::Null,
            error: None,
            ship_pick: 0,
            fleet_pick: 0,
            scroll: 0,
            arms_open: false,
            map_info: false,
        };
        sk.load_map();
        sk
    }
    #[test]
    fn skirmish_weapon_choices_follow_fleet_entries_and_survive_launch_planning() {
        let lib = Library::load(&assets_dir());
        let mut sk = skirmish("red_rover");
        sk.fleet_size = 3;
        let human = sk.slots.iter().position(|s| s.kind == Kind::Human).unwrap();
        sk.slots[human].race = 0;
        sk.ship_pick = offered(&lib, 0).iter().position(|&i| lib.classes[i].name == "Ship_Navy_Cutter").unwrap();
        apply(&mut sk, &lib, &Action::AddShip);
        apply(&mut sk, &lib, &Action::AddShip);
        let class = &lib.classes[lib.index("Ship_Navy_Cutter")];
        let bank = class.mounts.iter().find(|m| m.gun == "Gun_CannonBall_Med").unwrap().bank;
        sk.slots[human].weapons[0].insert(bank, "Gun_Carronade_Med".into());
        let plan = sk.plan(&lib);
        let group = plan.iter().find(|g| g.human).unwrap();
        assert_eq!(group.ships[0].0, group.ships[1].0);
        assert_eq!(group.weapons[0][&bank], "Gun_Carronade_Med");
        assert!(group.weapons[1].is_empty(), "duplicate hulls must not share fittings");
        sk.fleet_pick = 0;
        apply(&mut sk, &lib, &Action::OpenArms);
        assert!(sk.arms_open);
        apply(&mut sk, &lib, &Action::DropShip);
        assert_eq!(sk.slots[human].weapons.len(), 1);
        assert!(sk.slots[human].weapons[0].is_empty(), "dropping a ship also drops its fitting");
        sk.slots[human].weapons[0].insert(bank, "Gun_GravCharge_Med".into());
        apply(&mut sk, &lib, &Action::CycleRace(human));
        assert!(sk.slots[human].fleet.is_empty());
        assert!(sk.slots[human].weapons.is_empty());
    }

    #[test]
    fn historical_map_keeps_preplaced_fleets_and_fixed_fleet_types() {
        let slots = new_slots(&map("ambush"));
        assert_eq!(slots.iter().map(|s| (s.race, s.team)).collect::<Vec<_>>(), [(0, 0), (2, 1), (2, 1), (2, 1)]);
        assert!(slots.iter().all(|s| !s.fleet.is_empty() && s.kind != Kind::Closed));
        assert_eq!(slots[0].kind, Kind::Human);
        // Taking a pirate slot makes the other pirates allies and the Navy hostile.
        let lib = Library::load(&assets_dir());
        let mut sk = skirmish("ambush");
        let navy = &sk.plan(&lib)[0].ships;
        assert_eq!((navy[0].0.as_str(), navy[0].2.as_deref()), ("Ship_Navy_ManOWar", Some("RLS Indefatigable")), "historical ships keep their set names");
        sk.slots[0].kind = Kind::Computer;
        sk.slots[2].kind = Kind::Human;
        let sides: Vec<_> = sk.plan(&lib).iter().map(|g| (g.player, g.side, g.human, !g.ships.is_empty() && g.ships.iter().all(|s| s.1.is_some()))).collect();
        assert_eq!(
            sides,
            [(0, crate::Side::Enemy, false, true), (1, crate::Side::Player, false, true), (2, crate::Side::Player, true, true), (3, crate::Side::Player, false, true)]
        );
    }
    #[test]
    fn open_map_locks_fleet_type_to_team_and_needs_a_fleet() {
        let mut sk = skirmish("red_rover");
        assert_eq!((sk.slots[0].race, sk.slots[1].race), (0, 2));
        assert!(sk.slots.iter().skip(2).all(|s| s.kind == Kind::Closed));
        assert_eq!(sk.launch_error(), Some("IDGS_TPFRONTENDTEXT_SCREENS_ERROR_FLEET"));
        sk.slots[0].fleet.push("Ship_Navy_Cutter".into());
        assert_eq!(sk.launch_error(), None);
        let lib = Library::load(&assets_dir());
        let plan = sk.plan(&lib);
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[1].side, crate::Side::Enemy);
        assert!(plan[1].ships.len() == sk.fleet_size && plan[1].ships.iter().all(|s| s.0.contains("Pirate")), "computer fleet is built from its fleet type");
        // Free-for-all map: you choose the fleet type yourself.
        let sk = skirmish("diablo_straight");
        assert_eq!((sk.slots[0].race, sk.slots[1].race), (4, 2));
        assert_eq!(sk.launch_error(), Some("IDGS_TPFRONTENDTEXT01_PLEASE_SELECT_A_RACE"));
    }
    #[test]
    fn lobby_actions_edit_slots_and_fleet() {
        let lib = Library::load(&assets_dir());
        let mut sk = skirmish("diablo_straight"); // free-for-all, fleet size 2
        apply(&mut sk, &lib, &Action::CycleRace(0)); // none -> Navy
        assert_eq!(sk.slots[0].race, 0);
        let cheapest = offered(&lib, 0).len() - 1; // the list is most expensive first; points never limit cheap ships here
        apply(&mut sk, &lib, &Action::PickShip(cheapest));
        for _ in 0..3 {
            apply(&mut sk, &lib, &Action::AddShip);
        }
        assert_eq!(sk.slots[0].fleet.len(), 2, "fleet size limit");
        apply(&mut sk, &lib, &Action::DropShip);
        assert_eq!(sk.slots[0].fleet.len(), 1);
        apply(&mut sk, &lib, &Action::CycleRace(0));
        assert!(sk.slots[0].fleet.is_empty() && sk.slots[0].race == 2, "a new fleet type clears the fleet");
        apply(&mut sk, &lib, &Action::Claim(1));
        assert_eq!((sk.slots[0].kind, sk.slots[1].kind), (Kind::Computer, Kind::Human));
        apply(&mut sk, &lib, &Action::ToggleClosed(2));
        assert_eq!(sk.slots[2].kind, Kind::Computer);
        // Team maps: changing team moves the locked fleet type with it.
        let mut sk = skirmish("red_rover");
        apply(&mut sk, &lib, &Action::PickShip(0));
        apply(&mut sk, &lib, &Action::AddShip);
        apply(&mut sk, &lib, &Action::CycleTeam(0));
        assert_eq!((sk.slots[0].team, sk.slots[0].race, sk.slots[0].fleet.len()), (1, 2, 0));
    }
    #[test]
    fn fleet_screen_offers_every_fleet_type_without_story_hulls() {
        let lib = Library::load(&assets_dir());
        for (race, name) in [(0, "Navy"), (2, "Pirate"), (3, "Procyon")] {
            let list = offered(&lib, race);
            assert!(list.len() >= 5, "{name}: {}", list.len());
            assert!(list.iter().all(|&i| lib.classes[i].race == name && !NOT_OFFERED.iter().any(|n| lib.classes[i].name.contains(n))));
        }
        assert!(offered(&lib, 4).is_empty());
    }
    #[test]
    fn battle_entry_removes_menu_and_descendants_only() {
        for source in [Screen::SinglePlayer, Screen::Lobby] {
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin)).init_state::<Screen>().add_systems(OnEnter(Screen::Battle), clear_menu);
            // This test exercises state transitions/cleanup without renderer-dependent menu building.
            app.update();
            app.world_mut().resource_mut::<NextState<Screen>>().set(source);
            app.update();
            let root = app.world_mut().spawn(MenuRoot).id();
            let child = app.world_mut().spawn(ChildOf(root)).id();
            let grandchild = app.world_mut().spawn(ChildOf(child)).id();
            let battle_ui = app.world_mut().spawn_empty().id();
            app.world_mut().resource_mut::<NextState<Screen>>().set(Screen::Battle);
            app.update();
            assert_eq!(*app.world().resource::<State<Screen>>().get(), Screen::Battle);
            for e in [root, child, grandchild] {
                assert!(app.world().get_entity(e).is_err());
            }
            assert!(app.world().get_entity(battle_ui).is_ok());
            // Returning to the same screen must not leave a new menu behind on relaunch.
            app.world_mut().resource_mut::<NextState<Screen>>().set(source);
            app.update();
            let root = app.world_mut().spawn(MenuRoot).id();
            app.world_mut().resource_mut::<NextState<Screen>>().set(Screen::Battle);
            app.update();
            assert!(app.world().get_entity(root).is_err());
        }
    }
}
