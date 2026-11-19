//! Ship art made from a ship's model, for ships that bring none (docs/modding.md "A ship from a model"): the side silhouette the
//! command bar and fleet cells show (`ShipBar_ShipIcon_<art>`) and the top-down outline of the weapon panel
//! (`WeaponBar_ShipIcon_<art>`), plus the panel slot each gun bank takes so its icon lands over its guns.
//!
//! The look copies the retail art: the silhouette is a solid pale-cyan fill about 40x35 px, the outline a soft 1.5 px pale-blue line,
//! bow up, in a 126x230 box (hull length fitted to 227 px). Colours, sizes and offsets are measured from the retail sprites; the
//! shapes are the model's own, smoothed so they sit with the hand-drawn ones. PROVISIONAL until compared by eye in the game.
//! Pure functions plus `generate`, which writes the sprite files into the overlay; no UI code.
use crate::data::derive::Layer;
use std::path::Path;

/// Supersampling of the masks before they are boxed down to reference pixels (gives the anti-aliased edge).
const S: usize = 4;
/// The weapon-bar outline's reference box, where the art may sit in it (retail: top 2, 227 tall, <= ~104 wide) and the box's offset in the panel.
pub const OUTLINE_BOX: [f64; 2] = [126.0, 230.0];
const OUTLINE_TOP: f64 = 2.0;
const OUTLINE_HEIGHT: f64 = 227.0;
const OUTLINE_WIDTH: f64 = 110.0;
pub const PANEL_ORIGIN: [f64; 2] = [13.0, 12.0];
/// The command-bar icon's reference box and the art's size and trim in it.
const ICON_BOX: [f64; 2] = [48.0, 38.0];
const ICON_ART: [usize; 2] = [40, 35];
const ICON_TRIM: [f64; 4] = [3.0, 0.0, 5.0, 3.0];
/// Share of the athwartships coordinate added to the icon's horizontal axis.
const SHEAR: f64 = 0.15;
const FILL: [u8; 3] = [200, 255, 255];
const LINE: [u8; 3] = [195, 222, 254];

fn is_sail(name: &str) -> bool {
    let l = name.to_lowercase();
    ["sail", "mast", "cloth", "canvas"].iter().any(|w| l.contains(w))
}

/// The intact layers: the damaged variants of a section are not part of the ship's shape.
fn intact(layers: &[Layer]) -> impl Iterator<Item = &Layer> {
    layers.iter().filter(|l| !l.name.contains("Damage_"))
}

/// How the hull lands in the outline's reference box: a uniform scale that fits it, centred on the box, bow up.
pub struct TopDown {
    k: f64,
    cx: f64,
    ymax: f64,
    top: f64,
}
impl TopDown {
    /// From the hull layers (not the sails); `None` for a model with no triangles.
    pub fn of(layers: &[Layer]) -> Option<TopDown> {
        let hull: Vec<&Layer> = intact(layers).filter(|l| !is_sail(&l.name)).collect();
        let hull = if hull.is_empty() { intact(layers).collect() } else { hull };
        // Extents by area, not by the farthest vertex: a yard or a spar sticking out of a hull layer must not shrink the hull.
        let (x, y) = (area_extent(&hull, 0), area_extent(&hull, 1));
        let ((x0, x1), (y0, y1)) = (x?, y?);
        let (w, h) = (x1 - x0, y1 - y0);
        (h > 0.0).then(|| {
            let k = (OUTLINE_HEIGHT / h).min(OUTLINE_WIDTH / w.max(f64::EPSILON));
            TopDown { k, cx: (x0 + x1) / 2.0, ymax: y1, top: OUTLINE_TOP + (OUTLINE_HEIGHT - h * k) / 2.0 }
        })
    }
    /// A game-axes point's position in the outline's reference box.
    pub fn point(&self, x: f64, y: f64) -> [f64; 2] {
        [OUTLINE_BOX[0] / 2.0 + (x - self.cx) * self.k, self.top + (self.ymax - y) * self.k]
    }
    /// A game-axes point's position in the weapon panel.
    pub fn panel(&self, x: f64, y: f64) -> [f64; 2] {
        let p = self.point(x, y);
        [PANEL_ORIGIN[0] + p[0], PANEL_ORIGIN[1] + p[1]]
    }
}

/// The span of `axis` (0 = x, 1 = y) that holds all but the outermost 0.5% of the layers' corner-weighted triangle area at each end.
fn area_extent(layers: &[&Layer], axis: usize) -> Option<(f64, f64)> {
    let mut parts: Vec<(f64, f64)> = vec![];
    for t in layers.iter().flat_map(|l| l.tris.as_chunks::<9>().0) {
        let (a, b, c) = ([t[0], t[1]], [t[3], t[4]], [t[6], t[7]]);
        let area = ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0;
        // Each corner carries a third of the area, so a coarse mesh keeps its true edges.
        parts.extend([a, b, c].map(|v| (v[axis], area / 3.0)));
    }
    let total: f64 = parts.iter().map(|p| p.1).sum();
    if total.is_nan() || total <= 0.0 {
        return None;
    }
    parts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut run, mut lo, mut hi) = (0.0, None, None);
    for (c, area) in &parts {
        run += area;
        if lo.is_none() && run >= total * 0.005 {
            lo = Some(*c);
        }
        if hi.is_none() && run >= total * 0.995 {
            hi = Some(*c);
        }
    }
    lo.zip(hi)
}

/// Centres of the weapon panel's 80 gun-bank slots (retail's lattice: `WeaponBar_SlotOffset_From_TopLeft_NN` plus the gun icon's offset
/// and half its 31 px size); empty when `hud/hud.json` is missing.
pub fn slot_centres(root: &Path) -> Vec<[f64; 2]> {
    let Some(hud) = crate::data::try_json(&root.join("hud/hud.json")) else { return vec![] };
    let at = |k: &str| [hud[k]["Navy"]["TopLeft X"].as_f64(), hud[k]["Navy"]["TopLeft Y"].as_f64()];
    let icon = at("Offset_WeaponBar_GunIcon");
    (0..)
        .map_while(|i| match at(&format!("WeaponBar_SlotOffset_From_TopLeft_{i:02}")) {
            [Some(x), Some(y)] => Some([x + icon[0].unwrap_or(0.0) + 15.5, y + icon[1].unwrap_or(0.0) + 15.5]),
            _ => None,
        })
        .collect()
}

/// A boolean image at `S` times the reference resolution.
struct Mask {
    w: usize,
    h: usize,
    on: Vec<bool>,
}
impl Mask {
    fn new(w: usize, h: usize) -> Mask {
        Mask { w: w * S, h: h * S, on: vec![false; w * S * h * S] }
    }
    fn set(&mut self, x: i64, y: i64) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.on[y as usize * self.w + x as usize] = true;
        }
    }
    fn get(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h && self.on[y as usize * self.w + x as usize]
    }
    /// A triangle given in reference pixels: filled, and its edges drawn so that an edge-on one (a flat sail seen from its side) still shows.
    fn triangle(&mut self, t: [[f64; 2]; 3]) {
        let p: Vec<[f64; 2]> = t.iter().map(|v| [v[0] * S as f64, v[1] * S as f64]).collect();
        for i in 0..3 {
            let (a, b) = (p[i], p[(i + 1) % 3]);
            let n = ((b[0] - a[0]).abs().max((b[1] - a[1]).abs()) * 2.0).ceil().max(1.0) as usize;
            for s in 0..=n {
                let f = s as f64 / n as f64;
                self.set((a[0] + (b[0] - a[0]) * f).floor() as i64, (a[1] + (b[1] - a[1]) * f).floor() as i64);
            }
        }
        let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
        if area.abs() < 1e-9 {
            return;
        }
        let (y0, y1) =
            (p.iter().map(|v| v[1]).fold(f64::MAX, f64::min).floor().max(0.0) as i64, p.iter().map(|v| v[1]).fold(f64::MIN, f64::max).ceil().min(self.h as f64 - 1.0) as i64);
        for y in y0..=y1 {
            let yc = y as f64 + 0.5;
            let mut xs = vec![];
            for i in 0..3 {
                let (a, b) = (p[i], p[(i + 1) % 3]);
                if (a[1] <= yc) != (b[1] <= yc) {
                    xs.push(a[0] + (yc - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
                }
            }
            if xs.len() >= 2 {
                let (a, b) = (xs.iter().copied().fold(f64::MAX, f64::min), xs.iter().copied().fold(f64::MIN, f64::max));
                for x in (a - 0.5).ceil().max(0.0) as i64..=(b - 0.5).floor().min(self.w as f64 - 1.0) as i64 {
                    self.set(x, y);
                }
            }
        }
    }
    /// Pixels within `r` (a square) of an "on" pixel are on.
    fn dilate(&self, r: i64) -> Mask {
        // Two passes (rows, then columns) of a square window.
        let pass = |src: &Mask, horizontal: bool| {
            let mut out = Mask { w: src.w, h: src.h, on: vec![false; src.on.len()] };
            for y in 0..src.h as i64 {
                for x in 0..src.w as i64 {
                    out.on[y as usize * src.w + x as usize] = (-r..=r).any(|d| if horizontal { src.get(x + d, y) } else { src.get(x, y + d) });
                }
            }
            out
        };
        pass(&pass(self, true), false)
    }
    fn erode(&self, r: i64) -> Mask {
        let mut inv = Mask { w: self.w, h: self.h, on: self.on.iter().map(|b| !b).collect() };
        // Outside the image counts as empty, so the border erodes too.
        inv = inv.dilate(r);
        Mask { w: self.w, h: self.h, on: inv.on.iter().map(|b| !b).collect() }.border_off(r)
    }
    /// Clears the `r` pixels at the image edge (erosion treats the outside as empty).
    fn border_off(mut self, r: i64) -> Mask {
        for y in 0..self.h as i64 {
            for x in 0..self.w as i64 {
                if x < r || y < r || x >= self.w as i64 - r || y >= self.h as i64 - r {
                    self.on[y as usize * self.w + x as usize] = false;
                }
            }
        }
        self
    }
    /// Everything not reachable from the image border without crossing an "on" pixel becomes "on" (closes holes between triangles).
    fn fill_holes(&self) -> Mask {
        let mut outside = vec![false; self.on.len()];
        let mut stack: Vec<(i64, i64)> = vec![];
        for x in 0..self.w as i64 {
            stack.extend([(x, 0), (x, self.h as i64 - 1)]);
        }
        for y in 0..self.h as i64 {
            stack.extend([(0, y), (self.w as i64 - 1, y)]);
        }
        while let Some((x, y)) = stack.pop() {
            if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
                continue;
            }
            let i = y as usize * self.w + x as usize;
            if outside[i] || self.on[i] {
                continue;
            }
            outside[i] = true;
            stack.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
        }
        Mask { w: self.w, h: self.h, on: outside.iter().map(|o| !o).collect() }
    }
    /// Rounds corners: a box blur, kept where more than half is "on".
    fn smooth(&self, r: i64) -> Mask {
        let mut sum = vec![0i64; (self.w + 1) * (self.h + 1)];
        for y in 0..self.h {
            for x in 0..self.w {
                sum[(y + 1) * (self.w + 1) + x + 1] = self.on[y * self.w + x] as i64 + sum[y * (self.w + 1) + x + 1] + sum[(y + 1) * (self.w + 1) + x] - sum[y * (self.w + 1) + x];
            }
        }
        let mut out = Mask { w: self.w, h: self.h, on: vec![false; self.on.len()] };
        for y in 0..self.h as i64 {
            for x in 0..self.w as i64 {
                let (x0, y0, x1, y1) = ((x - r).max(0) as usize, (y - r).max(0) as usize, ((x + r + 1) as usize).min(self.w), ((y + r + 1) as usize).min(self.h));
                let n = ((x1 - x0) * (y1 - y0)) as i64;
                let s = sum[y1 * (self.w + 1) + x1] - sum[y0 * (self.w + 1) + x1] - sum[y1 * (self.w + 1) + x0] + sum[y0 * (self.w + 1) + x0];
                out.on[y as usize * self.w + x as usize] = s * 2 > n;
            }
        }
        out
    }
    /// Coverage (0..=255) of each reference pixel.
    fn coverage(&self) -> Vec<u8> {
        let (w, h) = (self.w / S, self.h / S);
        (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let n = (0..S * S).filter(|j| self.on[(y * S + j / S) * self.w + x * S + j % S]).count();
                (n * 255 / (S * S)) as u8
            })
            .collect()
    }
}

/// RGBA of a coverage plane in one colour.
fn tint(coverage: &[u8], colour: [u8; 3], gain: f32) -> Vec<u8> {
    coverage.iter().flat_map(|&a| [colour[0], colour[1], colour[2], (a as f32 * gain).min(255.0) as u8]).collect()
}

/// The command-bar silhouette: every intact layer seen from the side, bow to the right, as a solid fill. RGBA, `ICON_ART` pixels.
pub fn side_icon(layers: &[Layer]) -> Option<Vec<u8>> {
    let tris = || intact(layers).flat_map(|l| l.tris.as_chunks::<9>().0);
    // Slightly from the side (a shear of the athwartships axis into the length) so square sails, which face fore and aft, show their
    // width the way the retail icons' sails do instead of vanishing edge-on.
    let screen = |v: &[f64]| [v[1] + SHEAR * v[0], v[2]];
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for t in tris() {
        for v in t.as_chunks::<3>().0 {
            let c = screen(v);
            for k in 0..2 {
                lo[k] = lo[k].min(c[k]);
                hi[k] = hi[k].max(c[k]);
            }
        }
    }
    let (l, h) = (hi[0] - lo[0], hi[1] - lo[1]);
    if !(l > 0.0 && h > 0.0) {
        return None;
    }
    let k = ((ICON_ART[0] - 2) as f64 / l).min((ICON_ART[1] - 2) as f64 / h);
    // Centred along the length, standing on the bottom edge.
    let (x0, y1) = ((ICON_ART[0] as f64 - l * k) / 2.0, ICON_ART[1] as f64 - 1.0);
    let mut m = Mask::new(ICON_ART[0], ICON_ART[1]);
    for t in tris() {
        let p = |i: usize| {
            let c = screen(&t[i * 3..i * 3 + 3]);
            [x0 + (c[0] - lo[0]) * k, y1 - (c[1] - lo[1]) * k]
        };
        m.triangle([p(0), p(1), p(2)]);
    }
    // Thickened a little and not eroded back: masts and rails are thin, and the retail icon is bold.
    let m = m.dilate(1).smooth(1);
    Some(tint(&m.coverage(), FILL, 1.0))
}

/// The weapon panel's outline of the hull seen from above: a 1.5 px line, bow up. RGBA of the whole `OUTLINE_BOX`.
pub fn outline(layers: &[Layer]) -> Option<Vec<u8>> {
    let td = TopDown::of(layers)?;
    let hull = |l: &&Layer| !is_sail(&l.name);
    let any_hull = intact(layers).any(|l| hull(&l));
    let mut m = Mask::new(OUTLINE_BOX[0] as usize, OUTLINE_BOX[1] as usize);
    for l in intact(layers).filter(|l| !any_hull || hull(l)) {
        for t in l.tris.as_chunks::<9>().0 {
            let p = |i: usize| {
                let v = td.point(t[i * 3], t[i * 3 + 1]);
                [v[0], v[1]]
            };
            m.triangle([p(0), p(1), p(2)]);
        }
    }
    let closed = m.dilate(3).fill_holes().erode(3);
    // Opened again so that spars and rails thinner than three pixels do not become spurs of the outline.
    let solid = closed.erode(6).dilate(6).smooth(5);
    let edge = Mask { w: solid.w, h: solid.h, on: solid.on.iter().zip(&solid.erode(3).on).map(|(a, b)| *a && !*b).collect() };
    Some(tint(&edge.coverage(), LINE, 1.6))
}

/// Bounding box (left, top, right, bottom; right and bottom exclusive) of the pixels with any alpha.
fn alpha_box(rgba: &[u8], w: usize) -> Option<[usize; 4]> {
    let mut b = [usize::MAX, usize::MAX, 0, 0];
    for (i, p) in rgba.as_chunks::<4>().0.iter().enumerate().filter(|(_, p)| p[3] > 0) {
        let _ = p;
        let (x, y) = (i % w, i / w);
        b = [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)];
    }
    (b[0] != usize::MAX).then_some(b)
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// A PNG (8-bit RGBA) with a stored, uncompressed deflate stream: the sprites are tiny and this needs no dependency.
pub fn png(w: usize, h: usize, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((w * 4 + 1) * h);
    for row in rgba.chunks_exact(w * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push((i + 1 == blocks.len()) as u8);
        z.extend((b.len() as u16).to_le_bytes());
        z.extend((!(b.len() as u16)).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut s) = (1u32, 0u32);
    for &b in &raw {
        a = (a + b as u32) % 65521;
        s = (s + a) % 65521;
    }
    z.extend(((s << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend(&body);
        out.extend(crc32(&body).to_be_bytes());
    };
    let mut ihdr = (w as u32).to_be_bytes().to_vec();
    ihdr.extend((h as u32).to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

/// Writes one sprite (`hud/sprites/<name>/<skin>.json` and `<skin>/NoCursor.png`, the same art for every skin as in retail).
fn write_sprite(root: &Path, name: &str, size: [f64; 2], trim: [f64; 4], w: usize, h: usize, rgba: &[u8]) {
    let dir = root.join("hud/sprites").join(name);
    let sidecar = serde_json::json!({"states": {"NoCursor": {"size": size, "trim": trim}}});
    for skin in ["Navy", "Pirate", "Procyon"] {
        let _ = std::fs::create_dir_all(dir.join(skin));
        let _ = std::fs::write(dir.join(skin).join("NoCursor.png"), png(w, h, rgba));
        let _ = std::fs::write(dir.join(format!("{skin}.json")), sidecar.to_string());
    }
}

/// The art name a ship's sprites carry (`Class::icon`): its `icon` without the bar prefix, else its id without `Ship_`.
fn art_name(ship: &serde_json::Value, id: &str) -> String {
    ship["icon"].as_str().and_then(|i| i.strip_prefix("ShipBar_ShipIcon_")).unwrap_or_else(|| id.trim_start_matches("Ship_")).to_string()
}

/// For every ship of `ids` (a mod's) in `root` whose command-bar icon or weapon-panel outline has no art, makes it from the model. Called once when the
/// overlay is built, so every loader sees the sprites as ordinary files; a ship that has art of its own is left alone.
pub fn generate(root: &Path, ids: &[String]) {
    for id in ids {
        let Some(ship) = crate::data::try_json(&root.join(format!("ships/{id}/ship.json"))) else { continue };
        let art = art_name(&ship, id);
        let (icon, panel) = (format!("ShipBar_ShipIcon_{art}"), format!("WeaponBar_ShipIcon_{art}"));
        let sprites = root.join("hud/sprites");
        if sprites.join(&icon).is_dir() && sprites.join(&panel).is_dir() {
            continue;
        }
        let model = ship["model"].as_str().unwrap_or(id);
        let Some(facts) = std::fs::read(root.join(format!("ships/{model}/model.glb"))).ok().and_then(|b| crate::data::derive::read(&b)) else { continue };
        if !sprites.join(&icon).is_dir()
            && let Some(rgba) = side_icon(&facts.layers)
        {
            write_sprite(root, &icon, ICON_BOX, ICON_TRIM, ICON_ART[0], ICON_ART[1], &rgba);
        }
        if !sprites.join(&panel).is_dir()
            && let Some(rgba) = outline(&facts.layers)
        {
            let (w, h) = (OUTLINE_BOX[0] as usize, OUTLINE_BOX[1] as usize);
            if let Some([l, t, r, b]) = alpha_box(&rgba, w) {
                let crop: Vec<u8> = (t..b).flat_map(|y| rgba[(y * w + l) * 4..(y * w + r) * 4].iter().copied()).collect();
                write_sprite(root, &panel, OUTLINE_BOX, [l as f64, t as f64, (w - r) as f64, (h - b) as f64], r - l, b - t, &crop);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// A layer of one long flat quad (two triangles) in game axes.
    fn plank(name: &str, half_w: f64, len: f64) -> Layer {
        let (a, b, c, d) = ([-half_w, -len / 2.0, 0.0], [half_w, -len / 2.0, 0.0], [half_w, len / 2.0, 0.0], [-half_w, len / 2.0, 0.0]);
        let tris = [a, b, c, a, c, d].concat();
        Layer { name: name.into(), aabb: [[-half_w, -len / 2.0, 0.0], [half_w, len / 2.0, 0.0]], tris }
    }

    #[test]
    fn outline_fits_the_panel_box_and_is_a_line_not_a_fill() {
        let layers = [plank("Hull", 2.0, 20.0)];
        let rgba = outline(&layers).unwrap();
        let [l, t, r, b] = alpha_box(&rgba, 126).unwrap();
        assert!(t >= 2 && b <= 230 && b - t >= 220, "hull length fills the box height: {t}..{b}");
        assert!(l > 30 && r < 96, "a narrow hull stays centred: {l}..{r}");
        let mid = 115usize;
        let opaque = (0..126).filter(|&x| rgba[(mid * 126 + x) * 4 + 3] > 40).count();
        assert!((2..=8).contains(&opaque), "two edge strokes at mid hull, not a fill: {opaque}");
    }

    #[test]
    fn the_side_icon_is_a_solid_fill_standing_on_the_bottom_and_ignores_damage_meshes() {
        // Seen from the side the quad (flat in z) is edge-on: add a mast layer so there is height.
        let mut mast = plank("Mast", 0.1, 4.0);
        mast.tris = [[0.0, -0.1, 0.0], [0.0, 0.1, 0.0], [0.0, 0.1, 8.0], [0.0, -0.1, 0.0], [0.0, 0.1, 8.0], [0.0, -0.1, 8.0]].concat();
        let layers = [plank("Hull", 2.0, 20.0), mast, plank("HullDamage_Layer1", 9.0, 90.0)];
        let rgba = side_icon(&layers).unwrap();
        assert_eq!(rgba.len(), 40 * 35 * 4);
        assert!(rgba[(34 * 40 + 20) * 4 + 3] > 0, "hull line at the bottom edge");
        let lit = rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).count();
        assert!(lit > 40 && lit < 400, "a thin hull and a mast, not the damage mesh's 90 units: {lit}");
    }

    #[test]
    fn panel_points_follow_the_outline() {
        let layers = [plank("Hull", 2.0, 20.0)];
        let td = TopDown::of(&layers).unwrap();
        let bow = td.panel(0.0, 10.0);
        let stern = td.panel(0.0, -10.0);
        assert!(bow[1] < stern[1] && (bow[0] - stern[0]).abs() < 1e-9, "bow is up, on the centreline");
        assert!((bow[0] - (PANEL_ORIGIN[0] + 63.0)).abs() < 1e-9);
    }

    /// Writes the generated art of some retail ships to `local/shipart/` to be judged beside the retail sprites (tools/compare_shipart.py).
    #[test]
    #[ignore = "needs imported assets; writes local/shipart"]
    fn writes_generated_art_of_retail_ships_for_comparison() {
        let root = crate::assets_dir();
        let out = root.join("../shipart");
        std::fs::create_dir_all(&out).unwrap();
        for id in ["Ship_Navy_Cutter", "Ship_Pirate_FastAttack", "Ship_Pirate_Galleon", "Ship_Procyon_Cutter", "Ship_Navy_ManOWar", "Ship_Pirate_Sloop"] {
            let mut f = crate::data::derive::read(&std::fs::read(root.join(format!("ships/{id}/model.glb"))).unwrap()).unwrap();
            // Retail layers are called LayerN: tell the sails by the sections' materials, as a mod's names would.
            let ship = crate::data::try_json(&root.join(format!("ships/{id}/ship.json"))).unwrap();
            for l in &mut f.layers {
                if ship["sections"].as_object().unwrap().values().any(|s| s["layer"] == l.name.as_str() && s["material"].as_str().unwrap_or("").contains("Cloth")) {
                    l.name.push_str("_sail");
                }
            }
            let art = id.trim_start_matches("Ship_");
            std::fs::write(out.join(format!("{art}_side.png")), png(40, 35, &side_icon(&f.layers).unwrap())).unwrap();
            std::fs::write(out.join(format!("{art}_top.png")), png(126, 230, &outline(&f.layers).unwrap())).unwrap();
        }
    }

    #[test]
    fn generate_writes_sprites_for_a_mod_ship_and_leaves_ships_with_art_alone() {
        let root = std::env::temp_dir().join(format!("procyon-shipart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for id in ["Mod_Ship", "Has_Art"] {
            std::fs::create_dir_all(root.join(format!("ships/{id}"))).unwrap();
            std::fs::write(root.join(format!("ships/{id}/model.glb")), crate::data::derive::tests::model()).unwrap();
            std::fs::write(root.join(format!("ships/{id}/ship.json")), "{}").unwrap();
        }
        std::fs::create_dir_all(root.join("hud/sprites/ShipBar_ShipIcon_Has_Art")).unwrap();
        std::fs::create_dir_all(root.join("hud/sprites/WeaponBar_ShipIcon_Has_Art")).unwrap();
        generate(&root, &["Mod_Ship".into(), "Has_Art".into()]);
        for bar in ["ShipBar", "WeaponBar"] {
            for skin in ["Navy", "Pirate", "Procyon"] {
                let dir = root.join(format!("hud/sprites/{bar}_ShipIcon_Mod_Ship"));
                assert!(dir.join(format!("{skin}/NoCursor.png")).is_file() && dir.join(format!("{skin}.json")).is_file(), "{bar} {skin}");
            }
            assert!(!root.join(format!("hud/sprites/{bar}_ShipIcon_Has_Art/Navy.json")).exists(), "{bar}: a ship with art keeps it");
        }
        let side: Value = crate::data::try_json(&root.join("hud/sprites/ShipBar_ShipIcon_Mod_Ship/Navy.json")).unwrap();
        assert_eq!(side["states"]["NoCursor"]["size"], serde_json::json!([48.0, 38.0]));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn png_is_a_valid_8_bit_rgba_file() {
        let bytes = png(2, 1, &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(&bytes[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        assert_eq!(&bytes[bytes.len() - 4..], &0xae42_6082u32.to_be_bytes());
    }
}
