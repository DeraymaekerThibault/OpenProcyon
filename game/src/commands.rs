//! Player orders as data (#369 Phase 1): the one path from the player's input to the simulation, and command-log replays.
//!
//! Input systems do not change the battle themselves: they `send` a [`PlayerCommand`] ([`Orders`]), and [`apply_tick`] applies it at
//! the start of the next battle tick (`BattleSet::Tick`, after `tick_start`), which is where an input's effect landed before (the input
//! wrote the ship in the frame, the next tick read it). While the battle is paused no tick runs, so a command sent then is applied at
//! the end of its frame instead ([`apply_paused`]), as the order dial and the target marker answered at once before.
//!
//! Recording (`PROCYON_COMMANDS=<file>`, like `PROCYON_TRACE`) writes JSON lines: a header (format, game version, seed, scenario
//! arguments, mod folders and, for a battle launched from the skirmish lobby, the lobby's whole setup: `menu::SkirmishSetup`), written
//! once the battle has loaded, then the battle's frames (each frame's game-time advance in nanoseconds and how many ticks ran in it,
//! run-length packed), and the commands stamped with the tick (or, while paused, the frame) they were applied on. Ids only (`NetId`:
//! object kind and `SpawnIndex`), never names or strings from the game files.
//!
//! `--replay <file>` plays a recording back from a normal session, not only a `--fixed-step` one: once the battle has loaded it feeds
//! the recorded frame advances to Bevy's clock (`TimeUpdateStrategy::ManualDuration`) and runs exactly the recorded ticks in each
//! frame, so the frame-driven rules (`FrameRules`) and the ticks see the same clock as when the log was made, and the recorded
//! commands go in on their ticks while live input is dropped. The scenario arguments, seed and mods must match the recording (the
//! game says which to pass when they do not). A recording that holds a skirmish setup launches it through the lobby's Launch path by
//! itself (`--replay <file>` alone, however the lobby was set up); campaign missions started from the menus are still refused. Only
//! the first battle of a run is recorded; a restart ends the recording.
//!
//! Routed so far: engine orders, target picks, move, course and cancel orders, the right-click menu's Stop, Hold, Cloak, Dock,
//! Tow, Board and Grapple, Backspace's dock and grapple cancels, the Fleet Commands (formations, attack, cease fire), P,
//! panic fire and indirect (lob) shots. The other orders still change the battle directly and are not in a recording yet
//! (later PRs, one group each). PROVISIONAL: the file format (version 2; version 1, without the skirmish setup, still reads).
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::time::Duration;

use crate::{BATTLE_TICK, EngineOrder, Ship};

/// Version of the command-log format. Bump on any change a reader of the old format would misread.
pub const FORMAT: u32 = 2;
/// The oldest format this game still reads.
const OLDEST_FORMAT: u32 = 1;

/// The player who gave an order: a lobby slot. Single-player has one, 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PlayerId(pub u8);

/// The simulation's mark of a human's command ship: the ship that player commands directly (its engines, its own target, panic fire,
/// indirect shots), where an order to any other ship of the fleet is a captain's order. Battle setup and `ship::reconcile_fleets` set
/// it, one per human player; `PlayerShip` (the local player's ship, for the view) follows it on the local player's ship (the hooks
/// here). A world built by hand that marks only `PlayerShip` (tests, smokes) gets `Commander` of the local player (`ship.rs`).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[component(on_insert = commander_set, on_remove = commander_removed)]
#[require(crate::combat::ManualFire)]
pub struct Commander(pub PlayerId);

fn commander_set(mut world: bevy::ecs::world::DeferredWorld, ctx: bevy::ecs::lifecycle::HookContext) {
    let local = world.get::<Commander>(ctx.entity).is_some_and(|c| c.0 == local_player(&world));
    let mut commands = world.commands();
    let mut e = commands.entity(ctx.entity);
    if local {
        e.try_insert(crate::PlayerShip);
    } else {
        e.try_remove::<crate::PlayerShip>();
    }
}
fn commander_removed(mut world: bevy::ecs::world::DeferredWorld, ctx: bevy::ecs::lifecycle::HookContext) {
    world.commands().entity(ctx.entity).try_remove::<crate::PlayerShip>();
}

/// One player of the battle: the fleet it commands (`ship::Fleets` key), its side, and whether a person plays it.
#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    pub owner: crate::names::Owner,
    pub side: crate::Side,
    pub human: bool,
}

/// The battle's players by id (#369 phase 2), built at battle setup where `Fleets.local` is set, and which one this machine plays.
/// Skirmish: one per lobby slot in the battle (command line: the human 0, the enemy 1). Campaign and the test arena: the human, 0.
#[derive(Resource, Default, Debug, Clone)]
pub struct Players {
    pub list: std::collections::BTreeMap<PlayerId, Player>,
    pub local: PlayerId,
}

impl Players {
    /// A battle with one human, player 0, commanding `owner`'s fleet (campaign missions, the test arena).
    pub fn single(world: &mut World, owner: crate::names::Owner) {
        let list = [(PlayerId(0), Player { owner, side: crate::Side::Player, human: true })].into();
        world.insert_resource(Players { list, local: PlayerId(0) });
    }
}

/// The player this machine plays (0 without a table).
pub fn local_player(world: &World) -> PlayerId {
    world.get_resource::<Players>().map_or_else(PlayerId::default, |p| p.local)
}

/// The fleet `player` commands: its table entry, else (a world without one: unit tests, a battle before setup) the local fleet for the
/// local player, none for any other.
pub fn fleet_of(world: &World, player: PlayerId) -> Option<crate::names::Owner> {
    match world.get_resource::<Players>().and_then(|p| p.list.get(&player)) {
        Some(p) => Some(p.owner.clone()),
        None if player == local_player(world) => world.get_resource::<crate::ship::Fleets>().map(|f| f.local.clone()),
        None => None,
    }
}

/// The human players and their fleets, by id. A world without a table has one: the local player with the local fleet.
pub fn humans(world: &World) -> Vec<(PlayerId, crate::names::Owner)> {
    let listed: Vec<_> = world.get_resource::<Players>().into_iter().flat_map(|p| p.list.iter()).filter(|(_, p)| p.human).map(|(id, p)| (*id, p.owner.clone())).collect();
    if !listed.is_empty() {
        return listed;
    }
    let local = local_player(world);
    fleet_of(world, local).map(|owner| (local, owner)).into_iter().collect()
}

/// An engine order: a step up or down the dial (the speed keys) or straight to one setting (the dial, its tabs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    Faster,
    Slower,
    To(EngineOrder),
}

/// What a command orders.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// The ship's engine order (only the player's command ship takes these from the UI).
    Engine { ship: Entity, engine: Engine },
    /// The ship's target: on the command ship the player's own target and the spot clicked on it (target-local), which also ends panic
    /// fire; on a fleet captain an attack order. `None` clears the command ship's target (C); a captain ignores it.
    Target { ship: Entity, target: Option<Entity>, spot: Option<Vec3> },
    /// A click on open water (basic movement): the command ship sails there (`append`, Shift held: one more waypoint; light-green
    /// Full if it was stopped), a captain is ordered there. Either way a dock routine in progress and a hold end.
    Move { ship: Entity, to: Vec3, append: bool },
    /// Advanced movement: the plotted course becomes the command ship's (light-green Full if it was stopped). `release`: the click
    /// that closed the course also ends a dock routine and a hold, as a move does; Enter does not.
    Course { ship: Entity, points: Vec<Vec3>, release: bool },
    /// Backspace with no course being plotted: the ship's waypoints and grapple order go, and with them the armed bank and its shot
    /// (command ship), or the captain's destination, target and hold, and its engines.
    Cancel { ship: Entity },
    /// The right-click menu's Stop: the ship's course, heading goal and engines end (`Ship::stand_still`), a captain's destination
    /// and target too (the formation hold and hold fire stay), and so does a dock routine in progress.
    Stop { ship: Entity },
    /// The menu's Hold (`on`) and its release: a held ship stands still where it is and keeps firing (`ship::HoldPosition`).
    Hold { ship: Entity, on: bool },
    /// The menu's Cloak Start / Stop, for a ship that can cloak (`cloak::Cloak`).
    Cloak { ship: Entity, on: bool },
    /// Dock with `dockee` (a left click on it, or the menu's Dock). `release`: the menu's Dock also ends a hold; the click does not.
    /// PROVISIONAL: kept as the engine did it; whether the original's left-click dock also ends a hold is not observed.
    Dock { ship: Entity, dockee: Entity, release: bool },
    /// The menu's Tow Start (`Some`, which ends a hold) and Tow Stop (`None`). Tows are made in campaign missions only.
    Tow { ship: Entity, towee: Option<Entity> },
    /// The menu's Board Start (`Some`, which ends a hold) and Board Stop (`None`).
    Board { ship: Entity, target: Option<Entity> },
    /// The menu's Grapple Start (`Some`) and Grapple Stop (`None`).
    Grapple { ship: Entity, target: Option<Entity> },
    /// Backspace also ends the ship's dock routine in progress and its tow approach (`docking::DockMsg::CancelShip`), even while a
    /// course is being plotted.
    CancelDock { ship: Entity },
    /// Backspace also drops the command ship's grapple order, even while a course is being plotted.
    DropGrapple { ship: Entity },
    /// A Fleet Commands formation button: the fleet takes `formation`; `hold` (the button was not the active formation) has the
    /// captains hold it, otherwise they leave it (`ship::set_formation`).
    Formation { formation: crate::ship::Formation, hold: bool },
    /// Fleet Commands Attack: the captains attack the command ship's target (`ship::fleet_help`).
    FleetAttack,
    /// Fleet Commands Cease Fire: the fleet's hold fire toggles.
    CeaseFire,
    /// P (Change Primary, skirmish only): `ship` becomes the fleet's command ship (`ship::promote`).
    Promote { ship: Entity },
    /// Panic fire (X, or the panic button): every gun of the command ship picks its own nearest enemy until the player retargets.
    PanicFire { ship: Entity },
    /// The command ship's indirect shot: `Some((bank, point))` is the ground point a click gave the armed bank; `None` drops a shot
    /// not fired yet (a click on a gun-bank icon). Which bank is armed is the player's own input state, not an order.
    Lob { ship: Entity, shot: Option<(usize, Vec3)> },
}

/// One player order.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerCommand {
    pub player: PlayerId,
    pub kind: Kind,
}

/// Commands sent this frame and not applied yet.
#[derive(Resource, Default)]
pub struct Outbox(Vec<PlayerCommand>);

/// What input systems take to give orders.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Orders<'w> {
    outbox: ResMut<'w, Outbox>,
    players: Option<Res<'w, Players>>,
}

impl Orders<'_> {
    /// An order from the local player.
    pub fn send(&mut self, kind: Kind) {
        let player = self.players.as_ref().map_or_else(PlayerId::default, |p| p.local);
        self.outbox.0.push(PlayerCommand { player, kind });
    }
}

/// The battle's frame and tick count since it finished loading, the clock recordings and replays are stamped with.
#[derive(Resource, Default, Debug)]
pub struct Clock {
    /// A battle has loaded and is running (frames and ticks count).
    active: bool,
    /// Battles of this run that have ended (left or restarted); 0 during the first. Only the first is recorded or replayed, but every
    /// battle counts its own frames and ticks, so orders work the same in all of them.
    battle: u32,
    frame: u32,
    tick: u32,
    frame_ticks: u32,
}

/// Applies one command. Validation stays what the UI already checks (#369 design: single-player behaviour does not change); a command
/// whose ship is gone does nothing.
pub fn apply(world: &mut World, player: PlayerId, kind: &Kind) {
    match *kind {
        Kind::Move { .. } | Kind::Course { .. } | Kind::Cancel { .. } => apply_movement(world, player, kind),
        Kind::Stop { .. } | Kind::Hold { .. } | Kind::Cloak { .. } => apply_menu(world, kind),
        Kind::Dock { .. } | Kind::Tow { .. } | Kind::Board { .. } | Kind::Grapple { .. } | Kind::CancelDock { .. } | Kind::DropGrapple { .. } => apply_links(world, kind),
        Kind::Formation { formation, hold } => {
            if let Some(owner) = fleet_of(world, player) {
                crate::ship::set_formation(world, &owner, Some(formation), hold);
            }
        }
        Kind::FleetAttack | Kind::CeaseFire => {
            if let Some(owner) = fleet_of(world, player) {
                crate::ship::fleet_help(world, &owner, *kind == Kind::FleetAttack);
            }
        }
        Kind::Promote { ship } => {
            crate::ship::promote(world, player, ship, false);
        }
        Kind::PanicFire { ship } => {
            if direct(world, ship, player)
                && let Some(mut f) = world.get_mut::<crate::combat::ManualFire>(ship)
            {
                f.panic = true;
            }
        }
        Kind::Lob { ship, shot } => {
            if direct(world, ship, player)
                && let Some(mut f) = world.get_mut::<crate::combat::ManualFire>(ship)
            {
                f.shot = shot;
            }
        }
        Kind::Engine { ship, engine } => {
            if let Some(mut s) = world.get_mut::<Ship>(ship) {
                s.throttle = None;
                s.order = match engine {
                    Engine::Faster => s.order.faster(),
                    Engine::Slower => s.order.slower(),
                    Engine::To(order) => order,
                };
            }
        }
        Kind::Target { ship, target, spot } => {
            if direct(world, ship, player) {
                // The attack click writes the command ship's own target slot (the original keeps no per-player target).
                if let Some(mut slot) = world.get_mut::<crate::ship::TargetSlot>(ship) {
                    *slot = crate::ship::TargetSlot::order(target, spot);
                }
                if let Some(mut f) = world.get_mut::<crate::combat::ManualFire>(ship) {
                    f.panic = false;
                }
            } else if let Some(target) = target
                && attackable(world, ship, target)
                && let Some(mut p) = world.get_mut::<crate::ship::FleetPilot>(ship)
            {
                p.attack(target);
                crate::ship::aggressive(world, ship);
            }
        }
    }
}

/// A captain's attack order stands only against a thing hostile to its ship (an order against an ally or a thing of no player is
/// dropped); a target without a side (a rock, an animal) stands.
fn attackable(world: &World, ship: Entity, target: Entity) -> bool {
    let table = world.get_resource::<crate::alliance::Alliances>().unwrap_or(crate::alliance::empty());
    let side = |e: Entity| world.get::<Ship>(e).map(|s| s.side).or_else(|| table.base_side(world.get(e), world.get(e)));
    side(ship).zip(side(target)).is_none_or(|(a, b)| table.hostile(a, b))
}

/// Direct control: `ship` is the command ship of the player who gave the order (`Commander`); an order to any other ship is a
/// captain's order (`Fleets::direct`, what the input handlers decided on, for the local player).
fn direct(world: &World, ship: Entity, player: PlayerId) -> bool {
    world.get::<Commander>(ship).is_some_and(|c| c.0 == player)
}

/// A move ends a dock routine in progress and gives control back (original behaviour), and ends a hold (`ship::release_hold`).
fn release(world: &mut World, ship: Entity) {
    dock_msg(world, crate::docking::DockMsg::CancelShip(ship));
    release_hold(world, ship);
}

/// A move, dock, tow or board order ends a hold (`ship::release_hold`).
fn release_hold(world: &mut World, ship: Entity) {
    if let Ok(mut e) = world.get_entity_mut(ship) {
        e.remove::<crate::ship::HoldPosition>();
    }
}

/// Docking and towing act on their messages (`docking::handle_messages`, later in the same tick).
fn dock_msg(world: &mut World, msg: crate::docking::DockMsg) {
    if let Some(mut msgs) = world.get_resource_mut::<Messages<crate::docking::DockMsg>>() {
        msgs.write(msg);
    }
}

/// Boarding and grappling act on their messages (`boarding::fight::handle_messages`, later in the same tick).
fn board_msg(world: &mut World, msg: crate::boarding::BoardMsg) {
    if let Some(mut msgs) = world.get_resource_mut::<Messages<crate::boarding::BoardMsg>>() {
        msgs.write(msg);
    }
}

/// A stopped ship given a course sails at light-green Full, as in the original.
fn sail_on(ship: &mut Ship) {
    if ship.order == EngineOrder::Stop || ship.throttle == Some(0.0) {
        ship.throttle = None;
        ship.order = EngineOrder::Full;
    }
}

/// Move, course and cancel orders.
fn apply_movement(world: &mut World, player: PlayerId, kind: &Kind) {
    match kind {
        &Kind::Move { ship, to, append } => {
            release(world, ship);
            if direct(world, ship, player) {
                if let Some(mut s) = world.get_mut::<Ship>(ship) {
                    if !append {
                        s.waypoints.clear();
                    }
                    s.waypoints.push(to);
                    sail_on(&mut s);
                }
            } else if let Some(mut p) = world.get_mut::<crate::ship::FleetPilot>(ship) {
                p.move_to(to);
            }
        }
        Kind::Course { ship, points, release: ends } => {
            if *ends {
                release(world, *ship);
            }
            if let Some(mut s) = world.get_mut::<Ship>(*ship) {
                s.waypoints.clone_from(points);
                sail_on(&mut s);
            }
        }
        &Kind::Cancel { ship } => {
            let direct = direct(world, ship, player);
            if let Ok(mut e) = world.get_entity_mut(ship) {
                e.remove::<crate::boarding::GrappleOrder>();
            }
            if let Some(mut s) = world.get_mut::<Ship>(ship) {
                s.waypoints.clear();
                if !direct {
                    s.order = EngineOrder::Stop;
                }
            }
            if direct {
                // (The view disarms its armed bank itself when it sends the cancel.)
                if let Some(mut f) = world.get_mut::<crate::combat::ManualFire>(ship) {
                    f.shot = None;
                }
            } else if let Some(mut p) = world.get_mut::<crate::ship::FleetPilot>(ship) {
                p.destination = None;
                p.target = None;
                p.hold = false;
            }
        }
        _ => {}
    }
}

/// Stop, hold and cloak orders (the right-click menu).
fn apply_menu(world: &mut World, kind: &Kind) {
    match *kind {
        Kind::Stop { ship } => {
            if let Some(mut s) = world.get_mut::<Ship>(ship) {
                s.stand_still();
            }
            if let Some(mut p) = world.get_mut::<crate::ship::FleetPilot>(ship) {
                p.cancel_actions();
            }
            dock_msg(world, crate::docking::DockMsg::CancelShip(ship));
        }
        Kind::Hold { ship, on: true } => {
            let Ok(mut e) = world.get_entity_mut(ship) else { return };
            e.insert(crate::ship::HoldPosition);
            if let Some(mut s) = e.get_mut::<Ship>() {
                s.stand_still();
            }
            if let Some(mut p) = e.get_mut::<crate::ship::FleetPilot>() {
                p.hold_position();
            }
        }
        Kind::Hold { ship, on: false } => {
            if let Ok(mut e) = world.get_entity_mut(ship) {
                e.remove::<crate::ship::HoldPosition>();
            }
        }
        Kind::Cloak { ship, on } => {
            let side = world.get::<Ship>(ship).map(|s| s.side);
            let table = world.get_resource::<crate::alliance::Alliances>().cloned().unwrap_or_default();
            if let (Some(mut c), Some(side)) = (world.get_mut::<crate::cloak::Cloak>(ship), side) {
                if on {
                    c.start(side, &table);
                } else {
                    c.stop();
                }
            }
        }
        _ => {}
    }
}

/// Dock, tow, board and grapple orders, and Backspace's dock and grapple cancels.
fn apply_links(world: &mut World, kind: &Kind) {
    use crate::boarding::BoardMsg;
    use crate::docking::DockMsg;
    match *kind {
        Kind::Dock { ship, dockee, release } => {
            if release {
                release_hold(world, ship);
            }
            dock_msg(world, DockMsg::Dock { docker: Some(ship), dockee });
        }
        Kind::Tow { ship, towee: Some(towee) } => {
            release_hold(world, ship);
            dock_msg(world, DockMsg::TowStart { tower: Some(ship), towee });
        }
        Kind::Tow { ship, towee: None } => dock_msg(world, DockMsg::TowBreak { tower: Some(ship) }),
        Kind::Board { ship, target: Some(target) } => {
            release_hold(world, ship);
            board_msg(world, BoardMsg::Board { boarder: Some(ship), target });
        }
        Kind::Board { ship, target: None } => board_msg(world, BoardMsg::Unboard { boarder: Some(ship) }),
        Kind::Grapple { ship, target: Some(target) } => board_msg(world, BoardMsg::Grapple { grappler: Some(ship), target }),
        Kind::Grapple { ship, target: None } => board_msg(world, BoardMsg::Ungrapple { grappler: Some(ship) }),
        Kind::CancelDock { ship } => dock_msg(world, DockMsg::CancelShip(ship)),
        Kind::DropGrapple { ship } => {
            if let Ok(mut e) = world.get_entity_mut(ship) {
                e.remove::<crate::boarding::GrappleOrder>();
            }
        }
        _ => {}
    }
}

/// Start of a frame, before the fixed loop: counts the battle's frames, and in a replay makes the fixed loop run the recorded ticks.
fn frame_start(world: &mut World) {
    let live = world.get_resource::<State<crate::menu::Screen>>().is_some_and(|s| *s.get() == crate::menu::Screen::Battle) && !world.contains_resource::<crate::loading::Loading>();
    let mut clock = world.resource_mut::<Clock>();
    if !live {
        if clock.active {
            clock.active = false;
            clock.battle += 1;
        }
        world.resource_mut::<Outbox>().0.clear();
        return;
    }
    let starts = !clock.active;
    if clock.active {
        clock.frame += 1;
    } else {
        *clock = Clock { active: true, battle: clock.battle, ..default() };
    }
    clock.frame_ticks = 0;
    let (frame, first) = (clock.frame, clock.battle == 0);
    if starts && first {
        write_header(world);
    }
    let Some(replay) = world.get_resource::<Replay>().filter(|r| first && !r.finished) else { return };
    if let Some(&(advance, ticks)) = replay.frames.get(frame as usize) {
        // Exactly `ticks` ticks run this frame: with the overstep at max(0, ticks x tick - advance), the loop fits floor(max(ticks x
        // tick, advance) / tick) of them, and the recording's advance never held a further whole tick (its overstep was below one).
        let mut fixed = world.resource_mut::<Time<Fixed>>();
        let over = fixed.overstep();
        fixed.discard_overstep(over);
        fixed.accumulate_overstep((BATTLE_TICK * ticks).saturating_sub(advance));
    }
}

/// First thing in a battle tick: the tick's commands (the outbox, or in a replay the recording's) go in, stamped and recorded.
pub fn apply_tick(world: &mut World) {
    let (tick, live) = {
        let clock = world.resource::<Clock>();
        (clock.tick, clock.active)
    };
    let sent = std::mem::take(&mut world.resource_mut::<Outbox>().0);
    if live {
        let mut todo = replayed(world, At::Tick(tick)).unwrap_or(sent);
        // Every machine applies a tick's commands in one order: by player, then in the order that player sent them (#369 section 3).
        todo.sort_by_key(|c| c.player);
        for c in todo {
            record(world, At::Tick(tick), &c);
            apply(world, c.player, &c.kind);
        }
        let mut clock = world.resource_mut::<Clock>();
        clock.tick += 1;
        clock.frame_ticks += 1;
    } else {
        // Not counted (a world without the frame clock: unit tests): applied as they come.
        for c in sent {
            apply(world, c.player, &c.kind);
        }
    }
}

/// End of a paused frame: no tick will run, so the frame's commands are applied now (in a replay, the recording's for this frame).
fn apply_paused(world: &mut World) {
    let clock = world.resource::<Clock>();
    if !clock.active {
        return;
    }
    let frame = clock.frame;
    let todo = match replayed(world, At::Frame(frame)) {
        Some(todo) => todo,
        None if world.resource::<Time<Virtual>>().is_paused() => std::mem::take(&mut world.resource_mut::<Outbox>().0),
        None => return,
    };
    for c in todo {
        record(world, At::Frame(frame), &c);
        apply(world, c.player, &c.kind);
    }
}

// ---- The file -------------------------------------------------------------------------------------------------------------

/// A replicated object: its kind and its creation number within the kind (`scenario::SpawnIndex`, from 0 at battle start), or for
/// map scenery without one (bases without guns, islands) its place in the map's object list (`scenario::SceneryIndex`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetId {
    pub kind: NetKind,
    pub index: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetKind {
    Ship,
    ArmedBase,
    Asteroid,
    /// A space animal (whale, dragon).
    Animal,
    /// Map scenery that is none of the above: a base without guns, an island.
    Scenery,
}

impl NetId {
    /// `e`'s id, None for an object a player cannot name (an effect, a shell).
    pub fn of(world: &World, e: Entity) -> Option<Self> {
        let numbered = world.get::<crate::scenario::SpawnIndex>(e).and_then(|i| {
            let kind = if world.get::<Ship>(e).is_some() {
                NetKind::Ship
            } else if world.get::<crate::combat::ArmedBase>(e).is_some() {
                NetKind::ArmedBase
            } else if world.get::<crate::combat::Asteroid>(e).is_some() {
                NetKind::Asteroid
            } else if world.get::<crate::environment::SpaceAnimal>(e).is_some() {
                NetKind::Animal
            } else {
                return None;
            };
            Some(Self { kind, index: i.0 })
        });
        numbered.or_else(|| world.get::<crate::scenario::SceneryIndex>(e).map(|i| Self { kind: NetKind::Scenery, index: i.0 }))
    }
    /// The entity with this id now.
    pub fn find(self, world: &mut World) -> Option<Entity> {
        let found: Vec<Entity> = if self.kind == NetKind::Scenery {
            let mut q = world.query::<(Entity, &crate::scenario::SceneryIndex)>();
            q.iter(world).filter(|(_, i)| i.0 == self.index).map(|(e, _)| e).collect()
        } else {
            let mut q = world.query::<(Entity, &crate::scenario::SpawnIndex)>();
            q.iter(world).filter(|(_, i)| i.0 == self.index).map(|(e, _)| e).collect()
        };
        found.into_iter().find(|e| NetId::of(world, *e) == Some(self))
    }
}

/// When a recorded command was applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum At {
    /// At the start of this battle tick (0 = the first).
    Tick(u32),
    /// At the end of this paused battle frame (0 = the first after loading).
    Frame(u32),
}

/// A command as the file holds it. Points are game axes (x starboard, y forward, z up), as in `PROCYON_TRACE`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Saved {
    Engine {
        ship: NetId,
        engine: SavedEngine,
    },
    Target {
        ship: NetId,
        target: Option<NetId>,
        spot: Option<[f32; 3]>,
    },
    Move {
        ship: NetId,
        to: [f32; 3],
        append: bool,
    },
    Course {
        ship: NetId,
        points: Vec<[f32; 3]>,
        release: bool,
    },
    Cancel {
        ship: NetId,
    },
    Stop {
        ship: NetId,
    },
    Hold {
        ship: NetId,
        on: bool,
    },
    Cloak {
        ship: NetId,
        on: bool,
    },
    Dock {
        ship: NetId,
        dockee: NetId,
        release: bool,
    },
    Tow {
        ship: NetId,
        towee: Option<NetId>,
    },
    Board {
        ship: NetId,
        target: Option<NetId>,
    },
    Grapple {
        ship: NetId,
        target: Option<NetId>,
    },
    CancelDock {
        ship: NetId,
    },
    DropGrapple {
        ship: NetId,
    },
    Formation {
        formation: crate::ship::Formation,
        hold: bool,
    },
    FleetAttack,
    CeaseFire,
    Promote {
        ship: NetId,
    },
    PanicFire {
        ship: NetId,
    },
    /// Bank and ground point (game axes).
    Lob {
        ship: NetId,
        shot: Option<(usize, [f32; 3])>,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedEngine {
    Faster,
    Slower,
    /// Engine order index: 0 All Stop, 1 Ahead Slow, 2 Ahead Full, 3 Emergency.
    To(u8),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Header {
    format: u32,
    version: String,
    seed: u64,
    args: Vec<String>,
    mods: Vec<String>,
    /// The battle was set up in the menus and no skirmish setup holds it (a campaign mission): neither it nor the arguments say what
    /// the battle was, so it cannot be replayed yet.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    from_menu: bool,
    /// The skirmish lobby's setup at Launch (format 2). The arguments then leave out `--lobby`: the setup replaces it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    skirmish: Option<crate::menu::SkirmishSetup>,
}

impl Header {
    /// This run's header once its battle has loaded; `skirmish` is what the lobby launched, if the lobby launched it.
    fn new(seed: u64, args: &[String], skirmish: Option<crate::menu::SkirmishSetup>) -> Self {
        let mut args = scenario_args(args);
        if skirmish.is_some()
            && let Some(i) = args.iter().position(|a| a == "--lobby")
        {
            args.drain(i..(i + 2).min(args.len()));
        }
        let from_menu = skirmish.is_none() && !sets_up_battle(&args);
        Header { format: FORMAT, version: env!("CARGO_PKG_VERSION").into(), seed, args, mods: mod_names(), from_menu, skirmish }
    }
}

/// The skirmish setup the `--replay` recording holds, if any: the run then opens the lobby and launches it. A file that cannot be
/// read gives none here; the replay itself says why (`plugin`).
pub fn recorded_setup(args: &[String]) -> Option<crate::menu::SkirmishSetup> {
    let path = args.iter().position(|a| a == "--replay").and_then(|i| args.get(i + 1))?;
    let mut first = String::new();
    // A header is one short line: a damaged file cannot make this read without end.
    std::io::BufReader::new(std::io::Read::take(std::fs::File::open(path).ok()?, 1 << 20)).read_line(&mut first).ok()?;
    serde_json::from_str::<Header>(&first).ok()?.skirmish
}

/// Whether these scenario arguments set up a battle by themselves.
fn sets_up_battle(args: &[String]) -> bool {
    args.iter().any(|a| matches!(a.as_str(), "--map" | "--lobby" | "--mission"))
}

/// Most frames a recording may hold (about six hours at 240 frames a second): a damaged file cannot ask for unbounded memory.
const MAX_FRAMES: usize = 5_000_000;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Line {
    /// Frames in order: [advance ns, ticks] or [advance ns, ticks, repeat].
    Frames(Vec<Vec<u64>>),
    Command {
        at: At,
        player: PlayerId,
        order: Saved,
    },
    /// A command the format cannot name yet (its target has no id); a replay of this file differs from the battle from there.
    Unrecorded {
        at: At,
        what: String,
    },
}

fn game_axes(v: Vec3) -> [f32; 3] {
    [v.x, -v.z, v.y]
}
fn bevy_axes(v: [f32; 3]) -> Vec3 {
    crate::data::conv(v[0], v[1], v[2])
}

impl Saved {
    /// The command as the file holds it; Err names what has no id.
    pub fn save(world: &World, kind: &Kind) -> Result<Self, &'static str> {
        let id = |e: Entity| NetId::of(world, e).ok_or("an object without an id");
        Ok(match *kind {
            Kind::Engine { ship, engine } => Saved::Engine {
                ship: id(ship)?,
                engine: match engine {
                    Engine::Faster => SavedEngine::Faster,
                    Engine::Slower => SavedEngine::Slower,
                    Engine::To(o) => SavedEngine::To(o as u8),
                },
            },
            Kind::Target { ship, target, spot } => Saved::Target { ship: id(ship)?, target: target.map(id).transpose()?, spot: spot.map(game_axes) },
            Kind::Move { ship, to, append } => Saved::Move { ship: id(ship)?, to: game_axes(to), append },
            Kind::Course { ship, ref points, release } => Saved::Course { ship: id(ship)?, points: points.iter().copied().map(game_axes).collect(), release },
            Kind::Cancel { ship } => Saved::Cancel { ship: id(ship)? },
            Kind::Stop { ship } => Saved::Stop { ship: id(ship)? },
            Kind::Hold { ship, on } => Saved::Hold { ship: id(ship)?, on },
            Kind::Cloak { ship, on } => Saved::Cloak { ship: id(ship)?, on },
            Kind::Dock { ship, dockee, release } => Saved::Dock { ship: id(ship)?, dockee: id(dockee)?, release },
            Kind::Tow { ship, towee } => Saved::Tow { ship: id(ship)?, towee: towee.map(id).transpose()? },
            Kind::Board { ship, target } => Saved::Board { ship: id(ship)?, target: target.map(id).transpose()? },
            Kind::Grapple { ship, target } => Saved::Grapple { ship: id(ship)?, target: target.map(id).transpose()? },
            Kind::CancelDock { ship } => Saved::CancelDock { ship: id(ship)? },
            Kind::DropGrapple { ship } => Saved::DropGrapple { ship: id(ship)? },
            Kind::Formation { formation, hold } => Saved::Formation { formation, hold },
            Kind::FleetAttack => Saved::FleetAttack,
            Kind::CeaseFire => Saved::CeaseFire,
            Kind::Promote { ship } => Saved::Promote { ship: id(ship)? },
            Kind::PanicFire { ship } => Saved::PanicFire { ship: id(ship)? },
            Kind::Lob { ship, shot } => Saved::Lob { ship: id(ship)?, shot: shot.map(|(bank, at)| (bank, game_axes(at))) },
        })
    }
    /// The command for this world; None when an object it names is gone.
    pub fn load(&self, world: &mut World) -> Option<Kind> {
        Some(match *self {
            Saved::Engine { ship, engine } => Kind::Engine {
                ship: ship.find(world)?,
                engine: match engine {
                    SavedEngine::Faster => Engine::Faster,
                    SavedEngine::Slower => Engine::Slower,
                    SavedEngine::To(i) => Engine::To(EngineOrder::from_index(i as usize)?),
                },
            },
            Saved::Target { ship, target, spot } => Kind::Target { ship: ship.find(world)?, target: find(world, target)?, spot: spot.map(bevy_axes) },
            Saved::Move { ship, to, append } => Kind::Move { ship: ship.find(world)?, to: bevy_axes(to), append },
            Saved::Course { ship, ref points, release } => Kind::Course { ship: ship.find(world)?, points: points.iter().copied().map(bevy_axes).collect(), release },
            Saved::Cancel { ship } => Kind::Cancel { ship: ship.find(world)? },
            Saved::Stop { ship } => Kind::Stop { ship: ship.find(world)? },
            Saved::Hold { ship, on } => Kind::Hold { ship: ship.find(world)?, on },
            Saved::Cloak { ship, on } => Kind::Cloak { ship: ship.find(world)?, on },
            Saved::Dock { ship, dockee, release } => Kind::Dock { ship: ship.find(world)?, dockee: dockee.find(world)?, release },
            Saved::Tow { ship, towee } => Kind::Tow { ship: ship.find(world)?, towee: find(world, towee)? },
            Saved::Board { ship, target } => Kind::Board { ship: ship.find(world)?, target: find(world, target)? },
            Saved::Grapple { ship, target } => Kind::Grapple { ship: ship.find(world)?, target: find(world, target)? },
            Saved::CancelDock { ship } => Kind::CancelDock { ship: ship.find(world)? },
            Saved::DropGrapple { ship } => Kind::DropGrapple { ship: ship.find(world)? },
            Saved::Formation { formation, hold } => Kind::Formation { formation, hold },
            Saved::FleetAttack => Kind::FleetAttack,
            Saved::CeaseFire => Kind::CeaseFire,
            Saved::Promote { ship } => Kind::Promote { ship: ship.find(world)? },
            Saved::PanicFire { ship } => Kind::PanicFire { ship: ship.find(world)? },
            Saved::Lob { ship, shot } => Kind::Lob { ship: ship.find(world)?, shot: shot.map(|(bank, at)| (bank, bevy_axes(at))) },
        })
    }
}

/// An optional object of a command: Some(None) when the command names none, None when the one it names is gone.
fn find(world: &mut World, id: Option<NetId>) -> Option<Option<Entity>> {
    match id {
        Some(id) => id.find(world).map(Some),
        None => Some(None),
    }
}

/// The command-line arguments that decide the battle, as a recording keeps them: without the program, the run's presentation and
/// timing (screenshots, camera, `--speed`, `--fixed-step`), scripts (a script gave the recorded commands), the replay itself, and any
/// argument that takes a path (`--mod <dir>` among them: mods are compared by folder name, and a file meant for bug reports must not
/// carry the player's folders).
pub fn scenario_args(args: &[String]) -> Vec<String> {
    let mut out = vec![];
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        // The flags of the run's presentation, timing, scripts and paths (`cli::FLAGS`, not `recorded`) are left out with their value.
        if let Some(flag) = crate::cli::flag_named(a).filter(|f| !f.recorded) {
            if flag.value {
                it.next();
            }
        } else {
            out.push(a.clone());
        }
    }
    out
}

fn mod_names() -> Vec<String> {
    crate::modding::active().iter().filter_map(|d| d.file_name().map(|n| n.to_string_lossy().into_owned())).collect()
}

#[derive(Resource)]
struct Recorder {
    out: std::io::BufWriter<std::fs::File>,
    /// The run's seed and arguments until the header goes out, once the first battle has loaded (`write_header`).
    pending: Option<(u64, Vec<String>)>,
    /// Frames not written yet, run-length packed.
    frames: Vec<Vec<u64>>,
    warned: bool,
}

impl Recorder {
    fn line(&mut self, line: &Line) {
        let _ = serde_json::to_writer(&mut self.out, line);
        let _ = self.out.write_all(b"\n");
    }
    fn frame(&mut self, advance: Duration, ticks: u32) {
        let (ns, n) = (advance.as_nanos() as u64, u64::from(ticks));
        match self.frames.last_mut() {
            Some(last) if last[0] == ns && last[1] == n => match last.get_mut(2) {
                Some(k) => *k += 1,
                None => last.push(2),
            },
            _ => self.frames.push(vec![ns, n]),
        }
        if self.frames.len() >= 64 {
            self.flush();
        }
    }
    /// Writes the frames held back and flushes the file (also before a command line, so a crash keeps the frames up to it).
    fn flush(&mut self) {
        if !self.frames.is_empty() {
            let frames = std::mem::take(&mut self.frames);
            self.line(&Line::Frames(frames));
        }
        let _ = self.out.flush();
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.flush();
    }
}

/// The recording's first line, once its battle has loaded: by then the lobby (if it launched the battle) holds the setup.
fn write_header(world: &mut World) {
    let Some((seed, args)) = world.get_resource_mut::<Recorder>().and_then(|mut r| r.pending.take()) else { return };
    let skirmish = match (world.get_resource::<crate::menu::Skirmish>(), world.get_resource::<crate::combat::Library>()) {
        (Some(sk), Some(lib)) if sk.map.is_some() => crate::menu::SkirmishSetup::capture(sk, lib),
        _ => None,
    };
    let header = Header::new(seed, &args, skirmish);
    if header.from_menu {
        warn!("PROCYON_COMMANDS: this battle was set up in the menus without a skirmish setup (a campaign mission): --replay will refuse it");
    }
    let mut rec = world.resource_mut::<Recorder>();
    let _ = serde_json::to_writer(&mut rec.out, &header);
    let _ = rec.out.write_all(b"\n");
    let _ = rec.out.flush();
}

fn record(world: &mut World, at: At, c: &PlayerCommand) {
    if !world.contains_resource::<Recorder>() || world.resource::<Clock>().battle != 0 {
        return;
    }
    let saved = Saved::save(world, &c.kind);
    let mut rec = world.resource_mut::<Recorder>();
    rec.flush();
    match saved {
        Ok(order) => rec.line(&Line::Command { at, player: c.player, order }),
        Err(what) => {
            if !rec.warned {
                rec.warned = true;
                warn!("PROCYON_COMMANDS: a command names {what}; a replay of this recording will differ from there");
            }
            rec.line(&Line::Unrecorded { at, what: what.into() });
        }
    }
    rec.flush();
}

/// End of each frame: the battle frame goes into the recording (its game-time advance and ticks).
fn record_frame(clock: Res<Clock>, vtime: Res<Time<Virtual>>, rec: Option<ResMut<Recorder>>, mut exit: MessageReader<AppExit>, mut done: Local<bool>) {
    let Some(mut rec) = rec else { return };
    if *done {
        return;
    }
    let over = clock.battle != 0;
    if clock.active && !over {
        rec.frame(vtime.delta(), clock.frame_ticks);
    }
    if over || exit.read().next().is_some() {
        rec.flush();
        *done = over;
    }
}

/// In a replay, the recording's commands applied at `at`, for this world; None when this is no replay.
fn replayed(world: &mut World, at: At) -> Option<Vec<PlayerCommand>> {
    // A finished replay (end of the recording, or a later battle) hands the battle back to live input.
    if world.resource::<Clock>().battle != 0 {
        return None;
    }
    let saved: Vec<(PlayerId, Saved)> =
        world.get_resource::<Replay>().filter(|r| !r.finished)?.commands.iter().filter(|(a, ..)| *a == at).map(|(_, p, s)| (*p, s.clone())).collect();
    Some(
        saved
            .iter()
            .filter_map(|(player, s)| {
                let kind = s.load(world);
                if kind.is_none() {
                    warn!("--replay: a command at {at:?} names an object this battle does not have: {s:?}");
                }
                kind.map(|kind| PlayerCommand { player: *player, kind })
            })
            .collect(),
    )
}

/// A recording being played back.
#[derive(Resource, Debug)]
pub struct Replay {
    /// Each battle frame's game-time advance and tick count.
    frames: Vec<(Duration, u32)>,
    /// When, by whom, what.
    commands: Vec<(At, PlayerId, Saved)>,
    finished: bool,
}

impl Replay {
    /// Reads a recording: (header, replay, unrecorded count).
    fn read(text: impl BufRead) -> Result<(Header, Replay, usize), String> {
        let mut lines = text.lines().enumerate();
        let header: Header = match lines.next() {
            Some((_, Ok(l))) => serde_json::from_str(&l).map_err(|e| format!("line 1 is not a command-log header: {e}"))?,
            _ => return Err("empty file".into()),
        };
        if !(OLDEST_FORMAT..=FORMAT).contains(&header.format) {
            return Err(format!("format {} (this game reads {OLDEST_FORMAT} to {FORMAT})", header.format));
        }
        let mut replay = Replay { frames: vec![], commands: vec![], finished: false };
        let mut unrecorded = 0;
        for (i, l) in lines {
            let l = l.map_err(|e| e.to_string())?;
            if l.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Line>(&l).map_err(|e| format!("line {}: {e}", i + 1))? {
                Line::Frames(frames) => {
                    for f in frames {
                        let (&ns, &n) = f.first().zip(f.get(1)).ok_or(format!("line {}: a frame needs two numbers", i + 1))?;
                        let ticks = u32::try_from(n).map_err(|_| format!("line {}: tick count {n}", i + 1))?;
                        let repeat = usize::try_from(f.get(2).copied().unwrap_or(1)).unwrap_or(usize::MAX);
                        if repeat > MAX_FRAMES - replay.frames.len() {
                            return Err(format!("line {}: more than {MAX_FRAMES} frames", i + 1));
                        }
                        replay.frames.extend(std::iter::repeat_n((Duration::from_nanos(ns), ticks), repeat));
                    }
                }
                Line::Command { at, player, order } => replay.commands.push((at, player, order)),
                Line::Unrecorded { .. } => unrecorded += 1,
            }
        }
        Ok((header, replay, unrecorded))
    }
}

/// End of each frame of a replay: the next battle frame's game-time advance goes to Bevy's clock (read at the start of that frame),
/// at 1x and unpaused, so the frame advances exactly as recorded whatever this machine's frame rate or the speed and pause controls.
fn replay_clock(
    clock: Res<Clock>,
    mut replay: ResMut<Replay>,
    mut strategy: ResMut<bevy::time::TimeUpdateStrategy>,
    mut vtime: ResMut<Time<Virtual>>,
    state: Option<Res<State<crate::menu::Screen>>>,
    loading: Option<Res<crate::loading::Loading>>,
) {
    if replay.finished {
        return;
    }
    // The frame after this one: the next of a running battle, or the battle's first once it has just loaded.
    let starting = !clock.active && clock.battle == 0 && loading.is_none() && state.is_some_and(|s| *s.get() == crate::menu::Screen::Battle);
    let next = match (clock.active, starting) {
        (true, _) => clock.frame as usize + 1,
        (false, true) => 0,
        (false, false) => {
            if clock.battle != 0 {
                finish(&mut replay, &mut strategy, "the battle ended");
            }
            return;
        }
    };
    match replay.frames.get(next) {
        Some(&(advance, _)) => {
            *strategy = bevy::time::TimeUpdateStrategy::ManualDuration(advance);
            vtime.unpause();
            vtime.set_relative_speed(1.0);
            vtime.set_max_delta(Duration::from_secs(3600));
        }
        None => finish(&mut replay, &mut strategy, "end of the recording"),
    }
}

fn finish(replay: &mut Replay, strategy: &mut bevy::time::TimeUpdateStrategy, why: &str) {
    replay.finished = true;
    *strategy = if crate::fixed_step() { bevy::time::TimeUpdateStrategy::ManualDuration(BATTLE_TICK) } else { default() };
    info!("--replay: {why}; the game runs on its own clock from here");
}

/// Why a recording cannot replay with this launch, if it cannot: the scenario arguments, seed or mods differ.
fn mismatch(header: &Header, args: &[String], seed: u64, mods: &[String]) -> Option<String> {
    if header.from_menu {
        return Some("it was recorded from a campaign mission started in the menus; only skirmishes and battles started with --map or --mission replay so far".into());
    }
    if header.skirmish.is_some() && args.iter().any(|a| a == "--lobby") {
        return Some("it holds its own skirmish setup: leave out --lobby".into());
    }
    let mine = scenario_args(args);
    if header.args != mine {
        return Some(format!("it was recorded with the arguments {:?}, this run has {:?}", header.args, mine));
    }
    if header.seed != seed {
        return Some(format!("it was recorded with --seed {}, this run has {seed}", header.seed));
    }
    if header.mods != mods {
        return Some(format!("it was recorded with the mods {:?}, this run has {:?}", header.mods, mods));
    }
    None
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Outbox>()
        .init_resource::<Clock>()
        .init_resource::<Players>()
        .add_systems(RunFixedMainLoop, frame_start.in_set(bevy::app::RunFixedMainLoopSystems::BeforeFixedMainLoop))
        .add_systems(FixedUpdate, apply_tick.in_set(crate::BattleSet::Tick).after(crate::tick_start))
        .add_systems(PostUpdate, apply_paused)
        .add_systems(Last, record_frame);
    let args = crate::cli::args();
    let seed = crate::combat::SimRng::configured_seed();
    if let Some(path) = args.iter().position(|a| a == "--replay").and_then(|i| args.get(i + 1)) {
        let read = std::fs::File::open(path).map_err(|e| e.to_string()).and_then(|f| Replay::read(std::io::BufReader::new(f)));
        let (header, replay, unrecorded) = read.unwrap_or_else(|e| {
            eprintln!("error: --replay {path}: {e}");
            crate::exit::quit(2)
        });
        if let Some(why) = mismatch(&header, args, seed, &mod_names()) {
            eprintln!("error: --replay {path}: {why}");
            crate::exit::quit(2);
        }
        if header.version != env!("CARGO_PKG_VERSION") {
            warn!("--replay: recorded with version {}, this is {}; the battle may differ", header.version, env!("CARGO_PKG_VERSION"));
        }
        if unrecorded > 0 {
            warn!("--replay: the recording has {unrecorded} command(s) it could not name; the battle differs from the first of them");
        }
        info!("--replay {path}: {} frames, {} commands", replay.frames.len(), replay.commands.len());
        app.insert_resource(replay).add_systems(Last, replay_clock);
    }
    if let Ok(path) = std::env::var("PROCYON_COMMANDS") {
        let file = std::fs::File::create(&path).unwrap_or_else(|e| panic!("PROCYON_COMMANDS: cannot write {path}: {e}"));
        app.insert_resource(Recorder { out: std::io::BufWriter::new(file), pending: Some((seed, args.to_vec())), frames: vec![], warned: false });
    }
}

#[cfg(test)]
mod tests;
