//! Shared geometry helpers (game axes: x = starboard, y = forward, z = up for data; Bevy world: -Z is the bow).
//! Every body here is the exact expression the call sites used, so replacing a site changes no bit of the result.

use bevy::prelude::*;
use std::f32::consts::{PI, TAU};

/// Heading (as `Ship::heading`; 0 = bow along -Z, positive turns about +Y) of the direction `d`.
pub fn heading_of(d: Vec3) -> f32 {
    (-d.x).atan2(-d.z)
}

/// Signed difference `a - b` of two headings, wrapped to [-PI, PI).
pub fn angdiff(a: f32, b: f32) -> f32 {
    (a - b + PI).rem_euclid(TAU) - PI
}

/// Whether the direction `local` (in a mount's own frame, bow = -Z) lies inside a firing arc of total width `arc`.
pub fn arc_covers(local: Vec3, arc: f32) -> bool {
    (-local.x).atan2(-local.z).abs() <= arc / 2.0
}
