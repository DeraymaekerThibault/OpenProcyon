//! The original's collision avoider for AI-steered ships (docs/re/movement.md, "Collision avoidance"): one local rule for land,
//! ships and rocks alike, no route planning. Twice a second each ship looks up to 10 s ahead along its current heading; an obstacle
//! in the way caps its speed by the gap, sends it to a detour point on the smallest clear turn, and after about 3 s of the same
//! obstacle the lower-priority ship stops for 4 s. PROVISIONAL (not ported): mines, torpedoes, dragons, black holes and nebulae as
//! obstacles, the scanner range limit, the avoider's exclusion list, and the turn-in-place unstick.
use super::{Obstacle, box_body, contact, hits};
use bevy::prelude::*;
use std::f32::consts::PI;

/// Seconds between thinks, look-ahead (s), release look-ahead factor, same-obstacle thinks before the right-of-way test, wait (s).
const THINK: f32 = 0.5;
pub(super) const LOOK: f32 = 10.0;
const RELEASE: f32 = 1.75;
const SAME_LIMIT: u32 = 5;
const WAIT: f32 = 4.0;
/// Priority a moving ship of the human's gets on top of its speed: computer ships give way to it.
pub(super) const HUMAN_PRIORITY: f32 = 50.0;

/// What an obstacle is, for the same-obstacle count.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Key {
    Entity(Entity),
    Rock(usize),
}

/// One obstacle as the avoider sees it.
pub struct Other<'a> {
    pub key: Key,
    pub centre: Vec3,
    pub radius: f32,
    /// Right of way: speed, plus `HUMAN_PRIORITY` for a moving ship of the human's; 0 for anything that is not a ship.
    pub priority: f32,
    pub lifeboat: bool,
    pub shape: Shape<'a>,
}
pub enum Shape<'a> {
    /// A ship: its box carried along its heading at its speed.
    Ship { bounds: [Vec3; 2], heading: f32, velocity: Vec3 },
    /// Scenery: `island` measures the gap along the course to the contact instead of between centres.
    Fixed { obstacle: &'a Obstacle, island: bool },
}

/// The avoiding ship.
pub struct Me {
    pub pos: Vec3,
    pub heading: f32,
    pub speed: f32,
    pub bounds: [Vec3; 2],
    pub radius: f32,
    pub priority: f32,
    pub lifeboat: bool,
}

/// One ship's avoider state between thinks.
#[derive(Clone, Debug)]
pub struct Avoid {
    next: f32,
    wait: f32,
    pub cap: f32,
    pub detour: Option<Vec3>,
    last: Option<Key>,
    same: u32,
}
impl Avoid {
    /// `stagger` delays the first think (the original spreads thinks over 0.05 s steps).
    pub fn new(stagger: f32) -> Self {
        Avoid { next: stagger, wait: 0.0, cap: 1.0, detour: None, last: None, same: 0 }
    }
    /// Advances the clocks by `dt`; true when the ship thinks this tick.
    pub fn due(&mut self, dt: f32) -> bool {
        self.next -= dt;
        if self.wait > 0.0 {
            self.wait -= dt;
            if self.wait <= 0.0 {
                self.cap = 1.0;
            }
        }
        if self.next > 0.0 {
            return false;
        }
        self.next += THINK;
        self.wait <= 0.0
    }
    fn reset(&mut self) {
        (self.cap, self.detour, self.last, self.same) = (1.0, None, None, 0);
    }

    /// One think toward `dest` (the current waypoint, or a point ahead on the goal heading); `me_key` is the ship itself.
    pub fn think(&mut self, me: &Me, dest: Vec3, others: &[Other], me_key: Key) {
        let to_dest = (dest - me.pos).with_y(0.0);
        let left = to_dest.length() - me.radius;
        if left <= 0.0 {
            return self.reset();
        }
        let v = me.speed.abs();
        let shortened = v > 0.0 && left < LOOK * v;
        let t = if shortened { left / v } else { LOOK };
        let reach = t * v;
        let cands: Vec<&Other> =
            others.iter().filter(|o| o.key != me_key && !(me.lifeboat && o.lifeboat) && (o.centre - me.pos).with_y(0.0).length() - me.radius - o.radius <= reach).collect();
        let dest_heading = (-to_dest.x).atan2(-to_dest.z);
        if self.detour.is_some() && sweep(me, dest_heading, if shortened { t } else { RELEASE * t }, &cands).is_none() {
            self.detour = None;
        }
        let Some((hit_t, o)) = sweep(me, me.heading, t, &cands) else {
            (self.cap, self.last, self.same) = (1.0, None, 0);
            return;
        };
        self.same = if self.last == Some(o.key) { self.same + 1 } else { 0 };
        self.last = Some(o.key);
        if self.same > SAME_LIMIT && o.priority >= me.priority {
            (self.cap, self.wait) = (0.0, WAIT);
            return;
        }
        let gap = match o.shape {
            Shape::Fixed { island: true, .. } => v * hit_t,
            _ => (o.centre - me.pos).with_y(0.0).length() - me.radius - o.radius,
        };
        let low = me.radius.min(reach);
        let high = 4.0 * me.radius;
        self.cap = 0.1 + 0.9 * ((gap - low) / (high - low).max(1e-3)).clamp(0.0, 1.0);
        if let Some(h) = clear_heading(me, dest_heading, t, &cands) {
            self.detour = Some(me.pos + dir(h) * (4.0 * me.radius).max(reach));
        }
    }
}

fn dir(heading: f32) -> Vec3 {
    Vec3::new(-heading.sin(), 0.0, -heading.cos())
}

/// The first obstacle the ship's box meets sailing `heading` at its speed for `secs`, sampled once per hull length of travel, with
/// the time of contact; the nearest by sphere gap when several are met at that sample.
fn sweep<'a>(me: &Me, heading: f32, secs: f32, others: &[&'a Other<'a>]) -> Option<(f32, &'a Other<'a>)> {
    let v = me.speed.abs();
    let length = (me.bounds[1].z - me.bounds[0].z).max(1.0);
    // ponytail: one step of `secs` when the ship covers less than a hull length (the original's rule there is unclear).
    let step = if v * secs < length { secs } else { length / v };
    let mut t = 0.0;
    while t <= secs + 1e-4 {
        let tf = Transform::from_translation(me.pos + dir(heading) * v * t).with_rotation(Quat::from_rotation_y(heading));
        let body = box_body(me.bounds, &tf);
        let gap = |o: &Other| (o.centre - me.pos).length() - o.radius;
        let hit = others
            .iter()
            .filter(|o| match &o.shape {
                Shape::Fixed { obstacle, .. } => hits(&body, obstacle),
                Shape::Ship { bounds, heading, velocity } => {
                    let other = box_body(*bounds, &Transform::from_translation(o.centre + *velocity * t).with_rotation(Quat::from_rotation_y(*heading)));
                    body.bounds.overlaps(other.bounds) && contact(&body.points, &other.points).is_some()
                }
            })
            .min_by(|a, b| gap(a).total_cmp(&gap(b)));
        if let Some(o) = hit {
            return Some((t, *o));
        }
        t += step;
    }
    None
}

/// The smallest turn of the current heading, in steps of the hull's angular width up to half a circle, after which a full sweep is
/// clear; turning toward and away from the destination are both tried, a tie goes away from it.
fn clear_heading(me: &Me, dest_heading: f32, secs: f32, others: &[&Other]) -> Option<f32> {
    let half_width = me.bounds[0].x.abs().max(me.bounds[1].x.abs());
    let step = 2.0 * (half_width / (me.bounds[1].z - me.bounds[0].z).max(1e-3)).atan();
    let toward = if crate::ship::angdiff(dest_heading, me.heading) >= 0.0 { 1.0 } else { -1.0 };
    let first = |side: f32| (1..).map(|k| k as f32 * step).take_while(|a| *a <= PI).find(|a| sweep(me, me.heading + side * a, secs, others).is_none());
    match (first(toward), first(-toward)) {
        (Some(a), Some(b)) if a < b => Some(me.heading + toward * a),
        (_, Some(b)) => Some(me.heading - toward * b),
        (Some(a), None) => Some(me.heading + toward * a),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests;
