//! The battle's system wiring that crosses module boundaries: the shared run condition, the per-tick systems of several owners
//! whose order is fixed by one chain, and the per-frame chain. Each owner's own systems are added by its own plugin (`scenario::plugin`,
//! `course::plugin`, `view::plugin`, ...).
//!
//! `ship::Fleets` and `collision::Shapes` are wired here only until `ship.rs` and `collision.rs` can host plugins of their own.
use crate::{
    BATTLE_TICK, ai, blackhole, briefing, camera, collision, combat, course, debug_kill, hud, input, leave_battle, loading, menu, outcome, restart, screenshot, scripted_kills,
    scripted_steps, ship, sky, toggle_debug, view, wake,
};
use bevy::prelude::*;

/// Queued for removal: the object is destroyed at the start of the next tick and, until then, still takes part in the logic of
/// the tick it was queued in (the original's deferred removal, docs/re/tick.md).
#[derive(Component)]
pub struct Removed;

/// Queues `e` for removal at the start of the next tick. Safe to call twice, or for an entity already gone.
pub fn queue_removal(commands: &mut Commands, e: Entity) {
    commands.entity(e).try_insert(Removed);
}

/// Created during a tick: the object joins at the start of the next tick and is neither moved nor tested before then.
/// Movement systems of such objects (shells, blasts, wells) leave `With<Joining>` out.
#[derive(Component)]
pub struct Joining;

/// Height below which a moving object has left the map (docs/re/tick.md).
pub const MAP_FLOOR: f32 = -130.0;

/// Whether `p` (Bevy axes, y up) is below the map's floor.
pub fn below_floor(p: Vec3) -> bool {
    p.y < MAP_FLOOR
}

/// After the movement of a tick: moving objects that ended below the floor are queued for removal. Shells test their own step.
pub(crate) fn out_of_map(mut commands: Commands, movers: Query<(Entity, &Transform), (With<combat::Asteroid>, Without<Removed>)>) {
    for (e, tf) in &movers {
        if below_floor(tf.translation) {
            queue_removal(&mut commands, e);
        }
    }
}

/// Start of a tick: destroy what was queued for removal last tick, and let what was created last tick join.
pub(crate) fn tick_start(mut commands: Commands, removed: Query<Entity, With<Removed>>, joining: Query<Entity, With<Joining>>) {
    for e in &removed {
        commands.entity(e).try_despawn();
    }
    for e in &joining {
        commands.entity(e).try_remove::<Joining>();
    }
}

/// Single-player rules (mission and skirmish scripts): evaluated once per frame, after the last tick of that frame and also while the
/// game is paused (docs/re/tick.md). At 2x and 5x they run a half or a fifth as often as ticks. Their `Res<Time>` is the frame's
/// game-time advance, so a script timer still adds up to the ticks' time.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameRules;

/// Ordered shared simulation stages; input and presentation stay on the frame schedules.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BattleSet {
    /// Removals queued last tick are applied and new objects join, before anything else of the tick.
    Tick,
    Commands,
    Rules,
    /// Step 6 of the original's tick, the mission tick: boarding fights are resolved and links (grapples, tows) test whether they break
    /// and reel in. Skipped while the mission is frozen. No position is written here.
    MissionTick,
    Forces,
    Simulate,
    /// Sensor snapshot of the positions after movement and collision, for the decisions below.
    Sense,
    /// Per-object decisions (captains, mission AI) run after movement and collision; what they decide acts in the next tick's integration.
    Ai,
    Fleet,
    /// Ship upkeep (fire, deaths, repair) and shells, after the object's decision.
    Combat,
    Cleanup,
    Outcome,
}

/// The order inside the stages, from the original's tick (docs/re/tick.md "One tick, in order", #700); where the original does not
/// say, the order the tick ran in before it was written down. Every two systems of the tick that touch the same data have an order
/// (`PROCYON_SCHEDULE_CHECK`, run by the gate), so a new system cannot silently reorder two others.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Step {
    /// Commands: the autoplay's briefing skip, then the orders in queue order (dock, board, grapple), then the fleets' bookkeeping
    /// (captures, promotions), then the outfit of ships that joined, then the cloak.
    Script,
    Orders,
    Fleets,
    Joining,
    Cloak,
    /// Forces: the field effects (black hole and gravity wells, then the nebula), then the link keeper (docked hold, winch).
    Fields,
    Links,
    /// Movement: animals, dragons and asteroids move in the one physics list with the ships, before the ships' collision pass.
    Move,
    Ships,
    /// Decisions: the dragons' think, then a ship's always-on routine (a computer's grapple-and-board, whose roll comes before the
    /// current routine's), then the captain and the mission AI, then the current routine's dock and tow decisions.
    Animals,
    AlwaysOn,
    Captains,
    Routines,
    /// Upkeep: guns and shells, then damage, fires and deaths, then the docked repair, the last part of a ship's upkeep.
    Guns,
    Upkeep,
    Repair,
    /// Cleanup: battle statistics, then the lifeboats of lost ships. Outcome: the battle's end, then the dev report and trace.
    Stats,
    Boats,
    End,
    Report,
}

pub(crate) fn battle_schedule(app: &mut App) {
    // The battle tick runs its systems one at a time, in the schedule's sorted order, the same on every run of a build. The
    // multithreaded executor orders two systems that touch the same data but have no `before`/`after` between them by whichever
    // becomes ready first, so two runs of one seed could roll the battle generator in another order (#536). Every such pair now has
    // an order (`Step`), and single-threaded the tick also ran about 7 % faster on the heaviest lobby, so it stays.
    app.edit_schedule(FixedUpdate, |s| {
        s.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        // `PROCYON_SCHEDULE_CHECK=1`: building the tick fails on any two systems that touch the same data with no order between them
        // (`=warn` only lists them; names need Bevy's `debug` feature). The gate's startup smoke sets it.
        if let Ok(v) = std::env::var("PROCYON_SCHEDULE_CHECK") {
            use bevy::ecs::schedule::{LogLevel, ScheduleBuildSettings};
            let level = if v == "warn" || v == "list" { LogLevel::Warn } else { LogLevel::Error };
            s.set_build_settings(ScheduleBuildSettings { ambiguity_detection: level, ..default() });
        }
    });
    app.insert_resource(Time::<Fixed>::from_duration(BATTLE_TICK)).add_systems(FixedUpdate, tick_start.in_set(BattleSet::Tick)).configure_sets(
        FixedUpdate,
        (
            BattleSet::Tick,
            BattleSet::Commands,
            BattleSet::Rules,
            BattleSet::MissionTick,
            BattleSet::Forces,
            BattleSet::Simulate,
            BattleSet::Sense,
            BattleSet::Ai,
            BattleSet::Fleet,
            BattleSet::Combat,
            BattleSet::Cleanup,
            BattleSet::Outcome,
        )
            .chain()
            .run_if(loading::done),
    );
    use Step::*;
    app.configure_sets(FixedUpdate, (Script, Orders, Fleets, Joining, Cloak).chain().in_set(BattleSet::Commands))
        .configure_sets(FixedUpdate, (Fields, Links).chain().in_set(BattleSet::Forces))
        .configure_sets(FixedUpdate, (Move, Ships).chain().in_set(BattleSet::Simulate))
        .configure_sets(FixedUpdate, (Animals, AlwaysOn, Captains, Routines).chain().in_set(BattleSet::Ai))
        .configure_sets(FixedUpdate, (Guns, Upkeep, Repair).chain().in_set(BattleSet::Combat))
        .configure_sets(FixedUpdate, (Stats, Boats).chain().in_set(BattleSet::Cleanup))
        .configure_sets(FixedUpdate, (End, Report).chain().in_set(BattleSet::Outcome));
}

/// Systems that only run while a battle is on screen and no menu overlay is open. One set carries the two run conditions, so a
/// system joins the set instead of repeating them.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BattleActive;

pub fn plugin(app: &mut App) {
    app.init_resource::<collision::Shapes>().add_message::<collision::Ram>().init_resource::<ship::Fleets>().add_plugins(systems);
}

/// The sets and systems, apart from the resources they read (`Shapes` loads game data, so a test leaves it out).
fn systems(app: &mut App) {
    app.configure_sets(FixedUpdate, BattleActive.run_if(in_state(menu::Screen::Battle)).run_if(menu::overlay_closed))
        .configure_sets(Update, BattleActive.run_if(in_state(menu::Screen::Battle)).run_if(menu::overlay_closed))
        .add_systems(
            FixedUpdate,
            (
                ship::reconcile_fleets.in_set(Step::Fleets),
                ai.in_set(Step::Captains),
                ship::fleet_follow.in_set(BattleSet::Fleet),
                blackhole::pull.in_set(Step::Fields),
                (briefing::halt_arrival, collision::sail, out_of_map).chain().in_set(Step::Ships),
                outcome.in_set(Step::End),
            )
                .in_set(BattleActive),
        )
        // The script frame count comes before Update, so the scripts and pictures there each see every frame number once (#590);
        // `loading::wait` runs before it, so the frame a battle finishes loading counts (else a script step repeats: R restarts again).
        .add_systems(PreUpdate, screenshot)
        .add_systems(
            Update,
            (
                (scripted_steps, restart, debug_kill, scripted_kills).chain(),
                briefing::update,
                view::fleet_keys,
                input,
                toggle_debug,
                camera,
                sky,
                wake::update,
                blackhole::update,
                course::update,
                hud,
                leave_battle,
            )
                .chain()
                .in_set(BattleActive),
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::schedule::{ScheduleLabel, Schedules};

    /// The battle loop is still wired: its tick systems (with the tick start of `battle_schedule`) and its frame systems are in
    /// their schedules, and the built schedules have no ordering cycle. (System names are hidden without Bevy's `debug` feature, so
    /// the test counts systems; automatic sync points may add to the count.)
    #[test]
    fn the_battle_loop_is_wired() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin)).init_state::<menu::Screen>();
        app.add_plugins((battle_schedule, systems));
        let mut count = |label: bevy::ecs::intern::Interned<dyn ScheduleLabel>| {
            let world = app.world_mut();
            let mut schedule = world.resource_mut::<Schedules>().remove(label).expect("schedule exists");
            schedule.initialize(world).expect("builds without an ordering cycle");
            let systems = schedule.systems().expect("initialized").count();
            world.resource_mut::<Schedules>().insert(schedule);
            systems
        };
        assert!(count(FixedUpdate.intern()) > 8, "tick start and the eight battle tick systems");
        assert!(count(Update.intern()) > 14, "the battle frame chain");
    }
}
