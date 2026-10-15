//! Right-click options menu and context cursors (docs/re/docking-and-orders.md §1-2): a click (not a drag) with the right button on a
//! ship or base opens an icon menu around the cursor; three variants (enemy / own primary / other) with the retail icons, backgrounds and
//! slot offsets from hud.hdt. Implemented items: Focus, Stop, Dock, Tow start / stop (campaign missions only, the rules keep the link),
//! and the enemy menu's Board and Grapple start / stop (boarding.rs); Hold position and Cloak are drawn greyed out.
//! Sounds as in the exe: HUD_Options_Open on opening, _Focus / _Stop / _Dock for the item clicked, _NoSelection for a click on no
//! (enabled) item; HUD_Options_MouseOver when the cursor enters an enabled slot (PROVISIONAL: the exe plays it from its generic button code).
//! The context cursor (dock / can't / attack) follows the mouse while it hovers a ship or base.
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::audio::Sfx;
use crate::boarding::BoardMsg;
use crate::combat::{Health, Library, ray_hit};
use crate::docking::Dockees;
use crate::panels::REF_H;
use crate::sprites::{HudData, image, offset, sprite_of};
use crate::{Orbit, Ship, Side};

const CLICK_PX: f32 = 6.0;
const CLICK_SECS: f32 = 0.4;

/// Flags other systems read (atomics so the menu system can share the resource with `Dockees` without a borrow conflict).
#[derive(Resource, Default)]
pub struct BarMenuRequest(pub Option<Entity>);

#[derive(Resource, Default)]
pub struct OptionsMenu {
    /// True on the frame a click closed the menu: the world must not also act on it.
    pub swallow: AtomicBool,
    open: AtomicBool,
}
impl OptionsMenu {
    pub fn reset(&self) {
        self.open.store(false, Ordering::Relaxed);
        self.swallow.store(false, Ordering::Relaxed);
    }
    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Relaxed)
    }
}

#[derive(Default)]
struct Local2 {
    open: Option<Open>,
    press: Option<(Vec2, f32)>,
    hover: Option<usize>,
}
struct Open {
    actor: Entity,
    target: Entity,
    root: Entity,
    items: Vec<(Act, Rect, bool)>, // screen-pixel rect, enabled
}
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Enemy,
    Friendly,
    Selected,
}
#[derive(Clone, Copy, PartialEq, Debug)]
enum Act {
    Focus,
    Stop,
    Dock,
    TowStart,
    TowStop,
    BoardStart,
    BoardStop,
    GrappleStart,
    GrappleStop,
    HoldOn,
    HoldOff,
    CloakStart,
    CloakStop,
    Unavailable,
}
/// The enemy menu's Board and Grapple slots, each (shows stop, enabled).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
struct Hooks {
    board: (bool, bool),
    grapple: (bool, bool),
    /// Own ship: holding position now; cloak state when its hull can cloak (Some(on)).
    hold: bool,
    cloak: Option<bool>,
    focus_unavailable: bool,
}
/// The tow slot of a friendly ship's menu: start (enabled when a tow can be made) or, when the player's ship already tows it, stop.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Tow {
    Start(bool),
    Stop,
}
#[derive(Component)]
struct MenuUi;
#[derive(Component)]
struct ContextCursor;

pub fn plugin(app: &mut App) {
    app.init_resource::<OptionsMenu>().init_resource::<BarMenuRequest>().add_systems(
        Update,
        (
            (dev_click, menu, context_cursor, dev_menu).chain().before(crate::input).run_if(in_state(crate::menu::Screen::Battle)).run_if(crate::menu::overlay_closed),
            restore_pointer.run_if(crate::menu::menu_active),
        ),
    );
}

/// `context_cursor` hides the system pointer over ships; it stops running once a menu opens, so show the pointer again here.
fn restore_pointer(mut pointer: Single<&mut CursorOptions, With<PrimaryWindow>>, mut shown: Query<&mut Visibility, With<ContextCursor>>) {
    if !pointer.visible {
        pointer.visible = true;
    }
    for mut v in &mut shown {
        v.set_if_neq(Visibility::Hidden);
    }
}

/// (action, hud sprite, enabled) per slot, in the exe's builder order. `can_dock`: the dock test passed.
fn items(kind: Kind, can_dock: bool, tow: Tow, hooks: Hooks) -> Vec<(Act, &'static str, bool)> {
    use Act::*;
    match kind {
        Kind::Enemy => {
            let board = if hooks.board.0 { (BoardStop, "OPTIONS_MENU_BUTTON_BOARD_STOP", hooks.board.1) } else { (BoardStart, "OPTIONS_MENU_BUTTON_BOARD_START", hooks.board.1) };
            let grapple = if hooks.grapple.0 {
                (GrappleStop, "OPTIONS_MENU_BUTTON_GRAPPLE_STOP", hooks.grapple.1)
            } else {
                (GrappleStart, "OPTIONS_MENU_BUTTON_GRAPPLE_START", hooks.grapple.1)
            };
            vec![(Focus, "OPTIONS_MENU_BUTTON_FOCUS", !hooks.focus_unavailable), board, grapple]
        }
        Kind::Friendly => {
            let tow = match tow {
                Tow::Start(on) => (TowStart, "OPTIONS_MENU_BUTTON_TOW_START", on),
                Tow::Stop => (TowStop, "OPTIONS_MENU_BUTTON_TOW_STOP", true),
            };
            vec![(Focus, "OPTIONS_MENU_BUTTON_FOCUS", !hooks.focus_unavailable), tow, (Dock, "OPTIONS_MENU_BUTTON_DOCK", can_dock)]
        }
        Kind::Selected => vec![
            (Focus, "OPTIONS_MENU_BUTTON_FOCUS", true),
            (Stop, "OPTIONS_MENU_BUTTON_STOP", true),
            if hooks.hold { (HoldOff, "OPTIONS_MENU_BUTTON_AI_NOT_LOCK_POSITION", true) } else { (HoldOn, "OPTIONS_MENU_BUTTON_AI_LOCK_POSITION", true) },
            match hooks.cloak {
                Some(true) => (CloakStop, "OPTIONS_MENU_BUTTON_CLOAK_STOP", true),
                Some(false) => (CloakStart, "OPTIONS_MENU_BUTTON_CLOAK_START", true),
                None => (Unavailable, "OPTIONS_MENU_BUTTON_CLOAK_START", false),
            },
        ],
    }
}
fn background(kind: Kind) -> [&'static str; 4] {
    match kind {
        Kind::Enemy => [
            "OPTIONS_MENU_ENEMY_SHIP_BACKGROUND_TOPLEFT",
            "OPTIONS_MENU_ENEMY_SHIP_BACKGROUND_TOPRIGHT",
            "OPTIONS_MENU_ENEMY_SHIP_BACKGROUND_BOTTOMRIGHT",
            "OPTIONS_MENU_ENEMY_SHIP_BACKGROUND_BOTTOMLEFT",
        ],
        Kind::Friendly => [
            "OPTIONS_MENU_FRIENDLY_SHIP_BACKGROUND_TOPLEFT",
            "OPTIONS_MENU_FRIENDLY_SHIP_BACKGROUND_TOPRIGHT",
            "OPTIONS_MENU_FRIENDLY_SHIP_BACKGROUND_BOTTOMRIGHT",
            "OPTIONS_MENU_FRIENDLY_SHIP_BACKGROUND_BOTTOMLEFT",
        ],
        Kind::Selected => [
            "OPTIONS_MENU_SELECTED_SHIP_BACKGROUND_TOPLEFT",
            "OPTIONS_MENU_SELECTED_SHIP_BACKGROUND_TOPRIGHT",
            "OPTIONS_MENU_SELECTED_SHIP_BACKGROUND_BOTTOMRIGHT",
            "OPTIONS_MENU_SELECTED_SHIP_BACKGROUND_BOTTOMLEFT",
        ],
    }
}

fn menu(
    (mouse, mut keys, mission, boarders, mut board, fleets, mut bar, status, ui, flags, sensors): (
        Res<ButtonInput<MouseButton>>,
        ResMut<ButtonInput<KeyCode>>,
        Option<Res<crate::campaign::Mission>>,
        crate::boarding::Boarders,
        MessageWriter<BoardMsg>,
        Res<crate::ship::Fleets>,
        ResMut<BarMenuRequest>,
        Res<crate::boarding::BoardStatus>,
        Res<crate::panels::UiClick>,
        Query<(Has<crate::ship::HoldPosition>, Option<&crate::cloak::Cloak>)>,
        Res<crate::environment::Sensors>,
    ),
    time: Res<Time<Real>>,
    motion: Res<AccumulatedMouseMotion>,
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform)>,
    hud: Option<Res<HudData>>,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    state: Res<OptionsMenu>,
    mut local: Local<Local2>,
    mut commands: Commands,
    mut orbit: ResMut<Orbit>,
    actors: Query<(Entity, &Ship, &Transform)>,
    ships: Query<(Entity, &Transform, &Ship, &Health)>,
    mut docks: Dockees,
    mut sfx: MessageWriter<Sfx>,
) {
    let Some(actor) = fleets.selected else { return };
    let Ok((e, source, tf)) = actors.get(actor) else { return };
    let mut player = (e, source.clone(), tf);
    let Local2 { open: open_menu, press, hover } = &mut *local;
    if open_menu.as_ref().is_some_and(|o| o.actor != actor || commands.get_entity(o.root).is_err() || !sensors.player_sees(o.target, mission.as_deref())) {
        if let Some(o) = open_menu.take()
            && let Ok(mut e) = commands.get_entity(o.root)
        {
            e.despawn();
        }
        *press = None;
        *hover = None;
        state.open.store(false, Ordering::Relaxed);
    }
    state.swallow.store(false, Ordering::Relaxed);
    if crate::panels::cinematic(mission.as_deref()) || crate::rules::locked(mission.as_deref()) {
        if let Some(open) = open_menu.take() {
            commands.entity(open.root).despawn();
        }
        state.open.store(false, Ordering::Relaxed);
        *press = None;
        *hover = None;
        return;
    }
    let Some(hud) = hud else { return };
    let cursor = window.cursor_position();
    let slot = cursor.and_then(|p| open_menu.as_ref().and_then(|o| o.items.iter().position(|(_, r, on)| *on && r.contains(p))));
    if slot != *hover {
        *hover = slot;
        if slot.is_some() {
            sfx.write(Sfx::ui("HUD_Options_MouseOver"));
        }
    }
    // Left click: pick a slot (or just close).
    if open_menu.is_some() && (mouse.just_pressed(MouseButton::Left) || keys.just_pressed(KeyCode::Escape)) {
        let open = open_menu.take().unwrap();
        keys.clear_just_pressed(KeyCode::Escape); // this Esc closed the menu; it must not also open the in-game menu
        state.open.store(false, Ordering::Relaxed);
        commands.entity(open.root).despawn();
        if !mouse.just_pressed(MouseButton::Left) {
            return;
        }
        state.swallow.store(true, Ordering::Relaxed);
        let sound = match slot.map(|i| open.items[i].0) {
            Some(Act::Focus) if open.target == actor || sensors.in_sight(actor, open.target, mission.as_deref()) => {
                orbit.focus = Some(open.target);
                "HUD_Options_Focus"
            }
            Some(Act::Stop) => {
                player.1.order = 0;
                player.1.waypoints.clear();
                commands.entity(actor).insert(player.1.clone()).remove::<crate::boarding::GrappleOrder>();
                commands.queue(move |w: &mut World| {
                    if let Some(mut p) = w.get_mut::<crate::ship::FleetPilot>(actor) {
                        p.hold = false;
                        p.destination = None;
                        p.target = None;
                    }
                });
                docks.cancel_as(actor);
                "HUD_Options_Stop"
            }
            Some(Act::HoldOn) => {
                player.1.order = 0;
                player.1.waypoints.clear();
                commands.entity(actor).insert((player.1.clone(), crate::ship::HoldPosition));
                "HUD_Options_Select"
            }
            Some(Act::HoldOff) => {
                commands.entity(actor).remove::<crate::ship::HoldPosition>();
                "HUD_Options_Select"
            }
            Some(a @ (Act::CloakStart | Act::CloakStop)) => {
                let on = a == Act::CloakStart;
                commands.queue(move |w: &mut World| match w.get_mut::<crate::cloak::Cloak>(actor) {
                    Some(mut c) => c.on = on,
                    None => {
                        let mut c = crate::cloak::Cloak::new(false, false);
                        c.on = on;
                        w.entity_mut(actor).insert(c);
                    }
                });
                if on { "HUD_Options_CloakStart" } else { "HUD_Options_CloakStop" }
            }
            Some(Act::BoardStart) => {
                board.write(BoardMsg::Board { boarder: Some(actor), target: open.target });
                "HUD_Options_BoardStart"
            }
            Some(Act::BoardStop) => {
                board.write(BoardMsg::Unboard { boarder: Some(actor) });
                "HUD_Options_BoardStop"
            }
            Some(Act::GrappleStart) => {
                board.write(BoardMsg::Grapple { grappler: Some(actor), target: open.target });
                "HUD_Options_GrappleStart"
            }
            Some(Act::GrappleStop) => {
                board.write(BoardMsg::Ungrapple { grappler: Some(actor) });
                "HUD_Options_GrappleStop"
            }
            Some(Act::Dock) if docks.can_dock(actor, open.target) => {
                docks.dock_as(actor, open.target);
                "HUD_Options_Dock"
            }
            Some(Act::TowStart) => {
                docks.tow_as(actor, open.target);
                "HUD_Options_TowStart"
            }
            Some(Act::TowStop) => {
                docks.break_tow_as(actor);
                "HUD_Options_TowStop"
            }
            Some(Act::Unavailable | Act::Focus | Act::Dock) | None => "HUD_Options_NoSelection",
        };
        sfx.write(Sfx::ui(sound));
        return;
    }
    let Some(cursor) = cursor else { return };
    // Right click = press and release close together (a longer or moving press is the camera drag).
    if mouse.just_pressed(MouseButton::Right) {
        *press = Some((cursor, time.elapsed_secs()));
    }
    if mouse.pressed(MouseButton::Right) && motion.delta.length() > CLICK_PX {
        *press = None;
    }
    let bar_target = bar.0.take();
    if bar_target.is_none() {
        if ui.0 {
            *press = None;
            return;
        }
        if !mouse.just_released(MouseButton::Right) {
            return;
        }
        let Some((at, t0)) = press.take() else { return };
        if at.distance(cursor) > CLICK_PX || time.elapsed_secs() - t0 > CLICK_SECS {
            return;
        }
        if crate::panels::cinematic(mission.as_deref()) {
            return;
        }
    }
    *press = None;
    if let Some(old) = open_menu.take() {
        commands.entity(old.root).despawn();
        state.open.store(false, Ordering::Relaxed);
        if bar_target == Some(old.target) {
            *hover = None;
            return;
        }
    }
    let (camera, gt) = *cam;
    let Ok(ray) = camera.viewport_to_world(gt, cursor) else { return };
    let ally = ships
        .iter()
        .filter(|(e, _, sh, h)| *e != actor && sh.side == Side::Player && !h.destroyed && sensors.player_sees(*e, mission.as_deref()))
        .filter_map(|(e, tf, sh, _)| ray_hit(&lib.classes[sh.class], tf, ray).map(|t| (e, t)))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let enemy = ships
        .iter()
        .filter(|(e, _, sh, h)| sh.side == Side::Enemy && !h.destroyed && sensors.player_sees(*e, mission.as_deref()))
        .filter_map(|(e, tf, sh, _)| ray_hit(&lib.classes[sh.class], tf, ray).map(|t| (e, t)))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let own = ray_hit(&lib.classes[player.1.class], player.2, ray);
    let (target, kind, can_dock) = if let Some(e) = bar_target {
        if !fleets.roster().contains(&e) {
            return;
        }
        (e, if e == actor { Kind::Selected } else { Kind::Friendly }, docks.can_dock(actor, e))
    } else if own.is_some() {
        (actor, Kind::Selected, false)
    } else if let Some((e, ok)) = docks.under(ray, actor) {
        (e, Kind::Friendly, ok)
    } else if let Some((e, _)) = ally {
        (e, Kind::Friendly, false) // any other friendly ship: Focus, Tow and a greyed Dock
    } else if let Some((e, _)) = enemy {
        (e, Kind::Enemy, false)
    } else {
        return;
    };
    // The player's ship already towing this one gets Stop; any other ship gets Start, enabled for ships when a tow can be made.
    let tow = if docks.towee_of(player.0) == Some(target) { Tow::Stop } else { Tow::Start(docks.can_tow() && docks.towable(target) && ships.get(target).is_ok()) };
    let (held, cloak) = flags.get(actor).map(|(h, c)| (h, c.map(|c| c.on))).unwrap_or_default();
    let cloak = cloak.or_else(|| lib.classes[player.1.class].cloakable.then_some(false));
    let mut hooks = if Some(actor) == fleets.primary() {
        Hooks { board: status.board(target), grapple: status.grapple(target), ..default() }
    } else {
        let boarding = boarders.boarding(actor, target);
        let grappling = boarders.grappling(actor, target);
        Hooks { board: (boarding, boarding || boarders.can_board(actor, target)), grapple: (grappling, grappling || boarders.can_grapple(actor)), ..default() }
    };
    (hooks.hold, hooks.cloak) = (held, cloak);
    hooks.focus_unavailable = target != actor && !sensors.in_sight(actor, target, mission.as_deref());
    let (root, rects) = spawn_menu(&mut commands, &assets, &hud, &window, cursor, kind, can_dock, tow, hooks);
    *open_menu = Some(Open { actor, target, root, items: rects });
    *hover = None;
    sfx.write(Sfx::ui("HUD_Options_Open"));
    state.open.store(true, Ordering::Relaxed);
}

/// What a view draws for an open menu (on its root, a zero-size node at the menu centre): the variant's four background corners and
/// each slot as (sprite, enabled, offset from the centre in reference px).
#[derive(Component)]
pub struct OptionsMenuArt {
    pub background: [&'static str; 4],
    pub slots: Vec<(&'static str, bool, Vec2)>,
}

/// Spawns the menu root around `cursor` with its `OptionsMenuArt`; hit rects are the retail icons at the retail slot offsets.
fn spawn_menu(
    commands: &mut Commands,
    assets: &AssetServer,
    hud: &HudData,
    window: &Window,
    cursor: Vec2,
    kind: Kind,
    can_dock: bool,
    tow: Tow,
    hooks: Hooks,
) -> (Entity, Vec<(Act, Rect, bool)>) {
    let s = window.height() / REF_H;
    let list = items(kind, can_dock, tow, hooks);
    let n = list.len();
    let centre = menu_centre(window, cursor);
    let mut rects = vec![];
    let mut slots = vec![];
    for (i, (act, sprite, on)) in list.into_iter().enumerate() {
        let Some((_, size)) = image(assets, hud, sprite) else { continue };
        let at = offset(hud, &format!("OPTIONS_MENU_SLOT_OFFSET_{i}_OF_{n}"));
        let min = centre + at * s;
        rects.push((act, Rect::from_corners(min, min + size * s), on));
        slots.push((sprite, on, at));
    }
    let root = commands
        .spawn((
            MenuUi,
            crate::Scenario,
            OptionsMenuArt { background: background(kind), slots },
            GlobalZIndex(170),
            Node { position_type: PositionType::Absolute, left: Val::Px(centre.x), top: Val::Px(centre.y), ..default() },
        ))
        .id();
    (root, rects)
}

fn menu_centre(window: &Window, cursor: Vec2) -> Vec2 {
    let s = window.height() / REF_H;
    cursor.clamp(Vec2::splat(70.0 * s), Vec2::new(window.width(), window.height()) - Vec2::splat(70.0 * s))
}

/// Dev hook: `--script dock-click|dock-menu` (rules.rs puts the player next to mission 1's tender) docks with RLS Bounty through
/// synthetic mouse input once the dock test passes: focus the camera on it, then a left click on it, or a right click on it and a left
/// click on the menu's Dock slot. One step per frame so the real `menu` / `input` systems see ordinary presses and releases.
fn dev_click(
    cfg: Res<crate::Config>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform)>,
    hud: Option<Res<HudData>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut orbit: ResMut<Orbit>,
    player: Single<Entity, With<crate::PlayerShip>>,
    tender: Query<(Entity, &Transform, &crate::campaign::MissionShip), With<crate::docking::Dockable>>,
    docks: Dockees,
    mut step: Local<u32>,
) {
    let menu = match cfg.script.as_deref() {
        Some("dock-click") => false,
        Some("dock-menu") => true,
        _ => return,
    };
    let Some((target, tf, _)) = tender.iter().find(|t| t.2.name == "RLS Bounty") else { return };
    let (camera, gt) = *cam;
    let on_target = camera.world_to_viewport(gt, tf.translation).ok();
    let button = if menu { MouseButton::Right } else { MouseButton::Left };
    match *step {
        0 => {
            orbit.focus = Some(target);
            let ready = on_target.and_then(|p| docks.under(camera.viewport_to_world(gt, p).ok()?, *player));
            if ready == Some((target, true)) {
                *step = 1;
            }
        }
        1 => {
            window.set_cursor_position(on_target);
            mouse.press(button);
            *step = 2;
        }
        2 => {
            mouse.release(button);
            *step = if menu { 3 } else { 9 };
        }
        3 => {
            // The Friendly menu's Dock icon is slot 2 of 3 (`items`).
            let (Some(hud), Some(cursor)) = (hud, window.cursor_position()) else { return };
            let s = window.height() / REF_H;
            let size = sprite_of(&hud, "OPTIONS_MENU_BUTTON_DOCK").expect("dock icon").1.size();
            let at = menu_centre(&window, cursor) + (offset(&hud, "OPTIONS_MENU_SLOT_OFFSET_2_OF_3") + size / 2.0) * s;
            window.set_cursor_position(Some(at));
            *step = 4;
        }
        4 => {
            mouse.press(MouseButton::Left);
            *step = 5;
        }
        5 => {
            mouse.release(MouseButton::Left);
            *step = 9;
        }
        _ => {}
    }
}

/// Dev hook: `--script menu-selected|menu-friendly|menu-enemy` opens that variant in the middle of the screen at frame 30 (screenshots).
fn dev_menu(cfg: Res<crate::Config>, window: Single<&Window, With<PrimaryWindow>>, hud: Option<Res<HudData>>, assets: Res<AssetServer>, mut commands: Commands) {
    let Some(hud) = hud else { return };
    let kind = match cfg.script.as_deref() {
        Some("menu-selected") => Kind::Selected,
        Some("menu-friendly") => Kind::Friendly,
        Some("menu-enemy") => Kind::Enemy,
        _ => return,
    };
    if cfg.frame == 30 {
        spawn_menu(&mut commands, &assets, &hud, &window, Vec2::new(window.width(), window.height()) / 2.0, kind, true, Tow::Start(true), Hooks::default());
    }
}

/// The context cursor sprite (hud.hdt `Cursor_*` with its `Offset_Cursor_*` hot spot) while hovering a ship or base; the system pointer is hidden then.
fn context_cursor(
    window: Single<&Window, With<PrimaryWindow>>,
    mut pointer: Single<&mut CursorOptions, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform)>,
    hud: Option<Res<HudData>>,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    menu: Res<OptionsMenu>,
    ui: Res<crate::panels::UiClick>,
    mission: Option<Res<crate::campaign::Mission>>,
    ships: Query<(Entity, &Transform, &Ship, &Health)>,
    sensors: Res<crate::environment::Sensors>,
    fleets: Res<crate::ship::Fleets>,
    docks: Dockees,
    mut commands: Commands,
    mut shown: Query<(Entity, &mut Node, &mut ImageNode, &mut Visibility), With<ContextCursor>>,
    mut current: Local<&'static str>,
) {
    let want = (|| {
        let hud = hud.as_ref()?;
        if crate::panels::cinematic(mission.as_deref()) || menu.open.load(Ordering::Relaxed) || ui.0 {
            return None;
        }
        let cursor = window.cursor_position()?;
        let (camera, gt) = *cam;
        let ray = camera.viewport_to_world(gt, cursor).ok()?;
        let actor = fleets.selected?;
        let own = ships
            .iter()
            .filter(|(e, _, _, h)| !h.destroyed && fleets.roster().contains(e))
            .filter_map(|(e, tf, sh, _)| ray_hit(&lib.classes[sh.class], tf, ray).map(|t| (e, t)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let name = if let Some((e, _)) = own {
            if Some(e) == fleets.selected {
                "Cursor_Focus_On_WorldObject_Under_Cursor"
            } else if Some(e) == fleets.primary() {
                "Cursor_PrimaryShip_Select"
            } else {
                "Cursor_SecondaryShip_Select"
            }
        } else if let Some((_, ok)) = docks.under(ray, actor) {
            if ok { "Cursor_Dock_With_WorldObject_Under_Cursor" } else { "Cursor_Cant_Do_Anything_To_WorldObject_Under_Cursor" }
        } else if docks.hostile_base_hit(ray, ships.get(actor).ok()?.1.translation).is_some() {
            if fleets.direct() { "Cursor_PrimaryShip_AttackShip" } else { "Cursor_SecondaryShip_AttackShip" }
        } else if let Some((_, _, sh, _)) =
            ships.iter().find(|(e, tf, sh, h)| !h.destroyed && sensors.player_sees(*e, mission.as_deref()) && ray_hit(&lib.classes[sh.class], tf, ray).is_some())
        {
            // Exe hover: attack an enemy; any other friendly ship that is not dockable is "can't do anything".
            if sh.side == Side::Enemy {
                if fleets.direct() { "Cursor_PrimaryShip_AttackShip" } else { "Cursor_SecondaryShip_AttackShip" }
            } else {
                "Cursor_Cant_Do_Anything_To_WorldObject_Under_Cursor"
            }
        } else {
            return None;
        };
        Some((hud, cursor, name))
    })();
    let Some((hud, cursor, name)) = want else {
        pointer.visible = true;
        for (_, _, _, mut v) in &mut shown {
            *v = Visibility::Hidden;
        }
        return;
    };
    pointer.visible = false;
    let s = window.height() / REF_H;
    let Some((img, size)) = image(&assets, hud, name) else { return };
    let hot = offset(hud, &format!("Offset_{name}"));
    let node = Node {
        position_type: PositionType::Absolute,
        left: Val::Px(cursor.x + hot.x * s),
        top: Val::Px(cursor.y + hot.y * s),
        width: Val::Px(size.x * s),
        height: Val::Px(size.y * s),
        ..default()
    };
    match shown.single_mut() {
        Ok((_, mut n, mut i, mut v)) => {
            *n = node;
            if *current != name {
                *i = img;
                *current = name;
            }
            *v = Visibility::Inherited;
        }
        Err(_) => {
            *current = "";
            commands.spawn((ContextCursor, crate::Scenario, GlobalZIndex(180), img, node));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled(list: &[(Act, &str, bool)], act: Act) -> Option<bool> {
        list.iter().find(|i| i.0 == act).map(|i| i.2)
    }

    #[test]
    fn enemy_menu_board_and_grapple_slots_follow_their_state() {
        let idle = items(Kind::Enemy, false, Tow::Start(false), Hooks::default());
        assert_eq!(idle.iter().map(|i| i.0).collect::<Vec<_>>(), [Act::Focus, Act::BoardStart, Act::GrappleStart]);
        assert_eq!(enabled(&idle, Act::BoardStart), Some(false));
        let ready = items(Kind::Enemy, false, Tow::Start(false), Hooks { board: (false, true), grapple: (false, true), ..Hooks::default() });
        assert_eq!((enabled(&ready, Act::BoardStart), enabled(&ready, Act::GrappleStart)), (Some(true), Some(true)));
        let busy = items(Kind::Enemy, false, Tow::Start(false), Hooks { board: (true, true), grapple: (true, true), ..Hooks::default() });
        assert_eq!((busy[1].0, busy[1].1), (Act::BoardStop, "OPTIONS_MENU_BUTTON_BOARD_STOP"));
        assert_eq!((busy[2].0, busy[2].1), (Act::GrappleStop, "OPTIONS_MENU_BUTTON_GRAPPLE_STOP"));
    }

    #[test]
    fn focus_is_greyed_when_the_group_has_no_sight_of_the_object() {
        for kind in [Kind::Enemy, Kind::Friendly] {
            let list = items(kind, false, Tow::Start(false), Hooks { focus_unavailable: true, ..default() });
            assert_eq!(enabled(&list, Act::Focus), Some(false));
        }
        assert_eq!(enabled(&items(Kind::Selected, false, Tow::Start(false), Hooks { focus_unavailable: true, ..default() }), Act::Focus), Some(true));
    }

    #[test]
    fn friendly_menu_tow_slot_follows_the_link() {
        let start = items(Kind::Friendly, false, Tow::Start(true), Hooks::default());
        assert_eq!(start.len(), 3, "Focus, Tow, Dock");
        assert_eq!(start[1].0, Act::TowStart);
        assert_eq!(start[1].1, "OPTIONS_MENU_BUTTON_TOW_START");
        assert_eq!(enabled(&start, Act::Dock), Some(false), "no dock test pass: Dock greyed");
        let linked = items(Kind::Friendly, true, Tow::Stop, Hooks::default());
        assert_eq!(linked[1].0, Act::TowStop);
        assert_eq!(linked[1].1, "OPTIONS_MENU_BUTTON_TOW_STOP");
        assert_eq!(enabled(&linked, Act::TowStop), Some(true));
        assert_eq!(enabled(&linked, Act::Dock), Some(true));
        assert_eq!(enabled(&items(Kind::Friendly, false, Tow::Start(false), Hooks::default()), Act::TowStart), Some(false), "not towable: greyed");
    }
}
