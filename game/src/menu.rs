//! Front-end model shared by both presentations: Main Menu -> Single Player -> Create Skirmish Game -> Skirmish Game Lobby -> battle.
//! Owns the screen state, the lobby/fleet choices (`Skirmish`), saves, the in-game Esc menu and the `Action`s buttons carry.
//! Views (`classic/menu.rs` retail screens) draw the current screen in `BuildMenu` from `MenuModel`
//! and tag their buttons with an `Action`; `buttons` / `save_actions` here run them. Maps: assets/maps/<id> (tools/export_maps.py).

mod lobby_scenario;
mod missions;
mod mods;
mod preview;
mod saves;
mod script_hooks;
mod scroll;
mod settings;
mod skirmish;
pub use lobby_scenario::*;
pub use missions::*;
pub use mods::*;
pub use preview::*;
pub use saves::*;
pub use scroll::*;
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
    /// Options > Mods: enable or disable the mods installed in the user folder.
    Mods,
    Controls,
    /// Main menu > Tutorial: Basic Training / Full Training (`Tutorial Screen.tst`).
    Tutorial,
    /// Not retail: Single Player > Custom Missions, the missions mods add (`MissionList`).
    Missions,
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
    /// Pick custom mission `n` (`MissionList`) for the Custom Missions panel; play it.
    PickMission(usize),
    PlayMission(usize),
    OpenLoad,
    Ingame(Option<Page>),
    PickSave(usize),
    DoLoad,
    DoSave,
    Remove,
    /// Open or close the lobby's Map Description dialog.
    MapInfo(bool),
    /// Open the Load (false) or Save (true) dialog of fleets, crews or arms; pick a file in it; Load / Save; Cancel.
    OpenProfile(crate::profiles::Kind, bool),
    PickProfile(usize),
    DoProfile,
    CloseProfile,
    QuitToMenu,
    QuickSave,
    QuickLoad,
    Rebind(usize),
    RestoreKeys,
    /// Step one Sound / Display option (`Settings`).
    Setting(Setting),
    /// Switch installed mod `n` on or off (applies on the next launch).
    ToggleMod(usize),
    SortMods(ModCol),
    /// Pick mod `n` for the Mods screen's right-hand panel.
    PickMod(usize),
    /// Open / close the Mods category pull-down; pick its entry `n` (0 = All).
    ModFilterOpen,
    ModFilter(usize),
    /// Bake the HD skies from the imported ones (`SkyBake`), or stop the bake running.
    BakeSkies,
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
/// What the retail screen files' button elements (`menu/<screen>.json`, by their `impl` name) do in this engine.
#[derive(Clone)]
pub enum RetailButton {
    /// Enabled: runs this action.
    Run(Action),
    /// Shown greyed out (retail greys the button out until its condition holds).
    Disabled,
    /// The tab of the screen that is open now: drawn lit, does nothing.
    CurrentTab,
}

/// The state `retail_button` decides from.
pub struct ButtonContext<'a> {
    /// The open page of the in-game menu.
    pub page: Option<Page>,
    pub skirmish: &'a Skirmish,
    pub outcome: crate::scenario::Outcome,
}

/// The action table of the retail buttons and when they are enabled. `None`: not a button of this table (a backdrop, a list, a label,
/// a slider ...), or a retail button this engine does not implement on `screen` (the view then greys it out when it has a label).
/// Views only draw the result.
pub fn retail_button(screen: Screen, impl_: &str, cx: &ButtonContext) -> Option<RetailButton> {
    use RetailButton::{CurrentTab, Disabled, Run};
    let sk = cx.skirmish;
    Some(match (screen, impl_) {
        (Screen::Main, "Open Single Player Menu Button") => Run(Action::Goto(Screen::SinglePlayer)),
        (Screen::Main, "Open Option Screen") => Run(Action::Goto(Screen::Options)),
        (Screen::Main, "Open Tutorial Button") => Run(Action::Goto(Screen::Tutorial)),
        (Screen::Main, "Open Next Screen Button") => Run(Action::Quit),
        (Screen::Tutorial, "Start Basic Training Tutorial Button") => Run(Action::Tutorial("basic_tutorial")),
        (Screen::Tutorial, "Start Full Training Tutorial Button") => Run(Action::Tutorial("tutorial")),
        (Screen::Tutorial, "Open Prev Arrow Button") => Run(Action::Goto(Screen::Main)),
        (Screen::SinglePlayer, "Open Load Campaign Screen Button") => Run(Action::OpenLoad),
        (Screen::SinglePlayer, "Open Next Screen Button") => Run(Action::Campaign),
        (Screen::SinglePlayer, "Open Create Skirmish Game Screen") => Run(Action::Goto(Screen::CreateSkirmish)),
        (Screen::SinglePlayer, "Open Prev Arrow Button") => Run(Action::Goto(Screen::Main)),
        (Screen::CreateSkirmish, "Open Prev Arrow Button") => Run(Action::Goto(Screen::SinglePlayer)),
        (Screen::CreateSkirmish, "Open Next Arrow Button") => Run(Action::Goto(Screen::Lobby)),
        (Screen::Load, "Open Prev Arrow Button") => Run(Action::Goto(Screen::SinglePlayer)),
        (Screen::Load | Screen::Battle, "Load or Save Campaign Button") => Run(if cx.page == Some(Page::Save) { Action::DoSave } else { Action::DoLoad }),
        (Screen::Load | Screen::Battle, "Open Confirm Saved Game Delete Dialog Button") => Run(Action::Remove),
        (Screen::Battle, "Open Next Screen Button") => Run(Action::Ingame(Some(Page::Options))),
        (Screen::Battle, "In Game Menu Stats Screen Button") => Run(Action::Ingame(Some(Page::Summary))),
        // A decided battle continues to the main menu (measured on a won skirmish); opened from the in-game menu it goes back there.
        (Screen::Battle, "Open Next Arrow Button") => Run(if cx.outcome == crate::scenario::Outcome::Playing { Action::Ingame(Some(Page::Menu)) } else { Action::QuitToMenu }),
        (Screen::Battle, "Open Saved Campaign Screen Button") => Run(Action::Ingame(Some(Page::Save))),
        (Screen::Battle, "Open Load Campaign Screen Button") => Run(Action::Ingame(Some(Page::Load))),
        (Screen::Battle, "InGameMenu QuitGame Button") => Run(Action::QuitToMenu),
        (Screen::Battle, "Open Previous Screen Button") => Run(Action::Ingame(if cx.page == Some(Page::Menu) { None } else { Some(Page::Menu) })),
        // Retail greys Launch Game until a computer opponent is in and (open maps) a fleet is assembled.
        (Screen::Lobby, "Open Next Screen Button") => {
            if sk.launch_error().is_some() {
                Disabled
            } else {
                Run(Action::Launch)
            }
        }
        (Screen::Lobby, "Open Map Description Dialog Button") => Run(Action::MapInfo(true)),
        (Screen::Lobby, "Open Previous Screen Button") => Run(Action::Goto(Screen::CreateSkirmish)),
        (Screen::Lobby, "Open Ship Selection Screen") if sk.open() => Run(Action::Goto(Screen::Fleet)),
        (Screen::Fleet, "Open Ship Selection Screen") => CurrentTab,
        // Retail greys these out: Crew, Arms and Drop until the picked fleet slot holds a ship, Add until a hull type is clicked.
        (Screen::Fleet, "Add Ship To Fleet Button") => enabled_if(sk.ship_chosen, Action::AddShip),
        (Screen::Fleet, "Drop Ship From Fleet Button") => enabled_if(sk.slot_filled(), Action::DropShip),
        (Screen::Fleet, "Open Crew Selection Screen") => enabled_if(sk.slot_filled(), Action::OpenCrew),
        (Screen::Fleet, "Open Arms Selection Screen") => enabled_if(sk.slot_filled(), Action::OpenArms),
        (Screen::Fleet, "Open Next Arrow Button") => Run(Action::Goto(Screen::Lobby)),
        (Screen::Fleet, "AutoAssign Button") => Run(Action::AutoAssign),
        (Screen::Fleet, "Open Load Fleet Profile Dialog Button") => Run(Action::OpenProfile(crate::profiles::Kind::Fleet, false)),
        (Screen::Fleet, "Open Save Fleet Profile Dialog Button") if sk.human().is_some_and(|h| !h.fleet.is_empty()) => Run(Action::OpenProfile(crate::profiles::Kind::Fleet, true)),
        _ => return None,
    })
}

fn enabled_if(on: bool, action: Action) -> RetailButton {
    if on { RetailButton::Run(action) } else { RetailButton::Disabled }
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
        (
            Action::FleetSize(_)
            | Action::VictoryLimit(_)
            | Action::Setting(_)
            | Action::ToggleMod(_)
            | Action::SortMods(_)
            | Action::PickMod(_)
            | Action::ModFilterOpen
            | Action::ModFilter(_),
            _,
        ) => "SpinButton",
        (Action::Launch | Action::Campaign | Action::Tutorial(_), _) => "StartButton",
        (Action::AddShip, _) => "FE_Add",
        (Action::DropShip | Action::Remove, _) => "FE_Drop",
        _ => "FE_Button_Select",
    }
}

/// Rollover when the pointer first reaches a widget, the select sound when it is pressed (not again when the press is released).
#[allow(deprecated)] // Interaction, see pointer.rs
fn button_sounds(
    q: Query<(Entity, &Interaction, &Action, Option<&ArrowButton>), Changed<Interaction>>,
    mut pointed: Local<std::collections::HashSet<Entity>>,
    mut sfx: MessageWriter<crate::audio::Sfx>,
) {
    for (e, i, action, arrow) in &q {
        if !crate::pointer::over(i) {
            pointed.remove(&e);
            continue;
        }
        if pointed.insert(e) {
            sfx.write(crate::audio::Sfx::ui(button_sound(action, arrow, false)));
        }
        if crate::pointer::down(i) {
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
        draw: 0,
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
        profile: None,
    };
    sk.load_map();
    app.insert_resource(sk)
        .add_systems(OnEnter(Screen::Battle), (clear_menu, stop_music))
        .add_systems(OnEnter(Screen::Main), start_music)
        .add_systems(Update, follow_music_volume)
        .insert_resource(SaveUi { list: crate::campaign::list_saves(), ..default() })
        .init_resource::<InGame>()
        .init_resource::<ScrollTo>()
        .init_resource::<CtrlUi>()
        .insert_resource(Settings::load())
        .insert_resource(ModList::load())
        .insert_resource(MissionList::load())
        .init_resource::<BakeJob>()
        .add_systems(Update, (apply_settings, poll_bake))
        .add_systems(Last, stop_bake_on_exit)
        .configure_sets(Update, BuildMenu.after(capture_key).before(RunButtons).run_if(menu_active))
        .add_systems(
            Update,
            (
                capture_key,
                slider_drag.run_if(in_state(Screen::CreateSkirmish)),
                scroll_drag,
                dropdown_release,
                buttons.in_set(RunButtons).run_if(menu_active),
                button_sounds.before(RunButtons).run_if(menu_active),
                save_actions,
                save_text.run_if(menu_active),
                profile_keys.run_if(menu_active),
            ),
        )
        .add_systems(PostUpdate, (summary_on_end, esc_menu).chain().run_if(in_state(Screen::Battle)))
        .add_systems(Update, (lobby_scenario, autolaunch).chain().run_if(in_state(Screen::Lobby)))
        .add_systems(Update, (scroll_controls, scroll_saves, scroll_mods, scroll_missions).before(BuildMenu).run_if(menu_active))
        .add_systems(Update, scroll_skirmish.after(RunButtons).run_if(menu_active))
        .add_plugins(script_hooks::plugin);
}

#[derive(Component)]
struct MenuMusic;

/// Retail front-end track (the original loads "FrontEndMusic"); plays from the main menu until a battle starts.
fn start_music(mut commands: Commands, assets: Res<AssetServer>, playing: Query<(), With<MenuMusic>>) {
    if playing.is_empty() {
        let e = crate::audio::music(&mut commands, &assets, "FrontEndMusic");
        // Starts silent: the saved levels are only applied in the first frame, so `follow_music_volume` sets the real level.
        commands.entity(e).remove::<crate::audio::Music>().insert((MenuMusic, PlaybackSettings::LOOP.with_volume(bevy::audio::Volume::Linear(0.0))));
    }
}

/// Keeps the menu track at the Music slider's level every frame (the volume given at spawn is never revisited).
fn follow_music_volume(mut music: Query<&mut AudioSink, With<MenuMusic>>) {
    for mut sink in &mut music {
        sink.set_volume(bevy::audio::Volume::Linear(crate::audio::MUSIC_VOL * volume(Setting::Music)));
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
/// The retail string table (`menu/strings.json`), read once.
pub fn strings() -> &'static Value {
    static S: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    S.get_or_init(|| crate::data::try_json(&assets_dir().join("menu/strings.json")).unwrap_or(Value::Null))
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
    pub mods: Res<'w, ModList>,
    pub missions: Res<'w, MissionList>,
    pub stats: Res<'w, crate::stats::GameStats>,
    pub outcome: Res<'w, crate::scenario::Outcome>,
    pub end_message: Res<'w, crate::scenario::EndMessage>,
}
/// An entry of an open pull-down list (see `classic::menu::dropdown`); `.0` is whether it also picks on release.
#[derive(Component)]
pub struct ListEntry(pub bool);

/// Retail pull-downs stay open while the button is held: press the cell, drag to an entry, release to pick it. The lobby's current
/// value (a plain click) does not pick on release, so that click keeps the list open.
#[allow(deprecated)] // Interaction, see pointer.rs
fn dropdown_release(mouse: Res<ButtonInput<MouseButton>>, entries: Query<(&Action, &Interaction, &ListEntry)>, mut sk: ResMut<Skirmish>, lib: Res<Library>) {
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    if let Some((action, ..)) = entries.iter().find(|(_, i, e)| e.0 && crate::pointer::over(i)) {
        apply(&mut sk, &lib, action);
    }
}

/// Which list a draggable scrollbar scrolls (retail scrollbars all work the same: press the track or thumb and drag).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScrollKind {
    Maps,
    Ships,
    Mods,
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

/// The track a held mouse button is dragging: the one pressed when the drag began, until the button is let go.
fn held_track<T: Copy>(mouse: &ButtonInput<MouseButton>, dragging: &mut Option<T>, pressed: Option<T>) -> Option<T> {
    if !mouse.pressed(MouseButton::Left) {
        *dragging = None;
        return None;
    }
    if dragging.is_none() {
        *dragging = pressed;
    }
    *dragging
}

/// Dragging any scrollbar. The menu is rebuilt after every change, so the drag is remembered here rather than in the node.
#[allow(deprecated)] // Interaction, see pointer.rs
fn scroll_drag(
    tracks: Query<(&ScrollTrack, &Interaction, &bevy::ui::RelativeCursorPosition)>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut sk: ResMut<Skirmish>,
    mut to: ResMut<ScrollTo>,
    mut mods: ResMut<ModList>,
    mut dragging: Local<Option<ScrollKind>>,
) {
    let Some(kind) = held_track(&mouse, &mut dragging, tracks.iter().find(|(_, i, _)| crate::pointer::down(i)).map(|(t, ..)| t.kind)) else { return };
    let Some((t, y)) = tracks.iter().find(|(t, ..)| t.kind == kind).and_then(|(t, _, rel)| rel.normalized.map(|p| (*t, p.y + 0.5))) else { return };
    // The thumb's centre follows the cursor along the room its length leaves.
    let pos = ((y - t.thumb / 2.0) / (1.0 - t.thumb).max(0.01)).clamp(0.0, 1.0);
    let row = (pos * t.total.saturating_sub(t.visible) as f32).round() as usize;
    match kind {
        ScrollKind::Maps => sk.map_scroll = row,
        ScrollKind::Ships => sk.scroll = row,
        ScrollKind::Mods => mods.scroll = row,
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
#[allow(deprecated)] // Interaction, see pointer.rs
fn slider_drag(
    tracks: Query<(&SliderTrack, &Interaction, &bevy::ui::RelativeCursorPosition)>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut sk: ResMut<Skirmish>,
    lib: Res<Library>,
    mut dragging: Local<Option<SliderTrack>>,
) {
    let Some(kind) = held_track(&mouse, &mut dragging, tracks.iter().find(|(_, i, _)| crate::pointer::down(i)).map(|(t, ..)| *t)) else { return };
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
    /// Whether anything `key` reads was touched this frame; when not, the key (a large formatted string) cannot have changed.
    pub fn changed(&self) -> bool {
        self.sk.is_changed()
            || self.ig.is_changed()
            || self.saves.is_changed()
            || self.binds.is_changed()
            || self.ctrl.is_changed()
            || self.state.is_changed()
            || self.settings.is_changed()
            || self.mods.is_changed()
            || self.missions.is_changed()
            || self.stats.is_changed()
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
                (&self.settings, &self.mods, (self.missions.picked, self.missions.scroll)),
                (sk.dropdown, sk.name_open, sk.map_info, sk.arms_open, sk.arms_tab),
                sk.flag_picker,
                sk.map_scroll,
                sk.slot_scroll
            ) + &if self.ig.page == Some(Page::Summary) { self.stats.table() } else { String::new() },
        )
    }
}

/// The Launch button: the battle starts when the lobby has a human and an opponent (and, on open maps, a fleet).
fn launch(sk: &mut Skirmish, next: &mut NextState<Screen>) {
    sk.error = sk.launch_error();
    if sk.error.is_none() {
        start_battle(sk, next);
    }
}

fn start_battle(sk: &mut Skirmish, next: &mut NextState<Screen>) {
    sk.map = sk.maps.get(sk.selected).map(|m| m.0.clone());
    next.set(Screen::Battle);
}

/// `--lobby <spec>` (`lobby_scenario.rs`, tools/battle_check.py): sets the lobby up from the spec, then presses Launch. A spec the
/// lobby refuses ends the run with status 2 instead of silently playing something else.
fn lobby_scenario(scenario: Option<Res<LobbyScenario>>, mut sk: ResMut<Skirmish>, lib: Res<Library>, mut next: ResMut<NextState<Screen>>, mut done: Local<bool>) {
    let Some(scenario) = scenario.filter(|_| !*done) else { return };
    *done = true;
    if let Err(err) = scenario.apply(&mut sk, &lib) {
        eprintln!("{err}");
        std::process::exit(2);
    }
    launch(&mut sk, &mut next);
}

/// Dev/smoke hook (`--menu lobby --autolaunch`): fills the human fleet like the fleet screen would and launches.
fn autolaunch(mut sk: ResMut<Skirmish>, lib: Res<Library>, cfg: Res<crate::Config>, mut next: ResMut<NextState<Screen>>) {
    if !cfg.autolaunch {
        return;
    }
    if sk.open() {
        sk.choose(Cell::Player(1), Choice::Computer);
        for _ in 0..sk.fleet_size {
            apply(&mut sk, &lib, &Action::AddShip);
        }
    }
    if sk.launch_error().is_none() {
        start_battle(&mut sk, &mut next);
    }
}

/// Esc: open the in-game menu (pausing the simulation), or step back / resume. Runs in PostUpdate so systems that used this Esc
/// (star map, right-click menu) can swallow it first. `--script ingame|ingame-load|ingame-save|ingame-quit` opens a page at frame 60 for screenshots.
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
        Some("ingame-quit") => Some(Page::Menu),
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

/// A decided skirmish plays on under the end line (`scenario::EndLine`) for `END_LINE_SECS`, then shows the Game Summary (missions end
/// through their own rules).
fn summary_on_end(
    mission: Option<Res<crate::campaign::Mission>>,
    out: Res<crate::scenario::Outcome>,
    mut ig: ResMut<InGame>,
    mut vtime: ResMut<Time<Virtual>>,
    mut waited: Local<f32>,
) {
    if *out == crate::scenario::Outcome::Playing || mission.is_some() {
        *waited = 0.0;
        return;
    }
    let before = *waited;
    *waited += vtime.delta_secs();
    if before < crate::scenario::END_LINE_SECS && *waited >= crate::scenario::END_LINE_SECS {
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
    let race = slot.race;
    auto_build(&mut slot, sk.victory_limit, sk.fleet_size, lib, 0, false, race);
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
        Action::PickShip(row) => {
            (sk.ship_pick, sk.ship_chosen, sk.view_slot, sk.name_open) = (*row, true, false, false);
            sk.error = None;
        }
        Action::MapInfo(open) => sk.map_info = *open,
        Action::OpenProfile(kind, save) => sk.open_profile(*kind, *save, lib),
        Action::PickProfile(n) => sk.pick_profile(*n),
        Action::DoProfile => sk.do_profile(lib),
        Action::CloseProfile => sk.profile = None,
        Action::PickFleet(row) => {
            (sk.fleet_pick, sk.view_slot, sk.name_open) = (*row, true, false);
            sk.show_picked_slot();
        }
        Action::AddShip => {
            sk.name_open = false;
            let (limit, pick, left) = (sk.fleet_size, sk.ship_pick, sk.points_left(lib));
            let mut error = None;
            if let Some(h) = sk.slots.iter_mut().find(|s| s.kind == Kind::Human).filter(|h| h.fleet.len() < limit)
                && let Some(&ci) = offered(lib, h.race).get(pick)
            {
                if i64::from(lib.classes[ci].points) <= left {
                    h.push_ship(&lib.classes[ci].name);
                    sk.fleet_pick = h.fleet.len() - 1;
                    sk.view_slot = true;
                } else {
                    // PROVISIONAL: retail greys the Add button only; the message is ours.
                    error = Some("Not enough victory points.");
                }
            }
            sk.error = error;
            sk.show_picked_slot();
        }
        // PROVISIONAL: the original's auto-assign rule is not recovered; `auto_build` picks random hulls that fit (skirmish.rs).
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

/// Seconds between two clicks on the same list row that make a double-click (Bevy has none). PROVISIONAL: the retail value is not
/// recovered.
pub const DOUBLE_CLICK: f64 = 0.4;

/// Remembers the last list-row click so the second of two quick clicks on one row can be told apart. `kind` separates the lists
/// (ships, crew roster, weapons); `row` is the row clicked; `now` is real time in seconds.
#[derive(Default, Debug)]
pub struct ClickClock {
    last: Option<(u8, usize, f64)>,
}
impl ClickClock {
    /// Records a click and says whether it completes a double-click (a third click starts over).
    pub fn double(&mut self, kind: u8, row: usize, now: f64) -> bool {
        let hit = self.last.is_some_and(|(k, r, t)| (k, r) == (kind, row) && now - t <= DOUBLE_CLICK);
        self.last = if hit { None } else { Some((kind, row, now)) };
        hit
    }
}

/// Runs the `Action` of a pressed button (whichever view drew it).
#[allow(deprecated)] // Interaction, see pointer.rs
fn buttons(
    q: Query<(&Interaction, &Action), Changed<Interaction>>,
    mut next: ResMut<NextState<Screen>>,
    mut sk: ResMut<Skirmish>,
    lib: Res<Library>,
    mut exit: MessageWriter<AppExit>,
    mut cfg: ResMut<crate::Config>,
    (mut binds, mut cui, mut settings, mut mods, mut bake, mut missions): (
        ResMut<Bindings>,
        ResMut<CtrlUi>,
        ResMut<Settings>,
        ResMut<ModList>,
        ResMut<BakeJob>,
        ResMut<MissionList>,
    ),
    time: Res<Time<Real>>,
    mut clicks: Local<ClickClock>,
) {
    for (i, action) in &q {
        if !crate::pointer::down(i) {
            continue;
        }
        let profile = matches!(action, Action::OpenProfile(..) | Action::PickProfile(_) | Action::DoProfile | Action::CloseProfile);
        if sk.arms_open && !profile {
            continue;
        }
        apply(&mut sk, &lib, action);
        // Double-click a hull in Available Ships: add it.
        if let Action::PickShip(row) = action
            && clicks.double(0, *row, time.elapsed_secs_f64())
        {
            apply(&mut sk, &lib, &Action::AddShip);
        }
        match action {
            Action::Goto(s) => next.set(*s),
            Action::Rebind(n) => cui.capture = Some(*n),
            Action::ToggleMod(n) => mods.toggle(*n),
            Action::SortMods(c) => mods.sort_by(*c),
            Action::PickMod(n) => {
                mods.picked = Some(*n);
                if clicks.double(1, *n, time.elapsed_secs_f64()) {
                    mods.toggle(*n);
                }
            }
            Action::ModFilterOpen => mods.filter_open = !mods.filter_open,
            Action::ModFilter(n) => mods.set_filter(*n),
            Action::BakeSkies => mods.bake_or_stop(&mut bake),
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
            Action::PickMission(n) | Action::PlayMission(n) => {
                missions.picked = Some(*n);
                let play = matches!(action, Action::PlayMission(_)) || clicks.double(2, *n, time.elapsed_secs_f64());
                if let Some(id) = missions.id(*n).filter(|_| play) {
                    sk.map = None;
                    cfg.map = Some(id.into());
                    cfg.ended = None;
                    next.set(Screen::Battle);
                }
            }
            Action::Launch => launch(&mut sk, &mut next),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
