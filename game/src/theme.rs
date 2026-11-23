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
    /// The tube rim, outside in: a dark outline, the gold (a bright core line in its middle), a dark keyline; a hard dark band under the
    /// bottom edge.
    pub rim_outline: Color,
    pub rim_core: Color,
    pub rim_keyline: Color,
    pub rim_shadow: Color,
    /// The shadow along the inside of a filled shape, and how far it reaches (reference px) in panels and in boxes.
    pub inset_shadow: Color,
    pub inset_size: f32,
    pub box_inset_size: f32,
    /// Soft teal glow along the inside edge of a box, and its reach.
    pub glow: Color,
    pub glow_size: f32,
    /// Scrollbar and slider track fill (top, bottom), thumb and knob cylinder (left, right), check-box tick.
    pub track_fill: [Color; 2],
    pub thumb: [Color; 2],
    pub tick: Color,
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

/// One state of a drawn control: a five-stop vertical gradient fill (top edge, lit peak, dark body, bottom reflection, bottom edge) and
/// two rim colours (a disabled button's gold, a pull-down's glow).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub fill: [Color; 5],
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
                if let Some(c) = list("fill", 5) {
                    slot.fill = [c[0], c[1], c[2], c[3], c[4]];
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

/// The theme file's place in the asset root (the shipped one is overlaid there, a mod replaces it).
const THEME_FILE: &str = "data/hud/theme.json";

/// The shipped `content/data/hud/theme.json`, compiled in: the one place the default numbers live.
const SHIPPED: &str = include_str!("../../content/data/hud/theme.json");

impl Default for Theme {
    /// The shipped file applied over a blank theme (a malformed or missing key there would show as black or zero).
    fn default() -> Self {
        let v: Value = serde_json::from_str(SHIPPED).expect("content/data/hud/theme.json is valid JSON");
        Self::from_json_over(Self::blank(), &v)
    }
}

impl Theme {
    /// Every colour black, every length zero: only the base `from_json_over` starts from.
    fn blank() -> Self {
        let k = Color::BLACK;
        let l = Look { fill: [k; 5], rim: [k; 2] };
        let looks = Looks { idle: l, hover: l, pressed: l, selected: l, disabled: l, rim_width: 0.0 };
        Self {
            panel_fill: k,
            box_fill: k,
            band_fill: k,
            edge: k,
            window_fill: k,
            window_rim: k,
            heading: k,
            picked: k,
            rim: k,
            rim_light: k,
            rim_width: 0.0,
            rim_outline: k,
            rim_core: k,
            rim_keyline: k,
            rim_shadow: k,
            inset_shadow: k,
            inset_size: 0.0,
            box_inset_size: 0.0,
            glow: k,
            glow_size: 0.0,
            track_fill: [k; 2],
            thumb: [k; 2],
            tick: k,
            edge_width: 0.0,
            panel_radius: 0.0,
            box_radius: 0.0,
            slot_radius: 0.0,
            shadow_alpha: 0.0,
            shadow_blur: 0.0,
            button: looks.clone(),
            pulldown: looks,
        }
    }

    /// The defaults with every key present in `v` applied (a malformed value keeps its default).
    pub fn from_json(v: &Value) -> Self {
        Self::from_json_over(Self::default(), v)
    }

    fn from_json_over(base: Self, v: &Value) -> Self {
        let mut t = base;
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
            (&mut t.rim_outline, "rim_outline"),
            (&mut t.rim_core, "rim_core"),
            (&mut t.rim_keyline, "rim_keyline"),
            (&mut t.rim_shadow, "rim_shadow"),
            (&mut t.inset_shadow, "inset_shadow"),
            (&mut t.glow, "glow"),
            (&mut t.tick, "tick"),
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
            (&mut t.inset_size, "inset_size"),
            (&mut t.box_inset_size, "box_inset_size"),
            (&mut t.glow_size, "glow_size"),
        ] {
            *slot = num(key).filter(|n| *n >= 0.0).unwrap_or(*slot);
        }
        for (slot, key) in [(&mut t.track_fill, "track_fill"), (&mut t.thumb, "thumb")] {
            let pair: Option<Vec<Color>> = v.get(key).and_then(Value::as_array).and_then(|a| a.iter().map(|c| hex(c.as_str()?)).collect());
            if let Some(p) = pair.filter(|p| p.len() == 2) {
                *slot = [p[0], p[1]];
            }
        }
        t.button = Looks::from_json(v.get("button"), t.button);
        t.pulldown = Looks::from_json(v.get("pulldown"), t.pulldown);
        t
    }

    /// The theme of the loaded assets (`data/hud/theme.json`, overlaid by mods), read once per process; the defaults without the file.
    pub fn current() -> &'static Theme {
        static T: std::sync::OnceLock<Theme> = std::sync::OnceLock::new();
        T.get_or_init(|| Self::from_files(&crate::assets_dir().join(THEME_FILE), Some(&crate::base_assets().join(THEME_FILE))))
    }

    /// The theme in `path`, over the shipped defaults. A file that is present but not JSON (or not an object) is reported with its
    /// file and mod and the base file's theme is used instead, else the shipped one; a wrong value inside is skipped key by key,
    /// as `from_json_over` always did (a mod's theme names only what it changes).
    fn from_files(path: &std::path::Path, base: Option<&std::path::Path>) -> Theme {
        match crate::data::load_or_base::<serde_json::Map<String, Value>>(path, base, "the shipped theme is used") {
            Some(map) => Theme::from_json(&Value::Object(map)),
            None => Theme::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bad_theme_file_is_reported_and_the_shipped_theme_stays() {
        let dir = std::env::temp_dir().join(format!("procyon-theme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let shipped = Theme::default();
        assert_eq!(Theme::from_files(&dir.join("absent.json"), None), shipped, "no file: the defaults");
        for (name, text) in [("syntax", "{ \"rim_width\": "), ("array", "[1, 2]"), ("number", "4")] {
            std::fs::write(dir.join(format!("{name}.json")), text).unwrap();
            assert_eq!(Theme::from_files(&dir.join(format!("{name}.json")), None), shipped, "{name}");
        }
        std::fs::write(dir.join("base.json"), r#"{"rim_width": 7}"#).unwrap();
        let from_base = Theme::from_files(&dir.join("syntax.json"), Some(&dir.join("base.json")));
        assert_eq!(from_base.rim_width, 7.0, "the base file stands in for the broken one");
        std::fs::write(dir.join("good.json"), r#"{"rim_width": 5, "unknown_key": 1}"#).unwrap();
        assert_eq!(Theme::from_files(&dir.join("good.json"), None).rim_width, 5.0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_defaults_come_from_the_shipped_file() {
        let d = Theme::default();
        assert_eq!((d.rim_width, d.button.rim_width, d.pulldown.rim_width), (4.0, 3.0, 0.0));
        assert_eq!(d.panel_fill, hex("#001A44").unwrap().with_alpha(0.82));
        assert_eq!(d.button.hover.fill[0], hex("#2FD3EB").unwrap());
    }

    #[test]
    fn every_key_of_the_shipped_file_is_read() {
        // Dropping any one key must change the theme: a key nothing reads (a typo) or one the default silently fills in would not.
        let full: Value = serde_json::from_str(SHIPPED).unwrap();
        let reference = Theme::from_json_over(Theme::blank(), &full);
        for key in full.as_object().unwrap().keys().filter(|k| !k.starts_with('_')) {
            let mut v = full.clone();
            v.as_object_mut().unwrap().remove(key);
            // panel_alpha equals the fallback alpha of panel_fill (0.82), so dropping it alone changes nothing visible.
            if key != "panel_alpha" {
                assert_ne!(Theme::from_json_over(Theme::blank(), &v), reference, "{key} is not read");
            }
        }
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
    fn the_depth_keys_overlay_one_at_a_time() {
        let t = Theme::from_json(
            &serde_json::json!({"rim_core": "#FFFFFF", "inset_size": 3, "glow_size": -2, "track_fill": ["#102030", "#405060"], "thumb": ["#000000"], "tick": "#00FF00"}),
        );
        let d = Theme::default();
        assert_eq!((t.rim_core, t.inset_size, t.tick), (Color::srgb_u8(255, 255, 255), 3.0, Color::srgb_u8(0, 255, 0)));
        // A negative size and a one-colour pair are malformed: they keep their defaults.
        assert_eq!((t.glow_size, t.thumb, t.rim_outline, t.box_inset_size), (d.glow_size, d.thumb, d.rim_outline, d.box_inset_size));
        assert_eq!(t.track_fill[1], Color::srgb_u8(0x40, 0x50, 0x60));
    }

    #[test]
    fn a_mod_overlay_changes_only_the_keys_it_names() {
        let t = Theme::from_json(&serde_json::json!({"rim": "#112233", "rim_width": 5, "box_radius": "bad", "edge": "nope"}));
        assert_eq!(t.rim, Color::srgb_u8(0x11, 0x22, 0x33));
        assert_eq!((t.rim_width, t.box_radius, t.edge), (5.0, 4.0, Theme::default().edge));
        let t = Theme::from_json(&serde_json::json!({"button": {"hover": {"fill": ["#000000", "#111111", "#222222", "#333333", "#444444"], "rim": ["#333333"]}, "rim_width": 1}}));
        assert_eq!(t.button.hover.fill[4], Color::srgb_u8(0x44, 0x44, 0x44));
        // A malformed list (one rim colour, not two) keeps its default.
        assert_eq!((t.button.hover.rim, t.button.rim_width, &t.button.idle), (Theme::default().button.hover.rim, 1.0, &Theme::default().button.idle));
    }
}
