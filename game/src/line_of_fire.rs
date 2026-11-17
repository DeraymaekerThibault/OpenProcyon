//! A computer captain's line-of-fire hold (docs/re/gunnery.md, "Line of fire"): a computer-run ship fires nothing (no bank fire, no
//! ground-fire point, no harpoon roll) while a friendly ship or a piece of scenery lies on the straight segment from its centre to
//! the aim point. Pure predicates; `combat::gunnery` and `boarding` call them. Not applied to the player's own ship.
//!
//! PROVISIONAL: which scenery counts (islands and bases by their triangles, every other obstacle, asteroids included, by its bounding
//! sphere) and the aim point for broadside, frontal and rear runs (the target's aim point is used throughout).
use crate::collision::Obstacle;
use bevy::prelude::*;

/// Whether the segment `a`-`b` crosses the sphere's surface: a root of the line-sphere equation with parameter in [0, 1], ends included.
/// A segment lying wholly inside the sphere does not cross it; a zero-length segment never does.
pub fn crosses_sphere(a: Vec3, b: Vec3, centre: Vec3, radius: f32) -> bool {
    let (d, f) = (b - a, a - centre);
    let (qa, qb, qc) = (d.dot(d), 2.0 * d.dot(f), f.dot(f) - radius * radius);
    let disc = qb * qb - 4.0 * qa * qc;
    if qa <= 0.0 || disc < 0.0 || disc.is_nan() {
        return false;
    }
    if disc == 0.0 {
        return (0.0..=1.0).contains(&(-qb / (2.0 * qa)));
    }
    let root = disc.sqrt();
    [(-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa)].iter().any(|t| (0.0..=1.0).contains(t))
}

/// A friendly ship (bounding spheres, world space; the firing ship left out by the caller) lies on the segment. Dead and cloaked ships
/// are passed in like any other.
pub fn friend_in_line(start: Vec3, end: Vec3, friends: impl IntoIterator<Item = (Vec3, f32)>) -> bool {
    friends.into_iter().any(|(c, r)| crosses_sphere(start, end, c, r))
}

/// One piece of scenery that may be in the way.
pub struct Scenery<'a> {
    /// Bounding sphere (world space).
    pub sphere: (Vec3, f32),
    /// Islands and bases are tested by their triangles; the rest by `sphere`.
    pub mesh: Option<&'a Obstacle>,
    /// The aimed-at object, which never blocks its own shot.
    pub target: bool,
}

/// Scenery lies on the segment: candidates within the segment's length of the shooter by surface distance (centre distance minus the
/// two radii), the target exempt. A plain obstacle blocks when the segment crosses its sphere or an end lies inside it; a mesh when the
/// segment meets one of its triangles.
pub fn scenery_in_line<'a>(start: Vec3, end: Vec3, own_radius: f32, candidates: impl IntoIterator<Item = Scenery<'a>>) -> bool {
    let length = start.distance(end);
    candidates.into_iter().filter(|s| !s.target).any(|s| {
        let (c, r) = s.sphere;
        if start.distance(c) - own_radius - r > length {
            return false;
        }
        match s.mesh {
            Some(o) => o.segment_hit(start, end).is_some(),
            None => crosses_sphere(start, end, c, r) || start.distance(c) <= r || end.distance(c) <= r,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const START: Vec3 = Vec3::ZERO;
    const END: Vec3 = Vec3::new(100.0, 0.0, 0.0);
    fn rock(centre: Vec3, r: f32, target: bool) -> Scenery<'static> {
        Scenery { sphere: (centre, r), mesh: None, target }
    }

    #[test]
    fn a_sphere_the_segment_passes_through_blocks() {
        assert!(friend_in_line(START, END, [(Vec3::new(50.0, 3.0, 0.0), 5.0)]));
        assert!(!friend_in_line(START, END, [(Vec3::new(50.0, 6.0, 0.0), 5.0)]));
    }
    #[test]
    fn a_sphere_beyond_the_end_or_behind_the_start_does_not_block() {
        assert!(!friend_in_line(START, END, [(Vec3::new(120.0, 0.0, 0.0), 5.0), (Vec3::new(-20.0, 0.0, 0.0), 5.0)]));
    }
    #[test]
    fn a_segment_wholly_inside_a_ship_sphere_does_not_block_but_one_leaving_it_does() {
        assert!(!friend_in_line(START, END, [(Vec3::new(50.0, 0.0, 0.0), 200.0)]));
        assert!(friend_in_line(START, END, [(Vec3::new(150.0, 0.0, 0.0), 60.0)]));
        assert!(friend_in_line(START, END, [(END, 5.0)]), "ending inside the sphere crosses its surface");
    }
    #[test]
    fn a_zero_length_segment_blocks_nothing() {
        assert!(!friend_in_line(START, START, [(START, 5.0)]));
    }
    #[test]
    fn a_plain_obstacle_blocks_when_an_end_is_inside_it() {
        assert!(scenery_in_line(START, END, 0.0, [rock(Vec3::new(50.0, 1.0, 0.0), 10.0, false)]));
        assert!(scenery_in_line(START, END, 0.0, [rock(Vec3::new(50.0, 0.0, 0.0), 500.0, false)]), "both ends inside");
        assert!(!scenery_in_line(START, END, 0.0, [rock(Vec3::new(50.0, 20.0, 0.0), 10.0, false)]));
    }
    #[test]
    fn the_target_never_blocks_its_own_shot() {
        assert!(!scenery_in_line(START, END, 0.0, [rock(Vec3::new(100.0, 0.0, 0.0), 10.0, true)]));
        assert!(scenery_in_line(START, END, 0.0, [rock(Vec3::new(100.0, 0.0, 0.0), 10.0, false)]));
    }
    #[test]
    fn scenery_farther_than_the_segment_is_never_a_candidate() {
        // surface distance 150 - 5 - 5 = 140 > 100
        assert!(!scenery_in_line(START, END, 5.0, [rock(Vec3::new(150.0, 0.0, 0.0), 5.0, false)]));
    }
    #[test]
    fn an_island_is_tested_by_its_triangles_not_its_sphere() {
        let (u, v) = (Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, 0.0, 10.0));
        let c = Vec3::new(50.0, 0.0, 0.0);
        let wall = Obstacle::from_triangles(&[[c - u - v, c + u - v, c + u + v], [c - u - v, c + u + v, c - u + v]]);
        let island = |at: Vec3| Scenery { sphere: (at, 100.0), mesh: Some(&wall), target: false };
        assert!(scenery_in_line(START, END, 0.0, [island(c)]));
        assert!(!scenery_in_line(START, Vec3::new(100.0, 40.0, 0.0), 0.0, [island(c)]), "the sphere would be crossed, the triangles are not");
    }
}
