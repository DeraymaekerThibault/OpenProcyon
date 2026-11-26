//! Data-driven interpreter for the rule list of World/SinglePlayer/Mission01.twt (docs/re/campaign.md).
//! Conditions (all must hold; PROVISIONAL: the original's combination rule is unknown) and actions are evaluated every frame from the
//! exported records; unsupported types are logged once and treated as false / no-op. `Script::step` is pure (no ECS) so it can be tested.
//! PROVISIONAL: Enter Volume is level-triggered for run-once rules, formations/borders are approximated or ignored,
//! docking is docking.rs and camera moves are smoothed guesses. Environment presentation lives in environment.rs.
pub mod active;
pub mod check;
pub mod cli;
pub mod console;
mod eval;
#[cfg(test)]
mod fixtures;
pub mod refs;
pub mod skirmish;
mod systems;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_ext;

pub use eval::*;
pub use systems::*;

/// The (name, text) of every task that is active and not yet complete: the dev autopilot (`smoke/autopilot.rs`) reads the tasks this way.
pub fn live_tasks(sc: &Script) -> impl Iterator<Item = (&str, &str)> {
    sc.tasks.iter().filter(|t| t.active && !t.complete).map(|t| (t.name.as_str(), t.text.as_str()))
}

/// How often the human's ships have hit `victim` (the dev autopilot's "someone is fighting it" test).
pub fn player_hits(sc: &Script, victim: bevy::prelude::Entity) -> u32 {
    sc.hits_by.get(&(eval::PLAYER.to_string(), victim)).copied().unwrap_or(0)
}
