//! The classic front end's widget kit: retail hud.hdt FrontEnd_* art (Navy set, `MENU_SKIN`) laid out on the 800x600 reference
//! canvas and scaled to the window. Every retail screen (`menu.rs`, `crew_arms.rs`) builds from these; behaviour stays in the shared
//! markers each widget carries (`menu::Action`, `crew_arms::Action`, `ScrollTrack`, ...). `hover` swaps the hover/press art.

#![allow(deprecated)] // shortcut: bevy 0.20 Interaction (deprecated for picking Hovered/Pressed); port when it is removed
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};

use crate::menu::{ArrowButton, ListEntry, RunButtons, SLIDER_KNOB, SLOT_BAR, ScrollKind, ScrollTrack, SliderTrack, menu_active};
use crate::sprites::{BODY_FONT, Fonts, HudData, IDLE, MENU_SKIN, meta_in, sprite_in};
use crate::theme::{Look, Looks, State, Theme};

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
}

/// Hover/press art: a widget's pieces, swapped between NoCursor / CursorOver / CursorDown by `hover`.
#[derive(Component)]
struct Art(Vec<(Entity, Piece)>);

/// Which drawn control family a node belongs to (`hover_drawn` restyles it with the cursor).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Family {
    Button,
    Pulldown,
}

/// A control drawn from `theme.json`: its family, and whether its resting state is the selected one.
#[derive(Component, Clone, Copy)]
struct Drawn {
    family: Family,
    selected: bool,
}

/// The state a drawn control shows: the cursor's wins, else selected or idle.
pub(super) fn state_of(interaction: Interaction, selected: bool) -> State {
    match interaction {
        Interaction::Pressed => State::Pressed,
        Interaction::Hovered => State::Hover,
        Interaction::None if selected => State::Selected,
        Interaction::None => State::Idle,
    }
}

fn looks(theme: &Theme, family: Family) -> &Looks {
    match family {
        Family::Button => &theme.button,
        Family::Pulldown => &theme.pulldown,
    }
}

fn gradient(look: &Look) -> BackgroundGradient {
    let [top, mid, bottom] = look.fill;
    BackgroundGradient::from(LinearGradient::to_bottom(vec![
        ColorStop::new(top, Val::Percent(0.0)),
        ColorStop::new(mid, Val::Percent(50.0)),
        ColorStop::new(bottom, Val::Percent(100.0)),
    ]))
}

fn rim(look: &Look) -> BorderColor {
    BorderColor { top: look.rim[0], left: look.rim[0], right: look.rim[1], bottom: look.rim[1] }
}

/// Restyle drawn buttons and pull-downs when the cursor enters, presses or leaves them.
fn hover_drawn(mut q: Query<(&Interaction, &Drawn, &mut BackgroundGradient, &mut BorderColor), Changed<Interaction>>) {
    let theme = Theme::current();
    for (i, drawn, mut fill, mut border) in &mut q {
        let look = looks(theme, drawn.family).get(state_of(*i, drawn.selected));
        *fill = gradient(look);
        *border = rim(look);
    }
}

/// The fill a cell returns to when the cursor leaves it.
#[derive(Component)]
pub(super) struct Rest(pub(super) Color);

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (hover, hover_drawn, hover_knob).after(RunButtons).run_if(menu_active)).add_systems(Update, fit_cover.after(crate::menu::BuildMenu));
}

/// A backdrop picture that covers its frame: scaled up (never distorted) until both sides are filled, centre-cropped. The picture's
/// size is only known once it has loaded, so `fit_cover` sizes the node then. `ratio` is the height of the part of the picture to
/// show per unit of its width (retail files carry spare rows below their picture; a mod's picture of any aspect shows whole).
#[derive(Component)]
pub(super) struct Cover {
    /// The frame in physical px.
    frame: Vec2,
    ratio: f32,
}
#[derive(Component)]
struct Fitted;

fn fit_cover(mut commands: Commands, images: Res<Assets<Image>>, mut q: Query<(Entity, &Cover, &mut ImageNode, &mut Node), Without<Fitted>>) {
    for (e, cover, mut img, mut node) in &mut q {
        let Some(size) = images.get(&img.image).map(|i| i.size().as_vec2()) else { continue };
        let shown = Vec2::new(size.x, size.y.min(size.x * cover.ratio));
        let size = cover_size(shown, cover.frame);
        img.rect = Some(Rect::from_corners(Vec2::ZERO, shown));
        (node.left, node.top) = (Val::Px((cover.frame.x - size.x) / 2.0), Val::Px((cover.frame.y - size.y) / 2.0));
        (node.width, node.height) = (Val::Px(size.x), Val::Px(size.y));
        commands.entity(e).insert(Fitted);
    }
}

/// The size a picture of `shown` px is drawn at to cover `frame` without distortion.
fn cover_size(shown: Vec2, frame: Vec2) -> Vec2 {
    shown * (frame.x / shown.x).max(frame.y / shown.y)
}

/// Hover/press art of the retail buttons and list cells.
fn hover(
    q: Query<(Entity, &Interaction, Option<&Art>, Option<&Rest>), Changed<Interaction>>,
    mut fills: Query<&mut BackgroundColor>,
    assets: Res<AssetServer>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    mut imgs: Query<(&mut ImageNode, &mut Node)>,
) {
    let scale = canvas(&window).1;
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
            let Ok((mut img, mut node)) = imgs.get_mut(*e) else { continue };
            match piece {
                Piece::Sprite(name) => {
                    if let Some(art) = sprite_in(&assets, MENU_SKIN, name, state) {
                        art.show(&mut img, &mut node, scale);
                    }
                }
            }
        }
    }
}

/// Untrimmed reference size of a menu sprite (the Navy set, `MENU_SKIN`).
pub(super) fn size_of(name: &str) -> Option<Vec2> {
    meta_in(MENU_SKIN, name, IDLE).map(|m| m.size)
}

/// What every widget needs: art, fonts and the reference-to-window scale.
pub(super) struct Ctx<'a> {
    pub(super) assets: &'a AssetServer,
    pub(super) hud: &'a HudData,
    pub(super) fonts: &'a Fonts,
    pub(super) scale: f32,
    pub(super) theme: &'a Theme,
    /// Reference px of window beyond the canvas on each side (zero at 4:3): where backdrops reach past it.
    pub(super) pad: Vec2,
}

/// `Ctx::pad` of a window.
pub(super) fn pad(window: &Window, scale: f32) -> Vec2 {
    Vec2::new((window.width() / scale - 800.0) / 2.0, (window.height() / scale - 600.0) / 2.0).max(Vec2::ZERO)
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
            left: crate::panels::px(x, self.scale),
            top: crate::panels::px(y, self.scale),
            // Right and bottom edges are rounded, so abutting boxes share an edge.
            width: Val::Px(crate::panels::snap(x + w, self.scale) - crate::panels::snap(x, self.scale)),
            height: Val::Px(crate::panels::snap(y + h, self.scale) - crate::panels::snap(y, self.scale)),
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
    /// A full-screen backdrop picture: covers the whole window (the 800x600 canvas sits on it), so a mod's wider picture fills it.
    pub(super) fn backdrop(&self, c: &mut ChildSpawnerCommands, path: &str) {
        let (w, h) = (800.0 + 2.0 * self.pad.x, 600.0 + 2.0 * self.pad.y);
        self.cover(c, path, [-self.pad.x, -self.pad.y, w, h], 0.75);
    }
    /// The picture at `path` covering `[x, y, w, h]` (reference units, may reach past the canvas); see `Cover`.
    pub(super) fn cover(&self, c: &mut ChildSpawnerCommands, path: &str, [x, y, w, h]: [f32; 4], ratio: f32) {
        let mut img = ImageNode::new(self.assets.load(path.to_string()));
        img.image_mode = NodeImageMode::Stretch;
        let frame = Node { overflow: Overflow::clip(), ..self.node(x, y, w, h) };
        let picture = Node { position_type: PositionType::Absolute, width: Val::Px(0.0), height: Val::Px(0.0), ..default() };
        c.spawn(frame).with_children(|c| {
            c.spawn((img, picture, Cover { frame: Vec2::new(w, h) * self.scale, ratio }));
        });
    }
    /// A flat colour box.
    pub(super) fn fill(&self, c: &mut ChildSpawnerCommands, [x, y, w, h]: [f32; 4], colour: Color) {
        c.spawn((self.node(x, y, w, h), BackgroundColor(colour)));
    }
    /// UI text in the vector face of a retail font (retail advances for the layout), horizontally centred in [x, x+w] (or left-aligned), vertically centred on `cy`.
    pub(super) fn text(&self, c: &mut ChildSpawnerCommands, font: &str, s: &str, x: f32, w: f32, cy: f32, centre: bool, tint: Color) {
        let Some(f) = self.fonts.0.get(font).or_else(|| self.fonts.0.get("BlackChancery22")) else { return };
        let mut n = self.node(x, cy - f.height / 2.0, w, f.height);
        n.align_items = AlignItems::Center;
        n.justify_content = if centre { JustifyContent::Center } else { JustifyContent::FlexStart };
        c.spawn(n).with_children(|c| {
            // The vector text is as wide as it is, not as the retail advances: centre that, not the advance sum.
            c.spawn((f.text(s, self.scale, tint), TextLayout::no_wrap(), Node { flex_shrink: 0.0, ..default() }));
        });
    }
    /// A drawn text button over `[l, t, r, b]`: a pill with a gold rim and a gradient fill that follows the cursor (theme.json `button`).
    pub(super) fn text_button(&self, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], label: &str, font: &str, action: impl Bundle) {
        let (w, h) = (r - l, b - t);
        let inset = self.theme.button.rim_width;
        let mut root = c.spawn((Interaction::None, action, Drawn { family: Family::Button, selected: false }, self.drawn(Family::Button, State::Idle, [l, t, w, h])));
        // Children sit inside the rim, so the label is shifted back by its width to stay centred on the whole pill.
        root.with_children(|c| self.text(c, font, label, -inset, w, h / 2.0 - inset, true, Color::WHITE));
    }
    /// The idle pill of a text button over `[x, y, w, h]` (no label, not clickable).
    pub(super) fn button_art(&self, c: &mut ChildSpawnerCommands, rect: [f32; 4]) -> Entity {
        c.spawn(self.drawn(Family::Button, State::Idle, rect)).id()
    }
    /// The node, rim, fill and shadow of a drawn control in `state` over `[x, y, w, h]` (reference units, edges snapped to whole pixels):
    /// a button is a pill, a pull-down a rounded box.
    fn drawn(&self, family: Family, state: State, [x, y, w, h]: [f32; 4]) -> impl Bundle {
        let looks = looks(self.theme, family);
        let look = looks.get(state);
        let [left, top, width, height, ..] = panel_geometry([x, y, x + w, y + h], PanelKind::Band, self.theme, self.scale);
        let border = crate::panels::snap(looks.rim_width, self.scale).max(1.0).min(height / 2.0);
        let radius = if family == Family::Button { width.min(height) / 2.0 } else { crate::panels::snap(6.0, self.scale).min(height / 2.0) };
        (
            Node { border: UiRect::all(Val::Px(border)), border_radius: BorderRadius::all(Val::Px(radius)), ..bare(left, top, width, height) },
            gradient(look),
            rim(look),
            BoxShadow::new(Color::BLACK.with_alpha(0.35), Val::Px(0.0), Val::Px(self.scale.round()), Val::Px(0.0), Val::Px((3.0 * self.scale).round())),
        )
    }
    /// A text button when it has an action, else its greyed-out retail art.
    pub(super) fn button(&self, c: &mut ChildSpawnerCommands, rect: [f32; 4], label: &str, font: &str, action: Option<impl Bundle>) {
        match action {
            Some(a) => self.text_button(c, rect, label, font, a),
            None => disabled(self, c, rect, label, font),
        }
    }
}

/// The kinds of drawn panel (`panel`), looks from `data/hud/theme.json`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PanelKind {
    /// Big translucent gold-rimmed rounded panel.
    Outer,
    /// Dark list / information box with a teal hairline.
    Inner,
    /// The lighter strip along the bottom of a list box (flat, no rim).
    Band,
    /// A small bevelled frame: a fleet slot, the ship icon, a stat bar.
    Slot,
}

/// Physical-px geometry of a panel: `(left, top, width, height, border, radius)`, every edge on a whole pixel so the rim stays crisp
/// at fractional scales (the right and bottom edges are rounded, so abutting panels share them).
pub(super) fn panel_geometry([l, t, r, b]: [f32; 4], kind: PanelKind, theme: &Theme, scale: f32) -> [f32; 6] {
    use crate::panels::snap;
    let (border, radius) = match kind {
        PanelKind::Outer => (theme.rim_width, theme.panel_radius),
        PanelKind::Inner => (theme.edge_width, theme.box_radius),
        PanelKind::Band => (0.0, 0.0),
        PanelKind::Slot => (theme.edge_width, theme.slot_radius),
    };
    let border = if border > 0.0 { snap(border, scale).max(1.0) } else { 0.0 };
    let (w, h) = (snap(r, scale) - snap(l, scale), snap(b, scale) - snap(t, scale));
    [snap(l, scale), snap(t, scale), w, h, border, snap(radius, scale).min(w.min(h) / 2.0).floor()]
}

/// A drawn panel over the retail rect `[l, t, r, b]` (reference units): rounded corners, a gold bevelled rim or teal hairline, a gradient
/// fill and a soft shadow, all UI node features (no art). Returns the node so a caller can parent content to it.
pub(super) fn panel(ctx: &Ctx, c: &mut ChildSpawnerCommands, rect: [f32; 4], kind: PanelKind) -> Entity {
    let theme = ctx.theme;
    let [left, top, width, height, border, radius] = panel_geometry(rect, kind, theme, ctx.scale);
    let node = Node {
        position_type: PositionType::Absolute,
        left: Val::Px(left),
        top: Val::Px(top),
        width: Val::Px(width),
        height: Val::Px(height),
        border: UiRect::all(Val::Px(border)),
        border_radius: BorderRadius::all(Val::Px(radius)),
        ..default()
    };
    let shade = |k: f32| {
        let s = theme.panel_fill.to_srgba();
        Color::srgba((s.red * k).min(1.0), (s.green * k).min(1.0), (s.blue * k).min(1.0), s.alpha)
    };
    match kind {
        PanelKind::Outer => c
            .spawn((
                node,
                BackgroundGradient::from(LinearGradient::to_bottom(vec![ColorStop::new(shade(1.25), Val::Percent(0.0)), ColorStop::new(shade(0.85), Val::Percent(100.0))])),
                BorderColor { top: theme.rim_light, left: theme.rim_light, right: theme.rim, bottom: theme.rim },
                BoxShadow::new(
                    Color::BLACK.with_alpha(theme.shadow_alpha),
                    Val::Px(0.0),
                    Val::Px((2.0 * ctx.scale).round()),
                    Val::Px(0.0),
                    Val::Px((theme.shadow_blur * ctx.scale).round()),
                ),
            ))
            .id(),
        PanelKind::Inner => c.spawn((node, BackgroundColor(theme.box_fill), BorderColor::all(theme.edge))).id(),
        PanelKind::Slot => c.spawn((node, BackgroundColor(theme.box_fill), BorderColor { top: theme.rim_light, left: theme.rim_light, right: theme.rim, bottom: theme.rim })).id(),
        PanelKind::Band => c.spawn((node, BackgroundColor(theme.band_fill))).id(),
    }
}

/// The scenery and panels of the Ships, Crew and Arms screens: `menu/gamelobby.png` (the scene without the panels) over the whole
/// window, then the engine-drawn panels at the rects measured on the retail `menu/shipscreen.png` (PROVISIONAL, 800x600 part): the
/// fleet list panel, the ship information panel, the slot bar panel, the list box with its band and the information box.
pub(super) fn ship_screen_backdrop(ctx: &Ctx, c: &mut ChildSpawnerCommands) {
    ctx.backdrop(c, "menu/gamelobby.png");
    for rect in [[23.0, 38.0, 459.0, 471.0], [482.0, 38.0, 776.0, 506.0], [25.0, 477.0, 459.0, 560.0]] {
        panel(ctx, c, rect, PanelKind::Outer);
    }
    panel(ctx, c, [44.0, 89.0, 440.0, 425.0], PanelKind::Inner);
    panel(ctx, c, [46.0, 375.0, 438.0, 423.0], PanelKind::Band);
    panel(ctx, c, [492.0, 117.0, 768.0, 283.0], PanelKind::Inner);
}

/// One retail sprite stretched over a box (reference units).
pub(super) fn piece(ctx: &Ctx, c: &mut ChildSpawnerCommands, name: &str, rect: [f32; 4]) -> Option<Entity> {
    piece_flipped(ctx, c, name, rect, (false, false))
}

pub(super) fn piece_flipped(ctx: &Ctx, c: &mut ChildSpawnerCommands, name: &str, [l, t, r, b]: [f32; 4], flip: (bool, bool)) -> Option<Entity> {
    let art = sprite_in(ctx.assets, MENU_SKIN, name, IDLE)?;
    let mut img = art.image_node();
    img.image_mode = NodeImageMode::Stretch;
    (img.flip_x, img.flip_y) = flip;
    let mut node = ctx.node(l, t, r - l, b - t);
    art.fit(Vec2::new(r - l, b - t)).flipped(flip.0, flip.1).place(&mut node, ctx.scale, UiRect::DEFAULT);
    Some(c.spawn((img, node)).id())
}

/// A greyed-out retail text button (not clickable).
pub(super) fn disabled(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], label: &str, font: &str) {
    c.spawn(ctx.drawn(Family::Button, State::Disabled, [l, t, r - l, b - t]));
    ctx.text(c, font, label, l, r - l, (t + b) / 2.0, true, Color::srgb(0.55, 0.55, 0.55));
}

/// The current tab of a Ships / Crew / Arms row: the green `FrontEnd_Texts_Over_Button_Selected_*` button, not clickable.
pub(super) fn selected_tab(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], label: &str, font: &str) {
    c.spawn(ctx.drawn(Family::Button, State::Selected, [l, t, r - l, b - t]));
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

/// A vertical scrollbar for a list of `total` rows of which `visible` show from row `first`: a rounded dark track and a gold-rimmed pill
/// thumb that never gets shorter than the track is wide and stays inside it; pressing and dragging the track moves it
/// (`menu::scroll_drag`, the same everywhere).
pub(super) fn scrollbar(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], kind: ScrollKind, total: usize, visible: usize, first: usize) {
    let h = b - t;
    let (len, top) = thumb_span(h, total, visible, first, r - l);
    track(ctx, c, [l, t, r, b]);
    c.spawn(ctx.drawn(Family::Button, State::Idle, [l + 2.0, t + top, r - l - 4.0, len]));
    c.spawn((Interaction::None, ScrollTrack { kind, total, visible, thumb: len / h }, RelativeCursorPosition::default(), ctx.node(l, t, r - l, h)));
}

/// Length and offset (reference px) of a scrollbar thumb in a track `h` long: its share of the track is `visible / total` rows but never
/// less than `min`, and `first` of the `total - visible` scrollable rows places it.
fn thumb_span(h: f32, total: usize, visible: usize, first: usize, min: f32) -> (f32, f32) {
    let share = if total == 0 { 1.0 } else { (visible as f32 / total as f32).min(1.0) };
    let len = (share * h).max(min).min(h);
    let pos = first as f32 / total.saturating_sub(visible).max(1) as f32;
    (len, pos.clamp(0.0, 1.0) * (h - len))
}

/// A rounded dark groove (scrollbar and slider tracks): the inner-box fill with its teal hairline.
fn track(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4]) {
    let [left, top, width, height, border, _] = panel_geometry([l, t, r, b], PanelKind::Inner, ctx.theme, ctx.scale);
    let node = Node { border: UiRect::all(Val::Px(border)), border_radius: BorderRadius::all(Val::Px((width.min(height) / 2.0).floor())), ..bare(left, top, width, height) };
    c.spawn((node, BackgroundColor(ctx.theme.box_fill), BorderColor::all(ctx.theme.edge)));
}

/// The Create Skirmish slider: a rounded track between two round step buttons (33 px, each with a chevron) and a pill knob that follows
/// `fraction` (dragging the track moves it, see `menu::slider_drag`). The 134 px hit box between the buttons is the retail track.
pub(super) fn slider(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], fraction: f32, dec: impl Bundle, inc: impl Bundle, kind: SliderTrack) {
    const CAP: f32 = 33.0;
    let cy = (t + b) / 2.0;
    let top = cy - 16.5;
    track(ctx, c, [l + CAP / 2.0, cy - 7.0, r - CAP / 2.0, cy + 7.0]);
    step_button(ctx, c, true, [l, top, CAP, 33.0], Some((dec, ArrowButton { forward: false })));
    step_button(ctx, c, false, [r - CAP, top, CAP, 33.0], Some((inc, ArrowButton { forward: true })));
    let track_w = r - l - 2.0 * CAP;
    let mut root = c.spawn((Interaction::None, kind, RelativeCursorPosition::default(), ctx.node(l + CAP, top, track_w, 33.0)));
    let mut knob = Entity::PLACEHOLDER;
    root.with_children(|c| {
        let x = fraction * (track_w - SLIDER_KNOB);
        knob = c.spawn((ctx.drawn(Family::Button, State::Idle, [x, 7.5, SLIDER_KNOB, 18.0]),)).id();
    });
    root.insert(Knob(knob));
}

/// The knob of a slider, on the slider's hit box: it follows the cursor over that box.
#[derive(Component)]
struct Knob(Entity);

fn hover_knob(q: Query<(&Interaction, &Knob), Changed<Interaction>>, mut knobs: Query<(&mut BackgroundGradient, &mut BorderColor)>) {
    for (i, Knob(e)) in &q {
        let Ok((mut fill, mut border)) = knobs.get_mut(*e) else { continue };
        let look = Theme::current().button.get(state_of(*i, false));
        *fill = gradient(look);
        *border = rim(look);
    }
}

/// A round step button over `[x, y, w, h]` with a chevron pointing left or right: gold-rimmed and clickable with an action, greyed and
/// inert without one.
fn step_button(ctx: &Ctx, c: &mut ChildSpawnerCommands, left: bool, rect: [f32; 4], action: Option<impl Bundle>) {
    let state = if action.is_some() { State::Idle } else { State::Disabled };
    let mut root = c.spawn(ctx.drawn(Family::Button, state, rect));
    if let Some(a) = action {
        root.insert((Interaction::None, Drawn { family: Family::Button, selected: false }, a));
    }
    let (w, border) = (crate::panels::snap(rect[2], ctx.scale), crate::panels::snap(ctx.theme.button.rim_width, ctx.scale).max(1.0));
    let (cx, cy) = ((w - 2.0 * border) / 2.0, (crate::panels::snap(rect[3], ctx.scale) - 2.0 * border) / 2.0);
    let tint = if state == State::Disabled { Color::srgb(0.55, 0.55, 0.55) } else { Color::WHITE };
    root.with_children(|c| chevron(ctx, c, left, rect[2].min(rect[3]), (cx, cy), tint));
}

/// A chevron of two rotated bars centred on `(cx, cy)` physical px; `size` (reference px) is the button it sits on.
fn chevron(ctx: &Ctx, c: &mut ChildSpawnerCommands, left: bool, size: f32, (cx, cy): (f32, f32), tint: Color) {
    let s = ctx.scale * size / 33.0;
    let (len, thick, dx, dy) = (9.0 * s, 3.0 * s, if left { -1.0 } else { 1.0 } * 0.8 * s, 3.2 * s);
    // "<": the upper bar runs up-right ("/", anticlockwise), the lower down-right ("\"); ">" is the mirror.
    for (up, deg) in [(true, if left { -45.0 } else { 45.0 }), (false, if left { 45.0 } else { -45.0 })] {
        let y = cy + if up { -dy } else { dy };
        let n = Node { border_radius: BorderRadius::all(Val::Px(thick / 2.0)), ..bare(cx + dx - len / 2.0, y - thick / 2.0, len, thick) };
        c.spawn((n, BackgroundColor(tint), UiTransform::from_rotation(Rot2::degrees(deg))));
    }
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
        step_button(ctx, c, left, [x, 507.0, 22.0, 22.0], live.then(|| scroll_by(if left { -1 } else { 1 })));
    }
    for k in 0..SLOT_BAR {
        let (i, x) = (scroll + k, 75.0 + k as f32 * 65.0);
        let mut b = c.spawn((Interaction::None, ctx.node(x, 488.0, 60.0, 62.0)));
        if i < icons.len() {
            b.insert(pick(i));
        }
        b.with_children(|c| {
            panel(ctx, c, [0.0, 0.0, 60.0, 62.0], PanelKind::Slot);
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
    panel(ctx, c, [510.0, 151.0, 568.0, 210.0], PanelKind::Slot);
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

/// A drawn check box over `[x, y, w, h]`: a small rounded square with a gold rim and, when `on`, a tick of two rotated bars.
fn check_box(ctx: &Ctx, c: &mut ChildSpawnerCommands, [x, y, w, h]: [f32; 4], on: bool) {
    let theme = ctx.theme;
    let [left, top, width, height, border, _] = panel_geometry([x, y, x + w, y + h], PanelKind::Inner, theme, ctx.scale);
    let node = Node { border: UiRect::all(Val::Px(border)), border_radius: BorderRadius::all(Val::Px(crate::panels::snap(3.0, ctx.scale))), ..bare(left, top, width, height) };
    c.spawn((node, BackgroundColor(theme.box_fill), BorderColor::all(theme.rim_light))).with_children(|c| {
        if !on {
            return;
        }
        // Bars are placed by their centre inside the box, then turned about it: the short stroke runs down-right, the long one up-right.
        for (len, cx, cy, deg) in [(5.0, 4.5, 8.5, 45.0), (9.0, 8.2, 6.5, -45.0)] {
            let (bw, bh) = (len * ctx.scale, 2.2 * ctx.scale);
            let n = Node { border_radius: BorderRadius::all(Val::Px(bh / 2.0)), ..bare(cx * ctx.scale - bw / 2.0, cy * ctx.scale - bh / 2.0, bw, bh) };
            c.spawn((n, BackgroundColor(GOLD), UiTransform::from_rotation(Rot2::degrees(deg))));
        }
    });
}

/// An absolutely placed node of physical-px geometry.
fn bare(left: f32, top: f32, width: f32, height: f32) -> Node {
    Node { position_type: PositionType::Absolute, left: Val::Px(left), top: Val::Px(top), width: Val::Px(width), height: Val::Px(height), ..default() }
}

/// A radio choice: a drawn check box and its Univers12 label.
pub(super) fn radio(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], label: &str, on: bool, action: impl Bundle) {
    let cy = (t + b) / 2.0 - 3.0; // measured: the original row sits 3 units above the element's centre
    let mut root = c.spawn((Interaction::None, action, ctx.node(l, t, r - l, b - t)));
    root.with_children(|c| {
        check_box(ctx, c, [0.0, (b - t) / 2.0 - 9.0, 18.0, 18.0], on);
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

/// A drawn pull-down box (theme.json `pulldown`, its selected look for the local player) with centred Univers10 text and, for
/// drop-downs, the retail `FrontEnd_PullDown_Arrow` at its right end.
pub(super) fn pulldown_cell(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], label: &str, selected: bool, action: Option<impl Bundle>, arrow: bool) {
    let inset = ctx.theme.pulldown.rim_width;
    let state = state_of(Interaction::None, selected);
    let mut root = c.spawn((Interaction::None, Drawn { family: Family::Pulldown, selected }, ctx.drawn(Family::Pulldown, state, [l, t, r - l, b - t])));
    if let Some(a) = action {
        root.insert(a);
    }
    root.with_children(|c| {
        let (w, h) = (r - l, b - t);
        let text_w = if arrow { w - 24.0 } else { w };
        ctx.text(c, "Univers10", label, -inset, text_w, h / 2.0 - inset, true, Color::WHITE);
        if arrow {
            piece(ctx, c, "FrontEnd_PullDown_Arrow", [w - 20.0 - inset, h / 2.0 - 8.5 - inset, w - inset, h / 2.0 + 8.5 - inset]);
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
    #[ignore = "needs imported assets"]
    fn every_strip_the_widgets_draw_is_a_slice_group() {
        // The groups come from data/hud/slices.json: explicit caps win, else the widths of the end pieces.
        assert_eq!(crate::sprites::slice_caps("hscroll", MENU_SKIN), Some((33.0, 33.0)));
        for group in ["button", "button_disabled", "button_selected", "pulldown", "pulldown_selected"] {
            let (l, r) = crate::sprites::slice_caps(group, MENU_SKIN).unwrap_or_else(|| panic!("no slice group {group}"));
            assert!(l > 0.0 && r > 0.0, "{group}");
        }
        assert_eq!(crate::sprites::slice_caps("no_such_group", MENU_SKIN), None);
    }

    #[test]
    fn backdrops_cover_the_window_and_the_canvas_keeps_its_scale() {
        // 16:9: the 4:3 picture is scaled to the width and cropped top and bottom, never distorted.
        let near = |a: Vec2, b: Vec2| assert!(a.abs_diff_eq(b, 0.01), "{a} != {b}");
        near(cover_size(Vec2::new(800.0, 600.0), Vec2::new(1920.0, 1080.0)), Vec2::new(1920.0, 1440.0));
        // A wide picture fills 21:9 as it is; a taller window scales by height.
        near(cover_size(Vec2::new(2560.0, 1080.0), Vec2::new(2560.0, 1080.0)), Vec2::new(2560.0, 1080.0));
        near(cover_size(Vec2::new(800.0, 600.0), Vec2::new(600.0, 900.0)), Vec2::new(1200.0, 900.0));
        // The canvas is scaled by height on a wide window and has no padding at 4:3; the sides are what is left.
        let mut window = Window::default();
        window.resolution.set(2560.0, 1080.0);
        let scale = canvas(&window).1;
        assert_eq!(scale, 1.8);
        assert_eq!(pad(&window, scale), Vec2::new((2560.0 / 1.8 - 800.0) / 2.0, 0.0));
        window.resolution.set(1600.0, 1200.0);
        assert_eq!(pad(&window, canvas(&window).1), Vec2::ZERO);
    }

    #[test]
    fn cursor_and_selection_map_to_a_state() {
        assert_eq!(state_of(Interaction::None, false), State::Idle);
        assert_eq!(state_of(Interaction::None, true), State::Selected);
        assert_eq!(state_of(Interaction::Hovered, true), State::Hover);
        assert_eq!(state_of(Interaction::Pressed, false), State::Pressed);
        // The looks the states pick are the theme's own per-state colours.
        let t = Theme::default();
        assert_eq!(looks(&t, Family::Button).get(State::Hover).fill[0], t.button.hover.fill[0]);
        assert_ne!(looks(&t, Family::Pulldown).get(State::Idle), looks(&t, Family::Pulldown).get(State::Selected));
    }

    #[test]
    fn thumb_fills_its_share_and_stays_inside_the_track() {
        // 5 of 20 rows show: a quarter of a 100 px track, at the top, middle and bottom of the scroll range.
        assert_eq!(thumb_span(100.0, 20, 5, 0, 10.0), (25.0, 0.0));
        assert_eq!(thumb_span(100.0, 20, 5, 15, 10.0), (25.0, 75.0));
        let (len, top) = thumb_span(100.0, 20, 5, 7, 10.0);
        assert_eq!(len, 25.0);
        assert!((top - 75.0 * 7.0 / 15.0).abs() < 1e-4);
        // A long list keeps the minimum length; a list that fits fills the track; an empty list does not divide by zero.
        assert_eq!(thumb_span(100.0, 1000, 5, 0, 21.0).0, 21.0);
        assert_eq!(thumb_span(100.0, 3, 5, 0, 21.0), (100.0, 0.0));
        assert_eq!(thumb_span(100.0, 0, 5, 0, 21.0), (100.0, 0.0));
        // Scrolled past the end clamps.
        assert_eq!(thumb_span(100.0, 20, 5, 99, 10.0).1, 75.0);
    }

    #[test]
    fn panel_edges_land_on_whole_physical_pixels() {
        let theme = Theme::default();
        for scale in [1.0, 1.125, 1.8, 2.4] {
            for kind in [PanelKind::Outer, PanelKind::Inner, PanelKind::Band, PanelKind::Slot] {
                let g = panel_geometry([23.3, 38.7, 459.4, 471.1], kind, &theme, scale);
                assert!(g.iter().all(|v| v.fract() == 0.0), "{kind:?} at {scale}: {g:?}");
            }
        }
        // 1080p canvas scale 1.8: the 3 px rim is 5 physical px, the box hairline 4, a band has no border.
        assert_eq!(panel_geometry([0.0, 0.0, 10.0, 10.0], PanelKind::Outer, &theme, 1.8)[4], 5.0);
        assert_eq!(panel_geometry([0.0, 0.0, 10.0, 10.0], PanelKind::Inner, &theme, 1.8)[4], 4.0);
        assert_eq!(panel_geometry([0.0, 0.0, 10.0, 10.0], PanelKind::Band, &theme, 1.8)[4], 0.0);
        // Abutting panels share an edge: widths sum to the span.
        let a = panel_geometry([0.0, 0.0, 10.5, 10.0], PanelKind::Band, &theme, 1.125);
        let b = panel_geometry([10.5, 0.0, 20.0, 10.0], PanelKind::Band, &theme, 1.125);
        assert_eq!(a[0] + a[2], b[0]);
    }

    #[test]
    fn wheel_step_stays_in_range() {
        assert_eq!(wheel_step(0, 1.0, 5), 0);
        assert_eq!(wheel_step(5, -1.0, 5), 5);
        assert_eq!(wheel_step(2, -1.0, 5), 3);
    }
}
