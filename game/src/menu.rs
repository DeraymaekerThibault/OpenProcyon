//! Front-end model shared by both presentations: Main Menu -> Single Player -> Create Skirmish Game -> Skirmish Game Lobby -> battle.
//! Owns the screen state, the lobby/fleet choices (`Skirmish`), saves, the in-game Esc menu and the `Action`s buttons carry.
//! Views (`classic/menu.rs` retail screens) draw the current screen in `BuildMenu` from `MenuModel`
//! and tag their buttons with an `Action`; `buttons` / `save_actions` here run them. Maps: assets/maps/<id> (tools/export_maps.py).

#![allow(deprecated)] // shortcut: bevy 0.20 Interaction (deprecated for picking Hovered/Pressed); port when it is removed
mod saves;
mod settings;
mod skirmish;
pub use saves::*;
pub use settings::*;
pub use skirmish::*;

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
    /// Step the fleet size limit by this many ships (open maps; the slider's arrows).
    FleetSize(i32),
    /// Step the victory point limit by this many 10-point steps (open maps; the slider's arrows).
    VictoryLimit(i32),
    Claim(usize),
    /// Open or close a lobby pull-down (`None` closes).
    Dropdown(Option<Cell>),
    /// Open or close the fleet screen's ship-name pull-down.
    NameList(bool),
    /// Give the shown fleet ship the name `.0` and close the pull-down.
    PickName(String),
    /// Open or close the lobby's flag picker.
    FlagPicker(bool),
    /// Fly the human slot's flag number `n` (0-9) and close the picker.
    PickFlag(u8),
    /// Scroll the fleet slot bar.
    SlotScroll(i32),
    /// The fleet screen's Crew button (opens the crew tab).
    OpenCrew,
    /// Pick an entry of the open pull-down.
    Choose(Cell, Choice),
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
    /// Game Summary (`Multiplayer Stats Screen.tst`): opened by the in-game menu's Summary button and when a skirmish is decided.
    Summary,
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

/// Control Options screen: first visible row and the row waiting for a key press.
#[derive(Resource, Default)]
pub struct CtrlUi {
    /// First row of the classic fixed-height list.
    pub scroll: usize,
    pub capture: Option<usize>,
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
        (Action::FleetSize(_) | Action::VictoryLimit(_) | Action::Setting(_), _) => "SpinButton",
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
        fleet_size: FLEET_SIZE.default as usize,
        victory_limit: VICTORY_LIMIT.default,
        map_json: Value::Null,
        error: None,
        dropdown: None,
        name_open: false,
        flag_picker: false,
        ship_pick: 0,
        ship_chosen: false,
        view_slot: false,
        fleet_pick: 0,
        slot_scroll: 0,
        map_scroll: 0,
        scroll: 0,
        arms_open: false,
        arms_tab: false,
        map_info: false,
    };
    sk.load_map();
    app.insert_resource(sk)
        .add_systems(OnEnter(Screen::Battle), (clear_menu, stop_music))
        .add_systems(OnEnter(Screen::Main), start_music)
        .insert_resource(SaveUi { list: crate::campaign::list_saves(), ..default() })
        .init_resource::<InGame>()
        .init_resource::<ScrollTo>()
        .init_resource::<CtrlUi>()
        .insert_resource(Settings::load())
        .add_systems(Update, apply_settings)
        .configure_sets(Update, BuildMenu.after(capture_key).before(RunButtons).run_if(menu_active))
        .add_systems(
            Update,
            (
                capture_key,
                slider_drag.run_if(in_state(Screen::CreateSkirmish)),
                scroll_drag,
                dropdown_release.run_if(in_state(Screen::Lobby)),
                name_release.run_if(in_state(Screen::Fleet)),
                buttons.in_set(RunButtons).run_if(menu_active),
                button_sounds.before(RunButtons).run_if(menu_active),
                save_actions,
                save_text.run_if(menu_active),
            ),
        )
        .add_systems(PostUpdate, (summary_on_end, esc_menu).chain().run_if(in_state(Screen::Battle)))
        .add_systems(Update, autolaunch.run_if(in_state(Screen::Lobby)))
        .add_systems(Update, fleet_hook.run_if(in_state(Screen::Fleet)));
}

#[derive(Component)]
struct MenuMusic;

/// Retail front-end track (the original loads "FrontEndMusic"); plays from the main menu until a battle starts.
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
    crate::data::try_json(&assets_dir().join("menu/strings.json")).unwrap_or(Value::Null)
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
    pub stats: Res<'w, crate::stats::GameStats>,
    pub outcome: Res<'w, crate::scenario::Outcome>,
    pub end_message: Res<'w, crate::scenario::EndMessage>,
}
/// Retail pull-downs stay open while the button is held: press the cell, drag to an entry, release to pick it. A release over the
/// first entry (the cell itself, i.e. a plain click) keeps the list open for a second click.
fn dropdown_release(mouse: Res<ButtonInput<MouseButton>>, entries: Query<(&Action, &Interaction)>, mut sk: ResMut<Skirmish>) {
    let Some(cell) = sk.dropdown else { return };
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    let first = sk.choices(cell).first().copied();
    let picked = entries.iter().find_map(|(a, i)| match a {
        Action::Choose(c, choice) if *c == cell && *i != Interaction::None && Some(*choice) != first => Some(*choice),
        _ => None,
    });
    if let Some(choice) = picked {
        sk.choose(cell, choice);
    }
}

/// The ship-name pull-down opens on a press and picks on the release over an entry (retail holds it open only while the button is down).
fn name_release(mouse: Res<ButtonInput<MouseButton>>, entries: Query<(&Action, &Interaction)>, mut sk: ResMut<Skirmish>) {
    if !sk.name_open || !mouse.just_released(MouseButton::Left) {
        return;
    }
    let picked = entries.iter().find_map(|(a, i)| match a {
        Action::PickName(n) if *i != Interaction::None => Some(n.clone()),
        _ => None,
    });
    if let Some(n) = picked {
        sk.pick_name(&n);
    }
}

/// Which list a draggable scrollbar scrolls (retail scrollbars all work the same: press the track or thumb and drag).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScrollKind {
    Maps,
    Ships,
    /// The crew / weapon list of the Crew and Arms screens (owned by `crew_arms`, which reads `ScrollTo`).
    Crew,
}
/// A scrollbar's track (views attach it, with a `RelativeCursorPosition`): the list's row counts and the thumb's share of the track.
#[derive(Component, Clone, Copy, Debug)]
pub struct ScrollTrack {
    pub kind: ScrollKind,
    pub total: usize,
    pub visible: usize,
    pub thumb: f32,
}
/// The first row a dragged scrollbar asks for; the owner of a list not held by `Skirmish` takes it.
#[derive(Resource, Default)]
pub struct ScrollTo(pub Option<(ScrollKind, usize)>);

/// Dragging any scrollbar. The menu is rebuilt after every change, so the drag is remembered here rather than in the node.
fn scroll_drag(
    tracks: Query<(&ScrollTrack, &Interaction, &bevy::ui::RelativeCursorPosition)>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut sk: ResMut<Skirmish>,
    mut to: ResMut<ScrollTo>,
    mut dragging: Local<Option<ScrollKind>>,
) {
    if !mouse.pressed(MouseButton::Left) {
        *dragging = None;
        return;
    }
    if dragging.is_none() {
        *dragging = tracks.iter().find(|(_, i, _)| **i == Interaction::Pressed).map(|(t, ..)| t.kind);
    }
    let Some(kind) = *dragging else { return };
    let Some((t, y)) = tracks.iter().find(|(t, ..)| t.kind == kind).and_then(|(t, _, rel)| rel.normalized.map(|p| (*t, p.y + 0.5))) else { return };
    // The thumb's centre follows the cursor along the room its length leaves.
    let pos = ((y - t.thumb / 2.0) / (1.0 - t.thumb).max(0.01)).clamp(0.0, 1.0);
    let row = (pos * t.total.saturating_sub(t.visible) as f32).round() as usize;
    match kind {
        ScrollKind::Maps => sk.map_scroll = row,
        ScrollKind::Ships => sk.scroll = row,
        ScrollKind::Crew => to.0 = Some((kind, row)),
    }
}

/// A Create Skirmish slider's track (views attach it, with a `RelativeCursorPosition`, to the node between the end caps): holding the
/// mouse on it drags the knob. Measured on a retail screenshot: a 134 px track and a 22 px knob.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SliderTrack {
    Victory,
    Fleet,
}
pub const SLIDER_TRACK: f32 = 134.0;
pub const SLIDER_KNOB: f32 = 22.0;

/// Dragging a slider's knob. The menu is rebuilt after every change, so the drag is remembered here rather than in the node.
fn slider_drag(
    tracks: Query<(&SliderTrack, &Interaction, &bevy::ui::RelativeCursorPosition)>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut sk: ResMut<Skirmish>,
    lib: Res<Library>,
    mut dragging: Local<Option<SliderTrack>>,
) {
    if !mouse.pressed(MouseButton::Left) {
        *dragging = None;
        return;
    }
    if dragging.is_none() {
        *dragging = tracks.iter().find(|(_, i, _)| **i == Interaction::Pressed).map(|(t, ..)| *t);
    }
    let Some(kind) = *dragging else { return };
    let Some(x) = tracks.iter().find(|(t, ..)| **t == kind).and_then(|(.., rel)| rel.normalized).map(|p| p.x + 0.5) else { return };
    let fraction = (x * SLIDER_TRACK - SLIDER_KNOB / 2.0) / (SLIDER_TRACK - SLIDER_KNOB);
    match kind {
        SliderTrack::Victory => {
            let v = VICTORY_LIMIT.at(fraction);
            if v != sk.victory_limit {
                sk.victory_limit = v;
            }
        }
        SliderTrack::Fleet => {
            let v = FLEET_SIZE.at(fraction) as i32 - sk.fleet_size as i32;
            if v != 0 {
                apply(&mut sk, &lib, &Action::FleetSize(v));
            }
        }
    }
}

/// Changes whenever the drawn screen would: views rebuild when it differs from the last one they drew.
pub type MenuKey = (Screen, usize, bool, usize, Vec<Slot>, Option<&'static str>, [usize; 6], u32, u32, String);
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
            [sk.ship_pick, sk.fleet_pick, sk.scroll, usize::from(sk.ship_chosen), usize::from(sk.view_slot), sk.victory_limit as usize],
            window.width() as u32,
            window.height() as u32,
            format!(
                "{:?}|{:?}|{}|{:?}|{}|{}|{}|{:?}|{:?}|{:?}|{}|{}|{}",
                self.ig.page,
                ui.selected,
                ui.name,
                ui.status,
                ui.scroll,
                ui.list.len(),
                cui.scroll,
                (cui.capture, &self.binds.rows),
                self.settings,
                sk.dropdown,
                sk.flag_picker,
                sk.map_scroll,
                sk.slot_scroll
            ) + &if self.ig.page == Some(Page::Summary) { self.stats.table() } else { String::new() },
        )
    }
}

/// Dev/smoke hook (`--menu lobby --autolaunch`): fills the human fleet like the fleet screen would and launches.
/// Screenshot hook (`--menu fleet --script fleet-crew|fleet-arms`): an auto-assigned Navy fleet with the Crew / Arms screen open.
fn fleet_hook(mut sk: ResMut<Skirmish>, lib: Res<Library>, mut done: Local<bool>) {
    let args: Vec<String> = std::env::args().collect();
    if *done || !args.windows(2).any(|w| w[0] == "--script" && w[1].starts_with("fleet-")) {
        return;
    }
    *done = true;
    if let Some(i) = sk.slots.iter().position(|s| s.kind == Kind::Human) {
        sk.slots[i].race = 0;
    }
    sk.fleet_size = 5;
    apply(&mut sk, &lib, &Action::AutoAssign);
    sk.arms_open = true;
}

fn autolaunch(mut sk: ResMut<Skirmish>, lib: Res<Library>, mut next: ResMut<NextState<Screen>>) {
    // Screenshot hook (`--menu lobby --script map-info`): open the Map Description dialog.
    if !sk.map_info && std::env::args().collect::<Vec<_>>().windows(2).any(|w| w[0] == "--script" && w[1] == "map-info") {
        sk.map_info = true;
    }
    if !std::env::args().any(|a| a == "--autolaunch") {
        return;
    }
    if sk.open() {
        sk.choose(Cell::Player(1), Choice::Computer);
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

/// A decided skirmish shows the Game Summary (missions end through their own rules). PROVISIONAL: retail delay before it opens unknown.
fn summary_on_end(mission: Option<Res<crate::campaign::Mission>>, out: Res<crate::scenario::Outcome>, mut ig: ResMut<InGame>, mut vtime: ResMut<Time<Virtual>>) {
    if out.is_changed() && *out != crate::scenario::Outcome::Playing && mission.is_none() {
        (ig.paused_by_us, ig.page) = (!vtime.is_paused(), Some(Page::Summary));
        vtime.pause();
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

/// The lobby / fleet-screen choices (everything that only edits the `Skirmish`).
/// Auto Assign for the human fleet (see `auto_build`).
pub fn auto_assign(sk: &mut Skirmish, lib: &Library) {
    let Some(i) = sk.slots.iter().position(|s| s.kind == Kind::Human) else { return };
    let mut slot = sk.slots[i].clone();
    auto_build(&mut slot, sk.victory_limit, sk.fleet_size, lib, 0, false);
    sk.slots[i] = slot;
    sk.error = None;
    if let Some(h) = sk.human().filter(|h| !h.fleet.is_empty()) {
        sk.fleet_pick = h.fleet.len() - 1;
        sk.view_slot = true;
        sk.show_picked_slot();
    }
}

fn apply(sk: &mut Skirmish, lib: &Library, action: &Action) {
    match action {
        Action::PickMap(m) => {
            sk.selected = *m;
            sk.load_map();
        }
        Action::Historical(h) => {
            sk.historical = *h;
            sk.map_scroll = 0;
            if let Some(p) = sk.maps.iter().position(|m| m.2 != *h) {
                sk.selected = p;
                sk.load_map();
            }
        }
        Action::VictoryLimit(d) => sk.victory_limit = VICTORY_LIMIT.step_value(sk.victory_limit, *d),
        Action::FleetSize(d) => {
            sk.fleet_size = FLEET_SIZE.step_value(sk.fleet_size as u32, *d) as usize;
            let n = sk.fleet_size;
            for s in &mut sk.slots {
                s.truncate_fleet(n);
            }
            sk.fleet_pick = sk.fleet_pick.min(n - 1);
            sk.scroll_slots(0);
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
                (sk.ship_chosen, sk.view_slot) = (false, false);
            }
        }
        Action::FlagPicker(open) => sk.flag_picker = *open,
        Action::PickFlag(n) => {
            let n = (*n).min(9);
            if let Some(i) = sk.slots.iter().position(|s| s.kind == Kind::Human)
                && !sk.flag_taken(i, sk.slots[i].race, n)
            {
                sk.slots[i].flag = n;
                sk.flag_picker = false;
            }
        }
        Action::SlotScroll(d) => sk.scroll_slots(*d),
        Action::Dropdown(cell) => sk.dropdown = *cell,
        Action::NameList(open) => sk.name_open = *open && sk.view_slot && sk.slot_filled(),
        Action::PickName(name) => sk.pick_name(name),
        Action::Choose(cell, choice) => sk.choose(*cell, *choice),
        Action::OpenArms | Action::OpenCrew => {
            sk.arms_open = sk.open() && sk.slot_filled();
            sk.arms_tab = matches!(action, Action::OpenArms);
        }
        Action::PickShip(row) => (sk.ship_pick, sk.ship_chosen, sk.view_slot, sk.name_open) = (*row, true, false, false),
        Action::MapInfo(open) => sk.map_info = *open,
        Action::PickFleet(row) => {
            (sk.fleet_pick, sk.view_slot, sk.name_open) = (*row, true, false);
            sk.show_picked_slot();
        }
        Action::AddShip => {
            sk.name_open = false;
            let (limit, pick, left) = (sk.fleet_size, sk.ship_pick, sk.points_left(lib));
            if let Some(h) = sk.slots.iter_mut().find(|s| s.kind == Kind::Human).filter(|h| h.fleet.len() < limit)
                && let Some(&ci) = offered(lib, h.race).get(pick)
                && i64::from(lib.classes[ci].points) <= left
            {
                h.push_ship(&lib.classes[ci].name);
                sk.fleet_pick = h.fleet.len() - 1;
                sk.view_slot = true;
            }
            sk.error = None;
            sk.show_picked_slot();
        }
        // PROVISIONAL: the original's auto-assign rule is not recovered; this adds the most expensive ship that still fits until the
        // fleet is full or nothing fits.
        Action::AutoAssign => auto_assign(sk, lib),
        Action::DropShip => {
            sk.name_open = false;
            let pick = sk.fleet_pick;
            if let Some(h) = sk.slots.iter_mut().find(|s| s.kind == Kind::Human).filter(|h| pick < h.fleet.len()) {
                h.remove_ship(pick);
                sk.fleet_pick = pick.saturating_sub(1);
                sk.view_slot = !h.fleet.is_empty();
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
    fn difficulty_toggle_cycles_normal_hard_easy() {
        let mut s = Settings::default();
        let mut seen = vec![s.difficulty];
        for _ in 0..3 {
            s.step(Setting::Difficulty);
            seen.push(s.difficulty);
        }
        assert_eq!(seen, [1, 2, 0, 1]);
    }

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
        let actions = [Action::Quit, Action::PickMap(0), Action::FleetSize(1), Action::Launch, Action::AddShip, Action::DropShip];
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
            fleet_size: 5,
            victory_limit: 300,
            map_json: Value::Null,
            error: None,
            dropdown: None,
            name_open: false,
            flag_picker: false,
            ship_pick: 0,
            ship_chosen: false,
            view_slot: false,
            fleet_pick: 0,
            slot_scroll: 0,
            map_scroll: 0,
            scroll: 0,
            arms_open: false,
            arms_tab: false,
            map_info: false,
        };
        sk.load_map();
        sk
    }
    #[test]
    fn skirmish_ship_names_default_to_the_pool_and_follow_the_pull_down() {
        let lib = Library::load(&assets_dir());
        let mut sk = skirmish("diablo_straight");
        sk.fleet_size = 2;
        sk.choose(Cell::Player(1), Choice::Computer);
        let human = sk.slots.iter().position(|s| s.kind == Kind::Human).unwrap();
        sk.ship_pick = offered(&lib, 0).iter().position(|&i| lib.classes[i].name == "Ship_Navy_Cutter").unwrap();
        apply(&mut sk, &lib, &Action::AddShip);
        apply(&mut sk, &lib, &Action::AddShip);
        let pool = lib.names["Ship_Navy_Cutter"].clone();
        let names = |sk: &Skirmish| sk.slots[human].ship_names(&lib);
        assert_eq!(names(&sk), vec![Some(pool[0].clone()), Some(pool[1].clone())], "same-hull ships get different default names");
        assert!(!sk.slots[human].name_choices(&lib, 0).contains(&pool[0]), "the pull-down omits the name the ship bears");
        sk.fleet_pick = 1;
        apply(&mut sk, &lib, &Action::NameList(true));
        assert!(sk.name_open);
        apply(&mut sk, &lib, &Action::PickName(pool[5].clone()));
        assert!(!sk.name_open);
        assert_eq!(names(&sk), vec![Some(pool[0].clone()), Some(pool[5].clone())]);
        let plan = sk.plan(&lib);
        let human_group = plan.iter().find(|g| g.human).unwrap();
        assert_eq!(human_group.ships[1].2.as_deref(), Some(pool[5].as_str()), "the chosen name reaches the battle");
        apply(&mut sk, &lib, &Action::DropShip);
        assert_eq!(sk.slots[human].names.len(), 1, "dropping a ship drops its name");
    }
    #[test]
    fn border_dispute_keeps_the_humans_command_ship() {
        use crate::*;
        use bevy::ecs::system::RunSystemOnce;
        let lib = Library::load(&assets_dir());
        let mut sk = skirmish("border_dispute");
        let other = sk.slots.iter().position(|s| s.kind != Kind::Human).unwrap();
        sk.choose(Cell::Player(other), Choice::Computer);
        if sk.open() {
            sk.ship_pick = 0;
            apply(&mut sk, &lib, &Action::AddShip);
        }
        let plan = sk.plan(&lib);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<bevy::world_serialization::WorldAsset>()
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<StandardMaterial>()
            .init_asset::<AudioSource>()
            .init_asset::<crate::fx::EffectMaterial>()
            .insert_resource(Library::load(&crate::assets_dir()))
            .init_resource::<fx::FxLib>()
            .init_resource::<collision::Shapes>()
            .init_resource::<data::AsteroidDefs>()
            .init_resource::<ship::Fleets>()
            .init_resource::<Target>()
            .insert_resource(Orbit { yaw: 0.0, pitch: 0.0, dist: 60.0, focus: None })
            .insert_resource(Config {
                groups: plan,
                player: String::new(),
                allies: vec![],
                enemies: vec![],
                map: Some("border_dispute".into()),
                shot: None,
                frame: 0,
                speed: 1.0,
                ended: None,
                script: None,
                shot_frame: 0,
                late_frame: 0,
            });
        app.world_mut()
            .run_system_once(|mut c: Commands, a: Res<AssetServer>, l: Res<Library>, f: Res<fx::FxLib>, s: Res<collision::Shapes>, r: Res<data::AsteroidDefs>, cfg: Res<Config>| {
                crate::scenario::spawn_scenario(&mut c, &a, &l, &f, &s, &r, &cfg)
            })
            .unwrap();
        let w = app.world_mut();
        let n = w.query_filtered::<&Ship, With<PlayerShip>>().iter(w).count();
        assert_eq!(n, 1, "the human's command ship must stay the player's ship once the scenario has been reconciled");
    }
    #[test]
    fn skirmish_weapon_choices_follow_fleet_entries_and_survive_launch_planning() {
        let lib = Library::load(&assets_dir());
        let mut sk = skirmish("diablo_straight");
        sk.fleet_size = 3;
        sk.choose(Cell::Player(1), Choice::Computer);
        let human = sk.slots.iter().position(|s| s.kind == Kind::Human).unwrap();
        sk.ship_pick = offered(&lib, 0).iter().position(|&i| lib.classes[i].name == "Ship_Navy_Cutter").unwrap();
        apply(&mut sk, &lib, &Action::AddShip);
        apply(&mut sk, &lib, &Action::AddShip);
        let class = &lib.classes[lib.index("Ship_Navy_Cutter")];
        let bank = class.mounts.iter().find(|m| m.gun == "Gun_CannonBall_Med").unwrap().bank;
        let before = sk.points_left(&lib);
        sk.slots[human].weapons[0].insert(bank, "Gun_Carronade_Med".into());
        let mounts = class.mounts.iter().filter(|m| m.bank == bank).count() as i64;
        let price = |g: &str| i64::from(lib.guns[g].points.unwrap());
        assert_eq!(before - sk.points_left(&lib), (price("Gun_Carronade_Med") - price("Gun_CannonBall_Med")) * mounts, "a refit costs the price difference per gun");
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
        sk.choose(Cell::Fleet(human), Choice::Fleet(2));
        assert!(sk.slots[human].fleet.is_empty());
        assert!(sk.slots[human].weapons.is_empty());
    }

    #[test]
    fn historical_map_commands_the_ship_its_rules_mark_primary() {
        // Iron War lists Navy 1's two torpedo boats first, but its Setup Ship rule makes the fast frigate the primary.
        let lib = Library::load(&assets_dir());
        let navy = &skirmish("iron_war").plan(&lib)[0].ships;
        assert_eq!((navy[0].0.as_str(), navy[0].2.as_deref()), ("Ship_Navy_FastFrigate", Some("RLS Star Finder")));
        assert_eq!(navy.len(), 3);
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
    fn three_team_historical_map_is_a_three_way_battle() {
        // The Maw: Navy, Procyon and Pirates are three teams; the two computer fleets are hostile to each other as well.
        let lib = Library::load(&assets_dir());
        let sides: Vec<_> = skirmish("the_maw").plan(&lib).iter().map(|g| g.side).collect();
        assert_eq!(sides, [crate::Side::Player, crate::Side::Enemy, crate::Side(2)]);
    }
    #[test]
    fn open_map_locks_fleet_type_to_team_and_needs_a_fleet() {
        let mut sk = skirmish("red_rover");
        assert_eq!((sk.slots[0].race, sk.slots[1].race), (0, 2));
        assert!(sk.slots.iter().skip(1).all(|s| s.kind == Kind::Open), "retail: you alone, every other slot Open");
        assert_eq!(sk.launch_error(), Some("No opponent: open another slot on a different team."));
        sk.choose(Cell::Player(1), Choice::Computer);
        assert_eq!(sk.launch_error(), Some("IDGS_TPFRONTENDTEXT_SCREENS_ERROR_FLEET"));
        assert!(sk.choices(Cell::Fleet(1)).is_empty() && sk.choices(Cell::Team(0)).is_empty(), "team maps lock fleet type and team");
        sk.slots[0].fleet.push("Ship_Navy_Cutter".into());
        assert_eq!(sk.launch_error(), None);
        let lib = Library::load(&assets_dir());
        let plan = sk.plan(&lib);
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[1].side, crate::Side::Enemy);
        assert!(plan[1].ships.len() == sk.fleet_size && plan[1].ships.iter().all(|s| s.0.contains("Pirate")), "computer fleet is built from its fleet type");
        // Free-for-all map: you choose the fleet type yourself.
        let mut sk = skirmish("diablo_straight");
        assert_eq!((sk.slots[0].race, sk.slots[0].team, sk.slots[1].kind), (0, 0, Kind::Open), "retail: you are Navy, Team 1");
        sk.choose(Cell::Player(1), Choice::Computer);
        assert_eq!((sk.slots[1].race, sk.slots[1].team), (2, 1), "a new computer: Pirate, Team 2");
        assert_eq!(sk.choices(Cell::Team(1))[..2], [Choice::Team(1), Choice::Team(0)], "current team first");
        assert_eq!(sk.choices(Cell::Fleet(0)), [Choice::Fleet(0), Choice::Fleet(2), Choice::Fleet(3)]);
        sk.choose(Cell::Player(1), Choice::Close);
        assert_eq!((sk.slots[1].kind, sk.slots[1].race, sk.slots[1].team), (Kind::Closed, 4, -1));
    }
    #[test]
    fn lobby_actions_edit_slots_and_fleet() {
        let lib = Library::load(&assets_dir());
        let mut sk = skirmish("diablo_straight"); // free-for-all
        sk.fleet_size = 2;
        assert_eq!(sk.slots[0].race, 0);
        let cheapest = offered(&lib, 0).len() - 1; // the list is most expensive first; points never limit cheap ships here
        apply(&mut sk, &lib, &Action::PickShip(cheapest));
        for _ in 0..3 {
            apply(&mut sk, &lib, &Action::AddShip);
        }
        assert_eq!(sk.slots[0].fleet.len(), 2, "fleet size limit");
        apply(&mut sk, &lib, &Action::DropShip);
        assert_eq!(sk.slots[0].fleet.len(), 1);
        sk.choose(Cell::Fleet(0), Choice::Fleet(2));
        assert!(sk.slots[0].fleet.is_empty() && sk.slots[0].race == 2, "a new fleet type clears the fleet");
        apply(&mut sk, &lib, &Action::PickShip(0));
        apply(&mut sk, &lib, &Action::AddShip);
        sk.choose(Cell::Team(0), Choice::Team(4));
        assert!(sk.slots[0].fleet.is_empty() && sk.slots[0].team == 4, "the original resets the fleet when the team changes");
        sk.choose(Cell::Player(2), Choice::Computer);
        assert_eq!(sk.slots[2].kind, Kind::Computer);
    }
    #[test]
    fn named_crew_cost_victory_points_and_leave_with_their_ship() {
        let lib = Library::load(&assets_dir());
        let mut sk = skirmish("diablo_straight");
        apply(&mut sk, &lib, &Action::PickShip(1));
        apply(&mut sk, &lib, &Action::AddShip);
        let before = sk.points_left(&lib);
        let id = crate::crew_arms::roster("Navy", 0)[0].clone();
        let cost = i64::from(crate::crew_arms::crew_points(&id));
        sk.slots[0].crew_mut(0)[1].push(id.clone());
        assert_eq!(before - sk.points_left(&lib), cost);
        assert_eq!(sk.slots[0].crew_at(&id), Some((0, 1)));
        apply(&mut sk, &lib, &Action::DropShip);
        assert!(sk.slots[0].crew_at(&id).is_none(), "dropping a ship returns its crew");
        assert_eq!(sk.points_left(&lib), 300);
    }
    #[test]
    fn auto_assign_mixes_hulls_that_fit_and_spends_the_rest_on_crew() {
        let lib = Library::load(&assets_dir());
        let mut sk = skirmish("diablo_straight");
        auto_assign(&mut sk, &lib);
        let h = &sk.slots[0];
        assert_eq!(h.fleet.len(), 5, "a 300-point budget fills five slots");
        assert!(h.fleet.iter().any(|n| n != &h.fleet[0]), "classes are mixed");
        assert!(h.crew.iter().flatten().flatten().count() > 0, "leftover points buy named crew");
        assert!(sk.points_left(&lib) >= 0, "weapon swaps stay inside the budget");
        let left = sk.points_left(&lib);
        assert!((0..30).contains(&left), "nearly everything is spent: {left}");
        // A tiny budget buys the few cheap hulls that fit.
        let mut sk = skirmish("diablo_straight");
        sk.victory_limit = 50;
        auto_assign(&mut sk, &lib);
        assert!(!sk.slots[0].fleet.is_empty() && sk.points_left(&lib) >= 0);
    }
    #[test]
    fn computers_cycle_fleet_types_by_slot_and_fleets_never_share_a_flag() {
        let mut sk = skirmish("diablo_straight");
        for i in 1..=4 {
            sk.choose(Cell::Player(i), Choice::Computer);
        }
        assert_eq!(sk.slots.iter().take(5).map(|s| s.race).collect::<Vec<_>>(), [0, 2, 3, 0, 2], "Navy, Pirate, Procyon, then round again");
        // Two Navy fleets: the second gets the next flag, and the first cannot take it.
        assert_eq!((sk.slots[0].flag, sk.slots[3].flag), (0, 1));
        assert!(sk.flag_taken(0, 0, 1));
        let lib = Library::load(&assets_dir());
        apply(&mut sk, &lib, &Action::PickFlag(1));
        assert_eq!(sk.slots[0].flag, 0, "a taken flag is refused");
        apply(&mut sk, &lib, &Action::PickFlag(5));
        assert_eq!(sk.slots[0].flag, 5);
        // Changing a fleet type moves it off a flag the new type's fleets already fly.
        sk.choose(Cell::Fleet(3), Choice::Fleet(2));
        assert_ne!(sk.slots[3].flag, sk.slots[4].flag);
    }
    #[test]
    fn computer_fleets_use_auto_assign_inside_the_victory_limit() {
        let lib = Library::load(&assets_dir());
        let mut sk = skirmish("diablo_straight");
        sk.choose(Cell::Player(1), Choice::Computer);
        sk.slots[0].push_ship("Ship_Navy_Cutter");
        let cost = |sk: &Skirmish| -> (usize, i64) {
            let g = &sk.plan(&lib)[1];
            let ships: i64 = g.ships.iter().map(|(n, ..)| i64::from(lib.classes[lib.index(n)].points)).sum();
            (g.ships.len(), ships)
        };
        let (n, points) = cost(&sk);
        assert!(n > 0 && points <= 300, "{n} ships cost {points}");
        sk.victory_limit = 60;
        let (n, points) = cost(&sk);
        assert!(n > 0 && points <= 60, "a small limit buys cheap hulls: {n} ships cost {points}");
        assert!(sk.plan(&lib)[1].crew.is_empty(), "computer ships keep the measured computer crew factor");
    }
    #[test]
    fn slot_bar_shows_five_slots_and_follows_the_picked_one() {
        let lib = Library::load(&assets_dir());
        let mut sk = skirmish("diablo_straight");
        sk.fleet_size = 8;
        sk.scroll_slots(10);
        assert_eq!(sk.slot_scroll, 3, "eight slots: the last window starts at slot 4");
        sk.scroll_slots(-10);
        assert_eq!(sk.slot_scroll, 0);
        apply(&mut sk, &lib, &Action::PickShip(offered(&lib, 0).len() - 1));
        for _ in 0..7 {
            apply(&mut sk, &lib, &Action::AddShip);
        }
        assert_eq!((sk.fleet_pick, sk.slot_scroll), (6, 2), "adding the seventh ship scrolls it into view");
        apply(&mut sk, &lib, &Action::PickFleet(0));
        assert_eq!(sk.slot_scroll, 0);
    }
    #[test]
    fn create_skirmish_sliders_use_the_retail_ranges() {
        let lib = Library::load(&assets_dir());
        let mut sk = skirmish("diablo_straight");
        assert_eq!((sk.victory_limit, sk.fleet_size), (300, 5));
        apply(&mut sk, &lib, &Action::VictoryLimit(-100));
        assert_eq!(sk.victory_limit, 50);
        apply(&mut sk, &lib, &Action::VictoryLimit(1));
        assert_eq!(sk.victory_limit, 60);
        apply(&mut sk, &lib, &Action::VictoryLimit(500));
        assert_eq!(sk.victory_limit, 1000);
        apply(&mut sk, &lib, &Action::FleetSize(-20));
        assert_eq!(sk.fleet_size, 1);
        apply(&mut sk, &lib, &Action::FleetSize(20));
        assert_eq!(sk.fleet_size, 10);
        // Dragging the knob: the end stops and the step rounding.
        assert_eq!((VICTORY_LIMIT.at(0.0), VICTORY_LIMIT.at(1.0), VICTORY_LIMIT.at(0.5)), (50, 1000, 530));
        assert_eq!((FLEET_SIZE.at(-3.0), FLEET_SIZE.at(0.5), FLEET_SIZE.at(9.0)), (1, 6, 10));
    }
    #[test]
    fn fleet_screen_starts_blank_and_greys_crew_arms_drop() {
        let lib = Library::load(&assets_dir());
        let mut sk = skirmish("diablo_straight");
        assert!(sk.shown_class(&lib).is_none() && !sk.slot_filled() && !sk.ship_chosen);
        apply(&mut sk, &lib, &Action::PickShip(1));
        assert!(sk.shown_class(&lib).is_some() && !sk.slot_filled(), "a picked hull shows, Crew/Arms/Drop stay greyed");
        apply(&mut sk, &lib, &Action::AddShip);
        assert!(sk.slot_filled());
        apply(&mut sk, &lib, &Action::DropShip);
        assert!(!sk.slot_filled());
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
