//! In-game HUD contract shared by every presentation (`classic/panels.rs` draws it with retail sprites).
//!
//! A view builds its widgets under one `HudRoot` and tags them with the markers below (`PanicButton`, `GunButton`, `Contact`, ...);
//! this module gives them their behaviour: clicks, what the gauges read (`gauge_value`), radar contact placement (`Radar`), the
//! docking/boarding status icon, the talking-head state (`HeadState`), pause and the game-speed buttons. Views only draw.
use bevy::prelude::*;

use crate::bindings::Act;
use crate::combat::{self, GunStates, Health};
use crate::sprites::{HudData, Sprite};
use crate::{Library, PlayerShip, Ship};

/// Height of the reference screen the HUD art is scaled against (the reference video draws the 640x480 art at twice that height).
pub const REF_H: f32 = 960.0;

/// Scale from the 960-pixel-high reference layout to this window: every HUD size is multiplied by it.
pub fn hud_scale(window: &Window) -> f32 {
    window.height() / REF_H
}

/// Size of full-screen message text (the end line, "Loading...") at `MESSAGE_TEXT_REF_H` window height.
/// PROVISIONAL: one reference-footage frame measured about 16-17 px of 1080.
pub const MESSAGE_TEXT_PX: f32 = 17.0;
pub const MESSAGE_TEXT_REF_H: f32 = 1080.0;

/// Message text size in pixels for this window.
pub fn message_text_size(window: &Window) -> f32 {
    MESSAGE_TEXT_PX * window.height() / MESSAGE_TEXT_REF_H
}
pub const MAX_CONTACTS: usize = 16;
/// Radar range in world units mapped to the disk's radius: retail draws ships out to ~153 units (0.297 px per unit on a 45.6 px disk)
/// and none beyond (docs/re/hud.md).
const RADAR_RANGE: f32 = 153.0;
/// Ship status icons (docking, boarding, ...): `HUDRadar Ship Status Icon - Time To Next Frame` 0.13 s.
pub const STATUS_FPS: f32 = 1.0 / 0.13;
/// Retail `Animation Speed TalkingHeadWindow` is 2.0; PROVISIONAL: read as open/close progress per second.
const HEAD_SPEED: f32 = 2.0;

/// The shared HUD systems; views order their art updates after it and their builders before it.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub struct HudSet;

/// Player-facing HUD switches: weapon panel (button above the radar, or W) and ship-name banners (N).
#[derive(Resource)]
pub struct Toggles {
    pub weapons: bool,
    pub names: bool,
}
/// Set while the cursor is over a HUD panel so the world ignores clicks that land on it.
#[derive(Resource, Default)]
pub struct UiClick(pub bool);

/// Every "is a UI layer taking this input?" question, named by what it blocks. The sets differ on purpose or by accident, and each
/// predicate reproduces exactly one definition that existed at the call site, so none is a general "UI is open" answer:
///
/// | predicate | options menu open | click swallowed | HUD panel under cursor | crew screen | star map | cinematic | script-locked | not direct |
/// |---|---|---|---|---|---|---|---|---|
/// | `blocks_world_click` | | yes | yes | | | | | |
/// | `blocks_fleet_keys` | yes | | | | yes | yes | yes | |
/// | `blocks_fleet_bar` | yes | | | | | | | |
/// | `blocks_engine_dial` | yes | | | yes | | | | yes |
/// | `blocks_hover` | yes | | yes | | | yes | | |
///
/// MAINTAINER QUESTION: should these agree? Today the engine dial ignores the star map and cinematics, the fleet keys ignore the crew
/// screen, and the hover cursor ignores the crew screen and the star map. Not changed here.
#[derive(bevy::ecs::system::SystemParam)]
pub struct UiFocus<'w, 's> {
    click: Option<Res<'w, UiClick>>,
    menu: Option<Res<'w, crate::options_menu::OptionsMenu>>,
    crew: Option<Res<'w, crate::crew_arms::ScreenModel>>,
    maps: Query<'w, 's, (), With<crate::starmap::Starmap>>,
    mission: Option<Res<'w, crate::campaign::Mission>>,
}

impl UiFocus<'_, '_> {
    fn menu_open(&self) -> bool {
        self.menu.as_ref().is_some_and(|m| m.is_open())
    }
    fn over_hud(&self) -> bool {
        self.click.as_ref().is_some_and(|u| u.0)
    }
    /// A left click is not a world click: it lands on a HUD panel, or it just closed the options menu.
    pub fn blocks_world_click(&self) -> bool {
        self.over_hud() || self.menu.as_ref().is_some_and(|m| m.swallow.load(std::sync::atomic::Ordering::Relaxed))
    }
    /// The number keys and the primary-ship key do nothing: options menu, star map, cinematic or scripted lock.
    pub fn blocks_fleet_keys(&self) -> bool {
        self.menu_open() || !self.maps.is_empty() || cinematic(self.mission.as_deref()) || crate::rules::locked(self.mission.as_deref())
    }
    /// The ship bar's clicks (and its release latch) wait while the options menu is open.
    pub fn blocks_fleet_bar(&self) -> bool {
        self.menu_open()
    }
    /// The engine dial and order tabs do nothing: crew screen, options menu, or the selected ship is not the one flown directly.
    /// (The star map and cinematics are not in this set; see the type's maintainer question.)
    pub fn blocks_engine_dial(&self, fleets: Option<&crate::ship::Fleets>) -> bool {
        self.crew.as_ref().is_some_and(|m| m.open) || self.menu_open() || fleets.is_some_and(|f| !f.direct())
    }
    /// The world-under-cursor pick (cursor shape, hover) is off: cinematic, options menu, or the cursor is over a HUD panel.
    pub fn blocks_hover(&self) -> bool {
        cinematic(self.mission.as_deref()) || self.menu_open() || self.over_hud()
    }
}

/// Opening briefing or an in-game cutscene: the HUD hides and its controls do nothing.
pub fn cinematic(mission: Option<&crate::campaign::Mission>) -> bool {
    crate::briefing::active(mission) || crate::rules::nis(mission)
}

/// What the world HUD marks this frame, decided once here so every view draws the same things (views only place and fade them).
#[derive(Resource, Default, Debug, PartialEq)]
pub struct WorldMarks {
    /// The ship or animal the target reticle stands on: the aimed target while the player flies the selected ship, else the target
    /// the selected ship was ordered at; none when it is hidden, unseen or the HUD is off.
    pub reticle: Option<Entity>,
    /// The ships whose name banner may show (alive, visible to the mission, seen by the player, HUD on). A view still fades it with
    /// the camera distance and the toggle, and hides it when the ship is off screen.
    pub banners: std::collections::HashSet<Entity>,
}

/// The target the reticle marks: `aimed` (the player's own target) when the selected ship is the one flown, else its ordered one
/// (a fleet pilot's order first, then a mission ship's).
pub fn reticle_target(direct: bool, aimed: Option<Entity>, pilot_target: Option<Entity>, mission_target: Option<Entity>) -> Option<Entity> {
    if direct { aimed } else { pilot_target.or(mission_target) }
}

/// Whether a ship's name banner may show.
pub fn banner_allowed(destroyed: bool, mission_visible: bool, seen: bool, hud_off: bool) -> bool {
    !hud_off && !destroyed && mission_visible && seen
}

#[allow(clippy::too_many_arguments)]
fn decide_world_marks(
    mut marks: ResMut<WorldMarks>,
    command: Query<&crate::ship::TargetSlot, With<crate::PlayerShip>>,
    fleets: Res<crate::ship::Fleets>,
    pilots: Query<&crate::ship::FleetPilot>,
    ships: Query<(Entity, &Health, Option<&crate::campaign::MissionShip>), With<Ship>>,
    animals: Query<&crate::environment::SpaceAnimal>,
    sensors: Res<crate::environment::Sensors>,
    mission: Option<Res<crate::campaign::Mission>>,
) {
    let hud_off = cinematic(mission.as_deref()) || crate::rules::console::hud_hidden();
    let mut next = WorldMarks::default();
    for (e, h, ms) in &ships {
        if banner_allowed(h.destroyed, ms.is_none_or(|m| m.visible), sensors.player_sees(e, mission.as_deref()), hud_off) {
            next.banners.insert(e);
        }
    }
    if !hud_off {
        let ordered = fleets.selected.map(|e| (pilots.get(e).ok().and_then(|p| p.target), ships.get(e).ok().and_then(|s| s.2.and_then(|m| m.ordered))));
        let (pilot, mission_target) = ordered.unwrap_or((None, None));
        let manual = command.single().ok().and_then(crate::ship::TargetSlot::ordered).map(|o| o.0);
        next.reticle = reticle_target(fleets.direct(), manual, pilot, mission_target).filter(|&e| {
            let shown = match ships.get(e) {
                Ok((_, h, ms)) => !h.destroyed && ms.is_none_or(|m| m.visible),
                Err(_) => animals.get(e).is_ok_and(|a| a.visible),
            };
            shown && sensors.player_sees(e, mission.as_deref())
        });
    }
    if *marks != next {
        *marks = next;
    }
}

/// Root of the view's HUD tree: hidden during cinematics, and clicks only count while it shows.
#[derive(Component)]
pub struct HudRoot;
/// Every node that takes the mouse: clicks on it must not reach the world.
#[derive(Component)]
pub struct Solid;
/// Not retail: the game-speed control, present only when the overlay has `data/hud/speed.json` (`{"max": 16}`), which the bundled
/// `game-speed` mod ships (on by default). `max` is the fastest speed the buttons reach (default `MAX_GAME_SPEED`, at least 1).
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub struct SpeedControl {
    pub max: f32,
}
impl SpeedControl {
    pub fn load(root: &std::path::Path) -> Option<Self> {
        let v = crate::data::try_json(&root.join("data/hud/speed.json"))?;
        Some(SpeedControl { max: v["max"].as_f64().map_or(crate::MAX_GAME_SPEED, |m| m as f32).max(1.0) })
    }
}
/// Game-speed buttons (with `SpeedControl`); each click halves / doubles the speed between 1x and its `max`.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub enum SpeedButton {
    Slower,
    Faster,
}
/// Text that shows the game speed ("2x", or "Paused").
#[derive(Component)]
pub struct SpeedLabel;
/// Presentations tag either a normalized retail dial or discrete engine-order tabs.
#[derive(Component)]
pub struct EngineDial;
#[derive(Component)]
pub struct EngineTab(pub crate::EngineOrder);
/// Weapon panel container: shown or hidden by the weapon-bar toggle (button or key).
#[derive(Component)]
pub struct WeaponPanel;
#[derive(Component)]
pub struct ToggleButton;
#[derive(Component)]
pub struct PanicButton;
/// Bottom-bar movement-type button (`MainBar_BeginnerMovementTypeButton`): basic (click = go) or advanced (plot, then accept).
#[derive(Component)]
pub struct MovementButton;
/// Bottom-bar PAUSE button (`MAINBAR_IS_NOT_PAUSED_BUTTON`): a click toggles the simulation pause.
#[derive(Component)]
pub struct PauseButton;
/// The command ship's portrait selects/refocuses it without changing command ownership.
#[derive(Component)]
pub struct ShipIconButton;

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum FleetButton {
    Ships,
    Commands,
    Slot(usize),
    Formation(crate::ship::Formation),
    Attack,
    CeaseFire,
    /// The main bar's speech-bubble button: toggles the Chat History panel.
    Chat,
}
/// One line of the Chat History: its text and RGBA colour.
#[derive(Clone, Debug, PartialEq)]
pub struct ChatLine {
    pub text: String,
    pub colour: [f32; 4],
}
/// Text colour of a speech line whose event names none (the subtitle's gold).
pub const CHAT_DEFAULT_COLOUR: [f32; 4] = [1.0, 0.65, 0.1, 1.0];
/// Chat History: every text line of the battle so far, in the order the lines started playing, and whether the panel is open.
/// Lines are never dropped during a battle; the log starts empty with each battle (`scenario::reset_fleets`).
#[derive(Resource, Default)]
pub struct ChatLog {
    pub lines: Vec<ChatLine>,
    pub open: bool,
}
impl ChatLog {
    pub fn push(&mut self, text: impl Into<String>, colour: Option<[f32; 4]>) {
        self.lines.push(ChatLine { text: text.into(), colour: colour.unwrap_or(CHAT_DEFAULT_COLOUR) });
    }
    /// The speech-bubble button: closes the panel when open, else opens it and closes the fleet-commands bar.
    pub fn toggle(&mut self, fleets: &mut crate::ship::Fleets) {
        self.open = !self.open;
        if self.open {
            fleets.commands_open = false;
        }
    }
}
/// Chat History panel height in reference px (measured on the original at 1440x1080: ~440 x 170 px) and the margin above and below
/// its list; the scroll thumb is `CHAT_THUMB_H` tall and runs the list's height.
pub const CHAT_SIZE: [f32; 2] = [391.0, 151.0];
pub const CHAT_LIST_MARGIN: f32 = 14.0;
pub const CHAT_THUMB_H: f32 = 20.0;
/// Reference px one mouse-wheel notch scrolls the chat list.
pub const CHAT_WHEEL_STEP: f32 = 24.0;
/// New scroll offset (px from the top) after a wheel turn of `wheel` notches (up is positive) at UI scale `scale`, kept within what the
/// list can scroll (`reach` = content - view, px). The list is built scrolled "to the end" with a huge offset, so the clamp is what
/// makes the first notch up move it.
pub fn chat_scroll_after_wheel(offset: f32, wheel: f32, scale: f32, reach: f32) -> f32 {
    (offset.min(reach) - wheel * CHAT_WHEEL_STEP * scale).clamp(0.0, reach.max(0.0))
}
/// Where the scroll thumb's top sits, in reference px from the panel's top: the list's offset as a share of what it can scroll
/// (`content` - `view`, both in px); a list that fits has the thumb at the bottom.
pub fn chat_thumb_top(offset: f32, view: f32, content: f32) -> f32 {
    let (reach, track) = ((content - view).max(0.0), CHAT_SIZE[1] - 2.0 * CHAT_LIST_MARGIN - CHAT_THUMB_H);
    CHAT_LIST_MARGIN + track * if reach > 0.0 { (offset / reach).clamp(0.0, 1.0) } else { 1.0 }
}
/// Moves the lines of mission and skirmish speech that just started into the log.
fn chat_feed(mut script: crate::rules::active::ActiveScriptMut, mut log: ResMut<ChatLog>) {
    if let Some(script) = script.script_mut().filter(|s| !s.started.is_empty()) {
        for (text, colour) in std::mem::take(&mut script.started) {
            log.push(text, colour);
        }
    }
}
#[derive(Component)]
pub struct FleetWindow(pub bool); // true: ship bar, false: commands
/// Name plate of the selected ship, shown while it is a secondary (not the command ship).
#[derive(Component)]
pub struct SelectedName;
#[derive(Clone, Default)]
pub struct FleetRow {
    pub entity: Option<Entity>,
    pub icon: String,
    pub name: String,
    pub health: f32,
    pub status: &'static str,
    pub selected: bool,
}
#[derive(Resource, Default)]
pub struct FleetHud {
    pub rows: Vec<FleetRow>,
    pub name: String,
    pub shown: bool,
    pub ships: bool,
    pub commands: bool,
    pub direct: bool,
    pub active: Option<crate::ship::Formation>,
    pub cease_fire: bool,
}

pub fn all_held(f: &crate::ship::Fleet, pilots: &Query<&crate::ship::FleetPilot>) -> bool {
    f.members.iter().skip(1).all(|e| pilots.get(*e).is_ok_and(|p| p.hold))
}

fn fleet_model(
    fleets: Res<crate::ship::Fleets>,
    mut model: ResMut<FleetHud>,
    lib: Res<Library>,
    mission: Option<Res<crate::campaign::Mission>>,
    ships: Query<(&Ship, &combat::Health, Option<&crate::ShipName>, Option<&crate::campaign::MissionShip>, Option<&crate::docking::Docker>, Option<&crate::boarding::Boarding>)>,
    pilots: Query<&crate::ship::FleetPilot>,
    maps: Query<(), With<crate::starmap::Starmap>>,
    fights: Query<&crate::boarding::Boarding>,
) {
    let row = |e: Entity| -> FleetRow {
        let Ok((s, h, name, ms, dock, board)) = ships.get(e) else { return FleetRow::default() };
        let c = &lib.classes[s.class];
        let total = c.core_hp + c.sections.iter().map(|s| s.hp).sum::<f32>();
        FleetRow {
            entity: Some(e),
            icon: c.icon("ShipBar"),
            name: name.map(|n| n.0.clone()).or_else(|| ms.map(|m| m.name.clone())).unwrap_or_else(|| c.name.clone()),
            health: ((h.core.max(0.0) + h.sections.iter().map(|hp| hp.max(0.0)).sum::<f32>()) / total.max(1.0)).clamp(0.0, 1.0),
            status: if dock.is_some() {
                "Docking"
            } else if board.is_some_and(|f| f.live()) || fights.iter().any(|f| f.live() && f.target() == e) {
                "Boarding"
            } else {
                ""
            },
            selected: fleets.selected == Some(e),
        }
    };
    model.rows = fleets.roster().iter().skip(1).take(10).copied().map(row).collect();
    model.name = fleets.selected.map(row).map_or_else(String::new, |r| r.name);
    model.shown = maps.is_empty() && !cinematic(mission.as_deref()) && !crate::rules::locked(mission.as_deref()) && fleets.primary().is_some();
    model.ships = fleets.bar_open && model.shown;
    model.commands = fleets.commands_open && model.shown;
    model.direct = fleets.direct();
    let f = fleets.groups.get(&fleets.local);
    model.active = f.filter(|f| all_held(f, &pilots)).map(|f| f.formation);
    model.cease_fire = f.is_some_and(|f| f.cease_fire);
}

#[allow(deprecated)] // Interaction, see pointer.rs
fn fleet_input(
    mouse: Res<ButtonInput<MouseButton>>,
    model: Res<FleetHud>,
    mut fleets: ResMut<crate::ship::Fleets>,
    mut orbit: ResMut<crate::Orbit>,
    buttons: Query<(Entity, &FleetButton, &Interaction)>,
    mut latch: Local<Option<(Entity, FleetButton, Option<Entity>)>>,
    mut orders: crate::commands::Orders,
    mut sfx: MessageWriter<crate::audio::Sfx>,
    mut menu: ResMut<crate::options_menu::BarMenuRequest>,
    focus: UiFocus,
    mut chat: ResMut<ChatLog>,
) {
    if !model.shown {
        *latch = None;
        return;
    }
    let hovered = buttons
        .iter()
        .find(|(_, b, i)| {
            crate::pointer::over(i)
                && match b {
                    FleetButton::Slot(n) => model.ships && model.rows.get(*n).is_some(),
                    FleetButton::Formation(_) | FleetButton::Attack | FleetButton::CeaseFire => model.commands,
                    _ => true,
                }
        })
        .map(|(e, b, _)| (e, *b));
    if mouse.just_released(MouseButton::Right)
        && let Some((_, FleetButton::Slot(n))) = hovered
    {
        menu.0 = model.rows.get(n).and_then(|r| r.entity);
    }
    if focus.blocks_fleet_bar() {
        *latch = None;
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        *latch = hovered.map(|(widget, b)| (widget, b, if let FleetButton::Slot(n) = b { model.rows.get(n).and_then(|r| r.entity) } else { None }));
    }
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    let Some((_, button, entity)) = latch.take().filter(|p| Some((p.0, p.1)) == hovered) else {
        return;
    };
    match button {
        FleetButton::Ships => {
            fleets.bar_open = !fleets.bar_open;
            sfx.write(crate::audio::Sfx::ui(if fleets.bar_open { "HUD_ShipBar_Open" } else { "HUD_ShipBar_Close" }));
        }
        FleetButton::Commands => {
            fleets.commands_open = !fleets.commands_open;
            // The two bars share the strip above the main bar: opening one closes the other.
            if fleets.commands_open {
                chat.open = false;
            }
            sfx.write(crate::audio::Sfx::ui(if fleets.commands_open { "HUD_FormationBar_Open" } else { "HUD_FormationBar_Close" }));
        }
        FleetButton::Chat => chat.toggle(&mut fleets),
        FleetButton::Slot(n) => {
            if model.rows.get(n).and_then(|r| r.entity) == entity
                && let Some(e) = entity
                && fleets.select(e, &mut orbit)
            {
                sfx.write(crate::audio::Sfx::ui("HUD_SelectShip_Secondary"));
            }
        }
        FleetButton::Formation(formation) => {
            orders.send(crate::commands::Kind::Formation { formation, hold: model.active != Some(formation) });
            // PROVISIONAL: native formation-click pool is unmapped; use the available fleet selection sound.
            sfx.write(crate::audio::Sfx::ui("HUD_ShipBar_Select"));
        }
        FleetButton::Attack => orders.send(crate::commands::Kind::FleetAttack),
        FleetButton::CeaseFire => orders.send(crate::commands::Kind::CeaseFire),
    }
}

/// Image showing the primary ship's docking / boarding animation (hidden otherwise).
#[derive(Component)]
pub struct DockingStatus;
/// Clickable gun-bank icon: an indirect bank's click arms it.
#[derive(Component)]
pub struct GunButton(pub usize);
/// The radar disk as a button: clicking it opens the star map (`starmap.rs`).
#[derive(Component)]
pub struct RadarButton;
/// Radar blip `i` of a hostile (`false`) or friendly (`true`) ship, positioned by `update_contacts` inside the view's radar node.
#[derive(Component)]
pub struct Contact(pub usize, pub bool);
/// The view's radar disk in its own reference px (inserted by the view's builder).
#[derive(Resource)]
pub struct Radar {
    pub centre: Vec2,
    pub radius: f32,
    pub contact_size: Vec2,
}
#[derive(Clone, Copy)]
pub enum Gauge {
    Health,
    Combat,
    Sail,
    /// Weapon-panel recharge bar of a gun bank.
    Recharge(usize),
}
/// Face sprite (`TalkingHead_Character_*`) of the campaign line being spoken outside cutscenes; written by `rules/systems.rs::ui`.
#[derive(Resource, Default)]
pub struct Talk(pub Option<String>);
/// A crew voice line's audio entity (helm acknowledgement or callout) with its speaker's face (`crew::voice_line`): the talking
/// head shows it while the line plays.
#[derive(Component)]
pub struct CrewSpeech(pub String);
/// Bottom-right talking-head device (retail `TalkingHead_*`): it opens while a campaign voice line or a crew line is spoken.
#[derive(Resource, Default)]
pub struct HeadState {
    /// Open progress 0..=1.
    pub open: f32,
    pub speaking: bool,
    /// Face sprite of the last speaker (kept while the device closes).
    pub speaker: Option<String>,
}

pub fn plugin(app: &mut App) {
    if let Some(speed) = SpeedControl::load(crate::assets_dir()) {
        app.insert_resource(speed);
    }
    app.insert_resource(Toggles { weapons: true, names: true })
        .init_resource::<UiClick>()
        .init_resource::<Talk>()
        .init_resource::<HeadState>()
        .init_resource::<FleetHud>()
        .init_resource::<WorldMarks>()
        .init_resource::<ChatLog>()
        .add_systems(Update, chat_feed.run_if(in_state(crate::menu::Screen::Battle)))
        .configure_sets(Update, HudSet.after(crate::sprites::choose_skin).run_if(in_state(crate::menu::Screen::Battle)))
        .add_systems(
            PreUpdate,
            (engine_input, hud_input, fleet_input, crate::starmap::block_world)
                .chain()
                .after(bevy::ui::UiSystems::Focus)
                .run_if(in_state(crate::menu::Screen::Battle))
                .run_if(crate::menu::overlay_closed),
        )
        .add_systems(PreUpdate, name_toggle)
        .add_systems(Update, (fleet_model, update_visibility, update_contacts, update_docking_status, talking_head, pause_button, speed_button).chain().in_set(HudSet))
        .add_systems(Update, decide_world_marks.in_set(HudSet).run_if(resource_exists::<combat::ArmedBank>));
}

/// `N` toggles the ship-name banners.
fn name_toggle(keys: Res<ButtonInput<KeyCode>>, mut toggles: ResMut<Toggles>) {
    if keys.just_pressed(KeyCode::KeyN) {
        toggles.names = !toggles.names;
    }
}

/// Reference pixels to whole physical pixels: at a fractional scale (1.125 at 1080p) unsnapped edges blur and butted parts show hairlines.
pub fn snap(v: f32, s: f32) -> f32 {
    (v * s).round()
}

/// A span of `size` starting `offset` inside a box `to` wide, as whole physical pixels `(before, size, after)` that sum to `snap(to)`.
pub fn snap_span(offset: f32, size: f32, to: f32, s: f32) -> (f32, f32, f32) {
    let (a, b, end) = (snap(offset, s), snap(offset + size, s), snap(to, s));
    (a, b - a, end - b)
}

pub fn px(v: f32, s: f32) -> Val {
    Val::Px(snap(v, s))
}

fn update_visibility(
    mut root: Query<&mut Visibility, (With<HudRoot>, Without<WeaponPanel>)>,
    mut weapons: Query<&mut Visibility, With<WeaponPanel>>,
    toggles: Res<Toggles>,
    mission: Option<Res<crate::campaign::Mission>>,
    fleet: Option<Res<FleetHud>>,
) {
    for mut v in &mut root {
        v.set_if_neq(if cinematic(mission.as_deref()) || crate::rules::console::hud_hidden() || fleet.as_ref().is_some_and(|f| !f.shown) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
    for mut v in &mut weapons {
        v.set_if_neq(if toggles.weapons && fleet.as_ref().is_none_or(|f| f.direct) { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// The primary ship's status icon: the retail docking animation while it docks, the boarding animation while it boards or is boarded.
/// PROVISIONAL: the boarding icon shares the docking icon's slot and 8 fps cadence.
fn update_docking_status(
    time: Res<Time>,
    hud: Res<HudData>,
    assets: Res<AssetServer>,
    player: Query<(Entity, Option<&crate::docking::Docker>, Option<&crate::boarding::Boarding>), With<PlayerShip>>,
    fights: Query<&crate::boarding::Boarding>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    mut frames: Local<Option<(&'static str, [Vec<Sprite>; 2])>>,
    mut icons: Query<(&mut ImageNode, &mut Node, &mut Visibility), With<DockingStatus>>,
) {
    if frames.as_ref().is_none_or(|f| f.0 != hud.skin) {
        let load = |kind: &str| (0..6).filter_map(|i| crate::sprites::sprite(&assets, &hud, &format!("ShipStatus_Icon_{kind}_Frame{i}"))).collect();
        *frames = Some((hud.skin, [load("Docking"), load("Boarding")]));
    }
    let [docking_art, boarding_art] = &frames.as_ref().unwrap().1;
    if docking_art.is_empty() {
        return;
    }
    let docking = player.iter().any(|p| p.1.is_some_and(|d| d.shows_status()));
    let boarding = player.iter().any(|p| p.2.is_some_and(|f| f.live()) || fights.iter().any(|f| f.live() && f.target() == p.0));
    let set = if docking || boarding_art.is_empty() || !boarding { docking_art } else { boarding_art };
    let frame = &set[(time.elapsed_secs() * STATUS_FPS) as usize % set.len()];
    for (mut img, mut node, mut vis) in &mut icons {
        frame.show(&mut img, &mut node, hud_scale(&window));
        vis.set_if_neq(if docking || boarding { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// Opens the talking head on a campaign voice line (outside cutscenes) or a crew line and closes it when the speech ends.
fn talking_head(
    time: Res<Time>,
    talk: Res<Talk>,
    crew: Query<&CrewSpeech>,
    script: crate::rules::active::ActiveScript,
    cfg: Res<crate::Config>,
    mut sfx: MessageWriter<crate::audio::Sfx>,
    mut head: ResMut<HeadState>,
) {
    let line = talk.0.clone().filter(|_| script.is_mission());
    let talk_smoke = (cfg.script.as_deref() == Some("talk")).then(|| "TalkingHead_Character_1stMate".to_string());
    let speaker = line.or_else(|| crew.iter().next().map(|c| c.0.clone())).or(talk_smoke);
    let speaking = speaker.is_some();
    if speaker.is_some() {
        head.speaker = speaker;
    }
    if speaking != head.speaking {
        head.speaking = speaking;
        sfx.write(crate::audio::Sfx::ui(if speaking { "HUD_TalkingHead_Iris_Open" } else { "HUD_TalkingHead_Iris_Close" }));
    }
    let step = HEAD_SPEED * time.delta_secs();
    head.open = if speaking { (head.open + step).min(1.0) } else { (head.open - step).max(0.0) };
}

fn update_contacts(
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    radar: Option<Res<Radar>>,
    mission: Option<Res<crate::campaign::Mission>>,
    player: Single<&Transform, With<PlayerShip>>,
    others: Query<(Entity, &Ship, &Health, &Transform, Option<&crate::campaign::MissionShip>), Without<PlayerShip>>,
    (sensors, table): (Res<crate::environment::Sensors>, crate::alliance::Hostility),
    mut q: Query<(&Contact, &mut Node, &mut Visibility)>,
) {
    let Some(radar) = radar else { return };
    let s = hud_scale(&window);
    // 'Set Radar Active State' FALSE blanks the disk.
    let on = mission.as_deref().is_none_or(|m| m.script.radar);
    // Living ships within range relative to the player's heading (forward = up); retail shows friendly ships too and none past the rim.
    let inv = player.rotation.inverse();
    let contacts = |friendly: bool| -> Vec<Vec2> {
        others
            .iter()
            .filter(|_| on)
            .filter(|(e, sh, h, _, ms)| table.with_human(sh.side) == friendly && !h.destroyed && ms.is_none_or(|m| m.visible) && sensors.player_sees(*e, mission.as_deref()))
            .map(|(_, _, _, tf, _)| {
                let l = inv * (tf.translation - player.translation);
                Vec2::new(l.x, -l.z) / RADAR_RANGE
            })
            .filter(|v| v.length() <= 1.0)
            .collect()
    };
    let contacts = [contacts(false), contacts(true)];
    for (Contact(i, friendly), mut node, mut vis) in &mut q {
        let Some(v) = contacts[usize::from(*friendly)].get(*i) else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        vis.set_if_neq(Visibility::Inherited);
        let size = radar.contact_size;
        let p = radar.centre + Vec2::new(v.x, -v.y) * radar.radius - size / 2.0;
        node.left = px(p.x, s);
        node.top = px(p.y, s);
    }
}

/// Fill fraction 0..=1 of a status gauge, whatever draws it.
pub fn gauge_value(gauge: Gauge, lib: &Library, ship: &Ship, health: &Health, crew: &crate::boarding::Boardable, guns: &GunStates) -> f32 {
    let class = &lib.classes[ship.class];
    match gauge {
        Gauge::Health => health.core / class.core_hp,
        // 'Primary Ship Combat Strength' (tooltip) is the original's Combat Strength: the crew's defending strength left (docs/re/boarding.md).
        // PROVISIONAL [?]: the retail gauge's source is inferred from the name, not traced.
        Gauge::Combat => crew.strength.defend as f32 / crew.strength.full.max(1) as f32,
        Gauge::Sail => combat::speed_factor(class, health) * ship.sail_energy,
        Gauge::Recharge(bank) => combat::bank_recharge(lib, guns, class, health, bank),
    }
    .clamp(0.0, 1.0)
}

#[derive(bevy::ecs::system::SystemParam)]
pub struct HudFocus<'w, 's> {
    pub orbit: ResMut<'w, crate::Orbit>,
    pub fleets: Option<ResMut<'w, crate::ship::Fleets>>,
    menu: Option<Res<'w, crate::options_menu::OptionsMenu>>,
    sfx: Option<ResMut<'w, Messages<crate::audio::Sfx>>>,
    plot: Option<ResMut<'w, crate::course::Plot>>,
    movement: Query<'w, 's, &'static crate::pointer::Raw, With<MovementButton>>,
    crew: Option<Res<'w, crate::crew_arms::ScreenModel>>,
}

/// Clicks on the HUD: weapon-panel toggle (also W), panic, the ship icon (camera back to the player's ship) and the gun icons.
/// `UiClick` tells the world to ignore clicks that land on the HUD.
#[allow(deprecated)] // Interaction, see pointer.rs
pub fn hud_input(
    keys: Res<ButtonInput<KeyCode>>,
    binds: Res<crate::bindings::Bindings>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut toggles: ResMut<Toggles>,
    mut ui: ResMut<UiClick>,
    ship: Single<(Entity, &Ship, Option<&GunStates>), With<PlayerShip>>,
    solid: Query<&Interaction, With<Solid>>,
    toggle: Query<&Interaction, With<ToggleButton>>,
    panic: Query<&Interaction, With<PanicButton>>,
    ship_icon: Query<&Interaction, With<ShipIconButton>>,
    mut focus: HudFocus,
    guns: Query<(&GunButton, &Interaction)>,
    lib: Res<Library>,
    mut armed: ResMut<combat::ArmedBank>,
    visible: Query<&Visibility, With<HudRoot>>,
    mut orders: crate::commands::Orders,
) {
    if focus.crew.as_ref().is_some_and(|m| m.open) {
        ui.0 = true;
        return;
    }
    let shown = hud_shown(&visible);
    ui.0 = shown && solid.iter().any(crate::pointer::over);
    if focus.menu.as_ref().is_some_and(|m| m.is_open()) {
        return;
    }
    let click = shown && mouse.just_pressed(MouseButton::Left);
    let mut flip = shown && binds.just_pressed(&keys, Act::WeaponBar);
    if click && toggle.iter().any(crate::pointer::over) {
        flip = true;
    }
    if ((click && panic.iter().any(crate::pointer::over)) || (shown && binds.just_pressed(&keys, Act::PanicFire))) && focus.fleets.as_ref().is_none_or(|f| f.direct()) {
        orders.send(crate::commands::Kind::PanicFire { ship: ship.0 });
    }
    if click && ship_icon.iter().any(crate::pointer::over) {
        if let Some(f) = focus.fleets.as_mut() {
            if let Some(e) = f.primary() {
                f.select(e, &mut focus.orbit);
                if let Some(sfx) = focus.sfx.as_mut() {
                    sfx.write(crate::audio::Sfx::ui("HUD_SelectShip_Primary"));
                }
            }
        } else {
            focus.orbit.focus = None;
        }
    }
    if click
        && focus.movement.iter().any(crate::pointer::over)
        && let Some(plot) = focus.plot.as_mut()
    {
        plot.advanced = !plot.advanced;
        plot.pending.clear();
        if let Some(sfx) = focus.sfx.as_mut() {
            sfx.write(crate::audio::Sfx::ui(if plot.advanced { "HUD_Movement_Advanced" } else { "HUD_Movement_Basic" }));
        }
    }
    if click && focus.fleets.as_ref().is_none_or(|f| f.direct()) {
        // Clicking an indirect bank's icon arms it (again to disarm); the next map click is its shot. Arming is the player's own
        // input state; dropping a shot not fired yet is an order.
        let (me, ship, equipped) = *ship;
        let class = &lib.classes[ship.class];
        for (GunButton(bank), _) in guns.iter().filter(|(_, i)| crate::pointer::over(i)) {
            let indirect = class.mounts.iter().enumerate().any(|(i, m)| m.bank == *bank && combat::is_indirect(&lib, equipped.map_or(m.gun.as_str(), |g| g.gun(i))));
            armed.0 = if indirect && armed.0 != Some(*bank) { Some(*bank) } else { None };
            orders.send(crate::commands::Kind::Lob { ship: me, shot: None });
        }
    }
    if flip {
        toggles.weapons = !toggles.weapons;
    }
}

/// PROVISIONAL: retail dial segment geometry uses the existing Navy 86x101 coordinates.
pub fn speed_order_at(p: Vec2) -> crate::EngineOrder {
    let i = [Vec2::new(54.0, 71.0), Vec2::new(44.0, 57.0), Vec2::new(31.0, 44.0), Vec2::new(14.0, 34.0)]
        .iter()
        .enumerate()
        .min_by(|a, b| p.distance_squared(*a.1).total_cmp(&b.1.distance_squared(p)))
        .unwrap()
        .0;
    crate::EngineOrder::from_index(i).unwrap_or_default()
}

#[allow(deprecated)] // Interaction, see pointer.rs
pub fn engine_input(
    mouse: Res<ButtonInput<MouseButton>>,
    ship: Single<Entity, With<PlayerShip>>,
    mut orders: crate::commands::Orders,
    dial: Query<(&Interaction, &bevy::ui::RelativeCursorPosition), With<EngineDial>>,
    tabs: Query<(&EngineTab, &Interaction)>,
    visible: Query<&Visibility, With<HudRoot>>,
    fleets: Option<Res<crate::ship::Fleets>>,
    focus: UiFocus,
) {
    if focus.blocks_engine_dial(fleets.as_deref()) || !(hud_shown(&visible) && mouse.just_pressed(MouseButton::Left)) {
        return;
    }
    let order = tabs.iter().find(|(_, i)| crate::pointer::over(i)).map(|(o, _)| o.0).or_else(|| {
        dial.iter().find(|(i, _)| crate::pointer::over(i)).and_then(|(_, rel)| rel.normalized).map(|p| {
            let p = p + Vec2::splat(0.5);
            speed_order_at(Vec2::new(p.x * 86.0, p.y * 101.0))
        })
    });
    if let Some(order) = order {
        orders.send(crate::commands::Kind::Engine { ship: *ship, engine: crate::commands::Engine::To(order) });
    }
}

/// The HUD exists and is not hidden (cinematics hide it).
pub fn hud_shown(visible: &Query<&Visibility, With<HudRoot>>) -> bool {
    !visible.is_empty() && visible.iter().all(|v| *v != Visibility::Hidden)
}

/// One owner's claim on the pause of virtual time: it remembers whether it was the one that paused, so releasing resumes only then
/// (a battle the player or another owner paused stays paused). Each owner (the in-game menu, the console, the star map pause) holds
/// its own claim; claims do not stack or know about each other.
#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub struct PauseClaim(bool);

impl PauseClaim {
    /// Pauses unless time is already paused; remembers that it did (an earlier claim of ours is kept when it was not).
    pub fn claim(&mut self, vtime: &mut Time<Virtual>) {
        if !vtime.is_paused() {
            vtime.pause();
            self.0 = true;
        }
    }
    /// Pauses and sets the claim to whether it was this call that paused, forgetting an earlier claim (the in-game menu's rule).
    pub fn claim_afresh(&mut self, vtime: &mut Time<Virtual>) {
        self.0 = !vtime.is_paused();
        vtime.pause();
    }
    /// Resumes if this claim paused; a no-op otherwise.
    pub fn release(&mut self, vtime: &mut Time<Virtual>) {
        if std::mem::take(&mut self.0) {
            vtime.unpause();
        }
    }
}

/// PAUSE button: a click toggles simulation pause (the Pause key does the same, `view.rs`).
#[allow(deprecated)] // Interaction, see pointer.rs
fn pause_button(mouse: Res<ButtonInput<MouseButton>>, mut vtime: ResMut<Time<Virtual>>, q: Query<&Interaction, With<PauseButton>>, mission: Option<Res<crate::campaign::Mission>>) {
    if !cinematic(mission.as_deref()) && mouse.just_pressed(MouseButton::Left) && q.iter().any(crate::pointer::over) {
        crate::toggle_pause(&mut vtime);
    }
}

/// Speed buttons step the game speed; labels follow it (and `--speed`).
#[allow(deprecated)] // Interaction, see pointer.rs
fn speed_button(
    mouse: Res<ButtonInput<MouseButton>>,
    mut vtime: ResMut<Time<Virtual>>,
    mut window: Single<&mut Window, With<bevy::window::PrimaryWindow>>,
    buttons: Query<(&SpeedButton, &Interaction)>,
    mut labels: Query<&mut Text, With<SpeedLabel>>,
    mission: Option<Res<crate::campaign::Mission>>,
    control: Option<Res<SpeedControl>>,
) {
    let clicked = buttons.iter().find(|(_, i)| crate::pointer::over(i)).map(|(b, _)| *b);
    if let (Some(b), Some(control)) = (clicked.filter(|_| !cinematic(mission.as_deref()) && mouse.just_pressed(MouseButton::Left)), control) {
        let speed = vtime.relative_speed() * if b == SpeedButton::Faster { 2.0 } else { 0.5 };
        crate::set_game_speed(&mut vtime, &mut window, speed.clamp(1.0, control.max));
    }
    let label = if vtime.is_paused() { "Paused".to_string() } else { format!("{}x", vtime.relative_speed()) };
    for mut text in &mut labels {
        if text.0 != label {
            text.0 = label.clone();
        }
    }
}

/// A minimal battle world with `hud_input` and a player ship; returns the app and the HUD root.
/// Test fixture shared with the views' tests (they live in the binary, where `cfg(test)` library code is invisible).
#[doc(hidden)]
pub fn input_app() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<UiClick>()
        .init_resource::<ChatLog>()
        .init_resource::<crate::bindings::Bindings>()
        .init_resource::<combat::ArmedBank>()
        .init_resource::<crate::commands::Outbox>()
        .init_resource::<crate::commands::Clock>()
        .insert_resource(Library::load(crate::assets_dir()))
        .insert_resource(Toggles { weapons: true, names: true })
        .insert_resource(crate::Orbit { yaw: 0.0, pitch: 0.5, dist: 50.0, focus: Some(Entity::PLACEHOLDER) })
        .add_systems(Update, hud_input);
    app.world_mut().spawn((PlayerShip, Ship { heading: 0.0, ..Ship::new(0, crate::Side::Player) }));
    let root = app.world_mut().spawn((HudRoot, Visibility::Inherited)).id();
    (app, root)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn the_chat_wheel_scrolls_by_notch_and_scale_and_the_thumb_follows() {
        assert_eq!(chat_scroll_after_wheel(100.0, 1.0, 1.0, 200.0), 76.0, "wheel up goes back");
        assert_eq!(chat_scroll_after_wheel(0.0, -2.0, 0.5, 200.0), 24.0, "wheel down, half scale");
        assert_eq!(chat_scroll_after_wheel(1.0e6, 1.0, 1.0, 200.0), 176.0, "from the built-at-the-end offset, one notch up moves");
        assert_eq!(chat_scroll_after_wheel(190.0, -1.0, 1.0, 200.0), 200.0, "down stops at the end");
        assert_eq!(chat_scroll_after_wheel(10.0, 1.0, 1.0, 200.0), 0.0, "up stops at the top");
        assert_eq!(chat_scroll_after_wheel(1.0e6, 1.0, 1.0, -20.0), 0.0, "a list that fits does not scroll");
        let (top, bottom) = (chat_thumb_top(0.0, 100.0, 300.0), chat_thumb_top(200.0, 100.0, 300.0));
        assert_eq!((top, bottom), (CHAT_LIST_MARGIN, CHAT_SIZE[1] - CHAT_LIST_MARGIN - CHAT_THUMB_H));
        assert_eq!(chat_thumb_top(500.0, 100.0, 300.0), bottom, "past the end clamps");
        assert_eq!(chat_thumb_top(0.0, 100.0, 80.0), bottom, "a list that fits rests at the bottom");
    }

    #[test]
    fn the_reticle_follows_the_aim_when_flying_else_the_order() {
        let (aim, pilot, mission) = (Some(Entity::from_raw_u32(1).unwrap()), Some(Entity::from_raw_u32(2).unwrap()), Some(Entity::from_raw_u32(3).unwrap()));
        assert_eq!(reticle_target(true, aim, pilot, mission), aim, "the flown ship marks the player's own target");
        assert_eq!(reticle_target(true, None, pilot, mission), None, "no aimed target, no reticle, whatever the fleet orders");
        assert_eq!(reticle_target(false, aim, pilot, mission), pilot, "a fleet pilot's order comes first");
        assert_eq!(reticle_target(false, aim, None, mission), mission, "then the mission ship's");
        assert_eq!(reticle_target(false, aim, None, None), None);
    }

    #[test]
    fn a_banner_needs_a_living_visible_seen_ship_and_the_hud_on() {
        assert!(banner_allowed(false, true, true, false));
        for (destroyed, visible, seen, hud_off) in [(true, true, true, false), (false, false, true, false), (false, true, false, false), (false, true, true, true)] {
            assert!(!banner_allowed(destroyed, visible, seen, hud_off), "{destroyed} {visible} {seen} {hud_off}");
        }
    }

    #[test]
    fn chat_history_keeps_lines_in_order_with_their_colour() {
        let mut log = ChatLog::default();
        log.push("one", Some([1.0, 0.0, 0.0, 1.0]));
        log.push("two", None);
        let lines: Vec<_> = log.lines.iter().map(|l| (l.text.as_str(), l.colour)).collect();
        assert_eq!(lines, [("one", [1.0, 0.0, 0.0, 1.0]), ("two", CHAT_DEFAULT_COLOUR)]);
    }

    #[test]
    fn opening_the_chat_history_closes_the_fleet_commands_bar_and_a_second_press_closes_it() {
        let (mut log, mut fleets) = (ChatLog::default(), crate::ship::Fleets { commands_open: true, ..default() });
        log.toggle(&mut fleets);
        assert!(log.open && !fleets.commands_open);
        fleets.commands_open = true;
        log.toggle(&mut fleets);
        assert!(!log.open && fleets.commands_open, "closing leaves the commands bar alone");
    }

    #[test]
    fn a_new_battle_starts_with_an_empty_closed_chat_history() {
        let mut world = World::new();
        let mut log = ChatLog::default();
        log.push("old", None);
        log.open = true;
        world.insert_resource(log);
        crate::scenario::reset_fleets(&mut world);
        let log = world.resource::<ChatLog>();
        assert!(log.lines.is_empty() && !log.open);
    }

    #[test]
    fn fractional_scales_snap_to_whole_pixels_and_butted_parts_stay_butted() {
        let s = 1080.0 / REF_H;
        assert_eq!(s, 1.125);
        let widths = [37.0_f32, 61.5, 23.0, 90.0];
        let (mut x, mut right) = (0.0, 0.0);
        for w in widths {
            let (before, size, _) = snap_span(x, w, 300.0, s);
            assert_eq!((before, size), (before.round(), size.round()));
            assert_eq!(before, right, "part at {x} leaves a gap or overlaps");
            right = before + size;
            x += w;
        }
        assert_eq!(right, snap(x, s));
        assert_eq!(snap_span(3.3, 10.7, 20.0, s).0 + snap_span(3.3, 10.7, 20.0, s).1 + snap_span(3.3, 10.7, 20.0, s).2, snap(20.0, s));
    }

    #[test]
    fn a_crew_line_opens_the_talking_head_on_its_speaker() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).init_resource::<Talk>().init_resource::<HeadState>().add_message::<crate::audio::Sfx>().add_systems(Update, talking_head);
        app.insert_resource(crate::Config { speed: 1.0, ..default() });
        let line = app.world_mut().spawn(CrewSpeech("TalkingHead_Character_Aquanogs_0".into())).id();
        app.update();
        let head = app.world().resource::<HeadState>();
        assert!(head.speaking && head.speaker.as_deref() == Some("TalkingHead_Character_Aquanogs_0"));
        app.world_mut().despawn(line);
        app.update();
        assert!(!app.world().resource::<HeadState>().speaking, "closes when the line ends");
    }

    #[test]
    #[allow(deprecated)] // Interaction, see pointer.rs
    fn ship_bar_release_latch_hidden_gate_and_context_menu_do_not_promote() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<FleetHud>()
            .init_resource::<ChatLog>()
            .init_resource::<crate::options_menu::BarMenuRequest>()
            .init_resource::<crate::commands::Outbox>()
            .insert_resource(crate::Orbit { yaw: 0.0, pitch: 0.0, dist: 60.0, focus: None })
            .add_message::<crate::audio::Sfx>()
            .add_systems(Update, fleet_input);
        let primary = app.world_mut().spawn(PlayerShip).id();
        let secondary = app.world_mut().spawn_empty().id();
        let mut fleets = crate::ship::Fleets { local: "Player".into(), selected: Some(primary), ..default() };
        fleets.groups.insert("Player".into(), crate::ship::Fleet { members: vec![primary, secondary], ..default() });
        app.insert_resource(fleets);
        app.world_mut().resource_mut::<FleetHud>().shown = true;
        app.world_mut().resource_mut::<FleetHud>().ships = true;
        app.world_mut().resource_mut::<FleetHud>().rows.push(FleetRow { entity: Some(secondary), ..default() });
        let slot = app.world_mut().spawn((FleetButton::Slot(0), Interaction::Hovered)).id();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        *app.world_mut().get_mut::<Interaction>(slot).unwrap() = Interaction::None;
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        app.update();
        assert_eq!(app.world().resource::<crate::ship::Fleets>().selected, Some(primary));
        *app.world_mut().get_mut::<Interaction>(slot).unwrap() = Interaction::Hovered;
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        app.update();
        assert_eq!(app.world().resource::<crate::ship::Fleets>().selected, Some(secondary));
        assert!(app.world().get::<PlayerShip>(primary).is_some());
        assert!(app.world().get::<PlayerShip>(secondary).is_none());
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Right);
        app.update();
        assert_eq!(app.world().resource::<crate::options_menu::BarMenuRequest>().0, Some(secondary));
        app.world_mut().resource_mut::<FleetHud>().shown = false;
        app.world_mut().resource_mut::<crate::options_menu::BarMenuRequest>().0 = None;
        app.update();
        assert_eq!(app.world().resource::<crate::options_menu::BarMenuRequest>().0, None);
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn ten_ship_bar_slots_match_keys_without_limiting_the_fleet_and_rebuild_after_loss() {
        use bevy::ecs::system::RunSystemOnce;
        let (mut w, ids) = crate::ship::movement_tests::fleet_world();
        let ship = w.get::<Ship>(ids[0]).unwrap().clone();
        for i in 4..14 {
            let health = Health::new(&w.resource::<Library>().classes[ship.class]);
            w.spawn((ship.clone(), health, Transform::default(), crate::ship::FleetMember("Player".into(), i)));
        }
        crate::ship::reconcile_fleets(&mut w);
        w.insert_resource(FleetHud::default());
        w.insert_resource(crate::bindings::Bindings::default());
        w.insert_resource(ButtonInput::<KeyCode>::default());
        w.insert_resource(Messages::<crate::audio::Sfx>::default());
        w.run_system_once(fleet_model).unwrap();
        assert_eq!(w.resource::<crate::ship::Fleets>().roster().len(), 14);
        assert_eq!(w.resource::<FleetHud>().rows.len(), 10);
        let tenth = w.resource::<FleetHud>().rows[9].entity.unwrap();
        assert!(!w.resource::<crate::ship::Fleets>().bar_open);
        w.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Backslash);
        w.run_system_once(crate::view::fleet_keys).unwrap();
        assert_eq!(w.resource::<crate::ship::Fleets>().selected, Some(tenth), "number keys work with the bar closed");
        assert!(w.get::<PlayerShip>(ids[0]).is_some());
        w.get_mut::<Health>(tenth).unwrap().destroyed = true;
        crate::ship::reconcile_fleets(&mut w);
        w.run_system_once(fleet_model).unwrap();
        assert_eq!(w.resource::<FleetHud>().rows.len(), 10);
        assert!(w.resource::<FleetHud>().rows.iter().all(|r| r.entity != Some(tenth)));
        assert_eq!(w.resource::<crate::ship::Fleets>().selected, Some(ids[0]));
    }

    #[test]
    fn yellow_formation_requires_every_secondary_and_empty_fleet_is_all_held() {
        let mut world = World::new();
        let p = world.spawn(crate::ship::FleetPilot::default()).id();
        let a = world.spawn(crate::ship::FleetPilot { hold: true, ..default() }).id();
        let b = world.spawn(crate::ship::FleetPilot { hold: false, ..default() }).id();
        let f = crate::ship::Fleet { members: vec![p, a, b], formation: crate::ship::Formation::Diamond, ..default() };
        let mut state = bevy::ecs::system::SystemState::<Query<&crate::ship::FleetPilot>>::new(&mut world);
        assert!(!all_held(&f, &state.get(&world).unwrap()));
        world.get_mut::<crate::ship::FleetPilot>(b).unwrap().hold = true;
        assert!(all_held(&f, &state.get(&world).unwrap()));
        assert!(all_held(&crate::ship::Fleet::default(), &state.get(&world).unwrap()));
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn cinematics_hide_hud_and_restore_player_toggles() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).insert_resource(Toggles { weapons: false, names: true }).add_systems(Update, update_visibility);
        let root = app.world_mut().spawn((HudRoot, Visibility::Inherited)).id();
        let weapons = app.world_mut().spawn((WeaponPanel, Visibility::Inherited)).id();
        app.update();
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Inherited);
        app.insert_resource(crate::campaign::load());
        app.update();
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Hidden, "opening briefing");
        {
            let mut mission = app.world_mut().resource_mut::<crate::campaign::Mission>();
            mission.opening = None;
            mission.script.nis = true;
        }
        app.update();
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Hidden, "in-game cutscene");
        app.world_mut().resource_mut::<crate::campaign::Mission>().script.nis = false;
        app.update();
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Inherited, "gameplay restored");
        assert_eq!(*app.world().get::<Visibility>(weapons).unwrap(), Visibility::Hidden, "player toggle preserved");
    }

    #[test]
    #[ignore = "needs imported assets"]
    #[allow(deprecated)] // Interaction, see pointer.rs
    fn engine_tabs_clear_throttle_only_for_direct_visible_controls() {
        let (mut app, root) = input_app();
        // The dial sends an order; outside a running battle clock (this world has none) `apply_tick` applies it at once.
        app.add_systems(Update, (engine_input.before(hud_input), crate::commands::apply_tick.after(engine_input)));
        let w = app.world_mut();
        let primary = w.query_filtered::<Entity, With<PlayerShip>>().single(w).unwrap();
        let secondary = w.spawn_empty().id();
        let mut f = crate::ship::Fleets { local: "Player".into(), selected: Some(secondary), ..default() };
        f.groups.insert("Player".into(), crate::ship::Fleet { members: vec![primary, secondary], ..default() });
        w.insert_resource(f);
        w.spawn((Solid, EngineTab(crate::EngineOrder::Full), Interaction::Pressed));
        w.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        w.get_mut::<Ship>(primary).unwrap().throttle = Some(0.4);
        app.update();
        assert_eq!(app.world().get::<Ship>(primary).unwrap().order, crate::EngineOrder::Stop);
        assert_eq!(app.world().get::<Ship>(primary).unwrap().throttle, Some(0.4));
        app.world_mut().resource_mut::<crate::ship::Fleets>().selected = Some(primary);
        *app.world_mut().get_mut::<Visibility>(root).unwrap() = Visibility::Hidden;
        app.update();
        assert_eq!(app.world().get::<Ship>(primary).unwrap().throttle, Some(0.4));
        *app.world_mut().get_mut::<Visibility>(root).unwrap() = Visibility::Inherited;
        app.update();
        assert_eq!(app.world().get::<Ship>(primary).unwrap().order, crate::EngineOrder::Full);
        assert_eq!(app.world().get::<Ship>(primary).unwrap().throttle, None);
    }

    #[test]
    #[ignore = "needs imported assets"]
    #[allow(deprecated)] // Interaction, see pointer.rs
    fn hud_clicks_do_not_leak_to_world_and_hidden_hud_ignores_input() {
        let (mut app, root) = input_app();
        app.world_mut().spawn((Solid, PanicButton, Interaction::Pressed));
        app.world_mut().spawn((Solid, ShipIconButton, Interaction::Pressed));
        // Panic fire is an order; outside a running battle clock `apply_tick` applies it at once.
        app.add_systems(Update, crate::commands::apply_tick.after(hud_input));
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        let panic = |app: &mut App| app.world_mut().query::<&combat::ManualFire>().single(app.world()).unwrap().panic;
        assert!(panic(&mut app));
        assert_eq!(app.world().resource::<crate::Orbit>().focus, None, "ship icon returns the camera to the player");
        assert!(app.world().resource::<UiClick>().0);
        app.world_mut().entity_mut(root).insert(Visibility::Hidden);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        app.update();
        assert!(!app.world().resource::<UiClick>().0);
        assert!(app.world().resource::<Toggles>().weapons);
        app.world_mut().query::<&mut combat::ManualFire>().single_mut(app.world_mut()).unwrap().panic = false;
        app.world_mut().entity_mut(root).insert(Visibility::Inherited);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyX);
        app.update();
        assert!(panic(&mut app));
    }

    #[test]
    #[allow(deprecated)] // Interaction, see pointer.rs
    fn speed_buttons_step_and_clamp_the_game_speed() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).init_resource::<ButtonInput<MouseButton>>().insert_resource(SpeedControl { max: 16.0 }).add_systems(Update, speed_button);
        app.world_mut().spawn((Window::default(), bevy::window::PrimaryWindow));
        let faster = app.world_mut().spawn((SpeedButton::Faster, Interaction::None)).id();
        let slower = app.world_mut().spawn((SpeedButton::Slower, Interaction::None)).id();
        let label = app.world_mut().spawn((SpeedLabel, Text::new(""))).id();
        let mut seen = vec![];
        for (button, other) in [(faster, slower); 5].into_iter().chain([(slower, faster); 6]) {
            app.world_mut().entity_mut(button).insert(Interaction::Pressed);
            app.world_mut().entity_mut(other).insert(Interaction::None);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            app.update();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            seen.push(app.world().resource::<Time<Virtual>>().relative_speed());
        }
        assert_eq!(seen, [2.0, 4.0, 8.0, 16.0, 16.0, 8.0, 4.0, 2.0, 1.0, 1.0, 1.0]);
        assert_eq!(app.world().get::<Text>(label).unwrap().0, "1x");
    }

    #[test]
    fn speed_control_comes_from_the_mod_file() {
        let dir = std::env::temp_dir().join(format!("procyon-speed-control-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("data/hud")).unwrap();
        assert_eq!(SpeedControl::load(&dir), None, "vanilla: no speed control");
        std::fs::write(dir.join("data/hud/speed.json"), r#"{"max": 4}"#).unwrap();
        assert_eq!(SpeedControl::load(&dir), Some(SpeedControl { max: 4.0 }));
        std::fs::write(dir.join("data/hud/speed.json"), "{}").unwrap();
        assert_eq!(SpeedControl::load(&dir), Some(SpeedControl { max: crate::MAX_GAME_SPEED }));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[allow(deprecated)] // Interaction, see pointer.rs
    fn pause_button_toggles_virtual_time_and_keeps_speed() {
        let mut app = App::new();
        let mut time = Time::<Virtual>::default();
        time.set_relative_speed(4.0);
        app.add_plugins(MinimalPlugins).init_resource::<ButtonInput<MouseButton>>().insert_resource(time).add_systems(Update, pause_button);
        app.world_mut().spawn((Interaction::Pressed, PauseButton));
        let mut seen = vec![];
        for _ in 0..3 {
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            app.update();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            seen.push(app.world().resource::<Time<Virtual>>().is_paused());
        }
        assert_eq!(seen, [true, false, true], "each click toggles the pause");
        assert_eq!(app.world().resource::<Time<Virtual>>().relative_speed(), 4.0, "pausing and resuming keep the speed setting");
    }

    /// A world with the real sensor snapshot and sight pass feeding `decide_world_marks` (no assets: one synthetic hull class).
    /// Returns the app and the entities: player ship, fleet ship (pilot targets `enemy`), enemy, script-hidden, destroyed, out of sight, animal.
    struct MarkWorld {
        app: App,
        player: Entity,
        fleet: Entity,
        enemy: Entity,
        hidden: Entity,
        destroyed: Entity,
        far: Entity,
        animal: Entity,
    }

    fn mark_world() -> MarkWorld {
        let class = crate::fixed_tests::synthetic_class();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(Library { classes: vec![class], guns: default(), bullets: default(), names: default(), clips: default() })
            .init_resource::<crate::environment::Sensors>()
            .init_resource::<combat::ArmedBank>()
            .init_resource::<crate::ship::Fleets>()
            .init_resource::<WorldMarks>()
            .add_systems(
                Update,
                (
                    crate::environment::refresh_sensors,
                    |mut s: ResMut<crate::environment::Sensors>, m: Option<Res<crate::campaign::Mission>>| s.advance_sightings(m.as_deref(), 0.0, 0.0),
                    decide_world_marks,
                )
                    .chain(),
            );
        let alive = || Health { sections: vec![], core: 100.0, destroyed: false };
        let at = |x: f32| Transform::from_xyz(x, 0.0, 0.0);
        let enemy_side = crate::Side::Enemy;
        let mission_ship = |visible: bool| crate::campaign::MissionShip { owner: "Enemy".into(), visible, ..default() };
        let world = app.world_mut();
        let player = world.spawn((Ship::new(0, crate::Side::Player), alive(), at(0.0), PlayerShip, crate::ship::FleetMember("Player".into(), 0))).id();
        let enemy = world.spawn((Ship::new(0, enemy_side), alive(), at(20.0), mission_ship(true))).id();
        let fleet = world
            .spawn((
                Ship::new(0, crate::Side::Player),
                alive(),
                at(10.0),
                crate::ship::FleetMember("Player".into(), 1),
                crate::ship::FleetPilot { target: Some(enemy), ..default() },
            ))
            .id();
        let hidden = world.spawn((Ship::new(0, enemy_side), alive(), at(25.0), mission_ship(false))).id();
        let destroyed = world.spawn((Ship::new(0, enemy_side), Health { destroyed: true, ..alive() }, at(30.0), mission_ship(true))).id();
        let far = world.spawn((Ship::new(0, enemy_side), alive(), at(5000.0), mission_ship(true))).id();
        let animal = world.spawn((crate::environment::SpaceAnimal::new("Whales", 5.0, Vec3::ZERO, 1, true), at(35.0))).id();
        let mut fleets = world.resource_mut::<crate::ship::Fleets>();
        fleets.local = "Player".into();
        fleets.groups.insert("Player".into(), crate::ship::Fleet { members: vec![player, fleet], formation: default(), cease_fire: false });
        fleets.selected = Some(player);
        MarkWorld { app, player, fleet, enemy, hidden, destroyed, far, animal }
    }

    fn marks(w: &mut MarkWorld, aimed: Option<Entity>) -> (Option<Entity>, Vec<Entity>) {
        w.app.world_mut().entity_mut(w.player).insert(crate::ship::TargetSlot::order(aimed, None));
        w.app.update();
        let m = w.app.world().resource::<WorldMarks>();
        let mut banners: Vec<_> = m.banners.iter().copied().collect();
        banners.sort();
        (m.reticle, banners)
    }

    #[test]
    fn banners_show_only_living_visible_seen_ships() {
        let mut w = mark_world();
        let (_, banners) = marks(&mut w, None);
        let mut expect = vec![w.player, w.fleet, w.enemy];
        expect.sort();
        assert_eq!(banners, expect, "not the script-hidden, destroyed or out-of-sight ships, and no animal");
        for e in [w.hidden, w.destroyed, w.far, w.animal] {
            assert!(!banners.contains(&e));
        }
    }

    #[test]
    fn the_reticle_stands_on_the_aimed_target_only_when_it_is_shown_and_seen() {
        let mut w = mark_world();
        let (enemy, animal) = (w.enemy, w.animal);
        assert_eq!(marks(&mut w, Some(enemy)).0, Some(enemy), "the flown ship marks its own target");
        assert_eq!(marks(&mut w, Some(animal)).0, Some(animal), "a space animal can be marked");
        assert_eq!(marks(&mut w, None).0, None);
        let bad = [w.hidden, w.destroyed, w.far];
        for hidden in bad {
            assert_eq!(marks(&mut w, Some(hidden)).0, None, "no reticle on a hidden, destroyed or unseen target");
        }
    }

    #[test]
    fn a_selected_fleet_ship_shows_its_ordered_target_not_the_aimed_one() {
        let mut w = mark_world();
        let (enemy, animal, far, fleet) = (w.enemy, w.animal, w.far, w.fleet);
        w.app.world_mut().resource_mut::<crate::ship::Fleets>().selected = Some(fleet);
        assert_eq!(marks(&mut w, Some(animal)).0, Some(enemy), "the fleet pilot's order wins over the player's aim");
        w.app.world_mut().get_mut::<crate::ship::FleetPilot>(fleet).unwrap().target = Some(far);
        assert_eq!(marks(&mut w, Some(enemy)).0, None, "an ordered target out of sight is not marked either");
    }

    #[test]
    fn a_cinematic_clears_every_mark() {
        let mut w = mark_world();
        let enemy = w.enemy;
        assert!(marks(&mut w, Some(enemy)).0.is_some(), "marks show before the cutscene");
        let mut script = crate::rules::Script::new(&serde_json::json!({"rules": []}), serde_json::json!({}));
        script.nis = true;
        w.app.insert_resource(crate::campaign::Mission { id: "t".into(), data: serde_json::json!({}), opening: None, script });
        assert_eq!(marks(&mut w, Some(enemy)), (None, vec![]), "an in-game cutscene hides banners and the reticle");
    }
    /// Pins what each input-blocking predicate looks at, one definition per call site (`UiFocus`): they differ on purpose or by
    /// accident, and merging them would change behaviour.
    #[test]
    fn ui_focus_predicates_each_look_at_their_own_inputs() {
        #[derive(Clone, Copy, Default, Debug)]
        struct In {
            over_hud: bool,
            swallowed: bool,
            menu_open: bool,
            crew_open: bool,
            map: bool,
            nis: bool,
            locked: bool,
            not_direct: bool,
        }
        // (world click, fleet keys, fleet bar, engine dial, hover)
        fn answers(i: In) -> [bool; 5] {
            use bevy::ecs::system::RunSystemOnce;
            let mut w = World::new();
            w.insert_resource(UiClick(i.over_hud));
            let menu = crate::options_menu::OptionsMenu::default();
            menu.swallow.store(i.swallowed, std::sync::atomic::Ordering::Relaxed);
            menu.set_open(i.menu_open);
            w.insert_resource(menu);
            w.insert_resource(crate::crew_arms::ScreenModel { open: i.crew_open, ..Default::default() });
            if i.map {
                w.spawn(crate::starmap::Starmap { opened: 0.0, half: 1.0, backdrop: None });
            }
            let mut script = crate::rules::Script::new(&serde_json::json!({"rules": []}), serde_json::json!({}));
            script.nis = i.nis;
            script.crew_screen = i.locked;
            if i.nis || i.locked {
                w.insert_resource(crate::campaign::Mission { id: "t".into(), data: serde_json::json!({}), opening: None, script });
            }
            let mut fleets = crate::ship::Fleets::default();
            if !i.not_direct {
                let e = w.spawn_empty().id();
                fleets.groups.entry(fleets.local.clone()).or_default().members.push(e);
                fleets.selected = Some(e);
            }
            w.run_system_once(move |f: UiFocus| [f.blocks_world_click(), f.blocks_fleet_keys(), f.blocks_fleet_bar(), f.blocks_engine_dial(Some(&fleets)), f.blocks_hover()])
                .unwrap()
        }
        let base = In::default();
        assert_eq!(answers(base), [false; 5]);
        assert_eq!(answers(In { over_hud: true, ..base }), [true, false, false, false, true], "a HUD panel under the cursor");
        assert_eq!(answers(In { swallowed: true, ..base }), [true, false, false, false, false], "the click that closed the options menu");
        assert_eq!(answers(In { menu_open: true, ..base }), [false, true, true, true, true], "the options menu is open");
        assert_eq!(answers(In { crew_open: true, ..base }), [false, false, false, true, false], "the crew screen: only the engine dial looks");
        assert_eq!(answers(In { map: true, ..base }), [false, true, false, false, false], "the star map: only the fleet keys look");
        assert_eq!(answers(In { nis: true, ..base }), [false, true, false, false, true], "a cutscene");
        assert_eq!(answers(In { locked: true, ..base }), [false, true, false, false, false], "a script lock without a cutscene: only the fleet keys look");
        assert_eq!(answers(In { not_direct: true, ..base }), [false, false, false, true, false], "a ship flown by its captain: only the engine dial looks");
    }

    #[test]
    fn pause_claims_resume_only_what_they_paused() {
        let mut t = Time::<Virtual>::default();
        // Already paused by someone else: claiming changes nothing and releasing does not resume.
        t.pause();
        let mut c = PauseClaim::default();
        c.claim(&mut t);
        c.release(&mut t);
        assert!(t.is_paused());
        // Running: the claim pauses and its release resumes; a second release is a no-op.
        t.unpause();
        c.claim(&mut t);
        assert!(t.is_paused());
        t.unpause();
        t.pause();
        c.release(&mut t);
        assert!(!t.is_paused());
        t.pause();
        c.release(&mut t);
        assert!(t.is_paused(), "released twice: the second does nothing");
        // `claim` keeps an earlier claim when time is already paused; `claim_afresh` forgets it (the in-game menu's rule).
        t.unpause();
        let (mut keep, mut fresh) = (PauseClaim::default(), PauseClaim::default());
        keep.claim(&mut t);
        fresh.claim(&mut t);
        fresh.claim_afresh(&mut t);
        keep.claim(&mut t);
        keep.release(&mut t);
        assert!(!t.is_paused(), "the kept claim still resumes");
        t.pause();
        fresh.release(&mut t);
        assert!(t.is_paused(), "the afresh claim saw it paused already and let it stay paused");
    }
}
