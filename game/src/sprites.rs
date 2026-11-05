//! Retail sprite and font access (tools/export_hud.py writes both): `HudData` (hud.hdt components per faction art set), one trimmed
//! PNG per sprite (`hud/sprites/<component>/<skin>/<state>.png`, layout in docs/data-format.md), 9-slice strips and the `Fonts`
//! stand-ins. Shared helpers: any view may use retail art through these.
//!
//! Layout is in 640x480 reference pixels. A `Sprite` is a whole PNG plus where it sits in its untrimmed reference box: the importer
//! cuts the transparent border off every sprite, `Sprite::view` puts the trimmed image back where the untrimmed one was, and the scale
//! of a sprite is its PNG size over its trimmed reference size, so a mod can replace one PNG with a copy at any resolution.
use bevy::prelude::*;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::{Library, PlayerShip, Ship, assets_dir};

#[derive(Resource)]
pub struct HudData {
    /// Shared so a view can look sprites up in another skin without copying the table.
    pub comps: Arc<Value>,
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

/// The idle cursor state of a hud.hdt component ("CursorOver" / "CursorDown" are the hover and press states).
pub const IDLE: &str = "NoCursor";
/// The retail body text font (menus, lists, star map labels).
pub const BODY_FONT: &str = "Univers12";

/// Reference pixels cut off the edges of a sprite's reference box by the importer (all of them fully transparent).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Trim {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// Size and trim of one state in a `<skin>.json` file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Meta {
    /// The untrimmed reference size (the hud.hdt rectangle).
    pub size: Vec2,
    pub trim: Trim,
}
impl Meta {
    fn parse(v: &Value) -> Option<Meta> {
        let n = |a: &Value, i: usize| a.get(i).and_then(Value::as_f64).map(|x| x as f32);
        let (size, trim) = (v.get("size")?, v.get("trim")?);
        Some(Meta { size: Vec2::new(n(size, 0)?, n(size, 1)?), trim: Trim { left: n(trim, 0)?, top: n(trim, 1)?, right: n(trim, 2)?, bottom: n(trim, 3)? } })
    }
    /// The part of the reference box that has art.
    pub fn inner(&self) -> Rect {
        inner_of(self.size, self.trim)
    }
}
fn inner_of(size: Vec2, t: Trim) -> Rect {
    Rect::new(t.left, t.top, (size.x - t.right).max(t.left), (size.y - t.bottom).max(t.top))
}

/// The state's entry in a `<skin>.json` `states` table: the state itself, else the idle art (a state the data draws like NoCursor
/// has no file of its own). Returns the state whose file to load.
fn pick_state<'a>(states: &'a Value, state: &'a str) -> Option<(&'a str, Meta)> {
    [state, IDLE].into_iter().find_map(|s| Some((s, Meta::parse(states.get(s)?)?)))
}

/// Image pixels per reference pixel of a sprite whose PNG is `image` pixels wide and high: 1 for the retail art, 2 for a 2x copy.
/// A missing file counts as retail resolution.
pub fn scale_of(image: Option<(u32, u32)>, meta: &Meta) -> f32 {
    let inner = meta.inner();
    match image {
        Some((w, _)) if inner.width() > 0.0 && w > 0 => w as f32 / inner.width(),
        _ => 1.0,
    }
}

/// Width and height of a PNG from its header, without decoding it.
fn png_size(path: &Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut head = [0u8; 24];
    std::fs::File::open(path).ok()?.read_exact(&mut head).ok()?;
    let signature = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    (head[..8] == signature).then(|| (u32::from_be_bytes(head[16..20].try_into().unwrap()), u32::from_be_bytes(head[20..24].try_into().unwrap())))
}

type Cache<T> = Mutex<HashMap<PathBuf, T>>;
/// `hud/sprites/<component>/<skin>.json`, read once per process (the asset folder does not change while the game runs).
fn sidecar(name: &str, skin: &str) -> Option<Arc<Value>> {
    static CACHE: std::sync::OnceLock<Cache<Option<Arc<Value>>>> = std::sync::OnceLock::new();
    let path = assets_dir().join(format!("hud/sprites/{name}/{skin}.json"));
    CACHE.get_or_init(Default::default).lock().unwrap().entry(path.clone()).or_insert_with(|| crate::data::try_json(&path).map(Arc::new)).clone()
}
fn image_size(path: PathBuf) -> Option<(u32, u32)> {
    static CACHE: std::sync::OnceLock<Cache<Option<(u32, u32)>>> = std::sync::OnceLock::new();
    *CACHE.get_or_init(Default::default).lock().unwrap().entry(path.clone()).or_insert_with(|| png_size(&path))
}

/// Size and trim of a sprite's state in an art set, without loading its image; a state without art of its own gives the idle one.
pub fn meta_in(skin: &str, name: &str, state: &str) -> Option<Meta> {
    pick_state(sidecar(name, skin)?.get("states")?, state).map(|(_, m)| m)
}
/// `meta_in` for the current faction's art set.
pub fn meta(h: &HudData, name: &str, state: &str) -> Option<Meta> {
    meta_in(h.skin, name, state)
}
/// Untrimmed reference size of a sprite (its hud.hdt rectangle).
pub fn size_of(h: &HudData, name: &str) -> Option<Vec2> {
    meta(h, name, IDLE).map(|m| m.size)
}

/// One loaded sprite: the whole (trimmed) image and where it sits in its untrimmed reference box.
#[derive(Clone, Debug, PartialEq)]
pub struct Sprite {
    pub image: Handle<Image>,
    /// Untrimmed reference size: layout always uses this, whatever the image is trimmed to.
    pub size: Vec2,
    pub trim: Trim,
    /// Image pixels per reference pixel (see `scale_of`).
    pub scale: f32,
}

/// The sprite `name` in an art set for a cursor state; a state without art of its own uses `NoCursor`.
pub fn sprite_in(assets: &AssetServer, skin: &str, name: &str, state: &str) -> Option<Sprite> {
    let side = sidecar(name, skin)?;
    let (used, meta) = pick_state(side.get("states")?, state)?;
    let path = format!("hud/sprites/{name}/{skin}/{used}.png");
    let scale = scale_of(image_size(assets_dir().join(&path)), &meta);
    Some(Sprite { image: assets.load(path), size: meta.size, trim: meta.trim, scale })
}
/// `sprite_in` for the current faction's art set.
pub fn sprite_state(assets: &AssetServer, h: &HudData, name: &str, state: &str) -> Option<Sprite> {
    sprite_in(assets, h.skin, name, state)
}
/// The idle art of the sprite `name` (see `sprite_state`).
pub fn sprite(assets: &AssetServer, h: &HudData, name: &str) -> Option<Sprite> {
    sprite_state(assets, h, name, IDLE)
}

/// How a sprite (or a part of its reference box) lands in a box: the margins and size of the node that draws its image, and which
/// part of the image shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Part of the image to show in image pixels; `None` for all of it.
    pub rect: Option<Rect>,
    /// Where the visible art starts and how big it is, in box units.
    pub offset: Vec2,
    pub size: Vec2,
    /// The box the region maps to.
    pub to: Vec2,
}

impl Sprite {
    /// `ImageNode` showing the whole image.
    pub fn image_node(&self) -> ImageNode {
        ImageNode::new(self.image.clone())
    }
    /// The whole reference box stretched over a box of `to` reference units.
    pub fn fit(&self, to: Vec2) -> View {
        self.view(Rect::from_corners(Vec2::ZERO, self.size), to)
    }
    /// The 1:1 view of the reference box.
    pub fn own(&self) -> View {
        self.fit(self.size)
    }
    /// The `region` of the untrimmed reference box (reference units) stretched over a box of `to` units: the part of it that has
    /// art, placed by its trim.
    pub fn view(&self, region: Rect, to: Vec2) -> View {
        let inner = inner_of(self.size, self.trim);
        let min = region.min.max(inner.min);
        let vis = Rect::from_corners(min, region.max.min(inner.max).max(min));
        let k = to / region.size().max(Vec2::splat(f32::EPSILON));
        let origin = Vec2::new(self.trim.left, self.trim.top);
        let rect = (vis != inner).then(|| Rect::from_corners((vis.min - origin) * self.scale, (vis.max - origin) * self.scale));
        View { rect, offset: (vis.min - region.min) * k, size: vis.size() * k, to }
    }
    /// Shows the sprite on a node that already draws sprites, keeping the box the node had (a swap changes the art, never the
    /// layout) and writing only what changes (a write re-renders the node).
    pub fn show(&self, img: &mut Mut<ImageNode>, node: &mut Mut<Node>, s: f32) {
        self.show_with(img, node, s, UiRect::DEFAULT);
    }
    /// `show` for the `region` of the reference box only (a crop of the sprite), keeping the node's box.
    pub fn show_region(&self, img: &mut Mut<ImageNode>, node: &mut Mut<Node>, s: f32, region: Rect) {
        let to = outer_box(node, UiRect::DEFAULT).map_or(region.size(), |px| px / s);
        self.view(region, to).flipped(img.flip_x, img.flip_y).show(&self.image, img, node, s, UiRect::DEFAULT);
    }
    /// `show` on a node whose margins carry `extra` for the layout (not part of the sprite's box).
    pub fn show_with(&self, img: &mut Mut<ImageNode>, node: &mut Mut<Node>, s: f32, extra: UiRect) {
        let to = outer_box(node, extra).map_or(self.size, |px| px / s);
        self.fit(to).flipped(img.flip_x, img.flip_y).show(&self.image, img, node, s, extra);
    }
}

/// Width and height in px of the box a sprite node covers: its size plus its margins, less the `extra` the layout added.
fn outer_box(node: &Node, extra: UiRect) -> Option<Vec2> {
    let px = |v: Val| if let Val::Px(p) = v { Some(p) } else { None };
    let m = node.margin;
    Some(Vec2::new(
        px(node.width)? + px(m.left)? + px(m.right)? - px(extra.left)? - px(extra.right)?,
        px(node.height)? + px(m.top)? + px(m.bottom)? - px(extra.top)? - px(extra.bottom)?,
    ))
}

fn add(v: Val, d: f32) -> Val {
    match v {
        Val::Px(p) => Val::Px(p + d),
        other => other,
    }
}
impl View {
    /// Mirrors the view with the image (`ImageNode::flip_x/flip_y`).
    pub fn flipped(mut self, x: bool, y: bool) -> View {
        if x {
            self.offset.x = self.to.x - self.offset.x - self.size.x;
        }
        if y {
            self.offset.y = self.to.y - self.offset.y - self.size.y;
        }
        self
    }
    /// The node size and margins that draw the image: the node's outer box (size plus margins, `extra` included) is the whole
    /// `to` box, so flex layout and absolute positions behave as if the untrimmed sprite were drawn.
    pub fn geometry(&self, s: f32, extra: UiRect) -> (Val, Val, UiRect) {
        // Edges, not sizes, are rounded: the three spans always add up to the snapped `to` box, so neighbours still butt.
        let (x, y) = (crate::panels::snap_span(self.offset.x, self.size.x, self.to.x, s), crate::panels::snap_span(self.offset.y, self.size.y, self.to.y, s));
        let margin = UiRect { left: add(extra.left, x.0), right: add(extra.right, x.2), top: add(extra.top, y.0), bottom: add(extra.bottom, y.2) };
        (Val::Px(x.1), Val::Px(y.1), margin)
    }
    /// The same placement as shares of the box, for a child of a node that is the box itself (its size changes every frame):
    /// left, top, width and height.
    pub fn fractions(&self) -> [Val; 4] {
        let p = |v: f32, whole: f32| Val::Percent(if whole > 0.0 { v / whole * 100.0 } else { 0.0 });
        [p(self.offset.x, self.to.x), p(self.offset.y, self.to.y), p(self.size.x, self.to.x), p(self.size.y, self.to.y)]
    }
    /// Sets the geometry on a node being built.
    pub fn place(&self, node: &mut Node, s: f32, extra: UiRect) {
        (node.width, node.height, node.margin) = self.geometry(s, extra);
    }
    /// Draws `image` through this view on an existing node, writing only what changes.
    pub fn show(&self, image: &Handle<Image>, img: &mut Mut<ImageNode>, node: &mut Mut<Node>, s: f32, extra: UiRect) {
        if img.image != *image {
            img.image = image.clone();
        }
        if img.rect != self.rect {
            img.rect = self.rect;
        }
        let (w, h, m) = self.geometry(s, extra);
        if (node.width, node.height, node.margin) != (w, h, m) {
            (node.width, node.height, node.margin) = (w, h, m);
        }
    }
}

/// Texture-pixels per reference pixel for a slice strip: 1 for retail art, 2 for a 2x replacement. Mods declare it in
/// `hud/scales.json` (`{"slice_button_navy_nocursor": 2}`); layout always stays in reference pixels. Sprites need no entry: their
/// scale comes from the size of their PNG.
pub fn sheet_scale(name: &str) -> f32 {
    static S: std::sync::OnceLock<HashMap<String, f32>> = std::sync::OnceLock::new();
    S.get_or_init(|| crate::data::try_json(&crate::assets_dir().join("hud/scales.json")).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
        .get(name)
        .copied()
        .unwrap_or(1.0)
}
/// The slice groups of `data/hud/slices.json` (the file `tools/make_slices.py` builds the strips from), read once per process. A mod
/// adds a group by overlaying the file and shipping the strip PNGs `hud/tex/slice_<group>_<skin>_<state>.png`.
fn slice_groups() -> &'static Value {
    static G: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    G.get_or_init(|| crate::data::read_json(&assets_dir().join("data/hud/slices.json")))
}

/// Left and right cap widths (reference px) of a slice group: its `caps`, else the widths of its end pieces in `skin`.
pub fn slice_caps(group: &str, skin: &str) -> Option<(f32, f32)> {
    let spec = slice_groups().get(group)?;
    if let Some([l, r]) = spec.get("caps").and_then(|c| c.as_array()).map(Vec::as_slice) {
        return Some((l.as_f64()? as f32, r.as_f64()? as f32));
    }
    let width = |end: &str| meta_in(skin, spec.get(end)?.as_str()?, IDLE).map(|m| m.size.x);
    Some((width("left")?, width("right")?))
}

/// The strip of slice group `group` in `skin` and cursor `state` (lower case, as in the file name) as a 9-slice node.
pub fn slice(assets: &AssetServer, group: &str, skin: &str, state: &str) -> Option<ImageNode> {
    let (left, right) = slice_caps(group, skin)?;
    Some(sliced(assets, group, skin, state, left, right))
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
pub fn offset(h: &HudData, name: &str) -> Vec2 {
    h.comps.get(name).and_then(|c| c.get(h.skin)).map_or(Vec2::ZERO, |d| Vec2::new(d["TopLeft X"].as_f64().unwrap_or(0.0) as f32, d["TopLeft Y"].as_f64().unwrap_or(0.0) as f32))
}

pub fn plugin(app: &mut App) {
    let path = assets_dir().join("hud/hud.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        warn!("no HUD data at {path:?}; run tools/build_assets.sh");
        return;
    };
    let comps: Value = serde_json::from_str(&text).expect("hud.json");
    app.insert_resource(HudData { comps: comps.into(), skin: "Navy" })
        .add_systems(PostStartup, load_fonts)
        .add_systems(Update, choose_skin.run_if(in_state(crate::menu::Screen::Battle)));
}

// ---- bitmap fonts (Fonts/*.fft) --------------------------------------------------------------------------------

/// A retail font (Fonts/*.fft): glyph sheets in `hud/tex/` with per-character rows and advances, drawn as the original does.
/// The vector face remains for plain Bevy `Text` (world labels, star map) and for characters a sheet lacks.
pub struct Font {
    pub face: Handle<bevy::text::Font>,
    metrics: std::sync::Arc<ab_glyph::FontVec>,
    pub size: f32,
    pub height: f32,
    glyphs: HashMap<char, Glyph>,
    sheets: Vec<Handle<Image>>,
}

/// One character on a glyph sheet: `row` (of `height` px) and `x`/`w` on sheet `tex`; `next` is the pen advance.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Glyph {
    tex: usize,
    row: f32,
    x: f32,
    w: f32,
    next: f32,
}

/// The retail glyph table: single-character keys plus `QUOTE` for '"'; `row < 0` marks a character the font does not draw.
fn glyph_table(v: &Value) -> HashMap<char, Glyph> {
    let num = |g: &Value, k: &str| g[k].as_f64().unwrap_or(0.0) as f32;
    v["chars"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, g)| {
            let c = if k == "QUOTE" { '"' } else { k.chars().next().filter(|_| k.chars().count() == 1)? };
            (num(g, "row") >= 0.0).then(|| (c, Glyph { tex: g["tex"].as_u64().unwrap_or(0) as usize, row: num(g, "row"), x: num(g, "x"), w: num(g, "w"), next: num(g, "next") }))
        })
        .collect()
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
        let sheets = v["textures"].as_array().into_iter().flatten().filter_map(|t| t.as_str()).map(|t| assets.load(format!("hud/tex/{}.png", t.to_lowercase()))).collect();
        fonts.0.insert(name, Font { face, metrics, size, height: v["height"].as_f64().unwrap_or(0.0) as f32, glyphs: glyph_table(&v), sheets });
    }
    commands.insert_resource(fonts);
}

impl Font {
    /// The sheet glyph for `c`; fonts without lower case (Univers26) draw its capital.
    fn glyph(&self, c: char) -> Option<&Glyph> {
        self.glyphs.get(&c).or_else(|| self.glyphs.get(&c.to_ascii_uppercase()))
    }
    /// Text width in reference px (retail advances; the vector face only for characters the sheet lacks).
    pub fn width(&self, text: &str) -> f32 {
        use ab_glyph::{Font as _, ScaleFont};
        let f = self.metrics.as_scaled(ab_glyph::PxScale::from(self.size));
        text.chars().map(|c| self.glyph(c).map_or_else(|| f.h_advance(f.glyph_id(c)), |g| g.next)).sum()
    }
    /// Draws `s` from the glyph sheets as one line node, `height` reference px tall and as wide as the text, at `scale` (reference
    /// to screen px), tinted with `color` (the sheets are white). Characters missing from the sheet are skipped.
    pub fn glyphs(&self, c: &mut ChildSpawnerCommands, s: &str, scale: f32, color: Color) -> Entity {
        let px = |v: f32| Val::Px(v * scale);
        let node = Node { width: px(self.width(s)), height: px(self.height), flex_shrink: 0.0, ..default() };
        c.spawn(node)
            .with_children(|c| {
                let mut pen = 0.0;
                for ch in s.chars() {
                    let Some(g) = self.glyph(ch).copied() else { continue };
                    if let Some(sheet) = self.sheets.get(g.tex) {
                        let mut img = ImageNode::new(sheet.clone()).with_color(color);
                        img.rect = Some(Rect::new(g.x, g.row * self.height, g.x + g.w, (g.row + 1.0) * self.height));
                        img.image_mode = NodeImageMode::Stretch;
                        c.spawn((img, Node { position_type: PositionType::Absolute, left: px(pen), top: px(0.0), width: px(g.w), height: px(self.height), ..default() }));
                    }
                    pen += g.next;
                }
            })
            .id()
    }
    /// Text components at `scale` (reference px to screen px).
    pub fn text(&self, s: &str, scale: f32, color: Color) -> impl Bundle {
        (Text::new(s), TextFont { font: self.face.clone().into(), font_size: FontSize::Px(self.size * scale), ..default() }, TextColor(color))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 10x8 sprite whose art starts 2 px from the left and 3 from the top and ends 1 px / 4 px short of the right / bottom.
    fn art(scale: f32) -> Sprite {
        Sprite { image: Handle::default(), size: Vec2::new(10.0, 8.0), trim: Trim { left: 2.0, top: 3.0, right: 1.0, bottom: 4.0 }, scale }
    }
    fn meta_of(art: &Sprite) -> Meta {
        Meta { size: art.size, trim: art.trim }
    }

    #[test]
    fn scale_comes_from_the_image_size() {
        let m = meta_of(&art(1.0)); // 7 x 1 reference px of art
        assert_eq!(scale_of(Some((7, 1)), &m), 1.0, "retail art");
        assert_eq!(scale_of(Some((14, 2)), &m), 2.0, "a 2x copy needs no scales.json entry");
        assert_eq!(scale_of(Some((28, 9)), &m), 4.0, "the width decides");
        assert_eq!(scale_of(None, &m), 1.0, "no file: retail resolution");
        let gone = Meta { size: Vec2::new(4.0, 4.0), trim: Trim { left: 0.0, top: 0.0, right: 4.0, bottom: 4.0 } };
        assert_eq!(scale_of(Some((1, 1)), &gone), 1.0, "a sprite with no art has nothing to measure");
    }

    #[test]
    fn png_size_reads_the_header() {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];
        bytes.extend(300u32.to_be_bytes());
        bytes.extend(40u32.to_be_bytes());
        let path = std::env::temp_dir().join(format!("openprocyon-png-size-{}.png", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(png_size(&path), Some((300, 40)));
        std::fs::write(&path, b"not a png at all, but long enough").unwrap();
        assert_eq!(png_size(&path), None);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(png_size(&path), None, "missing file");
    }

    #[test]
    fn a_missing_state_falls_back_to_the_idle_art() {
        let states = serde_json::json!({
            "NoCursor": {"size": [10, 8], "trim": [2, 3, 1, 4]},
            "CursorDown": {"size": [12, 9], "trim": [0, 0, 0, 0]},
        });
        let (used, meta) = pick_state(&states, "CursorDown").unwrap();
        assert_eq!((used, meta.size), ("CursorDown", Vec2::new(12.0, 9.0)));
        let (used, meta) = pick_state(&states, "CursorOver").unwrap();
        assert_eq!((used, meta.size, meta.trim.left), (IDLE, Vec2::new(10.0, 8.0), 2.0), "no CursorOver entry: the NoCursor file and size");
        assert!(pick_state(&serde_json::json!({}), "CursorOver").is_none());
    }

    #[test]
    fn trim_offsets_place_the_art_where_the_untrimmed_sprite_was() {
        let a = art(1.0);
        let v = a.own();
        assert_eq!((v.offset, v.size, v.to), (Vec2::new(2.0, 3.0), Vec2::new(7.0, 1.0), Vec2::new(10.0, 8.0)));
        assert_eq!(v.rect, None, "the whole image shows");
        // At 2 px per reference px the node is the art, the margins carry the transparent border: the outer box is the whole sprite.
        let (w, h, m) = v.geometry(2.0, UiRect::DEFAULT);
        assert_eq!((w, h), (Val::Px(14.0), Val::Px(2.0)));
        assert_eq!((m.left, m.right, m.top, m.bottom), (Val::Px(4.0), Val::Px(2.0), Val::Px(6.0), Val::Px(8.0)));
        // Stretched over a box twice as big, the border stretches with it.
        let (w, _, m) = a.fit(Vec2::new(20.0, 16.0)).geometry(1.0, UiRect::DEFAULT);
        assert_eq!((w, m.left, m.right), (Val::Px(14.0), Val::Px(4.0), Val::Px(2.0)));
        // A layout margin is kept on top of the trim.
        let (_, _, m) = v.geometry(1.0, UiRect::left(Val::Px(-0.5)));
        assert_eq!(m.left, Val::Px(1.5));
        // Mirrored with the image the border swaps sides.
        let flipped = v.flipped(true, false);
        assert_eq!(flipped.offset, Vec2::new(1.0, 3.0));
    }

    #[test]
    fn a_crop_of_the_box_shows_the_art_inside_it() {
        let a = art(2.0); // image pixels are twice the reference pixels
        // The left 5 reference px of the box hold 3 px of art (columns 2..5), stretched over a 10 px wide box.
        let v = a.view(Rect::new(0.0, 0.0, 5.0, 8.0), Vec2::new(10.0, 8.0));
        assert_eq!(v.rect, Some(Rect::new(0.0, 0.0, 6.0, 2.0)));
        assert_eq!((v.offset.x, v.size.x), (4.0, 6.0));
        // A crop that misses the art shows nothing.
        let v = a.view(Rect::new(0.0, 0.0, 1.0, 8.0), Vec2::new(1.0, 8.0));
        assert_eq!(v.size.x, 0.0);
    }

    #[test]
    fn a_gauge_shows_the_bottom_rows_of_its_box() {
        let a = art(1.0);
        // Half full: rows 4..8 of the box; the art ends at row 4, so only the rows from 4 down that are inside it show (none).
        let v = a.view(Rect::new(0.0, 4.0, 10.0, 8.0), Vec2::new(10.0, 4.0));
        assert_eq!(v.size.y, 0.0);
        // Rows 2..8: the art's row 3 is the only one in the box.
        let v = a.view(Rect::new(0.0, 2.0, 10.0, 8.0), Vec2::new(10.0, 6.0));
        assert_eq!((v.offset.y, v.size.y), (1.0, 1.0));
        assert_eq!(v.rect, None, "the whole image is inside those rows");
    }
    #[test]
    #[ignore = "needs imported assets"]
    fn retail_glyph_table_and_advances() {
        let v = serde_json::json!({"chars": {
            "A": {"row": 1, "x": 54, "w": 23, "tex": 0, "next": 19},
            "QUOTE": {"row": 0, "x": 11, "w": 8, "tex": 0, "next": 8},
            "a": {"row": -1, "x": 0, "w": 0, "tex": 0, "next": 0}
        }});
        let t = glyph_table(&v);
        assert_eq!(t[&'A'], Glyph { tex: 0, row: 1.0, x: 54.0, w: 23.0, next: 19.0 });
        assert!(t.contains_key(&'"'), "QUOTE is the double quote");
        assert!(!t.contains_key(&'a'), "row -1: the font does not draw it");
        // Measured on an original screenshot: the Controls header "Control" is 80 units wide in UniversBold14 (advance 77).
        let f: Value = crate::data::read_json(&assets_dir().join("hud/fonts/UniversBold14.json"));
        let t = glyph_table(&f);
        assert_eq!("Control".chars().map(|c| t[&c].next).sum::<f32>(), 77.0);
    }
}
