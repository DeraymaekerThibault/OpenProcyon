//! The battle's system wiring that crosses module boundaries: the shared run condition, the per-tick systems of several owners
//! whose order is fixed by one chain, and the per-frame chain. Each owner's own systems are added by its own plugin (`scenario::plugin`,
//! `course::plugin`, `view::plugin`, ...).
//!
//! `ship::Fleets` and `collision::Shapes` are wired here only until `ship.rs` and `collision.rs` can host plugins of their own.
use crate::*;

/// Systems that only run while a battle is on screen and no menu overlay is open. One set carries the two run conditions, so a
/// system joins the set instead of repeating them.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BattleActive;

pub fn plugin(app: &mut App) {
    app.init_resource::<collision::Shapes>().add_message::<collision::Ram>().init_resource::<ship::Fleets>().init_resource::<ship::CaptainTargets>().add_plugins(systems);
}

/// The sets and systems, apart from the resources they read (`Shapes` loads game data, so a test leaves it out).
fn systems(app: &mut App) {
    app.configure_sets(FixedUpdate, BattleActive.run_if(in_state(menu::Screen::Battle)).run_if(menu::overlay_closed))
        .configure_sets(Update, BattleActive.run_if(in_state(menu::Screen::Battle)).run_if(menu::overlay_closed))
        .add_systems(
            FixedUpdate,
            (
                ship::reconcile_fleets.in_set(BattleSet::Commands),
                ai.in_set(BattleSet::Ai),
                ship::fleet_follow.in_set(BattleSet::Fleet),
                (briefing::halt_arrival, blackhole::pull, collision::sail, out_of_map).chain().in_set(BattleSet::Simulate),
                outcome.in_set(BattleSet::Outcome),
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
