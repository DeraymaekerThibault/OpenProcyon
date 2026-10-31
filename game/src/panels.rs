//! In-game HUD contract shared by every presentation (`classic/panels.rs` draws it with retail sprites).
//!
//! A view builds its widgets under one `HudRoot` and tags them with the markers below (`PanicButton`, `GunButton`, `Contact`, ...);
//! this module gives them their behaviour: clicks, what the gauges read (`gauge_value`), radar contact placement (`Radar`), the
//! docking/boarding status icon, the talking-head state (`HeadState`), pause and the game-speed buttons. Views only draw.
#![allow(deprecated)] // shortcut: bevy 0.20 Interaction (deprecated for picking Hovered/Pressed); port with menu.rs when it is removed
use bevy::prelude::*;

use crate::bindings::Act;
use crate::combat::{self, GunStates, Health};
use crate::sprites::{HudData, inset, sprite_of};
use crate::{Library, PlayerShip, Ship};

/// Height of the reference screen the HUD art is scaled against (the reference video draws the 640x480 art at twice that height).
pub const REF_H: f32 = 960.0;
pub const MAX_CONTACTS: usize = 16;
/// PROVISIONAL: radar range in world units mapped to the disk's radius.
const RADAR_RANGE: f32 = 300.0;
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

/// Opening briefing or an in-game cutscene: the HUD hides and its controls do nothing.
pub fn cinematic(mission: Option<&crate::campaign::Mission>) -> bool {
    crate::briefing::active(mission) || crate::rules::nis(mission)
}

/// Root of the view's HUD tree: hidden during cinematics, and clicks only count while it shows.
#[derive(Component)]
pub struct HudRoot;
/// Every node that takes the mouse: clicks on it must not reach the world.
#[derive(Component)]
pub struct Solid;
/// Testing aid (not retail): game-speed buttons; each click halves / doubles the speed between 1x and `MAX_GAME_SPEED`.
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
pub struct EngineOrder(pub usize);
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

fn fleet_input(
    mouse: Res<ButtonInput<MouseButton>>,
    model: Res<FleetHud>,
    mut fleets: ResMut<crate::ship::Fleets>,
    mut orbit: ResMut<crate::Orbit>,
    buttons: Query<(Entity, &FleetButton, &Interaction)>,
    mut latch: Local<Option<(Entity, FleetButton, Option<Entity>)>>,
    mut commands: Commands,
    mut sfx: MessageWriter<crate::audio::Sfx>,
    mut menu: ResMut<crate::options_menu::BarMenuRequest>,
    options: Option<Res<crate::options_menu::OptionsMenu>>,
) {
    if !model.shown {
        *latch = None;
        return;
    }
    let hovered = buttons
        .iter()
        .find(|(_, b, i)| {
            **i != Interaction::None
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
    if options.as_ref().is_some_and(|o| o.is_open()) {
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
            sfx.write(crate::audio::Sfx::ui(if fleets.commands_open { "HUD_FormationBar_Open" } else { "HUD_FormationBar_Close" }));
        }
        FleetButton::Slot(n) => {
            if model.rows.get(n).and_then(|r| r.entity) == entity
                && let Some(e) = entity
                && fleets.select(e, &mut orbit)
            {
                sfx.write(crate::audio::Sfx::ui("HUD_SelectShip_Secondary"));
            }
        }
        FleetButton::Formation(kind) => {
            let hold = model.active != Some(kind);
            let owner = fleets.local.clone();
            commands.queue(move |w: &mut World| crate::ship::set_formation(w, &owner, Some(kind), hold));
            // PROVISIONAL: native formation-click pool is unmapped; use the available fleet selection sound.
            sfx.write(crate::audio::Sfx::ui("HUD_ShipBar_Select"));
        }
        FleetButton::Attack | FleetButton::CeaseFire => {
            let owner = fleets.local.clone();
            commands.queue(move |w: &mut World| crate::ship::fleet_help(w, &owner, button == FleetButton::Attack));
        }
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
/// Radar blip `i`, positioned by `update_contacts` inside the view's radar node.
#[derive(Component)]
pub struct Contact(pub usize);
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
    app.insert_resource(Toggles { weapons: true, names: true })
        .init_resource::<UiClick>()
        .init_resource::<Talk>()
        .init_resource::<HeadState>()
        .init_resource::<FleetHud>()
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
        .add_systems(Update, (fleet_model, update_visibility, update_contacts, update_docking_status, talking_head, pause_button, speed_button).chain().in_set(HudSet));
}

/// `N` toggles the ship-name banners.
fn name_toggle(keys: Res<ButtonInput<KeyCode>>, mut toggles: ResMut<Toggles>) {
    if keys.just_pressed(KeyCode::KeyN) {
        toggles.names = !toggles.names;
    }
}

pub fn px(v: f32, s: f32) -> Val {
    Val::Px(v * s)
}

/// (handle, source rect) of a hud.hdt sprite in the current faction's set.
pub fn frame(assets: &AssetServer, hud: &HudData, name: &str) -> Option<(Handle<Image>, Rect)> {
    sprite_of(hud, name).map(|(sheet, r)| (assets.load::<Image>(format!("hud/tex/{sheet}.png")), inset(&sheet, r)))
}

fn update_visibility(
    mut root: Query<&mut Visibility, (With<HudRoot>, Without<WeaponPanel>)>,
    mut weapons: Query<&mut Visibility, With<WeaponPanel>>,
    toggles: Res<Toggles>,
    mission: Option<Res<crate::campaign::Mission>>,
    fleet: Option<Res<FleetHud>>,
) {
    for mut v in &mut root {
        v.set_if_neq(if cinematic(mission.as_deref()) || fleet.as_ref().is_some_and(|f| !f.shown) { Visibility::Hidden } else { Visibility::Inherited });
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
    mut frames: Local<Option<(&'static str, [Vec<(Handle<Image>, Rect)>; 2])>>,
    mut icons: Query<(&mut ImageNode, &mut Visibility), With<DockingStatus>>,
) {
    if frames.as_ref().is_none_or(|f| f.0 != hud.skin) {
        let load = |kind: &str| (0..6).filter_map(|i| frame(&assets, &hud, &format!("ShipStatus_Icon_{kind}_Frame{i}"))).collect();
        *frames = Some((hud.skin, [load("Docking"), load("Boarding")]));
    }
    let [docking_art, boarding_art] = &frames.as_ref().unwrap().1;
    if docking_art.is_empty() {
        return;
    }
    let docking = player.iter().any(|p| p.1.is_some_and(|d| d.shows_status()));
    let boarding = player.iter().any(|p| p.2.is_some_and(|f| f.live()) || fights.iter().any(|f| f.live() && f.target() == p.0));
    let set = if docking || boarding_art.is_empty() || !boarding { docking_art } else { boarding_art };
    let (image, rect) = &set[(time.elapsed_secs() * STATUS_FPS) as usize % set.len()];
    for (mut img, mut vis) in &mut icons {
        if img.image != *image {
            img.image = image.clone();
        }
        if img.rect != Some(*rect) {
            img.rect = Some(*rect);
        }
        vis.set_if_neq(if docking || boarding { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// Opens the talking head on a campaign voice line (outside cutscenes) or a crew line and closes it when the speech ends.
fn talking_head(
    time: Res<Time>,
    talk: Res<Talk>,
    crew: Query<&CrewSpeech>,
    mission: Option<Res<crate::campaign::Mission>>,
    cfg: Res<crate::Config>,
    mut sfx: MessageWriter<crate::audio::Sfx>,
    mut head: ResMut<HeadState>,
) {
    let line = talk.0.clone().filter(|_| mission.is_some());
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
    sensors: Res<crate::environment::Sensors>,
    mut q: Query<(&Contact, &mut Node, &mut Visibility)>,
) {
    let Some(radar) = radar else { return };
    let s = window.height() / REF_H;
    // 'Set Radar Active State' FALSE blanks the disk.
    let on = mission.as_deref().is_none_or(|m| m.script.radar);
    // Living hostile ships relative to the player's heading (forward = up), clamped to the disk.
    let inv = player.rotation.inverse();
    let contacts: Vec<Vec2> = others
        .iter()
        .filter(|_| on)
        .filter(|(e, sh, h, _, ms)| sh.side != crate::Side::Player && !h.destroyed && ms.is_none_or(|m| m.visible) && sensors.player_sees(*e, mission.as_deref()))
        .map(|(_, _, _, tf, _)| {
            let l = inv * (tf.translation - player.translation);
            let v = Vec2::new(l.x, -l.z) / RADAR_RANGE;
            if v.length() > 1.0 { v.normalize() } else { v }
        })
        .collect();
    for (Contact(i), mut node, mut vis) in &mut q {
        let Some(v) = contacts.get(*i) else {
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
    movement: Query<'w, 's, &'static Interaction, With<MovementButton>>,
    crew: Option<Res<'w, crate::crew_arms::ScreenModel>>,
}

/// Clicks on the HUD: weapon-panel toggle (also W), panic, the ship icon (camera back to the player's ship) and the gun icons.
/// `UiClick` tells the world to ignore clicks that land on the HUD.
pub fn hud_input(
    keys: Res<ButtonInput<KeyCode>>,
    binds: Res<crate::bindings::Bindings>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut toggles: ResMut<Toggles>,
    mut ui: ResMut<UiClick>,
    ship: Single<(&Ship, Option<&GunStates>), With<PlayerShip>>,
    solid: Query<&Interaction, With<Solid>>,
    toggle: Query<&Interaction, With<ToggleButton>>,
    panic: Query<&Interaction, With<PanicButton>>,
    ship_icon: Query<&Interaction, With<ShipIconButton>>,
    mut focus: HudFocus,
    guns: Query<(&GunButton, &Interaction)>,
    lib: Res<Library>,
    mut target: ResMut<combat::Target>,
    visible: Query<&Visibility, With<HudRoot>>,
) {
    if focus.crew.as_ref().is_some_and(|m| m.open) {
        ui.0 = true;
        return;
    }
    let shown = hud_shown(&visible);
    ui.0 = shown && solid.iter().any(|i| *i != Interaction::None);
    if focus.menu.as_ref().is_some_and(|m| m.is_open()) {
        return;
    }
    let click = shown && mouse.just_pressed(MouseButton::Left);
    let mut flip = shown && binds.just_pressed(&keys, Act::WeaponBar);
    if click && toggle.iter().any(|i| *i != Interaction::None) {
        flip = true;
    }
    if ((click && panic.iter().any(|i| *i != Interaction::None)) || (shown && binds.just_pressed(&keys, Act::PanicFire))) && focus.fleets.as_ref().is_none_or(|f| f.direct()) {
        target.2.panic = true;
    }
    if click && ship_icon.iter().any(|i| *i != Interaction::None) {
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
        && focus.movement.iter().any(|i| *i != Interaction::None)
        && let Some(plot) = focus.plot.as_mut()
    {
        plot.advanced = !plot.advanced;
        plot.pending.clear();
        if let Some(sfx) = focus.sfx.as_mut() {
            sfx.write(crate::audio::Sfx::ui(if plot.advanced { "HUD_Movement_Advanced" } else { "HUD_Movement_Basic" }));
        }
    }
    if click && focus.fleets.as_ref().is_none_or(|f| f.direct()) {
        // Clicking an indirect bank's icon arms it (again to disarm); the next map click is its shot.
        let (ship, equipped) = *ship;
        let class = &lib.classes[ship.class];
        for (GunButton(bank), _) in guns.iter().filter(|(_, i)| **i != Interaction::None) {
            let indirect = class.mounts.iter().enumerate().any(|(i, m)| m.bank == *bank && combat::is_indirect(equipped.map_or(m.gun.as_str(), |g| g.gun(i))));
            target.2.armed = if indirect && target.2.armed != Some(*bank) { Some(*bank) } else { None };
            target.2.shot = None;
        }
    }
    if flip {
        toggles.weapons = !toggles.weapons;
    }
}

/// PROVISIONAL: retail dial segment geometry uses the existing Navy 86x101 coordinates.
pub fn speed_order_at(p: Vec2) -> usize {
    [Vec2::new(54.0, 71.0), Vec2::new(44.0, 57.0), Vec2::new(31.0, 44.0), Vec2::new(14.0, 34.0)]
        .iter()
        .enumerate()
        .min_by(|a, b| p.distance_squared(*a.1).total_cmp(&b.1.distance_squared(p)))
        .unwrap()
        .0
}

pub fn engine_input(
    mouse: Res<ButtonInput<MouseButton>>,
    mut ship: Single<&mut Ship, With<PlayerShip>>,
    dial: Query<(&Interaction, &bevy::ui::RelativeCursorPosition), With<EngineDial>>,
    tabs: Query<(&EngineOrder, &Interaction)>,
    visible: Query<&Visibility, With<HudRoot>>,
    fleets: Option<Res<crate::ship::Fleets>>,
    options: Option<Res<crate::options_menu::OptionsMenu>>,
    crew: Option<Res<crate::crew_arms::ScreenModel>>,
) {
    if crew.as_ref().is_some_and(|m| m.open)
        || options.as_ref().is_some_and(|o| o.is_open())
        || fleets.as_ref().is_some_and(|f| !f.direct())
        || !(hud_shown(&visible) && mouse.just_pressed(MouseButton::Left))
    {
        return;
    }
    let order = tabs.iter().find(|(_, i)| **i != Interaction::None).map(|(o, _)| o.0).or_else(|| {
        dial.iter().find(|(i, _)| **i != Interaction::None).and_then(|(_, rel)| rel.normalized).map(|p| {
            let p = p + Vec2::splat(0.5);
            speed_order_at(Vec2::new(p.x * 86.0, p.y * 101.0))
        })
    });
    if let Some(order) = order {
        ship.throttle = None;
        ship.order = order;
    }
}

/// The HUD exists and is not hidden (cinematics hide it).
pub fn hud_shown(visible: &Query<&Visibility, With<HudRoot>>) -> bool {
    !visible.is_empty() && visible.iter().all(|v| *v != Visibility::Hidden)
}

/// PAUSE button: a click toggles simulation pause (also available on the non-retail Pause key).
fn pause_button(mouse: Res<ButtonInput<MouseButton>>, mut vtime: ResMut<Time<Virtual>>, q: Query<&Interaction, With<PauseButton>>, mission: Option<Res<crate::campaign::Mission>>) {
    if !cinematic(mission.as_deref()) && mouse.just_pressed(MouseButton::Left) && q.iter().any(|i| *i != Interaction::None) {
        crate::toggle_pause(&mut vtime);
    }
}

/// Speed buttons step the game speed; labels follow it (and `--speed`).
fn speed_button(
    mouse: Res<ButtonInput<MouseButton>>,
    mut vtime: ResMut<Time<Virtual>>,
    mut window: Single<&mut Window, With<bevy::window::PrimaryWindow>>,
    buttons: Query<(&SpeedButton, &Interaction)>,
    mut labels: Query<&mut Text, With<SpeedLabel>>,
    mission: Option<Res<crate::campaign::Mission>>,
) {
    let clicked = buttons.iter().find(|(_, i)| **i != Interaction::None).map(|(b, _)| *b);
    if let Some(b) = clicked.filter(|_| !cinematic(mission.as_deref()) && mouse.just_pressed(MouseButton::Left)) {
        let speed = vtime.relative_speed() * if b == SpeedButton::Faster { 2.0 } else { 0.5 };
        crate::set_game_speed(&mut vtime, &mut window, speed.clamp(1.0, crate::MAX_GAME_SPEED));
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
        .init_resource::<crate::bindings::Bindings>()
        .init_resource::<combat::Target>()
        .insert_resource(Library::load(crate::assets_dir()))
        .insert_resource(Toggles { weapons: true, names: true })
        .insert_resource(crate::Orbit { yaw: 0.0, pitch: 0.5, dist: 50.0, focus: Some(Entity::PLACEHOLDER) })
        .add_systems(Update, hud_input);
    app.world_mut().spawn((
        PlayerShip,
        Ship {
            class: 0,
            side: crate::Side::Player,
            heading: 0.0,
            speed: 0.0,
            ang_vel: 0.0,
            order: 0,
            waypoints: vec![],
            goal: None,
            cap: 1.0,
            throttle: None,
            sail_energy: 1.0,
            crew_speed: 1.0,
            crew_turn: 1.0,
            avoid: 1.0,
        },
    ));
    let root = app.world_mut().spawn((HudRoot, Visibility::Inherited)).id();
    (app, root)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn a_crew_line_opens_the_talking_head_on_its_speaker() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).init_resource::<Talk>().init_resource::<HeadState>().add_message::<crate::audio::Sfx>().add_systems(Update, talking_head);
        app.insert_resource(crate::Config {
            groups: vec![],
            player: String::new(),
            allies: vec![],
            enemies: vec![],
            map: None,
            shot: None,
            frame: 0,
            speed: 1.0,
            ended: None,
            script: None,
            autolaunch: false,
            shot_frame: 0,
            late_frame: 0,
        });
        let line = app.world_mut().spawn(CrewSpeech("TalkingHead_Character_Aquanogs_0".into())).id();
        app.update();
        let head = app.world().resource::<HeadState>();
        assert!(head.speaking && head.speaker.as_deref() == Some("TalkingHead_Character_Aquanogs_0"));
        app.world_mut().despawn(line);
        app.update();
        assert!(!app.world().resource::<HeadState>().speaking, "closes when the line ends");
    }

    #[test]
    fn ship_bar_release_latch_hidden_gate_and_context_menu_do_not_promote() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<FleetHud>()
            .init_resource::<crate::options_menu::BarMenuRequest>()
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
        w.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Digit0);
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
    fn engine_tabs_clear_throttle_only_for_direct_visible_controls() {
        let (mut app, root) = input_app();
        app.add_systems(Update, engine_input.before(hud_input));
        let w = app.world_mut();
        let primary = w.query_filtered::<Entity, With<PlayerShip>>().single(w).unwrap();
        let secondary = w.spawn_empty().id();
        let mut f = crate::ship::Fleets { local: "Player".into(), selected: Some(secondary), ..default() };
        f.groups.insert("Player".into(), crate::ship::Fleet { members: vec![primary, secondary], ..default() });
        w.insert_resource(f);
        w.spawn((Solid, EngineOrder(2), Interaction::Pressed));
        w.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        w.get_mut::<Ship>(primary).unwrap().throttle = Some(0.4);
        app.update();
        assert_eq!(app.world().get::<Ship>(primary).unwrap().order, 0);
        assert_eq!(app.world().get::<Ship>(primary).unwrap().throttle, Some(0.4));
        app.world_mut().resource_mut::<crate::ship::Fleets>().selected = Some(primary);
        *app.world_mut().get_mut::<Visibility>(root).unwrap() = Visibility::Hidden;
        app.update();
        assert_eq!(app.world().get::<Ship>(primary).unwrap().throttle, Some(0.4));
        *app.world_mut().get_mut::<Visibility>(root).unwrap() = Visibility::Inherited;
        app.update();
        assert_eq!(app.world().get::<Ship>(primary).unwrap().order, 2);
        assert_eq!(app.world().get::<Ship>(primary).unwrap().throttle, None);
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn hud_clicks_do_not_leak_to_world_and_hidden_hud_ignores_input() {
        let (mut app, root) = input_app();
        app.world_mut().spawn((Solid, PanicButton, Interaction::Pressed));
        app.world_mut().spawn((Solid, ShipIconButton, Interaction::Pressed));
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        assert!(app.world().resource::<combat::Target>().2.panic);
        assert_eq!(app.world().resource::<crate::Orbit>().focus, None, "ship icon returns the camera to the player");
        assert!(app.world().resource::<UiClick>().0);
        app.world_mut().entity_mut(root).insert(Visibility::Hidden);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        app.update();
        assert!(!app.world().resource::<UiClick>().0);
        assert!(app.world().resource::<Toggles>().weapons);
        app.world_mut().resource_mut::<combat::Target>().2.panic = false;
        app.world_mut().entity_mut(root).insert(Visibility::Inherited);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyX);
        app.update();
        assert!(app.world().resource::<combat::Target>().2.panic);
    }

    #[test]
    fn speed_buttons_step_and_clamp_the_game_speed() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).init_resource::<ButtonInput<MouseButton>>().add_systems(Update, speed_button);
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
}
