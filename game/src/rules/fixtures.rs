//! Shared fixtures of the rule tests: entities, units and the Hero/Escort/Raider fleet. Synthetic data only.

use super::*;
use bevy::prelude::*;
use serde_json::Value;

/// The entity with index `n`.
pub(super) fn e(n: u32) -> Entity {
    Entity::from_raw_u32(n).unwrap()
}

/// A live, visible unit `n` (entity `e(n)`, id -1, at the origin) with a name and an owner; tests set the rest with `..unit(..)`.
pub(super) fn unit(n: u32, name: &str, owner: &str) -> Unit {
    Unit { name: name.into(), owner: owner.into(), ..Unit::new(e(n), -1, Vec3::ZERO) }
}

/// Flagship (the primary) and Escort of "Hero", Raider of "Raiders".
pub(super) fn hero_world() -> Vec<Unit> {
    vec![Unit { primary: true, ..unit(1, "Flagship", "Hero") }, unit(2, "Escort", "Hero"), unit(3, "Raider", "Raiders")]
}

/// A script over mission `data` with no media.
pub(super) fn script_with(data: Value) -> Script {
    script_with_media(data, Value::Null)
}

/// A script over mission `data` and `media`.
pub(super) fn script_with_media(data: Value, media: Value) -> Script {
    Script::new(&data, media)
}
