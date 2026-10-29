//! Campaign save and load lists (front end and in-game menu) and F3/F4 quick save/load.
use bevy::prelude::*;

use super::*;

/// State of the Load / Save lists (front-end Load Campaign screen and the in-game pages).
#[derive(Resource, Default)]
pub struct SaveUi {
    pub list: Vec<(CampaignSave, bool)>,
    pub selected: Option<usize>,
    /// First row of the classic fixed-height list.
    pub scroll: usize,
    pub name: String,
    pub status: Option<&'static str>, // string id (or plain text) shown above the list
}
impl SaveUi {
    pub(super) fn open(&mut self) {
        *self = Self { list: crate::campaign::list_saves(), ..default() };
    }
}

pub(super) const QUICK: &str = "QuickSave";

/// Load / Save / Remove / Quit buttons of the Load Campaign screen and the in-game menu.
/// PROVISIONAL: a save is a mission-start checkpoint plus the campaign counters; loading restarts that mission. No delete or overwrite
/// confirmation dialogs (the retail screens have them).
pub(super) fn save_actions(
    q: Query<(&Interaction, &Action), Changed<Interaction>>,
    mut commands: Commands,
    (mut ui, mut ig, mut points, mut sk): (ResMut<SaveUi>, ResMut<InGame>, ResMut<CampaignPoints>, ResMut<Skirmish>),
    mut cfg: ResMut<crate::Config>,
    mission: Option<Res<crate::campaign::Mission>>,
    old: Query<Entity, Or<(With<crate::Scenario>, With<crate::Transient>)>>,
    roots: Query<Entity, With<MenuRoot>>,
    mut vtime: ResMut<Time<Virtual>>,
    mut next: ResMut<NextState<Screen>>,
    (keys, state): (Res<ButtonInput<KeyCode>>, Res<State<Screen>>),
) {
    let mut acts: Vec<Action> = q.iter().filter(|(i, _)| **i == Interaction::Pressed).map(|(_, a)| a.clone()).collect();
    // Dev hook: `--script ingame-do-load` loads the mission 5 start save from the Load page at frame 120.
    if cfg.script.as_deref() == Some("ingame-do-load") && cfg.frame == 120 {
        ui.selected = ui.list.iter().position(|s| s.0.mission == "mission05");
        acts.push(Action::DoLoad);
    }
    // Dev hook: `--script quick` quick-saves at frame 60 and quick-loads at frame 120.
    acts.extend(match (cfg.script.as_deref(), cfg.frame) {
        (Some("quick"), 60) => Some(Action::QuickSave),
        (Some("quick"), 120) => Some(Action::QuickLoad),
        _ => None,
    });
    // Retail default bindings (UserProfiles/default.pft, DirectInput scancodes 0x3d / 0x3e): F3 Quick Save, F4 Quick Load.
    if *state.get() == Screen::Battle && ig.page.is_none() {
        acts.extend([(KeyCode::F3, Action::QuickSave), (KeyCode::F4, Action::QuickLoad)].into_iter().filter(|(k, _)| keys.just_pressed(*k)).map(|k| k.1));
    }
    acts.reverse();
    while let Some(a) = acts.pop() {
        match a {
            Action::QuickSave => {
                if let Some(m) = &mission {
                    match crate::campaign::write_save(&points.snapshot(QUICK, &m.id)) {
                        Ok(()) => info!("quick saved {}", m.id),
                        Err(e) => warn!("quick save failed: {e}"),
                    }
                }
            }
            Action::QuickLoad => {
                ui.open();
                if let Some(n) = ui.list.iter().position(|s| !s.1 && s.0.name == QUICK) {
                    info!("quick loading {}", ui.list[n].0.mission);
                    ui.selected = Some(n);
                    acts.push(Action::DoLoad);
                }
            }
            Action::OpenLoad => {
                ui.open();
                next.set(Screen::Load);
            }
            Action::Ingame(None) => close_overlay(&mut commands, &roots, &mut ig, &mut vtime),
            Action::Ingame(Some(p)) => {
                if p != Page::Menu {
                    ui.open();
                }
                ig.page = Some(p);
            }
            Action::PickSave(n) => {
                ui.selected = Some(n);
                ui.status = None;
                if let (Some(Page::Save), Some((s, false))) = (ig.page, ui.list.get(n)) {
                    ui.name = s.name.clone();
                }
            }
            Action::DoLoad => {
                let Some(save) = ui.selected.and_then(|n| ui.list.get(n)).map(|s| s.0.clone()) else { continue };
                points.restore(&save);
                (sk.map, cfg.map, cfg.ended) = (None, Some(save.mission), None);
                if *state.get() == Screen::Battle {
                    for e in &old {
                        commands.entity(e).despawn();
                    }
                    commands.remove_resource::<crate::campaign::Mission>();
                    close_overlay(&mut commands, &roots, &mut ig, &mut vtime);
                }
                next.set(Screen::Battle); // from the battle too: re-entering the state spawns the mission again
            }
            Action::DoSave => {
                let Some(m) = &mission else {
                    ui.status = Some("Only campaign missions can be saved.");
                    continue;
                };
                let name = if ui.name.trim().is_empty() { format!("Mission {}", m.id.trim_start_matches("mission")) } else { ui.name.trim().to_string() };
                match crate::campaign::write_save(&points.snapshot(&name, &m.id)) {
                    Ok(()) => {
                        ui.list = crate::campaign::list_saves();
                        ui.selected = ui.list.iter().position(|s| !s.1 && s.0.name == name);
                        ui.status = Some("IDGS_TPFRONTENDTEXT02_FILECHANGE_SAVED");
                    }
                    Err(e) => {
                        warn!("could not save: {e}");
                        ui.status = Some("Could not write the save file.");
                    }
                }
            }
            Action::Remove => {
                if let Some((s, false)) = ui.selected.and_then(|n| ui.list.get(n)) {
                    crate::campaign::delete_save(&s.name);
                    let status = ui.status;
                    ui.open();
                    ui.status = status;
                }
            }
            Action::QuitToMenu => {
                for e in &old {
                    commands.entity(e).despawn();
                }
                commands.remove_resource::<crate::campaign::Mission>();
                close_overlay(&mut commands, &roots, &mut ig, &mut vtime);
                next.set(Screen::Main);
            }
            _ => {}
        }
    }
}

/// Typing into the Save page's file name entry.
pub(super) fn save_text(mut keys: MessageReader<bevy::input::keyboard::KeyboardInput>, ig: Res<InGame>, mut ui: ResMut<SaveUi>) {
    use bevy::input::keyboard::Key;
    for k in keys.read().filter(|k| k.state.is_pressed() && ig.page == Some(Page::Save)) {
        match &k.logical_key {
            Key::Character(c) if ui.name.chars().count() < 32 => ui.name.extend(c.chars().filter(|c| !c.is_control())),
            Key::Space if ui.name.chars().count() < 32 => ui.name.push(' '),
            Key::Backspace => {
                ui.name.pop();
            }
            _ => {}
        }
    }
}

/// Keys of the Load / Save profile dialog: typing the file name, Backspace, Enter (Load / Save), Escape (Cancel) and the wheel over the list.
pub(super) fn profile_keys(
    mut keys: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut sk: ResMut<Skirmish>,
    lib: Res<Library>,
) {
    use bevy::input::keyboard::Key;
    if sk.profile.is_none() {
        keys.clear();
        wheel.clear();
        return;
    }
    let dy: f32 = wheel.read().map(|w| w.y).sum();
    for k in keys.read().filter(|k| k.state.is_pressed()) {
        let Some(d) = sk.profile.as_mut() else { break };
        match &k.logical_key {
            Key::Character(c) if d.save && d.name.chars().count() < 32 => d.name.extend(c.chars().filter(|c| !c.is_control())),
            Key::Space if d.save && d.name.chars().count() < 32 => d.name.push(' '),
            Key::Backspace => {
                d.name.pop();
            }
            Key::Escape => sk.profile = None,
            Key::Enter => sk.do_profile(&lib),
            _ => {}
        }
    }
    if let Some(d) = sk.profile.as_mut().filter(|_| dy != 0.0) {
        d.scroll = (d.scroll as i64 - dy.signum() as i64).clamp(0, d.files.len().saturating_sub(PROFILE_ROWS) as i64) as usize;
    }
}

/// Rows of the dialog's file list (the retail list area is 90 units high).
pub const PROFILE_ROWS: usize = 4;
