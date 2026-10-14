//! Classic (retail) in-game HUD: the main bar, the compass with its speed dial, the talking head and the weapon panel, as trees
//! of Bevy UI nodes. Behaviour (clicks, gauge values, contacts, status icon, talking-head state) is in the shared `panels.rs`.
//!
//! Sprites come from the retail `hud.hdt` (`hud/hud.json`) in the art set of the player's faction (`HudData::skin`). Sizes are in the
//! 640x480 reference pixels of that data, multiplied by `window height / REF_H` when the tree is built; the tree is rebuilt when the ship,
//! faction or window size changes. Layout is done by the UI engine (flex rows and anchored containers), so the bar needs no per-faction
//! numbers: it is the list of parts below, each as wide and tall as its sprite.
#![allow(deprecated)] // shortcut: bevy 0.20 Interaction (deprecated for picking Hovered/Pressed); port with menu.rs when it is removed
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use std::collections::HashMap;

use crate::combat::{self, GunStates, Health};
use crate::panels::{
    Contact, DockingStatus, Gauge, GunButton, HeadState, HudRoot, HudSet, MAX_CONTACTS, PanicButton, PauseButton, REF_H, Radar, RadarButton, ShipIconButton, Solid, ToggleButton,
    WeaponPanel, gauge_value, px,
};
use crate::sprites::{HudData, inset, offset, sheet_scale, sprite_area, sprite_of, sprite_trimmed, src};
use crate::{Library, PlayerShip, Ship};

/// Ordered bar parts and optional decorative backdrops, editable through the asset overlay.
#[derive(Resource)]
struct BarParts(Vec<(String, Option<String>)>);
impl BarParts {
    fn load() -> Self {
        let path = crate::assets_dir().join("data/hud/bar.json");
        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("HUD bar layout")).expect("invalid HUD bar JSON");
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
/// PROVISIONAL: the exe cycles the voice box through its Start..Stop frames while speaking; the cadence was not recovered.
const VOICE_FPS: f32 = 8.0;
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
/// The radar disk's idle/hover/pressed art (the hover and pressed art live on another sheet).
#[derive(Component)]
struct RadarFace([(Handle<Image>, Rect); 3]);

pub fn plugin(app: &mut App) {
    app.insert_resource(BarParts::load()).add_systems(
        Update,
        (
            build.before(HudSet).after(crate::sprites::choose_skin).run_if(in_state(crate::menu::Screen::Battle)),
            (overlay, update_dial, update_sweep, radar_face, update_gauges, update_guns, head_art, pause_art, fleet_art).after(HudSet),
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
    let tallest = parts.iter().map(|i| c.bar_size(&bar.0[*i].0).y).fold(0.0, f32::max);
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
    {
        let dock = commands
            .spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), bottom: px(-1.0, s), justify_content: JustifyContent::Center, ..default() },
                ChildOf(root),
            ))
            .id();
        let row = commands.spawn((Node { flex_direction: FlexDirection::Row, align_items: AlignItems::FlexEnd, ..default() }, ChildOf(dock))).id();
        for (n, item) in items.iter().enumerate() {
            let gap = if n == 0 { UiRect::DEFAULT } else { UiRect::left(px(-overlap, s)) };
            match item {
                Item::Part(i) => spawn_part(&mut commands, &c, class, row, &bar.0[*i].0, tallest, Node { margin: gap, ..default() }),
                Item::Wing(strip, members) => {
                    let w: f32 = members.iter().map(|i| c.bar_size(&bar.0[*i].0).x - overlap).sum::<f32>() + overlap;
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
                                    bottom: Val::Px(0.0),
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
                        let node = Node { position_type: PositionType::Absolute, left: px(x, s), bottom: Val::Px(0.0), ..default() };
                        spawn_part(&mut commands, &c, class, wing, &bar.0[*i].0, tallest, node);
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
        let face = ["NoCursor", "CursorOver", "CursorDown"].map(|k| {
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
    if let Some((img, sz)) = c.sprite("WeaponBar_ToggleButton", false) {
        commands.spawn((ToggleButton, img, c.abs(4.0, -38.0, sz), ChildOf(compass), solid()));
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

    // ---- talking head (bottom-right corner): wooden housing with the portrait seen through its ring ---------------------------
    let housing = c.size_of("TalkingHead_Solid_Background");
    if housing.x > 0.0 {
        let names = || {
            ["TalkingHead_Portal_Closed"]
                .into_iter()
                .map(String::from)
                .chain((0..10).map(|i| format!("TalkingHead_Portal_ClosedToOpen{i}")))
                .chain(["TalkingHead_Portal_Open".into()])
        };
        let voice = ["Start", "Frame0", "Frame1", "Stop", "Off"].map(|n| format!("TalkingHead_Portal_VoiceBox_{n}"));
        commands.insert_resource(HeadFrames { iris: names().filter_map(|n| c.frame(&n)).collect(), voice: voice.iter().filter_map(|n| c.frame(n)).collect() });
        let head = commands
            .spawn((
                Node { position_type: PositionType::Absolute, right: Val::Px(0.0), bottom: Val::Px(0.0), width: px(housing.x, s), height: px(housing.y, s), ..default() },
                ChildOf(root),
            ))
            .id();
        let (face_at, iris_at) = (offset(&hud, "Right_TalkingHead_Character_From_TopLeft"), offset(&hud, "Right_TalkingHead_PortalOverlay_From_TopLeft"));
        let portal = Vec2::splat(80.0);
        let layer = |commands: &mut Commands, kind: Head, art: Option<(ImageNode, Vec2)>, at: Vec2, hidden: bool| {
            let Some((img, sz)) = art else { return };
            let vis = if hidden { Visibility::Hidden } else { Visibility::Inherited };
            commands.spawn((kind, img, c.abs(at.x, at.y, sz), vis, ChildOf(head)));
        };
        {
            // Draw order: portrait, iris, housing (its ring is transparent), then the light and voice box on the wood.
            layer(&mut commands, Head::Face, Some((ImageNode::default(), portal)), face_at, true);
            layer(&mut commands, Head::Iris, c.sprite("TalkingHead_Portal_Closed", false), iris_at, false);
            if let Some((img, sz)) = c.sprite("TalkingHead_Solid_Background", false) {
                commands.spawn((img, c.abs(0.0, 0.0, sz), ChildOf(head), solid()));
            }
            // PROVISIONAL placement, measured on the housing art: the light sits over its green lamp, the voice box in the name plate.
            layer(&mut commands, Head::Light, c.sprite("TalkingHead_Portal_Light_On", false), Vec2::new(75.0, 2.0), true);
            layer(&mut commands, Head::Voice, c.sprite("TalkingHead_Portal_VoiceBox_Off", false), Vec2::new(20.0, 103.0), false);
        }
    }

    {
        // ---- weapon panel (left edge, vertically centred) -----------------------------------------------------------------
        let wbg = c.size_of("WeaponBar_ShipIcon_Background");
        let weapon = commands
            .spawn((
                WeaponPanel,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px((window.height() - 255.0 * s) / 2.0),
                    width: px(wbg.x, s),
                    height: px(255.0, s),
                    ..default()
                },
                ChildOf(root),
            ))
            .id();
        if let Some((img, sz)) = c.sprite("WeaponBar_ShipIcon_Background", false) {
            commands.spawn((img, c.abs(0.0, 0.0, sz), ChildOf(weapon), solid()));
        }
        if let Some((img, sz)) = c.sprite(&format!("WeaponBar_ShipIcon_{}", class.name.trim_start_matches("Ship_")), false) {
            let o = offset(&hud, "WEAPONBAR_SHIPICON_OFFSET_FROM_TOPLEFT");
            commands.spawn((img, c.abs(o.x, o.y, sz), ChildOf(weapon)));
        }
        for (bank, first) in &banks {
            let pos = offset(&hud, &format!("WeaponBar_SlotOffset_From_TopLeft_{bank:02}"));
            let icon_at = pos + offset(&hud, "Offset_WeaponBar_GunIcon");
            if let Some((img, sz)) = c.sprite("WeaponBar_GunBankCanHitTarget", false) {
                commands.spawn((GunSlot { bank: *bank, icon: false }, img, c.abs(icon_at.x, icon_at.y, sz), ChildOf(weapon)));
            }
            if let Some((img, sz)) = c.sprite(&combat::gun_icon(guns.gun(*first)), false) {
                commands.spawn((GunSlot { bank: *bank, icon: true }, GunButton(*bank), img, c.abs(icon_at.x, icon_at.y, sz), ChildOf(weapon), solid()));
            }
            // Recharge bar beside the icon: the empty bar (WeaponBar_SlotBackground) with the yellow WeaponBar_RechargeStatus growing from the bottom.
            // PROVISIONAL: drawing order/fill direction are read from the artwork; the exe's drawing code was not found.
            if let (Some((outline, osz)), Some((img, fsz))) = (c.sprite("WeaponBar_SlotBackground", false), c.sprite("WeaponBar_RechargeStatus", false)) {
                let at = pos + offset(&hud, "Offset_WeaponBar_Recharge");
                commands.spawn((outline, c.abs(at.x, at.y, osz), ChildOf(weapon)));
                let base = img.rect.unwrap();
                let k = sprite_of(&hud, "WeaponBar_RechargeStatus").map_or(1.0, |(sheet, _)| sheet_scale(&sheet));
                commands.spawn((Fill { gauge: Gauge::Recharge(*bank), full: fsz.y, top: at.y, base, k }, img, c.abs(at.x, at.y, fsz), ChildOf(weapon)));
            }
        }
    }
    commands.insert_resource(frames);
}

/// One bar part plus whatever sits on it: the ship icon on the ship frame, a fill on each status gauge.
fn spawn_part(commands: &mut Commands, c: &Ctx, class: &combat::Class, parent: Entity, name: &str, tallest: f32, mut node: Node) {
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
    // Offsets in the data are from the top of the bar; parts are bottom-aligned.
    let from_part = tallest - size.y;
    let fill = |commands: &mut Commands, sprite: &str, offset_name: &str, gauge: Gauge| {
        let Some((img, fsize)) = c.sprite(sprite, false) else { return };
        let o = offset(c.hud, offset_name);
        let top = o.y - from_part;
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
            if let Some((img, isz)) = c.sprite(&format!("ShipBar_ShipIcon_{}", class.name.trim_start_matches("Ship_")), false) {
                let o = offset(c.hud, "MainBar_ShipIcon_Offset_From_Background_TopLeft");
                let portrait = commands.spawn((img, c.abs(o.x, o.y - from_part, isz), ChildOf(part))).id();
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
        img.image = h.clone();
        img.rect = Some(*r);
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
        let set = |img: &mut ImageNode, (h, r): &(Handle<Image>, Rect)| {
            img.image = h.clone();
            img.rect = Some(*r);
        };
        match kind {
            Head::Iris => set(&mut img, &frames.iris[iris_frame(open, frames.iris.len())]),
            Head::Voice => set(&mut img, &frames.voice[if speaking && open >= 1.0 { cycle } else { 4 }]),
            Head::Face => {
                if let Some((sheet, r)) = head.speaker.as_deref().and_then(|n| sprite_of(&hud, n)) {
                    img.image = assets.load(format!("hud/tex/{sheet}.png"));
                    img.rect = Some(inset(&sheet, r));
                }
                *vis = if open > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
            }
            Head::Light => *vis = if speaking && open >= 1.0 { Visibility::Inherited } else { Visibility::Hidden },
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
        img.image = h.clone();
        img.rect = Some(*r);
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
        if img.image != *h || img.rect != Some(*r) {
            img.image = h.clone();
            img.rect = Some(*r);
        }
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
        node.top = px(f.top + f.full - h, s);
        node.height = px(h, s);
        let mut r = f.base;
        r.min.y = r.max.y - h * f.k;
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
                img.image = h.clone();
                img.rect = Some(*r);
            }
            // Dimmed while the bank reloads (the bar beside it shows how far along it is).
            img.color = match st {
                2 => Color::srgba(1.0, 1.0, 1.0, 0.25),
                1 => Color::srgb(0.55, 0.55, 0.55),
                _ => Color::WHITE,
            };
        } else if let Some((h, r)) = frames.gun.get(st) {
            img.image = h.clone();
            img.rect = Some(*r);
            img.color = if armed { Color::srgb(1.0, 0.9, 0.3) } else { Color::WHITE };
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

#[derive(Component)]
struct FleetIcon(usize);
#[derive(Component)]
struct FleetHealth(usize);
#[derive(Component)]
struct FleetLabel(usize);

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn fleet_art(
    mut commands: Commands,
    assets: Res<AssetServer>,
    hud: Res<HudData>,
    model: Res<crate::panels::FleetHud>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    roots: Query<Entity, With<HudRoot>>,
    mut windows: Query<(&crate::panels::FleetWindow, &mut Visibility), (Without<crate::panels::FleetButton>, Without<crate::panels::SelectedName>)>,
    mut buttons: Query<(&crate::panels::FleetButton, &Interaction, &mut ImageNode, &mut Visibility, &mut Node), Without<crate::panels::FleetWindow>>,
    mut icons: Query<(&FleetIcon, &mut ImageNode), Without<crate::panels::FleetButton>>,
    mut health: Query<(&FleetHealth, &mut Node, &mut BackgroundColor), Without<crate::panels::FleetButton>>,
    mut labels: Query<(&FleetLabel, &mut Text), Without<crate::panels::SelectedName>>,
    mut name: Query<(&mut Text, &mut Visibility), (With<crate::panels::SelectedName>, Without<crate::panels::FleetWindow>, Without<crate::panels::FleetButton>)>,
    mut help: Query<(&crate::panels::FleetButton, &mut Text), (Without<FleetLabel>, Without<crate::panels::SelectedName>)>,
) {
    use crate::panels::{FleetButton as B, FleetWindow, SelectedName};
    let s = window.height() / REF_H;
    if windows.is_empty() {
        let Some(root) = roots.iter().next() else { return };
        let button = |commands: &mut Commands, parent, action, art: &str, node| {
            let img = crate::sprites::image(&assets, &hud, art).map(|p| p.0).unwrap_or_default();
            commands.spawn((action, img, node, Solid, Interaction::None, FocusPolicy::Block, ChildOf(parent))).id()
        };
        button(
            &mut commands,
            root,
            B::Ships,
            "ShipBar_ToggleButton",
            Node { position_type: PositionType::Absolute, right: px(8.0, s), bottom: px(82.0, s), width: px(44.0, s), height: px(32.0, s), ..default() },
        );
        let bar = commands
            .spawn((
                FleetWindow(true),
                Node { position_type: PositionType::Absolute, right: px(8.0, s), bottom: px(120.0, s), width: px(112.0, s), height: px(280.0, s), ..default() },
                ChildOf(root),
            ))
            .id();
        for i in 0..10 {
            let slot = button(
                &mut commands,
                bar,
                B::Slot(i),
                "ShipBar_SlotBackground",
                Node {
                    position_type: PositionType::Absolute,
                    left: px((i % 2) as f32 * 56.0, s),
                    bottom: px((i / 2) as f32 * 56.0, s),
                    width: px(52.0, s),
                    height: px(52.0, s),
                    ..default()
                },
            );
            commands.spawn((
                FleetIcon(i),
                ImageNode::default(),
                Node { position_type: PositionType::Absolute, left: px(8.0, s), top: px(4.0, s), width: px(40.0, s), height: px(36.0, s), ..default() },
                ChildOf(slot),
            ));
            commands.spawn((
                FleetHealth(i),
                Node { position_type: PositionType::Absolute, left: px(2.0, s), bottom: px(2.0, s), width: px(4.0, s), ..default() },
                BackgroundColor(Color::srgb(0.0, 1.0, 0.0)),
                ChildOf(slot),
            ));
            commands.spawn((
                FleetLabel(i),
                Text::new(""),
                TextFont { font_size: FontSize::Px(10.0 * s), ..default() },
                Node { position_type: PositionType::Absolute, bottom: Val::Px(0.0), left: px(9.0, s), ..default() },
                ChildOf(slot),
            ));
        }
        let bar = commands
            .spawn((
                FleetWindow(false),
                Node { position_type: PositionType::Absolute, left: Val::Percent(30.0), bottom: px(82.0, s), flex_direction: FlexDirection::Row, ..default() },
                ChildOf(root),
            ))
            .id();
        for kind in crate::ship::Formation::BUTTONS {
            button(&mut commands, bar, B::Formation(kind), &format!("FormationBar_Icon_{kind:?}"), Node { width: px(44.0, s), height: px(36.0, s), ..default() });
        }
        for (action, text) in [(B::Attack, "Attack"), (B::CeaseFire, "Cease / resume")] {
            commands.spawn((
                action,
                Text::new(text),
                TextFont { font_size: FontSize::Px(14.0 * s), ..default() },
                Solid,
                Interaction::None,
                FocusPolicy::Block,
                Node { padding: UiRect::all(px(5.0, s)), ..default() },
                ChildOf(bar),
            ));
        }
        commands.spawn((
            SelectedName,
            Text::new(""),
            TextFont { font_size: FontSize::Px(18.0 * s), ..default() },
            Node { position_type: PositionType::Absolute, top: px(40.0, s), left: Val::Percent(40.0), ..default() },
            ChildOf(root),
        ));
    }
    for (w, mut v) in &mut windows {
        *v = if if w.0 { model.ships } else { model.commands } { Visibility::Inherited } else { Visibility::Hidden };
    }
    for (b, interaction, mut img, mut v, mut node) in &mut buttons {
        if let B::Slot(i) = b {
            let row = model.rows.get(*i);
            *v = if model.ships && row.is_some() { Visibility::Inherited } else { Visibility::Hidden };
            if let Some(row) = row {
                if let Some((art, _)) = crate::sprites::image(&assets, &hud, if row.selected { "ShipBar_Selected_SlotBackground" } else { "ShipBar_SlotBackground" }) {
                    *img = art;
                }
                img.color = if *interaction == Interaction::Pressed {
                    Color::srgb(0.7, 0.7, 0.7)
                } else if *interaction == Interaction::Hovered {
                    Color::srgb(1.0, 0.9, 0.7)
                } else {
                    Color::WHITE
                };
                node.left = px(if *i + 1 == model.rows.len() && i % 2 == 0 { 28.0 } else { (i % 2) as f32 * 56.0 }, s);
            }
        } else if let B::Formation(kind) = b {
            img.color = if model.active == Some(*kind) { Color::srgb(1.0, 0.9, 0.15) } else { Color::WHITE };
        }
    }
    for (FleetIcon(i), mut img) in &mut icons {
        if let Some(row) = model.rows.get(*i)
            && let Some((art, _)) = crate::sprites::image(&assets, &hud, &row.icon)
        {
            *img = art;
        }
    }
    for (FleetHealth(i), mut node, mut color) in &mut health {
        if let Some(row) = model.rows.get(*i) {
            node.height = px(48.0 * row.health, s);
            color.0 = Color::srgb(1.0 - row.health, row.health, 0.0);
        }
    }
    for (FleetLabel(i), mut text) in &mut labels {
        text.set_if_neq(Text::new(model.rows.get(*i).map_or_else(String::new, |r| format!("{} {}", (*i + 1) % 10, r.status))));
    }
    for (b, mut text) in &mut help {
        if *b == B::CeaseFire {
            text.set_if_neq(Text::new(if model.cease_fire { "Resume fire" } else { "Cease fire" }));
        }
    }
    for (mut text, mut v) in &mut name {
        text.set_if_neq(Text::new(model.name.clone()));
        *v = if model.shown { Visibility::Inherited } else { Visibility::Hidden };
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
    fn speed_segments_and_frames_match_engine_orders() {
        for (order, centre) in [(0, Vec2::new(54.0, 71.0)), (1, Vec2::new(44.0, 57.0)), (2, Vec2::new(31.0, 44.0)), (3, Vec2::new(14.0, 34.0))] {
            assert_eq!(crate::panels::speed_order_at(centre), order);
            assert_eq!(crate::panels::speed_order_at(centre + Vec2::splat(1.0)), order);
        }
        let data = HudData { comps: serde_json::from_str(&std::fs::read_to_string(assets_dir().join("hud/hud.json")).unwrap()).unwrap(), skin: "Navy" };
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
    fn every_faction_has_the_bar_parts_it_needs() {
        let comps: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(assets_dir().join("hud/hud.json")).unwrap()).unwrap();
        for skin in ["Navy", "Pirate", "Procyon"] {
            let data = HudData { comps: comps.clone(), skin };
            for (part, _) in BarParts::load().0 {
                assert!(sprite_of(&data, &part).is_some(), "{skin} lacks {part}");
            }
        }
    }
}
