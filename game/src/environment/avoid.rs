//! Flock animals steering round hulls and rocks on their path. Engine-added: the original's whale is pushed apart from whatever it
//! touches by mass and keeps swimming (docs/re/collision.md "Dragons and whales in contact"); here it sidesteps a body that would
//! actually hit it, by a margin, and rejoins its path (docs/re/dragons.md "Space animals").
use bevy::prelude::*;

/// Clearance kept between the animal's body and a hull or rock it swims past, in units.
pub const MARGIN: f32 = 1.5;
/// How far ahead the animal looks, in seconds of swimming.
pub const LOOKAHEAD: f32 = 2.5;

/// A body on the water plane: a rectangle (a hull's box, `half` 0 for a round rock) grown by `round`, with the heights it spans.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub centre: Vec2,
    /// Unit vector along the rectangle's first side.
    pub right: Vec2,
    pub half: Vec2,
    pub round: f32,
    pub y: [f32; 2],
}
impl Body {
    pub fn rock(centre: Vec3, radius: f32) -> Self {
        Body { centre: centre.xz(), right: Vec2::X, half: Vec2::ZERO, round: radius, y: [centre.y - radius, centre.y + radius] }
    }
    /// A ship's collision box `bounds` placed by `tf`.
    pub fn hull(bounds: [Vec3; 2], tf: &Transform) -> Self {
        let mid = tf.transform_point((bounds[0] + bounds[1]) * 0.5);
        let right = (tf.rotation * Vec3::X).xz().try_normalize().unwrap_or(Vec2::X);
        let half = (bounds[1] - bounds[0]).xz().abs() * 0.5 * tf.scale.xz().abs();
        let h = (bounds[1].y - bounds[0].y) * 0.5 * tf.scale.y.abs();
        Body { centre: mid.xz(), right, half, round: 0.0, y: [mid.y - h, mid.y + h] }
    }
    /// The point of the body's core rectangle nearest to `p` and the distance from `p` to the body's surface (0 inside).
    pub(super) fn nearest(&self, p: Vec2) -> (Vec2, f32) {
        let up = Vec2::new(-self.right.y, self.right.x);
        let rel = p - self.centre;
        let local = Vec2::new(rel.dot(self.right), rel.dot(up));
        let on = local.clamp(-self.half, self.half);
        let at = self.centre + self.right * on.x + up * on.y;
        (at, ((p - at).length() - self.round).max(0.0))
    }
}

/// How far ahead on its path line an animal that has been turned off it aims, in units.
pub const REJOIN: f32 = 25.0;

/// Where an animal off its path line `from` -> `to` aims: a point `REJOIN` ahead of its foot on the line (or the end), so it comes back
/// to the line instead of cutting straight for the far point. None while it is on the line (within a quarter unit), or on a zero leg.
pub fn rejoin(pos: Vec3, from: Vec3, to: Vec3) -> Option<Vec3> {
    let seg = to - from;
    let len = seg.length();
    if len < 1e-3 {
        return None;
    }
    let along = seg / len;
    let t = (pos - from).dot(along).clamp(0.0, len);
    let foot = from + along * t;
    if foot.distance(pos) < 0.25 {
        return None;
    }
    Some(from + along * (t + REJOIN).min(len))
}

/// The direction to swim in: `dir` (unit) unless a body within `lookahead` units would be hit by an animal of `radius` at `pos`;
/// then `dir` turned away from it, in proportion to how deep the body is inside the clearance. A body the animal flies over or
/// under, or one that clears it by the margin, changes nothing, so a gap that leaves room still leaves room.
pub fn steer(pos: Vec3, dir: Vec3, radius: f32, lookahead: f32, bodies: &[Body]) -> Vec3 {
    let clear = radius + MARGIN;
    let flat_dir = dir.xz().try_normalize().unwrap_or(Vec2::ZERO);
    if flat_dir == Vec2::ZERO {
        return dir;
    }
    let mut push = Vec2::ZERO;
    for b in bodies {
        if pos.y + clear < b.y[0] || pos.y - clear > b.y[1] {
            continue;
        }
        for k in 1..=3 {
            let p = pos.xz() + flat_dir * (lookahead * k as f32 / 3.0);
            let (at, d) = b.nearest(p);
            if d >= clear {
                continue;
            }
            // Away from the nearest point; dead ahead of its centre, to the side the animal already leans to.
            let side = flat_dir.perp_dot(b.centre - pos.xz());
            let sidestep = Vec2::new(flat_dir.y, -flat_dir.x) * if side >= 0.0 { 1.0 } else { -1.0 };
            // Only sideways: the animal never turns back, it goes round.
            let raw = p - at;
            let away = (raw - flat_dir * raw.dot(flat_dir))
                .try_normalize()
                .filter(|l| (raw.length() > 1e-3) && l.is_finite() && (raw - flat_dir * raw.dot(flat_dir)).length() > 0.2 * raw.length())
                .unwrap_or(sidestep);
            push += away * ((clear - d) / clear) * (1.0 - 0.25 * (k - 1) as f32);
            break;
        }
    }
    if push == Vec2::ZERO {
        return dir;
    }
    let flat = (flat_dir + push * 2.0).try_normalize().unwrap_or(flat_dir);
    Vec3::new(flat.x, dir.y, flat.y).normalize_or(dir)
}

/// Moves `pos` out of any body it has sunk into (a hull that sailed onto it): the retail push-apart, for the animal only.
pub fn push_out(pos: Vec3, radius: f32, bodies: &[Body]) -> Vec3 {
    let clear = radius * 0.5;
    let mut out = pos;
    for b in bodies {
        if out.y + clear < b.y[0] || out.y - clear > b.y[1] {
            continue;
        }
        let (at, d) = b.nearest(out.xz());
        if d >= clear {
            continue;
        }
        let p = match (out.xz() - at).try_normalize() {
            Some(dir) => at + dir * (b.round + clear),
            // Sunk into the core rectangle: out through the nearest side.
            None => {
                let up = Vec2::new(-b.right.y, b.right.x);
                let rel = out.xz() - b.centre;
                let local = Vec2::new(rel.dot(b.right), rel.dot(up));
                let room = b.half - local.abs();
                let out_to = |h: f32, l: f32| (h + b.round + clear) * if l >= 0.0 { 1.0 } else { -1.0 };
                let to = if room.x < room.y { Vec2::new(out_to(b.half.x, local.x), local.y) } else { Vec2::new(local.x, out_to(b.half.y, local.y)) };
                b.centre + b.right * to.x + up * to.y
            }
        };
        out = Vec3::new(p.x, out.y, p.y);
    }
    out
}
