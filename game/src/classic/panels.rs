//! Classic (retail) in-game HUD: the main bar, the compass with its speed dial, the talking head and the weapon panel, as trees
//! of Bevy UI nodes. Behaviour (clicks, gauge values, contacts, status icon, talking-head state) is in the shared `panels.rs`.
//!
//! Sprites come from the retail `hud.hdt` (`hud/hud.json`) in the art set of the player's faction (`HudData::skin`). Sizes are in the
//! 640x480 reference pixels of that data, multiplied by `window height / REF_H` when the tree is built; the tree is rebuilt when the ship,
//! faction or window size changes. Every node edge is rounded to whole physical pixels (`panels::snap`). The main bar is one image per
//! faction composed at import (`tools/compose_bar.py`: `hud/bar/<skin>.png` and the part boxes in `.json`); only what moves is drawn on
//! it: the buttons' hover and pressed art and the gauge fills. Parts share their top edge (the data's gauge offsets are "from MainBar
//! TopLeft"), and the bar is docked with its shortest part's bottom on the screen edge.
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use std::collections::HashMap;

use crate::combat::{self, GunStates, Health};
use crate::panels::{
    Contact, DockingStatus, Gauge, GunButton, HeadState, HudRoot, HudSet, MAX_CONTACTS, PanicButton, PauseButton, Radar, RadarButton, ShipIconButton, Solid, ToggleButton,
    WeaponPanel, gauge_value, hud_scale, px,
};
use crate::pointer::Pointer;
use crate::sprites::{HudData, IDLE, Sprite, offset, size_of, sprite, sprite_state};
use crate::{Library, PlayerShip, Ship};

/// The composed main bar of an art set (`hud/bar/<skin>.json` next to its `.png`): the image holds every part's idle art, the parts
/// are the boxes the dynamic pieces sit on. Reference pixels.
/// The composed main bar of one faction (`hud/bar/<skin>.json`): its size and the parts on it, in reference pixels.
#[derive(serde::Deserialize, Default, Debug, PartialEq)]
#[serde(from = "RawBar")]
struct BarLayout {
    size: Vec2,
    parts: Vec<BarPart>,
}
#[derive(Debug, PartialEq)]
struct BarPart {
    sprite: String,
    x: f32,
    size: Vec2,
}
/// The file's fields (`width` and `height` beside the parts); unknown fields are allowed.
#[derive(serde::Deserialize)]
struct RawBar {
    width: f32,
    height: f32,
    parts: Vec<RawPart>,
}
#[derive(serde::Deserialize)]
struct RawPart {
    sprite: String,
    x: f32,
    width: f32,
    height: f32,
}
impl From<RawBar> for BarLayout {
    fn from(r: RawBar) -> Self {
        let parts = r.parts.into_iter().map(|p| BarPart { sprite: p.sprite, x: p.x, size: Vec2::new(p.width, p.height) }).collect();
        Self { size: Vec2::new(r.width, r.height), parts }
    }
}
/// The bars read so far, by skin.
#[derive(Resource, Default)]
struct BarLayouts(Vec<(String, std::sync::Arc<BarLayout>)>);

impl BarLayouts {
    fn get(&mut self, skin: &str) -> std::sync::Arc<BarLayout> {
        if let Some((_, layout)) = self.0.iter().find(|(s, _)| s == skin) {
            return layout.clone();
        }
        let layout = BarLayout::load(skin);
        self.0.push((skin.to_string(), layout.clone()));
        layout
    }
}

impl BarLayout {
    fn path(skin: &str) -> String {
        format!("hud/bar/{}", skin.to_lowercase())
    }
    /// The skin's bar. A file that does not read is reported by `data::load_asset` (file, mod, line and column); the retail file
    /// stands in for it, and with none the bar is empty (nothing drawn) rather than a crash. The HUD systems read it through
    /// `BarLayouts`, once per skin.
    fn load(skin: &str) -> std::sync::Arc<Self> {
        std::sync::Arc::new(crate::data::load_asset::<BarLayout>(&format!("{}.json", Self::path(skin)), "the main bar is not drawn").unwrap_or_default())
    }
    /// The layout as the code read it before it was typed: `Value` walking with a panic per missing field.
    #[cfg(test)]
    fn untyped(value: &serde_json::Value) -> Self {
        let num = |v: &serde_json::Value, key: &str| v[key].as_f64().unwrap_or_else(|| panic!("HUD bar: `{key}` must be a number")) as f32;
        let parts = value["parts"]
            .as_array()
            .expect("HUD bar needs parts")
            .iter()
            .map(|p| BarPart { sprite: p["sprite"].as_str().expect("HUD bar part needs a sprite").to_string(), x: num(p, "x"), size: Vec2::new(num(p, "width"), num(p, "height")) })
            .collect();
        Self { size: Vec2::new(num(value, "width"), num(value, "height")), parts }
    }
}

/// A main-bar button: its box is drawn by the composed bar, so it shows art only while hovered or pressed (or, for PAUSE, while paused).
#[derive(Component)]
struct BarButton {
    /// Idle / hover / pressed art.
    states: States,
    /// The same for the paused game, on the PAUSE button.
    paused: Option<States>,
    /// The same for the advanced (waypoint-plotting) movement type, on the movement-type button: shown, always, while it is chosen.
    advanced: Option<States>,
    /// The part's box in reference pixels (art past its right edge is not part of the layout).
    region: Rect,
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
    iris: Vec<Sprite>,
    /// `VoiceBox_Start`, `Frame0`, `Frame1`, `Stop`, then `Off`.
    voice: Vec<Sprite>,
    /// Reference-to-screen scale the HUD was built at.
    s: f32,
}
/// Iris frame for open progress 0..=1.
fn iris_frame(progress: f32, frames: usize) -> usize {
    (progress.clamp(0.0, 1.0) * (frames - 1) as f32).round() as usize
}
#[derive(Component)]
struct Sweep;
/// A gauge fill that grows from the bottom: `top` is the y (reference px, inside the parent) of its sprite's box when full.
#[derive(Component)]
struct Fill {
    gauge: Gauge,
    art: Sprite,
    top: f32,
}
/// Weapon-panel slot of a gun bank: its status frame, or the gun's icon when `icon`.
#[derive(Component)]
struct GunSlot {
    bank: usize,
    icon: bool,
}

/// Sprites that change at runtime, and the scale the HUD was built at.
#[derive(Resource)]
struct Frames {
    gun: Vec<Sprite>,
    dial: Vec<Sprite>,
    sweep: Vec<Sprite>,
    icons: HashMap<String, Sprite>,
    s: f32,
}
/// Idle / hover / pressed art of a button, swapped by its own `Interaction` (`s`: the scale it was built at).
#[derive(Component, Clone)]
pub(super) struct States {
    art: [Sprite; 3],
    s: f32,
}
/// The radar disk's idle/hover/pressed art.
#[derive(Component)]
struct RadarFace(States);

pub fn plugin(app: &mut App) {
    app.init_resource::<BarLayouts>().add_systems(PreUpdate, chat_capture_wheel.after(bevy::ui::UiSystems::Focus).run_if(in_state(crate::menu::Screen::Battle))).add_systems(
        Update,
        (
            build.before(HudSet).after(crate::sprites::choose_skin).run_if(in_state(crate::menu::Screen::Battle)),
            (overlay, chat_panel, chat_scroll, update_dial, update_sweep, radar_face, button_states, update_gauges, update_guns, head_art, bar_button_art, fleet_art, name_plate)
                .after(HudSet),
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
pub(super) struct Ctx<'a> {
    pub(super) assets: &'a AssetServer,
    pub(super) hud: &'a HudData,
    pub(super) s: f32,
}
impl Ctx<'_> {
    /// The idle sprite of a component in the player's art set.
    pub(super) fn sprite(&self, name: &str) -> Option<Sprite> {
        sprite(self.assets, self.hud, name)
    }
    /// Image and absolutely positioned node drawing `art` at reference position `(x, y)`, trimmed art placed where the untrimmed
    /// sprite would be.
    pub(super) fn put(&self, art: &Sprite, x: f32, y: f32) -> (ImageNode, Node) {
        self.put_box(art, x, y, art.size)
    }
    /// `put` with the sprite's whole box stretched over `to` reference px.
    pub(super) fn put_box(&self, art: &Sprite, x: f32, y: f32, to: Vec2) -> (ImageNode, Node) {
        let mut node = Node { position_type: PositionType::Absolute, left: px(x, self.s), top: px(y, self.s), ..default() };
        art.fit(to).place(&mut node, self.s, UiRect::DEFAULT);
        (art.image_node(), node)
    }
    /// The NoCursor / CursorOver / CursorDown art of a sprite (a missing state uses the idle art).
    pub(super) fn states(&self, name: &str) -> Option<States> {
        let art = [IDLE, "CursorOver", "CursorDown"].map(|k| sprite_state(self.assets, self.hud, name, k));
        let [Some(a), Some(b), Some(c)] = art else { return None };
        Some(States { art: [a, b, c], s: self.s })
    }
    /// A button: its decorative arm (`detail`, drawn first) and the button sprite at `at` inside it, with hover/press art.
    #[allow(deprecated)] // Interaction, see pointer.rs
    fn toggle(&self, commands: &mut Commands, parent: Entity, detail: &str, detail_at: Vec2, button: &str, at: Vec2, marker: impl Bundle) {
        if let Some(art) = self.sprite(detail) {
            commands.spawn((self.put(&art, detail_at.x, detail_at.y), ChildOf(parent), Solid, Interaction::None, FocusPolicy::Block));
        }
        if let (Some(art), Some(states)) = (self.sprite(button), self.states(button)) {
            let at = detail_at + at;
            commands.spawn((marker, self.put(&art, at.x, at.y), states, ChildOf(parent), Solid, Interaction::None, FocusPolicy::Block));
        }
    }
    fn frame(&self, name: &str) -> Option<Sprite> {
        self.sprite(name)
    }
    /// Untrimmed reference size of a sprite (zero when the art set lacks it).
    pub(super) fn size_of(&self, name: &str) -> Vec2 {
        size_of(self.hud, name).unwrap_or(Vec2::ZERO)
    }
    /// Absolutely positioned node at reference position `(x, y)`.
    pub(super) fn abs(&self, x: f32, y: f32, size: Vec2) -> Node {
        let snap = |at: f32, len: f32| (px(at, self.s), Val::Px(crate::panels::snap(at + len, self.s) - crate::panels::snap(at, self.s)));
        let ((left, width), (top, height)) = (snap(x, size.x), snap(y, size.y));
        Node { position_type: PositionType::Absolute, left, top, width, height, ..default() }
    }
}

/// (Re)builds the whole HUD tree.
#[allow(deprecated)] // Interaction, see pointer.rs
fn build(
    mut commands: Commands,
    assets: Res<AssetServer>,
    hud: Res<HudData>,
    lib: Res<Library>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    fonts: Option<Res<crate::sprites::Fonts>>,
    player: Single<(&Ship, &GunStates), With<PlayerShip>>,
    old: Query<Entity, With<HudRoot>>,
    mut last: Local<Option<(usize, &'static str, u32, Vec<String>)>>,
    speed: Option<Res<crate::panels::SpeedControl>>,
    mut bars: ResMut<BarLayouts>,
) {
    let s = hud_scale(&window);
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
    let c = Ctx { assets: &assets, hud: &hud, s };
    let solid = || (Interaction::None, FocusPolicy::Block, Solid);

    let root = commands
        .spawn((crate::Scenario, HudRoot, Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, GlobalZIndex(10)))
        .id();

    // ---- main bar: the composed image bottom-docked and centred, the dynamic parts on their boxes ---------------------------
    // PROVISIONAL: which bottom rows retail shows is not measured per faction; the composed bar ends on its shortest part.
    let layout = bars.get(hud.skin);
    let (bar_w, floor) = (layout.size.x, layout.size.y);
    commands_window(&mut commands, &c, root, window.width() / s, bar_w, floor);
    if speed.is_some() {
        speed_plate(&mut commands, &c, fonts.as_deref(), root, (window.width() / s + bar_w) / 2.0);
    }
    {
        // Centred on a whole pixel: half a pixel of slack would blur the whole image.
        let width = crate::panels::snap(bar_w, s);
        let node = Node {
            position_type: PositionType::Absolute,
            left: Val::Px(((window.width() - width) / 2.0).floor()),
            bottom: Val::Px(0.0),
            width: Val::Px(width),
            height: px(floor, s),
            ..default()
        };
        let bar = commands.spawn((node, ChildOf(root))).id();
        let mut art = ImageNode::new(assets.load(format!("{}.png", BarLayout::path(hud.skin))));
        art.image_mode = NodeImageMode::Stretch;
        let fill = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
        commands.spawn((art, fill, ChildOf(bar), solid(), Name::new("MainBar")));
        for part in &layout.parts {
            spawn_part(&mut commands, &c, class, bar, part, floor);
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
    if let (Some(art), Some(face)) = (c.sprite("Compass_Background"), c.states("Compass_Background")) {
        commands.spawn((c.put(&art, bg_pos.x, bg_pos.y), RadarFace(face), ChildOf(compass)));
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
        s,
    };
    if let Some(art) = c.sprite("HUDRadar_Sweep_Frame0") {
        let o = offset(&hud, "RADAR_SWEEP_OFFSET_FROM_TOPLEFT");
        commands.spawn((Sweep, c.put(&art, o.x, o.y), ChildOf(compass)));
    }
    for (friendly, icon) in [(false, "HUDRADAR_ICON_ENEMYSHIP_FRAME0"), (true, "HUDRADAR_ICON_FRIENDLYSHIP_FRAME0")] {
        if let Some(art) = c.sprite(icon) {
            let (img, node) = c.put(&art, 0.0, 0.0);
            for i in 0..MAX_CONTACTS {
                commands.spawn((Contact(i, friendly), img.clone(), node.clone(), Visibility::Hidden, ChildOf(compass)));
            }
        }
    }
    if let Some(art) = c.sprite("COMPASS_OVERLAY") {
        commands.spawn((c.put(&art, 0.0, 0.0), ChildOf(compass), solid()));
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
        if let Some(art) = c.sprite("SpeedControl_CurrActive0_NextActive0") {
            // The clickable box is the whole reference box (the dial input reads the cursor inside it); the art sits in it by its trim.
            let sz = art.size;
            let dial = commands
                .spawn((
                    crate::panels::EngineDial,
                    Node { position_type: PositionType::Absolute, left: px(76.0, s), bottom: px(overlay.y + 44.0 - sz.y, s), width: px(sz.x, s), height: px(sz.y, s), ..default() },
                    RelativeCursorPosition::default(),
                    ChildOf(root),
                    solid(),
                ))
                .id();
            commands.spawn((Dial, c.put(&art, 0.0, 0.0), ChildOf(dial)));
        }
    }

    talking_head(&mut commands, &c, root);
    weapon_panel(&mut commands, &c, root, class, &banks, guns, window.height());
    commands.insert_resource(frames);
}

/// The talking head (bottom-right corner): wooden housing with the portrait seen through its ring, the iris, light and voice box,
/// and the ship-bar toggle on its corner.
#[allow(deprecated)] // Interaction, see pointer.rs
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
    commands.insert_resource(HeadFrames { iris: names().filter_map(|n| c.frame(&n)).collect(), voice: voice.iter().filter_map(|n| c.frame(n)).collect(), s });
    let head = commands
        .spawn((
            Node { position_type: PositionType::Absolute, right: Val::Px(0.0), bottom: Val::Px(0.0), width: px(housing.x, s), height: px(housing.y, s), ..default() },
            ChildOf(root),
        ))
        .id();
    let (face_at, iris_at) = (offset(hud, "Right_TalkingHead_Character_From_TopLeft"), offset(hud, "Right_TalkingHead_PortalOverlay_From_TopLeft"));
    let portal = Vec2::splat(80.0);
    let layer = |commands: &mut Commands, kind: Head, art: Option<Sprite>, at: Vec2, hidden: bool| {
        let Some(art) = art else { return };
        let vis = if hidden { Visibility::Hidden } else { Visibility::Inherited };
        commands.spawn((kind, c.put(&art, at.x, at.y), vis, ChildOf(head)));
    };
    {
        // Draw order: portrait, iris, housing (its ring is transparent), then the light and voice box on the wood.
        commands.spawn((Head::Face, ImageNode::default(), c.abs(face_at.x, face_at.y, portal), Visibility::Hidden, ChildOf(head)));
        layer(commands, Head::Iris, c.sprite("TalkingHead_Portal_Closed"), iris_at, false);
        if let Some(art) = c.sprite("TalkingHead_Solid_Background") {
            commands.spawn((c.put(&art, 0.0, 0.0), ChildOf(head), solid()));
        }
        // PROVISIONAL placement, measured on the housing art: the light sits over its green lamp, the voice box in the name plate.
        // The art differs per faction (the pirate housing is wider, its portal further right): the lamp stays on the housing's right
        // edge and the name plate follows the portal.
        layer(commands, Head::Light, c.sprite("TalkingHead_Portal_Light_On"), Vec2::new(housing.x - 52.0, 2.0), true);
        layer(commands, Head::Voice, c.sprite("TalkingHead_Portal_VoiceBox_Off"), Vec2::new(13.0 + iris_at.x, 103.0), false);
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
#[allow(deprecated)] // Interaction, see pointer.rs
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
    if let Some(art) = c.sprite("WeaponBar_ShipIcon_Background") {
        commands.spawn((c.put(&art, 0.0, 0.0), ChildOf(weapon), solid()));
    }
    if let Some(art) = c.sprite(&class.icon("WeaponBar")) {
        let o = offset(hud, "WEAPONBAR_SHIPICON_OFFSET_FROM_TOPLEFT");
        commands.spawn((c.put(&art, o.x, o.y), ChildOf(weapon)));
    }
    for (bank, first) in banks {
        let pos = offset(hud, &format!("WeaponBar_SlotOffset_From_TopLeft_{bank:02}"));
        let icon_at = pos + offset(hud, "Offset_WeaponBar_GunIcon");
        if let Some(art) = c.sprite("WeaponBar_GunBankCanHitTarget") {
            commands.spawn((GunSlot { bank: *bank, icon: false }, c.put(&art, icon_at.x, icon_at.y), ChildOf(weapon)));
        }
        if let Some(art) = c.sprite(&combat::gun_icon(guns.gun(*first))) {
            commands.spawn((GunSlot { bank: *bank, icon: true }, GunButton(*bank), c.put(&art, icon_at.x, icon_at.y), ChildOf(weapon), solid()));
        }
        // Recharge bar beside the icon: the empty bar (WeaponBar_SlotBackground) with the yellow WeaponBar_RechargeStatus growing from the bottom.
        // PROVISIONAL: drawing order/fill direction are read from the artwork; the original's drawing order was not observed.
        if let (Some(outline), Some(fill)) = (c.sprite("WeaponBar_SlotBackground"), c.sprite("WeaponBar_RechargeStatus")) {
            let at = pos + offset(hud, "Offset_WeaponBar_Recharge");
            commands.spawn((c.put(&outline, at.x, at.y), ChildOf(weapon)));
            commands.spawn((Fill { gauge: Gauge::Recharge(*bank), art: fill.clone(), top: at.y }, c.put(&fill, at.x, at.y), ChildOf(weapon)));
        }
    }
}

/// One bar part plus whatever sits on it: the ship icon and docking icon on the ship frame, a fill on each status gauge.
/// The box of one bar part: clicks, hover and pressed art; what is drawn on the part (gauge fills, portrait) is measured from its untrimmed top-left.
#[allow(deprecated)] // Interaction, see pointer.rs
fn spawn_part(commands: &mut Commands, c: &Ctx, class: &combat::Class, parent: Entity, part: &BarPart, floor: f32) {
    let name = part.sprite.as_str();
    let Some(art) = c.sprite(name) else { return };
    let size = Vec2::new(part.size.x, part.size.y.min(floor));
    let node = c.abs(part.x, 0.0, size);
    // Retail art without hover or pressed variants (the gauge troughs) is only a box.
    let states = c.states(name).filter(|st| st.art[1].image != st.art[0].image || st.art[2].image != st.art[0].image);
    let paused = (name == "MAINBAR_IS_NOT_PAUSED_BUTTON").then(|| c.states("MAINBAR_IS_PAUSED_BUTTON")).flatten();
    let advanced = (name == "MainBar_BeginnerMovementTypeButton").then(|| c.states("MainBar_AdvancedMovementTypeButton")).flatten();
    let mut img = art.image_node();
    img.color = Color::NONE;
    let part_entity = commands.spawn((node, ChildOf(parent), Solid, Interaction::None, FocusPolicy::Block)).id();
    if let Some(states) = states {
        commands.entity(part_entity).insert((img, BarButton { states, paused, advanced, region: Rect::from_corners(Vec2::ZERO, size) }));
    }
    if name == "MAINBAR_IS_NOT_PAUSED_BUTTON" {
        commands.entity(part_entity).insert(PauseButton);
    }
    if name == "MainBar_PanicButton" {
        commands.entity(part_entity).insert(PanicButton);
    }
    if name == "MainBar_ShipIcon" {
        commands.entity(part_entity).insert(ShipIconButton);
    }
    let fill = |commands: &mut Commands, sprite: &str, offset_name: &str, gauge: Gauge| {
        let Some(art) = c.sprite(sprite) else { return };
        let o = offset(c.hud, offset_name);
        // Gauge offsets count from the part's untrimmed top-left, so the fill hangs on the part itself (a child node would start at the sprite's trimmed art).
        commands.spawn((Fill { gauge, art: art.clone(), top: o.y }, c.put(&art, o.x, o.y), ChildOf(part_entity)));
    };
    if name == "MainBar_FleetFormationsButton" {
        commands.entity(part_entity).insert(crate::panels::FleetButton::Commands);
    }
    if name == "MainBar_ChatButton" {
        commands.entity(part_entity).insert(crate::panels::FleetButton::Chat);
    }
    if name == "MainBar_BeginnerMovementTypeButton" {
        commands.entity(part_entity).insert(crate::panels::MovementButton);
    }
    match name {
        "MainBar_ShipIcon" => {
            if let Some(icon) = c.sprite(&class.icon("ShipBar")) {
                let o = offset(c.hud, "MainBar_ShipIcon_Offset_From_Background_TopLeft");
                // Both offsets count from untrimmed top-lefts (the part's, then the portrait's), so both hang on the part itself.
                commands.spawn((c.put(&icon, o.x, o.y), ChildOf(part_entity)));
                if let Some(status) = c.sprite("ShipStatus_Icon_Docking_Frame0") {
                    let at = o + offset(c.hud, "PRIMARY_SHIP_STATUS_ICON_OFFSET_FROM_PRIMARYSHIPICON_TOPLEFT");
                    commands.spawn((DockingStatus, c.put(&status, at.x, at.y), Visibility::Hidden, ChildOf(part_entity)));
                }
            }
        }
        "MAINBAR_PRIMARY_SHIP_HEALTH_BAR_BACKGROUND" => fill(commands, "MainBar_ShipStatus_Health", "MainBar_ShipStatus_Health_Offset_From_MainBar_TopLeft", Gauge::Health),
        "MAINBAR_PRIMARY_SHIP_COMBAT_BAR_BACKGROUND" => fill(commands, "MainBar_ShipStatus_CombatStrength", "MainBar_ShipStatus_Combat_Offset_From_MainBar_TopLeft", Gauge::Combat),
        "MAINBAR_PRIMARY_SHIP_SAILPOWER_BAR_BACKGROUND" => fill(commands, "MainBar_ShipStatus_SailPower", "MainBar_ShipStatus_SailPower_Offset_From_MainBar_TopLeft", Gauge::Sail),
        _ => {}
    }
}

fn update_dial(player: Single<&Ship, With<PlayerShip>>, frames: Option<Res<Frames>>, mut q: Query<(&mut ImageNode, &mut Node), With<Dial>>) {
    let Some(frames) = frames else { return };
    // Retail frames 0..3 match Stop / Slow / Full / Emergency (red through dark green).
    let Some(art) = frames.dial.get(player.order.index()) else { return };
    for (mut img, mut node) in &mut q {
        art.show(&mut img, &mut node, frames.s);
    }
}

/// Iris, face, light and voice box of the retail device, from the shared `HeadState`.
fn head_art(
    time: Res<Time>,
    head: Res<HeadState>,
    hud: Res<HudData>,
    assets: Res<AssetServer>,
    frames: Option<Res<HeadFrames>>,
    mut parts: Query<(&Head, &mut ImageNode, &mut Node, &mut Visibility)>,
) {
    let Some(frames) = frames.filter(|f| !f.iris.is_empty() && f.voice.len() == 5) else { return };
    let (open, speaking) = (head.open, head.speaking);
    let cycle = (time.elapsed_secs() * VOICE_FPS) as usize % 4;
    for (kind, mut img, mut node, mut vis) in &mut parts {
        match kind {
            Head::Iris => frames.iris[iris_frame(open, frames.iris.len())].show(&mut img, &mut node, frames.s),
            Head::Voice => frames.voice[if speaking && open >= 1.0 { cycle } else { 4 }].show(&mut img, &mut node, frames.s),
            Head::Face => {
                if let Some(art) = head.speaker.as_deref().and_then(|n| sprite(&assets, &hud, n)) {
                    art.show(&mut img, &mut node, frames.s);
                }
                vis.set_if_neq(if open > 0.0 { Visibility::Inherited } else { Visibility::Hidden });
            }
            Head::Light => {
                vis.set_if_neq(if speaking && open >= 1.0 { Visibility::Inherited } else { Visibility::Hidden });
            }
        }
    }
}

fn update_sweep(time: Res<Time>, frames: Option<Res<Frames>>, mut q: Query<(&mut ImageNode, &mut Node), With<Sweep>>) {
    let Some(frames) = frames else { return };
    if frames.sweep.is_empty() {
        return;
    }
    let art = &frames.sweep[(time.elapsed_secs() / 0.1) as usize % frames.sweep.len()];
    for (mut img, mut node) in &mut q {
        art.show(&mut img, &mut node, frames.s);
    }
}

#[allow(deprecated)] // Interaction, see pointer.rs
fn radar_face(button: Query<&Interaction, With<RadarButton>>, mut q: Query<(&RadarFace, &mut ImageNode, &mut Node)>) {
    let i = button.iter().next().map_or(0, state_index);
    for (f, mut img, mut node) in &mut q {
        f.0.art[i].show(&mut img, &mut node, f.0.s);
    }
}

fn update_gauges(
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    lib: Res<Library>,
    player: Single<(&Ship, &Health, &crate::boarding::Boardable), With<PlayerShip>>,
    mut q: Query<(&Fill, &mut Node, &mut ImageNode)>,
    guns: Single<&GunStates, With<PlayerShip>>,
) {
    let s = hud_scale(&window);
    let (ship, health, crew) = *player;
    for (f, mut node, mut img) in &mut q {
        let v = gauge_value(f.gauge, &lib, ship, health, crew, &guns).clamp(0.0, 1.0);
        // Gauges fill from the bottom: keep the bottom of the sprite, crop the top.
        let h = (f.art.size.y * v).round();
        clip(&mut node, &mut img, &f.art, f.top, h, s);
    }
}

/// A bar that empties from the top: the bottom `h` reference px of the sprite `art`, whose box starts at `top` when full. Written
/// only on a change.
fn clip(node: &mut Mut<Node>, img: &mut Mut<ImageNode>, art: &Sprite, top: f32, h: f32, s: f32) {
    let full = art.size.y;
    let region = Rect::new(0.0, full - h, art.size.x, full);
    let view = art.view(region, region.size());
    let (width, height, margin) = view.geometry(s, UiRect::DEFAULT);
    let top = px(top + full - h, s);
    if (node.top, node.width, node.height, node.margin) != (top, width, height, margin) {
        (node.top, node.width, node.height, node.margin) = (top, width, height, margin);
    }
    if img.rect != view.rect {
        img.rect = view.rect;
    }
}

fn update_guns(
    frames: Option<Res<Frames>>,
    lib: Res<Library>,
    armed: Res<combat::ArmedBank>,
    player: Single<(&Ship, &Health, &GunStates), With<PlayerShip>>,
    mut q: Query<(&GunSlot, &mut ImageNode, &mut Node)>,
) {
    let Some(frames) = frames else { return };
    let (ship, health, guns) = *player;
    let class = &lib.classes[ship.class];
    for (slot, mut img, mut node) in &mut q {
        let st = combat::bank_state(guns, class, health, slot.bank);
        let armed = armed.0 == Some(slot.bank);
        if slot.icon {
            let Some(first) = class.mounts.iter().position(|m| m.bank == slot.bank) else { continue };
            if let Some(art) = frames.icons.get(&combat::gun_icon(guns.gun(first))) {
                art.show(&mut img, &mut node, frames.s);
            }
            // Dimmed while the bank reloads (the bar beside it shows how far along it is).
            let tint = match st {
                combat::BankState::Destroyed => Color::srgba(1.0, 1.0, 1.0, 0.25),
                combat::BankState::Recharging => Color::srgb(0.55, 0.55, 0.55),
                _ => Color::WHITE,
            };
            img.map_unchanged(|i| &mut i.color).set_if_neq(tint);
        } else if let Some(art) = frames.gun.get(st.index()) {
            art.show(&mut img, &mut node, frames.s);
            img.map_unchanged(|i| &mut i.color).set_if_neq(if armed { Color::srgb(1.0, 0.9, 0.3) } else { Color::WHITE });
        }
    }
}

/// Bar buttons draw their hover and pressed art over the composed bar (PAUSE: the paused art, always, while the game is paused;
/// the movement-type button: the advanced art, always, while waypoint plotting is chosen).
/// PROVISIONAL: the retail hourglass art is swapped; any retail restrictions while paused are not decoded.
#[allow(deprecated)] // Interaction, see pointer.rs
fn bar_button_art(vtime: Res<Time<Virtual>>, plot: Option<Res<crate::course::Plot>>, mut q: Query<(&BarButton, &Interaction, &mut ImageNode, &mut Node)>) {
    let advanced = plot.is_some_and(|p| p.advanced);
    for (button, interaction, mut img, mut node) in &mut q {
        let at = state_index(interaction);
        let art = match (&button.paused, vtime.is_paused(), &button.advanced, at) {
            (Some(paused), true, _, _) => Some(&paused.art[at]),
            (_, _, Some(alt), _) if advanced => Some(&alt.art[at]),
            (_, _, _, 0) => None,
            _ => Some(&button.states.art[at]),
        };
        let color = if art.is_some() { Color::WHITE } else { Color::NONE };
        if img.color != color {
            img.color = color;
        }
        if let Some(art) = art {
            art.show_region(&mut img, &mut node, button.states.s, button.region);
        }
    }
}

/// Reference-px height above the screen bottom where the ship bar starts (top of the toggle button + the data's gap).
#[derive(Resource)]
struct ShipBarAnchor(f32);

/// The ship bar's distance from the screen bottom, in window px: centred on a `window_h` screen, or `lowest` when centring would
/// bring it lower (a tall bar on a short screen).
fn ship_bar_bottom(window_h: f32, bar_h: f32, lowest: f32) -> f32 {
    ((window_h - bar_h) / 2.0).max(lowest)
}
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
#[allow(deprecated)] // Interaction, see pointer.rs
fn commands_window(commands: &mut Commands, c: &Ctx, root: Entity, screen_w: f32, bar_w: f32, floor: f32) {
    use crate::panels::FleetButton as B;
    let top = offset(c.hud, "MainBarExtension_TopLeft_Offset_From_MainBar_TopLeft").y;
    let (l, r) = (c.size_of("FormationBar_LeftDetail"), c.size_of("FormationBar_RightDetail"));
    let h = l.y.max(r.y);
    let node = Node { bottom: px(floor - top - h, c.s), ..c.abs((screen_w - bar_w) / 2.0, 0.0, Vec2::new(bar_w, h)) };
    let bar = commands.spawn((crate::panels::FleetWindow(false), Node { top: Val::Auto, ..node }, Visibility::Hidden, ChildOf(root))).id();
    let piece = |commands: &mut Commands, name: &str, x: f32, w: f32| {
        if let Some(art) = c.sprite(name) {
            let (mut img, node) = c.put_box(&art, x, 0.0, Vec2::new(w, art.size.y));
            img.image_mode = NodeImageMode::Stretch;
            commands.spawn((img, node, ChildOf(bar), Solid, Interaction::None, FocusPolicy::Block));
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
        let (Some(art), Some(a), Some(b)) = (c.sprite(&idle), c.states(&idle), c.states(&on)) else { continue };
        at.x += gap;
        commands.spawn((action, c.put(&art, at.x, at.y), CommandArt([a, b]), ChildOf(bar), Solid, Interaction::None, FocusPolicy::Block));
        at.x += art.size.x;
    }
}

/// Not retail: the game-speed control (only with `SpeedControl`, from the `game-speed` mod), a tooltip-backdrop plate beside the main bar's right end between round slower /
/// faster buttons (the front-end Prev / Next art at 30 px, which only the Navy set has).
#[allow(deprecated)] // Interaction, see pointer.rs
fn speed_plate(commands: &mut Commands, c: &Ctx, fonts: Option<&crate::sprites::Fonts>, root: Entity, bar_right: f32) {
    use crate::panels::{SpeedButton, SpeedLabel};
    let h = c.size_of("ToolTipBackdrop_Center").y;
    let (w, b) = (110.0, 30.0);
    let plate = commands.spawn((Node { top: Val::Auto, bottom: px(6.0, c.s), ..c.abs(bar_right + 6.0, 0.0, Vec2::new(w, h)) }, ChildOf(root))).id();
    let (x0, x1) = (b / 2.0, w - b / 2.0);
    if let Some(art) = crate::sprites::slice(c.assets, "tooltip", c.hud.skin, "nocursor") {
        commands.spawn((art, c.abs(x0, 0.0, Vec2::new(x1 - x0, h)), ChildOf(plate), Solid, Interaction::None, FocusPolicy::Block));
    }
    let menu = HudData { comps: c.hud.comps.clone(), skin: crate::sprites::MENU_SKIN };
    let arrows = Ctx { hud: &menu, ..*c };
    for (button, name, x) in [(SpeedButton::Slower, "FrontEnd_Prev_Button", 0.0), (SpeedButton::Faster, "FrontEnd_Next_Button", w - b)] {
        if let (Some(art), Some(states)) = (arrows.sprite(name), arrows.states(name)) {
            let (mut img, node) = c.put_box(&art, x, (h - b) / 2.0, Vec2::splat(b));
            img.image_mode = NodeImageMode::Stretch;
            commands.spawn((button, img, node, states, ChildOf(plate), Solid, Interaction::None, FocusPolicy::Block));
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
    let s = hud_scale(&window);
    let c = Ctx { assets: &assets, hud: &hud, s };
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
            let Some(art) = c.sprite(&part(side)) else { continue };
            let sz = art.size;
            let mut img = art.image_node();
            img.image_mode = NodeImageMode::Stretch;
            let mut node = Node::default();
            art.own().place(&mut node, s, UiRect::DEFAULT);
            if side == "Center" {
                // The centre stretches with the name: its width is the text, at least the art's own.
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

#[allow(deprecated)] // Interaction, see pointer.rs
fn button_states(mut q: Query<(&States, &Interaction, &mut ImageNode, &mut Node), Or<(Changed<Interaction>, Added<States>)>>) {
    for (states, i, mut img, mut node) in &mut q {
        states.art[state_index(i)].show(&mut img, &mut node, states.s);
    }
}
#[allow(deprecated)] // Interaction, see pointer.rs
fn state_index(i: &Interaction) -> usize {
    match crate::pointer::of(i) {
        Pointer::Off => 0,
        Pointer::Over => 1,
        Pointer::Down => 2,
    }
}

/// Ship bar (secondary ships, `ShipBar_*` art) above the ship-bar toggle at the right edge: `TopDetail`, one `StretchedCenter` per
/// extra row, `BottomDetail`; two slot columns, a lone last slot centred (docs/re/fleet-controls.md). Each slot: its background
/// (selected art for the selected ship), the class icon at `Offset_ShipBar_ShipIcon`, the `ShipBar_Health` strip clipped from the
/// top as the ship loses health, and the docking/boarding icon. PROVISIONAL: slot columns and the 51 px row pitch are read off the
/// art (first row at `SHIPBAR_SHIPICON_ORIGIN_OFFSET_FROM_SHIPBAR_TOPLEFT`.y); scroll arrows and the open/close slide are not drawn.
#[allow(deprecated)] // Interaction, see pointer.rs
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
    mut buttons: Query<
        (&crate::panels::FleetButton, &Interaction, &mut ImageNode, &mut Node, Option<&CommandArt>),
        (Without<FleetIcon>, Without<FleetHealth>, Without<FleetStatus>),
    >,
    mut icons: Query<(&FleetIcon, &mut ImageNode, &mut Node), (Without<crate::panels::FleetButton>, Without<FleetHealth>, Without<FleetStatus>)>,
    mut health: Query<(&FleetHealth, &mut Node, &mut ImageNode), (Without<crate::panels::FleetButton>, Without<FleetIcon>, Without<FleetStatus>)>,
    mut status: Query<(&FleetStatus, &mut ImageNode, &mut Node, &mut Visibility), (Without<crate::panels::FleetButton>, Without<FleetIcon>, Without<FleetHealth>)>,
) {
    use crate::panels::{FleetButton as B, FleetWindow};
    let (Some(root), Some(anchor)) = (roots.iter().next(), anchor) else { return };
    let s = hud_scale(&window);
    let c = Ctx { assets: &assets, hud: &hud, s };
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
        // Vertically centred on the right edge like the weapon panel, and never lower than just above its toggle.
        // PROVISIONAL: from play (the retail bar stands about halfway up the screen); the data's offset only places it above the toggle.
        let lift = ship_bar_bottom(window.height(), h * s, anchor.0 * s);
        let node = Node { right: Val::Px(0.0), bottom: Val::Px(lift), left: Val::Auto, top: Val::Auto, ..c.abs(0.0, 0.0, Vec2::new(top.x, h)) };
        let bar = commands.spawn((FleetWindow(true), node, Visibility::Hidden, ChildOf(root))).id();
        let mut y = 0.0;
        for (name, size, count) in [("ShipBar_TopDetail", top, 1), ("ShipBar_StretchedCenter", mid, rows - 1), ("ShipBar_BottomDetail", bottom, 1)] {
            for _ in 0..count {
                if let Some(art) = c.sprite(name) {
                    commands.spawn((c.put_box(&art, 0.0, y, size), ChildOf(bar), Solid, Interaction::None, FocusPolicy::Block));
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
            let Some(art) = c.sprite("ShipBar_SlotBackground") else { continue };
            let cell = commands.spawn((B::Slot(i), c.put_box(&art, x, first + (i / 2) as f32 * mid.y, slot), ChildOf(bar), Solid, Interaction::None, FocusPolicy::Block)).id();
            let o = offset(&hud, "Offset_ShipBar_ShipIcon");
            commands.spawn((FleetIcon(i), ImageNode::default(), c.abs(o.x, o.y, c.size_of("ShipBar_ShipIcon_Navy_Cutter")), ChildOf(cell)));
            if let Some(art) = c.sprite("ShipBar_Health") {
                commands.spawn((FleetHealth(i), c.put(&art, 0.0, 0.0), ChildOf(cell)));
            }
            if let Some(art) = c.sprite("ShipStatus_Icon_Docking_Frame0") {
                let at = o + offset(&hud, "PRIMARY_SHIP_STATUS_ICON_OFFSET_FROM_PRIMARYSHIPICON_TOPLEFT");
                commands.spawn((FleetStatus(i), c.put(&art, at.x, at.y), Visibility::Hidden, ChildOf(cell)));
            }
        }
    }
    for (w, mut v) in &mut windows {
        v.set_if_neq(if if w.0 { model.ships } else { model.commands } { Visibility::Inherited } else { Visibility::Hidden });
    }
    for (b, interaction, mut img, mut node, art) in &mut buttons {
        let pick = match b {
            B::Slot(i) => model.rows.get(*i).and_then(|row| {
                let art = if row.selected { "ShipBar_Selected_SlotBackground" } else { "ShipBar_SlotBackground" };
                c.states(art).map(|st| st.art[state_index(interaction)].clone())
            }),
            B::Formation(kind) => art.map(|a| a.0[(model.active == Some(*kind)) as usize].art[state_index(interaction)].clone()),
            B::CeaseFire => art.map(|a| a.0[model.cease_fire as usize].art[state_index(interaction)].clone()),
            _ => None,
        };
        if let Some(pick) = pick {
            pick.show(&mut img, &mut node, s);
        }
    }
    for (FleetIcon(i), mut img, mut node) in &mut icons {
        if let Some(art) = model.rows.get(*i).and_then(|row| c.sprite(&row.icon)) {
            art.show(&mut img, &mut node, s);
        }
    }
    let health_art = c.sprite("ShipBar_Health");
    for (FleetHealth(i), mut node, mut img) in &mut health {
        let (Some(row), Some(art)) = (model.rows.get(*i), health_art.as_ref()) else { continue };
        // Clipped from the top: the strip shrinks towards the bottom as the ship loses health.
        let h = (art.size.y * row.health).round();
        clip(&mut node, &mut img, art, 0.0, h, s);
    }
    for (FleetStatus(i), mut img, mut node, mut v) in &mut status {
        let kind = model.rows.get(*i).map_or("", |r| r.status);
        v.set_if_neq(if kind.is_empty() { Visibility::Hidden } else { Visibility::Inherited });
        let frame = (time.elapsed_secs() * crate::panels::STATUS_FPS) as usize % 6;
        if let Some(art) = (!kind.is_empty()).then(|| c.frame(&format!("ShipStatus_Icon_{kind}_Frame{frame}"))).flatten() {
            art.show(&mut img, &mut node, s);
        }
    }
}

/// The Chat History panel above the main bar (`ChatBar_*` nine-part frame, text list inside, scroll thumb on the right).
#[derive(Component)]
struct ChatPanel;
/// The panel's clipped, scrolling list of lines.
#[derive(Component)]
struct ChatList;
/// The scroll thumb.
#[derive(Component)]
struct ChatThumb;

const CHAT_SIZE: Vec2 = Vec2::new(crate::panels::CHAT_SIZE[0], crate::panels::CHAT_SIZE[1]);

/// Builds the panel while the log is open and rebuilds it whenever a line is added, so it shows the newest line (scrolled to the bottom).
/// PROVISIONAL: the open and close animation is not drawn; the list wraps with the engine's text layout, not the original's.
#[allow(deprecated)] // Interaction, see pointer.rs
fn chat_panel(
    mut commands: Commands,
    assets: Res<AssetServer>,
    hud: Res<HudData>,
    log: Res<crate::panels::ChatLog>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    fonts: Option<Res<crate::sprites::Fonts>>,
    roots: Query<Entity, With<HudRoot>>,
    old: Query<Entity, With<ChatPanel>>,
    mut built: Local<Option<(Entity, bool, usize, u32)>>,
    mut bars: ResMut<BarLayouts>,
) {
    let Some(root) = roots.iter().next() else { return };
    let key = (root, log.open, log.lines.len(), window.height() as u32);
    if *built == Some(key) {
        return;
    }
    *built = Some(key);
    for e in &old {
        commands.entity(e).despawn();
    }
    let Some(font) = fonts.as_ref().and_then(|f| f.0.get("Univers10")).filter(|_| log.open) else { return };
    let s = hud_scale(&window);
    let c = Ctx { assets: &assets, hud: &hud, s };
    let layout = bars.get(hud.skin);
    // Same strip as the fleet-commands bar, which it replaces while open.
    let top = offset(&hud, "MainBarExtension_TopLeft_Offset_From_MainBar_TopLeft").y;
    let h_bar = c.size_of("FormationBar_LeftDetail").y.max(c.size_of("FormationBar_RightDetail").y);
    let (w, h) = (CHAT_SIZE.x, CHAT_SIZE.y);
    let node = Node { bottom: px(layout.size.y - top - h_bar, s), ..c.abs((window.width() / s - w) / 2.0, 0.0, CHAT_SIZE) };
    let panel = commands.spawn((ChatPanel, Node { top: Val::Auto, ..node }, GlobalZIndex(20), Interaction::None, FocusPolicy::Block, Solid, ChildOf(root))).id();
    let (tl, br) = (c.size_of("ChatBar_TopLeft"), c.size_of("ChatBar_BottomRight"));
    let cols = [(0.0, tl.x), (tl.x, w - tl.x - br.x), (w - br.x, br.x)];
    let rows = [("Top", 0.0, tl.y), ("Center", tl.y, h - tl.y - br.y), ("Bottom", h - br.y, br.y)];
    for (row, y, rh) in rows {
        for ((x, cw), col) in cols.into_iter().zip(["Left", "Center", "Right"]) {
            if let Some(art) = c.sprite(&format!("ChatBar_{row}{col}")) {
                let (mut img, node) = c.put_box(&art, x, y, Vec2::new(cw, rh));
                img.image_mode = NodeImageMode::Stretch;
                commands.spawn((img, node, ChildOf(panel)));
            }
        }
    }
    let list_area = Vec2::new(w - tl.x - 26.0, h - 2.0 * 14.0);
    let list = Node { overflow: Overflow::scroll_y(), flex_direction: FlexDirection::Column, ..c.abs(tl.x + 2.0, 14.0, list_area) };
    commands.spawn((ChatList, list, ScrollPosition(Vec2::new(0.0, 1.0e6)), ChildOf(panel))).with_children(|l| {
        for line in &log.lines {
            let [r, g, b, a] = line.colour;
            l.spawn((font.text(&line.text, s, Color::srgba(r, g, b, a)), Node { flex_shrink: 0.0, margin: UiRect::bottom(px(4.0, s)), ..default() }));
        }
    });
    let thumb = Node { width: px(7.0, s), height: px(20.0, s), border_radius: BorderRadius::all(px(3.5, s)), ..c.abs(w - 15.0, 14.0, Vec2::new(7.0, 20.0)) };
    commands.spawn((ChatThumb, thumb, BackgroundColor(Color::srgb(0.05, 0.12, 0.3)), ChildOf(panel)));
}

/// Over the Chat History the wheel belongs to its list: the camera reads the accumulated wheel, so it is cleared here (as the console and
/// the menus do); `chat_scroll` reads the wheel messages, which this leaves alone.
#[allow(deprecated)] // Interaction, see pointer.rs
fn chat_capture_wheel(panel: Query<&Interaction, With<ChatPanel>>, mut wheel: ResMut<bevy::input::mouse::AccumulatedMouseScroll>) {
    if panel.iter().any(crate::pointer::over) {
        wheel.delta = Vec2::ZERO;
    }
}

/// Mouse wheel over the panel scrolls the list; the thumb follows the list.
#[allow(deprecated)] // Interaction, see pointer.rs
fn chat_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    panel: Query<&Interaction, With<ChatPanel>>,
    mut list: Query<(&mut ScrollPosition, &ComputedNode), With<ChatList>>,
    mut thumb: Query<&mut Node, With<ChatThumb>>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
) {
    let over = panel.iter().any(crate::pointer::over);
    let s = hud_scale(&window);
    for (mut pos, node) in &mut list {
        let (view, content) = (node.size.y * node.inverse_scale_factor, node.content_size.y * node.inverse_scale_factor);
        for ev in wheel.read().filter(|_| over) {
            pos.y = crate::panels::chat_scroll_after_wheel(pos.y, ev.y, s, content - view);
        }
        for mut t in &mut thumb {
            t.top = px(crate::panels::chat_thumb_top(pos.y, view, content), s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineOrder;
    use crate::assets_dir;
    use crate::panels::REF_H;
    use crate::sprites::meta;

    #[test]
    fn iris_frames_run_closed_to_open() {
        assert_eq!((iris_frame(0.0, 12), iris_frame(0.5, 12), iris_frame(1.0, 12), iris_frame(2.0, 12)), (0, 6, 11, 11));
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn speed_segments_and_frames_match_engine_orders() {
        for (order, centre) in [
            (EngineOrder::Stop, Vec2::new(54.0, 71.0)),
            (EngineOrder::Slow, Vec2::new(44.0, 57.0)),
            (EngineOrder::Full, Vec2::new(31.0, 44.0)),
            (EngineOrder::Emergency, Vec2::new(14.0, 34.0)),
        ] {
            assert_eq!(crate::panels::speed_order_at(centre), order);
            assert_eq!(crate::panels::speed_order_at(centre + Vec2::splat(1.0)), order);
        }
        let data = HudData { comps: crate::data::hud_json().expect("hud/hud.json; run tools/build_assets.sh"), skin: "Navy" };
        let frames: Vec<_> = (0..4).map(|i| meta(&data, &format!("SpeedControl_CurrActive{i}_NextActive{i}"), IDLE).unwrap()).collect();
        for frame in &frames {
            assert_eq!(frame.size, Vec2::new(86.0, 101.0));
        }
        for i in 0..4 {
            for j in i + 1..4 {
                assert_ne!(frames[i].inner(), frames[j].inner(), "distinct art");
            }
        }
    }
    #[test]
    #[ignore = "needs imported assets"]
    #[allow(deprecated)] // Interaction, see pointer.rs
    fn dial_click_sets_the_engine_order_and_counts_as_a_hud_click() {
        let (mut app, root) = crate::panels::input_app();
        // The dial sends an order; outside a running battle clock `apply_tick` applies it at once.
        app.add_systems(Update, (crate::panels::engine_input.before(crate::panels::hud_input), crate::commands::apply_tick.after(crate::panels::engine_input)));
        let rel = RelativeCursorPosition { cursor_over: true, normalized: Some(Vec2::new(31.0 / 86.0 - 0.5, 44.0 / 101.0 - 0.5)) };
        app.world_mut().spawn((Solid, crate::panels::EngineDial, Interaction::Pressed, rel));
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        let order = app.world_mut().query_filtered::<&Ship, With<PlayerShip>>().single(app.world()).unwrap().order;
        assert_eq!(order, EngineOrder::Full);
        assert!(app.world().resource::<crate::panels::UiClick>().0);
        // Hidden HUD (cinematics): the dial does nothing.
        app.world_mut().entity_mut(root).insert(Visibility::Hidden);
        app.world_mut().query_filtered::<&mut Ship, With<PlayerShip>>().single_mut(app.world_mut()).unwrap().order = EngineOrder::Stop;
        app.update();
        assert_eq!(app.world_mut().query_filtered::<&Ship, With<PlayerShip>>().single(app.world()).unwrap().order, EngineOrder::Stop);
    }

    #[test]
    #[allow(deprecated)] // Interaction, see pointer.rs
    fn the_wheel_over_the_chat_panel_moves_its_list_and_thumb() {
        use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_message::<MouseWheel>().add_systems(Update, chat_scroll);
        app.world_mut().spawn((Window { resolution: (REF_H as u32 * 4 / 3, REF_H as u32).into(), ..default() }, bevy::window::PrimaryWindow));
        app.world_mut().spawn((ChatPanel, Interaction::Hovered));
        // 100 px of list showing 300 px of lines, built scrolled to the end.
        let node = ComputedNode { size: Vec2::new(300.0, 100.0), content_size: Vec2::new(300.0, 300.0), inverse_scale_factor: 1.0, ..default() };
        let list = app.world_mut().spawn((ChatList, ScrollPosition(Vec2::new(0.0, 1.0e6)), node)).id();
        let thumb = app.world_mut().spawn((ChatThumb, Node::default())).id();
        let turn = |app: &mut App, y: f32| {
            app.world_mut().write_message(MouseWheel { unit: MouseScrollUnit::Line, x: 0.0, y, window: Entity::PLACEHOLDER, phase: bevy::input::touch::TouchPhase::Moved });
            app.update();
            (app.world().get::<ScrollPosition>(list).unwrap().y, app.world().get::<Node>(thumb).unwrap().top)
        };
        let (up, thumb_up) = turn(&mut app, 1.0);
        assert_eq!(up, 200.0 - crate::panels::CHAT_WHEEL_STEP, "one notch up leaves the end");
        let (down, thumb_down) = turn(&mut app, -1.0);
        assert_eq!(down, 200.0, "one notch down is back at the end");
        assert_ne!(thumb_up, thumb_down, "the thumb follows the list");
    }

    #[test]
    #[allow(deprecated)] // Interaction, see pointer.rs
    fn the_wheel_over_the_chat_panel_does_not_reach_the_camera() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).init_resource::<bevy::input::mouse::AccumulatedMouseScroll>().add_systems(Update, chat_capture_wheel);
        let panel = app.world_mut().spawn((ChatPanel, Interaction::None)).id();
        let turn = |app: &mut App| {
            app.world_mut().resource_mut::<bevy::input::mouse::AccumulatedMouseScroll>().delta = Vec2::new(0.0, 2.0);
            app.update();
            app.world().resource::<bevy::input::mouse::AccumulatedMouseScroll>().delta.y
        };
        assert_eq!(turn(&mut app), 2.0, "off the panel the camera keeps the wheel");
        app.world_mut().entity_mut(panel).insert(Interaction::Hovered);
        assert_eq!(turn(&mut app), 0.0, "over the panel the list takes it");
    }

    #[test]
    #[allow(deprecated)] // Interaction, see pointer.rs
    fn bar_buttons_draw_art_only_when_hovered_or_paused() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_systems(Update, bar_button_art);
        let art = |image: Handle<Image>, trim: f32| Sprite { image, size: Vec2::new(20.0, 20.0), trim: crate::sprites::Trim { left: trim, ..default() }, scale: 1.0 };
        let states = |a: &Sprite| States { art: [a.clone(), a.clone(), a.clone()], s: 1.0 };
        let (idle, paused) =
            (art(bevy::asset::uuid_handle!("00000000-0000-0000-0000-000000000001"), 0.0), art(bevy::asset::uuid_handle!("00000000-0000-0000-0000-000000000002"), 4.0));
        let node = Node { width: Val::Px(20.0), height: Val::Px(20.0), ..default() };
        let region = Rect::from_corners(Vec2::ZERO, Vec2::splat(20.0));
        let button =
            app.world_mut().spawn((BarButton { states: states(&idle), paused: Some(states(&paused)), advanced: None, region }, Interaction::None, idle.image_node(), node)).id();
        app.update();
        // Idle and running: the composed bar shows the button, the node draws nothing.
        assert_eq!(app.world().get::<ImageNode>(button).unwrap().color, Color::NONE);
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Hovered;
        app.update();
        assert_eq!(app.world().get::<ImageNode>(button).unwrap().color, Color::WHITE);
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::None;
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
        app.update();
        assert_eq!(app.world().get::<ImageNode>(button).unwrap().image, paused.image);
        // The paused art has 4 transparent columns on the left: the node keeps its 20 px box and draws the rest.
        let node = app.world().get::<Node>(button).unwrap();
        assert_eq!((node.width, node.margin.left), (Val::Px(16.0), Val::Px(4.0)));
        app.world_mut().resource_mut::<Time<Virtual>>().unpause();
        app.update();
        assert_eq!(app.world().get::<ImageNode>(button).unwrap().color, Color::NONE);
    }

    #[test]
    fn the_ship_bar_is_centred_but_stays_above_its_toggle() {
        assert_eq!(ship_bar_bottom(720.0, 120.0, 200.0), 300.0, "a short bar: centred");
        assert_eq!(ship_bar_bottom(720.0, 500.0, 200.0), 200.0, "a tall one: just above the toggle");
    }

    /// The movement-type button keeps the advanced art while waypoint plotting is chosen, not only while the mouse is down.
    #[test]
    #[allow(deprecated)] // Interaction, see pointer.rs
    fn the_movement_button_shows_the_chosen_type() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_systems(Update, bar_button_art).init_resource::<crate::course::Plot>();
        let art = |image: Handle<Image>| Sprite { image, size: Vec2::new(20.0, 20.0), trim: default(), scale: 1.0 };
        let states = |a: &Sprite| States { art: [a.clone(), a.clone(), a.clone()], s: 1.0 };
        let (beginner, advanced) = (art(bevy::asset::uuid_handle!("00000000-0000-0000-0000-000000000003")), art(bevy::asset::uuid_handle!("00000000-0000-0000-0000-000000000004")));
        let node = Node { width: Val::Px(20.0), height: Val::Px(20.0), ..default() };
        let region = Rect::from_corners(Vec2::ZERO, Vec2::splat(20.0));
        let button = app
            .world_mut()
            .spawn((BarButton { states: states(&beginner), paused: None, advanced: Some(states(&advanced)), region }, Interaction::None, beginner.image_node(), node))
            .id();
        app.update();
        assert_eq!(app.world().get::<ImageNode>(button).unwrap().color, Color::NONE, "basic: the composed bar shows it");
        app.world_mut().resource_mut::<crate::course::Plot>().advanced = true;
        app.update();
        let img = app.world().get::<ImageNode>(button).unwrap();
        assert_eq!((img.color, img.image.clone()), (Color::WHITE, advanced.image.clone()), "advanced, with the mouse up");
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn every_class_has_its_hud_icons() {
        // Class names differ from the icon art (Ship_Navy_ManOWar -> Navy_Man_O_War); the WOT icon name resolves them all.
        let comps: std::sync::Arc<serde_json::Value> = crate::data::hud_json().expect("hud/hud.json; run tools/build_assets.sh");
        let lib = crate::Library::load(assets_dir());
        for class in lib.classes.iter().filter(|c| c.name.starts_with("Ship_")) {
            let data = HudData { comps: comps.clone(), skin: crate::sprites::skin_of(&class.race) };
            for bar in ["ShipBar", "WeaponBar"] {
                assert!(size_of(&data, &class.icon(bar)).is_some(), "{} lacks {}", class.name, class.icon(bar));
            }
        }
    }

    fn bar_files() -> Vec<std::path::PathBuf> {
        let mods = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mods");
        let mut roots = vec![assets_dir().to_path_buf()];
        roots.extend(std::fs::read_dir(mods).into_iter().flatten().flatten().map(|m| m.path()));
        let mut files: Vec<_> = roots
            .iter()
            .flat_map(|r| std::fs::read_dir(r.join("hud/bar")).into_iter().flatten().flatten().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        files.sort();
        files.dedup();
        files
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn typed_bar_layouts_equal_the_untyped_reader_on_retail_and_mod_data() {
        let files = bar_files();
        assert!(files.len() >= 3, "retail has a bar per faction: {files:?}");
        for f in files {
            let typed: BarLayout = crate::data::load(&f).unwrap_or_else(|e| panic!("{e}"));
            let old = BarLayout::untyped(&crate::data::read_json(&f));
            assert_eq!(typed, old, "{}", f.display());
        }
    }

    #[test]
    fn a_bad_bar_file_is_an_error_naming_the_file_and_never_a_panic() {
        let dir = std::env::temp_dir().join(format!("procyon-bar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let good = r#"{"width": 800, "height": 120, "extra": 1, "parts": [{"sprite": "A", "x": 1, "width": 2, "height": 3, "more": true}]}"#;
        std::fs::write(dir.join("good.json"), good).unwrap();
        let ok: BarLayout = crate::data::load(&dir.join("good.json")).unwrap();
        assert_eq!((ok.size, ok.parts.len(), ok.parts[0].size), (Vec2::new(800.0, 120.0), 1, Vec2::new(2.0, 3.0)));
        for (name, text) in [
            ("syntax", "{"),
            ("no_parts", r#"{"width": 1, "height": 1}"#),
            ("no_sprite", r#"{"width": 1, "height": 1, "parts": [{"x": 0, "width": 1, "height": 1}]}"#),
            ("text", r#"{"width": "1", "height": 1, "parts": []}"#),
        ] {
            let path = dir.join(format!("{name}.json"));
            std::fs::write(&path, text).unwrap();
            let e = crate::data::load::<BarLayout>(&path).unwrap_err().to_string();
            assert!(e.contains(&format!("{name}.json")), "{e}");
            // the loader the game uses: reported, the base file stands in when there is one, otherwise the bar is empty
            assert_eq!(crate::data::load_or_base::<BarLayout>(&path, Some(&dir.join("good.json")), "x").map(|l| l.parts.len()), Some(1), "{name}");
            assert_eq!(crate::data::load_or_base::<BarLayout>(&path, None, "x"), None, "{name}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn every_faction_has_the_bar_parts_it_needs() {
        let comps: std::sync::Arc<serde_json::Value> = crate::data::hud_json().expect("hud/hud.json; run tools/build_assets.sh");
        for skin in ["Navy", "Pirate", "Procyon"] {
            let data = HudData { comps: comps.clone(), skin };
            for part in &BarLayout::load(skin).parts {
                assert!(size_of(&data, &part.sprite).is_some(), "{skin} lacks {}", part.sprite);
            }
        }
    }
}
