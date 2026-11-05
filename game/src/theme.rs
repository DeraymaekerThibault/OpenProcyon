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
    /// Gold rim of a panel (the shaded sides) and its highlight (top and left).
    pub rim: Color,
    pub rim_light: Color,
    pub rim_width: f32,
    pub edge_width: f32,
    pub panel_radius: f32,
    pub box_radius: f32,
    /// Opacity and blur (reference px) of the soft shadow under a panel.
    pub shadow_alpha: f32,
    pub shadow_blur: f32,
}

fn hex(s: &str) -> Option<Color> {
    let s = s.strip_prefix('#')?;
    let v = u32::from_str_radix(s, 16).ok().filter(|_| s.len() == 6)?;
    Some(Color::srgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

impl Default for Theme {
    fn default() -> Self {
        let c = |s| hex(s).unwrap_or(Color::BLACK);
        Self {
            panel_fill: c("#001A44").with_alpha(0.82),
            box_fill: c("#00103E"),
            band_fill: c("#0F2E63"),
            edge: c("#0F7070"),
            rim: c("#9B721F"),
            rim_light: c("#FFB626"),
            rim_width: 3.0,
            edge_width: 2.0,
            panel_radius: 20.0,
            box_radius: 4.0,
            shadow_alpha: 0.5,
            shadow_blur: 8.0,
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
        for (slot, key) in [(&mut t.box_fill, "box_fill"), (&mut t.band_fill, "band_fill"), (&mut t.edge, "edge"), (&mut t.rim, "rim"), (&mut t.rim_light, "rim_light")] {
            *slot = colour(key).unwrap_or(*slot);
        }
        for (slot, key) in [
            (&mut t.rim_width, "rim_width"),
            (&mut t.edge_width, "edge_width"),
            (&mut t.panel_radius, "panel_radius"),
            (&mut t.box_radius, "box_radius"),
            (&mut t.shadow_alpha, "shadow_alpha"),
            (&mut t.shadow_blur, "shadow_blur"),
        ] {
            *slot = num(key).filter(|n| *n >= 0.0).unwrap_or(*slot);
        }
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
    fn a_mod_overlay_changes_only_the_keys_it_names() {
        let t = Theme::from_json(&serde_json::json!({"rim": "#112233", "rim_width": 5, "box_radius": "bad", "edge": "nope"}));
        assert_eq!(t.rim, Color::srgb_u8(0x11, 0x22, 0x33));
        assert_eq!((t.rim_width, t.box_radius, t.edge), (5.0, 4.0, Theme::default().edge));
    }
}
