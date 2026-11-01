//! Classic (retail) front-end screens: the retail Screens/*.tst element rectangles (800x600 reference, scaled to the window), texts
//! from the retail string table, button/list/arrow art from the hud.hdt FrontEnd_* sprites with their NoCursor/CursorOver/CursorDown
//! states, the backdrop BD_GUITextures FrontEndScreen0. State and actions live in `menu.rs`; this only draws and scrolls.
//! PROVISIONAL: which element shows the map picture (Lobby Information area), the description font, the fleet editor
//! no transitions.

#![allow(deprecated)] // shortcut: bevy 0.20 Interaction (deprecated for picking Hovered/Pressed); port when it is removed
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use serde_json::Value;

use crate::assets_dir;
use crate::bindings::Bindings;
use crate::combat::Library;
use crate::menu::{
    Action, BuildMenu, Cell, Choice, CtrlUi, FLEET_SIZE, InGame, Kind, MenuKey, MenuModel, MenuRoot, Page, RACES, RunButtons, SaveUi, Screen, ScrollKind, Skirmish, SliderTrack,
    VICTORY_LIMIT, menu_active, offered, pretty, strings, text_of,
};
use crate::sprites::{BODY_FONT, Fonts, HudData, IDLE};

use super::widgets::{
    Ctx, GOLD, HEADING, PICKED, Rest, arrow, canvas, cell, disabled, dropdown, fleet_bar, full_screen, info_panel, list_row, piece, piece_flipped, pulldown_cell, radio, scrollbar,
    selected_tab, slider, sprite, stretched_window, victory_points, wheel_step, wrapped,
};

pub fn plugin(app: &mut App) {
    app.add_systems(Update, ((scroll_controls, scroll_saves).before(BuildMenu), build.in_set(BuildMenu), wheel.after(RunButtons)).run_if(menu_active));
}

/// Rebuilds the current screen when the state, the selection or the window size changes.
fn build(
    mut commands: Commands,
    assets: Res<AssetServer>,
    hud: Option<Res<HudData>>,
    fonts: Option<Res<Fonts>>,
    model: MenuModel,
    window: Single<Ref<Window>, With<PrimaryWindow>>,
    roots: Query<Entity, With<MenuRoot>>,
    mut last: Local<Option<MenuKey>>,
    preview: Option<Res<super::ship_preview::ShipPreview>>,
) {
    let (Some(hud), Some(fonts)) = (hud, fonts) else { return };
    if fonts.0.is_empty() {
        return;
    }
    if last.is_some() && !roots.is_empty() && !model.changed() && !window.is_changed() {
        return;
    }
    let key = model.key(&window);
    if last.as_ref() == Some(&key) && !roots.is_empty() {
        return;
    }
    *last = Some(key);
    for e in &roots {
        commands.entity(e).despawn();
    }
    let MenuModel { sk, ig, binds, ctrl: cui, state, settings, .. } = &model;
    let strings = strings();
    let ingame = *state.get() == Screen::Battle;
    // Options / Control Options: no retail .tst is exported, so they are laid out below from the retail screenshots.
    let page = match (state.get(), ig.page()) {
        (Screen::Options, _) | (Screen::Battle, Some(Page::Options)) => Some(false),
        (Screen::Controls, _) | (Screen::Battle, Some(Page::Controls)) => Some(true),
        _ => None,
    };
    let settings_page = match (state.get(), ig.page()) {
        (Screen::Audio, _) | (Screen::Battle, Some(Page::Audio)) => Some(false),
        (Screen::Display, _) | (Screen::Battle, Some(Page::Display)) => Some(true),
        _ => None,
    };
    let file = match state.get() {
        _ if page.is_some() || settings_page.is_some() => "",
        Screen::Battle if ig.page() == Some(Page::Menu) => "ingamemenu_default",
        Screen::Battle if ig.page() == Some(Page::Summary) => "multiplayer_stats_screen",
        Screen::Battle if ig.page().is_some() => "ingamemenu_load_campaign",
        Screen::Load => "load_campaign_screen",
        Screen::Tutorial => "tutorial_screen",
        Screen::Options | Screen::Controls | Screen::Audio | Screen::Display => unreachable!(),
        Screen::Main => "mainmenu",
        Screen::SinglePlayer => "singleplayer_menu",
        Screen::CreateSkirmish => "create_skirmish_game_screen",
        Screen::Lobby => "skirmish_game_lobby",
        Screen::Fleet => "ship_selection_screen",
        Screen::Battle => return,
    };
    let els: Vec<Value> = crate::data::try_json(&assets_dir().join(format!("menu/{file}.json"))).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let (canvas, scale) = canvas(&window);
    let ctx = Ctx { assets: &assets, hud: &hud, fonts: &fonts, scale };
    commands
        .spawn((
            MenuRoot,
            full_screen(),
            BackgroundColor(if ingame && ig.page() != Some(Page::Summary) { Color::srgba(0.0, 0.0, 0.0, 0.5) } else { Color::BLACK }),
            GlobalZIndex(100),
        ))
        .with_children(|root| {
            root.spawn(canvas).with_children(|c| {
                if let Some(controls) = page {
                    options_page(&ctx, c, controls, ingame, strings, binds, cui);
                }
                if let Some(display) = settings_page {
                    sound_display_page(&ctx, c, display, ingame, strings, settings);
                }
                for e in &els {
                    element(&ctx, c, &model, e, strings, preview.as_deref());
                }
                if *state.get() == Screen::Lobby && sk.map_info {
                    map_info_dialog(&ctx, c, sk, strings);
                }
                if *state.get() == Screen::Fleet && !sk.arms_open {
                    profile_dialog(&ctx, c, sk, &model.lib, strings);
                }
            });
        });
}

/// One element of a retail screen file (`menu/<screen>.json`): its art and, for buttons, the `Action` it runs.
fn element(ctx: &Ctx, c: &mut ChildSpawnerCommands, m: &MenuModel, e: &Value, strings: &Value, preview: Option<&super::ship_preview::ShipPreview>) {
    let MenuModel { sk, ig, saves: ui, lib, state, settings, stats, outcome, end_message, .. } = m;
    let impl_ = e["impl"].as_str().unwrap_or("");
    let r: Vec<f32> = e["rect"].as_array().map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default();
    if r.len() != 4 || e["enabled"] == Value::Bool(false) {
        return;
    }
    let rect = [r[0], r[1], r[2], r[3]];
    let font = e["font"].as_str().unwrap_or("BlackChancery22");
    let label = e["text"].as_str().map(|t| text_of(strings, t)).unwrap_or_default();
    let label = if impl_ == "Load or Save Campaign Button" && ig.page() == Some(Page::Save) { text_of(strings, "IDGS_TPFRONTENDTEXT_SCREENS_SAVE") } else { label };
    // Element colour (`Color Colour(r,g,b,a)`): titles are a black shadow label plus the coloured one.
    let tint = e["color"].as_array().filter(|c| c.len() == 4).map_or(Color::WHITE, |c| {
        let k = |i: usize| c[i].as_f64().unwrap_or(1.0) as f32;
        Color::srgba(k(0), k(1), k(2), k(3))
    });
    let action = match (state.get(), impl_) {
        (Screen::SinglePlayer, "Open Load Campaign Screen Button") => Some(Action::OpenLoad),
        (Screen::Load, "Open Prev Arrow Button") => {
            arrow(ctx, c, rect, "FrontEnd_Prev_Button", Action::Goto(Screen::SinglePlayer));
            None
        }
        (Screen::Battle, "Front End Dialog Screen Backdrop") => {
            // The retail frame does not fill this ReferenceArea (its buttons reach past the area's right edge).
            let [x, y, w, h] = dialog_frame(rect);
            ctx.image(c, "menu/frontenddialogscreen.png", None, x, y, w, h, Color::WHITE);
            None
        }
        (Screen::Battle, "In Game Backdrop") => {
            // The window frame occupies the top-left of the 768x512 sheet.
            ingame_window(ctx, c, rect);
            None
        }
        (Screen::Battle, "Open Next Screen Button") => Some(Action::Ingame(Some(Page::Options))),
        (Screen::Battle, "In Game Menu Stats Screen Button") => Some(Action::Ingame(Some(Page::Summary))),
        (Screen::Battle, "Game Lobby Backdrop") => {
            ctx.backdrop(c, "menu/gamelobby.png");
            None
        }
        (Screen::Battle, "Game Stats - Player Kills by Object Type") => {
            summary(ctx, c, stats, **outcome, end_message.0.as_deref(), strings);
            None
        }
        // A decided battle continues to the main menu (measured on a won skirmish); opened from the in-game menu it goes back there.
        (Screen::Battle, "Open Next Arrow Button") => {
            let next = if **outcome == crate::scenario::Outcome::Playing { Action::Ingame(Some(Page::Menu)) } else { Action::QuitToMenu };
            arrow(ctx, c, rect, "FrontEnd_Next_Button", next);
            None
        }
        (Screen::Main, "Open Option Screen") => Some(Action::Goto(Screen::Options)),
        (Screen::Main, "Open Tutorial Button") => Some(Action::Goto(Screen::Tutorial)),
        (Screen::Tutorial, "Start Basic Training Tutorial Button") => Some(Action::Tutorial("basic_tutorial")),
        (Screen::Tutorial, "Start Full Training Tutorial Button") => Some(Action::Tutorial("tutorial")),
        (Screen::Tutorial, "Open Prev Arrow Button") => {
            arrow(ctx, c, rect, "FrontEnd_Prev_Button", Action::Goto(Screen::Main));
            None
        }
        (Screen::Battle, "Open Saved Campaign Screen Button") => Some(Action::Ingame(Some(Page::Save))),
        (Screen::Battle, "Open Load Campaign Screen Button") => Some(Action::Ingame(Some(Page::Load))),
        (Screen::Battle, "InGameMenu QuitGame Button") => Some(Action::QuitToMenu),
        (Screen::Battle, "Open Previous Screen Button") => Some(Action::Ingame(if ig.page() == Some(Page::Menu) { None } else { Some(Page::Menu) })),
        (Screen::Load | Screen::Battle, "Saved Campaign Scrollable List") => {
            save_list(ctx, c, rect, ui, strings);
            None
        }
        (Screen::Load | Screen::Battle, "Load or Save Campaign Button") => Some(if ig.page() == Some(Page::Save) { Action::DoSave } else { Action::DoLoad }),
        (Screen::Load | Screen::Battle, "Open Confirm Saved Game Delete Dialog Button") => Some(Action::Remove),
        (Screen::Battle, "Campaign Text Entry") if ig.page() == Some(Page::Save) => {
            cell(ctx, c, [r[0], r[1], r[2] - r[0], r[3] - r[1]], &format!("{}_", ui.name), false, None::<Action>);
            None
        }
        (Screen::Battle, "Right Justify Label") if ig.page() == Some(Page::Save) && r[0] as i32 == 272 => {
            ctx.text(c, BODY_FONT, &text_of(strings, "IDGS_TPFRONTENDTEXT_SCREENS_FILENAME"), r[0], r[2] - r[0], (r[1] + r[3]) / 2.0, true, Color::WHITE);
            None
        }
        (_, "Front End Screen Backdrop") => {
            frontend_backdrop(ctx, c, if *state.get() == Screen::CreateSkirmish { 2 } else { 0 });
            None
        }
        (_, "Center Justify Label") => {
            ctx.text(c, font, &label, r[0], r[2] - r[0], (r[1] + r[3]) / 2.0, true, tint);
            None
        }
        (Screen::Main, "Open Single Player Menu Button") => Some(Action::Goto(Screen::SinglePlayer)),
        (Screen::Main, "Open Next Screen Button") => Some(Action::Quit),
        (Screen::SinglePlayer, "Open Next Screen Button") => Some(Action::Campaign),
        (Screen::SinglePlayer, "Open Create Skirmish Game Screen") => Some(Action::Goto(Screen::CreateSkirmish)),
        (Screen::SinglePlayer, "Toggle Difficulty Button") => {
            let level = ["IDGS_TPFRONTENDTEXT03_EASY", "IDGS_TPFRONTENDTEXT03_NORMAL", "IDGS_TPFRONTENDTEXT03_HARD"][settings.difficulty.min(2) as usize];
            ctx.text_button(c, rect, &text_of(strings, level), font, Action::Setting(crate::menu::Setting::Difficulty));
            None
        }
        (Screen::SinglePlayer, "Open Prev Arrow Button") | (Screen::CreateSkirmish, "Open Prev Arrow Button") => {
            arrow(ctx, c, rect, "FrontEnd_Prev_Button", Action::Goto(if *state.get() == Screen::SinglePlayer { Screen::Main } else { Screen::SinglePlayer }));
            None
        }
        (Screen::CreateSkirmish, "Open Next Arrow Button") => {
            arrow(ctx, c, rect, "FrontEnd_Next_Button", Action::Goto(Screen::Lobby));
            None
        }
        (Screen::CreateSkirmish, "List World Files") => {
            map_list(ctx, c, sk, strings);
            None
        }
        (Screen::CreateSkirmish, "Map Information Button") => {
            // Measured: framed panel at the element's area; gold 'Map Description:' then Univers10 lines 16 apart.
            stretched_window(ctx, c, [r[0], r[1] - 3.0, r[2], r[3] + 2.0]);
            ctx.text(c, "Univers10", &format!("{}:", text_of(strings, "IDGS_TPFRONTENDTEXT02_MAP_DESCRIPTION")), r[0] + 5.0, 200.0, r[1] + 9.0, false, GOLD);
            wrapped(ctx, c, "Univers10", sk.map_json["description"].as_str().unwrap_or_default(), [r[0] + 5.0, r[1] + 25.0, r[2] - 12.0, r[3]]);
            None
        }
        (Screen::CreateSkirmish, "Radio Historical Map Button") => {
            radio(ctx, c, rect, &text_of(strings, "IDGS_TPFRONTENDTEXT01_VIEW_PRESET_MAPS"), sk.historical, Action::Historical(true));
            None
        }
        (Screen::CreateSkirmish, "Radio Open Map Button") => {
            radio(ctx, c, rect, &text_of(strings, "IDGS_TPFRONTENDTEXT01_VIEW_OPEN_MAPS"), !sk.historical, Action::Historical(false));
            None
        }
        // Fleet size (and the victory limit) only apply to open maps; historical maps bring their own fleets.
        (Screen::CreateSkirmish, "Fleet Size Label" | "Fleet Size Scrollbar Button" | "Victory Limit Label" | "Victory Points Scrollbar Button") if sk.historical => None,
        (Screen::CreateSkirmish, "Fleet Size Label") => {
            ctx.text(
                c,
                BODY_FONT,
                &format!("{} : {}", text_of(strings, "IDGS_TPFRONTENDTEXT01_FLEET_SIZE_LIMIT"), sk.fleet_size),
                r[0],
                r[2] - r[0],
                (r[1] + r[3]) / 2.0,
                true,
                Color::WHITE,
            );
            None
        }
        (Screen::CreateSkirmish, "Fleet Size Scrollbar Button") => {
            slider(ctx, c, rect, FLEET_SIZE.fraction(sk.fleet_size as u32), Action::FleetSize(-1), Action::FleetSize(1), SliderTrack::Fleet);
            None
        }
        // Retail greys Launch Game until a computer opponent is in and (open maps) a fleet is assembled.
        (Screen::Lobby, "Open Next Screen Button") if sk.launch_error().is_some() => {
            disabled(ctx, c, rect, &label, font);
            None
        }
        (Screen::Lobby, "Open Next Screen Button") => Some(Action::Launch),
        (Screen::Lobby, "Open Map Description Dialog Button") => Some(Action::MapInfo(true)),
        (Screen::Lobby, "Open Previous Screen Button") => Some(Action::Goto(Screen::CreateSkirmish)),
        (Screen::Lobby, "Open Ship Selection Screen") if sk.open() => Some(Action::Goto(Screen::Fleet)),
        (Screen::Lobby, "Game Lobby Backdrop") => {
            ctx.backdrop(c, "menu/gamelobby.png");
            None
        }
        (Screen::Lobby, "Lobby Information Button") => {
            // Measured: a gold Univers10 'Map' label and the map name in Univers12 under the title.
            ctx.text(c, "Univers10", &text_of(strings, "IDGS_TPFRONTENDTEXT01_SERVER_MAPNAME"), r[0], r[2] - r[0], 84.0, true, GOLD);
            ctx.text(c, BODY_FONT, sk.map_json["name"].as_str().unwrap_or_default(), r[0], r[2] - r[0], 101.0, true, Color::WHITE);
            None
        }
        (Screen::Lobby, "Lobby Player List Button") => {
            players_list(ctx, c, rect, sk, strings);
            None
        }
        (Screen::Fleet, "Assemble Fleet Backdrop") => {
            ctx.backdrop(c, "menu/shipscreen.png");
            None
        }
        (Screen::Fleet, "Ship Types Available") => {
            ship_list(ctx, c, rect, sk, lib, strings);
            None
        }
        (Screen::Fleet, "Fleet Select Button") => {
            fleet_list(ctx, c, sk, lib);
            None
        }
        (Screen::Fleet, "Ship Information") => {
            ship_info(ctx, c, rect, sk, lib, strings);
            None
        }
        (Screen::Fleet, "Ship Victory Points") => {
            victory_points(ctx, c, rect, &format!("{} : ", text_of(strings, "IDGS_TPFRONTENDTEXT01_CURRENT_VICTORY_POINTS")), sk.points_left(lib));
            None
        }
        (Screen::Fleet, "Open Ship Selection Screen") => {
            selected_tab(ctx, c, rect, &label, font);
            None
        }
        (Screen::Fleet, "Open Arms Selection Screen" | "Open Crew Selection Screen" | "Drop Ship From Fleet Button" | "Add Ship To Fleet Button") => {
            // Retail greys these out: Crew, Arms and Drop until the picked fleet slot holds a ship, Add until a hull type is clicked.
            let (on, action) = match impl_ {
                "Add Ship To Fleet Button" => (sk.ship_chosen, Action::AddShip),
                "Drop Ship From Fleet Button" => (sk.slot_filled(), Action::DropShip),
                "Open Crew Selection Screen" => (sk.slot_filled(), Action::OpenCrew),
                _ => (sk.slot_filled(), Action::OpenArms),
            };
            if on {
                Some(action)
            } else {
                disabled(ctx, c, rect, &label, font);
                None
            }
        }
        (Screen::Fleet, "Open Next Arrow Button") => {
            arrow(ctx, c, rect, "FrontEnd_Next_Button", Action::Goto(Screen::Lobby));
            None
        }
        (Screen::Fleet, "Ship ViewPort Button") => {
            // The globe grid with the 3D preview (classic/ship_preview.rs) on top.
            // The 512x288 texture holds the grid in its top-left 393x285, the viewport's size: drawn 1:1.
            ctx.image(c, "menu/fe_shipscreen_bg.png", Some(Rect::new(0.0, 0.0, r[2] - r[0], r[3] - r[1])), r[0], r[1], r[2] - r[0], r[3] - r[1], Color::WHITE);
            if let Some(p) = &preview {
                c.spawn((ImageNode::new(p.image.clone()), ctx.node(r[0], r[1], r[2] - r[0], r[3] - r[1])));
            }
            ship_name_field(ctx, c, sk, lib);
            None
        }
        (Screen::Fleet, "AutoAssign Button") => Some(Action::AutoAssign),
        (Screen::Fleet, "Open Load Fleet Profile Dialog Button") => Some(Action::OpenProfile(crate::profiles::Kind::Fleet, false)),
        (Screen::Fleet, "Open Save Fleet Profile Dialog Button") if sk.human().is_some_and(|h| !h.fleet.is_empty()) => {
            Some(Action::OpenProfile(crate::profiles::Kind::Fleet, true))
        }
        (Screen::CreateSkirmish, "Victory Limit Label") => {
            ctx.text(
                c,
                BODY_FONT,
                &format!("{} : {}", text_of(strings, "IDGS_TPFRONTENDTEXT_VICTORY_POINTS"), sk.victory_limit),
                r[0],
                r[2] - r[0],
                (r[1] + r[3]) / 2.0,
                true,
                Color::WHITE,
            );
            None
        }
        (Screen::CreateSkirmish, "Victory Points Scrollbar Button") => {
            slider(ctx, c, rect, VICTORY_LIMIT.fraction(sk.victory_limit), Action::VictoryLimit(-1), Action::VictoryLimit(1), SliderTrack::Victory);
            None
        }
        // Other retail buttons (multiplayer, options, profiles, campaign...) are shown but not implemented.
        (_, n) if !label.is_empty() && (n.starts_with("Open") || n.contains("Button")) && !n.contains("Arrow") => {
            disabled(ctx, c, rect, &label, font);
            None
        }
        _ => None,
    };
    if let Some(a) = action {
        ctx.text_button(c, rect, &label, font, a);
    }
}

/// The Game Summary table, measured on two retail summaries (800x600 window): the VICTORY!/DEFEAT! label (UniversLightBold14)
/// centred at y 132, a framed panel from (35,160) to x 767 with a 30 px heading band (Univers10 column titles), and per player a
/// row 60 apart from y 210: the small flag at x 37, the name (Univers12) from x 77 and four 128x24 boxes at x 197 + 140 * column
/// (hud.hdt: name width 110, column width 130, gap 10, row height 30). A column's best value gets the green bar (`GameStats::best`).
/// The skirmish summary is this one page (retail: Next goes to the main menu; no kill-by-type table seen in skirmish).
/// PROVISIONAL: the panel art (a stretched window stands in), flag choice.
fn summary(ctx: &Ctx, c: &mut ChildSpawnerCommands, stats: &crate::stats::GameStats, outcome: crate::scenario::Outcome, message: Option<&str>, strings: &Value) {
    let s = |id: &str| text_of(strings, id);
    let won = outcome == crate::scenario::Outcome::Victory;
    let status = if won { "IDGS_TPFRONTENDTEXT02_STATS_SCREEN_GAMEWON" } else { "IDGS_TPFRONTENDTEXT02_STATS_SCREEN_GAMELOST" };
    if outcome != crate::scenario::Outcome::Playing {
        ctx.text(c, "UniversLightBold14", &s(status), 230.0, 340.0, 132.0, true, Color::WHITE);
        // The map's own end message under the label. PROVISIONAL: retail position (and whether it shows here at all) not measured.
        if let Some(text) = message.map(s).filter(|t| !t.is_empty()) {
            ctx.text(c, "Univers10", &text, 35.0, 732.0, 150.0, true, Color::WHITE);
        }
    }
    let rows = stats.players.len().max(1) as f32;
    stretched_window(ctx, c, [35.0, 160.0, 767.0, 190.0 + rows * 60.0 - 20.0]);
    c.spawn((ctx.node(39.0, 162.0, 724.0, 28.0), BackgroundColor(HEADING)));
    let columns = ["SHIPSSUNK", "SHIPSLEFT", "DAMAGEINFLICTED", "DAMAGETAKEN"].map(|k| s(&format!("IDGS_TPFRONTENDTEXT02_STATS_SCREEN_{k}")));
    for (i, title) in columns.iter().enumerate() {
        ctx.text(c, "Univers10", title, 195.0 + i as f32 * 140.0, 130.0, 175.0, true, Color::WHITE);
    }
    // Retail lists the winner first (one victory and three defeats, two players each): rows by ships left, most first.
    // PROVISIONAL with more players or ties.
    let mut rows: Vec<_> = stats.players.iter().zip(stats.best()).collect();
    rows.sort_by_key(|((_, line), _)| std::cmp::Reverse(line.end));
    for (row, ((name, line), best)) in rows.into_iter().enumerate() {
        let cy = 210.0 + row as f32 * 60.0;
        let race = RACES.iter().find(|r| r.1 == line.race).map_or(0, |r| r.0);
        piece(ctx, c, &format!("FrontEnd_Flag_Small_Icon_{:02}", flag_base(race) + row % 10), [37.0, cy - 12.5, 65.0, cy + 12.5]);
        ctx.text(c, BODY_FONT, name, 77.0, 110.0, cy, false, Color::WHITE);
        for (i, (cell, green)) in line.cells().iter().zip(best).enumerate() {
            let x = 197.0 + i as f32 * 140.0;
            // The gold-rimmed pill of the retail text buttons, with the retail bar colour (hud.hdt 0.207, 0.566, 0.265) inside.
            ctx.button_art(c, [x, cy - 12.0, 128.0, 24.0]);
            if green {
                let fill = Node { border_radius: BorderRadius::all(Val::Px(7.0 * ctx.scale)), ..ctx.node(x + 4.0, cy - 8.0, 120.0, 16.0) };
                c.spawn((fill, BackgroundColor(Color::srgb(0.207, 0.566, 0.265))));
            }
            ctx.text(c, "Univers10", cell, x, 128.0, cy, true, Color::WHITE);
        }
    }
}

/// Rows of a save list (19 px each, mouse wheel scrolls): "name - N pts".
fn save_list(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], ui: &SaveUi, strings: &Value) {
    let rows = ((b - t) / 19.0) as usize;
    for (i, (s, _)) in ui.list.iter().enumerate().skip(ui.scroll).take(rows) {
        cell(ctx, c, [l, t + (i - ui.scroll) as f32 * 19.0, r - l, 18.0], &format!("{}  -  {} pts", s.name, s.points), ui.selected == Some(i), Some(Action::PickSave(i)));
    }
    if let Some(m) = ui.status {
        ctx.text(c, BODY_FONT, strings[m].as_str().unwrap_or(m), l, r - l, t - 10.0, false, Color::srgb(1.0, 0.9, 0.4));
    }
}

/// Options and Control Options (front end: green-banner screen; in game: gold dialog). Rectangles are 800x600 canvas
/// coordinates measured from the retail screenshots (PROVISIONAL: no .tst exported).
fn options_page(ctx: &Ctx, c: &mut ChildSpawnerCommands, controls: bool, ingame: bool, strings: &Value, binds: &Bindings, cui: &CtrlUi) {
    let s = |id: &str| text_of(strings, id);
    let gold = SETTINGS_GOLD;
    let back = if controls { (Page::Options, Screen::Options) } else { (Page::Menu, Screen::Main) };
    settings_frame(ctx, c, ingame, back);
    if !controls {
        ctx.text(c, "BlackChancery22", &s("IDGS_TPFRONTENDTEXT_SCREENS_OPTIONS"), 250.0, 300.0, if ingame { 190.0 } else { 195.0 }, true, gold);
        let to = |p: Page, s: Screen| Action::Goto(s).with_page(ingame.then_some(p));
        let items: [(&str, Option<Action>); 3] = [
            ("IDGS_TPFRONTENDTEXT_SCREENS_AUDIO_OPTIONS", Some(to(Page::Audio, Screen::Audio))),
            ("IDGS_TPFRONTENDTEXT_SCREENS_CONTROL_OPTIONS", Some(to(Page::Controls, Screen::Controls))),
            ("IDGS_TPFRONTENDTEXT_SCREENS_DISPLAY_OPTIONS", Some(to(Page::Display, Screen::Display))),
        ];
        for (i, (id, a)) in items.into_iter().enumerate() {
            let y = 229.0 + 50.0 * i as f32;
            let rect = [290.0, y, 508.0, y + 31.0];
            match a {
                Some(a) => ctx.text_button(c, rect, &s(id), "BlackChancery22", a),
                None => disabled(ctx, c, rect, &s(id), "BlackChancery22"), // audio / display options: not done
            }
        }
        return;
    }
    // Control Options: header + scrolling list of (control, key).
    let (l, t, r, b, rows, title) = if ingame {
        (207.0, 211.0, 579.0, 365.0, 8, [(400.0, 182.0, "IDGS_TPFRONTENDTEXT_SCREENS_CONTROL_OPTIONS")].as_slice())
    } else {
        (
            229.0,
            163.0,
            667.0,
            457.0,
            17,
            [(160.0, 188.0, "IDGS_TPFRONTENDTEXT03_CONTROL_OPTIONS_1_OF_2"), (160.0, 216.0, "IDGS_TPFRONTENDTEXT03_CONTROL_OPTIONS_2_OF_2")].as_slice(),
        )
    };
    for (x, y, id) in title {
        ctx.text(c, "BlackChancery22", &s(id), x - 100.0, 200.0, *y, true, gold);
    }
    c.spawn((ctx.node(l, t, r - l, 26.0), BackgroundColor(Color::srgba(0.1, 0.4, 0.6, 0.8))));
    ctx.text(c, "UniversBold14", &s("IDGS_TPFRONTENDTEXT_SCREENS_CONTROL"), l + 12.0, 150.0, t + 13.0, false, Color::WHITE);
    ctx.text(c, "UniversBold14", &s("IDGS_TPFRONTENDTEXT01_SELECT_KEY"), l + (r - l) * 0.6, 100.0, t + 13.0, false, Color::WHITE);
    c.spawn((ctx.node(l, t + 26.0, r - l, b - t - 26.0), BackgroundColor(Color::srgba(0.0, 0.1, 0.3, 0.6))));
    let row_h = (b - t - 30.0) / rows as f32;
    for (n, (_, label, key)) in binds.rows.iter().enumerate().skip(cui.scroll).take(rows) {
        let y = t + 30.0 + (n - cui.scroll) as f32 * row_h;
        let key = if cui.capture == Some(n) { s("IDGS_TPFRONTENDTEXT_SCREENS_PRESS_A_KEY") } else { crate::bindings::key_name(*key).to_string() };
        let mut row = c.spawn((ctx.node(l, y, r - l, row_h), Interaction::None, Action::Rebind(n), BackgroundColor(Color::NONE), Rest(Color::NONE)));
        row.with_children(|c| {
            // Measured on the original: rows in Univers10, the header in UniversBold14 (the widget declares Univers12).
            ctx.text(c, "Univers10", label, 12.0, 200.0, row_h / 2.0, false, Color::WHITE);
            ctx.text(c, "Univers10", &key, (r - l) * 0.6, 100.0, row_h / 2.0, false, if cui.capture == Some(n) { gold } else { Color::WHITE });
        });
    }
    // Scroll thumb (the mouse wheel scrolls).
    let (sx, st, sh) = if ingame { (592.0, 239.0, 132.0) } else { (683.0, 189.0, 271.0) };
    let max = binds.rows.len().saturating_sub(rows).max(1) as f32;
    let thumb = (sh * rows as f32 / binds.rows.len() as f32).max(20.0);
    c.spawn((ctx.node(sx, st, 18.0, sh), BackgroundColor(Color::srgba(0.0, 0.05, 0.2, 0.7))));
    c.spawn((ctx.node(sx, st + (sh - thumb) * cui.scroll as f32 / max, 18.0, thumb), BackgroundColor(Color::srgb(0.1, 0.2, 0.45))));
    let rect = if ingame { [450.0, 396.0, 609.0, 426.0] } else { [143.0, 481.0, 301.0, 511.0] };
    ctx.text_button(c, rect, &s("IDGS_TPFRONTENDTEXT_SCREENS_RESTORE_DEFAULTS"), "BlackChancery16", Action::RestoreKeys);
}

/// Sound Options / Display Options (`Audio_Options.tst`, `Display Options Screen.tst`): one row per option, label on the left and a
/// button with its value on the right at the retail rows' x; a click steps the option. PROVISIONAL: the retail scrollbars are
/// stepping buttons, and only the options with a remake equivalent are listed.
fn sound_display_page(ctx: &Ctx, c: &mut ChildSpawnerCommands, display: bool, ingame: bool, strings: &Value, settings: &crate::menu::Settings) {
    let s = |id: &str| text_of(strings, id);
    settings_frame(ctx, c, ingame, (Page::Options, Screen::Options));
    let title = if display { "IDGS_TPFRONTENDTEXT_SCREENS_DISPLAY_OPTIONS" } else { "IDGS_TPFRONTENDTEXT_SCREENS_SOUND_OPTIONS" };
    ctx.text(c, "BlackChancery22", &s(title), 250.0, 300.0, if ingame { 190.0 } else { 160.0 }, true, SETTINGS_GOLD);
    let (top, step, l, r) = if ingame { (215.0, 38.0, 170.0, 630.0) } else { (205.0, 50.0, 60.0, 700.0) };
    for (i, (setting, label, value)) in settings.rows(display).into_iter().enumerate() {
        let y = top + step * i as f32;
        ctx.text(c, if display { "UniversLightBold14" } else { "UniversBold14" }, label, l, 220.0, y + 16.0, false, Color::WHITE);
        ctx.text_button(c, [l + (r - l) * 0.5, y, r, y + 32.0], &value, "BlackChancery16", Action::Setting(setting));
    }
    if display {
        let y = top + step * 4.0;
        ctx.text_button(
            c,
            [l, y, l + 200.0, y + 32.0],
            &s("IDGS_TPFRONTENDTEXT_SCREENS_RESTORE_DEFAULTS"),
            "BlackChancery16",
            Action::Setting(crate::menu::Setting::RestoreDisplay),
        );
    }
}

/// Title gold of the Options pages.
const SETTINGS_GOLD: Color = Color::srgb(1.0, 0.85, 0.2);

/// The frame of the Options pages: the in-game window or the front-end backdrop, and the Prev arrow back to `back` (the in-game page or
/// the front-end screen). Retail 'Open Prev Arrow Button' areas: [357,397,447,487] in game, [351,500,453,602] in the front end.
fn settings_frame(ctx: &Ctx, c: &mut ChildSpawnerCommands, ingame: bool, (page, screen): (Page, Screen)) {
    if ingame {
        ingame_window(ctx, c, [140.0, 132.0, 660.0, 475.0]);
    } else {
        frontend_backdrop(ctx, c, 1);
    }
    let back = Action::Goto(if ingame { Screen::Battle } else { screen }).with_page(ingame.then_some(page));
    let ([cx, cy], r) = if ingame { ([402.0, 442.0], 45.0) } else { ([402.0, 551.0], 51.0) };
    arrow(ctx, c, [cx - r, cy - r, cx + r, cy + r], "FrontEnd_Prev_Button", back);
}

/// The in-game window frame (top-left 672x446 of the 768x512 `fe_ingame_window` sheet) stretched over `[l, t, r, b]`.
fn ingame_window(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4]) {
    ctx.image(c, "menu/fe_ingame_window.png", Some(Rect::new(10.0, 38.0, 682.0, 484.0)), l, t, r - l, b - t, Color::WHITE);
}

/// First flag icon of a fleet type's block of ten (`FrontEnd_Flag_*Icon_NN`): Navy 0, Pirate 10, Procyon 20.
fn flag_base(race: u8) -> usize {
    match race {
        2 => 10,
        3 => 20,
        _ => 0,
    }
}

/// Where a 'Front End Dialog Screen Backdrop' frame (`frontenddialogscreen.png`) is drawn for its ReferenceArea `[l, t, r, b]`:
/// centred on the screen, top at the area's top, scaled by 653/485 x 256/242. Fitted on the in-game menu (area [101,185,586,427]
/// measured at x 74..727, y 185..441); it puts the frame's inner area around the dialog's content (in-game menu buttons x 174..629,
/// map description text x 146..641). PROVISIONAL: one measured dialog.
fn dialog_frame([l, t, r, b]: [f32; 4]) -> [f32; 4] {
    let (w, h) = ((r - l) * 653.0 / 485.0, (b - t) * 256.0 / 242.0);
    [400.0 - w / 2.0, t, w, h]
}

/// The front-end frame around a "Front End Screen Backdrop" panel (y 113..507), as on original screenshots: grid letterbox strips above
/// and below (`FrontEnd_Letterbox_Tile`, mirrored below), one of the `FrontEndScreen0..4` scenes cropped to its 394 picture rows,
/// the faint `FrontEndMap` contour overlay and the `FrontEnd_TP_Logo` centred at the top. The engine draws these, not the screen files.
/// PROVISIONAL: which scene a screen shows is taken from screenshots (Skirmish 2, Options pages 1), 0 elsewhere.
fn frontend_backdrop(ctx: &Ctx, c: &mut ChildSpawnerCommands, scene: usize) {
    if let Some((_, r)) = sprite(ctx.hud, "FrontEnd_Letterbox_Tile", IDLE) {
        let (w, h) = (r.width(), r.height());
        let mut x = 0.0;
        while x < 800.0 {
            for (y, flip) in [(113.0 - h, false), (507.0, true)] {
                piece_flipped(ctx, c, "FrontEnd_Letterbox_Tile", [x, y, x + w, y + h], (false, flip));
            }
            x += w;
        }
    }
    ctx.image(c, &format!("menu/frontendscreen{scene}.png"), Some(Rect::new(0.0, 0.0, 800.0, 394.0)), 0.0, 113.0, 800.0, 394.0, Color::WHITE);
    ctx.image(c, "menu/frontendmap.png", None, 0.0, 113.0 + (394.0 - 375.0) / 2.0, 800.0, 375.0, Color::WHITE);
    if let Some((_, r)) = sprite(ctx.hud, "FrontEnd_TP_Logo", IDLE) {
        piece(ctx, c, "FrontEnd_TP_Logo", [400.0 - r.width() / 2.0, 0.0, 400.0 + r.width() / 2.0, r.height()]);
    }
}

/// Map list as measured on the original: a framed panel (x 280..712, y 120..318) with a lighter header bar holding 'Historical Maps' /
/// 'Open Maps' and 'Players' in UniversBold14, Univers12 rows 19 apart from y 172 with the playable player count at x 578, a teal bar
/// behind the selected map, and the retail scrollbar at x 727..752. PROVISIONAL: header bar and selection colours are matched by eye.
/// Rows of the Create Skirmish map list (the original's eight open maps fill it exactly).
const MAP_ROWS: usize = 8;

fn map_list(ctx: &Ctx, c: &mut ChildSpawnerCommands, sk: &Skirmish, strings: &Value) {
    let (l, r) = (280.0, 712.0);
    stretched_window(ctx, c, [l, 120.0, r, 318.0]);
    c.spawn((ctx.node(l + 4.0, 122.0, r - l - 8.0, 30.0), BackgroundColor(HEADING)));
    let heading = if sk.historical { "IDGS_TPFRONTENDTEXT01_VIEW_PRESET_MAPS" } else { "IDGS_TPFRONTENDTEXT01_VIEW_OPEN_MAPS" };
    ctx.text(c, "UniversBold14", &text_of(strings, heading), 298.0, 240.0, 136.0, false, Color::WHITE);
    ctx.text(c, "UniversBold14", &text_of(strings, "IDGS_TPFRONTENDTEXT01_PLAYERS"), 540.0, 80.0, 136.0, true, Color::WHITE);
    let rows: Vec<_> = sk.maps.iter().enumerate().filter(|(_, m)| m.2 != sk.historical).collect();
    for (row, (i, m)) in rows.iter().enumerate().skip(sk.map_scroll).take(MAP_ROWS) {
        let cy = 172.0 + (row - sk.map_scroll) as f32 * 19.0;
        let mut b = c.spawn((Interaction::None, Action::PickMap(*i), ctx.node(l + 4.0, cy - 9.5, r - l - 8.0, 19.0)));
        b.insert(BackgroundColor(if *i == sk.selected { PICKED } else { Color::NONE }));
        b.with_children(|c| {
            ctx.text(c, BODY_FONT, &m.1, 14.0, 240.0, 9.5, false, Color::WHITE);
            ctx.text(c, BODY_FONT, &m.3.to_string(), 278.0, 40.0, 9.5, true, Color::WHITE);
        });
    }
    scrollbar(ctx, c, [727.0, 155.0, 752.0, 322.0], ScrollKind::Maps, rows.len(), MAP_ROWS, sk.map_scroll);
}

/// Lobby player table as measured on the original: a framed panel (x 222..782, y 122..448) with a 'Flag / Player / Fleet / Team' header
/// in UniversBold14 and one row per playable player, 32 apart from y 184: the small flag icon, then Univers10 text in retail pull-down
/// boxes (Player x 287..460, Fleet 470..561, Team 571..779 with its drop-down arrow). Clicking a player's box takes that slot, the
/// fleet box cycles the fleet type and the team box the team (open maps). PROVISIONAL: the human row shows "Player" (no profiles),
/// the flag icon is the race's icon block plus the slot number, and closing a slot on an open map is a click on its flag.
/// 'Team N' on free-for-all maps (the original's TEAM_1 .. TEAM_8 strings).
fn team_label(s: &dyn Fn(&str) -> String, team: i32) -> String {
    s(&format!("IDGS_TPTEAMNAMES_TEAM_{}", team + 1))
}

fn players_list(ctx: &Ctx, c: &mut ChildSpawnerCommands, _rect: [f32; 4], sk: &Skirmish, strings: &Value) {
    let s = |id: &str| strings[id].as_str().unwrap_or(id).to_string();
    let open = sk.open();
    stretched_window(ctx, c, [222.0, 122.0, 782.0, 448.0]);
    c.spawn((ctx.node(226.0, 124.0, 552.0, 30.0), BackgroundColor(HEADING)));
    for (id, x0, x1) in [
        ("IDGS_TPFRONTENDTEXT03_FLAG", 236.0, 292.0),
        ("IDGS_TPFRONTENDTEXT_SCREENS_PLAYER", 287.0, 460.0),
        ("IDGS_TPFRONTENDTEXT03_FLEET", 470.0, 561.0),
        ("IDGS_TPFRONTENDTEXT02_TEAM", 571.0, 779.0),
    ] {
        ctx.text(c, "UniversBold14", &s(id), x0, x1 - x0, 138.0, true, Color::WHITE);
    }
    let teams = sk.map_json["teams"].as_array().cloned().unwrap_or_default();
    for (i, slot) in sk.slots.iter().enumerate() {
        let cy = 184.0 + i as f32 * 32.0;
        let me = slot.kind == Kind::Human;
        let live = slot.kind.live();
        let base = flag_base(slot.race);
        if live {
            let mut flag = c.spawn((ctx.node(250.0, cy - 12.5, 28.0, 25.0), Interaction::None));
            if me {
                flag.insert(Action::FlagPicker(true));
            }
            flag.with_children(|c| {
                piece(ctx, c, &format!("FrontEnd_Flag_Small_Icon_{:02}", base + usize::from(slot.flag)), [0.0, 0.0, 28.0, 25.0]);
            });
        }
        // Historical maps tell several live players of one team apart by a letter (Pirates A, B, C); open maps show the plain team name (measured on Border Dispute).
        let mates: Vec<usize> = sk.slots.iter().enumerate().filter(|(_, o)| o.kind.live() && o.team == slot.team && slot.team >= 0).map(|(k, _)| k).collect();
        let letter = match mates.iter().position(|&k| k == i) {
            Some(n) if mates.len() > 1 && !open => format!(" {}", (b'A' + n as u8) as char),
            _ => String::new(),
        };
        let team = match usize::try_from(slot.team).ok().and_then(|n| teams.get(n)) {
            Some(tm) => s(tm["name_id"].as_str().unwrap_or("")) + &letter,
            None if slot.team >= 0 => team_label(&s, slot.team),
            None => "-".into(),
        };
        let race = RACES.iter().find(|r| r.0 == slot.race).map_or_else(|| s("IDGS_TPFRONTENDTEXT01_PLEASE_SELECT_A_RACE"), |r| s(r.2));
        let kind = match slot.kind {
            Kind::Human => s("IDGS_TPFRONTENDTEXT_SCREENS_PLAYER"),
            Kind::Computer => s("IDGS_TPFRONTENDTEXT01_COMPUTER"),
            Kind::Open => s("IDGS_TPFRONTENDTEXT01_OPEN"),
            Kind::Closed => s("IDGS_TPFRONTENDTEXT01_CLOSED"),
        };
        let pull = |cell: Cell| (!sk.choices(cell).is_empty()).then_some(Action::Dropdown(Some(cell)));
        // Historical maps: clicking a name takes that side (no pull-down there).
        let player = pull(Cell::Player(i)).or((!open && live && !me).then_some(Action::Claim(i)));
        let mut cells = vec![(287.0, 460.0, kind, player.clone(), player.is_some() && open)];
        if live {
            cells.push((470.0, 561.0, race, pull(Cell::Fleet(i)), sk.free_teams()));
            cells.push((571.0, 779.0, team, pull(Cell::Team(i)), sk.free_teams()));
        }
        for (x0, x1, label, action, drop) in cells {
            pulldown_cell(ctx, c, [x0, cy - 14.5, x1, cy + 14.5], &label, false, action, drop);
        }
    }
    if sk.flag_picker {
        // Retail: a modal gold frame over the lobby with the fleet type's ten flags, two rows of five 75 apart; only a pick closes it.
        let base = flag_base(sk.human().map_or(0, |h| h.race));
        c.spawn((Interaction::None, bevy::ui::FocusPolicy::Block, Action::FlagPicker(true), ctx.node(0.0, 0.0, 800.0, 600.0), ZIndex(1)));
        c.spawn((ctx.node(150.0, 175.0, 475.0, 245.0), ZIndex(2))).with_children(|c| {
            ctx.image(c, "menu/frontenddialogscreen.png", None, 0.0, 0.0, 475.0, 245.0, Color::WHITE);
            for k in 0..10usize {
                let (x, y) = (87.0 + (k % 5) as f32 * 75.0 - 33.5, 76.0 + (k / 5) as f32 * 75.0 - 33.5);
                // A flag another fleet of this type already flies is shown dimmed and cannot be taken.
                let taken = sk.human().is_some_and(|h| sk.flag_taken(sk.slots.iter().position(|s| s.kind == Kind::Human).unwrap_or(0), h.race, k as u8));
                let mut b = c.spawn((ctx.node(x, y, 67.0, 67.0), Interaction::None, bevy::ui::FocusPolicy::Block));
                if !taken {
                    b.insert(Action::PickFlag(k as u8));
                }
                b.with_children(|c| {
                    piece(ctx, c, &format!("FrontEnd_Flag_Icon_{:02}", base + k), [0.0, 0.0, 67.0, 67.0]);
                    if taken {
                        c.spawn((ctx.node(0.0, 0.0, 67.0, 67.0), BackgroundColor(Color::srgba(0.0, 0.05, 0.15, 0.65))));
                    }
                });
            }
        });
    }
    if let Some(cell) = sk.dropdown {
        // The open list sits on its cell: the current value first, the other entries 28 apart below (retail).
        let (x0, x1, row) = match cell {
            Cell::Player(i) => (287.0, 460.0, i),
            Cell::Fleet(i) => (470.0, 561.0, i),
            Cell::Team(i) => (571.0, 779.0, i),
        };
        let entries = sk.choices(cell).into_iter().map(|choice| {
            let label = match choice {
                Choice::Computer => s("IDGS_TPFRONTENDTEXT01_COMPUTER"),
                Choice::Open => s("IDGS_TPFRONTENDTEXT01_OPEN"),
                Choice::Close => s("IDGS_TPFRONTENDTEXT_SCREENS_CLOSE"),
                Choice::Fleet(r) => RACES.iter().find(|x| x.0 == r).map_or_else(String::new, |x| s(x.2)),
                Choice::Team(t) => team_label(&s, t),
            };
            (label, Action::Choose(cell, choice))
        });
        dropdown(ctx, c, Action::Dropdown(None), [x0, 184.0 + row as f32 * 32.0 - 14.5, x1 - x0, 28.0], entries.collect(), Some(0));
    }
    if let Some(e) = sk.error {
        ctx.text(c, BODY_FONT, &s(e), 236.0, 540.0, 184.0 + 8.0 * 32.0, false, Color::srgb(1.0, 0.4, 0.3));
    }
}

/// The lobby's Map Description dialog (`Map Description Dialog Screen.tst`): the dialog frame, the map description in Univers10 inside
/// 'Map Information Button' [146,207,641,330] and a 'Back' button [324,339,457,383]. A full-screen blocker keeps the lobby beneath
/// from reacting. The frame follows `dialog_frame` for the screen's backdrop area [61,165,603,436].
fn map_info_dialog(ctx: &Ctx, c: &mut ChildSpawnerCommands, sk: &Skirmish, strings: &Value) {
    c.spawn((ctx.node(0.0, 0.0, 800.0, 600.0), Interaction::None, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35))));
    let [x, y, w, h] = dialog_frame([61.0, 165.0, 603.0, 436.0]);
    ctx.image(c, "menu/frontenddialogscreen.png", None, x, y, w, h, Color::WHITE);
    wrapped(ctx, c, "Univers10", sk.map_json["description"].as_str().unwrap_or_default(), [146.0, 207.0, 641.0, 330.0]);
    ctx.text_button(c, [324.0, 339.0, 457.0, 383.0], &text_of(strings, "IDGS_TPFRONTENDTEXT01_BACK"), "BlackChancery16", Action::MapInfo(false));
}

/// The Load / Save dialog of fleets, crews and arms (`Fleet File Dialog Screen.tst` and its Crew and Arms twins, same layout): the frame
/// over the screen, the profile list with its title, the picked profile's victory points, the file name entry, Save / Load and Cancel.
/// A full-screen blocker keeps what is beneath from reacting; it does not dim it (retail shows the screen behind through the frame art's
/// translucent middle, undimmed). PROVISIONAL: title and number positions are eyeballed from a screenshot.
pub(super) fn profile_dialog(ctx: &Ctx, c: &mut ChildSpawnerCommands, sk: &Skirmish, lib: &Library, strings: &Value) {
    use crate::profiles::Kind as K;
    let Some(d) = &sk.profile else { return };
    let s = |id: &str| text_of(strings, id);
    c.spawn((ctx.node(0.0, 0.0, 800.0, 600.0), Interaction::None, bevy::ui::FocusPolicy::Block, ZIndex(1)));
    c.spawn((ctx.node(0.0, 0.0, 800.0, 600.0), ZIndex(2))).with_children(|c| {
        let [x, y, w, h] = dialog_frame([69.0, 169.0, 603.0, 420.0]);
        ctx.image(c, "menu/frontenddialogscreen.png", None, x, y, w, h, Color::WHITE);
        let title = match d.kind {
            K::Fleet => "IDGS_TPFRONTENDTEXT01_PROFILES_FLEET",
            K::Crew => "IDGS_TPFRONTENDTEXT01_PROFILES_CREW",
            K::Arms => "IDGS_TPFRONTENDTEXT01_PROFILES_ARMS",
        };
        ctx.text(c, "UniversBold14", &s(title), 189.0, 297.0, 232.0, true, Color::WHITE);
        for (i, p) in d.files.iter().enumerate().skip(d.scroll).take(crate::menu::PROFILE_ROWS) {
            cell(ctx, c, [189.0, 248.0 + (i - d.scroll) as f32 * 19.0, 297.0, 18.0], &p.name, d.picked == Some(i), Some(Action::PickProfile(i)));
        }
        ctx.text(c, "UniversBold14", &s("IDGS_TPFRONTENDTEXT_SCREENS_VICTORYPOINTS"), 500.0, 134.0, 232.0, true, Color::WHITE);
        if let Some(p) = d.picked.and_then(|n| d.files.get(n)) {
            ctx.text(c, "UniversBold14", &p.points(lib).to_string(), 500.0, 134.0, 262.0, true, Color::srgb(1.0, 0.9, 0.4));
        }
        let (go, ready) = if d.save { ("IDGS_TPFRONTENDTEXT_SCREENS_SAVE", !d.name.trim().is_empty()) } else { ("IDGS_TPFRONTENDTEXT_SCREENS_LOAD", d.picked.is_some()) };
        ctx.button(c, [505.0, 284.0, 627.0, 328.0], &s(go), "BlackChancery16", ready.then_some(Action::DoProfile));
        ctx.text_button(c, [505.0, 337.0, 627.0, 381.0], &s("IDGS_TPFRONTENDTEXT_SCREENS_CANCEL"), "BlackChancery16", Action::CloseProfile);
        ctx.text(c, BODY_FONT, &s("IDGS_TPFRONTENDTEXT_SCREENS_FILENAME"), 128.0, 135.0, 359.5, true, Color::WHITE);
        let typed = if d.save { format!("{}_", d.name) } else { String::new() };
        cell(ctx, c, [278.0, 341.0, 159.0, 41.0], &typed, false, None::<Action>);
        // Why the last Save or Load was refused, inside the frame above the file name (it used to sit on the frame's border).
        if let Some(e) = d.error {
            ctx.text(c, BODY_FONT, e, 128.0, 309.0, 322.0, true, Color::srgb(1.0, 0.45, 0.35));
        }
    });
}

const SHIP_ROWS: usize = 8;

/// The ship-name pull-down at the top left of the preview, shown while a fleet ship is displayed: the name and the pull-down arrow;
/// holding it lists the hull's other names 19 apart under it (measured with the retail driver). PROVISIONAL: the box is the lobby's
/// pull-down art, retail draws a plainer outlined box that grows with the name.
fn ship_name_field(ctx: &Ctx, c: &mut ChildSpawnerCommands, sk: &Skirmish, lib: &Library) {
    let Some(h) = sk.human().filter(|_| sk.view_slot && sk.slot_filled()) else { return };
    let Some(name) = h.ship_names(lib).get(sk.fleet_pick).cloned().flatten() else { return };
    let w = ctx.fonts.0.get("Univers10").map_or(120.0, |f| f.width(&name)) + 44.0;
    pulldown_cell(ctx, c, [55.0, 94.0, 55.0 + w, 116.0], &name, false, Some(Action::NameList(!sk.name_open)), true);
    if sk.name_open {
        let names = sk.slots.iter().find(|s| s.kind == Kind::Human).map_or_else(Vec::new, |h| h.name_choices(lib, sk.fleet_pick));
        let entries = names.into_iter().map(|n| (n.clone(), Action::PickName(n))).collect();
        dropdown(ctx, c, Action::NameList(false), [55.0, 118.5, 140.0, 19.0], entries, None);
    }
}

/// Ship Types Available: the offered hulls of the human slot's fleet type (mouse wheel scrolls), the selected one highlighted.
/// 'Available Ships' as measured on the original: UniversBold14 header centred at y 311, Univers12 rows from x 525, 19 apart from
/// y 347.5, a teal selection bar (x 505..720) and the retail scrollbar at x 742..767; the box itself is part of the backdrop.
fn ship_list(ctx: &Ctx, c: &mut ChildSpawnerCommands, _rect: [f32; 4], sk: &Skirmish, lib: &Library, strings: &Value) {
    ctx.text(c, "UniversBold14", &text_of(strings, "IDGS_TPFRONTENDTEXT01_AVAILABLE_SHIPS"), 505.0, 215.0, 311.0, true, Color::WHITE);
    let list = sk.human().map(|h| offered(lib, h.race)).unwrap_or_default();
    for (row, &ci) in list.iter().enumerate().skip(sk.scroll).take(SHIP_ROWS) {
        let y = 338.0 + (row - sk.scroll) as f32 * 19.0;
        list_row(ctx, c, [505.0, y, 215.0, 19.0], &class_name(lib, ci, strings), sk.ship_chosen && row == sk.ship_pick, Action::PickShip(row));
    }
    scrollbar(ctx, c, [742.0, 330.0, 767.0, 505.0], ScrollKind::Ships, list.len(), SHIP_ROWS, sk.scroll);
}

/// Fleet Select: the assembled fleet on the slot bar (`fleet_bar`); the picked entry is the one Drop removes.
fn fleet_list(ctx: &Ctx, c: &mut ChildSpawnerCommands, sk: &Skirmish, lib: &Library) {
    let fleet = sk.human().map(|h| h.fleet.as_slice()).unwrap_or_default();
    let icons: Vec<_> = fleet.iter().map(|n| lib.classes.iter().find(|cl| &cl.name == n).and_then(|cl| cl.icon.as_deref())).collect();
    fleet_bar(ctx, c, (sk.slot_scroll, sk.fleet_size), &icons, None, Action::SlotScroll, Action::PickFleet);
}

/// A class's displayable name ('Displayable Shipname String ID'), else its WOT name tidied up.
pub(super) fn class_name(lib: &Library, ci: usize, strings: &Value) -> String {
    let cl = &lib.classes[ci];
    cl.display_name.as_deref().and_then(|id| strings[id].as_str()).map_or_else(|| pretty(&cl.name), String::from)
}

/// Ship Information for the picked hull (`info_panel`): seven stat lines and the point cost, as retail.
fn ship_info(ctx: &Ctx, c: &mut ChildSpawnerCommands, _rect: [f32; 4], sk: &Skirmish, lib: &Library, strings: &Value) {
    let Some(ci) = sk.shown_class(lib) else { return };
    let s = |id: &str| text_of(strings, id);
    let st = crate::menu::ship_stats(lib, ci);
    let rows = [
        (s("IDGS_TPFRONTENDTEXT_SHIP_HITPOINTS"), st.hit_points.to_string()),
        (s("IDGS_TPFRONTENDTEXT_MAX_SPEED"), format!("{} km/h", st.max_speed_kmh)),
        (s("IDGS_TPFRONTENDTEXT_MANEUVERABILITY"), s(st.turning)),
        (s("IDGS_TPFRONTENDTEXT_CREW"), st.crew.to_string()),
        (s("IDGS_TPFRONTENDTEXT_SHIP_MOUNTS_HEAVY"), st.heavy.to_string()),
        (s("IDGS_TPFRONTENDTEXT_SHIP_MOUNTS_MEDIUM"), st.medium.to_string()),
        (s("IDGS_TPFRONTENDTEXT_SHIP_MOUNTS_LIGHT"), st.light.to_string()),
    ];
    let cost = [s("IDGS_TPFRONTENDTEXT02_POINT_COST01"), s("IDGS_TPFRONTENDTEXT02_POINT_COST02")];
    info_panel(ctx, c, &class_name(lib, ci, strings), lib.classes[ci].icon.as_deref(), Some(st.points), &rows, cost.each_ref().map(String::as_str));
}

/// Mouse wheel scrolls the Ship Types Available list.
fn wheel(mut wheel: MessageReader<bevy::input::mouse::MouseWheel>, state: Res<State<Screen>>, mut sk: ResMut<Skirmish>, lib: Res<Library>) {
    let dy: f32 = wheel.read().map(|e| e.y).sum();
    if dy == 0.0 {
        return;
    }
    if *state.get() == Screen::CreateSkirmish {
        let n = sk.maps.iter().filter(|m| m.2 != sk.historical).count();
        let max = n.saturating_sub(MAP_ROWS);
        sk.map_scroll = wheel_step(sk.map_scroll, dy, max);
        return;
    }
    if *state.get() != Screen::Fleet {
        return;
    }
    let n = sk.human().map_or(0, |h| offered(&lib, h.race).len());
    let max = n.saturating_sub(SHIP_ROWS);
    sk.scroll = wheel_step(sk.scroll, dy, max);
}

fn scroll_controls(mut wheel: MessageReader<bevy::input::mouse::MouseWheel>, state: Res<State<Screen>>, ig: Res<InGame>, mut cui: ResMut<CtrlUi>, binds: Res<Bindings>) {
    let dy: f32 = wheel.read().map(|e| e.y).sum();
    if dy != 0.0 && (*state.get() == Screen::Controls || ig.page() == Some(Page::Controls)) {
        let max = binds.rows.len().saturating_sub(if ig.page().is_some() { 8 } else { 17 });
        cui.scroll = wheel_step(cui.scroll, dy, max);
    }
}

/// Mouse wheel scrolls the save lists.
fn scroll_saves(mut wheel: MessageReader<bevy::input::mouse::MouseWheel>, state: Res<State<Screen>>, ig: Res<InGame>, mut ui: ResMut<SaveUi>) {
    let dy: f32 = wheel.read().map(|e| e.y).sum();
    if dy != 0.0 && (*state.get() == Screen::Load || matches!(ig.page(), Some(Page::Load | Page::Save))) {
        let max = ui.list.len().saturating_sub(10);
        ui.scroll = wheel_step(ui.scroll, dy, max);
    }
}

#[cfg(test)]
mod frame_tests {
    use super::dialog_frame;

    #[test]
    fn dialog_frame_matches_the_measured_in_game_menu() {
        let [x, y, w, h] = dialog_frame([101.0, 185.0, 586.0, 427.0]);
        assert!((x - 73.5).abs() < 0.5 && y == 185.0 && (w - 653.0).abs() < 0.5 && (h - 256.0).abs() < 0.5);
    }
}
