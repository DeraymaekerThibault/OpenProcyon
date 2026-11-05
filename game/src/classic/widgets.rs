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
/// Teal bar behind the picked row of a list (theme.json `picked`).
pub(super) fn picked() -> Color {
    Theme::current().picked
}
/// Lighter heading band at the top of a framed table (theme.json `heading`).
pub(super) fn heading() -> Color {
    Theme::current().heading
}
/// Behind an open pull-down's entries: the pull-down art is translucent, so the list needs a solid backing to read over what it covers.
pub(super) const LIST_BACKING: Color = Color::srgb(0.03, 0.12, 0.3);
/// Resting / hover fill of a list cell (the retail SortList slot art carries decoration that only fits full-width rows).
const CELL: Color = Color::srgba(0.0, 0.05, 0.2, 0.7);
const CELL_ON: Color = Color::srgba(0.35, 0.28, 0.05, 0.75);
const CELL_HOVER: Color = Color::srgba(0.1, 0.25, 0.55, 0.85);

/// Hover/press art: a widget's sprites (by name), swapped between NoCursor / CursorOver / CursorDown by `hover`.
#[derive(Component)]
struct Art(Vec<(Entity, String)>);

/// Which drawn control family a node belongs to (`hover_drawn` restyles it with the cursor).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Family {
    Button,
    Pulldown,
}

/// A control drawn from `theme.json`: its family, whether its resting state is the selected one, the node whose fill follows the cursor
/// and whether that fill is a sphere (round buttons) rather than a vertical gradient.
#[derive(Component, Clone, Copy)]
struct Drawn {
    family: Family,
    selected: bool,
    fill: Entity,
    round: bool,
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

/// The fill of a drawn control in one state: five vertical stops, or for a sphere a radial highlight at the upper left (the lit peak
/// fading to the dark body).
fn gradient(look: &Look, round: bool) -> BackgroundGradient {
    if round {
        let stops = vec![ColorStop::new(look.fill[1], Val::Percent(0.0)), ColorStop::new(look.fill[2], Val::Percent(100.0))];
        let at = UiPosition::new(Vec2::new(-0.2, -0.25), Val::Px(0.0), Val::Px(0.0));
        return BackgroundGradient(vec![Gradient::Radial(RadialGradient::new(at, RadialGradientShape::FarthestCorner, stops))]);
    }
    let at = [0.0, 20.0, 72.0, 92.0, 100.0];
    BackgroundGradient::from(LinearGradient::to_bottom(look.fill.iter().zip(at).map(|(c, p)| ColorStop::new(*c, Val::Percent(p))).collect()))
}

/// Restyle drawn buttons and pull-downs when the cursor enters, presses or leaves them.
fn hover_drawn(q: Query<(&Interaction, &Drawn), Changed<Interaction>>, mut fills: Query<&mut BackgroundGradient>) {
    let theme = Theme::current();
    for (i, drawn) in &q {
        let look = looks(theme, drawn.family).get(state_of(*i, drawn.selected));
        if let Ok(mut fill) = fills.get_mut(drawn.fill) {
            *fill = gradient(look, drawn.round);
        }
    }
}

/// The fill a cell returns to when the cursor leaves it.
#[derive(Component)]
pub(super) struct Rest(pub(super) Color);

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (hover, hover_drawn).after(RunButtons).run_if(menu_active)).add_systems(Update, fit_cover.after(crate::menu::BuildMenu));
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
        for (e, name) in art.map(|a| a.0.as_slice()).unwrap_or(&[]) {
            let Ok((mut img, mut node)) = imgs.get_mut(*e) else { continue };
            if let Some(art) = sprite_in(&assets, MENU_SKIN, name, state) {
                art.show(&mut img, &mut node, scale);
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
    /// Like `text`, but the right end of the text sits at `x + w` (price columns).
    pub(super) fn text_end(&self, c: &mut ChildSpawnerCommands, font: &str, s: &str, x: f32, w: f32, cy: f32, tint: Color) {
        let Some(f) = self.fonts.0.get(font).or_else(|| self.fonts.0.get("BlackChancery22")) else { return };
        let mut n = self.node(x, cy - f.height / 2.0, w, f.height);
        n.align_items = AlignItems::Center;
        n.justify_content = JustifyContent::FlexEnd;
        c.spawn(n).with_children(|c| {
            c.spawn((f.text(s, self.scale, tint), TextLayout::no_wrap(), Node { flex_shrink: 0.0, ..default() }));
        });
    }
    /// A drawn text button over `[l, t, r, b]`: a pill in a gold tube rim whose fill follows the cursor (theme.json `button`).
    pub(super) fn text_button(&self, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], label: &str, font: &str, action: impl Bundle) {
        let (w, h) = (r - l, b - t);
        let (root, fill) = self.pill(c, Family::Button, State::Idle, [l, t, w, h], false);
        let mut cmds = c.commands();
        // Children sit inside the outline, so the label is shifted back by its width to stay centred on the whole pill.
        cmds.entity(root)
            .insert((Interaction::None, action, Drawn { family: Family::Button, selected: false, fill, round: false }))
            .with_children(|c| self.text(c, font, label, -1.0, w, h / 2.0 - 1.0, true, Color::WHITE));
    }
    /// The idle pill of a text button over `[x, y, w, h]` (no label, not clickable).
    pub(super) fn button_art(&self, c: &mut ChildSpawnerCommands, rect: [f32; 4]) -> Entity {
        self.pill(c, Family::Button, State::Idle, rect, false).0
    }
    /// A pill (or, `round`, a sphere) over `[x, y, w, h]` in a gold tube rim: `(root, fill node)`. A pull-down is one flat node.
    fn pill(&self, c: &mut ChildSpawnerCommands, family: Family, state: State, [x, y, w, h]: [f32; 4], round: bool) -> (Entity, Entity) {
        let looks = looks(self.theme, family);
        let look = looks.get(state);
        let [left, top, width, height, ..] = panel_geometry([x, y, x + w, y + h], PanelKind::Band, self.theme, self.scale);
        if family == Family::Pulldown {
            let radius = crate::panels::snap(6.0, self.scale).min(height / 2.0);
            let node = Node { border_radius: BorderRadius::all(Val::Px(radius)), ..bare(left, top, width, height) };
            let mut root = c.spawn((node, gradient(look, false)));
            let id = root.id();
            root.with_children(|c| edges(c, radius, [true, false, true, false], crate::panels::snap(6.0, self.scale), look.rim[0]));
            return (id, id);
        }
        let grey = (state == State::Disabled).then_some(look.rim);
        tube(self, c, [left, top, width, height], width.min(height) / 2.0, looks.rim_width, grey, true, |c, radius, _| {
            c.spawn((Node { border_radius: BorderRadius::all(Val::Px(radius)), ..fill_box() }, gradient(look, round))).id()
        })
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
    /// Big translucent panel in a gold tube rim, recessed along its inside edge.
    Outer,
    /// Dark list / information box with an inside shadow and a soft teal glow along its edge.
    Inner,
    /// The lighter strip along the bottom of a list box (flat, no rim).
    Band,
    /// A framed window: translucent fill, soft teal glow outside, no hard rim.
    Window,
    /// A small frame in a gold tube rim: a fleet slot, the ship icon.
    Slot,
    /// A thinner such frame for the stat bars.
    Stat,
}

/// Physical-px geometry of a panel: `(left, top, width, height, border, radius)`, every edge on a whole pixel so the rim stays crisp
/// at fractional scales (the right and bottom edges are rounded, so abutting panels share them). `border` is the single line of a
/// box or window; the tube rims of `Outer`, `Slot` and `Stat` are built from `tube_layers`.
pub(super) fn panel_geometry([l, t, r, b]: [f32; 4], kind: PanelKind, theme: &Theme, scale: f32) -> [f32; 6] {
    use crate::panels::snap;
    let (border, radius) = match kind {
        PanelKind::Outer => (theme.rim_width, theme.panel_radius),
        PanelKind::Inner => (theme.edge_width, theme.box_radius),
        PanelKind::Band => (0.0, 0.0),
        PanelKind::Slot | PanelKind::Stat => (0.0, theme.slot_radius),
        PanelKind::Window => (theme.edge_width, theme.box_radius),
    };
    let border = if border > 0.0 { snap(border, scale).max(1.0) } else { 0.0 };
    let (w, h) = (snap(r, scale) - snap(l, scale), snap(b, scale) - snap(t, scale));
    [snap(l, scale), snap(t, scale), w, h, border, snap(radius, scale).min(w.min(h) / 2.0).floor()]
}

/// One layer of a tube rim: its width in whole physical px and its colours.
struct Layer {
    w: f32,
    colour: BorderColor,
}

/// The layers of a tube rim, outside in: a dark outline (1 reference px), the gold (`gold` px; its middle line is a bright core) and a
/// dark keyline (1). Widths are the differences of the snapped running total, so the whole rim is as thick as `gold + 2` snapped and
/// every layer is at least one whole pixel. `grey` replaces the gold of a disabled control (top, other sides).
fn tube_layers(theme: &Theme, gold: f32, scale: f32, grey: Option<[Color; 2]>) -> Vec<Layer> {
    let side = match grey {
        Some([top, rest]) => BorderColor { top, left: rest, right: rest, bottom: rest },
        None => BorderColor { top: theme.rim_light, left: theme.rim, right: theme.rim, bottom: theme.rim },
    };
    let core = match grey {
        Some([top, _]) => BorderColor::all(top),
        None => BorderColor { top: theme.rim_core, left: theme.rim_light, right: theme.rim_light, bottom: theme.rim_light },
    };
    let mut parts = vec![(1.0, BorderColor::all(theme.rim_outline))];
    if gold >= 2.0 {
        parts.push((1.0, side));
    }
    parts.push((1.0, core));
    if gold >= 3.0 {
        parts.push((gold - 2.0, side));
    }
    parts.push((1.0, BorderColor::all(theme.rim_keyline)));
    let (mut total, mut drawn) = (0.0, 0.0);
    parts
        .into_iter()
        .map(|(w, colour)| {
            total += w;
            let px = (crate::panels::snap(total, scale) - drawn).max(1.0);
            drawn += px;
            Layer { w: px, colour }
        })
        .collect()
}

/// A fill node covering its parent's whole padding box.
fn fill_box() -> Node {
    Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }
}

/// Nest the remaining `layers` (each a bordered node filling the one outside it), then let `inner` draw the content box; returns what
/// `inner` returned.
fn nest(c: &mut ChildSpawnerCommands, layers: &[Layer], radius: f32, size: Vec2, inner: &mut dyn FnMut(&mut ChildSpawnerCommands, f32, Vec2) -> Entity) -> Entity {
    let Some((layer, rest)) = layers.split_first() else { return inner(c, radius.max(0.0), size) };
    let node = Node { border: UiRect::all(Val::Px(layer.w)), border_radius: BorderRadius::all(Val::Px(radius.max(0.0))), ..fill_box() };
    let mut found = Entity::PLACEHOLDER;
    c.spawn((node, layer.colour)).with_children(|c| found = nest(c, rest, radius - layer.w, size - Vec2::splat(2.0 * layer.w), inner));
    found
}

/// A shape over physical `[left, top, width, height]` in a gold tube rim (see `tube_layers`) with a hard dark band under its bottom edge
/// and a soft shadow; `inner` draws the content box (given its corner radius and size in physical px) and returns its fill node.
/// Returns `(root, fill)`.
fn tube(
    ctx: &Ctx,
    c: &mut ChildSpawnerCommands,
    [left, top, width, height]: [f32; 4],
    radius: f32,
    gold: f32,
    grey: Option<[Color; 2]>,
    soft: bool,
    mut inner: impl FnMut(&mut ChildSpawnerCommands, f32, Vec2) -> Entity,
) -> (Entity, Entity) {
    let theme = ctx.theme;
    let layers = tube_layers(theme, gold, ctx.scale, grey);
    let band =
        ShadowStyle { color: theme.rim_shadow, x_offset: Val::Px(0.0), y_offset: Val::Px((2.0 * ctx.scale).round()), spread_radius: Val::Px(0.0), blur_radius: Val::Px(0.0) };
    let mut shadow = vec![band];
    if soft {
        shadow.push(ShadowStyle {
            color: Color::BLACK.with_alpha(theme.shadow_alpha),
            x_offset: Val::Px(0.0),
            y_offset: Val::Px((2.0 * ctx.scale).round()),
            spread_radius: Val::Px(0.0),
            blur_radius: Val::Px((theme.shadow_blur * ctx.scale).round()),
        });
    }
    let first = &layers[0];
    let node = Node { border: UiRect::all(Val::Px(first.w)), border_radius: BorderRadius::all(Val::Px(radius)), ..bare(left, top, width, height) };
    let mut root = c.spawn((node, first.colour, BoxShadow(shadow)));
    let id = root.id();
    let mut fill = Entity::PLACEHOLDER;
    root.with_children(|c| fill = nest(c, &layers[1..], radius - first.w, Vec2::new(width, height) - Vec2::splat(2.0 * first.w), &mut inner));
    (id, fill)
}

/// A node over its parent's padding box shading `sides` (top, right, bottom, left) from `colour` fading out over `size` physical px:
/// an inside shadow or glow. One gradient per side stacked on a single node.
fn edges(c: &mut ChildSpawnerCommands, radius: f32, sides: [bool; 4], size: f32, colour: Color) {
    let stops = || vec![ColorStop::new(colour, Val::Px(0.0)), ColorStop::new(colour.with_alpha(0.0), Val::Px(size))];
    let angles = [LinearGradient::TO_BOTTOM, LinearGradient::TO_LEFT, LinearGradient::TO_TOP, LinearGradient::TO_RIGHT];
    let all = sides.iter().zip(angles).filter(|(on, _)| **on).map(|(_, a)| Gradient::Linear(LinearGradient::new(a, stops()))).collect();
    c.spawn((Node { border_radius: BorderRadius::all(Val::Px(radius)), ..fill_box() }, BackgroundGradient(all)));
}

/// A drawn panel over the retail rect `[l, t, r, b]` (reference units), all UI node features (no art): `Outer`, `Slot` and `Stat` in a
/// gold tube rim with a recessed fill, `Inner` a dark box with an inside shadow and teal glow, `Window` a translucent glow-edged
/// window, `Band` a flat strip. Returns the root node.
pub(super) fn panel(ctx: &Ctx, c: &mut ChildSpawnerCommands, rect: [f32; 4], kind: PanelKind) -> Entity {
    let theme = ctx.theme;
    let scale = ctx.scale;
    let [left, top, width, height, border, radius] = panel_geometry(rect, kind, theme, scale);
    let snap = |v: f32| crate::panels::snap(v, scale);
    let node = Node { border: UiRect::all(Val::Px(border)), border_radius: BorderRadius::all(Val::Px(radius)), ..bare(left, top, width, height) };
    let shade = |k: f32| {
        let s = theme.panel_fill.to_srgba();
        Color::srgba((s.red * k).min(1.0), (s.green * k).min(1.0), (s.blue * k).min(1.0), s.alpha)
    };
    match kind {
        PanelKind::Outer => {
            let fill = LinearGradient::to_bottom(vec![ColorStop::new(shade(1.2), Val::Percent(0.0)), ColorStop::new(shade(0.85), Val::Percent(100.0))]);
            tube(ctx, c, [left, top, width, height], radius, theme.rim_width, None, true, |c, r, _| {
                let mut e = c.spawn((Node { border_radius: BorderRadius::all(Val::Px(r)), ..fill_box() }, BackgroundGradient::from(fill.clone())));
                e.with_children(|c| edges(c, r, [true; 4], snap(theme.inset_size), theme.inset_shadow));
                e.id()
            })
            .0
        }
        PanelKind::Slot | PanelKind::Stat => {
            let gold = if kind == PanelKind::Stat { 1.0 } else { 2.0 };
            tube(ctx, c, [left, top, width, height], radius, gold, None, false, |c, r, _| {
                let mut e = c.spawn((Node { border_radius: BorderRadius::all(Val::Px(r)), ..fill_box() }, BackgroundColor(theme.box_fill)));
                e.with_children(|c| edges(c, r, [true; 4], snap(theme.box_inset_size), theme.inset_shadow));
                e.id()
            })
            .0
        }
        PanelKind::Inner => {
            let mut e = c.spawn((node, BackgroundColor(theme.box_fill), BorderColor::all(theme.edge)));
            let inner = (radius - border).max(0.0);
            e.with_children(|c| {
                edges(c, inner, [true; 4], snap(theme.box_inset_size), theme.inset_shadow);
                edges(c, inner, [true; 4], snap(theme.glow_size), theme.glow);
            });
            e.id()
        }
        PanelKind::Window => c
            .spawn((
                node,
                BackgroundColor(theme.window_fill),
                BorderColor::all(theme.window_rim.with_alpha(0.5)),
                BoxShadow::new(theme.window_rim.with_alpha(0.55), Val::Px(0.0), Val::Px(0.0), Val::Px(0.0), Val::Px(snap(7.0))),
            ))
            .id(),
        PanelKind::Band => c.spawn((node, BackgroundColor(theme.band_fill))).id(),
    }
}

/// The scenery and panels of the Ships, Crew and Arms screens: `menu/gamelobby.png` (the scene without the panels) over the whole
/// window, then the engine-drawn panels at the rects measured on the retail `menu/shipscreen.png` (PROVISIONAL, 800x600 part): the
/// fleet list panel, the ship information panel, the slot bar panel, the list box with its band and the information box.
pub(super) fn ship_screen_backdrop(ctx: &Ctx, c: &mut ChildSpawnerCommands) {
    ctx.backdrop(c, "menu/gamelobby.png");
    for rect in [[23.0, 35.0, 459.0, 473.0], [482.0, 35.0, 776.0, 508.0], [25.0, 478.0, 459.0, 562.0]] {
        panel(ctx, c, rect, PanelKind::Outer);
    }
    panel(ctx, c, [40.0, 89.0, 444.0, 425.0], PanelKind::Inner);
    panel(ctx, c, [42.0, 375.0, 442.0, 423.0], PanelKind::Band);
    panel(ctx, c, [496.0, 117.0, 766.0, 283.0], PanelKind::Inner);
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
    ctx.pill(c, Family::Button, State::Disabled, [l, t, r - l, b - t], false);
    ctx.text(c, font, label, l, r - l, (t + b) / 2.0, true, Color::srgb(0.55, 0.55, 0.55));
}

/// The current tab of a Ships / Crew / Arms row: the green `FrontEnd_Texts_Over_Button_Selected_*` button, not clickable.
pub(super) fn selected_tab(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], label: &str, font: &str) {
    ctx.pill(c, Family::Button, State::Selected, [l, t, r - l, b - t], false);
    ctx.text(c, font, label, l, r - l, (t + b) / 2.0, true, Color::WHITE);
}

/// A round retail arrow button (`FrontEnd_Prev_Button`, `FrontEnd_Next_Button`, ...).
pub(super) fn arrow(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], name: &str, action: impl Bundle) {
    let forward = name.contains("Next") || name.contains("Right");
    let mut root = c.spawn((Interaction::None, action, ArrowButton { forward }, ctx.node(l, t, r - l, b - t)));
    let mut parts = vec![];
    root.with_children(|c| parts.extend(piece(ctx, c, name, [0.0, 0.0, r - l, b - t]).map(|e| (e, name.to_string()))));
    root.insert(Art(parts));
}

/// A framed window (the map list, the Game Summary table, the Arms help box, the lobby player list): a drawn translucent panel with a
/// teal rim and glow (theme.json `window_*`). The ornate frames stay bitmap, being pictorial rather than a shape: the in-game menu window
/// (`menu/fe_ingame_window.png`, `menu.rs` `ingame_window`) and the dialog frame (`menu/frontenddialogscreen.png`, `dialog_frame`).
pub(super) fn stretched_window(ctx: &Ctx, c: &mut ChildSpawnerCommands, rect: [f32; 4]) {
    panel(ctx, c, rect, PanelKind::Window);
}

/// A vertical scrollbar for a list of `total` rows of which `visible` show from row `first`: a light translucent track in a gold rim and a
/// cylinder-glossed pill thumb that never gets shorter than the track is wide and stays inside it; pressing and dragging the track moves
/// it (`menu::scroll_drag`, the same everywhere).
pub(super) fn scrollbar(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], kind: ScrollKind, total: usize, visible: usize, first: usize) {
    let h = b - t;
    let (len, top) = thumb_span(h, total, visible, first, r - l);
    track(ctx, c, [l, t, r, b]);
    thumb(ctx, c, [l + 4.0, t + top, r - l - 8.0, len]);
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

/// A scrollbar or slider track: a light translucent blue pill, lighter toward the bottom, in a gold tube rim.
fn track(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4]) {
    let [left, top, width, height, ..] = panel_geometry([l, t, r, b], PanelKind::Band, ctx.theme, ctx.scale);
    let [a, z] = ctx.theme.track_fill;
    tube(ctx, c, [left, top, width, height], width.min(height) / 2.0, 2.0, None, false, |c, r, _| {
        let fill = LinearGradient::to_bottom(vec![ColorStop::new(a, Val::Percent(0.0)), ColorStop::new(z, Val::Percent(100.0))]);
        c.spawn((Node { border_radius: BorderRadius::all(Val::Px(r)), ..fill_box() }, BackgroundGradient::from(fill))).id()
    });
}

/// A scrollbar thumb or slider knob: a pill in a thin gold rim with a left-to-right cylinder gloss.
fn thumb(ctx: &Ctx, c: &mut ChildSpawnerCommands, [x, y, w, h]: [f32; 4]) {
    let [left, top, width, height, ..] = panel_geometry([x, y, x + w, y + h], PanelKind::Band, ctx.theme, ctx.scale);
    let [a, z] = ctx.theme.thumb;
    tube(ctx, c, [left, top, width, height], width.min(height) / 2.0, 1.0, None, false, |c, r, _| {
        let fill = LinearGradient::to_right(vec![ColorStop::new(a, Val::Percent(0.0)), ColorStop::new(z, Val::Percent(100.0))]);
        c.spawn((Node { border_radius: BorderRadius::all(Val::Px(r)), ..fill_box() }, BackgroundGradient::from(fill))).id()
    });
}

/// The Create Skirmish slider: a rounded track between two round step buttons (33 px, each with a chevron) and a pill knob that follows
/// `fraction` (dragging the track moves it, see `menu::slider_drag`). The 134 px hit box between the buttons is the retail track.
pub(super) fn slider(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], fraction: f32, dec: impl Bundle, inc: impl Bundle, kind: SliderTrack) {
    const CAP: f32 = 33.0;
    let cy = (t + b) / 2.0;
    let top = cy - 16.5;
    track(ctx, c, [l + CAP / 2.0, cy - 8.0, r - CAP / 2.0, cy + 8.0]);
    step_button(ctx, c, true, [l, top, CAP, 33.0], Some((dec, ArrowButton { forward: false })));
    step_button(ctx, c, false, [r - CAP, top, CAP, 33.0], Some((inc, ArrowButton { forward: true })));
    let track_w = r - l - 2.0 * CAP;
    let mut root = c.spawn((Interaction::None, kind, RelativeCursorPosition::default(), ctx.node(l + CAP, top, track_w, 33.0)));
    root.with_children(|c| thumb(ctx, c, [fraction * (track_w - SLIDER_KNOB), 5.0, SLIDER_KNOB, 23.0]));
}

/// A round step button over `[x, y, w, h]`: a sphere in a gold ring with a chevron pointing left or right; clickable with an action,
/// greyed and inert without one.
fn step_button(ctx: &Ctx, c: &mut ChildSpawnerCommands, left: bool, rect: [f32; 4], action: Option<impl Bundle>) {
    let state = if action.is_some() { State::Idle } else { State::Disabled };
    let (root, fill) = ctx.pill(c, Family::Button, state, rect, true);
    let [_, _, w, h, ..] = panel_geometry([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]], PanelKind::Band, ctx.theme, ctx.scale);
    let outline = tube_layers(ctx.theme, ctx.theme.button.rim_width, ctx.scale, None)[0].w;
    let (cx, cy) = ((w - 2.0 * outline) / 2.0, (h - 2.0 * outline) / 2.0);
    let tint = if state == State::Disabled { Color::srgb(0.55, 0.55, 0.55) } else { Color::WHITE };
    let mut cmds = c.commands();
    let mut e = cmds.entity(root);
    if let Some(a) = action {
        e.insert((Interaction::None, Drawn { family: Family::Button, selected: false, fill, round: true }, a));
    }
    e.with_children(|c| chevron(ctx, c, left, rect[2].min(rect[3]), (cx, cy), tint));
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
pub(super) fn list_row(ctx: &Ctx, c: &mut ChildSpawnerCommands, rect: [f32; 4], label: &str, picked: bool, action: impl Bundle) {
    list_row_cost(ctx, c, rect, label, (None, None), picked, false, action);
}

/// A list row with a price column: `cost` is right-aligned, `grey` dims the whole row (a hull the points left cannot pay for).
#[allow(clippy::too_many_arguments)]
pub(super) fn list_row_cost(
    ctx: &Ctx,
    c: &mut ChildSpawnerCommands,
    [x, y, w, h]: [f32; 4],
    label: &str,
    (value, cost): (Option<&str>, Option<&str>),
    picked: bool,
    grey: bool,
    action: impl Bundle,
) {
    let fill = BackgroundColor(if picked { self::picked() } else { Color::NONE });
    let tint = if grey { Color::srgba(1.0, 1.0, 1.0, 0.4) } else { Color::WHITE };
    c.spawn((Interaction::None, action, fill, ctx.node(x, y, w, h))).with_children(|c| {
        ctx.text(c, BODY_FONT, label, 20.0, w - 20.0, h / 2.0, false, tint);
        if let Some(value) = value {
            ctx.text_end(c, BODY_FONT, value, 0.0, w - 52.0, h / 2.0, tint);
        }
        if let Some(cost) = cost {
            ctx.text_end(c, BODY_FONT, cost, 0.0, w - 12.0, h / 2.0, tint);
        }
    });
}

/// A drawn check box over `[x, y, w, h]`: a small rounded square with a gold rim and, when `on`, a green tick with a white core.
pub(super) fn check_box(ctx: &Ctx, c: &mut ChildSpawnerCommands, [x, y, w, h]: [f32; 4], on: bool) {
    let theme = ctx.theme;
    let [left, top, width, height, border, _] = panel_geometry([x, y, x + w, y + h], PanelKind::Inner, theme, ctx.scale);
    let border = border.max(crate::panels::snap(1.0, ctx.scale));
    let node = Node { border: UiRect::all(Val::Px(border)), border_radius: BorderRadius::all(Val::Px(crate::panels::snap(3.0, ctx.scale))), ..bare(left, top, width, height) };
    c.spawn((node, BackgroundColor(theme.box_fill), BorderColor::all(theme.rim_light))).with_children(|c| {
        if !on {
            return;
        }
        // Bars are placed by their centre inside the box, then turned about it: the short stroke runs down-right, the long one up-right.
        // The green stroke is drawn first, a thinner white one over its middle.
        for (thick, tint) in [(2.6, theme.tick), (1.0, Color::WHITE)] {
            for (len, cx, cy, deg) in [(5.0, 4.5, 8.5, 45.0), (9.0, 8.2, 6.5, -45.0)] {
                let (bw, bh) = ((len - 2.6 + thick) * ctx.scale, thick * ctx.scale);
                let n = Node { border_radius: BorderRadius::all(Val::Px(bh / 2.0)), ..bare(cx * ctx.scale - bw / 2.0, cy * ctx.scale - bh / 2.0, bw, bh) };
                c.spawn((n, BackgroundColor(tint), UiTransform::from_rotation(Rot2::degrees(deg))));
            }
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
    let state = state_of(Interaction::None, selected);
    let (root, fill) = ctx.pill(c, Family::Pulldown, state, [l, t, r - l, b - t], false);
    let mut cmds = c.commands();
    let mut e = cmds.entity(root);
    e.insert((Interaction::None, Drawn { family: Family::Pulldown, selected, fill, round: false }));
    if let Some(a) = action {
        e.insert(a);
    }
    e.with_children(|c| {
        let (w, h) = (r - l, b - t);
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
    #[ignore = "needs imported assets"]
    fn every_strip_the_battle_hud_draws_is_a_slice_group() {
        // The groups come from data/hud/slices.json (the front-end ones are drawn from theme.json now): explicit caps win, else the
        // widths of the end pieces.
        for group in ["tooltip", "ribbon"] {
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
            for kind in [PanelKind::Outer, PanelKind::Inner, PanelKind::Band, PanelKind::Slot, PanelKind::Stat, PanelKind::Window] {
                let g = panel_geometry([23.3, 38.7, 459.4, 471.1], kind, &theme, scale);
                assert!(g.iter().all(|v| v.fract() == 0.0), "{kind:?} at {scale}: {g:?}");
            }
        }
        // 1080p canvas scale 1.8: a 4 px rim is 7 physical px, the box line 2, a band has no border.
        assert_eq!(panel_geometry([0.0, 0.0, 10.0, 10.0], PanelKind::Outer, &theme, 1.8)[4], 7.0);
        assert_eq!(panel_geometry([0.0, 0.0, 10.0, 10.0], PanelKind::Inner, &theme, 1.8)[4], 2.0);
        assert_eq!(panel_geometry([0.0, 0.0, 10.0, 10.0], PanelKind::Band, &theme, 1.8)[4], 0.0);
        // Abutting panels share an edge: widths sum to the span.
        let a = panel_geometry([0.0, 0.0, 10.5, 10.0], PanelKind::Band, &theme, 1.125);
        let b = panel_geometry([10.5, 0.0, 20.0, 10.0], PanelKind::Band, &theme, 1.125);
        assert_eq!(a[0] + a[2], b[0]);
    }

    #[test]
    fn tube_rim_layers_land_on_whole_pixels() {
        let theme = Theme::default();
        for scale in [1.0, 1.2, 1.6, 2.4] {
            for gold in [1.0, 2.0, 3.0, 4.0] {
                let layers = tube_layers(&theme, gold, scale, None);
                // outline, gold (outer, core, inner as the width allows), keyline
                let count = 3 + usize::from(gold >= 2.0) + usize::from(gold >= 3.0);
                assert_eq!(layers.len(), count, "gold {gold}");
                assert!(layers.iter().all(|l| l.w >= 1.0 && l.w.fract() == 0.0), "{gold} at {scale}");
                // The whole rim is as thick as its reference width snapped: the layers add up exactly.
                let total: f32 = layers.iter().map(|l| l.w).sum();
                assert_eq!(total, (((gold + 2.0) * scale).round()).max(count as f32), "gold {gold} at {scale}");
            }
        }
        // A disabled control's gold is grey, its outline and keyline are not.
        let grey = tube_layers(&theme, 3.0, 1.0, Some([Color::WHITE, Color::BLACK]));
        assert_eq!(grey[0].colour, BorderColor::all(theme.rim_outline));
        assert_eq!(grey[1].colour.left, Color::BLACK);
        assert_eq!(grey.last().unwrap().colour, BorderColor::all(theme.rim_keyline));
    }

    #[test]
    fn wheel_step_stays_in_range() {
        assert_eq!(wheel_step(0, 1.0, 5), 0);
        assert_eq!(wheel_step(5, -1.0, 5), 5);
        assert_eq!(wheel_step(2, -1.0, 5), 3);
    }
}
