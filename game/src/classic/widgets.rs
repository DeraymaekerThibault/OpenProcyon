//! The classic front end's widget kit: retail hud.hdt FrontEnd_* art (Navy set, `MENU_SKIN`) laid out on the 800x600 reference
//! canvas and scaled to the window. Every retail screen (`menu.rs`, `crew_arms.rs`) builds from these; behaviour stays in the shared
//! markers each widget carries (`menu::Action`, `crew_arms::Action`, `ScrollTrack`, ...). `hover` swaps the hover/press art.

#![allow(deprecated)] // shortcut: bevy 0.20 Interaction (deprecated for picking Hovered/Pressed); port when it is removed
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};

use crate::menu::{ArrowButton, ListEntry, RunButtons, SLIDER_KNOB, SLOT_BAR, ScrollKind, ScrollTrack, SliderTrack, menu_active};
use crate::sprites::{BODY_FONT, Fonts, HudData, IDLE, MENU_SKIN, inset, sliced};

/// Retail gold for highlighted labels (the `Center Justify Label` title colour).
pub(super) const GOLD: Color = Color::srgb(1.0, 0.796, 0.0);
/// Teal bar behind the picked row of a list.
pub(super) const PICKED: Color = Color::srgba(0.2, 0.7, 0.8, 0.45);
/// Lighter heading band at the top of a framed table.
pub(super) const HEADING: Color = Color::srgba(0.25, 0.75, 0.85, 0.22);
/// Behind an open pull-down's entries: the pull-down art is translucent, so the list needs a solid backing to read over what it covers.
pub(super) const LIST_BACKING: Color = Color::srgb(0.03, 0.12, 0.3);
/// Resting / hover fill of a list cell (the retail SortList slot art carries decoration that only fits full-width rows).
const CELL: Color = Color::srgba(0.0, 0.05, 0.2, 0.7);
const CELL_ON: Color = Color::srgba(0.35, 0.28, 0.05, 0.75);
const CELL_HOVER: Color = Color::srgba(0.1, 0.25, 0.55, 0.85);

/// One swappable part of a widget's art.
enum Piece {
    Sprite(String),
    /// The left or right `w` px of a sprite (all of its states share the crop): the slider's and slot bar's end caps.
    Crop {
        name: String,
        left: bool,
        w: f32,
    },
    /// A `sprites::sliced` strip of this group.
    Slice(&'static str),
}

/// Hover/press art: a widget's pieces, swapped between NoCursor / CursorOver / CursorDown by `hover`.
#[derive(Component)]
struct Art(Vec<(Entity, Piece)>);

/// The fill a cell returns to when the cursor leaves it.
#[derive(Component)]
pub(super) struct Rest(pub(super) Color);

pub fn plugin(app: &mut App) {
    app.add_systems(Update, hover.after(RunButtons).run_if(menu_active));
}

/// Hover/press art of the retail buttons and list cells.
fn hover(
    q: Query<(Entity, &Interaction, Option<&Art>, Option<&Rest>), Changed<Interaction>>,
    mut fills: Query<&mut BackgroundColor>,
    hud: Option<Res<HudData>>,
    assets: Res<AssetServer>,
    mut imgs: Query<&mut ImageNode>,
) {
    let Some(hud) = hud else { return };
    for (e, i, art, rest) in &q {
        if let (Some(rest), Ok(mut bg)) = (rest, fills.get_mut(e)) {
            bg.0 = if *i == Interaction::None { rest.0 } else { CELL_HOVER };
        }
        let state = match i {
            Interaction::Pressed => "CursorDown",
            Interaction::Hovered => "CursorOver",
            Interaction::None => IDLE,
        };
        for (e, piece) in art.map(|a| a.0.as_slice()).unwrap_or(&[]) {
            let Ok(mut img) = imgs.get_mut(*e) else { continue };
            let (name, cropped) = match piece {
                Piece::Slice(group) => {
                    img.image = assets.load(format!("hud/tex/slice_{group}_{}_{}.png", MENU_SKIN.to_lowercase(), state.to_lowercase()));
                    continue;
                }
                Piece::Sprite(name) => (name, None),
                Piece::Crop { name, left, w } => (name, Some((*left, *w))),
            };
            if let Some((sheet, r)) = sprite(&hud, name, state) {
                img.image = assets.load(format!("hud/tex/{sheet}.png"));
                img.rect = Some(inset(&sheet, cropped.map_or(r, |(left, w)| crop(r, left, w))));
            }
        }
    }
}

/// Sprite rect of a hud.hdt component for a cursor state (`IDLE` = "NoCursor", "CursorOver", "CursorDown").
pub(super) fn sprite(hud: &HudData, name: &str, state: &str) -> Option<(String, Rect)> {
    let d = hud.comps.get(name)?.get(MENU_SKIN)?;
    let r = d[format!("TextureArea - {state}")].as_array()?;
    let n = |i: usize| r[i].as_f64().unwrap_or(0.0) as f32;
    Some((d[format!("TextureName - {state}")].as_str()?.to_lowercase(), Rect::new(n(0), n(1), n(2) + 1.0, n(3) + 1.0)))
}

fn crop(r: Rect, left: bool, w: f32) -> Rect {
    if left { Rect::new(r.min.x, r.min.y, r.min.x + w, r.max.y) } else { Rect::new(r.max.x - w, r.min.y, r.max.x, r.max.y) }
}

/// What every widget needs: art, fonts and the reference-to-window scale.
pub(super) struct Ctx<'a> {
    pub(super) assets: &'a AssetServer,
    pub(super) hud: &'a HudData,
    pub(super) fonts: &'a Fonts,
    pub(super) scale: f32,
}

/// The 800x600 reference canvas centred in the window, and its scale.
pub(super) fn canvas(window: &Window) -> (Node, f32) {
    let (ww, wh) = (window.width(), window.height());
    let scale = (ww / 800.0).min(wh / 600.0);
    let node = Node {
        position_type: PositionType::Absolute,
        left: Val::Px((ww - 800.0 * scale) / 2.0),
        top: Val::Px((wh - 600.0 * scale) / 2.0),
        width: Val::Px(800.0 * scale),
        height: Val::Px(600.0 * scale),
        ..default()
    };
    (node, scale)
}

/// A node covering the whole window (the screen root under the canvas).
pub(super) fn full_screen() -> Node {
    Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }
}

impl Ctx<'_> {
    pub(super) fn node(&self, x: f32, y: f32, w: f32, h: f32) -> Node {
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(x * self.scale),
            top: Val::Px(y * self.scale),
            width: Val::Px(w * self.scale),
            height: Val::Px(h * self.scale),
            ..default()
        }
    }
    pub(super) fn image(&self, c: &mut ChildSpawnerCommands, path: &str, rect: Option<Rect>, x: f32, y: f32, w: f32, h: f32, tint: Color) -> Entity {
        let mut img = ImageNode::new(self.assets.load(path.to_string()));
        img.rect = rect;
        img.color = tint;
        img.image_mode = NodeImageMode::Stretch;
        c.spawn((img, self.node(x, y, w, h))).id()
    }
    /// A full 800x600 backdrop picture.
    pub(super) fn backdrop(&self, c: &mut ChildSpawnerCommands, path: &str) {
        self.image(c, path, Some(Rect::new(0.0, 0.0, 800.0, 600.0)), 0.0, 0.0, 800.0, 600.0, Color::WHITE);
    }
    /// A flat colour box.
    pub(super) fn fill(&self, c: &mut ChildSpawnerCommands, [x, y, w, h]: [f32; 4], colour: Color) {
        c.spawn((self.node(x, y, w, h), BackgroundColor(colour)));
    }
    /// Retail bitmap text, horizontally centred in [x, x+w] (or left-aligned), vertically centred on `cy`.
    pub(super) fn text(&self, c: &mut ChildSpawnerCommands, font: &str, s: &str, x: f32, w: f32, cy: f32, centre: bool, tint: Color) {
        let Some(f) = self.fonts.0.get(font).or_else(|| self.fonts.0.get("BlackChancery22")) else { return };
        let mut n = self.node(x, cy - f.height / 2.0, w, f.height);
        n.align_items = AlignItems::Center;
        n.justify_content = if centre { JustifyContent::Center } else { JustifyContent::FlexStart };
        c.spawn(n).with_children(|c| {
            f.glyphs(c, s, self.scale, tint);
        });
    }
    /// Text button over the 3-piece FrontEnd_Texts_Over_Button art (44 px tall in the sheet, stretched to the rect).
    pub(super) fn text_button(&self, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], label: &str, font: &str, action: impl Bundle) {
        let (w, h) = (r - l, b - t);
        let mut root = c.spawn((Interaction::None, action, self.node(l, t, w, h)));
        let mut slice = Entity::PLACEHOLDER;
        root.with_children(|c| {
            slice = self.button_art(c, [0.0, 0.0, w, h]);
            self.text(c, font, label, 0.0, w, h / 2.0, true, Color::WHITE);
        });
        root.insert(Art(vec![(slice, Piece::Slice("button"))]));
    }
    /// The idle gold-rimmed pill of the retail text buttons over `[x, y, w, h]` (one 9-slice node).
    pub(super) fn button_art(&self, c: &mut ChildSpawnerCommands, [x, y, w, h]: [f32; 4]) -> Entity {
        let cap = |name| sprite(self.hud, name, IDLE).map_or(50.0, |(_, r)| r.width());
        let art = sliced(self.assets, "button", MENU_SKIN, "nocursor", cap("FrontEnd_Texts_Over_Button_Left"), cap("FrontEnd_Texts_Over_Button_Right"));
        c.spawn((art, self.node(x, y, w, h))).id()
    }
    /// A text button when it has an action, else its greyed-out retail art.
    pub(super) fn button(&self, c: &mut ChildSpawnerCommands, rect: [f32; 4], label: &str, font: &str, action: Option<impl Bundle>) {
        match action {
            Some(a) => self.text_button(c, rect, label, font, a),
            None => disabled(self, c, rect, label, font),
        }
    }
}

/// One retail sprite stretched over a box (reference units).
pub(super) fn piece(ctx: &Ctx, c: &mut ChildSpawnerCommands, name: &str, rect: [f32; 4]) -> Option<Entity> {
    piece_flipped(ctx, c, name, rect, (false, false))
}

pub(super) fn piece_flipped(ctx: &Ctx, c: &mut ChildSpawnerCommands, name: &str, [l, t, r, b]: [f32; 4], flip: (bool, bool)) -> Option<Entity> {
    let (sheet, rect) = sprite(ctx.hud, name, IDLE)?;
    let mut img = ImageNode::new(ctx.assets.load(format!("hud/tex/{sheet}.png")));
    img.rect = Some(inset(&sheet, rect));
    img.image_mode = NodeImageMode::Stretch;
    (img.flip_x, img.flip_y) = flip;
    Some(c.spawn((img, ctx.node(l, t, r - l, b - t))).id())
}

/// A cropped part of a sprite (see `Piece::Crop`) stretched over `[x, y, w, h]`, with its `Art` entry.
fn crop_piece(ctx: &Ctx, c: &mut ChildSpawnerCommands, name: &str, left: bool, w: f32, [x, y, bw, bh]: [f32; 4]) -> Option<(Entity, Piece)> {
    let (sheet, r) = sprite(ctx.hud, name, IDLE)?;
    let e = ctx.image(c, &format!("hud/tex/{sheet}.png"), Some(inset(&sheet, crop(r, left, w))), x, y, bw, bh, Color::WHITE);
    Some((e, Piece::Crop { name: name.to_string(), left, w }))
}

/// A left cap | stretched centre | right cap strip of three sprites over `[l, t, r, b]`, the caps `cap` wide.
fn strip(ctx: &Ctx, c: &mut ChildSpawnerCommands, [left, centre, right]: [&str; 3], [l, t, r, b]: [f32; 4], cap: f32) {
    piece(ctx, c, left, [l, t, l + cap, b]);
    piece(ctx, c, centre, [l + cap, t, (r - cap).max(l + cap), b]);
    piece(ctx, c, right, [r - cap, t, r, b]);
}

/// A greyed-out retail text button (not clickable).
pub(super) fn disabled(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], label: &str, font: &str) {
    let names = ["FrontEnd_Texts_Over_Button_Disabled_Left", "FrontEnd_Texts_Over_Button_Disabled_Center", "FrontEnd_Texts_Over_Button_Disabled_Right"];
    strip(ctx, c, names, [l, t, r, b], 50.0 * (b - t) / 44.0);
    ctx.text(c, font, label, l, r - l, (t + b) / 2.0, true, Color::srgb(0.55, 0.55, 0.55));
}

/// The current tab of a Ships / Crew / Arms row: the green `FrontEnd_Texts_Over_Button_Selected_*` button, not clickable.
pub(super) fn selected_tab(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], label: &str, font: &str) {
    let names = ["FrontEnd_Texts_Over_Button_Selected_Left", "FrontEnd_Texts_Over_Button_Selected_Center", "FrontEnd_Texts_Over_Button_Selected_Right"];
    strip(ctx, c, names, [l, t, r, b], 51.0 * (b - t) / 45.0);
    ctx.text(c, font, label, l, r - l, (t + b) / 2.0, true, Color::WHITE);
}

/// A round retail arrow button (`FrontEnd_Prev_Button`, `FrontEnd_Next_Button`, ...).
pub(super) fn arrow(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], name: &str, action: impl Bundle) {
    let forward = name.contains("Next") || name.contains("Right");
    let mut root = c.spawn((Interaction::None, action, ArrowButton { forward }, ctx.node(l, t, r - l, b - t)));
    let mut parts = vec![];
    root.with_children(|c| parts.extend(piece(ctx, c, name, [0.0, 0.0, r - l, b - t]).map(|e| (e, Piece::Sprite(name.to_string())))));
    root.insert(Art(parts));
}

/// The retail framed panel: `FRONTEND_STRETCHEDWINDOW_*` gives one corner (mirrored to the other three), an edge column/row and a fill.
pub(super) fn stretched_window(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4]) {
    let k = 33.0; // corner tile size
    piece(ctx, c, "FRONTEND_STRETCHEDWINDOW_CENTERFILL_TILE", [l + k, t + k, r - k, b - k]);
    for (flip_x, x0, x1) in [(false, l, l + k), (true, r - k, r)] {
        for (flip_y, y0, y1) in [(false, t, t + k), (true, b - k, b)] {
            piece_flipped(ctx, c, "FRONTEND_STRETCHEDWINDOW_TOPLEFT_CORNERTILE", [x0, y0, x1, y1], (flip_x, flip_y));
        }
        piece_flipped(ctx, c, "FRONTEND_STRETCHEDWINDOW_LEFTCENTER_TILE", [x0, t + k, x1, b - k], (flip_x, false));
    }
    for (flip_y, y0, y1) in [(false, t, t + k), (true, b - k, b)] {
        piece_flipped(ctx, c, "FRONTEND_STRETCHEDWINDOW_TOPCENTER_TILE", [l + k, y0, r - k, y1], (false, flip_y));
    }
}

/// The retail vertical scrollbar for a list of `total` rows of which `visible` show from row `first`: track
/// (`..._SLIDER_BACKGROUND_TOP/STRETCHED_CENTER/BOTTOM`) and a thumb that never gets shorter than its two end caps and stays inside the
/// track; pressing and dragging the track moves it (`menu::scroll_drag`, the same everywhere).
pub(super) fn scrollbar(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], kind: ScrollKind, total: usize, visible: usize, first: usize) {
    let cap = |name: &str| sprite(ctx.hud, name, IDLE).map_or(20.0, |(_, r)| r.height());
    let track = |c: &mut ChildSpawnerCommands, prefix: &str, t: f32, b: f32| {
        let (top, bottom) = (cap(&format!("{prefix}_TOP")), cap(&format!("{prefix}_BOTTOM")));
        piece(ctx, c, &format!("{prefix}_TOP"), [l, t, r, t + top]);
        piece(ctx, c, &format!("{prefix}_STRETCHED_CENTER"), [l, t + top, r, b - bottom]);
        piece(ctx, c, &format!("{prefix}_BOTTOM"), [l, b - bottom, r, b]);
    };
    track(c, "FRONTEND_SCROLLBAR_SLIDER_BACKGROUND", t, b);
    let h = b - t;
    let min = cap("FRONTEND_SCROLLBAR_SLIDER_TOP") + cap("FRONTEND_SCROLLBAR_SLIDER_BOTTOM");
    let share = if total == 0 { 1.0 } else { (visible as f32 / total as f32).min(1.0) };
    let len = (share * h).max(min).min(h);
    let pos = first as f32 / total.saturating_sub(visible).max(1) as f32;
    let top = t + pos.clamp(0.0, 1.0) * (h - len);
    track(c, "FRONTEND_SCROLLBAR_SLIDER", top, top + len);
    c.spawn((Interaction::None, ScrollTrack { kind, total, visible, thumb: len / h }, RelativeCursorPosition::default(), ctx.node(l, t, r - l, h)));
}

/// The Create Skirmish slider: the scrollbar art whose rounded 33 px ends are the step buttons, a 134 px track between them and a
/// knob that follows `fraction` (dragging the track moves it, see `menu::slider_drag`).
pub(super) fn slider(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], fraction: f32, dec: impl Bundle, inc: impl Bundle, kind: SliderTrack) {
    const CAP: f32 = 33.0;
    const ENDS: [&str; 2] = ["FrontEnd_Horizontal_Scrollbar_Background_Left", "FrontEnd_Horizontal_Scrollbar_Background_Right"];
    let (cy, half) = ((t + b) / 2.0, (r - l) / 2.0);
    let top = cy - 16.5;
    // The body: left half of the left piece, right half of the right piece, both idle.
    for (name, left, x) in [(ENDS[0], true, l), (ENDS[1], false, l + half)] {
        crop_piece(ctx, c, name, left, half, [x, top, half, 33.0]);
    }
    end_cap(ctx, c, ENDS[0], true, [l, top, CAP, 33.0], Some((dec, ArrowButton { forward: false })));
    end_cap(ctx, c, ENDS[1], false, [r - CAP, top, CAP, 33.0], Some((inc, ArrowButton { forward: true })));
    let track_w = r - l - 2.0 * CAP;
    let mut track = c.spawn((Interaction::None, kind, RelativeCursorPosition::default(), ctx.node(l + CAP, top, track_w, 33.0)));
    let mut parts = vec![];
    track.with_children(|c| {
        let x = fraction * (track_w - SLIDER_KNOB);
        let name = "FrontEnd_Horizontal_Scrollbar_Slider";
        parts.extend(piece(ctx, c, name, [x, 5.5, x + SLIDER_KNOB, 5.5 + SLIDER_KNOB]).map(|e| (e, Piece::Sprite(name.into()))));
    });
    track.insert(Art(parts));
}

/// A round 33 px end of the horizontal scrollbar art drawn as a step button over `[x, y, w, h]` (inert without an action).
fn end_cap(ctx: &Ctx, c: &mut ChildSpawnerCommands, name: &str, left: bool, [x, y, w, h]: [f32; 4], action: Option<impl Bundle>) {
    let mut root = c.spawn((Interaction::None, ctx.node(x, y, w, h)));
    if let Some(a) = action {
        root.insert(a);
    }
    let mut parts = vec![];
    root.with_children(|c| parts.extend(crop_piece(ctx, c, name, left, 33.0, [0.0, 0.0, w, h])));
    root.insert(Art(parts));
}

/// The fleet slot bar under the Ships / Crew / Arms screens ('Fleet Select Button' [27,479,459,559]) as measured: round scroll arrows
/// at both ends (live while slots are out of view on that side) and `SLOT_BAR` framed slots 65 apart from x 75 (60x62). `icons` holds
/// the fleet's ship-bar icons: a filled slot shows its icon and picks, an empty one shows its number; `current` gets a gold tint.
/// The arrows are the round end buttons of the horizontal scrollbar art (the FrontEnd_Scroll_*_Button entries point at an unrelated
/// sheet region): the first 33 px of FrontEnd_Horizontal_Scrollbar_Background_Left, the last 33 of _Right.
pub(super) fn fleet_bar<A: Bundle>(
    ctx: &Ctx,
    c: &mut ChildSpawnerCommands,
    (scroll, limit): (usize, usize),
    icons: &[Option<&str>],
    current: Option<usize>,
    scroll_by: impl Fn(i32) -> A,
    pick: impl Fn(usize) -> A,
) {
    for (left, x, live) in [(true, 36.0, scroll > 0), (false, 402.0, scroll + SLOT_BAR < limit)] {
        let name = if left { "FrontEnd_Horizontal_Scrollbar_Background_Left" } else { "FrontEnd_Horizontal_Scrollbar_Background_Right" };
        end_cap(ctx, c, name, left, [x, 507.0, 22.0, 22.0], live.then(|| scroll_by(if left { -1 } else { 1 })));
    }
    for k in 0..SLOT_BAR {
        let (i, x) = (scroll + k, 75.0 + k as f32 * 65.0);
        let mut b = c.spawn((Interaction::None, ctx.node(x, 488.0, 60.0, 62.0)));
        if i < icons.len() {
            b.insert(pick(i));
        }
        b.with_children(|c| {
            piece(ctx, c, "FrontEnd_Flag_Icon_Background", [0.0, 0.0, 60.0, 62.0]);
            match icons.get(i).copied().flatten() {
                Some(icon) => {
                    piece(ctx, c, icon, [8.0, 9.0, 52.0, 53.0]);
                    if current == Some(i) {
                        ctx.fill(c, [4.0, 4.0, 52.0, 54.0], GOLD.with_alpha(0.18));
                    }
                }
                None => ctx.text(c, "UniversBold14", &(i + 1).to_string(), 0.0, 60.0, 31.0, true, Color::WHITE),
            }
        });
    }
}

/// The info panel of the Ships / Crew / Arms screens, as measured on the original Navy Frigate panel: the Univers12 title at
/// (510, 130); the framed icon at (510, 151) 58x59; 'Point' / 'Cost' in Univers10 and the cost in gold Univers26 under it; Univers10
/// label / value lines from y 161.5, 16 apart, labels at x 580, the colon at 676 and gold values at 686.
pub(super) fn info_panel(ctx: &Ctx, c: &mut ChildSpawnerCommands, title: &str, icon: Option<&str>, cost: Option<u32>, lines: &[(String, String)], cost_label: [&str; 2]) {
    ctx.text(c, BODY_FONT, title, 510.0, 240.0, 130.0, false, Color::WHITE);
    piece(ctx, c, "FrontEnd_Flag_Icon_Background", [510.0, 151.0, 568.0, 210.0]);
    if let Some(icon) = icon {
        piece(ctx, c, icon, [517.0, 158.0, 561.0, 203.0]);
    }
    if let Some(cost) = cost {
        ctx.text(c, "Univers10", cost_label[0], 509.0, 60.0, 220.0, true, Color::WHITE);
        ctx.text(c, "Univers10", cost_label[1], 509.0, 60.0, 236.0, true, Color::WHITE);
        ctx.text(c, "Univers26", &cost.to_string(), 499.0, 80.0, 256.0, true, GOLD);
    }
    for (k, (label, value)) in lines.iter().enumerate() {
        let cy = 161.5 + k as f32 * 16.0;
        ctx.text(c, "Univers10", label, 580.0, 95.0, cy, false, Color::WHITE);
        ctx.text(c, "Univers10", ":", 676.0, 8.0, cy, false, Color::WHITE);
        ctx.text(c, "Univers10", value, 686.0, 80.0, cy, false, GOLD);
    }
}

/// One clickable Univers12 row of a list (`[x, y, w, h]`, text 20 in), on the teal bar when `picked`.
pub(super) fn list_row(ctx: &Ctx, c: &mut ChildSpawnerCommands, [x, y, w, h]: [f32; 4], label: &str, picked: bool, action: impl Bundle) {
    let fill = BackgroundColor(if picked { PICKED } else { Color::NONE });
    c.spawn((Interaction::None, action, fill, ctx.node(x, y, w, h))).with_children(|c| ctx.text(c, BODY_FONT, label, 20.0, w - 20.0, h / 2.0, false, Color::WHITE));
}

/// A retail radio choice: check box (`FrontEnd_Check_Box_Ready` ticked / `_NotReady`) and its Univers12 label.
pub(super) fn radio(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], label: &str, on: bool, action: impl Bundle) {
    let cy = (t + b) / 2.0 - 3.0; // measured: the original row sits 3 units above the element's centre
    let mut root = c.spawn((Interaction::None, action, ctx.node(l, t, r - l, b - t)));
    root.with_children(|c| {
        piece(ctx, c, if on { "FrontEnd_Check_Box_Ready" } else { "FrontEnd_Check_Box_NotReady" }, [0.0, (b - t) / 2.0 - 10.0, 18.0, (b - t) / 2.0 + 10.0]);
        ctx.text(c, BODY_FONT, label, 24.0, r - l - 24.0, cy - t, false, Color::WHITE);
    });
}

/// White text word-wrapped inside `[l, t, r, b]`, cut off at the bottom.
pub(super) fn wrapped(ctx: &Ctx, c: &mut ChildSpawnerCommands, font: &str, s: &str, [l, t, r, b]: [f32; 4]) {
    let Some(f) = ctx.fonts.0.get(font) else { return };
    let mut line = String::new();
    let mut y = t + f.height / 2.0;
    for word in s.split_whitespace() {
        let next = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if f.width(&next) > r - l && !line.is_empty() {
            ctx.text(c, font, &line, l, r - l, y, false, Color::WHITE);
            y += f.height;
            line = word.to_string();
            if y > b {
                return;
            }
        } else {
            line = next;
        }
    }
    ctx.text(c, font, &line, l, r - l, y, false, Color::WHITE);
}

/// A list cell with left-aligned text, highlighted when `on`; clickable when it has an action.
pub(super) fn cell(ctx: &Ctx, c: &mut ChildSpawnerCommands, [x, y, w, h]: [f32; 4], label: &str, on: bool, action: Option<impl Bundle>) {
    let tint = if on {
        Color::srgb(1.0, 0.9, 0.4)
    } else if action.is_some() {
        Color::WHITE
    } else {
        Color::srgb(0.75, 0.75, 0.75)
    };
    let rest = if on { CELL_ON } else { CELL };
    let mut b = c.spawn((ctx.node(x, y, w, h), BackgroundColor(rest)));
    if let Some(a) = action {
        b.insert((Interaction::None, a, Rest(rest)));
    }
    b.with_children(|c| ctx.text(c, BODY_FONT, label, 8.0, w - 8.0, h / 2.0, false, tint));
}

/// A retail pull-down box (`FrontEnd_Left/Center/Right_PullDown_Background`, `_Selected` for the local player) with centred Univers10
/// text and, for drop-downs, the `FrontEnd_PullDown_Arrow` at its right end.
pub(super) fn pulldown_cell(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], label: &str, selected: bool, action: Option<impl Bundle>, arrow: bool) {
    let sel = if selected { "_Selected" } else { "" };
    let mut root = c.spawn((ctx.node(l, t, r - l, b - t), Interaction::None));
    if let Some(a) = action {
        root.insert(a);
    }
    root.with_children(|c| {
        let (w, h) = (r - l, b - t);
        let names = ["Left", "Center", "Right"].map(|p| format!("FrontEnd_{p}_PullDown_Background{sel}"));
        strip(ctx, c, names.each_ref().map(String::as_str), [0.0, 0.0, w, h], 12.0);
        let text_w = if arrow { w - 24.0 } else { w };
        ctx.text(c, "Univers10", label, 0.0, text_w, h / 2.0, true, Color::WHITE);
        if arrow {
            piece(ctx, c, "FrontEnd_PullDown_Arrow", [w - 20.0, h / 2.0 - 8.5, w, h / 2.0 + 8.5]);
        }
    });
}

/// An open pull-down list, shared by every screen: a full-screen blocker (a click elsewhere runs `close`) and one backed entry per
/// `(label, action)` stacked `h` apart from `[x, y, w, h]`. Entries also pick on the release of a held press (`menu::dropdown_release`),
/// except the `keep` one, drawn highlighted (a plain click on the lobby's current value keeps the list open).
pub(super) fn dropdown<A: Component>(ctx: &Ctx, c: &mut ChildSpawnerCommands, close: A, [x, y, w, h]: [f32; 4], entries: Vec<(String, A)>, keep: Option<usize>) {
    c.spawn((Interaction::None, FocusPolicy::Block, close, ctx.node(0.0, 0.0, 800.0, 600.0), ZIndex(1)));
    for (k, (label, action)) in entries.into_iter().enumerate() {
        c.spawn((ctx.node(x, y + k as f32 * h, w, h), ZIndex(2), BackgroundColor(LIST_BACKING))).with_children(|c| {
            pulldown_cell(ctx, c, [0.0, 0.0, w, h], &label, keep == Some(k), Some((action, ListEntry(keep != Some(k)))), false);
        });
    }
}

/// 'Current Victory Points : N' (UniversLightBold14, the number in gold), centred in its area.
pub(super) fn victory_points(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], label: &str, points: i64) {
    let Some(f) = ctx.fonts.0.get("UniversLightBold14") else { return };
    let n = points.to_string();
    let x = (l + r - f.width(label) - f.width(&n)) / 2.0;
    ctx.text(c, "UniversLightBold14", label, x, f.width(label), (t + b) / 2.0, false, Color::WHITE);
    ctx.text(c, "UniversLightBold14", &n, x + f.width(label), f.width(&n) + 2.0, (t + b) / 2.0, false, GOLD);
}

/// One mouse-wheel step of a list's first visible row: up scrolls back, down forward up to `max`.
pub(super) fn wheel_step(first: usize, dy: f32, max: usize) -> usize {
    if dy > 0.0 { first.saturating_sub(1) } else { (first + 1).min(max) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_keeps_the_named_side() {
        let r = Rect::new(10.0, 0.0, 110.0, 20.0);
        assert_eq!(crop(r, true, 30.0), Rect::new(10.0, 0.0, 40.0, 20.0));
        assert_eq!(crop(r, false, 30.0), Rect::new(80.0, 0.0, 110.0, 20.0));
    }

    #[test]
    fn wheel_step_stays_in_range() {
        assert_eq!(wheel_step(0, 1.0, 5), 0);
        assert_eq!(wheel_step(5, -1.0, 5), 5);
        assert_eq!(wheel_step(2, -1.0, 5), 3);
    }
}
