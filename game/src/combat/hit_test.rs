//! Hit geometry: rays and shell segments against ship sections (boxes or meshes) and bases, and aim points on them.
use super::*;

/// Segment a->b against a sphere; entry fraction in [0, 1].
pub fn seg_sphere(a: Vec3, b: Vec3, c: Vec3, r: f32) -> Option<f32> {
    let (d, f) = (b - a, a - c);
    let (qa, qb, qc) = (d.dot(d), 2.0 * f.dot(d), f.dot(f) - r * r);
    if qc <= 0.0 {
        return Some(0.0);
    }
    let disc = qb * qb - 4.0 * qa * qc;
    if qa < 1e-9 || disc < 0.0 {
        return None;
    }
    let t = (-qb - disc.sqrt()) / (2.0 * qa);
    (0.0..=1.0).contains(&t).then_some(t)
}
/// Nearest ray hit against a class's triangles in the ship's local frame; used for click-targeting.
pub fn ray_hit(c: &Class, tf: &Transform, ray: Ray3d) -> Option<f32> {
    let inv = tf.to_matrix().inverse();
    let (o, d) = (inv.transform_point3(ray.origin), inv.transform_vector3(*ray.direction));
    c.sections.iter().filter_map(|s| seg_mesh(o, o + d * 1000.0, s)).map(|t| t * 1000.0).reduce(f32::min)
}

/// Only live mission sections (gun towers) are attackable; the island terrain is not a target.
pub fn base_ray_hit(c: &Class, hp: &Health, tf: &Transform, ray: Ray3d) -> Option<f32> {
    let inv = tf.to_matrix().inverse();
    let (o, d) = (inv.transform_point3(ray.origin), inv.transform_vector3(*ray.direction));
    c.sections.iter().zip(&hp.sections).filter(|(s, h)| s.mission && **h > 0.0).filter_map(|(s, _)| seg_mesh(o, o + d * 1000.0, s)).map(|t| t * 1000.0).reduce(f32::min)
}

pub(super) fn base_aim_point(c: &Class, hp: &Health, clicked: Option<Vec3>) -> Option<Vec3> {
    c.sections
        .iter()
        .zip(&hp.sections)
        .filter(|(s, h)| s.mission && **h > 0.0)
        .map(|(s, _)| (s.min + s.max) * 0.5)
        .min_by(|a, b| a.distance_squared(clicked.unwrap_or(c.centre)).total_cmp(&b.distance_squared(clicked.unwrap_or(c.centre))))
}

// PROVISIONAL: omit destroyed non-vital geometry until damaged-mesh collision is exported.
// Destroyed vital hulls must remain hittable so subsequent hits can damage the core.
pub fn ship_segment_hit(c: &Class, hp: &Health, a: Vec3, b: Vec3) -> Option<(f32, usize)> {
    c.sections.iter().enumerate().filter(|(k, s)| s.vital || hp.sections[*k] > 0.0).filter_map(|(k, s)| seg_mesh(a, b, s).map(|t| (t, k))).min_by(|x, y| x.0.total_cmp(&y.0))
}

pub(super) fn ship_aim_point(c: &Class, hp: &Health, clicked: Option<Vec3>) -> Vec3 {
    let Some(point) = clicked else { return c.centre };
    let contains = |s: &Section| point.cmpge(s.min - Vec3::splat(0.01)).all() && point.cmple(s.max + Vec3::splat(0.01)).all();
    // PROVISIONAL: use bounds to recognize a lost sail aim point; preserve overlapping hull targets.
    if !c.sections.iter().any(|s| s.vital && contains(s)) && c.sections.iter().zip(&hp.sections).any(|(s, h)| !s.vital && *h <= 0.0 && contains(s)) { c.centre } else { point }
}

/// Slab test of segment a->b against a box; returns the entry fraction in [0, 1].
pub(super) fn seg_aabb(a: Vec3, b: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let (d, mut t0, mut t1) = (b - a, 0.0f32, 1.0f32);
    for i in 0..3 {
        if d[i].abs() < 1e-9 {
            if a[i] < min[i] || a[i] > max[i] {
                return None;
            }
        } else {
            let (mut u, mut v) = ((min[i] - a[i]) / d[i], (max[i] - a[i]) / d[i]);
            if u > v {
                std::mem::swap(&mut u, &mut v);
            }
            t0 = t0.max(u);
            t1 = t1.min(v);
            if t0 > t1 {
                return None;
            }
        }
    }
    Some(t0)
}

/// Segment vs the section's triangles (box first); earliest hit fraction in [0, 1]. Triangles are double-sided.
pub(super) fn seg_mesh(a: Vec3, b: Vec3, s: &Section) -> Option<f32> {
    seg_aabb(a, b, s.min, s.max)?;
    let d = b - a;
    s.tris
        .iter()
        .filter_map(|[p, q, r]| {
            let (e1, e2) = (*q - *p, *r - *p);
            let h = d.cross(e2);
            let det = e1.dot(h);
            if det.abs() < 1e-9 {
                return None;
            }
            let (f, sv) = (1.0 / det, a - *p);
            let u = f * sv.dot(h);
            if !(0.0..=1.0).contains(&u) {
                return None;
            }
            let qv = sv.cross(e1);
            let v = f * d.dot(qv);
            if v < 0.0 || u + v > 1.0 {
                return None;
            }
            let t = f * e2.dot(qv);
            (0.0..=1.0).contains(&t).then_some(t)
        })
        .reduce(f32::min)
}

/// Distance from `p` to the segment `a`-`b`.
pub(super) fn segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + ab * t)
}
