//! Right-click options menu and context cursors (docs/re/docking-and-orders.md §1-2): a click (not a drag) with the right button on a
//! ship or base opens an icon menu around the cursor; three variants (enemy / own primary / other) with the retail icons, backgrounds and
//! slot offsets from hud.hdt. Implemented items: Focus, Stop, Dock, Tow start / stop (campaign missions only, the rules keep the link),
//! and the enemy menu's Board and Grapple start / stop (boarding.rs), Hold position and Cloak.
//! Sounds as in the original: HUD_Options_Open on opening, _Focus / _Stop / _Dock for the item clicked, _NoSelection for a click on no
//! (enabled) item; HUD_Options_MouseOver when the cursor enters an enabled slot (PROVISIONAL: the original may play it for any button).
//! The context cursor (dock / can't / attack) follows the mouse while it hovers a ship or base.
use bevy::ecs::system::SystemParam;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::audio::Sfx;
use crate::boarding::BoardMsg;
use crate::combat::{Health, Library, ray_hit, seg_sphere};
use crate::commands::{Kind as CommandKind, Orders};
use crate::docking::Dockees;
use crate::panels::REF_H;
use crate::sprites::{HudData, offset, size_of};
use crate::{Orbit, Ship, Side};

/// PROVISIONAL: the original's click tolerance is not measured.
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
    cloak: Option<(bool, bool)>,
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
/// The context cursor to draw instead of the system pointer: its hud.hdt `Cursor_*` sprite and the pointer position (window px).
/// None shows the system pointer.
#[derive(Resource, Default)]
pub struct HoverCursor(pub Option<(&'static str, Vec2)>);

pub fn plugin(app: &mut App) {
    app.init_resource::<OptionsMenu>().init_resource::<BarMenuRequest>().init_resource::<HoverCursor>().init_resource::<crate::combat::LobAim>().add_systems(
        Update,
        (
            (dev_click, menu, context_cursor, dev_menu).chain().before(crate::input).run_if(in_state(crate::menu::Screen::Battle)).run_if(crate::menu::overlay_closed),
            restore_pointer.run_if(crate::menu::menu_active),
        ),
    );
}

/// `context_cursor` hides the system pointer over ships; it stops running once a menu opens, so show the pointer again here.
fn restore_pointer(mut pointer: Single<&mut CursorOptions, With<PrimaryWindow>>, mut hover: ResMut<HoverCursor>) {
    if !pointer.visible {
        pointer.visible = true;
    }
    if hover.0.is_some() {
        hover.0 = None;
    }
}

/// (action, hud sprite, enabled) per slot, in the original's slot order. `can_dock`: the dock test passed.
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
                Some((true, _)) => (CloakStop, "OPTIONS_MENU_BUTTON_CLOAK_STOP", true),
                // Start is shown while off or decloaking, and greyed until a start is allowed (5 s idle after the last decloak).
                Some((false, ok)) => (CloakStart, "OPTIONS_MENU_BUTTON_CLOAK_START", ok),
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

/// The input a click on the menu reads.
#[derive(SystemParam)]
struct MenuInput<'w> {
    mouse: Res<'w, ButtonInput<MouseButton>>,
    keys: ResMut<'w, ButtonInput<KeyCode>>,
    motion: Res<'w, AccumulatedMouseMotion>,
    time: Res<'w, Time<Real>>,
    ui: Res<'w, crate::panels::UiClick>,
}

/// What the menu looks up about the battle.
#[derive(SystemParam)]
struct MenuScene<'w, 's> {
    mission: Option<Res<'w, crate::campaign::Mission>>,
    fleets: Res<'w, crate::ship::Fleets>,
    sensors: Res<'w, crate::environment::Sensors>,
    lib: Res<'w, Library>,
    actors: Query<'w, 's, (Entity, &'static Ship, &'static Transform)>,
    ships: Query<'w, 's, (Entity, &'static Transform, &'static Ship, &'static Health)>,
    flags: Query<'w, 's, (Has<crate::ship::HoldPosition>, Option<&'static crate::cloak::Cloak>)>,
    scenery: Query<'w, 's, ScenerySpot, (Without<Ship>, Or<(With<crate::combat::Asteroid>, With<crate::environment::SpaceAnimal>, With<crate::collision::Obstacle>)>)>,
}

/// Scenery a right click can open the menu on: an asteroid, a space animal or an island (its collision hull).
type ScenerySpot =
    (Entity, &'static Transform, Option<&'static crate::combat::Asteroid>, Option<&'static crate::environment::SpaceAnimal>, Option<&'static crate::collision::Obstacle>);

/// The nearest asteroid, space animal or island under `ray` (entity, distance) among the ones `sees` keeps.
fn scenery_hit<'a>(
    spots: impl Iterator<Item = (Entity, &'a Transform, Option<&'a crate::combat::Asteroid>, Option<&'a crate::environment::SpaceAnimal>, Option<&'a crate::collision::Obstacle>)>,
    sees: impl Fn(Entity) -> bool,
    ray: Ray3d,
) -> Option<(Entity, f32)> {
    spots
        .filter(|(e, ..)| sees(*e))
        .filter_map(|(e, tf, rock, animal, hull)| {
            let sphere = |r: f32| seg_sphere(ray.origin, ray.origin + *ray.direction * 3000.0, tf.translation, r).map(|t| t * 3000.0);
            let t = match (animal, rock, hull) {
                (Some(a), ..) => sphere(a.radius),
                (_, _, Some(hull)) => hull.ray_hit(ray),
                (_, Some(r), None) => sphere(r.radius),
                _ => None,
            }?;
            Some((e, t))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// Boarding and grappling: the tests for the slots and the messages the slots send.
#[derive(SystemParam)]
struct MenuBoarding<'w, 's> {
    boarders: crate::boarding::Boarders<'w, 's>,
    board: MessageWriter<'w, BoardMsg>,
    status: Res<'w, crate::boarding::BoardStatus>,
}

/// A right click is a press and release close together; a longer or moving press is the camera drag.
fn is_tap(press: (Vec2, f32), cursor: Vec2, now: f32) -> bool {
    press.0.distance(cursor) <= CLICK_PX && now - press.1 <= CLICK_SECS
}

/// The sound of a left click on slot `act` and the boarding message it sends, after doing what the slot says for ship `actor`
/// (Stop, Hold and Cloak are `commands` orders). A Focus slot the ship cannot see the target for comes in as `Unavailable`.
fn run_slot(
    act: Option<Act>,
    target: Entity,
    actor: Entity,
    orders: &mut Orders,
    commands: &mut Commands,
    orbit: &mut Orbit,
    docks: &mut Dockees,
) -> (&'static str, Option<BoardMsg>) {
    let mut msg = None;
    let sound = match act {
        Some(Act::Focus) => {
            orbit.focus = Some(target);
            "HUD_Options_Focus"
        }
        Some(Act::Stop) => {
            orders.send(CommandKind::Stop { ship: actor });
            "HUD_Options_Stop"
        }
        Some(Act::HoldOn) => {
            orders.send(CommandKind::Hold { ship: actor, on: true });
            "HUD_Options_Select"
        }
        Some(Act::HoldOff) => {
            orders.send(CommandKind::Hold { ship: actor, on: false });
            "HUD_Options_Select"
        }
        Some(a @ (Act::CloakStart | Act::CloakStop)) => {
            let on = a == Act::CloakStart;
            orders.send(CommandKind::Cloak { ship: actor, on });
            if on { "HUD_Options_CloakStart" } else { "HUD_Options_CloakStop" }
        }
        Some(Act::BoardStart) => {
            crate::ship::release_hold(commands, actor);
            msg = Some(BoardMsg::Board { boarder: Some(actor), target });
            "HUD_Options_BoardStart"
        }
        Some(Act::BoardStop) => {
            msg = Some(BoardMsg::Unboard { boarder: Some(actor) });
            "HUD_Options_BoardStop"
        }
        Some(Act::GrappleStart) => {
            msg = Some(BoardMsg::Grapple { grappler: Some(actor), target });
            "HUD_Options_GrappleStart"
        }
        Some(Act::GrappleStop) => {
            msg = Some(BoardMsg::Ungrapple { grappler: Some(actor) });
            "HUD_Options_GrappleStop"
        }
        Some(Act::Dock) if docks.can_dock(actor, target) => {
            crate::ship::release_hold(commands, actor);
            docks.dock_as(actor, target);
            "HUD_Options_Dock"
        }
        Some(Act::TowStart) => {
            crate::ship::release_hold(commands, actor);
            docks.tow_as(actor, target);
            "HUD_Options_TowStart"
        }
        Some(Act::TowStop) => {
            docks.break_tow_as(actor);
            "HUD_Options_TowStop"
        }
        Some(Act::Unavailable | Act::Dock) | None => "HUD_Options_NoSelection",
    };
    (sound, msg)
}

/// What a right click at `ray` opens the menu for: the target, the menu variant and whether Dock is enabled. None: nothing to open.
fn pick_target(
    scene: &MenuScene,
    docks: &Dockees,
    ray: Ray3d,
    actor: Entity,
    player_class: usize,
    player_at: &Transform,
    bar_target: Option<Entity>,
) -> Option<(Entity, Kind, bool)> {
    let mission = scene.mission.as_deref();
    let nearest = |side_ok: &dyn Fn(&Ship) -> bool, skip: Option<Entity>| {
        scene
            .ships
            .iter()
            .filter(|(e, _, sh, h)| Some(*e) != skip && side_ok(sh) && !h.destroyed && scene.sensors.player_sees(*e, mission))
            .filter_map(|(e, tf, sh, _)| ray_hit(&scene.lib.classes[sh.class], tf, ray).map(|t| (e, t)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    };
    let ally = nearest(&|sh| sh.side == Side::Player, Some(actor));
    let enemy = nearest(&|sh| sh.side != Side::Player, None);
    let own = ray_hit(&scene.lib.classes[player_class], player_at, ray);
    if let Some(e) = bar_target {
        if !scene.fleets.roster().contains(&e) {
            return None;
        }
        Some((e, if e == actor { Kind::Selected } else { Kind::Friendly }, docks.can_dock(actor, e)))
    } else if own.is_some() {
        Some((actor, Kind::Selected, false))
    } else if let Some((e, ok)) = docks.under(ray, actor) {
        Some((e, Kind::Friendly, ok))
    } else if let Some((e, _)) = ally {
        Some((e, Kind::Friendly, false)) // any other friendly ship: Focus, Tow and a greyed Dock
    } else {
        let hostile = [enemy, docks.hostile_base_hit(ray, player_at.translation).map(|(e, t, _)| (e, t))].into_iter().flatten().min_by(|a, b| a.1.total_cmp(&b.1));
        // An asteroid, space animal or island in front of any enemy: the friendly variant, Focus with Tow and Dock greyed.
        // PROVISIONAL: from play (the original opens a menu on them); which variant and slots it shows for scenery is not decoded.
        match (hostile, scenery_hit(scene.scenery.iter(), |e| scene.sensors.player_sees(e, mission), ray)) {
            (Some(h), Some(s)) if s.1 < h.1 && s.0 != h.0 => Some((s.0, Kind::Friendly, false)),
            (Some((e, _)), _) => Some((e, Kind::Enemy, false)), // an enemy ship or a hostile base: same menu
            (None, s) => s.map(|(e, _)| (e, Kind::Friendly, false)),
        }
    }
}

/// The state of the Board, Grapple, Hold and Cloak slots for `actor` and the menu's `target`.
fn slot_hooks(scene: &MenuScene, boarding: &MenuBoarding, actor: Entity, target: Entity, player_class: usize) -> Hooks {
    let mission = scene.mission.as_deref();
    let (held, cloak) = scene.flags.get(actor).map(|(h, c)| (h, c.map(|c| (c.stop_shown(), c.stop_shown() || c.can_start())))).unwrap_or_default();
    let cloak = cloak.or_else(|| scene.lib.classes[player_class].cloakable.then_some((false, false)));
    let b = &boarding.boarders;
    let mut hooks = if scene.ships.get(target).is_err() {
        // A base: boarding goes by range, there is nothing to grapple.
        let on = b.boarding(actor, target);
        Hooks { board: (on, on || b.can_board(actor, target)), ..default() }
    } else if Some(actor) == scene.fleets.primary() {
        Hooks { board: boarding.status.board(target), grapple: boarding.status.grapple(target), ..default() }
    } else {
        let on = b.boarding(actor, target);
        let grappling = b.grappling(actor, target);
        Hooks { board: (on, on || b.can_board(actor, target)), grapple: (grappling, grappling || b.can_grapple(actor)), ..default() }
    };
    (hooks.hold, hooks.cloak) = (held, cloak);
    hooks.focus_unavailable = target != actor && !scene.sensors.in_sight(actor, target, mission);
    hooks
}

#[expect(clippy::too_many_arguments, reason = "a Bevy system; the inputs are bundled as far as the borrows allow")]
fn menu(
    mut input: MenuInput,
    scene: MenuScene,
    mut boarding: MenuBoarding,
    mut bar: ResMut<BarMenuRequest>,
    mut target: ResMut<crate::combat::Target>,
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform)>,
    hud: Option<Res<HudData>>,
    state: Res<OptionsMenu>,
    mut local: Local<Local2>,
    mut commands: Commands,
    mut orbit: ResMut<Orbit>,
    mut docks: Dockees,
    mut sfx: MessageWriter<Sfx>,
    mut orders: Orders,
) {
    let mission = scene.mission.as_deref();
    let Some(actor) = scene.fleets.selected else { return };
    let Ok((e, source, tf)) = scene.actors.get(actor) else { return };
    let player = (e, source, tf);
    let Local2 { open: open_menu, press, hover } = &mut *local;
    if open_menu.as_ref().is_some_and(|o| o.actor != actor || commands.get_entity(o.root).is_err() || !scene.sensors.player_sees(o.target, mission)) {
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
    if crate::panels::cinematic(mission) || crate::rules::locked(mission) {
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
    if open_menu.is_some() && (input.mouse.just_pressed(MouseButton::Left) || input.keys.just_pressed(KeyCode::Escape)) {
        let open = open_menu.take().unwrap();
        input.keys.clear_just_pressed(KeyCode::Escape); // this Esc closed the menu; it must not also open the in-game menu
        state.open.store(false, Ordering::Relaxed);
        commands.entity(open.root).despawn();
        if !input.mouse.just_pressed(MouseButton::Left) {
            return;
        }
        state.swallow.store(true, Ordering::Relaxed);
        let act = slot.map(|i| open.items[i].0);
        let act = match act {
            Some(Act::Focus) if open.target != actor && !scene.sensors.in_sight(actor, open.target, mission) => Some(Act::Unavailable),
            act => act,
        };
        let (sound, msg) = run_slot(act, open.target, actor, &mut orders, &mut commands, &mut orbit, &mut docks);
        if let Some(msg) = msg {
            boarding.board.write(msg);
        }
        sfx.write(Sfx::ui(sound));
        return;
    }
    let Some(cursor) = cursor else { return };
    if input.mouse.just_pressed(MouseButton::Right) {
        *press = Some((cursor, input.time.elapsed_secs()));
    }
    if input.mouse.pressed(MouseButton::Right) && input.motion.delta.length() > CLICK_PX {
        *press = None;
    }
    let bar_target = bar.0.take();
    if bar_target.is_none() {
        if input.ui.0 {
            *press = None;
            return;
        }
        if !input.mouse.just_released(MouseButton::Right) {
            return;
        }
        let Some(pressed) = press.take() else { return };
        if !is_tap(pressed, cursor, input.time.elapsed_secs()) {
            return;
        }
        if crate::panels::cinematic(mission) {
            return;
        }
        // A right click while a lob bank is armed only cancels it (no menu).
        if target.2.armed.take().is_some() {
            sfx.write(Sfx::ui("HUD_LobWeapon_Cancel"));
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
    let Some((target, kind, can_dock)) = pick_target(&scene, &docks, ray, actor, player.1.class, player.2, bar_target) else { return };
    // The player's ship already towing this one gets Stop; any other ship gets Start, enabled for ships when a tow can be made.
    let tow = if docks.towee_of(player.0) == Some(target) { Tow::Stop } else { Tow::Start(docks.can_tow() && docks.towable(target) && scene.ships.get(target).is_ok()) };
    let hooks = slot_hooks(&scene, &boarding, actor, target, player.1.class);
    let (root, rects) = spawn_menu(&mut commands, &hud, &window, cursor, kind, can_dock, tow, hooks);
    *open_menu = Some(Open { actor, target, root, items: rects });
    *hover = None;
    sfx.write(Sfx::ui("HUD_Options_Open"));
    state.open.store(true, Ordering::Relaxed);
}

/// What a view draws for an open menu (on its root): the menu centre in window px, the variant's four background corners and each
/// slot as (sprite, enabled, offset from the centre in reference px).
#[derive(Component)]
pub struct OptionsMenuArt {
    pub centre: Vec2,
    pub background: [&'static str; 4],
    pub slots: Vec<(&'static str, bool, Vec2)>,
}

/// Spawns the menu root around `cursor` with its `OptionsMenuArt`; hit rects are the retail icons at the retail slot offsets.
fn spawn_menu(commands: &mut Commands, hud: &HudData, window: &Window, cursor: Vec2, kind: Kind, can_dock: bool, tow: Tow, hooks: Hooks) -> (Entity, Vec<(Act, Rect, bool)>) {
    let s = window.height() / REF_H;
    let list = items(kind, can_dock, tow, hooks);
    let n = list.len();
    let centre = menu_centre(window, cursor);
    let mut rects = vec![];
    let mut slots = vec![];
    for (i, (act, sprite, on)) in list.into_iter().enumerate() {
        let Some(size) = size_of(hud, sprite) else { continue };
        let at = offset(hud, &format!("OPTIONS_MENU_SLOT_OFFSET_{i}_OF_{n}"));
        let min = centre + at * s;
        rects.push((act, Rect::from_corners(min, min + size * s), on));
        slots.push((sprite, on, at));
    }
    let root = commands.spawn((MenuUi, crate::Scenario, OptionsMenuArt { centre, background: background(kind), slots })).id();
    (root, rects)
}

fn menu_centre(window: &Window, cursor: Vec2) -> Vec2 {
    let s = window.height() / REF_H;
    // PROVISIONAL: the 70 px margin the menu keeps from the window edge is eyeballed.
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
            let size = size_of(&hud, "OPTIONS_MENU_BUTTON_DOCK").expect("dock icon");
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
fn dev_menu(cfg: Res<crate::Config>, window: Single<&Window, With<PrimaryWindow>>, hud: Option<Res<HudData>>, mut commands: Commands) {
    let Some(hud) = hud else { return };
    let kind = match cfg.script.as_deref() {
        Some("menu-selected") => Kind::Selected,
        Some("menu-friendly") => Kind::Friendly,
        Some("menu-enemy") => Kind::Enemy,
        _ => return,
    };
    if cfg.frame == 30 {
        spawn_menu(&mut commands, &hud, &window, Vec2::new(window.width(), window.height()) / 2.0, kind, true, Tow::Start(true), Hooks::default());
    }
}

/// Picks the context cursor (`HoverCursor`) while hovering a ship or base; the system pointer is hidden then.
fn context_cursor(
    window: Single<&Window, With<PrimaryWindow>>,
    mut pointer: Single<&mut CursorOptions, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform)>,
    hud: Option<Res<HudData>>,
    lib: Res<Library>,
    menu: Res<OptionsMenu>,
    ui: Res<crate::panels::UiClick>,
    mission: Option<Res<crate::campaign::Mission>>,
    ships: Query<(Entity, &Transform, &Ship, &Health)>,
    objects: Query<
        (Entity, &Transform, Option<&crate::combat::Asteroid>, Option<&crate::environment::SpaceAnimal>),
        Or<(With<crate::combat::Asteroid>, With<crate::environment::SpaceAnimal>)>,
    >,
    sensors: Res<crate::environment::Sensors>,
    fleets: Res<crate::ship::Fleets>,
    docks: Dockees,
    mut hover: ResMut<HoverCursor>,
    (target, guns, mut lob): (Res<crate::combat::Target>, Query<&crate::combat::GunStates>, ResMut<crate::combat::LobAim>),
) {
    let mut aim = None;
    let want = (|| {
        hud.as_ref()?;
        if crate::panels::cinematic(mission.as_deref()) || menu.open.load(Ordering::Relaxed) || ui.0 {
            return None;
        }
        let cursor = window.cursor_position()?;
        let (camera, gt) = *cam;
        let ray = camera.viewport_to_world(gt, cursor).ok()?;
        let actor = fleets.selected?;
        // Armed lob bank: its cursor shows over everything (ships, bases, sea) while the pointer is on the sea plane.
        if let (true, Some(bank), Ok((_, tf, sh, health)), Ok(g)) = (fleets.direct(), target.2.armed, ships.get(actor), guns.get(actor)) {
            let class = &lib.classes[sh.class];
            let kind = class.mounts.iter().enumerate().find(|(_, m)| m.bank == bank).and_then(|(i, _)| crate::combat::lob_kind(lib.guns[g.gun(i)].bullet_family));
            let point = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y)).map(|t| ray.get_point(t));
            if let (Some(kind), Some(point)) = (kind, point) {
                let a = crate::combat::LobAimed { kind, solution: crate::combat::lob_solution(&lib, class, health, g, tf, bank, point), point };
                aim = Some(a);
                return Some((a.cursor(), cursor));
            }
        }
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
        } else if docks.hostile_base_hit(ray, ships.get(actor).ok()?.1.translation).is_some()
            || crate::view::object_hit(objects.iter(), ray, &sensors, mission.as_deref()).is_some()
        {
            if fleets.direct() { "Cursor_PrimaryShip_AttackShip" } else { "Cursor_SecondaryShip_AttackShip" }
        } else if let Some((_, _, sh, _)) =
            ships.iter().find(|(e, tf, sh, h)| !h.destroyed && sensors.player_sees(*e, mission.as_deref()) && ray_hit(&lib.classes[sh.class], tf, ray).is_some())
        {
            // Retail hover: attack an enemy; any other friendly ship that is not dockable is "can't do anything".
            if sh.side != Side::Player {
                if fleets.direct() { "Cursor_PrimaryShip_AttackShip" } else { "Cursor_SecondaryShip_AttackShip" }
            } else {
                "Cursor_Cant_Do_Anything_To_WorldObject_Under_Cursor"
            }
        } else {
            return None;
        };
        Some((name, cursor))
    })();
    pointer.visible = want.is_none();
    if lob.0 != aim {
        lob.0 = aim;
    }
    if hover.0 != want {
        hover.0 = want;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_right_click_is_a_short_press_that_stays_put() {
        let (at, t0) = (Vec2::new(100.0, 100.0), 10.0);
        assert!(is_tap((at, t0), at + Vec2::new(3.0, 0.0), t0 + 0.2));
        assert!(!is_tap((at, t0), at + Vec2::new(8.0, 0.0), t0 + 0.2), "moved: a camera drag");
        assert!(!is_tap((at, t0), at, t0 + 0.5), "held: a camera drag");
    }

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

    /// A right click on an island or a space animal finds it by its hull or its sphere; one the player does not see is passed over.
    #[test]
    fn a_right_click_finds_islands_and_animals_the_player_sees() {
        let island = crate::collision::Obstacle::from_triangles(&[[Vec3::new(-10.0, 0.0, -10.0), Vec3::new(10.0, 0.0, -10.0), Vec3::new(0.0, 0.0, 10.0)]]);
        let whale = crate::environment::SpaceAnimal::new("Whale", 3.0, Vec3::ZERO, 1, true);
        let (at_island, at_whale) = (Transform::default(), Transform::from_xyz(40.0, 0.0, 0.0));
        let (e_island, e_whale) = (Entity::from_raw_u32(1).unwrap(), Entity::from_raw_u32(2).unwrap());
        let spots = || [(e_island, &at_island, None, None, Some(&island)), (e_whale, &at_whale, None, Some(&whale), None)].into_iter();
        let down = |x: f32| Ray3d::new(Vec3::new(x, 50.0, 0.0), Dir3::NEG_Y);
        assert_eq!(scenery_hit(spots(), |_| true, down(0.0)).map(|h| h.0), Some(e_island));
        assert_eq!(scenery_hit(spots(), |_| true, down(40.0)).map(|h| h.0), Some(e_whale));
        assert_eq!(scenery_hit(spots(), |e| e != e_whale, down(40.0)), None, "unseen");
        assert_eq!(scenery_hit(spots(), |_| true, down(80.0)), None, "open space");
    }
}
