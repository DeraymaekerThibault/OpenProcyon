//! Classic (retail) in-game HUD: the main bar, the compass with its speed dial, the talking head and the weapon panel, as trees
//! of Bevy UI nodes. Behaviour (clicks, gauge values, contacts, status icon, talking-head state) is in the shared `panels.rs`.
//!
//! Sprites come from the retail `hud.hdt` (`hud/hud.json`) in the art set of the player's faction (`HudData::skin`). Sizes are in the
//! 640x480 reference pixels of that data, multiplied by `window height / REF_H` when the tree is built; the tree is rebuilt when the ship,
//! faction or window size changes. Layout is done by the UI engine (flex rows and anchored containers), so the bar needs no per-faction
//! numbers: it is the list of parts below, each as wide and tall as its sprite. Parts share their top edge (the data's gauge offsets are
//! "from MainBar TopLeft", and every part's art starts its frame on the same row); rects differ by 1-2 transparent or border rows at the
//! bottom, so the bar is docked with its shortest part's bottom on the screen edge and taller parts run off-screen.
#![allow(deprecated)] // shortcut: bevy 0.20 Interaction (deprecated for picking Hovered/Pressed); port with menu.rs when it is removed
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use std::collections::HashMap;

use crate::combat::{self, GunStates, Health};
use crate::panels::{
    Contact, DockingStatus, Gauge, GunButton, HeadState, HudRoot, HudSet, MAX_CONTACTS, PanicButton, PauseButton, REF_H, Radar, RadarButton, ShipIconButton, Solid, ToggleButton,
    WeaponPanel, gauge_value, px,
};
use crate::sprites::{HudData, IDLE, inset, offset, sheet_scale, sprite_area, sprite_of, sprite_trimmed, src};
use crate::{Library, PlayerShip, Ship};

/// Ordered bar parts and optional decorative backdrops, editable through the asset overlay.
#[derive(Resource)]
struct BarParts(Vec<(String, Option<String>)>);
impl BarParts {
    fn load() -> Self {
        let path = crate::assets_dir().join("data/hud/bar.json");
        let value = crate::data::read_json(&path);
        let parts = value
            .as_array()
            .expect("HUD bar must be an array")
            .iter()
            .map(|part| {
                let sprite = part["sprite"].as_str().filter(|s| !s.is_empty()).expect("HUD bar part needs a sprite").to_string();
                let backdrop = part.get("backdrop").map(|v| v.as_str().expect("HUD backdrop must be a sprite name").to_string());
                (sprite, backdrop)
            })
            .collect::<Vec<_>>();
        assert!(!parts.is_empty(), "HUD bar must have parts");
        Self(parts)
    }
}

/// Running / paused art of the bar's PAUSE button.
#[derive(Component)]
struct PauseArt {
    running: ImageNode,
    paused: ImageNode,
}
#[derive(Component)]
struct Dial;
/// Bottom-right talking-head device (retail `TalkingHead_*`): speaker portrait behind an iris that opens while a campaign voice line or
/// the helmsman speaks.
#[derive(Component, Clone, Copy, PartialEq)]
enum Head {
    Face,
    Iris,
    Light,
    Voice,
}
/// The voice box cycles while speaking: `Talking Head Voice Box - Time To Next Frame` 0.1 s.
const VOICE_FPS: f32 = 10.0;
#[derive(Resource)]
struct HeadFrames {
    /// `Portal_Closed`, `Portal_ClosedToOpen0..9`, `Portal_Open`.
    iris: Vec<(Handle<Image>, Rect)>,
    /// `VoiceBox_Start`, `Frame0`, `Frame1`, `Stop`, then `Off`.
    voice: Vec<(Handle<Image>, Rect)>,
}
/// Iris frame for open progress 0..=1.
fn iris_frame(progress: f32, frames: usize) -> usize {
    (progress.clamp(0.0, 1.0) * (frames - 1) as f32).round() as usize
}
#[derive(Component)]
struct Sweep;
/// A gauge fill that grows from the bottom: `full` is its sprite height and `top` its y (reference px, inside the parent) when full.
#[derive(Component)]
struct Fill {
    gauge: Gauge,
    full: f32,
    top: f32,
    base: Rect,
    k: f32,
}
/// Weapon-panel slot of a gun bank: its status frame, or the gun's icon when `icon`.
#[derive(Component)]
struct GunSlot {
    bank: usize,
    icon: bool,
}

/// Images that change at runtime, as (handle, source rect).
#[derive(Resource)]
struct Frames {
    gun: Vec<(Handle<Image>, Rect)>,
    dial: Vec<(Handle<Image>, Rect)>,
    sweep: Vec<(Handle<Image>, Rect)>,
    icons: HashMap<String, (Handle<Image>, Rect)>,
}
/// Idle / hover / pressed art of a button, swapped by its own `Interaction`.
#[derive(Component)]
struct States([(Handle<Image>, Rect); 3]);
/// The radar disk's idle/hover/pressed art (the hover and pressed art live on another sheet).
#[derive(Component)]
struct RadarFace([(Handle<Image>, Rect); 3]);

pub fn plugin(app: &mut App) {
    app.insert_resource(BarParts::load()).add_systems(
        Update,
        (
            build.before(HudSet).after(crate::sprites::choose_skin).run_if(in_state(crate::menu::Screen::Battle)),
            (overlay, update_dial, update_sweep, radar_face, button_states, update_gauges, update_guns, head_art, pause_art, fleet_art, name_plate).after(HudSet),
        ),
    );
}

#[derive(Component)]
struct Overlay;

/// The mission rules' full-screen texture (end credits, tutorial pages), centred at native size. PROVISIONAL: retail placement unknown.
fn overlay(mut commands: Commands, assets: Res<AssetServer>, mission: Option<Res<crate::campaign::Mission>>, old: Query<(Entity, &Name), With<Overlay>>) {
    let want = mission.as_ref().and_then(|m| m.script.overlay.as_deref());
    if old.iter().next().map(|(_, n)| n.as_str()) == want {
        return;
    }
    for (e, _) in &old {
        commands.entity(e).despawn();
    }
    if let Some(n) = want {
        commands
            .spawn((
                Overlay,
                Name::new(n.to_string()),
                crate::Scenario,
                GlobalZIndex(40),
                FocusPolicy::Pass,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_children(|p| {
                p.spawn(ImageNode::new(assets.load(format!("hud/overlay/{n}.png"))));
            });
    }
}

/// What the build needs to turn a sprite name into a node.
struct Ctx<'a> {
    assets: &'a AssetServer,
    hud: &'a HudData,
    s: f32,
    /// Bar parts use their trimmed rects (non-Navy sets tile exactly; Navy parts share an edge pixel).
    trim: bool,
}
impl Ctx<'_> {
    /// Image node and reference size of a sprite. `tight` keeps the full rect (parts that butt against each other); otherwise the
    /// rect is inset against sheet bleed.
    fn sprite(&self, name: &str, tight: bool) -> Option<(ImageNode, Vec2)> {
        let (sheet, r) = if tight && self.trim { sprite_trimmed(self.hud, name) } else { sprite_of(self.hud, name) }?;
        let standalone = format!("bar_{}_{}", self.hud.skin.to_lowercase(), name.to_lowercase());
        let path = format!("hud/tex/{standalone}.png");
        let node = if tight && sheet_scale(&sheet) == 1.0 && crate::assets_dir().join(&path).exists() {
            // Standalone bar sprites cannot sample their atlas neighbours at fractional UI scales.
            ImageNode::new(self.assets.load(path))
        } else {
            let mut image = ImageNode::new(self.assets.load(format!("hud/tex/{sheet}.png")));
            image.rect = Some(if tight && !self.trim { src(&sheet, r) } else { inset(&sheet, r) });
            image
        };
        Some((node, r.size()))
    }
    /// The NoCursor / CursorOver / CursorDown art of a sprite (a missing state uses the idle art).
    fn states(&self, name: &str) -> Option<States> {
        let idle = self.frame(name)?;
        let state = |k: &str| {
            sprite_area(self.hud, name, &format!("TextureArea - {k}"))
                .map_or(idle.clone(), |(sheet, r)| (self.assets.load::<Image>(format!("hud/tex/{sheet}.png")), inset(&sheet, r)))
        };
        Some(States([idle.clone(), state("CursorOver"), state("CursorDown")]))
    }
    /// A button: its decorative arm (`detail`, drawn first) and the button sprite at `at` inside it, with hover/press art.
    fn toggle(&self, commands: &mut Commands, parent: Entity, detail: &str, detail_at: Vec2, button: &str, at: Vec2, marker: impl Bundle) {
        if let Some((img, sz)) = self.sprite(detail, false) {
            commands.spawn((img, self.abs(detail_at.x, detail_at.y, sz), ChildOf(parent), Solid, Interaction::None, FocusPolicy::Block));
        }
        if let (Some((img, sz)), Some(states)) = (self.sprite(button, false), self.states(button)) {
            let at = detail_at + at;
            commands.spawn((marker, img, states, self.abs(at.x, at.y, sz), ChildOf(parent), Solid, Interaction::None, FocusPolicy::Block));
        }
    }
    fn frame(&self, name: &str) -> Option<(Handle<Image>, Rect)> {
        sprite_of(self.hud, name).map(|(sheet, r)| (self.assets.load::<Image>(format!("hud/tex/{sheet}.png")), inset(&sheet, r)))
    }
    fn size_of(&self, name: &str) -> Vec2 {
        sprite_of(self.hud, name).map_or(Vec2::ZERO, |(_, r)| r.size())
    }
    /// Size of a bar part as drawn.
    fn bar_size(&self, name: &str) -> Vec2 {
        if self.trim { sprite_trimmed(self.hud, name) } else { sprite_of(self.hud, name) }.map_or(Vec2::ZERO, |(_, r)| r.size())
    }
    /// Absolutely positioned node at reference position `(x, y)`.
    fn abs(&self, x: f32, y: f32, size: Vec2) -> Node {
        Node { position_type: PositionType::Absolute, left: px(x, self.s), top: px(y, self.s), width: px(size.x, self.s), height: px(size.y, self.s), ..default() }
    }
}

enum Item {
    Part(usize),
    /// A strip of wooden tiles (sprite name) with the listed parts on top of it.
    Wing(String, Vec<usize>),
}

/// (Re)builds the whole HUD tree.
fn build(
    mut commands: Commands,
    assets: Res<AssetServer>,
    hud: Res<HudData>,
    bar: Res<BarParts>,
    lib: Res<Library>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    fonts: Option<Res<crate::sprites::Fonts>>,
    player: Single<(&Ship, &GunStates), With<PlayerShip>>,
    old: Query<Entity, With<HudRoot>>,
    mut last: Local<Option<(usize, &'static str, u32, Vec<String>)>>,
) {
    let s = window.height() / REF_H;
    let (player, guns) = *player;
    let key = (player.class, hud.skin, window.height() as u32, guns.names());
    if last.as_ref() == Some(&key) && !old.is_empty() {
        return;
    }
    *last = Some(key);
    for e in &old {
        commands.entity(e).despawn();
    }
    let class = &lib.classes[player.class];
    let has_wings = sprite_of(&hud, "MainBar_LeftStretchedDetail").is_some();
    let c = Ctx { assets: &assets, hud: &hud, s, trim: !has_wings };
    let solid = || (Interaction::None, FocusPolicy::Block, Solid);

    let root = commands
        .spawn((crate::Scenario, HudRoot, Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, GlobalZIndex(10)))
        .id();

    // ---- main bar: a bottom-docked flex row of parts, each overlapping its neighbour by `overlap` --------------------------
    let parts: Vec<usize> = (0..bar.0.len()).filter(|i| c.bar_size(&bar.0[*i].0).x > 0.0).collect();
    // PROVISIONAL: which bottom rows retail shows is not measured per faction; Navy and Procyon art ends on the same row in every part.
    let floor = parts.iter().map(|i| c.bar_size(&bar.0[*i].0).y).fold(f32::MAX, f32::min);
    // Trimmed atlas edges still need a half-pixel overlap to hide bilinear edge blending at fractional scales.
    let overlap = if has_wings { 1.0 } else { 0.5 };
    let mut items: Vec<Item> = vec![];
    for &i in &parts {
        let strip = bar.0[i].1.as_ref().filter(|name| c.size_of(name).x > 1.0);
        match (strip, items.last_mut()) {
            (Some(name), Some(Item::Wing(n, members))) if n == name => members.push(i),
            (Some(name), _) => items.push(Item::Wing(name.clone(), vec![i])),
            (None, _) => items.push(Item::Part(i)),
        }
    }
    let width = |item: &Item| match item {
        Item::Part(i) => c.bar_size(&bar.0[*i].0).x,
        Item::Wing(_, members) => members.iter().map(|i| c.bar_size(&bar.0[*i].0).x - overlap).sum::<f32>() + overlap,
    };
    let bar_w = items.iter().map(width).sum::<f32>() - overlap * (items.len() as f32 - 1.0);
    commands_window(&mut commands, &c, root, window.width() / s, bar_w, floor);
    speed_plate(&mut commands, &c, fonts.as_deref(), root, (window.width() / s + bar_w) / 2.0);
    {
        let dock = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    height: px(floor, s),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                ChildOf(root),
            ))
            .id();
        let row = commands.spawn((Node { flex_direction: FlexDirection::Row, align_items: AlignItems::FlexStart, ..default() }, ChildOf(dock))).id();
        for (n, item) in items.iter().enumerate() {
            let gap = if n == 0 { UiRect::DEFAULT } else { UiRect::left(px(-overlap, s)) };
            match item {
                Item::Part(i) => spawn_part(&mut commands, &c, class, row, &bar.0[*i].0, Node { margin: gap, ..default() }),
                Item::Wing(strip, members) => {
                    let w = width(item);
                    let h = members.iter().map(|i| c.bar_size(&bar.0[*i].0).y).fold(0.0, f32::max);
                    let wing = commands.spawn((Node { width: px(w, s), height: px(h, s), margin: gap, ..default() }, ChildOf(row))).id();
                    let tile = c.size_of(strip);
                    for k in 0..(w / (tile.x - 1.0)).ceil() as usize {
                        if let Some((img, ts)) = c.sprite(strip, true) {
                            commands.spawn((
                                img,
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: px(k as f32 * (tile.x - 1.0), s),
                                    top: Val::Px(0.0),
                                    width: px(ts.x, s),
                                    height: px(ts.y, s),
                                    ..default()
                                },
                                ChildOf(wing),
                            ));
                        }
                    }
                    let mut x = 0.0;
                    for i in members {
                        let node = Node { position_type: PositionType::Absolute, left: px(x, s), top: Val::Px(0.0), ..default() };
                        spawn_part(&mut commands, &c, class, wing, &bar.0[*i].0, node);
                        x += c.bar_size(&bar.0[*i].0).x - overlap;
                    }
                }
            }
        }
    }

    // ---- compass, radar and speed dial (bottom-left corner) ----------------------------------------------------------
    let overlay = c.size_of("COMPASS_OVERLAY");
    let compass = commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(0.0), bottom: Val::Px(0.0), width: px(overlay.x, s), height: px(overlay.y, s), ..default() },
            ChildOf(root),
        ))
        .id();
    let (bg, bg_pos) = (c.size_of("Compass_Background"), offset(&hud, "COMPASS_WINDOW_BACKGROUND"));
    if let Some((img, sz)) = c.sprite("Compass_Background", false) {
        let face = [IDLE, "CursorOver", "CursorDown"].map(|k| {
            let (sheet, r) = sprite_area(&hud, "Compass_Background", &format!("TextureArea - {k}")).expect("Compass_Background art");
            (assets.load::<Image>(format!("hud/tex/{sheet}.png")), inset(&sheet, r))
        });
        commands.spawn((img, c.abs(bg_pos.x, bg_pos.y, sz), RadarFace(face), ChildOf(compass)));
    }
    let banks = combat::banks(&lib, class);
    let frames = Frames {
        gun: ["WeaponBar_GunBankCanHitTarget", "WeaponBar_GunBankNotFullyRecharged", "WeaponBar_GunBankDestroyed", "WeaponBar_SlotBackground"]
            .iter()
            .filter_map(|n| c.frame(n))
            .collect(),
        dial: (0..4).filter_map(|i| c.frame(&format!("SpeedControl_CurrActive{i}_NextActive{i}"))).collect(),
        sweep: (0..10).filter_map(|i| c.frame(&format!("HUDRadar_Sweep_Frame{i}"))).collect(),
        icons: banks.iter().map(|b| combat::gun_icon(guns.gun(b.1))).filter_map(|n| c.frame(&n).map(|f| (n, f))).collect(),
    };
    if let Some((img, sz)) = c.sprite("HUDRadar_Sweep_Frame0", false) {
        let o = offset(&hud, "RADAR_SWEEP_OFFSET_FROM_TOPLEFT");
        commands.spawn((Sweep, img, c.abs(o.x, o.y, sz), ChildOf(compass)));
    }
    if let Some((img, sz)) = c.sprite("HUDRADAR_ICON_ENEMYSHIP_FRAME0", false) {
        for i in 0..MAX_CONTACTS {
            commands.spawn((Contact(i), img.clone(), c.abs(0.0, 0.0, sz), Visibility::Hidden, ChildOf(compass)));
        }
    }
    if let Some((img, sz)) = c.sprite("COMPASS_OVERLAY", false) {
        commands.spawn((img, c.abs(0.0, 0.0, sz), ChildOf(compass), solid()));
    }
    commands.spawn((RadarButton, c.abs(bg_pos.x, bg_pos.y, bg), ChildOf(compass), solid()));
    {
        // The skull toggle's gold arm stands on the compass's top-left corner, its bottom `Move_ToggleWeaponBar_Detail_Down` px
        // behind the compass (matches the retail 1:1 screenshot).
        let detail = c.size_of("WeaponBar_ToggleDetail");
        let at = Vec2::new(0.0, offset(&hud, "Move_ToggleWeaponBar_Detail_Down").y - detail.y);
        c.toggle(&mut commands, compass, "WeaponBar_ToggleDetail", at, "WeaponBar_ToggleButton", offset(&hud, "ToggleWeaponBar_Button_Offset_From_Detail_TopLeft"), ToggleButton);
    }
    commands.insert_resource(Radar { centre: bg_pos + bg / 2.0, radius: bg.x * 0.47, contact_size: c.size_of("HUDRADAR_ICON_ENEMYSHIP_FRAME0") });
    {
        // The speed gauge hangs off the compass's top-right shoulder (reference video): its top edge is 44 px above the compass.
        if let Some((img, sz)) = c.sprite("SpeedControl_CurrActive0_NextActive0", false) {
            commands.spawn((
                Dial,
                crate::panels::EngineDial,
                img,
                Node { position_type: PositionType::Absolute, left: px(76.0, s), bottom: px(overlay.y + 44.0 - sz.y, s), width: px(sz.x, s), height: px(sz.y, s), ..default() },
                RelativeCursorPosition::default(),
                ChildOf(root),
                solid(),
            ));
        }
    }

    talking_head(&mut commands, &c, root);
    weapon_panel(&mut commands, &c, root, class, &banks, guns, window.height());
    commands.insert_resource(frames);
}

/// The talking head (bottom-right corner): wooden housing with the portrait seen through its ring, the iris, light and voice box,
/// and the ship-bar toggle on its corner.
fn talking_head(commands: &mut Commands, c: &Ctx, root: Entity) {
    let (hud, s) = (c.hud, c.s);
    let solid = || (Interaction::None, FocusPolicy::Block, Solid);
    let housing = c.size_of("TalkingHead_Solid_Background");
    if housing.x <= 0.0 {
        return;
    }
    let names = || {
        ["TalkingHead_Portal_Closed"].into_iter().map(String::from).chain((0..10).map(|i| format!("TalkingHead_Portal_ClosedToOpen{i}"))).chain(["TalkingHead_Portal_Open".into()])
    };
    let voice = ["Start", "Frame0", "Frame1", "Stop", "Off"].map(|n| format!("TalkingHead_Portal_VoiceBox_{n}"));
    commands.insert_resource(HeadFrames { iris: names().filter_map(|n| c.frame(&n)).collect(), voice: voice.iter().filter_map(|n| c.frame(n)).collect() });
    let head = commands
        .spawn((
            Node { position_type: PositionType::Absolute, right: Val::Px(0.0), bottom: Val::Px(0.0), width: px(housing.x, s), height: px(housing.y, s), ..default() },
            ChildOf(root),
        ))
        .id();
    let (face_at, iris_at) = (offset(hud, "Right_TalkingHead_Character_From_TopLeft"), offset(hud, "Right_TalkingHead_PortalOverlay_From_TopLeft"));
    let portal = Vec2::splat(80.0);
    let layer = |commands: &mut Commands, kind: Head, art: Option<(ImageNode, Vec2)>, at: Vec2, hidden: bool| {
        let Some((img, sz)) = art else { return };
        let vis = if hidden { Visibility::Hidden } else { Visibility::Inherited };
        commands.spawn((kind, img, c.abs(at.x, at.y, sz), vis, ChildOf(head)));
    };
    {
        // Draw order: portrait, iris, housing (its ring is transparent), then the light and voice box on the wood.
        layer(commands, Head::Face, Some((ImageNode::default(), portal)), face_at, true);
        layer(commands, Head::Iris, c.sprite("TalkingHead_Portal_Closed", false), iris_at, false);
        if let Some((img, sz)) = c.sprite("TalkingHead_Solid_Background", false) {
            commands.spawn((img, c.abs(0.0, 0.0, sz), ChildOf(head), solid()));
        }
        // PROVISIONAL placement, measured on the housing art: the light sits over its green lamp, the voice box in the name plate.
        layer(commands, Head::Light, c.sprite("TalkingHead_Portal_Light_On", false), Vec2::new(75.0, 2.0), true);
        layer(commands, Head::Voice, c.sprite("TalkingHead_Portal_VoiceBox_Off", false), Vec2::new(20.0, 103.0), false);
    }
    // The ship-bar toggle's arm stands on the housing's top-right corner, like the weapon toggle on the compass.
    let detail = c.size_of("ShipBar_ToggleDetail");
    let at = Vec2::new(housing.x - detail.x, offset(hud, "Move_ToggleShipBar_Detail_Down").y - detail.y);
    let button = offset(hud, "ToggleShipBar_Button_Offset_From_Detail_TopLeft");
    c.toggle(commands, head, "ShipBar_ToggleDetail", at, "ShipBar_ToggleButton", button, crate::panels::FleetButton::Ships);
    // The ship bar opens `SHIPBAR_OFFSET_Y_FROM_TOGGLESHIPBAR_BUTTON` above the toggle button.
    let top = housing.y - at.y - button.y + offset(hud, "SHIPBAR_OFFSET_Y_FROM_TOGGLESHIPBAR_BUTTON").y;
    commands.insert_resource(ShipBarAnchor(top));
}

/// The weapon panel (left edge, vertically centred): the ship outline, and per selectable gun bank its status frame, gun icon and
/// recharge bar.
fn weapon_panel(commands: &mut Commands, c: &Ctx, root: Entity, class: &combat::Class, banks: &[(usize, usize)], guns: &GunStates, window_h: f32) {
    let (hud, s) = (c.hud, c.s);
    let solid = || (Interaction::None, FocusPolicy::Block, Solid);
    let wbg = c.size_of("WeaponBar_ShipIcon_Background");
    let weapon = commands
        .spawn((
            WeaponPanel,
            Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px((window_h - 255.0 * s) / 2.0), width: px(wbg.x, s), height: px(255.0, s), ..default() },
            ChildOf(root),
        ))
        .id();
    if let Some((img, sz)) = c.sprite("WeaponBar_ShipIcon_Background", false) {
        commands.spawn((img, c.abs(0.0, 0.0, sz), ChildOf(weapon), solid()));
    }
    if let Some((img, sz)) = c.sprite(&class.icon("WeaponBar"), false) {
        let o = offset(hud, "WEAPONBAR_SHIPICON_OFFSET_FROM_TOPLEFT");
        commands.spawn((img, c.abs(o.x, o.y, sz), ChildOf(weapon)));
    }
    for (bank, first) in banks {
        let pos = offset(hud, &format!("WeaponBar_SlotOffset_From_TopLeft_{bank:02}"));
        let icon_at = pos + offset(hud, "Offset_WeaponBar_GunIcon");
        if let Some((img, sz)) = c.sprite("WeaponBar_GunBankCanHitTarget", false) {
            commands.spawn((GunSlot { bank: *bank, icon: false }, img, c.abs(icon_at.x, icon_at.y, sz), ChildOf(weapon)));
        }
        if let Some((img, sz)) = c.sprite(&combat::gun_icon(guns.gun(*first)), false) {
            commands.spawn((GunSlot { bank: *bank, icon: true }, GunButton(*bank), img, c.abs(icon_at.x, icon_at.y, sz), ChildOf(weapon), solid()));
        }
        // Recharge bar beside the icon: the empty bar (WeaponBar_SlotBackground) with the yellow WeaponBar_RechargeStatus growing from the bottom.
        // PROVISIONAL: drawing order/fill direction are read from the artwork; the original's drawing code was not found.
        if let (Some((outline, osz)), Some((img, fsz))) = (c.sprite("WeaponBar_SlotBackground", false), c.sprite("WeaponBar_RechargeStatus", false)) {
            let at = pos + offset(hud, "Offset_WeaponBar_Recharge");
            commands.spawn((outline, c.abs(at.x, at.y, osz), ChildOf(weapon)));
            let base = img.rect.unwrap();
            let k = sprite_of(hud, "WeaponBar_RechargeStatus").map_or(1.0, |(sheet, _)| sheet_scale(&sheet));
            commands.spawn((Fill { gauge: Gauge::Recharge(*bank), full: fsz.y, top: at.y, base, k }, img, c.abs(at.x, at.y, fsz), ChildOf(weapon)));
        }
    }
}

/// One bar part plus whatever sits on it: the ship icon on the ship frame, a fill on each status gauge.
fn spawn_part(commands: &mut Commands, c: &Ctx, class: &combat::Class, parent: Entity, name: &str, mut node: Node) {
    let Some((img, size)) = c.sprite(name, true) else { return };
    node.width = px(size.x, c.s);
    node.height = px(size.y, c.s);
    let pause_art = (name == "MAINBAR_IS_NOT_PAUSED_BUTTON").then(|| c.sprite("MAINBAR_IS_PAUSED_BUTTON", true)).flatten();
    let running = img.clone();
    let part = commands.spawn((img, node, ChildOf(parent), Solid, Interaction::None, FocusPolicy::Block)).id();
    if name == "MainBar_PanicButton" {
        commands.entity(part).insert(PanicButton);
    }
    if let Some((paused, _)) = pause_art {
        commands.entity(part).insert((PauseButton, PauseArt { running, paused }));
    }
    if name == "MainBar_ShipIcon" {
        commands.entity(part).insert(ShipIconButton);
    }
    let fill = |commands: &mut Commands, sprite: &str, offset_name: &str, gauge: Gauge| {
        let Some((img, fsize)) = c.sprite(sprite, false) else { return };
        let o = offset(c.hud, offset_name);
        let top = o.y;
        let base = img.rect.unwrap();
        let k = sprite_of(c.hud, sprite).map_or(1.0, |(sheet, _)| sheet_scale(&sheet));
        commands.spawn((Fill { gauge, full: fsize.y, top, base, k }, img, c.abs(o.x, top, fsize), ChildOf(part)));
    };
    if name == "MainBar_FleetFormationsButton" {
        commands.entity(part).insert(crate::panels::FleetButton::Commands);
    }
    if name == "MainBar_BeginnerMovementTypeButton" {
        commands.entity(part).insert(crate::panels::MovementButton);
    }
    match name {
        "MainBar_ShipIcon" => {
            if let Some((img, isz)) = c.sprite(&class.icon("ShipBar"), false) {
                let o = offset(c.hud, "MainBar_ShipIcon_Offset_From_Background_TopLeft");
                let portrait = commands.spawn((img, c.abs(o.x, o.y, isz), ChildOf(part))).id();
                if let Some((status, sz)) = c.sprite("ShipStatus_Icon_Docking_Frame0", false) {
                    let at = offset(c.hud, "PRIMARY_SHIP_STATUS_ICON_OFFSET_FROM_PRIMARYSHIPICON_TOPLEFT");
                    commands.spawn((DockingStatus, status, c.abs(at.x, at.y, sz), Visibility::Hidden, ChildOf(portrait)));
                }
            }
        }
        "MAINBAR_PRIMARY_SHIP_HEALTH_BAR_BACKGROUND" => fill(commands, "MainBar_ShipStatus_Health", "MainBar_ShipStatus_Health_Offset_From_MainBar_TopLeft", Gauge::Health),
        "MAINBAR_PRIMARY_SHIP_COMBAT_BAR_BACKGROUND" => fill(commands, "MainBar_ShipStatus_CombatStrength", "MainBar_ShipStatus_Combat_Offset_From_MainBar_TopLeft", Gauge::Combat),
        "MAINBAR_PRIMARY_SHIP_SAILPOWER_BAR_BACKGROUND" => fill(commands, "MainBar_ShipStatus_SailPower", "MainBar_ShipStatus_SailPower_Offset_From_MainBar_TopLeft", Gauge::Sail),
        _ => {}
    }
}

fn update_dial(player: Single<&Ship, With<PlayerShip>>, frames: Option<Res<Frames>>, mut q: Query<&mut ImageNode, With<Dial>>) {
    let Some(frames) = frames else { return };
    // Retail frames 0..3 match Stop / Slow / Full / Emergency (red through dark green).
    let Some((h, r)) = frames.dial.get(player.order.min(3)) else { return };
    for mut img in &mut q {
        show(&mut img, h, *r);
    }
}

/// Shows sprite `(h, r)` on an image node, touching the node (and so re-rendering it) only when it changes.
fn show(img: &mut Mut<ImageNode>, h: &Handle<Image>, r: Rect) {
    if img.image != *h || img.rect != Some(r) {
        img.image = h.clone();
        img.rect = Some(r);
    }
}

/// Iris, face, light and voice box of the retail device, from the shared `HeadState`.
fn head_art(
    time: Res<Time>,
    head: Res<HeadState>,
    hud: Res<HudData>,
    assets: Res<AssetServer>,
    frames: Option<Res<HeadFrames>>,
    mut parts: Query<(&Head, &mut ImageNode, &mut Visibility)>,
) {
    let Some(frames) = frames.filter(|f| !f.iris.is_empty() && f.voice.len() == 5) else { return };
    let (open, speaking) = (head.open, head.speaking);
    let cycle = (time.elapsed_secs() * VOICE_FPS) as usize % 4;
    for (kind, mut img, mut vis) in &mut parts {
        let set = |img: &mut Mut<ImageNode>, (h, r): &(Handle<Image>, Rect)| show(img, h, *r);
        match kind {
            Head::Iris => set(&mut img, &frames.iris[iris_frame(open, frames.iris.len())]),
            Head::Voice => set(&mut img, &frames.voice[if speaking && open >= 1.0 { cycle } else { 4 }]),
            Head::Face => {
                if let Some((sheet, r)) = head.speaker.as_deref().and_then(|n| sprite_of(&hud, n)) {
                    show(&mut img, &assets.load(format!("hud/tex/{sheet}.png")), inset(&sheet, r));
                }
                vis.set_if_neq(if open > 0.0 { Visibility::Inherited } else { Visibility::Hidden });
            }
            Head::Light => {
                vis.set_if_neq(if speaking && open >= 1.0 { Visibility::Inherited } else { Visibility::Hidden });
            }
        }
    }
}

fn update_sweep(time: Res<Time>, frames: Option<Res<Frames>>, mut q: Query<&mut ImageNode, With<Sweep>>) {
    let Some(frames) = frames else { return };
    if frames.sweep.is_empty() {
        return;
    }
    let (h, r) = &frames.sweep[(time.elapsed_secs() / 0.1) as usize % frames.sweep.len()];
    for mut img in &mut q {
        show(&mut img, h, *r);
    }
}

fn radar_face(button: Query<&Interaction, With<RadarButton>>, mut q: Query<(&RadarFace, &mut ImageNode)>) {
    let i = button.iter().next().map_or(0, |i| match i {
        Interaction::None => 0,
        Interaction::Hovered => 1,
        Interaction::Pressed => 2,
    });
    for (f, mut img) in &mut q {
        let (h, r) = &f.0[i];
        show(&mut img, h, *r);
    }
}

fn update_gauges(
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    lib: Res<Library>,
    player: Single<(&Ship, &Health, &crate::boarding::Boardable), With<PlayerShip>>,
    mut q: Query<(&Fill, &mut Node, &mut ImageNode)>,
    guns: Single<&GunStates, With<PlayerShip>>,
) {
    let s = window.height() / REF_H;
    let (ship, health, crew) = *player;
    for (f, mut node, mut img) in &mut q {
        let v = gauge_value(f.gauge, &lib, ship, health, crew, &guns).clamp(0.0, 1.0);
        // Gauges fill from the bottom: keep the bottom of the sprite, crop the top.
        let h = (f.full * v).round();
        clip(&mut node, &mut img, f.top + f.full - h, h, s, f.base, f.k);
    }
}

/// A bar that empties from the top: `h` reference px of sprite rect `base` (sheet pixels `k` per reference px) shown from `top`,
/// written only on a change.
fn clip(node: &mut Mut<Node>, img: &mut Mut<ImageNode>, top: f32, h: f32, s: f32, base: Rect, k: f32) {
    let (top, height) = (px(top, s), px(h, s));
    if node.top != top || node.height != height {
        (node.top, node.height) = (top, height);
    }
    let mut r = base;
    r.min.y = r.max.y - h * k;
    if img.rect != Some(r) {
        img.rect = Some(r);
    }
}

fn update_guns(
    frames: Option<Res<Frames>>,
    lib: Res<Library>,
    target: Res<combat::Target>,
    player: Single<(&Ship, &Health, &GunStates), With<PlayerShip>>,
    mut q: Query<(&GunSlot, &mut ImageNode)>,
) {
    let Some(frames) = frames else { return };
    let (ship, health, guns) = *player;
    let class = &lib.classes[ship.class];
    for (slot, mut img) in &mut q {
        let st = combat::bank_state(guns, class, health, slot.bank);
        let armed = target.2.armed == Some(slot.bank);
        if slot.icon {
            let Some(first) = class.mounts.iter().position(|m| m.bank == slot.bank) else { continue };
            if let Some((h, r)) = frames.icons.get(&combat::gun_icon(guns.gun(first))) {
                show(&mut img, h, *r);
            }
            // Dimmed while the bank reloads (the bar beside it shows how far along it is).
            let tint = match st {
                2 => Color::srgba(1.0, 1.0, 1.0, 0.25),
                1 => Color::srgb(0.55, 0.55, 0.55),
                _ => Color::WHITE,
            };
            img.map_unchanged(|i| &mut i.color).set_if_neq(tint);
        } else if let Some((h, r)) = frames.gun.get(st) {
            show(&mut img, h, *r);
            img.map_unchanged(|i| &mut i.color).set_if_neq(if armed { Color::srgb(1.0, 0.9, 0.3) } else { Color::WHITE });
        }
    }
}

/// PAUSE button art follows the simulation pause. PROVISIONAL: the retail hourglass art is swapped; any retail restrictions while
/// paused are not decoded.
fn pause_art(vtime: Res<Time<Virtual>>, mut q: Query<(&PauseArt, &mut ImageNode)>) {
    let paused = vtime.is_paused();
    for (button, mut img) in &mut q {
        let want = if paused { &button.paused } else { &button.running };
        if img.image != want.image || img.rect != want.rect {
            img.image = want.image.clone();
            img.rect = want.rect;
        }
    }
}

/// Reference-px height above the screen bottom where the ship bar starts (top of the toggle button + the data's gap).
#[derive(Resource)]
struct ShipBarAnchor(f32);
/// Ship-bar slot parts, by row index.
#[derive(Component)]
struct FleetIcon(usize);
#[derive(Component)]
struct FleetHealth(usize);
#[derive(Component)]
struct FleetStatus(usize);
/// The selected secondary ship's name on its plate.
#[derive(Component)]
struct PlateText;
/// A Fleet Commands button whose art follows the fleet state (active formation, hold fire).
#[derive(Component)]
struct CommandArt([States; 2]);

/// Fleet Commands window (`FormationBar_*` art): a frame as wide as the main bar standing on it (`MainBarExtension` top offset),
/// the six formation buttons from `Offset_FormationBar_Icons`, then the attack and hold-fire buttons `Offset_FleetHelpBar_Icons`.x
/// apart. PROVISIONAL: no retail capture of the open window; the frame width and the help buttons' place are inferred from the data
/// (the icons end at x 470 on the 487 px Navy bar).
fn commands_window(commands: &mut Commands, c: &Ctx, root: Entity, screen_w: f32, bar_w: f32, floor: f32) {
    use crate::panels::FleetButton as B;
    let top = offset(c.hud, "MainBarExtension_TopLeft_Offset_From_MainBar_TopLeft").y;
    let (l, r) = (c.size_of("FormationBar_LeftDetail"), c.size_of("FormationBar_RightDetail"));
    let h = l.y.max(r.y);
    let node = Node { bottom: px(floor - top - h, c.s), ..c.abs((screen_w - bar_w) / 2.0, 0.0, Vec2::new(bar_w, h)) };
    let bar = commands.spawn((crate::panels::FleetWindow(false), Node { top: Val::Auto, ..node }, Visibility::Hidden, ChildOf(root))).id();
    let piece = |commands: &mut Commands, name: &str, x: f32, w: f32| {
        if let Some((mut img, sz)) = c.sprite(name, false) {
            img.image_mode = NodeImageMode::Stretch;
            commands.spawn((img, c.abs(x, 0.0, Vec2::new(w, sz.y)), ChildOf(bar), Solid, Interaction::None, FocusPolicy::Block));
        }
    };
    piece(commands, "FormationBar_StretchedCenter", l.x, bar_w - l.x - r.x);
    piece(commands, "FormationBar_LeftDetail", 0.0, l.x);
    piece(commands, "FormationBar_RightDetail", bar_w - r.x, r.x);
    let mut buttons: Vec<(B, String, String, f32)> = crate::ship::Formation::BUTTONS
        .iter()
        .map(|kind| {
            let name = format!("{kind:?}");
            (B::Formation(*kind), format!("FormationBar_Icon_{name}"), format!("FORMATIONBAR_ICON_{}_ACTIVE", name.to_uppercase()), 0.0)
        })
        .collect();
    let attack = "FORMATIONBAR_ICON_ATTACK_MY_TARGET".to_string();
    buttons.push((B::Attack, attack.clone(), attack, offset(c.hud, "Offset_FleetHelpBar_Icons").x));
    // PROVISIONAL: shows the order a click gives (hold fire, or resume once held).
    buttons.push((B::CeaseFire, "FORMATIONBAR_ICON_HOLD_FIRE".into(), "FORMATIONBAR_ICON_NOT_HOLD_FIRE".into(), 0.0));
    let mut at = offset(c.hud, "Offset_FormationBar_Icons");
    for (action, idle, on, gap) in buttons {
        let (Some((img, sz)), Some(a), Some(b)) = (c.sprite(&idle, false), c.states(&idle), c.states(&on)) else { continue };
        at.x += gap;
        commands.spawn((action, img, CommandArt([a, b]), c.abs(at.x, at.y, sz), ChildOf(bar), Solid, Interaction::None, FocusPolicy::Block));
        at.x += sz.x;
    }
}

/// Game-speed control (testing aid, not retail): a tooltip-backdrop plate beside the main bar's right end between round slower /
/// faster buttons (the front-end Prev / Next art at 30 px, which only the Navy set has).
fn speed_plate(commands: &mut Commands, c: &Ctx, fonts: Option<&crate::sprites::Fonts>, root: Entity, bar_right: f32) {
    use crate::panels::{SpeedButton, SpeedLabel};
    let (l, r, h) = (c.size_of("ToolTipBackdrop_Left").x, c.size_of("ToolTipBackdrop_Right").x, c.size_of("ToolTipBackdrop_Center").y);
    let (w, b) = (110.0, 30.0);
    let plate = commands.spawn((Node { top: Val::Auto, bottom: px(6.0, c.s), ..c.abs(bar_right + 6.0, 0.0, Vec2::new(w, h)) }, ChildOf(root))).id();
    let (x0, x1) = (b / 2.0, w - b / 2.0);
    for (name, x, pw) in [("ToolTipBackdrop_Center", x0 + l, x1 - x0 - l - r), ("ToolTipBackdrop_Left", x0, l), ("ToolTipBackdrop_Right", x1 - r, r)] {
        if let Some((mut img, _)) = c.sprite(name, false) {
            img.image_mode = NodeImageMode::Stretch;
            commands.spawn((img, c.abs(x, 0.0, Vec2::new(pw, h)), ChildOf(plate), Solid, Interaction::None, FocusPolicy::Block));
        }
    }
    let menu = HudData { comps: c.hud.comps.clone(), skin: crate::sprites::MENU_SKIN };
    let arrows = Ctx { hud: &menu, ..*c };
    for (button, name, x) in [(SpeedButton::Slower, "FrontEnd_Prev_Button", 0.0), (SpeedButton::Faster, "FrontEnd_Next_Button", w - b)] {
        if let (Some((mut img, _)), Some(states)) = (arrows.sprite(name, false), arrows.states(name)) {
            img.image_mode = NodeImageMode::Stretch;
            commands.spawn((button, img, states, c.abs(x, (h - b) / 2.0, Vec2::splat(b)), ChildOf(plate), Solid, Interaction::None, FocusPolicy::Block));
        }
    }
    let Some(font) = fonts.and_then(|f| f.0.get("UniversBold14")) else { return };
    let label = Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..c.abs(b, 0.0, Vec2::new(w - 2.0 * b, h)) };
    commands.spawn((label, ChildOf(plate))).with_children(|p| {
        p.spawn((SpeedLabel, font.text("", c.s, Color::srgb(0.25, 0.12, 0.03))));
    });
}

/// Name plate of a selected secondary ship (the HUD then hides the controls that only work on the command ship).
fn name_plate(
    mut commands: Commands,
    assets: Res<AssetServer>,
    hud: Res<HudData>,
    fonts: Option<Res<crate::sprites::Fonts>>,
    model: Res<crate::panels::FleetHud>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    roots: Query<Entity, With<HudRoot>>,
    mut plate: Query<(&mut Visibility, &ChildOf), With<crate::panels::SelectedName>>,
    mut plate_text: Query<&mut Text, With<PlateText>>,
) {
    let Some(root) = roots.iter().next() else { return };
    let s = window.height() / REF_H;
    let c = Ctx { assets: &assets, hud: &hud, s, trim: false };
    let plate_live = plate.iter().any(|(_, p)| p.parent() == root);
    if !plate_live {
        // Name plate of a selected secondary ship (the HUD then hides the controls that only work on the command ship).
        // `ShipName_HUDComponent_Backdrop_*` of the player's art set, text `ShipName_HUDComponent_Text_Offset_From_Backdrop_TopLeft`
        // below its top. PROVISIONAL: top-centre placement, the art set (player's, not the selected ship's) and the font.
        let skin = match hud.skin {
            "Pirate" | "Procyon" => hud.skin,
            _ => "Navy",
        };
        let part = |p: &str| format!("ShipName_HUDComponent_Backdrop_{p}_{skin}");
        let row = Node { position_type: PositionType::Absolute, top: px(4.0, s), left: Val::Px(0.0), right: Val::Px(0.0), justify_content: JustifyContent::Center, ..default() };
        let row = commands.spawn((crate::panels::SelectedName, row, Visibility::Hidden, ChildOf(root))).id();
        let text_y = offset(&hud, "ShipName_HUDComponent_Text_Offset_From_Backdrop_TopLeft").y;
        for side in ["Left", "Center", "Right"] {
            let Some((mut img, sz)) = c.sprite(&part(side), false) else { continue };
            img.image_mode = NodeImageMode::Stretch;
            let mut node = Node { width: px(sz.x, s), height: px(sz.y, s), ..default() };
            if side == "Center" {
                (node.width, node.min_width) = (Val::Auto, px(sz.x, s));
                node.justify_content = JustifyContent::Center;
            }
            let piece = commands.spawn((img, node, ChildOf(row))).id();
            if let Some(font) = fonts.as_deref().and_then(|f| f.0.get("UniversBold14")).filter(|_| side == "Center") {
                // The offset goes on the text: padding would also move the plate art (images fill the content box).
                let dark = Color::srgb(0.25, 0.12, 0.03);
                commands.spawn((PlateText, font.text("", s, dark), Node { margin: UiRect::top(px(text_y, s)), ..default() }, ChildOf(piece)));
            }
        }
    }
    for mut text in &mut plate_text {
        text.set_if_neq(Text::new(model.name.clone()));
    }
    for (mut v, _) in &mut plate {
        v.set_if_neq(if model.shown && !model.direct && !model.name.is_empty() { Visibility::Inherited } else { Visibility::Hidden });
    }
}

fn button_states(mut q: Query<(&States, &Interaction, &mut ImageNode), Or<(Changed<Interaction>, Added<States>)>>) {
    for (states, i, mut img) in &mut q {
        let (h, r) = &states.0[state_index(i)];
        img.image = h.clone();
        img.rect = Some(*r);
    }
}
fn state_index(i: &Interaction) -> usize {
    match i {
        Interaction::None => 0,
        Interaction::Hovered => 1,
        Interaction::Pressed => 2,
    }
}

/// Ship bar (secondary ships, `ShipBar_*` art) above the ship-bar toggle at the right edge: `TopDetail`, one `StretchedCenter` per
/// extra row, `BottomDetail`; two slot columns, a lone last slot centred (docs/re/fleet-controls.md). Each slot: its background
/// (selected art for the selected ship), the class icon at `Offset_ShipBar_ShipIcon`, the `ShipBar_Health` strip clipped from the
/// top as the ship loses health, and the docking/boarding icon. PROVISIONAL: slot columns and the 51 px row pitch are read off the
/// art (first row at `SHIPBAR_SHIPICON_ORIGIN_OFFSET_FROM_SHIPBAR_TOPLEFT`.y); scroll arrows and the open/close slide are not drawn.
fn fleet_art(
    mut commands: Commands,
    assets: Res<AssetServer>,
    hud: Res<HudData>,
    time: Res<Time>,
    model: Res<crate::panels::FleetHud>,
    anchor: Option<Res<ShipBarAnchor>>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    roots: Query<Entity, With<HudRoot>>,
    mut built: Local<Option<(Entity, usize)>>,
    old: Query<(Entity, &crate::panels::FleetWindow)>,
    mut windows: Query<(&crate::panels::FleetWindow, &mut Visibility), (Without<crate::panels::FleetButton>, Without<FleetStatus>, Without<crate::panels::SelectedName>)>,
    mut buttons: Query<(&crate::panels::FleetButton, &Interaction, &mut ImageNode, Option<&CommandArt>), Without<FleetIcon>>,
    mut icons: Query<(&FleetIcon, &mut ImageNode), Without<crate::panels::FleetButton>>,
    mut health: Query<(&FleetHealth, &mut Node, &mut ImageNode), (Without<crate::panels::FleetButton>, Without<FleetIcon>)>,
    mut status: Query<(&FleetStatus, &mut ImageNode, &mut Visibility), (Without<crate::panels::FleetButton>, Without<FleetIcon>, Without<FleetHealth>)>,
) {
    use crate::panels::{FleetButton as B, FleetWindow};
    let (Some(root), Some(anchor)) = (roots.iter().next(), anchor) else { return };
    let s = window.height() / REF_H;
    let c = Ctx { assets: &assets, hud: &hud, s, trim: false };
    let n = model.rows.len();
    if *built != Some((root, n)) {
        *built = Some((root, n));
        for (e, w) in &old {
            if w.0 {
                commands.entity(e).despawn();
            }
        }
        let (top, mid, bottom) = (c.size_of("ShipBar_TopDetail"), c.size_of("ShipBar_StretchedCenter"), c.size_of("ShipBar_BottomDetail"));
        let rows = n.div_ceil(2).max(1);
        let h = top.y + (rows - 1) as f32 * mid.y + bottom.y;
        let node = Node { right: Val::Px(0.0), bottom: px(anchor.0, s), left: Val::Auto, top: Val::Auto, ..c.abs(0.0, 0.0, Vec2::new(top.x, h)) };
        let bar = commands.spawn((FleetWindow(true), node, Visibility::Hidden, ChildOf(root))).id();
        let mut y = 0.0;
        for (name, size, count) in [("ShipBar_TopDetail", top, 1), ("ShipBar_StretchedCenter", mid, rows - 1), ("ShipBar_BottomDetail", bottom, 1)] {
            for _ in 0..count {
                if let Some((img, _)) = c.sprite(name, false) {
                    commands.spawn((img, c.abs(0.0, y, size), ChildOf(bar), Solid, Interaction::None, FocusPolicy::Block));
                }
                y += size.y;
            }
        }
        let slot = c.size_of("ShipBar_SlotBackground");
        let first = offset(&hud, "SHIPBAR_SHIPICON_ORIGIN_OFFSET_FROM_SHIPBAR_TOPLEFT").y;
        let inner = top.x - 8.0; // the gold post on the right
        let gap = (inner - 2.0 * slot.x) / 3.0;
        for i in 0..n {
            let x = if i + 1 == n && i % 2 == 0 { (inner - slot.x) / 2.0 } else { gap + (i % 2) as f32 * (slot.x + gap) };
            let Some((img, _)) = c.sprite("ShipBar_SlotBackground", false) else { continue };
            let cell = commands.spawn((B::Slot(i), img, c.abs(x, first + (i / 2) as f32 * mid.y, slot), ChildOf(bar), Solid, Interaction::None, FocusPolicy::Block)).id();
            let o = offset(&hud, "Offset_ShipBar_ShipIcon");
            commands.spawn((FleetIcon(i), ImageNode::default(), c.abs(o.x, o.y, c.size_of("ShipBar_ShipIcon_Navy_Cutter")), ChildOf(cell)));
            if let Some((img, sz)) = c.sprite("ShipBar_Health", false) {
                commands.spawn((FleetHealth(i), img, c.abs(0.0, 0.0, sz), ChildOf(cell)));
            }
            if let Some((img, sz)) = c.sprite("ShipStatus_Icon_Docking_Frame0", false) {
                let at = o + offset(&hud, "PRIMARY_SHIP_STATUS_ICON_OFFSET_FROM_PRIMARYSHIPICON_TOPLEFT");
                commands.spawn((FleetStatus(i), img, c.abs(at.x, at.y, sz), Visibility::Hidden, ChildOf(cell)));
            }
        }
    }
    for (w, mut v) in &mut windows {
        v.set_if_neq(if if w.0 { model.ships } else { model.commands } { Visibility::Inherited } else { Visibility::Hidden });
    }
    for (b, interaction, mut img, art) in &mut buttons {
        let pick = match b {
            B::Slot(i) => model.rows.get(*i).and_then(|row| {
                let art = if row.selected { "ShipBar_Selected_SlotBackground" } else { "ShipBar_SlotBackground" };
                c.states(art).map(|st| st.0[state_index(interaction)].clone())
            }),
            B::Formation(kind) => art.map(|a| a.0[(model.active == Some(*kind)) as usize].0[state_index(interaction)].clone()),
            B::CeaseFire => art.map(|a| a.0[model.cease_fire as usize].0[state_index(interaction)].clone()),
            _ => None,
        };
        if let Some((h, r)) = pick {
            show(&mut img, &h, r);
        }
    }
    for (FleetIcon(i), mut img) in &mut icons {
        if let Some((art, _)) = model.rows.get(*i).and_then(|row| c.sprite(&row.icon, false)) {
            show(&mut img, &art.image, art.rect.unwrap_or_default());
        }
    }
    let full = c.size_of("ShipBar_Health").y;
    for (FleetHealth(i), mut node, mut img) in &mut health {
        let (Some(row), Some((base, _))) = (model.rows.get(*i), c.sprite("ShipBar_Health", false)) else { continue };
        // Clipped from the top: the strip shrinks towards the bottom as the ship loses health.
        let h = (full * row.health).round();
        let k = sprite_of(&hud, "ShipBar_Health").map_or(1.0, |(sheet, _)| sheet_scale(&sheet));
        clip(&mut node, &mut img, full - h, h, s, base.rect.unwrap_or_default(), k);
    }
    for (FleetStatus(i), mut img, mut v) in &mut status {
        let kind = model.rows.get(*i).map_or("", |r| r.status);
        v.set_if_neq(if kind.is_empty() { Visibility::Hidden } else { Visibility::Inherited });
        let frame = (time.elapsed_secs() * crate::panels::STATUS_FPS) as usize % 6;
        if let Some((h, r)) = (!kind.is_empty()).then(|| c.frame(&format!("ShipStatus_Icon_{kind}_Frame{frame}"))).flatten() {
            show(&mut img, &h, r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets_dir;

    #[test]
    fn iris_frames_run_closed_to_open() {
        assert_eq!((iris_frame(0.0, 12), iris_frame(0.5, 12), iris_frame(1.0, 12), iris_frame(2.0, 12)), (0, 6, 11, 11));
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn speed_segments_and_frames_match_engine_orders() {
        for (order, centre) in [(0, Vec2::new(54.0, 71.0)), (1, Vec2::new(44.0, 57.0)), (2, Vec2::new(31.0, 44.0)), (3, Vec2::new(14.0, 34.0))] {
            assert_eq!(crate::panels::speed_order_at(centre), order);
            assert_eq!(crate::panels::speed_order_at(centre + Vec2::splat(1.0)), order);
        }
        let data = HudData { comps: crate::data::read_json(&assets_dir().join("hud/hud.json")).into(), skin: "Navy" };
        let frames: Vec<_> = (0..4).map(|i| sprite_of(&data, &format!("SpeedControl_CurrActive{i}_NextActive{i}")).unwrap()).collect();
        for frame in &frames {
            assert_eq!(frame.1.size(), Vec2::new(86.0, 101.0));
        }
        for i in 0..4 {
            for j in i + 1..4 {
                assert_ne!(frames[i], frames[j]);
            }
        }
    }
    #[test]
    #[ignore = "needs imported assets"]
    fn dial_click_sets_the_engine_order_and_counts_as_a_hud_click() {
        let (mut app, root) = crate::panels::input_app();
        app.add_systems(Update, crate::panels::engine_input.before(crate::panels::hud_input));
        let rel = RelativeCursorPosition { cursor_over: true, normalized: Some(Vec2::new(31.0 / 86.0 - 0.5, 44.0 / 101.0 - 0.5)) };
        app.world_mut().spawn((Solid, crate::panels::EngineDial, Interaction::Pressed, rel));
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        let order = app.world_mut().query_filtered::<&Ship, With<PlayerShip>>().single(app.world()).unwrap().order;
        assert_eq!(order, 2);
        assert!(app.world().resource::<crate::panels::UiClick>().0);
        // Hidden HUD (cinematics): the dial does nothing.
        app.world_mut().entity_mut(root).insert(Visibility::Hidden);
        app.world_mut().query_filtered::<&mut Ship, With<PlayerShip>>().single_mut(app.world_mut()).unwrap().order = 0;
        app.update();
        assert_eq!(app.world_mut().query_filtered::<&Ship, With<PlayerShip>>().single(app.world()).unwrap().order, 0);
    }

    #[test]
    fn pause_art_follows_the_pause() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_systems(Update, pause_art);
        let running = ImageNode::new(bevy::asset::uuid_handle!("00000000-0000-0000-0000-000000000001"));
        let paused = ImageNode::new(bevy::asset::uuid_handle!("00000000-0000-0000-0000-000000000002"));
        let button = app.world_mut().spawn((PauseArt { running: running.clone(), paused: paused.clone() }, running.clone())).id();
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
        app.update();
        assert_eq!(app.world().get::<ImageNode>(button).unwrap().image, paused.image);
        app.world_mut().resource_mut::<Time<Virtual>>().unpause();
        app.update();
        assert_eq!(app.world().get::<ImageNode>(button).unwrap().image, running.image);
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn every_class_has_its_hud_icons() {
        // Class names differ from the icon art (Ship_Navy_ManOWar -> Navy_Man_O_War); the WOT icon name resolves them all.
        let comps: std::sync::Arc<serde_json::Value> = crate::data::read_json(&assets_dir().join("hud/hud.json")).into();
        let lib = crate::Library::load(assets_dir());
        for class in lib.classes.iter().filter(|c| c.name.starts_with("Ship_")) {
            let data = HudData { comps: comps.clone(), skin: crate::sprites::skin_of(&class.race) };
            for bar in ["ShipBar", "WeaponBar"] {
                assert!(sprite_of(&data, &class.icon(bar)).is_some(), "{} lacks {}", class.name, class.icon(bar));
            }
        }
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn every_faction_has_the_bar_parts_it_needs() {
        let comps: std::sync::Arc<serde_json::Value> = crate::data::read_json(&assets_dir().join("hud/hud.json")).into();
        for skin in ["Navy", "Pirate", "Procyon"] {
            let data = HudData { comps: comps.clone(), skin };
            for (part, _) in BarParts::load().0 {
                assert!(sprite_of(&data, &part).is_some(), "{skin} lacks {part}");
            }
        }
    }
}
