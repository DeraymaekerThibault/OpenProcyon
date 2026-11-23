//! Shared campaign/skirmish outfitting: recruitment, posts and mount-compatible bank weapon changes.
//! Retail prices/capacities are data-driven; catalogue fallback, unique hires and upgrade policy are provisional.
use bevy::prelude::*;
use serde_json::Value;
use std::collections::BTreeMap;

use crate::{campaign::CampaignPoints, combat::Library};

mod fitting;
mod posts;
mod view;
use fitting::{Carry, apply_outfit, apply_skirmish_crew, apply_skirmish_weapons, bank_delta, banks_for, fit_guns, fit_message, ship_key, signed, upgrade};
pub use fitting::{Outfit, ShipFit, SkirmishCrew, WeaponFit, bank_compatible, bank_cost, fitting_delta};
pub use posts::{
    CrewPosts, ability, accuracy, auto_post, best, crew_points, fallback_posts, generic_posts, leadership_bonus, portrait, post_skill_name, skill_factor, sort_for_post, species,
};
use posts::{crew_data, name, place_crew, recruit, spend, unpost};
pub use view::{Action, CrewSet, Detail, FitView, RowInfo, ScreenModel, crew_detail, roster, selectable_banks, weapon_banks, weapon_detail};
use view::{campaign_screen_model, skirmish_screen_model};
pub const ROLES: [&str; 8] = ["Gunners", "Captains", "First mates", "Navigators", "Engineers", "Riggers", "Fighters", "Lookouts"];
const SKILLS: [&str; 8] = ["gunnery", "leadership", "leadership", "navigation", "engineering", "rigging", "combat", "spotting"];
const PAGE: usize = 6;

/// The skill of a rating-1 hand (the default crewman, and what an empty slot holds), on the 0..1 scale.
pub const DEFAULT_HAND: f32 = 0.1;

/// Crew list sort fields, as the original's Sort By list offers them.
pub const SORTS: [&str; 9] = ["Name", "Victory Points", "Leadership", "Gunnery", "Navigation", "Rigging", "Engineering", "Combat", "Spotting"];

#[derive(Default)]
struct Choice {
    ship: usize,
    post: usize,
    page: usize,
    arms: bool,
    /// A post was chosen by clicking it (not just the default): roster clicks hire onto it.
    post_picked: bool,
    paused_here: bool,
    message: String,
}
fn cycle(value: usize, delta: i32, len: usize) -> usize {
    if len == 0 { 0 } else { (value as i64 + i64::from(delta)).rem_euclid(len as i64) as usize }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<ScreenModel>()
        .add_systems(Update, button_sounds.run_if(|m: Res<ScreenModel>| m.open))
        .add_systems(Update, (screen, skirmish_screen).chain().in_set(CrewSet).after(crate::menu::RunButtons).before(crate::input))
        .add_systems(FixedUpdate, (apply_outfit, apply_skirmish_weapons, apply_skirmish_crew).chain().in_set(crate::BattleSet::Commands));
}

/// The retail front-end sound for pointing at (`select == false`) or pressing a widget of these screens (names as `menu::button_sound`).
pub fn button_sound(action: &Action, select: bool) -> &'static str {
    let list = matches!(
        action,
        Action::SelectShip(_)
            | Action::SelectPost(_)
            | Action::PostFace(..)
            | Action::PickCrew(_)
            | Action::PickWeapon(_)
            | Action::Unpost(..)
            | Action::SortBy(_)
            | Action::Assign(_)
            | Action::Recruit(_)
            | Action::Reserve(_)
            | Action::Upgrade(_)
    );
    match (select, action) {
        (false, _) => {
            if list {
                "FE_List_Rollover"
            } else {
                "FE_Button_Rollover"
            }
        }
        (true, Action::Close) => "FE_Button_Forward_Select",
        (true, Action::Assign(_) | Action::Recruit(_) | Action::Upgrade(_)) => "FE_Add",
        (true, Action::Reserve(_) | Action::Drop) => "FE_Drop",
        (true, _) if list => "FE_List_Select",
        _ => "FE_Button_Select",
    }
}

/// Rollover when the pointer first reaches a widget, the select sound when it is pressed (as `menu::button_sounds`).
#[allow(deprecated)] // Interaction, see pointer.rs
fn button_sounds(
    q: Query<(Entity, &Interaction, &Action), Changed<Interaction>>,
    mut pointed: Local<std::collections::HashSet<Entity>>,
    mut sfx: MessageWriter<crate::audio::Sfx>,
) {
    for (e, i, action) in &q {
        if !crate::pointer::over(i) {
            pointed.remove(&e);
            continue;
        }
        if pointed.insert(e) {
            sfx.write(crate::audio::Sfx::ui(button_sound(action, false)));
        }
        if crate::pointer::down(i) {
            sfx.write(crate::audio::Sfx::ui(button_sound(action, true)));
        }
    }
}

#[allow(deprecated)] // Interaction, see pointer.rs
fn screen(
    mission: Option<ResMut<crate::campaign::Mission>>,
    state: Res<State<crate::menu::Screen>>,
    mut points: ResMut<CampaignPoints>,
    lib: Res<Library>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut vtime: ResMut<Time<Virtual>>,
    mut model: ResMut<ScreenModel>,
    ships: Query<(Entity, &crate::Ship, &crate::combat::Health, &crate::campaign::MissionShip)>,
    buttons: Query<(&Action, &Interaction), Changed<Interaction>>,
    mut choice: Local<Choice>,
    loading: Option<Res<crate::loading::Loading>>,
) {
    // while the battle loads the pause is the loading screen's: this screen opens after it
    if *state.get() != crate::menu::Screen::Battle || loading.is_some() {
        return;
    }
    let want = mission.as_deref().is_some_and(|m| m.script.crew_screen && !crate::rules::nis(Some(m)));
    if !want {
        if model.open {
            *model = ScreenModel::default();
            if choice.paused_here && *state.get() == crate::menu::Screen::Battle {
                vtime.unpause();
            }
        }
        *choice = Choice::default();
        return;
    }
    let mut mission = mission.unwrap();
    if !model.open {
        choice.paused_here = !vtime.is_paused();
        vtime.pause();
    }
    let mut fleet: Vec<_> = ships.iter().filter(|(_, _, h, m)| !h.destroyed && m.owner == mission.script.player()).collect();
    fleet.sort_by_key(|(_, _, _, m)| m.id);
    choice.ship = choice.ship.min(fleet.len().saturating_sub(1));
    let action = if keys.just_pressed(KeyCode::Enter) {
        Some(Action::Close)
    } else if mouse.just_pressed(MouseButton::Left) {
        buttons.iter().find(|(_, i)| crate::pointer::down(i)).map(|(a, _)| a.clone())
    } else {
        None
    };
    if action == Some(Action::Close) {
        keys.clear_just_pressed(KeyCode::Enter);
        mouse.clear_just_pressed(MouseButton::Left);
        mission.script.crew_screen = false;
        if choice.paused_here {
            vtime.unpause();
        }
        *choice = Choice::default();
        *model = ScreenModel::default();
        return;
    }
    if let Some(action) = action {
        let old_outfit = points.outfit.clone();
        let old_total = points.total;
        let result = campaign_action(action, &mut points, &lib, &fleet, &mut choice);
        let changed = points.outfit != old_outfit || points.total != old_total;
        let result = result.and_then(|()| if changed { points.persist() } else { Ok(()) });
        if let Err(error) = result {
            points.outfit = old_outfit;
            points.total = old_total;
            choice.message = error;
        } else {
            choice.message = if changed { "Saved.".into() } else { String::new() };
        }
    }
    let next = campaign_screen_model(&points, &lib, &fleet, &mut choice);
    if *model != next {
        *model = next;
    }
}

/// Applies one campaign Crew & Arms button: the screen's own state moves, or a purchase / post change on `points`.
fn campaign_action(
    action: Action,
    points: &mut CampaignPoints,
    lib: &Library,
    fleet: &[(Entity, &crate::Ship, &crate::combat::Health, &crate::campaign::MissionShip)],
    choice: &mut Choice,
) -> Result<(), String> {
    match action {
        Action::Tab => {
            choice.arms = !choice.arms;
            choice.post = 0;
            choice.page = 0;
            Ok(())
        }
        Action::Ship(d) => {
            choice.ship = cycle(choice.ship, d, fleet.len());
            choice.post = 0;
            choice.page = 0;
            Ok(())
        }
        Action::Post(d) => {
            let len = fleet.get(choice.ship).map_or(0, |(_, s, _, _)| if choice.arms { weapon_banks(lib, &lib.classes[s.class]).len() } else { ROLES.len() });
            choice.post = cycle(choice.post, d, len);
            choice.page = 0;
            Ok(())
        }
        Action::Page(d) => {
            choice.page = (choice.page as i64 + i64::from(d)).max(0) as usize;
            Ok(())
        }
        Action::SelectShip(i) => {
            choice.ship = i.min(fleet.len().saturating_sub(1));
            (choice.post, choice.page) = (0, 0);
            Ok(())
        }
        Action::SelectPost(i) | Action::PostFace(i, _) => {
            (choice.post, choice.page) = (i, 0);
            Ok(())
        }
        action => {
            if let Some((_, ship, _, ms)) = fleet.get(choice.ship) {
                let class = &lib.classes[ship.class];
                let key = ship_key(class, ms);
                match action {
                    Action::Recruit(id) => recruit(points, &id, &class.race),
                    Action::Assign(id) => points.outfit.assign(&key, choice.post, crate::boarding::counts(&class.name)[choice.post], &id),
                    Action::Reserve(id) => {
                        if let Some(fit) = points.outfit.ships.get_mut(&key) {
                            fit.crew[choice.post].retain(|c| c != &id);
                        }
                        Ok(())
                    }
                    Action::Upgrade(gun) => {
                        weapon_banks(lib, class).get(choice.post).ok_or_else(|| "No weapon bank.".into()).and_then(|(bank, _)| upgrade(points, lib, class, &key, *bank, &gun))
                    }
                    _ => Ok(()),
                }
            } else {
                Err("No surviving own ship.".into())
            }
        }
    }
}

#[allow(deprecated)] // Interaction, see pointer.rs
fn skirmish_screen(
    state: Res<State<crate::menu::Screen>>,
    mut sk: ResMut<crate::menu::Skirmish>,
    lib: Res<Library>,
    mut model: ResMut<ScreenModel>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    buttons: Query<(&Action, &Interaction), Changed<Interaction>>,
    all: Query<(&Action, &Interaction)>,
    mut choice: Local<Choice>,
    mut carry: Local<Option<Carry>>,
    mut sort: Local<usize>,
    mut sort_open: Local<bool>,
    mut origin: Local<Option<(usize, usize)>>,
    mut scroll_to: ResMut<crate::menu::ScrollTo>,
    (time, mut clicks): (Res<Time<Real>>, Local<crate::menu::ClickClock>),
) {
    use crate::menu::Screen;
    if *state.get() == Screen::Battle {
        return;
    }
    if *state.get() != Screen::Fleet || !sk.arms_open {
        if model.open {
            *model = ScreenModel::default();
        }
        sk.arms_open = false;
        *choice = Choice::default();
        *carry = None;
        *sort_open = false;
        return;
    }
    if !model.open {
        choice.ship = sk.fleet_pick;
        choice.arms = sk.arms_tab;
    }
    // A face is a drop target like its post while something is carried.
    let as_target = |a: &Action, carrying: bool| match a {
        Action::PostFace(r, _) if carrying => Action::SelectPost(*r),
        a => a.clone(),
    };
    let carrying = carry.is_some();
    let mut pressed = if mouse.just_pressed(MouseButton::Left) { buttons.iter().find(|(_, i)| crate::pointer::down(i)).map(|(a, _)| as_target(a, carrying)) } else { None };
    // Double-click a roster row with no post selected: seat the member on the first free post that counts their best skill.
    if let Some(Action::PickCrew(id)) = &pressed
        && !choice.post_picked
        && let Some(row) = model.fit.as_ref().and_then(|f| f.list.iter().position(|(_, a)| a == &Action::PickCrew(id.clone())))
        && clicks.double(1, row, time.elapsed_secs_f64())
    {
        pressed = Some(Action::HireAuto(id.clone()));
    }
    // Arms tab: shift-click a weapon fits every bank of its mount type, a double-click fits the selected bank.
    if let Some(Action::PickWeapon(gun)) = &pressed {
        let row = model.fit.as_ref().and_then(|f| f.list.iter().position(|(_, a)| a == &Action::PickWeapon(gun.clone())));
        if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
            pressed = Some(Action::FitAll(gun.clone()));
        } else if let Some(row) = row
            && clicks.double(2, row, time.elapsed_secs_f64())
        {
            pressed = Some(Action::FitWeapon(gun.clone()));
        }
    }
    // Clicking a roster row (press and release on it, no drag) with a post selected seats the member there.
    let clicked_row = if mouse.just_released(MouseButton::Left) && !choice.arms && choice.post_picked && origin.is_none() {
        match carry.as_ref() {
            Some(Carry::Crew(id)) if all.iter().any(|(a, i)| matches!(a, Action::PickCrew(x) if x == id) && crate::pointer::over(i)) => Some(Action::Hire(id.clone())),
            _ => None,
        }
    } else {
        None
    };
    // Right-click on a posted face: back to the roster.
    let unposted = if mouse.just_pressed(MouseButton::Right) && !choice.arms {
        all.iter().find_map(|(a, i)| match a {
            Action::PostFace(r, k) if crate::pointer::over(i) => Some(Action::Unpost(*r, *k)),
            _ => None,
        })
    } else {
        None
    };
    // Released over a post or mount while carrying: the drop of a drag.
    let released_on = if mouse.just_released(MouseButton::Left) && carry.is_some() {
        all.iter().find(|(a, i)| matches!(a, Action::SelectPost(_) | Action::PostFace(..)) && crate::pointer::over(i)).map(|(a, _)| as_target(a, true))
    } else {
        None
    };
    let action = if sk.profile.is_none() && (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Escape)) {
        Some(Action::Close)
    } else {
        pressed.or(released_on).or(clicked_row).or(unposted)
    };
    if action == Some(Action::Close) {
        keys.clear_just_pressed(KeyCode::Enter);
        keys.clear_just_pressed(KeyCode::Escape);
        mouse.clear_just_pressed(MouseButton::Left);
        sk.arms_open = false;
        *model = ScreenModel::default();
        *choice = Choice::default();
        *carry = None;
        return;
    }
    if let Some((_, row)) = scroll_to.0.take() {
        choice.page = row;
    }
    let scroll: f32 = wheel.read().map(|w| w.y).sum();
    if scroll != 0.0 {
        choice.page = (choice.page as i64 - scroll.signum() as i64).max(0) as usize;
    }
    if action == Some(Action::AutoAssign) {
        crate::menu::auto_assign(&mut sk, &lib);
    }
    let strings = crate::menu::strings();
    let said = |id: &str| crate::menu::text_of(strings, id);
    let points = sk.points_left(&lib);
    let Some(slot) = sk.slots.iter_mut().find(|s| s.kind == crate::menu::Kind::Human) else { return };
    choice.ship = choice.ship.min(slot.fleet.len().saturating_sub(1));
    let mut sk_slots = None;
    if let Some(action) = action {
        // Anything but picking a row or placing what is carried drops the carried item.
        let placing = matches!(action, Action::SelectPost(_));
        let picks = matches!(action, Action::PickCrew(_) | Action::PickWeapon(_) | Action::PostFace(..));
        let held = if picks { None } else { carry.take().filter(|_| placing) };
        let auto = matches!(action, Action::HireAuto(_));
        let fit_all = matches!(action, Action::FitAll(_));
        match action {
            Action::Ship(d) => {
                choice.ship = cycle(choice.ship, d, slot.fleet.len());
                (choice.post, choice.page, choice.post_picked) = (0, 0, false);
            }
            Action::Post(d) => {
                let len = slot.fleet.get(choice.ship).map_or(0, |n| weapon_banks(&lib, &lib.classes[lib.index(n)]).len());
                choice.post = cycle(choice.post, d, len);
                choice.page = 0;
            }
            Action::Page(d) => choice.page = (choice.page as i64 + i64::from(d)).max(0) as usize,
            Action::Tab => {
                choice.arms = !choice.arms;
                (choice.post, choice.page, choice.post_picked) = (0, 0, false);
                choice.message.clear();
            }
            Action::SelectShip(i) => {
                choice.ship = i.min(slot.fleet.len().saturating_sub(1));
                (choice.post, choice.page, choice.post_picked) = (0, 0, false);
                choice.message.clear();
            }
            Action::Slots(d) => sk_slots = Some(d),
            Action::Sort => *sort_open = !*sort_open,
            Action::SortBy(i) => (*sort, *sort_open) = (i.min(SORTS.len() - 1), false),
            Action::PickCrew(id) => {
                *carry = Some(Carry::Crew(id));
                *origin = None;
            }
            Action::PostFace(r, k) => {
                choice.post = r;
                if !choice.arms {
                    (choice.post_picked, *sort) = (true, sort_for_post(r));
                }
                if let Some(id) = slot.crew.get(choice.ship).and_then(|c| c[r.min(7)].get(k)).cloned() {
                    *carry = Some(Carry::Crew(id));
                    *origin = Some((r, k));
                }
            }
            Action::PickWeapon(gun) => *carry = Some(Carry::Gun(gun)),
            Action::Hire(id) | Action::HireAuto(id) if !choice.arms => {
                let Some(class_name) = slot.fleet.get(choice.ship).cloned() else { return };
                let post = if auto { auto_post(slot.crew_mut(choice.ship), &class_name, &id) } else { Some(choice.post) };
                choice.message = match post {
                    Some(r) => {
                        (choice.post, choice.post_picked, *sort) = (r, true, sort_for_post(r));
                        place_crew(slot.crew_mut(choice.ship), &class_name, id, r, None, points)
                    }
                    None => format!("No free post suits {}.", name(&id)),
                };
            }
            Action::FitWeapon(gun) | Action::FitAll(gun) if choice.arms => {
                let Some(class_name) = slot.fleet.get(choice.ship).cloned() else { return };
                let class = &lib.classes[lib.index(&class_name)];
                slot.weapons.resize_with(slot.fleet.len(), BTreeMap::new);
                let targets: Vec<_> = if fit_all { banks_for(&lib, class, &gun) } else { weapon_banks(&lib, class).get(choice.post).copied().into_iter().collect() };
                choice.message = fit_message(&said, fit_guns(&lib, class, &mut slot.weapons[choice.ship], &targets, &gun, points));
            }
            Action::Unpost(r, k) => {
                if let Some(gone) = unpost(slot.crew_mut(choice.ship), r, k) {
                    choice.message = format!("{} returned to the roster.", name(&gone));
                }
            }
            Action::Drop => {
                if let Some(last) = slot.crew.get_mut(choice.ship).and_then(|c| c.get_mut(choice.post)).and_then(Vec::pop) {
                    choice.message = format!("{} returned to the roster.", name(&last));
                }
            }
            Action::SelectPost(i) => {
                let Some(class_name) = slot.fleet.get(choice.ship).cloned() else { return };
                let class = &lib.classes[lib.index(&class_name)];
                match held {
                    // Placing a crew member on a post of the Crew tab.
                    Some(Carry::Crew(id)) if !choice.arms => {
                        (choice.post, choice.post_picked, *sort) = (i, true, sort_for_post(i));
                        // Moving a posted crew member lifts them off their post first (their points come back).
                        let moved = origin.take();
                        choice.message = place_crew(slot.crew_mut(choice.ship), &class.name, id, i, moved, points);
                    }
                    // Fitting a weapon on a mount of the Arms tab.
                    Some(Carry::Gun(gun)) if choice.arms => {
                        choice.post = i;
                        slot.weapons.resize_with(slot.fleet.len(), BTreeMap::new);
                        let targets: Vec<_> = weapon_banks(&lib, class).get(i).copied().into_iter().collect();
                        choice.message = fit_message(&said, fit_guns(&lib, class, &mut slot.weapons[choice.ship], &targets, &gun, points));
                    }
                    _ => {
                        (choice.post, choice.page) = (i, 0);
                        if !choice.arms {
                            (choice.post_picked, *sort) = (true, sort_for_post(i));
                        }
                        choice.message.clear();
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(d) = sk_slots {
        sk.scroll_slots(d);
    }
    if carry.is_none() {
        *origin = None;
    }
    let points = sk.points_left(&lib); // the budget after this frame's changes
    let Some(slot) = sk.slots.iter().find(|s| s.kind == crate::menu::Kind::Human) else { return };
    let next = skirmish_screen_model(&sk, slot, &lib, &mut choice, &carry, *sort, *sort_open, points);
    sk.fleet_pick = choice.ship;
    sk.arms_tab = choice.arms;
    sk.show_picked_slot();
    if *model != next {
        *model = next;
    }
}

#[cfg(test)]
mod tests;
