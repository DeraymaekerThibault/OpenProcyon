//! A free place for a hull that is put somewhere by a rule (teleport): the nearest spot to the wanted one where its box touches no
//! other hull. Engine-added (the original's teleport does no such test, docs/re/collision.md "Teleport and overlap").
use super::tree::{self, Box3};
use bevy::prelude::*;

/// Clearance kept between two hulls placed by a teleport, in units.
pub const GAP: f32 = 1.0;

/// A hull's collision box as the ship contact test places it.
pub fn hull(bounds: [Vec3; 2], tf: &Transform) -> Box3 {
    Box3::placed(bounds, tf)
}

fn touches(a: &Box3, others: &[Box3]) -> bool {
    others.iter().any(|b| tree::overlap(b, a).is_some())
}

/// `wanted` when a hull with `bounds` and `rotation` standing there touches none of `others`; otherwise the closest spot on the
/// horizontal plane that does not, searched in rings around `wanted` that widen by a quarter of the hull's smaller half-width.
/// Height is kept. Gives `wanted` back when nothing is free within 64 rings (a hull in a crowd it cannot leave).
pub fn free_spot(bounds: [Vec3; 2], rotation: Quat, wanted: Vec3, others: &[Box3]) -> Vec3 {
    // The hull being placed is grown by the gap; the others are as they stand.
    let at = |p: Vec3| {
        let mut b = hull(bounds, &Transform::from_translation(p).with_rotation(rotation));
        b.half += Vec3::splat(GAP);
        b
    };
    if !touches(&at(wanted), others) {
        return wanted;
    }
    let size = bounds[1] - bounds[0];
    let step = (size.x.min(size.z) * 0.125).max(0.5);
    for ring in 1..=64 {
        let r = step * ring as f32;
        let n = (ring * 8).min(64);
        let best = (0..n)
            .map(|k| {
                let a = std::f32::consts::TAU * k as f32 / n as f32;
                wanted + Vec3::new(a.cos(), 0.0, a.sin()) * r
            })
            .find(|p| !touches(&at(*p), others));
        if let Some(p) = best {
            return p;
        }
    }
    wanted
}
