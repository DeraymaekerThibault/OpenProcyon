//! Shared campaign/skirmish outfitting: recruitment, posts and mount-compatible bank weapon changes.
//! Retail prices/capacities are data-driven; catalogue fallback, unique hires and upgrade policy are provisional.
#![allow(deprecated)] // Bevy 0.20 Interaction, as menu.rs.
use bevy::prelude::*;
use serde_json::Value;
use std::collections::BTreeMap;

use crate::{campaign::CampaignPoints, combat::Library};

pub const ROLES: [&str; 8] = ["Gunners", "Captains", "First mates", "Navigators", "Engineers", "Riggers", "Fighters", "Lookouts"];
const SKILLS: [&str; 8] = ["gunnery", "leadership", "leadership", "navigation", "engineering", "rigging", "combat", "spotting"];
const PAGE: usize = 6;

fn crew_data() -> &'static Value {
    static DATA: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    DATA.get_or_init(|| std::fs::read_to_string(crate::assets_dir().join("data/crew.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default())
}
fn ability(id: &str, skill: &str) -> Option<f32> {
    crew_data()[id]["skills"][skill].as_f64().map(|v| (v as f32 / 10.0).clamp(0.0, 1.0))
}
fn name(id: &str) -> &str {
    crew_data()[id]["name"].as_str().unwrap_or(id)
}

/// Legacy mission-start saves have availability lists but no post assignments.
pub fn best(points: &CampaignPoints, skill: &str) -> Option<f32> {
    points.crew.iter().filter_map(|c| ability(c, skill)).reduce(f32::max)
}
pub fn accuracy(points: Option<&CampaignPoints>) -> f32 {
    points.and_then(|p| best(p, "gunnery")).map_or(0.5, |g| 1.0 - 0.8 * g)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShipFit {
    pub crew: [Vec<String>; 8],
    pub banks: BTreeMap<usize, String>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outfit {
    pub hired: Vec<String>,
    pub ships: BTreeMap<String, ShipFit>,
}
impl Outfit {
    pub fn from_json(v: &Value) -> Self {
        let mut out = Self { hired: crate::campaign::string_list(&v["hired"]), ..default() };
        for (key, ship) in v["ships"].as_object().into_iter().flatten() {
            let mut fit = ShipFit::default();
            for (i, slot) in fit.crew.iter_mut().enumerate() {
                *slot = crate::campaign::string_list(&ship["crew"][i]);
            }
            for (bank, gun) in ship["banks"].as_object().into_iter().flatten() {
                if let (Ok(bank), Some(gun)) = (bank.parse(), gun.as_str()) {
                    fit.banks.insert(bank, gun.into());
                }
            }
            out.ships.insert(key.clone(), fit);
        }
        out
    }
    pub fn to_json(&self) -> Value {
        let ships: BTreeMap<_, _> = self.ships.iter().map(|(k, s)| (k, serde_json::json!({"crew": s.crew, "banks": s.banks}))).collect();
        serde_json::json!({"hired": self.hired, "ships": ships})
    }
    fn location(&self, id: &str) -> Option<(&str, usize)> {
        self.ships.iter().find_map(|(key, fit)| fit.crew.iter().position(|r| r.iter().any(|c| c == id)).map(|r| (key.as_str(), r)))
    }
    fn assign(&mut self, key: &str, role: usize, capacity: u32, id: &str) -> Result<(), String> {
        if role >= ROLES.len() || !self.hired.iter().any(|c| c == id) {
            return Err("Recruit this crew member first.".into());
        }
        if self.location(id) == Some((key, role)) {
            return Err("Already assigned to this post.".into());
        }
        if self.ships.get(key).map_or(0, |s| s.crew[role].len()) >= capacity as usize {
            return Err("This post is full; return a member to reserve first.".into());
        }
        for fit in self.ships.values_mut() {
            for slot in &mut fit.crew {
                slot.retain(|c| c != id);
            }
        }
        self.ships.entry(key.into()).or_default().crew[role].push(id.into());
        Ok(())
    }
}

/// Ship identity, not ECS IDs: survives mission restart/save, without applying one hull's mounts to another hull.
fn ship_key(class: &crate::data::Class, mission: &crate::campaign::MissionShip) -> String {
    format!("{}:{}", class.name, if mission.name.is_empty() { mission.id.to_string() } else { mission.name.clone() })
}

#[derive(Component, Clone, PartialEq)]
pub struct CrewPosts(pub [Vec<String>; 8]);
impl CrewPosts {
    pub fn skill(&self, role: usize, skill: &str) -> Option<f32> {
        let values: Vec<_> = self.0[role].iter().filter_map(|id| ability(id, skill)).collect();
        (!values.is_empty()).then(|| values.iter().sum::<f32>() / values.len() as f32)
    }
    pub fn combat(&self, role: usize, generic: f32) -> Vec<f32> {
        self.0[role].iter().map(|c| ability(c, "combat").unwrap_or(generic)).collect()
    }
}

fn spend(points: &mut CampaignPoints, cost: u32) -> Result<(), String> {
    points.total = points.total.checked_sub(cost).ok_or("Not enough victory points.")?;
    Ok(())
}
fn recruit(points: &mut CampaignPoints, id: &str, race: &str) -> Result<(), String> {
    let d = &crew_data()[id];
    if d["race"].as_str() != Some(race) || (!points.crew.is_empty() && !points.crew.iter().any(|c| c == id)) {
        return Err("This crew member is not available to this fleet.".into());
    }
    if points.outfit.hired.iter().any(|c| c == id) {
        return Err("Already recruited.".into());
    }
    let price = d["points"].as_u64().and_then(|v| u32::try_from(v).ok()).ok_or("Missing crew price.")?;
    spend(points, price)?;
    points.outfit.hired.push(id.into());
    Ok(())
}
fn family(gun: &str) -> &str {
    for suffix in ["_Light", "_Med", "_Medium", "_Heavy"] {
        if let Some(family) = gun.strip_suffix(suffix) {
            return family;
        }
    }
    gun
}
/// Mount types/race restrictions are authored; comparing bank members' types is the provisional placement rule.
pub fn bank_compatible(lib: &Library, class: &crate::data::Class, bank: usize, gun: &str) -> bool {
    let Some(g) = lib.guns.get(gun) else { return false };
    if g.exclusion.as_deref() == Some(&class.race) || g.point_defense {
        return false;
    }
    let mut mounts = class.mounts.iter().filter(|m| m.bank == bank).peekable();
    mounts.peek().is_some()
        && mounts.all(|m| {
            let original = &lib.guns[&m.gun];
            if original.point_defense {
                return false;
            }
            match (&original.mount_type, &g.mount_type) {
                (Some(a), Some(b)) => !a.starts_with("Non-Selectable") && a == b,
                // shortcut: legacy/mod guns without mount metadata keep the old family check; export mount types to remove it.
                _ => family(&m.gun) == family(gun),
            }
        })
}

fn bank_cost(lib: &Library, class: &crate::data::Class, bank: usize, gun: &str) -> Option<u32> {
    let g = lib.guns.get(gun)?;
    if !bank_compatible(lib, class, bank, gun) {
        return None;
    }
    let mounts: Vec<_> = class.mounts.iter().filter(|m| m.bank == bank).collect();
    g.points?.checked_mul(u32::try_from(mounts.len()).ok()?)
}
fn upgrade(points: &mut CampaignPoints, lib: &Library, class: &crate::data::Class, key: &str, bank: usize, gun: &str) -> Result<(), String> {
    if !points.arms.is_empty() && !points.arms.iter().any(|g| g == gun) {
        return Err("This gun is not unlocked.".into());
    }
    let cost = bank_cost(lib, class, bank, gun).ok_or("Gun is incompatible or has no price.")?;
    if points.outfit.ships.get(key).and_then(|s| s.banks.get(&bank)).is_some_and(|g| g == gun)
        || (points.outfit.ships.get(key).and_then(|s| s.banks.get(&bank)).is_none() && class.mounts.iter().filter(|m| m.bank == bank).all(|m| m.gun == gun))
    {
        return Err("Already equipped.".into());
    }
    // PROVISIONAL: full destination price per gun, no trade-in refund.
    spend(points, cost)?;
    points.outfit.ships.entry(key.into()).or_default().banks.insert(bank, gun.into());
    Ok(())
}

#[derive(Component, Clone, Debug, PartialEq)]
pub enum Action {
    Close,
    Tab,
    Ship(i32),
    Post(i32),
    Page(i32),
    Recruit(String),
    /// Select a fleet ship / a crew role or weapon bank directly (retail screens: clicking a slot, a post or a mount).
    SelectShip(usize),
    SelectPost(usize),
    Assign(String),
    Reserve(String),
    Upgrade(String),
}
/// What the retail Crew Select / Arms Selection screens show, filled by both the campaign and the skirmish builders.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct FitView {
    /// Arms tab (else Crew).
    pub arms: bool,
    /// Class of each fleet ship (the slot bar) and the one being outfitted.
    pub fleet: Vec<usize>,
    pub ship: usize,
    pub ship_name: String,
    pub race: String,
    pub points: i64,
    /// Crew posts by role (`ROLES` order): one entry per post, `Some((name, portrait))` for a named crew member, `None` for a hand.
    pub posts: Vec<Vec<Option<(String, String)>>>,
    /// Selected role (crew) or index into `banks` (arms).
    pub post: usize,
    /// Maneuverability, Speed, Repair Rate, Strength, 0..1.
    pub bars: [f32; 4],
    /// Selectable weapon banks: (bank number, equipped gun).
    pub banks: Vec<(usize, String)>,
    /// The right-hand list (crew names or weapons) and what clicking each does.
    pub list: Vec<(String, Action)>,
    pub info: Vec<String>,
}

/// Crew-post bars (PROVISIONAL: the original's formula is not recovered): the mean skill of the posts that drive each, named crew
/// at their ability and generic hands at 0.2 — Helmsman navigation, Riggers rigging, Engineers engineering, Fighters combat.
fn bars(posts: &[Vec<Option<String>>]) -> [f32; 4] {
    let mean = |role: usize, skill: &str| {
        let v: Vec<f32> = posts.get(role).into_iter().flatten().map(|p| p.as_deref().and_then(|id| ability(id, skill)).unwrap_or(0.2)).collect();
        if v.is_empty() { 0.0 } else { v.iter().sum::<f32>() / v.len() as f32 }
    };
    [mean(3, "navigation"), mean(5, "rigging"), mean(4, "engineering"), mean(6, "combat")]
}

/// One ship's posts for the Crew screen: per role, its named crew (name, portrait) then generic hands up to the role's capacity.
fn posts_view(class: &crate::data::Class, assigned: Option<&[Vec<String>; 8]>) -> (Vec<Vec<Option<(String, String)>>>, [f32; 4]) {
    let caps = crate::boarding::counts(&class.name);
    let ids: Vec<Vec<Option<String>>> = (0..8)
        .map(|r| {
            let named = assigned.map(|a| a[r].as_slice()).unwrap_or_default();
            (0..caps[r] as usize).map(|i| named.get(i).cloned()).collect()
        })
        .collect();
    let view = ids
        .iter()
        .map(|role| role.iter().map(|id| id.as_ref().map(|id| (name(id).to_string(), crew_data()[id]["portrait"].as_str().unwrap_or_default().to_string()))).collect())
        .collect();
    (view, bars(&ids))
}

/// A weapon's list label: its name without the 'Gun_' prefix.
fn gun_label(gun: &str) -> String {
    gun.trim_start_matches("Gun_").replace('_', " ")
}

/// The weapon banks the Arms screen shows: those whose mounts are selectable (the retail screen hides 'Non-Selectable' mounts).
pub fn selectable_banks(lib: &Library, class: &crate::data::Class) -> Vec<(usize, usize)> {
    crate::combat::banks(lib, class)
        .into_iter()
        .filter(|(_, first)| lib.guns.get(&class.mounts[*first].gun).and_then(|g| g.mount_type.as_deref()).is_none_or(|t| !t.contains("Non-Selectable")))
        .collect()
}

#[derive(Resource, Default, Clone, PartialEq)]
pub struct ScreenModel {
    pub open: bool,
    /// The retail screens' view; `None` keeps the plain list presentation.
    pub fit: Option<FitView>,
    pub title: String,
    pub detail: String,
    pub message: String,
    pub navigation: Vec<(String, Action)>,
    pub rows: Vec<(String, Action)>,
}
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CrewSet;
#[derive(Default)]
struct Choice {
    ship: usize,
    post: usize,
    page: usize,
    arms: bool,
    paused_here: bool,
    message: String,
}
fn cycle(value: usize, delta: i32, len: usize) -> usize {
    if len == 0 { 0 } else { (value as i64 + i64::from(delta)).rem_euclid(len as i64) as usize }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<ScreenModel>()
        .add_systems(Update, (screen, skirmish_screen).chain().in_set(CrewSet).after(crate::menu::RunButtons).before(crate::input))
        .add_systems(Update, (apply_outfit, apply_skirmish_weapons).chain().after(CrewSet).before(crate::sail));
}

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
) {
    if *state.get() != crate::menu::Screen::Battle {
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
        buttons.iter().find(|(_, i)| **i == Interaction::Pressed).map(|(a, _)| a.clone())
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
        let result = match action {
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
                let len = fleet.get(choice.ship).map_or(0, |(_, s, _, _)| if choice.arms { selectable_banks(&lib, &lib.classes[s.class]).len() } else { ROLES.len() });
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
            Action::SelectPost(i) => {
                (choice.post, choice.page) = (i, 0);
                Ok(())
            }
            action => {
                if let Some((_, ship, _, ms)) = fleet.get(choice.ship) {
                    let class = &lib.classes[ship.class];
                    let key = ship_key(class, ms);
                    match action {
                        Action::Recruit(id) => recruit(&mut points, &id, &class.race),
                        Action::Assign(id) => points.outfit.assign(&key, choice.post, crate::boarding::counts(&class.name)[choice.post], &id),
                        Action::Reserve(id) => {
                            if let Some(fit) = points.outfit.ships.get_mut(&key) {
                                fit.crew[choice.post].retain(|c| c != &id);
                            }
                            Ok(())
                        }
                        Action::Upgrade(gun) => selectable_banks(&lib, class)
                            .get(choice.post)
                            .ok_or("No weapon bank.".into())
                            .and_then(|(bank, _)| upgrade(&mut points, &lib, class, &key, *bank, &gun)),
                        _ => Ok(()),
                    }
                } else {
                    Err("No surviving own ship.".into())
                }
            }
        };
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
    let mut next = ScreenModel {
        open: true,
        title: format!("CREW & ARMS — {} victory points", points.total),
        message: choice.message.clone(),
        navigation: vec![
            ("Crew / Arms".into(), Action::Tab),
            ("Previous ship".into(), Action::Ship(-1)),
            ("Next ship".into(), Action::Ship(1)),
            ("Previous post/bank".into(), Action::Post(-1)),
            ("Next post/bank".into(), Action::Post(1)),
        ],
        ..default()
    };
    if let Some((_, ship, _, ms)) = fleet.get(choice.ship) {
        let class = &lib.classes[ship.class];
        let key = ship_key(class, ms);
        let fit = points.outfit.ships.get(&key);
        let mut rows = vec![];
        if choice.arms {
            let banks = selectable_banks(&lib, class);
            choice.post = choice.post.min(banks.len().saturating_sub(1));
            if let Some((bank, first)) = banks.get(choice.post) {
                let equipped = fit.and_then(|s| s.banks.get(bank)).unwrap_or(&class.mounts[*first].gun);
                next.detail = format!(
                    "{} — Bank {}\nEquipped: {}\nPrices are for the entire bank; no trade-in refund.",
                    ms.name,
                    bank,
                    equipped.trim_start_matches("Gun_").replace('_', " ")
                );
                rows = weapon_rows(&lib, class, *bank, equipped, &points.arms, true);
            } else {
                next.detail = "This hull has no editable weapon banks.".into();
            }
        } else {
            let role = choice.post.min(ROLES.len() - 1);
            let assigned = fit.map(|f| f.crew[role].as_slice()).unwrap_or_default();
            next.detail = format!(
                "{} — {} ({}/{})\nNamed crew replace generic hands in these posts; unfilled posts retain generic crew.",
                ms.name,
                ROLES[role],
                assigned.len(),
                crate::boarding::counts(&class.name)[role]
            );
            for id in assigned {
                rows.push((format!("Return {} to reserve", name(id)), Action::Reserve(id.clone())));
            }
            let mut catalogue: Vec<_> = crew_data()
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(id, d)| {
                    (d["race"].as_str() == Some(&class.race) || points.outfit.hired.contains(id))
                        && d["points"].is_u64()
                        && (points.crew.is_empty() || points.crew.contains(id) || points.outfit.hired.contains(id))
                })
                .collect();
            catalogue.sort_by(|a, b| name(a.0).cmp(name(b.0)));
            for (id, d) in catalogue {
                if assigned.contains(id) {
                    continue;
                }
                let skill = ability(id, SKILLS[role]).unwrap_or(0.0) * 10.0;
                if points.outfit.hired.contains(id) {
                    let location = points.outfit.location(id).map_or("reserve".to_string(), |(_, r)| format!("assigned: {}", ROLES[r]));
                    rows.push((
                        format!("Assign {} — {} {:.0}, combat {:.0} ({})", name(id), SKILLS[role], skill, ability(id, "combat").unwrap_or(0.0) * 10.0, location),
                        Action::Assign(id.clone()),
                    ));
                } else {
                    rows.push((
                        format!("Recruit {} — {} VP, {} {:.0}, combat {:.0}", name(id), d["points"], SKILLS[role], skill, ability(id, "combat").unwrap_or(0.0) * 10.0),
                        Action::Recruit(id.clone()),
                    ));
                }
            }
        }
        let (posts, bars) = posts_view(class, fit.map(|f| &f.crew));
        let banks: Vec<(usize, String)> =
            selectable_banks(&lib, class).into_iter().map(|(bank, first)| (bank, fit.and_then(|f| f.banks.get(&bank)).unwrap_or(&class.mounts[first].gun).clone())).collect();
        let list = rows
            .iter()
            .filter_map(|(_, a)| match a {
                Action::Assign(id) | Action::Recruit(id) => Some((name(id).to_string(), a.clone())),
                Action::Upgrade(g) => Some((gun_label(g), a.clone())),
                _ => None,
            })
            .collect();
        next.fit = Some(FitView {
            arms: choice.arms,
            fleet: fleet.iter().map(|(_, s, _, _)| s.class).collect(),
            ship: choice.ship,
            ship_name: ms.name.clone(),
            race: class.race.clone(),
            points: i64::from(points.total),
            posts,
            post: choice.post,
            bars,
            info: if choice.arms { banks.get(choice.post).map(|(b, g)| vec![format!("Bank {b}"), gun_label(g)]).unwrap_or_default() } else { vec![] },
            banks,
            list,
        });
        let pages = rows.len().div_ceil(PAGE).max(1);
        choice.page = choice.page.min(pages - 1);
        next.detail += &format!("\nPage {}/{}", choice.page + 1, pages);
        next.rows = rows.into_iter().skip(choice.page * PAGE).take(PAGE).collect();
    } else {
        next.detail = "No surviving own ship.".into();
    }
    next.navigation.extend([("Previous page".into(), Action::Page(-1)), ("Next page".into(), Action::Page(1)), ("Continue (Enter)".into(), Action::Close)]);
    if *model != next {
        *model = next;
    }
}

fn weapon_rows(lib: &Library, class: &crate::data::Class, bank: usize, equipped: &str, available: &[String], paid: bool) -> Vec<(String, Action)> {
    let mut guns: Vec<_> = lib.guns.keys().filter(|g| available.is_empty() || available.contains(g)).collect();
    guns.sort();
    guns.into_iter()
        .filter_map(|gun| {
            if gun == equipped || !bank_compatible(lib, class, bank, gun) {
                return None;
            }
            let price = if paid { format!(" — {} VP", bank_cost(lib, class, bank, gun)?) } else { String::new() };
            Some((format!("Equip {}{}", gun.trim_start_matches("Gun_").replace('_', " "), price), Action::Upgrade(gun.clone())))
        })
        .collect()
}

/// Skirmish equipment belongs to a fleet entry, not the campaign's currency or a shared hull class.
#[derive(Component)]
pub struct WeaponFit(pub BTreeMap<usize, String>);

fn skirmish_screen(
    state: Res<State<crate::menu::Screen>>,
    mut sk: ResMut<crate::menu::Skirmish>,
    lib: Res<Library>,
    mut model: ResMut<ScreenModel>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    buttons: Query<(&Action, &Interaction), Changed<Interaction>>,
    mut choice: Local<Choice>,
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
        return;
    }
    if !model.open {
        choice.ship = sk.fleet_pick;
        // Screenshot hook (`--script fleet-arms`, see menu.rs `fleet_hook`): open on the Arms tab.
        choice.arms = std::env::args().any(|a| a == "fleet-arms");
    }
    let action = if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Escape) {
        Some(Action::Close)
    } else if mouse.just_pressed(MouseButton::Left) {
        buttons.iter().find(|(_, i)| **i == Interaction::Pressed).map(|(a, _)| a.clone())
    } else {
        None
    };
    if action == Some(Action::Close) {
        keys.clear_just_pressed(KeyCode::Enter);
        keys.clear_just_pressed(KeyCode::Escape);
        mouse.clear_just_pressed(MouseButton::Left);
        sk.arms_open = false;
        *model = ScreenModel::default();
        *choice = Choice::default();
        return;
    }
    let points = sk.points_left(&lib);
    let Some(slot) = sk.slots.iter_mut().find(|s| s.kind == crate::menu::Kind::Human) else { return };
    choice.ship = choice.ship.min(slot.fleet.len().saturating_sub(1));
    if let Some(action) = action {
        match action {
            Action::Ship(d) => {
                choice.ship = cycle(choice.ship, d, slot.fleet.len());
                choice.post = 0;
                choice.page = 0;
            }
            Action::Post(d) => {
                let len = slot.fleet.get(choice.ship).map_or(0, |n| selectable_banks(&lib, &lib.classes[lib.index(n)]).len());
                choice.post = cycle(choice.post, d, len);
                choice.page = 0;
            }
            Action::Page(d) => choice.page = (choice.page as i64 + i64::from(d)).max(0) as usize,
            Action::Tab => {
                choice.arms = !choice.arms;
                (choice.post, choice.page) = (0, 0);
            }
            Action::SelectShip(i) => {
                choice.ship = i.min(slot.fleet.len().saturating_sub(1));
                (choice.post, choice.page) = (0, 0);
            }
            Action::SelectPost(i) => (choice.post, choice.page) = (i, 0),
            Action::Upgrade(gun) => {
                let result = slot.fleet.get(choice.ship).ok_or("Select a ship first.").and_then(|name| {
                    let class = &lib.classes[lib.index(name)];
                    selectable_banks(&lib, class).get(choice.post).ok_or("No weapon bank.").and_then(|(bank, _)| {
                        if !bank_compatible(&lib, class, *bank, &gun) {
                            return Err("Incompatible weapon.");
                        }
                        slot.weapons.resize_with(slot.fleet.len(), BTreeMap::new);
                        slot.weapons[choice.ship].insert(*bank, gun);
                        Ok(())
                    })
                });
                choice.message = result.map_or_else(String::from, |()| "Equipped for this ship.".into());
            }
            _ => {}
        }
    }
    let mut next = ScreenModel {
        open: true,
        title: "SKIRMISH — ARMS".into(),
        message: choice.message.clone(),
        navigation: vec![
            ("Previous ship".into(), Action::Ship(-1)),
            ("Next ship".into(), Action::Ship(1)),
            ("Previous bank".into(), Action::Post(-1)),
            ("Next bank".into(), Action::Post(1)),
            ("Previous page".into(), Action::Page(-1)),
            ("Next page".into(), Action::Page(1)),
            ("Back to fleet (Enter/Esc)".into(), Action::Close),
        ],
        ..default()
    };
    if let Some(name) = slot.fleet.get(choice.ship) {
        let class = &lib.classes[lib.index(name)];
        let banks = selectable_banks(&lib, class);
        choice.post = choice.post.min(banks.len().saturating_sub(1));
        if let Some((bank, first)) = banks.get(choice.post) {
            let equipped = slot.weapons.get(choice.ship).and_then(|w| w.get(bank)).unwrap_or(&class.mounts[*first].gun);
            let rows = weapon_rows(&lib, class, *bank, equipped, &[], false);
            let pages = rows.len().div_ceil(PAGE).max(1);
            choice.page = choice.page.min(pages - 1);
            next.detail = format!(
                "Ship {}: {} — Bank {}\nEquipped: {}\nSkirmish budget is not implemented; fittings do not spend campaign points.\nPage {}/{}",
                choice.ship + 1,
                crate::menu::pretty(name),
                bank,
                equipped.trim_start_matches("Gun_").replace('_', " "),
                choice.page + 1,
                pages
            );
            next.rows = rows.into_iter().skip(choice.page * PAGE).take(PAGE).collect();
        } else {
            next.detail = "This hull has no weapon banks.".into();
        }
    } else {
        next.detail = "Add a ship to the fleet first.".into();
    }
    if let Some(name) = slot.fleet.get(choice.ship) {
        let ci = lib.index(name);
        let class = &lib.classes[ci];
        let banks: Vec<(usize, String)> = selectable_banks(&lib, class)
            .into_iter()
            .map(|(bank, first)| (bank, slot.weapons.get(choice.ship).and_then(|w| w.get(&bank)).unwrap_or(&class.mounts[first].gun).clone()))
            .collect();
        let (posts, bars) = posts_view(class, None);
        let list = match banks.get(choice.post) {
            Some((bank, equipped)) if choice.arms => weapon_rows(&lib, class, *bank, equipped, &[], false)
                .into_iter()
                .map(|(_, a)| (if let Action::Upgrade(g) = &a { gun_label(g) } else { String::new() }, a))
                .collect(),
            _ => vec![],
        };
        next.fit = Some(FitView {
            arms: choice.arms,
            fleet: slot.fleet.iter().map(|n| lib.index(n)).collect(),
            ship: choice.ship,
            ship_name: crate::menu::pretty(name),
            race: class.race.clone(),
            points,
            posts,
            post: choice.post,
            bars,
            info: banks.get(choice.post).filter(|_| choice.arms).map(|(b, g)| vec![format!("Bank {b}"), gun_label(g)]).unwrap_or_default(),
            banks,
            list,
        });
    }
    sk.fleet_pick = choice.ship;
    if *model != next {
        *model = next;
    }
}

fn apply_skirmish_weapons(
    mut commands: Commands,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    mut ships: Query<(Entity, &crate::Ship, &crate::combat::Health, &WeaponFit, &mut crate::combat::GunStates)>,
) {
    for (entity, ship, health, fit, mut guns) in &mut ships {
        if health.destroyed {
            continue;
        }
        let class = &lib.classes[ship.class];
        for (index, mount) in class.mounts.iter().enumerate() {
            let gun = fit.0.get(&mount.bank).filter(|g| bank_compatible(&lib, class, mount.bank, g)).unwrap_or(&mount.gun);
            if guns.gun(index) != gun {
                guns.replace(&mut commands, &assets, &lib, entity, mount, index, gun);
            }
        }
    }
}

fn apply_outfit(
    mut commands: Commands,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    points: Res<CampaignPoints>,
    mission: Option<Res<crate::campaign::Mission>>,
    mut ships: Query<(
        Entity,
        &crate::Ship,
        &crate::combat::Health,
        &crate::campaign::MissionShip,
        &mut crate::combat::GunStates,
        &mut crate::boarding::Boardable,
        Option<&CrewPosts>,
    )>,
) {
    let Some(mission) = mission else { return };
    for (e, ship, health, ms, mut guns, mut boarding, posts) in &mut ships {
        if health.destroyed {
            continue;
        }
        let class = &lib.classes[ship.class];
        let fit = (ms.owner == mission.script.player()).then(|| points.outfit.ships.get(&ship_key(class, ms))).flatten();
        let wanted = fit.map(|fit| {
            let mut crew: [Vec<String>; 8] = default();
            let mut used = std::collections::HashSet::new();
            for (role, slot) in crew.iter_mut().enumerate() {
                *slot = fit.crew[role]
                    .iter()
                    .filter(|id| points.outfit.hired.contains(id) && crew_data()[id.as_str()].is_object() && used.insert((*id).clone()))
                    .take(crate::boarding::counts(&class.name)[role] as usize)
                    .cloned()
                    .collect();
            }
            CrewPosts(crew)
        });
        if posts != wanted.as_ref() {
            if let Some(posts) = &wanted {
                boarding.apply_posts(posts);
                commands.entity(e).insert(posts.clone());
            } else {
                boarding.reset_crew();
                commands.entity(e).remove::<CrewPosts>();
            }
        }
        for (i, mount) in class.mounts.iter().enumerate() {
            let gun = fit.and_then(|s| s.banks.get(&mount.bank)).filter(|g| bank_compatible(&lib, class, mount.bank, g)).unwrap_or(&mount.gun);
            if guns.gun(i) != gun {
                guns.replace(&mut commands, &assets, &lib, e, mount, i, gun);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recruitment_spends_once_and_assignment_moves_without_duplication() {
        let mut p = CampaignPoints::default();
        p.total = 100;
        recruit(&mut p, "Crew_Navy_BenH", "Navy").unwrap();
        let paid = p.total;
        assert!(recruit(&mut p, "Crew_Navy_BenH", "Navy").is_err());
        assert_eq!(p.total, paid);
        assert!(recruit(&mut p, "not-a-crew", "Navy").is_err());
        p.outfit.assign("A", 0, 1, "Crew_Navy_BenH").unwrap();
        p.outfit.assign("B", 4, 1, "Crew_Navy_BenH").unwrap();
        assert!(p.outfit.ships["A"].crew[0].is_empty());
        assert_eq!(p.outfit.location("Crew_Navy_BenH"), Some(("B", 4)));
        assert!(p.outfit.assign("C", 0, 0, "Crew_Navy_BenH").is_err());
        assert_eq!(Outfit::from_json(&p.outfit.to_json()), p.outfit);
        assert_eq!(CrewPosts(p.outfit.ships["B"].crew.clone()).skill(4, "engineering"), Some(0.5));
    }
    #[test]
    fn bank_purchase_is_atomic_and_matches_mount_types_not_weapon_families() {
        let lib = Library::load(&crate::assets_dir());
        let class = &lib.classes[lib.index("Ship_Navy_Cutter")];
        let mount = class.mounts.iter().find(|m| family(&m.gun) == "Gun_CannonBall").unwrap();
        let gun = "Gun_Carronade_Med";
        let cost = bank_cost(&lib, class, mount.bank, gun).unwrap();
        let mut p = CampaignPoints::default();
        p.total = cost.saturating_sub(1);
        if cost > 0 {
            assert!(upgrade(&mut p, &lib, class, "A", mount.bank, gun).is_err());
        }
        assert!(p.outfit.ships.is_empty());
        p.total = cost;
        upgrade(&mut p, &lib, class, "A", mount.bank, gun).unwrap();
        assert_eq!(p.total, 0);
        assert_eq!(p.outfit.ships["A"].banks[&mount.bank], gun);
        assert!(upgrade(&mut p, &lib, class, "A", mount.bank, gun).is_err());
        assert!(bank_cost(&lib, class, mount.bank, "Gun_Harpoon_Light").is_none());
        assert!(bank_compatible(&lib, class, mount.bank, "Gun_GravCharge_Med"));
        assert!(!bank_compatible(&lib, class, mount.bank, "Gun_CannonBall_Heavy"));
        assert!(!bank_compatible(&lib, class, mount.bank, "Gun_Lancer_PD_Med"));
        let fixed = class.mounts.iter().find(|m| lib.guns[&m.gun].point_defense).unwrap();
        assert!(!bank_compatible(&lib, class, fixed.bank, "Gun_Lancer_Med"));
        assert!(weapon_rows(&lib, class, mount.bank, &mount.gun, &[], false).iter().any(|(_, a)| *a == Action::Upgrade("Gun_Carronade_Med".into())));
    }
    #[test]
    fn saved_outfit_changes_live_posts_and_weapon_models_without_resetting_casualties() {
        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()))
            .init_asset::<bevy::world_serialization::WorldAsset>()
            .add_systems(Update, (apply_outfit, apply_skirmish_weapons).chain());
        let assets = app.world().resource::<AssetServer>().clone();
        let lib = Library::load(&crate::assets_dir());
        let class = lib.index("Ship_Navy_Cutter");
        let mission = crate::campaign::load_id("mission03");
        let ms = crate::campaign::MissionShip { id: 77, name: "test".into(), owner: mission.script.player().into(), ..default() };
        let key = ship_key(&lib.classes[class], &ms);
        let index = lib.classes[class].mounts.iter().position(|m| m.gun == "Gun_CannonBall_Med").unwrap();
        let bank = lib.classes[class].mounts[index].bank;
        let boardable = crate::boarding::Boardable::ship(&lib.classes[class].name, crate::boarding::Skill::Green, true);
        let original_strength = boardable.strength.defend;
        let ship = app
            .world_mut()
            .spawn((
                crate::Ship {
                    class,
                    side: crate::Side::Player,
                    heading: 0.0,
                    speed: 0.0,
                    ang_vel: 0.0,
                    order: 0,
                    waypoints: vec![],
                    goal: None,
                    cap: 1.0,
                    throttle: None,
                    sail_energy: 1.0,
                },
                crate::combat::Health::new(&lib.classes[class]),
                ms,
                boardable,
            ))
            .id();
        let guns = crate::combat::spawn_guns(&mut app.world_mut().commands(), &assets, &lib, ship, class);
        app.world_mut().entity_mut(ship).insert(guns);
        app.world_mut().flush();
        let old_roots: Vec<_> = app.world().get::<Children>(ship).unwrap().iter().collect();
        let mut points = CampaignPoints::default();
        points.outfit.hired.push("Crew_Navy_BenH".into());
        let fit = points.outfit.ships.entry(key).or_default();
        fit.crew[4].push("Crew_Navy_BenH".into());
        fit.banks.insert(bank, "Gun_Carronade_Med".into());
        app.insert_resource(lib).insert_resource(points).insert_resource(mission);
        app.update();
        assert_eq!(app.world().get::<crate::combat::GunStates>(ship).unwrap().gun(index), "Gun_Carronade_Med");
        assert_eq!(app.world().get::<CrewPosts>(ship).unwrap().skill(4, "engineering"), Some(0.5));
        assert!(app.world().get::<crate::boarding::Boardable>(ship).unwrap().strength.defend > original_strength);
        assert_eq!(app.world().get::<Children>(ship).unwrap().len(), old_roots.len(), "replacement must not leak old gun hierarchies");
        app.world_mut().get_mut::<crate::boarding::Boardable>(ship).unwrap().strength.defend -= 1;
        let hurt = app.world().get::<crate::boarding::Boardable>(ship).unwrap().strength.defend;
        app.update();
        assert_eq!(app.world().get::<crate::boarding::Boardable>(ship).unwrap().strength.defend, hurt, "unchanged outfitting must not heal boarding casualties");
        // Skirmish uses the same equipped gun hierarchy without a mission or campaign transaction.
        let balance = app.world().resource::<CampaignPoints>().total;
        app.world_mut().entity_mut(ship).remove::<crate::campaign::MissionShip>().insert(WeaponFit(BTreeMap::from([(bank, "Gun_GravCharge_Med".into())])));
        app.update();
        assert_eq!(app.world().get::<crate::combat::GunStates>(ship).unwrap().gun(index), "Gun_GravCharge_Med");
        assert_eq!(app.world().resource::<CampaignPoints>().total, balance);
    }

    #[test]
    fn best_gunner_sets_the_legacy_crewed_accuracy() {
        let mut p = CampaignPoints::default();
        p.crew = vec!["Crew_Navy_BenH".into()];
        assert_eq!(best(&p, "gunnery"), Some(0.5));
        assert!((accuracy(Some(&p)) - 0.6).abs() < 1e-6);
        assert_eq!(accuracy(Some(&CampaignPoints::default())), 0.5);
    }
}
