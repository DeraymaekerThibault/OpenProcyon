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

/// Parameter (0..1) of the point of the segment nearest a point, given `rel` (point minus segment start) and the segment's
/// direction `ab`; 0 for a zero-length segment.
pub fn segment_t(rel: Vec3, ab: Vec3) -> f32 {
    if ab.length_squared() > 0.0 { (rel.dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 }
}

/// Distance from `p` to the segment `a`-`b`.
pub fn segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let t = segment_t(p - a, ab);
    p.distance(a + ab * t)
}

/// The point of the segment `a`-`b` nearest `p`, the segment's squared length floored at `floor` (so a degenerate segment
/// divides by `floor`, not by zero).
pub fn nearest_on_segment_floored(p: Vec3, a: Vec3, b: Vec3, floor: f32) -> Vec3 {
    let span = b - a;
    let along = ((p - a).dot(span) / span.length_squared().max(floor)).clamp(0.0, 1.0);
    a + span * along
}
