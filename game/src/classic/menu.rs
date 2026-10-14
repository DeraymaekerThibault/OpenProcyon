//! Classic (retail) front-end screens: the retail Screens/*.tst element rectangles (800x600 reference, scaled to the window), texts
//! from the retail string table, button/list/arrow art from the hud.hdt FrontEnd_* sprites with their NoCursor/CursorOver/CursorDown
//! states, the backdrop BD_GUITextures FrontEndScreen0. State and actions live in `menu.rs`; this only draws and scrolls.
//! PROVISIONAL: which element shows the map picture (Lobby Information area), the description font, the fleet editor
//! (click a slot to cycle ship classes; the retail Ship Selection screen is not done), no transitions.

#![allow(deprecated)] // shortcut: bevy 0.20 Interaction (deprecated for picking Hovered/Pressed); port when it is removed
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use serde_json::Value;

use crate::assets_dir;
use crate::bindings::Bindings;
use crate::combat::Library;
use crate::menu::{
    Action, BuildMenu, CtrlUi, InGame, Kind, MenuKey, MenuModel, MenuRoot, Page, RACES, RunButtons, SaveUi, Screen, Skirmish, menu_active, offered, pretty, strings, text_of,
};
use crate::sprites::{Fonts, HudData, MENU_SKIN, inset, sliced};

pub fn plugin(app: &mut App) {
    app.add_systems(Update, ((scroll_controls, scroll_saves).before(BuildMenu), build.in_set(BuildMenu), (hover, wheel).after(RunButtons)).run_if(menu_active));
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
            Interaction::None => "NoCursor",
        };
        for (e, name) in art.map(|a| a.0.as_slice()).unwrap_or(&[]) {
            if let Some(group) = name.strip_prefix("slice:") {
                if let Ok(mut img) = imgs.get_mut(*e) {
                    img.image = assets.load(format!("hud/tex/slice_{group}_{}_{}.png", MENU_SKIN.to_lowercase(), state.to_lowercase()));
                }
                continue;
            }
            if let (Ok(mut img), Some((sheet, r))) = (imgs.get_mut(*e), sprite(&hud, name, state)) {
                img.image = assets.load(format!("hud/tex/{sheet}.png"));
                img.rect = Some(inset(&sheet, r));
            }
        }
    }
}

/// Hover/press art: the sprite names of a button's pieces, swapped between NoCursor / CursorOver / CursorDown.
#[derive(Component)]
struct Art(Vec<(Entity, String)>);

/// Sprite rect of a hud.hdt component for a cursor state ("NoCursor", "CursorOver", "CursorDown").
fn sprite(hud: &HudData, name: &str, state: &str) -> Option<(String, Rect)> {
    let d = hud.comps.get(name)?.get(MENU_SKIN)?;
    let r = d[format!("TextureArea - {state}")].as_array()?;
    let n = |i: usize| r[i].as_f64().unwrap_or(0.0) as f32;
    Some((d[format!("TextureName - {state}")].as_str()?.to_lowercase(), Rect::new(n(0), n(1), n(2) + 1.0, n(3) + 1.0)))
}

struct Ctx<'a> {
    assets: &'a AssetServer,
    hud: &'a HudData,
    fonts: &'a Fonts,
    scale: f32,
}

impl Ctx<'_> {
    fn node(&self, x: f32, y: f32, w: f32, h: f32) -> Node {
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(x * self.scale),
            top: Val::Px(y * self.scale),
            width: Val::Px(w * self.scale),
            height: Val::Px(h * self.scale),
            ..default()
        }
    }
    fn image(&self, c: &mut ChildSpawnerCommands, path: &str, rect: Option<Rect>, x: f32, y: f32, w: f32, h: f32, tint: Color) -> Entity {
        let mut img = ImageNode::new(self.assets.load(path.to_string()));
        img.rect = rect;
        img.color = tint;
        img.image_mode = NodeImageMode::Stretch;
        c.spawn((img, self.node(x, y, w, h))).id()
    }
    fn hud_piece(&self, c: &mut ChildSpawnerCommands, name: &str, x: f32, y: f32, w: f32, h: f32) -> Option<(Entity, String)> {
        let (sheet, r) = sprite(self.hud, name, "NoCursor")?;
        Some((self.image(c, &format!("hud/tex/{sheet}.png"), Some(inset(&sheet, r)), x, y, w, h, Color::WHITE), name.to_string()))
    }
    /// Retail bitmap text, horizontally centred in [x, x+w] (or left-aligned), vertically centred on `cy`.
    fn text(&self, c: &mut ChildSpawnerCommands, font: &str, s: &str, x: f32, w: f32, cy: f32, centre: bool, tint: Color) {
        let Some(f) = self.fonts.0.get(font).or_else(|| self.fonts.0.get("BlackChancery22")) else { return };
        let mut n = self.node(x, cy - f.height / 2.0, w, f.height);
        n.align_items = AlignItems::Center;
        n.justify_content = if centre { JustifyContent::Center } else { JustifyContent::FlexStart };
        c.spawn(n).with_children(|c| {
            f.glyphs(c, s, self.scale, tint);
        });
    }
    /// Text button over the 3-piece FrontEnd_Texts_Over_Button art (44 px tall in the sheet, stretched to the rect).
    fn text_button(&self, c: &mut ChildSpawnerCommands, rect: [f32; 4], label: &str, font: &str, action: Action) {
        let [l, t, r, b] = rect;
        let (w, h) = (r - l, b - t);
        let mut parts = vec![];
        let mut root = c.spawn((Interaction::None, action, self.node(l, t, w, h)));
        let cap = |name| sprite(self.hud, name, "NoCursor").map_or(50.0, |(_, r)| r.width());
        root.with_children(|c| {
            let sub = self;
            let e = c
                .spawn((
                    sliced(sub.assets, "button", MENU_SKIN, "nocursor", cap("FrontEnd_Texts_Over_Button_Left"), cap("FrontEnd_Texts_Over_Button_Right")),
                    sub.node(0.0, 0.0, w, h),
                ))
                .id();
            parts.push((e, "slice:button".to_string()));
            sub.text(c, font, label, 0.0, w, h / 2.0, true, Color::WHITE);
        });
        root.insert(Art(parts));
    }
}

/// Rebuilds the current screen when the state, the selection or the window size changes.
fn build(
    mut commands: Commands,
    assets: Res<AssetServer>,
    hud: Option<Res<HudData>>,
    fonts: Option<Res<Fonts>>,
    model: MenuModel,
    window: Single<&Window, With<PrimaryWindow>>,
    roots: Query<Entity, With<MenuRoot>>,
    mut last: Local<Option<MenuKey>>,
) {
    let (Some(hud), Some(fonts)) = (hud, fonts) else { return };
    if fonts.0.is_empty() {
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
    let MenuModel { sk, ig, saves: ui, binds, ctrl: cui, lib, state, settings } = model;
    let (ww, wh) = (window.width(), window.height());
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
    let els: Vec<Value> = std::fs::read_to_string(assets_dir().join(format!("menu/{file}.json"))).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let scale = (ww / 800.0).min(wh / 600.0);
    let ctx = Ctx { assets: &assets, hud: &hud, fonts: &fonts, scale };
    let canvas = Node {
        position_type: PositionType::Absolute,
        left: Val::Px((ww - 800.0 * scale) / 2.0),
        top: Val::Px((wh - 600.0 * scale) / 2.0),
        width: Val::Px(800.0 * scale),
        height: Val::Px(600.0 * scale),
        ..default()
    };
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    commands.spawn((MenuRoot, full, BackgroundColor(if ingame { Color::srgba(0.0, 0.0, 0.0, 0.5) } else { Color::BLACK }), GlobalZIndex(100))).with_children(|root| {
        root.spawn(canvas).with_children(|c| {
            if let Some(controls) = page {
                options_page(&ctx, c, controls, ingame, &strings, &binds, &cui);
            }
            if let Some(display) = settings_page {
                sound_display_page(&ctx, c, display, ingame, &strings, &settings);
            }
            for e in &els {
                let impl_ = e["impl"].as_str().unwrap_or("");
                let r: Vec<f32> = e["rect"].as_array().map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default();
                if r.len() != 4 || e["enabled"] == Value::Bool(false) {
                    continue;
                }
                let rect = [r[0], r[1], r[2], r[3]];
                let font = e["font"].as_str().unwrap_or("BlackChancery22");
                let label = e["text"].as_str().map(|t| text_of(&strings, t)).unwrap_or_default();
                let label = if impl_ == "Load or Save Campaign Button" && ig.page() == Some(Page::Save) { text_of(&strings, "IDGS_TPFRONTENDTEXT_SCREENS_SAVE") } else { label };
                // Element colour (`Color Colour(r,g,b,a)`): titles are a black shadow label plus the coloured one.
                let tint = e["color"].as_array().filter(|c| c.len() == 4).map_or(Color::WHITE, |c| {
                    let k = |i: usize| c[i].as_f64().unwrap_or(1.0) as f32;
                    Color::srgba(k(0), k(1), k(2), k(3))
                });
                let action = match (state.get(), impl_) {
                    (Screen::SinglePlayer, "Open Load Campaign Screen Button") => Some(Action::OpenLoad),
                    (Screen::Load, "Open Prev Arrow Button") => {
                        arrow(&ctx, c, rect, "FrontEnd_Prev_Button", Action::Goto(Screen::SinglePlayer));
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
                        ctx.image(c, "menu/fe_ingame_window.png", Some(Rect::new(10.0, 38.0, 682.0, 484.0)), r[0], r[1], r[2] - r[0], r[3] - r[1], Color::WHITE);
                        None
                    }
                    (Screen::Battle, "Open Next Screen Button") => Some(Action::Ingame(Some(Page::Options))),
                    (Screen::Main, "Open Option Screen") => Some(Action::Goto(Screen::Options)),
                    (Screen::Main, "Open Tutorial Button") => Some(Action::Goto(Screen::Tutorial)),
                    (Screen::Tutorial, "Start Basic Training Tutorial Button") => Some(Action::Tutorial("basic_tutorial")),
                    (Screen::Tutorial, "Start Full Training Tutorial Button") => Some(Action::Tutorial("tutorial")),
                    (Screen::Tutorial, "Open Prev Arrow Button") => {
                        arrow(&ctx, c, rect, "FrontEnd_Prev_Button", Action::Goto(Screen::Main));
                        None
                    }
                    (Screen::Battle, "Open Saved Campaign Screen Button") => Some(Action::Ingame(Some(Page::Save))),
                    (Screen::Battle, "Open Load Campaign Screen Button") => Some(Action::Ingame(Some(Page::Load))),
                    (Screen::Battle, "InGameMenu QuitGame Button") => Some(Action::QuitToMenu),
                    (Screen::Battle, "Open Previous Screen Button") => Some(Action::Ingame(if ig.page() == Some(Page::Menu) { None } else { Some(Page::Menu) })),
                    (Screen::Load | Screen::Battle, "Saved Campaign Scrollable List") => {
                        save_list(&ctx, c, rect, &ui, &strings);
                        None
                    }
                    (Screen::Load | Screen::Battle, "Load or Save Campaign Button") => Some(if ig.page() == Some(Page::Save) { Action::DoSave } else { Action::DoLoad }),
                    (Screen::Load | Screen::Battle, "Open Confirm Saved Game Delete Dialog Button") => Some(Action::Remove),
                    (Screen::Battle, "Campaign Text Entry") if ig.page() == Some(Page::Save) => {
                        cell(&ctx, c, [r[0], r[1], r[2] - r[0], r[3] - r[1]], &format!("{}_", ui.name), false, None);
                        None
                    }
                    (Screen::Battle, "Right Justify Label") if ig.page() == Some(Page::Save) && r[0] as i32 == 272 => {
                        ctx.text(c, "Univers12", &text_of(&strings, "IDGS_TPFRONTENDTEXT_SCREENS_FILENAME"), r[0], r[2] - r[0], (r[1] + r[3]) / 2.0, true, Color::WHITE);
                        None
                    }
                    (_, "Front End Screen Backdrop") => {
                        frontend_backdrop(&ctx, c, if *state.get() == Screen::CreateSkirmish { 2 } else { 0 });
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
                    (Screen::SinglePlayer, "Open Prev Arrow Button") | (Screen::CreateSkirmish, "Open Prev Arrow Button") => {
                        arrow(&ctx, c, rect, "FrontEnd_Prev_Button", Action::Goto(if *state.get() == Screen::SinglePlayer { Screen::Main } else { Screen::SinglePlayer }));
                        None
                    }
                    (Screen::CreateSkirmish, "Open Next Arrow Button") => {
                        arrow(&ctx, c, rect, "FrontEnd_Next_Button", Action::Goto(Screen::Lobby));
                        None
                    }
                    (Screen::CreateSkirmish, "List World Files") => {
                        map_list(&ctx, c, &sk, &strings);
                        None
                    }
                    (Screen::CreateSkirmish, "Map Information Button") => {
                        // Measured: framed panel at the element's area; gold 'Map Description:' then Univers10 lines 16 apart.
                        stretched_window(&ctx, c, [r[0], r[1] - 3.0, r[2], r[3] + 2.0]);
                        ctx.text(c, "Univers10", &format!("{}:", text_of(&strings, "IDGS_TPFRONTENDTEXT02_MAP_DESCRIPTION")), r[0] + 5.0, 200.0, r[1] + 9.0, false, GOLD);
                        wrapped(&ctx, c, "Univers10", sk.map_json["description"].as_str().unwrap_or_default(), [r[0] + 5.0, r[1] + 25.0, r[2] - 12.0, r[3]]);
                        None
                    }
                    (Screen::CreateSkirmish, "Radio Historical Map Button") => {
                        radio(&ctx, c, rect, &text_of(&strings, "IDGS_TPFRONTENDTEXT01_VIEW_PRESET_MAPS"), sk.historical, Action::Historical(true));
                        None
                    }
                    (Screen::CreateSkirmish, "Radio Open Map Button") => {
                        radio(&ctx, c, rect, &text_of(&strings, "IDGS_TPFRONTENDTEXT01_VIEW_OPEN_MAPS"), !sk.historical, Action::Historical(false));
                        None
                    }
                    // Fleet size (and the victory limit) only apply to open maps; historical maps bring their own fleets.
                    (Screen::CreateSkirmish, "Fleet Size Label" | "Fleet Size Scrollbar Button" | "Victory Limit Label" | "Victory Points Scrollbar Button") if sk.historical => {
                        None
                    }
                    (Screen::CreateSkirmish, "Fleet Size Label") => {
                        ctx.text(c, "Univers12", &text_of(&strings, "IDGS_TPFRONTENDTEXT01_FLEET_SIZE_LIMIT"), r[0], r[2] - r[0], (r[1] + r[3]) / 2.0, true, Color::WHITE);
                        None
                    }
                    (Screen::CreateSkirmish, "Fleet Size Scrollbar Button") => {
                        ctx.text_button(c, rect, &sk.fleet_size.to_string(), "BlackChancery22", Action::FleetSize);
                        None
                    }
                    (Screen::Lobby, "Open Next Screen Button") => Some(Action::Launch),
                    (Screen::Lobby, "Open Map Description Dialog Button") => Some(Action::MapInfo(true)),
                    (Screen::Lobby, "Open Previous Screen Button") => Some(Action::Goto(Screen::CreateSkirmish)),
                    (Screen::Lobby, "Open Ship Selection Screen") if sk.open() => Some(Action::Goto(Screen::Fleet)),
                    (Screen::Lobby, "Game Lobby Backdrop") => {
                        ctx.image(c, "menu/gamelobby.png", Some(Rect::new(0.0, 0.0, 800.0, 600.0)), 0.0, 0.0, 800.0, 600.0, Color::WHITE);
                        None
                    }
                    (Screen::Lobby, "Lobby Information Button") => {
                        // Measured: a gold Univers10 'Map' label and the map name in Univers12 under the title.
                        ctx.text(c, "Univers10", &text_of(&strings, "IDGS_TPFRONTENDTEXT01_SERVER_MAPNAME"), r[0], r[2] - r[0], 84.0, true, GOLD);
                        ctx.text(c, "Univers12", sk.map_json["name"].as_str().unwrap_or_default(), r[0], r[2] - r[0], 101.0, true, Color::WHITE);
                        None
                    }
                    (Screen::Lobby, "Lobby Player List Button") => {
                        players_list(&ctx, c, rect, &sk, &strings);
                        None
                    }
                    (Screen::Fleet, "Assemble Fleet Backdrop") => {
                        ctx.image(c, "menu/shipscreen.png", Some(Rect::new(0.0, 0.0, 800.0, 600.0)), 0.0, 0.0, 800.0, 600.0, Color::WHITE);
                        None
                    }
                    (Screen::Fleet, "Ship Types Available") => {
                        ship_list(&ctx, c, rect, &sk, &lib);
                        None
                    }
                    (Screen::Fleet, "Fleet Select Button") => {
                        fleet_list(&ctx, c, rect, &sk);
                        None
                    }
                    (Screen::Fleet, "Ship Information") => {
                        ship_info(&ctx, c, rect, &sk, &lib, &strings);
                        None
                    }
                    (Screen::Fleet, "Ship Victory Points") => {
                        let text = format!("{}: {}/{}", text_of(&strings, "IDGS_TPFRONTENDTEXT01_FLEET_SIZE_LIMIT"), sk.human().map_or(0, |h| h.fleet.len()), sk.fleet_size);
                        ctx.text(c, font, &text, r[0], r[2] - r[0], (r[1] + r[3]) / 2.0, true, Color::WHITE);
                        None
                    }
                    (Screen::Fleet, "Open Arms Selection Screen") => Some(Action::OpenArms),
                    (Screen::Fleet, "Add Ship To Fleet Button") => Some(Action::AddShip),
                    (Screen::Fleet, "Drop Ship From Fleet Button") => Some(Action::DropShip),
                    (Screen::Fleet, "Open Next Arrow Button") => {
                        arrow(&ctx, c, rect, "FrontEnd_Next_Button", Action::Goto(Screen::Lobby));
                        None
                    }
                    (Screen::Fleet, "Ship ViewPort Button") => None, // retail: rotating 3D preview of the selected hull (not done)
                    (_, "Victory Points Scrollbar Button") | (_, "Victory Limit Label") => None, // victory points: not done
                    // Other retail buttons (multiplayer, options, profiles, campaign...) are shown but not implemented.
                    (_, n) if !label.is_empty() && (n.starts_with("Open") || n.contains("Button")) && !n.contains("Arrow") => {
                        disabled(&ctx, c, rect, &label, font);
                        None
                    }
                    _ => None,
                };
                if let Some(a) = action {
                    ctx.text_button(c, rect, &label, font, a);
                }
            }
            if *state.get() == Screen::Lobby && sk.map_info {
                map_info_dialog(&ctx, c, &sk, &strings);
            }
        });
    });
}

/// Rows of a save list (19 px each, mouse wheel scrolls): "name - N pts".
fn save_list(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], ui: &SaveUi, strings: &Value) {
    let rows = ((b - t) / 19.0) as usize;
    for (i, (s, _)) in ui.list.iter().enumerate().skip(ui.scroll).take(rows) {
        cell(ctx, c, [l, t + (i - ui.scroll) as f32 * 19.0, r - l, 18.0], &format!("{}  -  {} pts", s.name, s.points), ui.selected == Some(i), Some(Action::PickSave(i)));
    }
    if let Some(m) = ui.status {
        ctx.text(c, "Univers12", strings[m].as_str().unwrap_or(m), l, r - l, t - 10.0, false, Color::srgb(1.0, 0.9, 0.4));
    }
}

/// Options and Control Options (front end: green-banner screen; in game: gold dialog). Rectangles are 800x600 canvas
/// coordinates measured from the retail screenshots (PROVISIONAL: no .tst exported).
fn options_page(ctx: &Ctx, c: &mut ChildSpawnerCommands, controls: bool, ingame: bool, strings: &Value, binds: &Bindings, cui: &CtrlUi) {
    let s = |id: &str| text_of(strings, id);
    let gold = Color::srgb(1.0, 0.85, 0.2);
    let back = |p: Page, f: Screen| Action::Goto(if ingame { Screen::Battle } else { f }).with_page(ingame.then_some(p));
    if ingame {
        ctx.image(c, "menu/fe_ingame_window.png", Some(Rect::new(10.0, 38.0, 682.0, 484.0)), 140.0, 132.0, 520.0, 343.0, Color::WHITE);
    } else {
        frontend_backdrop(ctx, c, 1);
    }
    // Retail 'Open Prev Arrow Button' areas: [357,397,447,487] in game, [351,500,453,602] in the front end.
    let (arrow_c, arrow_r) = if ingame { ((402.0, 442.0), 45.0) } else { ((402.0, 551.0), 51.0) };
    let target = match (controls, ingame) {
        (true, _) => back(Page::Options, Screen::Options),
        (false, _) => back(Page::Menu, Screen::Main),
    };
    arrow(ctx, c, [arrow_c.0 - arrow_r, arrow_c.1 - arrow_r, arrow_c.0 + arrow_r, arrow_c.1 + arrow_r], "FrontEnd_Prev_Button", target);
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
    let gold = Color::srgb(1.0, 0.85, 0.2);
    if ingame {
        ctx.image(c, "menu/fe_ingame_window.png", Some(Rect::new(10.0, 38.0, 682.0, 484.0)), 140.0, 132.0, 520.0, 343.0, Color::WHITE);
    } else {
        frontend_backdrop(ctx, c, 1);
    }
    let title = if display { "IDGS_TPFRONTENDTEXT_SCREENS_DISPLAY_OPTIONS" } else { "IDGS_TPFRONTENDTEXT_SCREENS_SOUND_OPTIONS" };
    ctx.text(c, "BlackChancery22", &s(title), 250.0, 300.0, if ingame { 190.0 } else { 160.0 }, true, gold);
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
    let back = Action::Goto(if ingame { Screen::Battle } else { Screen::Options }).with_page(ingame.then_some(Page::Options));
    let ([cx, cy], r) = if ingame { ([402.0, 442.0], 45.0) } else { ([402.0, 551.0], 51.0) };
    arrow(ctx, c, [cx - r, cy - r, cx + r, cy + r], "FrontEnd_Prev_Button", back);
}

fn disabled(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], label: &str, font: &str) {
    let (w, h) = (r - l, b - t);
    let cap = 50.0 * h / 44.0;
    c.spawn(ctx.node(l, t, w, h)).with_children(|c| {
        ctx.hud_piece(c, "FrontEnd_Texts_Over_Button_Disabled_Left", 0.0, 0.0, cap, h);
        ctx.hud_piece(c, "FrontEnd_Texts_Over_Button_Disabled_Center", cap, 0.0, (w - 2.0 * cap).max(0.0), h);
        ctx.hud_piece(c, "FrontEnd_Texts_Over_Button_Disabled_Right", w - cap, 0.0, cap, h);
        ctx.text(c, font, label, 0.0, w, h / 2.0, true, Color::srgb(0.55, 0.55, 0.55));
    });
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
    if let Some((sheet, r)) = sprite(ctx.hud, "FrontEnd_Letterbox_Tile", "NoCursor") {
        let rect = inset(&sheet, r);
        let (w, h) = (r.width(), r.height());
        let mut x = 0.0;
        while x < 800.0 {
            for (y, flip) in [(113.0 - h, false), (507.0, true)] {
                let mut img = ImageNode::new(ctx.assets.load(format!("hud/tex/{sheet}.png")));
                img.rect = Some(rect);
                img.image_mode = NodeImageMode::Stretch;
                img.flip_y = flip;
                c.spawn((img, ctx.node(x, y, w, h)));
            }
            x += w;
        }
    }
    ctx.image(c, &format!("menu/frontendscreen{scene}.png"), Some(Rect::new(0.0, 0.0, 800.0, 394.0)), 0.0, 113.0, 800.0, 394.0, Color::WHITE);
    ctx.image(c, "menu/frontendmap.png", None, 0.0, 113.0 + (394.0 - 375.0) / 2.0, 800.0, 375.0, Color::WHITE);
    if let Some((sheet, r)) = sprite(ctx.hud, "FrontEnd_TP_Logo", "NoCursor") {
        ctx.image(c, &format!("hud/tex/{sheet}.png"), Some(inset(&sheet, r)), 400.0 - r.width() / 2.0, 0.0, r.width(), r.height(), Color::WHITE);
    }
}

fn arrow(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], name: &str, action: Action) {
    let mut parts = vec![];
    let forward = name.contains("Next");
    let mut root = c.spawn((Interaction::None, action, crate::menu::ArrowButton { forward }, ctx.node(l, t, r - l, b - t)));
    root.with_children(|c| parts.extend(ctx.hud_piece(c, name, 0.0, 0.0, r - l, b - t)));
    root.insert(Art(parts));
}

/// Retail gold for highlighted labels (the `Center Justify Label` title colour).
const GOLD: Color = Color::srgb(1.0, 0.796, 0.0);

/// Map list as measured on the original: a framed panel (x 280..712, y 120..318) with a lighter header bar holding 'Historical Maps' /
/// 'Open Maps' and 'Players' in UniversBold14, Univers12 rows 19 apart from y 172 with the playable player count at x 578, a teal bar
/// behind the selected map, and the retail scrollbar at x 727..752. PROVISIONAL: header bar and selection colours are matched by eye.
fn map_list(ctx: &Ctx, c: &mut ChildSpawnerCommands, sk: &Skirmish, strings: &Value) {
    let (l, r) = (280.0, 712.0);
    stretched_window(ctx, c, [l, 120.0, r, 318.0]);
    c.spawn((ctx.node(l + 4.0, 122.0, r - l - 8.0, 30.0), BackgroundColor(Color::srgba(0.25, 0.75, 0.85, 0.22))));
    let heading = if sk.historical { "IDGS_TPFRONTENDTEXT01_VIEW_PRESET_MAPS" } else { "IDGS_TPFRONTENDTEXT01_VIEW_OPEN_MAPS" };
    ctx.text(c, "UniversBold14", &text_of(strings, heading), 298.0, 240.0, 136.0, false, Color::WHITE);
    ctx.text(c, "UniversBold14", &text_of(strings, "IDGS_TPFRONTENDTEXT01_PLAYERS"), 540.0, 80.0, 136.0, true, Color::WHITE);
    let rows: Vec<_> = sk.maps.iter().enumerate().filter(|(_, m)| m.2 != sk.historical).collect();
    for (row, (i, m)) in rows.iter().enumerate() {
        let cy = 172.0 + row as f32 * 19.0;
        let mut b = c.spawn((Interaction::None, Action::PickMap(*i), ctx.node(l + 4.0, cy - 9.5, r - l - 8.0, 19.0)));
        b.insert(BackgroundColor(if *i == sk.selected { Color::srgba(0.2, 0.7, 0.8, 0.45) } else { Color::NONE }));
        b.with_children(|c| {
            ctx.text(c, "Univers12", &m.1, 14.0, 240.0, 9.5, false, Color::WHITE);
            ctx.text(c, "Univers12", &m.3.to_string(), 278.0, 40.0, 9.5, true, Color::WHITE);
        });
    }
    scrollbar(ctx, c, [727.0, 155.0, 752.0, 322.0], 0.0, 1.0);
}

/// One retail sprite stretched over a box (reference units).
fn piece(ctx: &Ctx, c: &mut ChildSpawnerCommands, name: &str, [l, t, r, b]: [f32; 4], flip: (bool, bool)) {
    let Some((sheet, rect)) = sprite(ctx.hud, name, "NoCursor") else { return };
    let mut img = ImageNode::new(ctx.assets.load(format!("hud/tex/{sheet}.png")));
    img.rect = Some(inset(&sheet, rect));
    img.image_mode = NodeImageMode::Stretch;
    (img.flip_x, img.flip_y) = flip;
    c.spawn((img, ctx.node(l, t, r - l, b - t)));
}

/// The retail framed panel: `FRONTEND_STRETCHEDWINDOW_*` gives one corner (mirrored to the other three), an edge column/row and a fill.
fn stretched_window(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4]) {
    let k = 33.0; // corner tile size
    piece(ctx, c, "FRONTEND_STRETCHEDWINDOW_CENTERFILL_TILE", [l + k, t + k, r - k, b - k], (false, false));
    for (flip_x, x0, x1) in [(false, l, l + k), (true, r - k, r)] {
        for (flip_y, y0, y1) in [(false, t, t + k), (true, b - k, b)] {
            piece(ctx, c, "FRONTEND_STRETCHEDWINDOW_TOPLEFT_CORNERTILE", [x0, y0, x1, y1], (flip_x, flip_y));
        }
        piece(ctx, c, "FRONTEND_STRETCHEDWINDOW_LEFTCENTER_TILE", [x0, t + k, x1, b - k], (flip_x, false));
    }
    for (flip_y, y0, y1) in [(false, t, t + k), (true, b - k, b)] {
        piece(ctx, c, "FRONTEND_STRETCHEDWINDOW_TOPCENTER_TILE", [l + k, y0, r - k, y1], (false, flip_y));
    }
}

/// The retail vertical scrollbar: track (`..._SLIDER_BACKGROUND_TOP/STRETCHED_CENTER/BOTTOM`) and a slider over the visible part.
fn scrollbar(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], start: f32, len: f32) {
    let cap = |name: &str| sprite(ctx.hud, name, "NoCursor").map_or(20.0, |(_, r)| r.height());
    let track = |c: &mut ChildSpawnerCommands, prefix: &str, t: f32, b: f32| {
        let (top, bottom) = (cap(&format!("{prefix}_TOP")), cap(&format!("{prefix}_BOTTOM")));
        piece(ctx, c, &format!("{prefix}_TOP"), [l, t, r, t + top], (false, false));
        piece(ctx, c, &format!("{prefix}_STRETCHED_CENTER"), [l, t + top, r, b - bottom], (false, false));
        piece(ctx, c, &format!("{prefix}_BOTTOM"), [l, b - bottom, r, b], (false, false));
    };
    track(c, "FRONTEND_SCROLLBAR_SLIDER_BACKGROUND", t, b);
    let h = b - t;
    track(c, "FRONTEND_SCROLLBAR_SLIDER", t + h * start, t + h * (start + len).min(1.0));
}

/// A retail radio choice: check box (`FrontEnd_Check_Box_Ready` ticked / `_NotReady`) and its Univers12 label.
fn radio(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], label: &str, on: bool, action: Action) {
    let cy = (t + b) / 2.0 - 3.0; // measured: the original row sits 3 units above the element's centre
    let mut root = c.spawn((Interaction::None, action, ctx.node(l, t, r - l, b - t)));
    root.with_children(|c| {
        piece(ctx, c, if on { "FrontEnd_Check_Box_Ready" } else { "FrontEnd_Check_Box_NotReady" }, [0.0, (b - t) / 2.0 - 10.0, 18.0, (b - t) / 2.0 + 10.0], (false, false));
        ctx.text(c, "Univers12", label, 24.0, r - l - 24.0, cy - t, false, Color::WHITE);
    });
}

fn wrapped(ctx: &Ctx, c: &mut ChildSpawnerCommands, font: &str, s: &str, [l, t, r, b]: [f32; 4]) {
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

/// Resting / hover fill of a list cell (the retail SortList slot art carries decoration that only fits full-width rows).
const CELL: Color = Color::srgba(0.0, 0.05, 0.2, 0.7);
const CELL_ON: Color = Color::srgba(0.35, 0.28, 0.05, 0.75);
const CELL_HOVER: Color = Color::srgba(0.1, 0.25, 0.55, 0.85);

/// A list cell with left-aligned text, highlighted when `on`; clickable when it has an action.
fn cell(ctx: &Ctx, c: &mut ChildSpawnerCommands, [x, y, w, h]: [f32; 4], label: &str, on: bool, action: Option<Action>) {
    let tint = if on {
        Color::srgb(1.0, 0.9, 0.4)
    } else if action.is_some() {
        Color::WHITE
    } else {
        Color::srgb(0.75, 0.75, 0.75)
    };
    let mut b = c.spawn((ctx.node(x, y, w, h), BackgroundColor(if on { CELL_ON } else { CELL })));
    if let Some(a) = action {
        b.insert((Interaction::None, a, Rest(if on { CELL_ON } else { CELL })));
    }
    b.with_children(|c| ctx.text(c, "Univers12", label, 8.0, w - 8.0, h / 2.0, false, tint));
}
/// The fill a cell returns to when the cursor leaves it.
#[derive(Component)]
struct Rest(Color);

/// Lobby player table as measured on the original: a framed panel (x 222..782, y 122..448) with a 'Flag / Player / Fleet / Team' header
/// in UniversBold14 and one row per playable player, 32 apart from y 184: the small flag icon, then Univers10 text in retail pull-down
/// boxes (Player x 287..460, Fleet 470..561, Team 571..779 with its drop-down arrow). Clicking a player's box takes that slot, the
/// fleet box cycles the fleet type and the team box the team (open maps). PROVISIONAL: the human row shows "Player" (no profiles),
/// the flag icon is the race's icon block plus the slot number, and closing a slot on an open map is a click on its flag.
fn players_list(ctx: &Ctx, c: &mut ChildSpawnerCommands, _rect: [f32; 4], sk: &Skirmish, strings: &Value) {
    let s = |id: &str| strings[id].as_str().unwrap_or(id).to_string();
    let open = sk.open();
    stretched_window(ctx, c, [222.0, 122.0, 782.0, 448.0]);
    c.spawn((ctx.node(226.0, 124.0, 552.0, 30.0), BackgroundColor(Color::srgba(0.25, 0.75, 0.85, 0.22))));
    for (id, x0, x1) in [
        ("IDGS_TPFRONTENDTEXT03_FLAG", 236.0, 292.0),
        ("IDGS_TPFRONTENDTEXT_SCREENS_PLAYER", 287.0, 460.0),
        ("IDGS_TPFRONTENDTEXT03_FLEET", 470.0, 561.0),
        ("IDGS_TPFRONTENDTEXT02_TEAM", 571.0, 779.0),
    ] {
        ctx.text(c, "UniversBold14", &s(id), x0, x1 - x0, 138.0, true, Color::WHITE);
    }
    let teams = sk.map_json["teams"].as_array().cloned().unwrap_or_default();
    let team_locked = open && !teams.is_empty();
    for (i, slot) in sk.slots.iter().enumerate() {
        let cy = 184.0 + i as f32 * 32.0;
        let me = slot.kind == Kind::Human;
        let live = slot.kind != Kind::Closed;
        let base = match slot.race {
            2 => 10,
            3 => 20,
            _ => 0,
        };
        let flag = c.spawn((ctx.node(250.0, cy - 12.5, 28.0, 25.0), Interaction::None)).id();
        if open && !me {
            c.commands().entity(flag).insert(Action::ToggleClosed(i));
        }
        c.commands().entity(flag).with_children(|c| {
            if live {
                piece(ctx, c, &format!("FrontEnd_Flag_Small_Icon_{:02}", base + i % 10), [0.0, 0.0, 28.0, 25.0], (false, false));
            }
        });
        // Several live players on one team are told apart by a letter ('Pirates A', 'B', 'C'), as on original screenshots.
        let mates: Vec<usize> = sk.slots.iter().enumerate().filter(|(_, o)| o.kind != Kind::Closed && o.team == slot.team && slot.team >= 0).map(|(k, _)| k).collect();
        let letter = match mates.iter().position(|&k| k == i) {
            Some(n) if mates.len() > 1 => format!(" {}", (b'A' + n as u8) as char),
            _ => String::new(),
        };
        let team = match usize::try_from(slot.team).ok().and_then(|n| teams.get(n)) {
            Some(tm) => s(tm["name_id"].as_str().unwrap_or("")) + &letter,
            None if slot.team >= 0 => format!("{} {}", s("IDGS_TPFRONTENDTEXT02_TEAM"), slot.team + 1),
            None => "-".into(),
        };
        let race = RACES.iter().find(|r| r.0 == slot.race).map_or_else(|| s("IDGS_TPFRONTENDTEXT01_PLEASE_SELECT_A_RACE"), |r| s(r.2));
        let kind = match slot.kind {
            Kind::Human => s("IDGS_TPFRONTENDTEXT_SCREENS_PLAYER"),
            Kind::Computer => s("IDGS_TPFRONTENDTEXT01_COMPUTER"),
            Kind::Closed => s("IDGS_TPFRONTENDTEXT01_CLOSED"),
        };
        let cells = [
            (287.0, 460.0, kind, (live && !me).then_some(Action::Claim(i)), false),
            (470.0, 561.0, race, (open && live && !team_locked).then_some(Action::CycleRace(i)), false),
            (571.0, 779.0, team, (open && live).then_some(Action::CycleTeam(i)), true),
        ];
        for (x0, x1, label, action, drop) in cells {
            pulldown_cell(ctx, c, [x0, cy - 14.5, x1, cy + 14.5], &label, false, action, drop);
        }
    }
    if let Some(e) = sk.error {
        ctx.text(c, "Univers12", &s(e), 236.0, 540.0, 184.0 + 8.0 * 32.0, false, Color::srgb(1.0, 0.4, 0.3));
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

/// A retail pull-down box (`FrontEnd_Left/Center/Right_PullDown_Background`, `_Selected` for the local player) with centred Univers10
/// text and, for drop-downs, the `FrontEnd_PullDown_Arrow` at its right end.
fn pulldown_cell(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, b]: [f32; 4], label: &str, selected: bool, action: Option<Action>, arrow: bool) {
    let sel = if selected { "_Selected" } else { "" };
    let mut root = c.spawn((ctx.node(l, t, r - l, b - t), Interaction::None));
    if let Some(a) = action {
        root.insert(a);
    }
    root.with_children(|c| {
        let (w, h) = (r - l, b - t);
        piece(ctx, c, &format!("FrontEnd_Left_PullDown_Background{sel}"), [0.0, 0.0, 12.0, h], (false, false));
        piece(ctx, c, &format!("FrontEnd_Center_PullDown_Background{sel}"), [12.0, 0.0, w - 12.0, h], (false, false));
        piece(ctx, c, &format!("FrontEnd_Right_PullDown_Background{sel}"), [w - 12.0, 0.0, w, h], (false, false));
        let text_w = if arrow { w - 24.0 } else { w };
        ctx.text(c, "Univers10", label, 0.0, text_w, h / 2.0, true, Color::WHITE);
        if arrow {
            piece(ctx, c, "FrontEnd_PullDown_Arrow", [w - 20.0, h / 2.0 - 8.5, w, h / 2.0 + 8.5], (false, false));
        }
    });
}

const SHIP_ROWS: usize = 9;

/// Ship Types Available: the offered hulls of the human slot's fleet type (mouse wheel scrolls), the selected one highlighted.
fn ship_list(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, _]: [f32; 4], sk: &Skirmish, lib: &Library) {
    let list = sk.human().map(|h| offered(lib, h.race)).unwrap_or_default();
    for (row, &ci) in list.iter().enumerate().skip(sk.scroll).take(SHIP_ROWS) {
        cell(ctx, c, [l, t + (row - sk.scroll) as f32 * 19.0, r - l, 18.0], &pretty(&lib.classes[ci].name), row == sk.ship_pick, Some(Action::PickShip(row)));
    }
}

/// Fleet Select: the assembled fleet, two columns of four; the picked entry is the one Drop removes.
fn fleet_list(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, _]: [f32; 4], sk: &Skirmish) {
    let w = (r - l) / 2.0 - 4.0;
    for (i, name) in sk.human().map(|h| h.fleet.as_slice()).unwrap_or_default().iter().enumerate() {
        cell(ctx, c, [l + (i / 4) as f32 * (w + 8.0), t + (i % 4) as f32 * 19.0, w, 18.0], &pretty(name), i == sk.fleet_pick, Some(Action::PickFleet(i)));
    }
}

/// Ship Information for the picked hull. PROVISIONAL: retail shows more (guns by class, crew, victory points).
fn ship_info(ctx: &Ctx, c: &mut ChildSpawnerCommands, [l, t, r, _]: [f32; 4], sk: &Skirmish, lib: &Library, strings: &Value) {
    let Some(ci) = sk.human().map(|h| offered(lib, h.race)).and_then(|l| l.get(sk.ship_pick).copied()) else { return };
    let cl = &lib.classes[ci];
    let lines = [
        pretty(&cl.name),
        format!("{}: {:.0}", strings["IDGS_TPFRONTENDTEXT_SHIP_HITPOINTS"].as_str().unwrap_or("Hit Points"), cl.core_hp),
        format!("Guns: {}", cl.mounts.len()),
        format!("Max speed: {:.1}", cl.phys.max_speed),
    ];
    for (i, line) in lines.iter().enumerate() {
        ctx.text(c, "Univers12", line, l + 8.0, r - l, t + 14.0 + i as f32 * 18.0, false, Color::WHITE);
    }
}

/// Mouse wheel scrolls the Ship Types Available list.
fn wheel(mut wheel: MessageReader<bevy::input::mouse::MouseWheel>, state: Res<State<Screen>>, mut sk: ResMut<Skirmish>, lib: Res<Library>) {
    let dy: f32 = wheel.read().map(|e| e.y).sum();
    if *state.get() != Screen::Fleet || dy == 0.0 {
        return;
    }
    let n = sk.human().map_or(0, |h| offered(&lib, h.race).len());
    let max = n.saturating_sub(SHIP_ROWS);
    sk.scroll = if dy > 0.0 { sk.scroll.saturating_sub(1) } else { (sk.scroll + 1).min(max) };
}

fn scroll_controls(mut wheel: MessageReader<bevy::input::mouse::MouseWheel>, state: Res<State<Screen>>, ig: Res<InGame>, mut cui: ResMut<CtrlUi>, binds: Res<Bindings>) {
    let dy: f32 = wheel.read().map(|e| e.y).sum();
    if dy != 0.0 && (*state.get() == Screen::Controls || ig.page() == Some(Page::Controls)) {
        let max = binds.rows.len().saturating_sub(if ig.page().is_some() { 8 } else { 17 });
        cui.scroll = if dy > 0.0 { cui.scroll.saturating_sub(1) } else { (cui.scroll + 1).min(max) };
    }
}

/// Mouse wheel scrolls the save lists.
fn scroll_saves(mut wheel: MessageReader<bevy::input::mouse::MouseWheel>, state: Res<State<Screen>>, ig: Res<InGame>, mut ui: ResMut<SaveUi>) {
    let dy: f32 = wheel.read().map(|e| e.y).sum();
    if dy != 0.0 && (*state.get() == Screen::Load || matches!(ig.page(), Some(Page::Load | Page::Save))) {
        let max = ui.list.len().saturating_sub(10);
        ui.scroll = if dy > 0.0 { ui.scroll.saturating_sub(1) } else { (ui.scroll + 1).min(max) };
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
