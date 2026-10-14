//! Keyboard bindings (retail "Control Options"; default list from the retail screenshots). Stored per user as `bindings.json`
//! (action label -> key name). PROVISIONAL: only the actions marked `wired` have an effect in battle; the rest are listed and
//! rebindable like retail but nothing reads them yet.
use bevy::prelude::*;
use std::collections::HashMap;

use KeyCode::*;
/// Bindable keys and their retail display names.
const KEYS: &[(KeyCode, &str)] = &[
    (KeyA, "A"),
    (KeyB, "B"),
    (KeyC, "C"),
    (KeyD, "D"),
    (KeyE, "E"),
    (KeyF, "F"),
    (KeyG, "G"),
    (KeyH, "H"),
    (KeyI, "I"),
    (KeyJ, "J"),
    (KeyK, "K"),
    (KeyL, "L"),
    (KeyM, "M"),
    (KeyN, "N"),
    (KeyO, "O"),
    (KeyP, "P"),
    (KeyQ, "Q"),
    (KeyR, "R"),
    (KeyS, "S"),
    (KeyT, "T"),
    (KeyU, "U"),
    (KeyV, "V"),
    (KeyW, "W"),
    (KeyX, "X"),
    (KeyY, "Y"),
    (KeyZ, "Z"),
    (Digit0, "0"),
    (Digit1, "1"),
    (Digit2, "2"),
    (Digit3, "3"),
    (Digit4, "4"),
    (Digit5, "5"),
    (Digit6, "6"),
    (Digit7, "7"),
    (Digit8, "8"),
    (Digit9, "9"),
    (Space, "space"),
    (Enter, "Return"),
    (Tab, "Tab"),
    (Backspace, "BackSpace"),
    (Backquote, "`"),
    (Minus, "-"),
    (Equal, "="),
    (BracketLeft, "["),
    (BracketRight, "]"),
    (Backslash, "\\"),
    (Semicolon, ";"),
    (Quote, "'"),
    (Comma, ","),
    (Period, "."),
    (Slash, "/"),
    (ArrowLeft, "Left"),
    (ArrowRight, "Right"),
    (ArrowUp, "Up"),
    (ArrowDown, "Down"),
    (Home, "Home"),
    (End, "End"),
    (PageUp, "Page_Up"),
    (PageDown, "Page_Down"),
    (Insert, "Insert"),
    (Delete, "Delete"),
    (ScrollLock, "Scroll_Lock"),
    (AltLeft, "Alt"),
    (ControlLeft, "Control"),
    (ShiftLeft, "Shift"),
    (F1, "F1"),
    (F2, "F2"),
    (F3, "F3"),
    (F4, "F4"),
    (F5, "F5"),
    (F6, "F6"),
    (F7, "F7"),
    (F8, "F8"),
    (F9, "F9"),
    (F10, "F10"),
    (F11, "F11"),
    (F12, "F12"),
];

pub fn key_name(k: KeyCode) -> &'static str {
    KEYS.iter().find(|e| e.0 == k).map_or("?", |e| e.1)
}
/// Whether the control screen may bind this key (Esc cancels capture; unknown keys are ignored).
pub fn bindable(k: KeyCode) -> bool {
    KEYS.iter().any(|e| e.0 == k)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Screenshot,
    SpeedUp,
    SpeedDown,
    PanicFire,
    Primary,
    Secondary(u8), // 1..=10
    WeaponBar,
    ShipBar,
    StarMap,
    FocusSelected,
    NextShip,
    PrevShip,
    FocusProjectile,
    SkipSpeech,
    RotateLeft,
    RotateRight,
    ZoomIn,
    ZoomOut,
    ChangePrimary,
    AttackOnly,
}

/// Retail list order. `Secondary(n)` rows are generated; see `table()`.
fn table() -> Vec<(Act, String, KeyCode)> {
    let secondary = [Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Digit0];
    let mut t = vec![
        (Act::Screenshot, "Take ScreenShot Description".into(), ScrollLock),
        (Act::SpeedUp, "Increase Speed".into(), Equal),
        (Act::SpeedDown, "Decrease Speed".into(), Minus),
        (Act::PanicFire, "Panic Fire".into(), KeyX),
        (Act::Primary, "Select Primary Ship".into(), Backquote),
    ];
    t.extend(secondary.iter().enumerate().map(|(i, k)| (Act::Secondary(i as u8 + 1), format!("Select Secondary Ship ( {} )", i + 1), *k)));
    t.extend([
        (Act::WeaponBar, "Open Weapon Bar".into(), KeyW),
        (Act::ShipBar, "Open Ship Bar".into(), KeyS),
        (Act::StarMap, "Open Star Map".into(), KeyM),
        (Act::FocusSelected, "Focus On Selected Ship".into(), KeyF),
        (Act::NextShip, "Select Next Ship".into(), BracketRight),
        (Act::PrevShip, "Select Prev Ship".into(), BracketLeft),
        (Act::FocusProjectile, "Focus On Nearest Projectile".into(), KeyB),
        (Act::SkipSpeech, "Skip Speech Event".into(), Space),
        (Act::RotateLeft, "Rotate Camera Left".into(), ArrowLeft),
        (Act::RotateRight, "Rotate Camera Right".into(), ArrowRight),
        (Act::ZoomIn, "Zoom Camera In".into(), ArrowUp),
        (Act::ZoomOut, "Zoom Camera Out".into(), ArrowDown),
        (Act::ChangePrimary, "Change Primary Ship".into(), KeyP),
        (Act::AttackOnly, "Attack Only Mode".into(), AltLeft),
    ]);
    t
}

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Bindings {
    pub rows: Vec<(Act, String, KeyCode)>,
}

impl Default for Bindings {
    fn default() -> Self {
        Self { rows: table() }
    }
}

impl Bindings {
    pub fn key(&self, a: Act) -> KeyCode {
        self.rows.iter().find(|r| r.0 == a).map_or(Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified), |r| r.2)
    }
    pub fn pressed(&self, keys: &ButtonInput<KeyCode>, a: Act) -> bool {
        keys.pressed(self.key(a))
    }
    pub fn just_pressed(&self, keys: &ButtonInput<KeyCode>, a: Act) -> bool {
        keys.just_pressed(self.key(a))
    }
    /// Bind row `i` to `k`; a key already used by another row is swapped onto this row's old key so no key is ever doubled.
    pub fn rebind(&mut self, i: usize, k: KeyCode) {
        let old = self.rows[i].2;
        if let Some(o) = self.rows.iter_mut().enumerate().find(|(j, r)| *j != i && r.2 == k) {
            o.1.2 = old;
        }
        self.rows[i].2 = k;
    }
    pub fn load() -> Self {
        let mut b = Self::default();
        let saved: HashMap<String, String> = path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        for r in &mut b.rows {
            if let Some(k) = saved.get(&r.1).and_then(|n| KEYS.iter().find(|e| e.1 == n)) {
                r.2 = k.0;
            }
        }
        b
    }
    pub fn save(&self) {
        let map: std::collections::BTreeMap<_, _> = self.rows.iter().map(|r| (r.1.as_str(), key_name(r.2))).collect();
        if let Some(p) = path() {
            let _ = p.parent().map(std::fs::create_dir_all);
            if let Err(e) = std::fs::write(p, serde_json::to_string_pretty(&map).unwrap_or_default()) {
                warn!("could not save key bindings: {e}");
            }
        }
    }
}

fn path() -> Option<std::path::PathBuf> {
    Some(crate::campaign::user_dir()?.join("bindings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_unique_and_named() {
        let b = Bindings::default();
        assert_eq!(b.rows.len(), 29);
        for r in &b.rows {
            assert!(bindable(r.2), "{}", r.1);
            assert_eq!(b.rows.iter().filter(|o| o.2 == r.2).count(), 1, "{}", r.1);
        }
    }
    #[test]
    fn rebinding_to_a_used_key_swaps() {
        let mut b = Bindings::default();
        b.rebind(1, KeyW); // Increase Speed (=) takes W from Open Weapon Bar
        assert_eq!((b.key(Act::SpeedUp), b.key(Act::WeaponBar)), (KeyW, Equal));
    }
}
