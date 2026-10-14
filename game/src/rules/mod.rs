//! Data-driven interpreter for the rule list of World/SinglePlayer/Mission01.twt (docs/re/campaign.md).
//! Conditions (all must hold; PROVISIONAL: the exe's combination rule is not decoded) and actions are evaluated every frame from the
//! exported records; unsupported types are logged once and treated as false / no-op. `Script::step` is pure (no ECS) so it can be tested.
//! PROVISIONAL: Enter Volume is level-triggered for run-once rules, formations/borders are approximated or ignored,
//! docking is docking.rs and camera moves are smoothed guesses. Environment presentation lives in environment.rs.
mod eval;
mod systems;
#[cfg(test)]
mod tests;

pub use eval::*;
pub use systems::*;
