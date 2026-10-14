//! Retail sprite and font access (tools/export_hud.py, tools/export_fonts.py): `HudData` (hud.hdt components per faction art set),
//! sprite rects and offsets in 640x480 reference pixels, HD sheet scales, 9-slice strips and the `Fonts` stand-ins. Shared helpers:
//! any view may use retail art through these.
use bevy::prelude::*;
use serde_json::Value;
use std::collections::HashMap;

use crate::{Library, PlayerShip, Ship, assets_dir};

#[derive(Resource)]
pub struct HudData {
    pub comps: Value,
    /// Art set of the player's faction ("Navy", "Pirate" or "Procyon"): the in-game HUD changes layout and sprites with it.
    pub skin: &'static str,
}
/// Front-end screens always use the Navy art.
pub const MENU_SKIN: &str = "Navy";

/// HUD skin of a ship race; races without their own set (civilians) use the Navy one.
pub fn skin_of(race: &str) -> &'static str {
    match race {
        "Pirate" => "Pirate",
        "Procyon" => "Procyon",
        _ => "Navy",
    }
}

pub fn choose_skin(mut hud: ResMut<HudData>, lib: Res<Library>, player: Single<&Ship, With<PlayerShip>>) {
    let skin = skin_of(&lib.classes[player.class].race);
    if hud.skin != skin {
        hud.skin = skin;
    }
}

pub fn sprite_of(h: &HudData, name: &str) -> Option<(String, Rect)> {
    sprite_area(h, name, "TextureArea - NoCursor")
}
/// Like `sprite_of` without trailing fully transparent columns (`tools/trim_hud_rects.py`): what parts that butt against each other need.
pub fn sprite_trimmed(h: &HudData, name: &str) -> Option<(String, Rect)> {
    sprite_area(h, name, "TextureAreaTrimmed - NoCursor").or_else(|| sprite_of(h, name))
}
pub fn sprite_area(h: &HudData, name: &str, key: &str) -> Option<(String, Rect)> {
    let d = h.comps.get(name)?.get(h.skin)?;
    let r = d[key].as_array()?;
    let n = |i: usize| r[i].as_f64().unwrap_or(0.0) as f32;
    let state = key.rsplit(" - ").next().unwrap_or("NoCursor"); // the sheet of the same cursor state
    let sheet = d[format!("TextureName - {state}")].as_str().filter(|s| !s.is_empty())?; // a skin without this part leaves the name empty
    Some((sheet.to_lowercase(), Rect::new(n(0), n(1), n(2) + 1.0, n(3) + 1.0)))
}
/// Texture-pixels per reference pixel for a sheet: 1 for retail art, 2 for a 2x replacement. Mods declare it in `hud/scales.json`
/// (`{"sheet_name": 2}`); layout always stays in reference pixels, only the source rectangle scales.
pub fn sheet_scale(sheet: &str) -> f32 {
    static S: std::sync::OnceLock<HashMap<String, f32>> = std::sync::OnceLock::new();
    S.get_or_init(|| std::fs::read_to_string(crate::assets_dir().join("hud/scales.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default())
        .get(sheet)
        .copied()
        .unwrap_or(1.0)
}
/// Source rect of a reference-pixel sprite in its sheet's own pixels (see `sheet_scale`).
pub fn src(sheet: &str, r: Rect) -> Rect {
    let k = sheet_scale(sheet);
    Rect::new(r.min.x * k, r.min.y * k, r.max.x * k, r.max.y * k)
}
/// A left-cap | stretchy-centre | right-cap strip from `tools/make_slices.py`, drawn as one Bevy 9-slice node. Caps are in reference
/// pixels and grow with the node height, so it stays seam-free at any UI scale or HD replacement.
pub fn sliced(assets: &AssetServer, group: &str, skin: &str, state: &str, left: f32, right: f32) -> ImageNode {
    let name = format!("slice_{group}_{}_{state}", skin.to_lowercase());
    let k = sheet_scale(&name);
    let mut img = ImageNode::new(assets.load(format!("hud/tex/{name}.png")));
    img.image_mode = NodeImageMode::Sliced(TextureSlicer {
        border: BorderRect { min_inset: Vec2::new(left * k, 0.0), max_inset: Vec2::new(right * k, 0.0) },
        center_scale_mode: SliceScaleMode::Stretch,
        max_corner_scale: 16.0,
        ..default()
    });
    img
}
/// Half-texel inset so bilinear sampling at non-integer UI scales does not pull in neighbouring sprites of the sheet.
pub fn inset(sheet: &str, r: Rect) -> Rect {
    let r = src(sheet, r);
    Rect::new(r.min.x + 0.5, r.min.y + 0.5, r.max.x - 0.5, r.max.y - 0.5)
}
pub fn offset(h: &HudData, name: &str) -> Vec2 {
    h.comps.get(name).and_then(|c| c.get(h.skin)).map_or(Vec2::ZERO, |d| Vec2::new(d["TopLeft X"].as_f64().unwrap_or(0.0) as f32, d["TopLeft Y"].as_f64().unwrap_or(0.0) as f32))
}

/// Image node (with its half-texel inset rect) and reference size of a hud.hdt sprite.
pub fn image(assets: &AssetServer, hud: &HudData, name: &str) -> Option<(ImageNode, Vec2)> {
    let (sheet, rect) = sprite_of(hud, name)?;
    let size = rect.size();
    let mut img = ImageNode::new(assets.load(format!("hud/tex/{sheet}.png")));
    img.rect = Some(inset(&sheet, rect));
    Some((img, size))
}

pub fn plugin(app: &mut App) {
    let path = assets_dir().join("hud/hud.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        warn!("no HUD data at {path:?}; run tools/build_assets.sh");
        return;
    };
    let comps: Value = serde_json::from_str(&text).expect("hud.json");
    app.insert_resource(HudData { comps, skin: "Navy" }).add_systems(PostStartup, load_fonts).add_systems(Update, choose_skin.run_if(in_state(crate::menu::Screen::Battle)));
}

// ---- bitmap fonts (Fonts/*.fft) --------------------------------------------------------------------------------

/// A retail font slot (name and line height from Fonts/*.fft) drawn with a scalable vector face instead of the bitmap sheet.
pub struct Font {
    pub face: Handle<bevy::text::Font>,
    metrics: std::sync::Arc<ab_glyph::FontVec>,
    pub size: f32,
    pub height: f32,
}
#[derive(Resource, Default)]
pub struct Fonts(pub HashMap<String, Font>);

fn load_fonts(mut commands: Commands, assets: Res<AssetServer>, mut faces_assets: ResMut<Assets<bevy::text::Font>>) {
    // Plain Text (subtitles, objectives, debug) uses the same sans instead of Bevy's monospace default.
    if let Ok(b) = std::fs::read(assets_dir().join("fonts/Sans.otf")) {
        let _ = faces_assets.insert(Handle::<bevy::text::Font>::default().id(), bevy::text::Font::from_bytes(b));
    }
    let mut fonts = Fonts::default();
    let Ok(dir) = std::fs::read_dir(assets_dir().join("hud/fonts")) else {
        commands.insert_resource(fonts);
        return;
    };
    let mut faces: HashMap<&str, (Handle<bevy::text::Font>, std::sync::Arc<ab_glyph::FontVec>)> = HashMap::new();
    for e in dir.flatten() {
        let Ok(v) = serde_json::from_str::<Value>(&std::fs::read_to_string(e.path()).unwrap_or_default()) else { continue };
        let name = e.path().file_stem().unwrap().to_string_lossy().to_string();
        // Retail names carry the nominal size ("BlackChancery22"); the face is a stand-in with the same character (Chancery, Univers-like sans).
        let (file, k) = if name.starts_with("BlackChancery") {
            ("Chancery", 1.1)
        } else if name.contains("Bold") {
            ("SansBold", 0.95)
        } else {
            ("Sans", 0.95)
        };
        let size = name.trim_start_matches(|c: char| !c.is_ascii_digit()).parse::<f32>().unwrap_or(12.0) * k;
        let (face, metrics) = faces
            .entry(file)
            .or_insert_with(|| {
                let bytes = std::fs::read(assets_dir().join(format!("fonts/{file}.otf"))).expect("UI font");
                (assets.load(format!("fonts/{file}.otf")), std::sync::Arc::new(ab_glyph::FontVec::try_from_vec(bytes).expect("UI font parse")))
            })
            .clone();
        fonts.0.insert(name, Font { face, metrics, size, height: v["height"].as_f64().unwrap_or(0.0) as f32 });
    }
    commands.insert_resource(fonts);
}

impl Font {
    /// Text width in reference px.
    pub fn width(&self, text: &str) -> f32 {
        use ab_glyph::{Font as _, ScaleFont};
        let f = self.metrics.as_scaled(ab_glyph::PxScale::from(self.size));
        text.chars().map(|c| f.h_advance(f.glyph_id(c))).sum()
    }
    /// Text components at `scale` (reference px to screen px).
    pub fn text(&self, s: &str, scale: f32, color: Color) -> impl Bundle {
        (Text::new(s), TextFont { font: self.face.clone().into(), font_size: FontSize::Px(self.size * scale), ..default() }, TextColor(color))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skin_follows_ship_race() {
        assert_eq!([skin_of("Navy"), skin_of("Pirate"), skin_of("Procyon"), skin_of("Civilian")], ["Navy", "Pirate", "Procyon", "Navy"]);
    }
}
