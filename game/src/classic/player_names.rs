//! The retail player list window (top right of a skirmish battle; docs/re/player-names.md): the state is `alliance::Roster`, this
//! draws it from the HUD art and tags the buttons the shared `alliance::click` reads. Rebuilt whenever the roster, the scale or the
//! art set changes. PROVISIONAL: the window height is +-1 px, and the strike-through colour is the footage's red.
#![allow(deprecated)] // bevy 0.20 Interaction, as in panels.rs
use bevy::prelude::*;
use bevy::ui::FocusPolicy;

use super::panels::Ctx;
use crate::alliance::{AllianceIcon, Roster, RosterName};
use crate::panels::{HudRoot, REF_H, Solid};
use crate::sprites::{HudData, MENU_SKIN, offset};

#[derive(Component)]
struct NamesWindow;

pub fn plugin(app: &mut App) {
    app.add_systems(Update, window.in_set(crate::panels::HudSet).run_if(resource_exists::<Roster>).run_if(in_state(crate::menu::Screen::Battle)));
}

const ROW: f32 = 23.0;
const FLAG: Vec2 = Vec2::new(29.0, 21.0);

fn window(
    mut commands: Commands,
    assets: Res<AssetServer>,
    hud: Res<HudData>,
    roster: Res<Roster>,
    fonts: Option<Res<crate::sprites::Fonts>>,
    win: Single<&Window, With<bevy::window::PrimaryWindow>>,
    roots: Query<Entity, With<HudRoot>>,
    old: Query<Entity, With<NamesWindow>>,
    mut last: Local<Option<(u32, &'static str)>>,
) {
    let key = (win.height() as u32, hud.skin);
    if !roster.is_changed() && *last == Some(key) && !old.is_empty() {
        return;
    }
    let Some(root) = roots.iter().next() else { return };
    for e in &old {
        commands.entity(e).despawn();
    }
    *last = Some(key);
    let s = win.height() / REF_H;
    let c = Ctx { assets: &assets, hud: &hud, s };
    let menu = HudData { comps: hud.comps.clone(), skin: MENU_SKIN };
    let flags = Ctx { hud: &menu, ..c };
    let (top, bottom, centre) = (c.size_of("PLAYER_NAMES_WINDOW_TOP_DETAIL"), c.size_of("PLAYER_NAMES_WINDOW_BOTTOM_DETAIL"), "PLAYER_NAMES_WINDOW_STRETCHED_CENTER");
    let anchor = offset(&hud, "PLAYER_NAMES_WINDOW_OFFSET_FROM_TOPLEFT");
    let list_y = offset(&hud, "PLAYER_NAMES_WINDOW_PLAYERLISTING_OFFSET_Y_FROM_WINDOWTOPEDGE").y;
    let squeeze = offset(&hud, "PLAYER_NAMES_WINDOW_VERTICALCOMPRESSION_OFFSET_Y").y;
    let (w, h) = (top.x, top.y + bottom.y + roster.rows.len() as f32 * ROW - squeeze);
    let window = Node { position_type: PositionType::Absolute, right: px(-anchor.x, s), top: px(anchor.y, s), width: px(w, s), height: px(h, s), ..default() };
    let window = commands.spawn((NamesWindow, window, ChildOf(root))).id();
    let solid = || (Solid, Interaction::None, FocusPolicy::Block);
    for (name, y, size) in
        [(centre, top.y, Vec2::new(w, h - top.y - bottom.y)), ("PLAYER_NAMES_WINDOW_TOP_DETAIL", 0.0, top), ("PLAYER_NAMES_WINDOW_BOTTOM_DETAIL", h - bottom.y, bottom)]
    {
        if let Some(art) = c.sprite(name) {
            let (mut img, node) = c.put_box(&art, 0.0, y, size);
            img.image_mode = NodeImageMode::Stretch;
            commands.spawn((img, node, ChildOf(window), solid()));
        }
    }
    let font = fonts.as_deref().and_then(|f| f.0.get("Univers10"));
    let strings = crate::menu::strings();
    let [margin, gap, edge] = ["Flag And Window Left Edge", "Flag And Player Name", "Alliance Status And Window Right Edge"]
        .map(|k| hud.comps["Player Names Window"]["_"][format!("PlayerNamesWindow: Pixels Between {k}")].as_f64().unwrap_or(0.0) as f32);
    for (i, row) in roster.rows.iter().enumerate() {
        let y = list_y + i as f32 * ROW;
        if let Some(art) = flags.sprite(&format!("FrontEnd_Flag_Small_Icon_{:02}", row.flag)) {
            let (mut img, node) = c.put_box(&art, margin, y, FLAG);
            img.image_mode = NodeImageMode::Stretch;
            commands.spawn((img, node, ChildOf(window)));
        }
        let (x, icon_w) = (margin + FLAG.x + gap, 29.0);
        let label = row.name.clone().unwrap_or_else(|| strings["IDGS_TPFRONTENDTEXT01_COMPUTER"].as_str().unwrap_or("Computer").to_string());
        let mut cell = Node { align_items: AlignItems::Center, ..c.abs(x, y, Vec2::new(w - x - edge - icon_w, FLAG.y)) };
        cell.overflow = Overflow::clip();
        let cell = commands.spawn((RosterName(i), cell, ChildOf(window), Interaction::None, FocusPolicy::Pass)).id();
        let text = commands.spawn((Node { position_type: PositionType::Relative, align_items: AlignItems::Center, ..default() }, ChildOf(cell))).id();
        if let Some(font) = font {
            commands.spawn((font.text(&label, s, row.colour), ChildOf(text)));
        }
        if !row.alive {
            // The strike-through: the cross-out line height, through the middle of the name.
            let line = hud.comps["Player Names Window"]["_"]["PlayerNamesWindow: Cross Out Line Height In Pixels"].as_f64().unwrap_or(2.0) as f32;
            let node = Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), top: Val::Percent(50.0), height: px(line, s), ..default() };
            commands.spawn((node, BackgroundColor(Color::srgb(0.85, 0.1, 0.1)), ChildOf(text)));
        }
        if row.local {
            continue;
        }
        let name = format!("PLAYER_ALLIANCE_STATUS_{}{}", if row.allied { "ALLIED" } else { "NOT_ALLIED" }, if roster.changeable { "_CHANGE" } else { "" });
        let Some(art) = c.sprite(&name) else { continue };
        let size = art.size;
        let at = Vec2::new(w - edge - size.x, y + (FLAG.y - size.y) / 2.0);
        let (mut img, node) = c.put(&art, at.x, at.y);
        img.image_mode = NodeImageMode::Stretch;
        let mut icon = commands.spawn((img, node, ChildOf(window), solid()));
        if roster.changeable && row.alive {
            icon.insert(AllianceIcon(i));
            if let Some(states) = c.states(&name) {
                icon.insert(states);
            }
        }
    }
}

fn px(v: f32, s: f32) -> Val {
    Val::Px(v * s)
}
