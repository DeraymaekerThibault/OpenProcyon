//! Retail Crew Select / Arms Selection screens over the shared outfitting model (`crew_arms::FitView`), laid out as measured on
//! original screenshots: the fleet-assembly backdrop, Ships / Crew / Arms tabs, the slot bar, crew posts around the ship outline or
//! the weapon mounts on it, stat bars, and the crew or weapon list. Falls back to a plain list when the model has no `fit`.
#![allow(deprecated)]
use super::menu::{Ctx, GOLD, disabled, piece, pulldown_cell, scrollbar, selected_tab, sheet_piece, sprite, stretched_window, victory_points, wrapped};
use crate::combat::Library;
use crate::crew_arms::{Action, CrewSet, FitView, ScreenModel};
use crate::menu::{strings, text_of};
use crate::sprites::{BODY_FONT, Fonts, HudData, IDLE, offset};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use serde_json::Value;

#[derive(Component)]
struct Root;

pub fn plugin(app: &mut App) {
    app.add_systems(Update, build.after(CrewSet));
}

/// Origin of the weapon-panel layout on this screen: every mount measured on the original sits at
/// `WeaponBar_SlotOffset_From_TopLeft_NN` from here.
const PANEL: Vec2 = Vec2::new(286.5, 116.5);
/// Crew portrait size (reference units).
const FACE: f32 = 30.0;

fn build(
    mut commands: Commands,
    model: Res<ScreenModel>,
    old: Query<Entity, With<Root>>,
    assets: Res<AssetServer>,
    hud: Option<Res<HudData>>,
    fonts: Option<Res<Fonts>>,
    lib: Res<Library>,
    window: Single<&Window, With<PrimaryWindow>>,
) {
    if !model.is_changed() && (model.open == !old.is_empty()) {
        return;
    }
    for root in &old {
        commands.entity(root).despawn();
    }
    if !model.open {
        return;
    }
    let (Some(hud), Some(fonts), Some(fit)) = (hud, fonts, model.fit.as_ref()) else {
        plain(&mut commands, &model);
        return;
    };
    let (ww, wh) = (window.width(), window.height());
    let scale = (ww / 800.0).min(wh / 600.0);
    let ctx = Ctx { assets: &assets, hud: &hud, fonts: &fonts, scale };
    let strings = strings();
    let s = |id: &str| text_of(&strings, id);
    let canvas = Node {
        position_type: PositionType::Absolute,
        left: Val::Px((ww - 800.0 * scale) / 2.0),
        top: Val::Px((wh - 600.0 * scale) / 2.0),
        width: Val::Px(800.0 * scale),
        height: Val::Px(600.0 * scale),
        ..default()
    };
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    commands.spawn((Root, crate::Scenario, Interaction::None, crate::panels::Solid, full, BackgroundColor(Color::BLACK), GlobalZIndex(180))).with_children(|root| {
        root.spawn(canvas).with_children(|c| {
            ctx.image(c, "menu/shipscreen.png", Some(Rect::new(0.0, 0.0, 800.0, 600.0)), 0.0, 0.0, 800.0, 600.0, Color::WHITE);
            // Tabs: Ships goes back to the fleet screen; the current tab is the green selected button.
            button(&ctx, c, [46.0, 45.0, 172.0, 83.0], &s("IDGS_TPFRONTENDTEXT_SCREENS_SHIPS"), Action::Close);
            for (arms, rect, id) in
                [(false, [177.0, 45.0, 303.0, 83.0], "IDGS_TPFRONTENDTEXT_SCREENS_CREW"), (true, [308.0, 45.0, 434.0, 83.0], "IDGS_TPFRONTENDTEXT_SCREENS_ARMS")]
            {
                if arms == fit.arms {
                    selected_tab(&ctx, c, rect, &s(id), "BlackChancery16");
                } else {
                    button(&ctx, c, rect, &s(id), Action::Tab);
                }
            }
            let (load, save) = if fit.arms {
                ("IDGS_TPFRONTENDTEXT_SCREENS_LOAD_ARMS", "IDGS_TPFRONTENDTEXT_SCREENS_SAVE_ARMS")
            } else {
                ("IDGS_TPFRONTENDTEXT_SCREENS_LOAD_CREW", "IDGS_TPFRONTENDTEXT_SCREENS_SAVE_CREW")
            };
            disabled(&ctx, c, [502.0, 45.0, 628.0, 83.0], &s(load), "BlackChancery16");
            disabled(&ctx, c, [634.0, 45.0, 759.0, 83.0], &s(save), "BlackChancery16");
            victory_points(&ctx, c, [488.0, 79.0, 772.0, 109.0], &format!("{} : ", s("IDGS_TPFRONTENDTEXT01_CURRENT_VICTORY_POINTS")), fit.points);
            slot_bar(&ctx, c, fit, &lib);
            if fit.arms {
                arms(&ctx, c, fit, &lib, &strings);
            } else {
                crew(&ctx, c, fit, &lib, &strings);
            }
            button(&ctx, c, [168.0, 427.0, 320.0, 465.0], &s("IDGS_TPFRONTENDTEXT_SCREENS_AUTO_ASSIGN"), Action::AutoAssign);
            list(&ctx, c, fit, &strings);
            // Next arrow: back to the ships.
            let mut next = c.spawn((Interaction::None, Action::Close, ctx.node(590.0, 510.0, 80.0, 80.0)));
            next.with_children(|c| piece(&ctx, c, "FrontEnd_Next_Button", [0.0, 0.0, 80.0, 80.0], (false, false)));
        });
    });
}

/// A retail text button carrying an outfitting action.
fn button(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], label: &str, action: Action) {
    let (w, h) = (r - l, b - t);
    let cap = 51.0 * h / 45.0;
    let mut root = c.spawn((Interaction::None, action, ctx.node(l, t, w, h)));
    root.with_children(|c| {
        piece(ctx, c, "FrontEnd_Texts_Over_Button_Left", [0.0, 0.0, cap, h], (false, false));
        piece(ctx, c, "FrontEnd_Texts_Over_Button_Center", [cap, 0.0, w - cap, h], (false, false));
        piece(ctx, c, "FrontEnd_Texts_Over_Button_Right", [w - cap, 0.0, w, h], (false, false));
        ctx.text(c, "BlackChancery16", label, 0.0, w, h / 2.0, true, Color::WHITE);
    });
}

/// The fleet slot bar as on the Ships screen: five framed slots with the ships' bar icons; clicking one outfits that ship.
/// PROVISIONAL: the ship being outfitted is marked by a gold tint over its slot.
fn slot_bar(ctx: &Ctx, c: &mut ChildSpawnerCommands, fit: &FitView, lib: &Library) {
    sheet_piece(ctx, c, "fe_04", Rect::new(0.0, 66.0, 33.0, 99.0), [36.0, 507.0, 58.0, 529.0]);
    sheet_piece(ctx, c, "fe_04", Rect::new(111.0, 165.0, 144.0, 198.0), [402.0, 507.0, 424.0, 529.0]);
    for i in 0..5 {
        let x = 75.0 + i as f32 * 65.0;
        let mut b = c.spawn((Interaction::None, ctx.node(x, 488.0, 60.0, 62.0)));
        if i < fit.fleet.len() {
            b.insert(Action::SelectShip(i));
        }
        b.with_children(|c| {
            piece(ctx, c, "FrontEnd_Flag_Icon_Background", [0.0, 0.0, 60.0, 62.0], (false, false));
            match fit.fleet.get(i).and_then(|&ci| lib.classes[ci].icon.as_ref()) {
                Some(icon) => {
                    piece(ctx, c, icon, [8.0, 9.0, 52.0, 53.0], (false, false));
                    if i == fit.ship {
                        c.spawn((ctx.node(4.0, 4.0, 52.0, 54.0), BackgroundColor(GOLD.with_alpha(0.18))));
                    }
                }
                None => ctx.text(c, "UniversBold14", &(i + 1).to_string(), 0.0, 60.0, 31.0, true, Color::WHITE),
            }
        });
    }
}

/// A crew portrait: the named crew member's portrait, else the race's generic crew face; clicking selects the post's role.
fn face(ctx: &Ctx, c: &mut ChildSpawnerCommands, centre: Vec2, slot: &Option<(String, String)>, race: &str, role: usize) {
    let mut b = c.spawn((Interaction::None, Action::SelectPost(role), ctx.node(centre.x - FACE / 2.0, centre.y - FACE / 2.0, FACE, FACE)));
    b.with_children(|c| {
        let generic = format!("CrewFace_{race}_0");
        let name = slot.as_ref().map(|(_, p)| p.as_str()).filter(|p| sprite(ctx.hud, p, IDLE).is_some()).unwrap_or(&generic);
        piece(ctx, c, name, [0.0, 0.0, FACE, FACE], (false, false));
    });
}

/// Crew Select as measured: Captain / First Mate and Spotter / Helmsman pairs with their labels, rows of Engineers, Riggers and
/// Fighters, Gunners around the ship outline at the gun-bank slots, and the four stat bars. The selected role's label is gold.
fn crew(ctx: &Ctx, c: &mut ChildSpawnerCommands, fit: &FitView, lib: &Library, strings: &Value) {
    let s = |id: &str| text_of(strings, id);
    let tint = |role: usize| if fit.post == role { GOLD } else { Color::WHITE };
    let post = |role: usize| fit.posts.get(role).and_then(|p| p.first()).cloned().flatten();
    let has = |role: usize| fit.posts.get(role).is_some_and(|p| !p.is_empty());
    // (role, label, label x, portrait x, row y)
    for (role, id, lx, px, y) in [
        (1, "IDGS_TPFRONTENDTEXT02_CREW_POSITION_CAPTAIN", 45.0, 113.0, 105.0),
        (2, "IDGS_TPFRONTENDTEXT02_CREW_POSITION_FIRSTMATE", 165.0, 145.0, 105.0),
        (7, "IDGS_TPFRONTENDTEXT02_CREW_POSITION_SPOTTER", 45.0, 113.0, 138.0),
        (3, "IDGS_TPFRONTENDTEXT02_CREW_POSITION_NAVIGATOR", 165.0, 145.0, 138.0),
    ] {
        if has(role) {
            ctx.text(c, "Univers10", &s(id), lx, 90.0, y - 3.0, false, tint(role));
            face(ctx, c, Vec2::new(px, y), &post(role), &fit.race, role);
        }
    }
    for (role, id, label_y) in [(4, "IDGS_TPFRONTENDTEXT03_ENGINEERS", 167.0), (5, "IDGS_TPFRONTENDTEXT03_RIGGERS", 228.0), (6, "IDGS_TPFRONTENDTEXT03_FIGHTERS", 288.0)] {
        if !has(role) {
            continue;
        }
        ctx.text(c, "Univers10", &s(id), 45.0, 120.0, label_y, false, tint(role));
        for (i, slot) in fit.posts[role].iter().enumerate() {
            face(ctx, c, Vec2::new(61.0 + i as f32 * 31.0, label_y + 29.5), slot, &fit.race, role);
        }
    }
    // Gunners: the ship outline with a portrait at each gun bank's slot.
    ctx.text(c, "Univers10", &s("IDGS_TPFRONTENDTEXT03_GUNNERS"), 320.0, 80.0, 112.0, true, tint(0));
    outline(ctx, c, fit, lib);
    let gunners = fit.posts.first().map(Vec::as_slice).unwrap_or_default();
    for ((bank, _), slot) in fit.banks.iter().zip(gunners) {
        face(ctx, c, slot_centre(ctx, *bank), slot, &fit.race, 0);
    }
    // Stat bars: label at y 387, the retail bar below, filled red to green with the value.
    for (k, id) in ["MANEUVERABILITY", "SPEED", "REPAIR_RATE", "STRENGTH"].iter().enumerate() {
        let cx = 95.0 + k as f32 * 100.0;
        ctx.text(c, "Univers10", &s(&format!("IDGS_TPFRONTENDTEXT01_STATBAR_LABEL_{id}")), cx - 50.0, 100.0, 387.0, true, Color::WHITE);
        let v = fit.bars[k].clamp(0.0, 1.0);
        let (l, r) = (cx - 39.0, cx + 39.0);
        c.spawn((ctx.node(l + 4.0, 404.0, (r - l - 8.0) * v, 11.0), BackgroundColor(Color::srgb(1.0 - v, 0.3 + 0.6 * v, 0.1))));
        piece(ctx, c, "FrontEnd_StatBar_Border", [l, 401.0, r, 418.0], (false, false));
    }
    // Drop sends the selected post's last named crew member back to the roster.
    if fit.can_drop {
        button(ctx, c, [60.0, 427.0, 158.0, 465.0], &s("IDGS_TPFRONTENDTEXT_SCREENS_DROP"), Action::Drop);
    } else {
        disabled(ctx, c, [60.0, 427.0, 158.0, 465.0], &s("IDGS_TPFRONTENDTEXT_SCREENS_DROP"), "BlackChancery16");
    }
}

/// The ship's weapon-bar outline at the panel origin.
fn outline(ctx: &Ctx, c: &mut ChildSpawnerCommands, fit: &FitView, lib: &Library) {
    let Some(&ci) = fit.fleet.get(fit.ship) else { return };
    let name = lib.classes[ci].icon("WeaponBar");
    if let Some((_, r)) = sprite(ctx.hud, &name, IDLE) {
        let at = PANEL + offset(ctx.hud, "WEAPONBAR_SHIPICON_OFFSET_FROM_TOPLEFT");
        piece(ctx, c, &name, [at.x, at.y, at.x + r.width(), at.y + r.height()], (false, false));
    }
}

/// Centre of a gun bank's icon on this screen (31-unit icons, `Offset_WeaponBar_GunIcon` inside the slot).
fn slot_centre(ctx: &Ctx, bank: usize) -> Vec2 {
    PANEL + offset(ctx.hud, &format!("WeaponBar_SlotOffset_From_TopLeft_{bank:02}")) + offset(ctx.hud, "Offset_WeaponBar_GunIcon") + Vec2::splat(15.5)
}

/// Arms Selection as measured: the help text in a framed box, the ship outline with each selectable bank's gun icon in its frame;
/// clicking a mount selects it (gold frame) and the list on the right then offers the compatible weapons.
fn arms(ctx: &Ctx, c: &mut ChildSpawnerCommands, fit: &FitView, lib: &Library, strings: &Value) {
    stretched_window(ctx, c, [52.0, 150.0, 253.0, 307.0]);
    let text = if fit.message.is_empty() { text_of(strings, "IDGS_TPFRONTENDTEXT02_WEAPON_UPGRADE_INFO") } else { fit.message.clone() };
    wrapped(ctx, c, BODY_FONT, &text, [63.0, 165.0, 245.0, 300.0]);
    outline(ctx, c, fit, lib);
    for (i, (bank, gun)) in fit.banks.iter().enumerate() {
        let at = slot_centre(ctx, *bank) - Vec2::splat(15.5);
        let mut b = c.spawn((Interaction::None, Action::SelectPost(i), ctx.node(at.x, at.y, 31.0, 31.0)));
        b.with_children(|c| {
            piece(ctx, c, "WeaponBar_GunBankCanHitTarget", [0.0, 0.0, 31.0, 31.0], (false, false));
            piece(ctx, c, &crate::combat::gun_icon(gun), [0.0, 0.0, 31.0, 31.0], (false, false));
            if i == fit.post {
                let mut ring = ctx.node(-2.0, -2.0, 35.0, 35.0);
                ring.border = UiRect::all(Val::Px(2.0 * ctx.scale));
                c.spawn((ring, BorderColor::all(GOLD)));
            }
        });
    }
}

/// The right-hand panel: info lines in the box at the top, then 'Sort By:' (crew) or 'Available Weapons' (arms) over Univers12
/// rows 19.5 apart from y 346 and the retail scrollbar; clicking a row assigns that crew member or equips that weapon.
fn list(ctx: &Ctx, c: &mut ChildSpawnerCommands, fit: &FitView, strings: &Value) {
    if let Some(d) = &fit.detail {
        // As the Ships screen's info panel: the name, the framed icon, 'Point Cost' and the label / value lines.
        ctx.text(c, BODY_FONT, &d.title, 510.0, 240.0, 130.0, false, Color::WHITE);
        piece(ctx, c, "FrontEnd_Flag_Icon_Background", [510.0, 151.0, 568.0, 210.0], (false, false));
        piece(ctx, c, &d.icon, [517.0, 158.0, 561.0, 203.0], (false, false));
        if let Some(cost) = d.cost {
            ctx.text(c, "Univers10", &text_of(strings, "IDGS_TPFRONTENDTEXT02_POINT_COST01"), 509.0, 60.0, 220.0, true, Color::WHITE);
            ctx.text(c, "Univers10", &text_of(strings, "IDGS_TPFRONTENDTEXT02_POINT_COST02"), 509.0, 60.0, 236.0, true, Color::WHITE);
            ctx.text(c, "Univers26", &cost.to_string(), 499.0, 80.0, 256.0, true, GOLD);
        }
        for (k, (label, value)) in d.lines.iter().enumerate() {
            let cy = 161.5 + k as f32 * 16.0;
            ctx.text(c, "Univers10", label, 580.0, 95.0, cy, false, Color::WHITE);
            ctx.text(c, "Univers10", ":", 676.0, 8.0, cy, false, Color::WHITE);
            ctx.text(c, "Univers10", value, 686.0, 80.0, cy, false, GOLD);
        }
    }
    if fit.arms {
        ctx.text(c, "UniversBold14", &text_of(strings, "IDGS_TPFRONTENDTEXT01_AVAILABLE_WEAPONS"), 505.0, 215.0, 311.0, true, Color::WHITE);
    } else {
        let sort = format!("{} {}", text_of(strings, "IDGS_TPFRONTENDTEXT01_SORT_BY"), fit.sort);
        pulldown_cell(ctx, c, [520.0, 297.0, 710.0, 326.0], &sort, false, Some(Action::Sort), true);
    }
    const ROWS: usize = 8;
    for (row, (label, action)) in fit.list.iter().enumerate().skip(fit.scroll).take(ROWS) {
        let cy = 346.0 + (row - fit.scroll) as f32 * 19.5;
        let mut b = c.spawn((Interaction::None, action.clone(), ctx.node(505.0, cy - 9.75, 215.0, 19.5)));
        if fit.picked == Some(row) {
            b.insert(BackgroundColor(Color::srgba(0.2, 0.7, 0.8, 0.45)));
        }
        b.with_children(|c| ctx.text(c, BODY_FONT, label, 20.0, 195.0, 9.75, false, Color::WHITE));
    }
    let n = fit.list.len().max(1) as f32;
    scrollbar(ctx, c, [742.0, 330.0, 767.0, 505.0], fit.scroll as f32 / (n - ROWS as f32).max(1.0), (ROWS as f32 / n).min(1.0));
}

/// The plain presentation (no retail view data): title, details, navigation and rows as text buttons.
fn plain(commands: &mut Commands, model: &ScreenModel) {
    commands
        .spawn((
            Root,
            crate::Scenario,
            Interaction::None,
            crate::panels::Solid,
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), padding: UiRect::all(Val::Percent(5.0)), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
            GlobalZIndex(180),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    padding: UiRect::all(Val::Px(12.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(6.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.02, 0.04, 0.1)),
            ))
            .with_children(|card| {
                card.spawn((Text::new(&model.title), TextFont { font_size: FontSize::Px(20.0), ..default() }, TextColor(Color::srgb(1.0, 0.92, 0.75))));
                card.spawn((Text::new(&model.detail), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::WHITE)));
                for (label, action) in model.navigation.iter().chain(&model.rows) {
                    card.spawn((action.clone(), Interaction::None, Node { padding: UiRect::all(Val::Px(6.0)), ..default() }, BackgroundColor(Color::srgb(0.07, 0.12, 0.23))))
                        .with_children(|b| {
                            b.spawn((Text::new(label), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::WHITE)));
                        });
                }
                card.spawn((Text::new(&model.message), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::srgb(1.0, 0.8, 0.35))));
            });
        });
}
