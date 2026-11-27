//! Ships against land: the mesh's tree of oriented boxes (docs/re/collision.md, "Ships against land: the box tree").
//! Pure functions of (tree, ship box) in the remake's axes; the world, the scheduling and the frame conversion live in `collision.rs`.
use bevy::prelude::*;

/// The deepest level whose own result may be the contact; below it a node reports as if it were a leaf.
const LAST_LEVEL: u32 = 6;
/// A cross product of two edges whose squared length (both unit) is below this is nearly parallel and is not an axis.
const PARALLEL: f32 = 0.1;

/// An oriented box: centre, three perpendicular unit axes and the half extents along them.
#[derive(Clone, Copy, Debug)]
pub struct Box3 {
    pub centre: Vec3,
    pub axes: [Vec3; 3],
    pub half: Vec3,
}
impl Box3 {
    /// Half the box's width along the unit vector `axis`.
    fn extent(&self, axis: Vec3) -> f32 {
        (0..3).map(|i| self.axes[i].dot(axis).abs() * self.half[i]).sum()
    }
    /// A local `[min, max]` box placed by `tf`.
    pub fn placed([min, max]: [Vec3; 2], tf: &Transform) -> Self {
        Self {
            centre: tf.transform_point((min + max) * 0.5),
            axes: [tf.rotation * Vec3::X, tf.rotation * Vec3::Y, tf.rotation * Vec3::Z],
            half: (max - min) * 0.5 * tf.scale.abs(),
        }
    }
}

/// One node of a layer's tree, in world space.
#[derive(Clone, Debug)]
pub struct BoxNode {
    pub b: Box3,
    pub level: u32,
    /// Indices into the layer's node list (at most two).
    pub kids: Vec<usize>,
}
impl BoxNode {
    pub fn shifted(&mut self, delta: Vec3) {
        self.b.centre += delta;
    }
}

/// Overlap of two boxes on the 15 axes: `Some(axis x depth)` with the axis of the shallowest overlap pointing from `node` towards
/// `ship`, `None` when any axis separates them.
pub fn overlap(node: &Box3, ship: &Box3) -> Option<Vec3> {
    let d = ship.centre - node.centre;
    let mut axes = [Vec3::ZERO; 15];
    for i in 0..3 {
        axes[i] = node.axes[i];
        axes[3 + i] = ship.axes[i];
        for j in 0..3 {
            let c = node.axes[i].cross(ship.axes[j]);
            // a skipped axis stays zero and is passed over below
            if c.length_squared() >= PARALLEL {
                axes[6 + 3 * i + j] = c.normalize();
            }
        }
    }
    let mut best: Option<(f32, Vec3)> = None;
    for axis in axes.into_iter().filter(|a| *a != Vec3::ZERO) {
        let along = d.dot(axis);
        let gap = along.abs() - node.extent(axis) - ship.extent(axis);
        if gap > 0.0 {
            return None;
        }
        if best.is_none_or(|(g, _)| gap > g) {
            best = Some((gap, if along < 0.0 { -axis } else { axis }));
        }
    }
    best.map(|(gap, axis)| axis * -gap)
}

/// The contact of one subtree: `Some(normal x depth)` or `None`.
pub fn contact(nodes: &[BoxNode], index: usize, ship: &Box3) -> Option<Vec3> {
    let node = &nodes[index];
    let own = overlap(&node.b, ship)?;
    if node.kids.is_empty() || node.level >= LAST_LEVEL {
        return Some(own);
    }
    let found: Vec<Vec3> = node.kids.iter().filter_map(|k| contact(nodes, *k, ship)).collect();
    let combined = match found.as_slice() {
        [] => return None,
        [one] => *one,
        [a, b, ..] => *a + *b,
    };
    Some(if combined.length() < own.length() { combined } else { own })
}

/// The ship's contact with all of an object's layers: each layer's root result, added.
pub fn land_contact(layers: &[Vec<BoxNode>], ship: &Box3) -> Option<Vec3> {
    layers.iter().filter(|l| !l.is_empty()).filter_map(|l| contact(l, 0, ship)).reduce(|a, b| a + b)
}

/// Whether the ship touches any layer: the avoider's yes/no land test, the first layer with contact deciding (docs/re/movement.md).
pub fn touches(layers: &[Vec<BoxNode>], ship: &Box3) -> bool {
    layers.iter().filter(|l| !l.is_empty()).any(|l| contact(l, 0, ship).is_some())
}

/// How far the ship moves for a contact `normal x depth` (remake axes, y up): depth x |cos| along the horizontal part of the normal,
/// where cos is the angle between that horizontal direction and the full normal.
pub fn push(contact: Vec3) -> Vec3 {
    let depth = contact.length();
    let flat = Vec3::new(contact.x, 0.0, contact.z);
    if depth < 1e-9 || flat.length_squared() < 1e-18 {
        return Vec3::ZERO;
    }
    // cos = |flat| / depth, so depth x cos is the horizontal part's length
    flat.normalize() * (depth * (flat.length() / depth))
}

/// The ship's push out of the land, `None` without contact: one step of the original's once-per-tick rule.
pub fn land_push(layers: &[Vec<BoxNode>], ship: &Box3) -> Option<Vec3> {
    land_contact(layers, ship).map(push)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn aligned(centre: Vec3, half: Vec3) -> Box3 {
        Box3 { centre, axes: [Vec3::X, Vec3::Y, Vec3::Z], half }
    }
    fn node(centre: Vec3, half: Vec3, level: u32, kids: &[usize]) -> BoxNode {
        BoxNode { b: aligned(centre, half), level, kids: kids.to_vec() }
    }
    fn near(a: Vec3, b: Vec3) {
        assert!((a - b).length() < 1e-4, "{a} != {b}");
    }
    /// A ship box of half extents 1 whose centre is at x = 10.5, so it reaches 0.5 into a box ending at x = 10.
    fn ship() -> Box3 {
        aligned(Vec3::new(10.5, 0.0, 0.0), Vec3::ONE)
    }
    /// A box spanning x from 0 to `edge`, wide in y and z.
    fn slab(edge: f32, level: u32, kids: &[usize]) -> BoxNode {
        node(Vec3::new(edge / 2.0, 0.0, 0.0), Vec3::new(edge / 2.0, 10.0, 10.0), level, kids)
    }

    #[test]
    fn the_shallowest_axis_is_chosen_and_points_from_the_box_to_the_ship() {
        let b = aligned(Vec3::ZERO, Vec3::splat(2.0));
        near(overlap(&b, &aligned(Vec3::new(2.5, 0.2, 0.0), Vec3::ONE)).unwrap(), Vec3::new(0.5, 0.0, 0.0));
        near(overlap(&b, &aligned(Vec3::new(-2.5, 0.0, 0.2), Vec3::ONE)).unwrap(), Vec3::new(-0.5, 0.0, 0.0));
        near(overlap(&b, &aligned(Vec3::new(0.0, 0.0, 2.9), Vec3::ONE)).unwrap(), Vec3::new(0.0, 0.0, 0.1));
    }
    #[test]
    fn a_gap_on_any_axis_is_no_contact() {
        let b = aligned(Vec3::ZERO, Vec3::splat(2.0));
        assert!(overlap(&b, &aligned(Vec3::new(3.01, 0.0, 0.0), Vec3::ONE)).is_none());
        assert!(overlap(&b, &aligned(Vec3::new(0.0, -3.01, 0.0), Vec3::ONE)).is_none());
        // touching exactly is still contact, with depth 0
        near(overlap(&b, &aligned(Vec3::new(3.0, 0.0, 0.0), Vec3::ONE)).unwrap(), Vec3::ZERO);
    }
    #[test]
    fn an_edge_cross_axis_can_be_the_shallowest() {
        // Computed independently: a unit-half box and a thin rod turned 45 degrees about two axes; neither box's own axes are the
        // shallowest, the cross of the node's up axis... and the rod's forward axis is.
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let rod = Box3 { centre: Vec3::new(2.1, -0.6, -0.1), axes: [Vec3::new(h, h, 0.0), Vec3::new(-0.5, 0.5, h), Vec3::new(0.5, -0.5, h)], half: Vec3::new(0.2, 2.0, 0.2) };
        let got = overlap(&aligned(Vec3::ZERO, Vec3::ONE), &rod).unwrap();
        near(got, Vec3::new(0.816_496_6, 0.0, 0.577_350_3) * 0.015_708_4);
    }
    #[test]
    fn near_parallel_edges_are_skipped_not_normalised() {
        // identical orientation: every cross product is zero; a skipped one must not turn into NaN
        let b = aligned(Vec3::ZERO, Vec3::splat(2.0));
        let r = overlap(&b, &aligned(Vec3::new(2.5, 0.0, 0.0), Vec3::ONE)).unwrap();
        assert!(r.is_finite());
        // a 10 degree tilt about z: the crosses (sin^2 = 0.03) are still below the threshold; the face axes decide
        let (s, c) = 10f32.to_radians().sin_cos();
        let tilted = Box3 { centre: Vec3::new(2.5, 0.0, 0.0), axes: [Vec3::new(c, s, 0.0), Vec3::new(-s, c, 0.0), Vec3::Z], half: Vec3::ONE };
        let r = overlap(&b, &tilted).unwrap();
        assert!(r.is_finite() && r.x > 0.0 && r.y.abs() < 1e-6 && r.z.abs() < 1e-6);
    }
    #[test]
    fn an_inner_node_counts_only_if_a_child_does() {
        // the root overlaps the ship, its only child (x from -10 to -1) does not
        let tree = [slab(10.0, 0, &[1]), node(Vec3::new(-5.5, 0.0, 0.0), Vec3::new(4.5, 10.0, 10.0), 1, &[])];
        assert!(overlap(&tree[0].b, &ship()).is_some());
        assert!(contact(&tree, 0, &ship()).is_none());
        // a leaf root reports its own result
        near(contact(&tree[..1].iter().map(|n| BoxNode { kids: vec![], ..n.clone() }).collect::<Vec<_>>(), 0, &ship()).unwrap(), Vec3::new(0.5, 0.0, 0.0));
    }
    #[test]
    fn a_single_child_replaces_the_node_only_if_shallower() {
        // the root's own depth is 0.5; the child ending at 9.8 overlaps by 0.3, the child ending at 10.4 by 0.9
        near(contact(&[slab(10.0, 0, &[1]), slab(9.8, 1, &[])], 0, &ship()).unwrap(), Vec3::new(0.3, 0.0, 0.0));
        near(contact(&[slab(10.0, 0, &[1]), slab(10.4, 1, &[])], 0, &ship()).unwrap(), Vec3::new(0.5, 0.0, 0.0));
    }
    #[test]
    fn two_children_add_and_the_sum_replaces_the_node_only_if_shallower() {
        // 0.3 + 0.1 = 0.4 is shallower than 0.5
        near(contact(&[slab(10.0, 0, &[1, 2]), slab(9.8, 1, &[]), slab(9.6, 1, &[])], 0, &ship()).unwrap(), Vec3::new(0.4, 0.0, 0.0));
        // 0.3 + 0.4 = 0.7 is not: the node's own result stands
        near(contact(&[slab(10.0, 0, &[1, 2]), slab(9.8, 1, &[]), slab(9.9, 1, &[])], 0, &ship()).unwrap(), Vec3::new(0.5, 0.0, 0.0));
        // a child that does not touch leaves the other as the single child
        let away = node(Vec3::new(-5.5, 0.0, 0.0), Vec3::new(4.5, 10.0, 10.0), 1, &[]);
        near(contact(&[slab(10.0, 0, &[1, 2]), away, slab(9.8, 1, &[])], 0, &ship()).unwrap(), Vec3::new(0.3, 0.0, 0.0));
    }
    #[test]
    fn level_six_is_a_leaf_whatever_it_has_below() {
        let mut tree = vec![slab(10.0, 0, &[1])];
        for level in 1..=6 {
            tree.push(slab(10.0 - level as f32 * 0.01, level, &[level as usize + 1]));
        }
        // level 7 would overlap by 0.5 - 0.07 = 0.43, but it is never looked at; level 6 reports its own 0.5 - 0.06 = 0.44 and, as
        // the shallowest the chain ever produces, it is also what each parent keeps
        tree.push(slab(9.0, 7, &[]));
        near(contact(&tree, 0, &ship()).unwrap(), Vec3::new(0.44, 0.0, 0.0));
    }
    #[test]
    fn layers_add_their_contacts() {
        let a = vec![slab(9.8, 0, &[])];
        let b = vec![slab(9.6, 0, &[])];
        near(land_contact(&[a, b], &ship()).unwrap(), Vec3::new(0.4, 0.0, 0.0));
        assert!(land_contact(&[], &ship()).is_none());
    }
    #[test]
    fn the_push_is_depth_times_cosine_along_the_horizontal_normal() {
        // normal (0.6, 0.8, 0), depth 0.5: horizontal direction +x, cos 0.6
        near(push(Vec3::new(0.3, 0.4, 0.0)), Vec3::new(0.3, 0.0, 0.0));
        // a mostly vertical normal barely moves the ship sideways
        near(push(Vec3::new(0.0, 1.0, 0.02)), Vec3::new(0.0, 0.0, 0.02));
        near(push(Vec3::new(0.0, 0.7, 0.0)), Vec3::ZERO);
        // purely horizontal: the whole depth
        near(push(Vec3::new(-0.3, 0.0, 0.4)), Vec3::new(-0.3, 0.0, 0.4));
    }
    #[test]
    fn a_ship_pushed_by_a_box_tree_ends_up_clear_of_it() {
        let tree = vec![vec![slab(10.0, 0, &[])]];
        let mut ship = ship();
        let push = land_push(&tree, &ship).unwrap();
        near(push, Vec3::new(0.5, 0.0, 0.0));
        ship.centre += push;
        // depth 0 contact: the next tick pushes nothing more
        near(land_push(&tree, &ship).unwrap(), Vec3::ZERO);
    }
}
