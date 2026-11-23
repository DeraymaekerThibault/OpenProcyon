//! The one place that reads Bevy's deprecated `Interaction` component as a pointer state (off / over / down). Bevy 0.20 replaces it
//! with `Hovered` and `Pressed`; moving to those is a change to this file plus the places that name the component in a query or spawn it.
#[allow(deprecated)]
use bevy::prelude::Interaction;

/// What the cursor does to a control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointer {
    Off,
    Over,
    Down,
}

/// Interaction under another name, for the few places that cannot carry an `allow` themselves (derive-generated code).
#[allow(deprecated)]
pub type Raw = Interaction;

#[allow(deprecated)]
pub fn of(i: &Interaction) -> Pointer {
    match i {
        Interaction::None => Pointer::Off,
        Interaction::Hovered => Pointer::Over,
        Interaction::Pressed => Pointer::Down,
    }
}
/// Cursor over the control or pressing it.
#[allow(deprecated)]
pub fn over(i: &Interaction) -> bool {
    of(i) != Pointer::Off
}
/// Cursor pressing the control.
#[allow(deprecated)]
pub fn down(i: &Interaction) -> bool {
    of(i) == Pointer::Down
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_interaction() {
        assert_eq!([Interaction::None, Interaction::Hovered, Interaction::Pressed].map(|i| of(&i)), [Pointer::Off, Pointer::Over, Pointer::Down]);
        assert_eq!([Interaction::None, Interaction::Hovered, Interaction::Pressed].map(|i| (over(&i), down(&i))), [(false, false), (true, false), (true, true)]);
    }
}
