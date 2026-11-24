//! Decisions behind the Ships screen's 3D hull preview: which hull is shown, how the camera frames it and how far a mouse drag turns
//! it. The view (`classic/ship_preview.rs`) only builds the render rig and applies these.
//! PROVISIONAL: the original's camera angle is matched by eye on one screenshot.

use bevy::prelude::*;

/// Bearing the hull stands at before it is dragged (radians about the vertical axis).
pub const PREVIEW_YAW: f32 = 0.9;
/// Radians of hull turn per pixel of mouse drag.
const DRAG_RADIANS_PER_PIXEL: f32 = 0.01;

/// What to do with the loaded preview hull when the selection may have changed.
#[derive(Debug, PartialEq, Eq)]
pub enum HullChange {
    /// The wanted hull is already loaded (or nothing is wanted and nothing is loaded).
    Keep,
    /// Drop the loaded hull; load `Some(class)` if there is one to show.
    Replace(Option<usize>),
}

/// Compares the loaded hull classes with the wanted one. Nothing wanted clears the preview.
pub fn hull_change(loaded: impl IntoIterator<Item = usize>, want: Option<usize>) -> HullChange {
    let mut loaded = loaded.into_iter().peekable();
    if (loaded.peek().is_none() && want.is_none()) || loaded.any(|c| Some(c) == want) {
        return HullChange::Keep;
    }
    HullChange::Replace(want)
}

/// The preview camera for a hull of the given bound: the original fills most of the viewport, seen from the bow quarter, slightly
/// above the deck.
pub fn preview_camera(hull_bound: f32) -> Transform {
    let r = hull_bound.max(5.0);
    let d = r * 1.75;
    Transform::from_xyz(d * 0.6, d * 0.18, d * 0.78).looking_at(Vec3::Y * r * 0.2, Vec3::Y)
}

/// Radians the hull turns about the vertical axis for a horizontal mouse movement of `dx` pixels while dragging.
pub fn drag_turn(dx: f32) -> f32 {
    dx * DRAG_RADIANS_PER_PIXEL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_is_kept_while_it_is_the_wanted_one() {
        assert_eq!(hull_change([3], Some(3)), HullChange::Keep);
        assert_eq!(hull_change([], None), HullChange::Keep);
    }

    #[test]
    fn hull_is_replaced_when_the_selection_changes_or_clears() {
        assert_eq!(hull_change([3], Some(4)), HullChange::Replace(Some(4)));
        assert_eq!(hull_change([], Some(4)), HullChange::Replace(Some(4)));
        assert_eq!(hull_change([3], None), HullChange::Replace(None));
    }

    #[test]
    fn camera_distance_grows_with_the_hull_and_has_a_floor() {
        let far = |b: f32| preview_camera(b).translation.length();
        assert_eq!(far(1.0), far(5.0));
        assert!(far(40.0) > far(20.0));
        assert!(preview_camera(20.0).translation.y > 0.0, "seen from above");
    }

    #[test]
    fn drag_turns_in_proportion() {
        assert_eq!(drag_turn(0.0), 0.0);
        assert!((drag_turn(100.0) - 1.0).abs() < 1e-6);
        assert!(drag_turn(-5.0) < 0.0);
    }
}
