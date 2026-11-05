//! The front-end theme: the palette and geometry of the chrome the classic view draws itself (panels now; buttons, scrollbars and windows
//! follow). Numbers live in `data/hud/theme.json` (shipped in `content/`, a mod overlays it); every value is PROVISIONAL, measured on the
//! retail Navy art. This module only holds data; `classic/widgets.rs` turns it into UI nodes.

use bevy::color::{Alpha, Color};
use serde_json::Value;

/// Colours (sRGB) and lengths (800x600 reference px) of the drawn chrome.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// Fill of the big outer panels (translucent: the scenery shows through).
    pub panel_fill: Color,
    /// Fill of the inner list / information boxes.
    pub box_fill: Color,
    /// The lighter band along the bottom of a list box.
    pub band_fill: Color,
    /// Teal hairline around an inner box.
    pub edge: Color,
    /// Framed windows (map list, summary table, lobby players): translucent fill, teal rim and glow.
    pub window_fill: Color,
    pub window_rim: Color,
    /// Heading band across the top of a framed table, and the bar behind the picked row of a list.
    pub heading: Color,
    pub picked: Color,
    /// Gold rim of a panel (the shaded sides) and its highlight (top and left).
    pub rim: Color,
    pub rim_light: Color,
    pub rim_width: f32,
    pub edge_width: f32,
    pub panel_radius: f32,
    pub box_radius: f32,
    /// Corner radius of the small slot frames.
    pub slot_radius: f32,
    /// Opacity and blur (reference px) of the soft shadow under a panel.
    pub shadow_alpha: f32,
    pub shadow_blur: f32,
    /// Text buttons (a pill with a gold rim) and the pull-down boxes.
    pub button: Looks,
    pub pulldown: Looks,
}

/// `#RRGGBB` or `#RRGGBBAA`.
fn hex(s: &str) -> Option<Color> {
    let s = s.strip_prefix('#')?;
    let v = u32::from_str_radix(s, 16).ok().filter(|_| s.len() == 6 || s.len() == 8)?;
    let (rgb, a) = if s.len() == 8 { (v >> 8, v as u8) } else { (v, 255) };
    Some(Color::srgba_u8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, a))
}

/// One state of a drawn control: a top / middle / bottom vertical gradient fill and the rim colours at the top and bottom edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub fill: [Color; 3],
    pub rim: [Color; 2],
}

/// The five states of a drawn control (`classic/widgets.rs` maps the cursor and the screen's rules onto them).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    Idle,
    Hover,
    Pressed,
    Selected,
    Disabled,
}

/// The looks of one control family (text buttons, pull-downs).
#[derive(Clone, Debug, PartialEq)]
pub struct Looks {
    pub idle: Look,
    pub hover: Look,
    pub pressed: Look,
    pub selected: Look,
    pub disabled: Look,
    /// Rim width in reference px.
    pub rim_width: f32,
}

impl Looks {
    pub fn get(&self, state: State) -> &Look {
        match state {
            State::Idle => &self.idle,
            State::Hover => &self.hover,
            State::Pressed => &self.pressed,
            State::Selected => &self.selected,
            State::Disabled => &self.disabled,
        }
    }

    fn from_json(v: Option<&Value>, mut base: Looks) -> Looks {
        let Some(v) = v else { return base };
        for (slot, key) in
            [(&mut base.idle, "idle"), (&mut base.hover, "hover"), (&mut base.pressed, "pressed"), (&mut base.selected, "selected"), (&mut base.disabled, "disabled")]
        {
            if let Some(look) = v.get(key) {
                let list = |name: &str, n: usize| -> Option<Vec<Color>> {
                    let c: Option<Vec<Color>> = look.get(name)?.as_array()?.iter().map(|c| hex(c.as_str()?)).collect();
                    c.filter(|c| c.len() == n)
                };
                if let Some(c) = list("fill", 3) {
                    slot.fill = [c[0], c[1], c[2]];
                }
                if let Some(c) = list("rim", 2) {
                    slot.rim = [c[0], c[1]];
                }
            }
        }
        if let Some(w) = v.get("rim_width").and_then(Value::as_f64).filter(|w| *w >= 0.0) {
            base.rim_width = w as f32;
        }
        base
    }
}

fn look(fill: [&str; 3], rim: [&str; 2]) -> Look {
    let c = |s| hex(s).unwrap_or(Color::BLACK);
    Look { fill: fill.map(c), rim: rim.map(c) }
}

impl Default for Theme {
    fn default() -> Self {
        let c = |s| hex(s).unwrap_or(Color::BLACK);
        Self {
            panel_fill: c("#001A44").with_alpha(0.82),
            box_fill: c("#00103E"),
            band_fill: c("#0F2E63"),
            edge: c("#0F7070"),
            window_fill: c("#0E478AB3"),
            window_rim: c("#2FC4D4"),
            heading: c("#40BFD938"),
            picked: c("#33B3CC73"),
            rim: c("#9B721F"),
            rim_light: c("#FFB626"),
            rim_width: 3.0,
            edge_width: 2.0,
            panel_radius: 20.0,
            box_radius: 4.0,
            slot_radius: 6.0,
            shadow_alpha: 0.5,
            shadow_blur: 8.0,
            button: Looks {
                idle: look(["#5186A3", "#1A4181", "#042657"], ["#E1A233", "#CB932E"]),
                hover: look(["#4BAFDA", "#2583AD", "#0E5D6D"], ["#F0B545", "#D9A03A"]),
                pressed: look(["#003830", "#007A66", "#00B89C"], ["#CB932E", "#E1A233"]),
                selected: look(["#004A40", "#00987E", "#00EECA"], ["#E1A233", "#CB932E"]),
                disabled: look(["#4A5560", "#2A3038", "#14181E"], ["#7A6A48", "#5E5238"]),
                rim_width: 2.0,
            },
            pulldown: Looks {
                idle: look(["#00133A", "#00133A", "#00133A"], ["#00D0E0", "#00B0C0"]),
                hover: look(["#0A2A5A", "#0A2A5A", "#0A2A5A"], ["#40E8F4", "#20D0DC"]),
                pressed: look(["#0C3A78", "#0C3A78", "#0C3A78"], ["#40E8F4", "#20D0DC"]),
                selected: look(["#0B376F", "#0C3A78", "#0B376F"], ["#1FA8C8", "#0A7F9F"]),
                disabled: look(["#10182A", "#10182A", "#10182A"], ["#3A5058", "#2E4048"]),
                rim_width: 2.0,
            },
        }
    }
}

impl Theme {
    /// The defaults with every key present in `v` applied (a malformed value keeps its default).
    pub fn from_json(v: &Value) -> Self {
        let mut t = Self::default();
        let colour = |key: &str| v.get(key).and_then(Value::as_str).and_then(hex);
        let num = |key: &str| v.get(key).and_then(Value::as_f64).map(|n| n as f32);
        if let Some(c) = colour("panel_fill") {
            t.panel_fill = c.with_alpha(num("panel_alpha").unwrap_or(0.82).clamp(0.0, 1.0));
        } else if let Some(a) = num("panel_alpha") {
            t.panel_fill = t.panel_fill.with_alpha(a.clamp(0.0, 1.0));
        }
        for (slot, key) in [
            (&mut t.box_fill, "box_fill"),
            (&mut t.band_fill, "band_fill"),
            (&mut t.edge, "edge"),
            (&mut t.window_fill, "window_fill"),
            (&mut t.window_rim, "window_rim"),
            (&mut t.heading, "heading"),
            (&mut t.picked, "picked"),
            (&mut t.rim, "rim"),
            (&mut t.rim_light, "rim_light"),
        ] {
            *slot = colour(key).unwrap_or(*slot);
        }
        for (slot, key) in [
            (&mut t.rim_width, "rim_width"),
            (&mut t.edge_width, "edge_width"),
            (&mut t.panel_radius, "panel_radius"),
            (&mut t.box_radius, "box_radius"),
            (&mut t.slot_radius, "slot_radius"),
            (&mut t.shadow_alpha, "shadow_alpha"),
            (&mut t.shadow_blur, "shadow_blur"),
        ] {
            *slot = num(key).filter(|n| *n >= 0.0).unwrap_or(*slot);
        }
        t.button = Looks::from_json(v.get("button"), t.button);
        t.pulldown = Looks::from_json(v.get("pulldown"), t.pulldown);
        t
    }

    /// The theme of the loaded assets (`data/hud/theme.json`, overlaid by mods), read once per process; the defaults without the file.
    pub fn current() -> &'static Theme {
        static T: std::sync::OnceLock<Theme> = std::sync::OnceLock::new();
        T.get_or_init(|| crate::data::try_json(&crate::assets_dir().join("data/hud/theme.json")).map(|v| Theme::from_json(&v)).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_theme_file_matches_the_defaults() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../content/data/hud/theme.json");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(Theme::from_json(&v), Theme::default());
        assert_eq!(Theme::default().rim_width, 3.0);
    }

    #[test]
    fn hex_takes_six_or_eight_digits() {
        assert_eq!(hex("#102030"), Some(Color::srgba_u8(0x10, 0x20, 0x30, 255)));
        assert_eq!(hex("#10203080"), Some(Color::srgba_u8(0x10, 0x20, 0x30, 0x80)));
        assert_eq!((hex("#12345"), hex("123456"), hex("#gg0000")), (None, None, None));
    }

    #[test]
    fn every_state_has_its_own_look() {
        let t = Theme::default();
        let states = [State::Idle, State::Hover, State::Pressed, State::Selected, State::Disabled];
        for family in [&t.button, &t.pulldown] {
            for (i, a) in states.iter().enumerate() {
                for b in &states[i + 1..] {
                    assert_ne!(family.get(*a), family.get(*b), "{a:?} vs {b:?}");
                }
            }
        }
    }

    #[test]
    fn a_mod_overlay_changes_only_the_keys_it_names() {
        let t = Theme::from_json(&serde_json::json!({"rim": "#112233", "rim_width": 5, "box_radius": "bad", "edge": "nope"}));
        assert_eq!(t.rim, Color::srgb_u8(0x11, 0x22, 0x33));
        assert_eq!((t.rim_width, t.box_radius, t.edge), (5.0, 4.0, Theme::default().edge));
        let t = Theme::from_json(&serde_json::json!({"button": {"hover": {"fill": ["#000000", "#111111", "#222222"], "rim": ["#333333"]}, "rim_width": 1}}));
        assert_eq!(t.button.hover.fill[2], Color::srgb_u8(0x22, 0x22, 0x22));
        // A malformed list (one rim colour, not two) keeps its default.
        assert_eq!((t.button.hover.rim, t.button.rim_width, &t.button.idle), (Theme::default().button.hover.rim, 1.0, &Theme::default().button.idle));
    }
}
