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
    DATA.get_or_init(|| crate::data::try_json(&crate::assets_dir().join("data/crew.json")).unwrap_or_default())
}
pub fn ability(id: &str, skill: &str) -> Option<f32> {
    crew_data()[id]["skills"][skill].as_f64().map(|v| (v as f32 / 10.0).clamp(0.0, 1.0))
}
pub fn species(id: &str) -> Option<&str> {
    crew_data()[id]["species"].as_str()
}
/// Talking-head sprite of a named crew member (`TalkingHead_Character_*`).
pub fn portrait(id: &str) -> Option<&str> {
    crew_data()[id]["portrait"].as_str().filter(|p| !p.is_empty())
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
    /// The post's skill over every slot (capacity `slots`), generic hands counting as 0.1 (default crew are rated 1 of 10).
    /// PROVISIONAL: the mean with several members per post; measured only on single-slot posts.
    pub fn post_skill(&self, role: usize, skill: &str, slots: u32) -> f32 {
        let named: Vec<f32> = self.0[role].iter().map(|id| ability(id, skill).unwrap_or(0.1)).collect();
        let slots = slots.max(named.len() as u32).max(1);
        (named.iter().sum::<f32>() + 0.1 * (slots as usize - named.len()) as f32) / slots as f32
    }
    /// (top-speed, rudder-torque) factors: engineers' engineering and the helmsman's navigation (docs/re/crew.md).
    pub fn handling(&self, caps: &[u32; 8]) -> (f32, f32) {
        (self.post_factor(4, "engineering", caps[4]), self.post_factor(3, "navigation", caps[3]))
    }
    /// Mean of every slot's `skill_factor` (capacity `slots`; empty slots hold a rating-1 hand), as in retail saves: three
    /// engineers rated 9, 9, 8 give (1.5 + 1.5 + 1.475) / 3 / 1.125.
    pub fn post_factor(&self, role: usize, skill: &str, slots: u32) -> f32 {
        let named: Vec<f32> = self.0[role].iter().map(|id| skill_factor(ability(id, skill).unwrap_or(0.1))).collect();
        let slots = slots.max(named.len() as u32).max(1);
        (named.iter().sum::<f32>() + (slots as usize - named.len()) as f32) / slots as f32
    }
    pub fn combat(&self, role: usize, generic: f32) -> Vec<f32> {
        self.0[role].iter().map(|c| ability(c, "combat").unwrap_or(generic)).collect()
    }
}

/// One crewman's factor on top speed (engineering) or rudder torque (navigation), relative to a rating-1 hand: retail saves
/// store min(1.075 + 0.05 x rating, 1.5) per post (rating 1 -> 1.125, 9 -> 1.5), and speed and turn follow it exactly
/// (docs/re/crew.md "Saved crew factors"). `skill` is rating / 10.
pub fn skill_factor(skill: f32) -> f32 {
    (1.075 + 0.5 * skill).min(1.5) / 1.125
}

/// The skill a post is rated on (`ROLES` order).
pub fn post_skill_name(role: usize) -> &'static str {
    SKILLS[role.min(SKILLS.len() - 1)]
}

/// What a crew member costs in victory points (0 when unknown).
pub fn crew_points(id: &str) -> u32 {
    crew_data()[id]["points"].as_u64().and_then(|v| u32::try_from(v).ok()).unwrap_or(0)
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

pub fn bank_cost(lib: &Library, class: &crate::data::Class, bank: usize, gun: &str) -> Option<u32> {
    let g = lib.guns.get(gun)?;
    if !bank_compatible(lib, class, bank, gun) {
        return None;
    }
    g.points?.checked_mul(u32::try_from(class.mounts.iter().filter(|m| m.bank == bank).count()).ok()?)
}
/// Victory points a skirmish ship's fitting costs beyond its stock guns: per bank, the price of the fitted guns minus the stock ones
/// (negative when cheaper). Measured on the original: swapping a four-gun heavy bank from a 10-point gun to an 8-point gun showed
/// 'Total Cost: -8' and returned 8 points.
pub fn fitting_delta(lib: &Library, class: &crate::data::Class, fit: &BTreeMap<usize, String>) -> i64 {
    fit.iter()
        .map(|(&bank, gun)| {
            let stock = class.mounts.iter().find(|m| m.bank == bank).and_then(|m| bank_cost(lib, class, bank, &m.gun)).unwrap_or(0);
            i64::from(bank_cost(lib, class, bank, gun).unwrap_or(stock)) - i64::from(stock)
        })
        .sum()
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
    /// Scroll the slot bar by this many slots.
    Slots(i32),
    SelectPost(usize),
    /// A face with a named crew member on it (role, index in the post): picks them up to move them; as a drop target it acts like its post.
    PostFace(usize, usize),
    Assign(String),
    Reserve(String),
    Upgrade(String),
    /// Skirmish screens: pick a roster entry or a weapon (then drag or click it onto a post or mount), cycle the sort, drop the selected post's last crew member.
    PickCrew(String),
    PickWeapon(String),
    /// Open or close the Sort By pull-down.
    Sort,
    /// Sort the roster by `SORTS[i]` and close the pull-down.
    SortBy(usize),
    Drop,
    AutoAssign,
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
    /// The info panel (top right): the picked crew member or weapon.
    pub detail: Option<Detail>,
    /// The left box on the Arms screen: retail shows the help text until an upgrade reports its cost or why it failed.
    pub message: String,
    /// The Drop button is live (a named crew member stands on the selected post).
    pub can_drop: bool,
    /// The list row of the carried (picked) crew member or weapon, highlighted.
    pub picked: Option<usize>,
    /// The crew list's sort field (the Sort By box).
    pub sort: &'static str,
    /// The Sort By pull-down is open (its entries are `SORTS`).
    pub sort_open: bool,
    /// The sprite of what the cursor carries (a crew portrait or a gun icon), drawn under the cursor.
    pub carrying: Option<String>,
    /// First visible row of `list`.
    pub scroll: usize,
    /// First fleet slot in the slot bar and the number of slots the fleet may have.
    pub slot_scroll: usize,
    pub limit: usize,
}

/// The info panel of the Crew and Arms screens: a name, an icon, an optional point cost and label / value lines.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Detail {
    pub title: String,
    pub icon: String,
    pub cost: Option<u32>,
    pub lines: Vec<(String, String)>,
}

/// Crew list sort fields, as the original's Sort By list offers them.
pub const SORTS: [&str; 9] = ["Name", "Victory Points", "Leadership", "Gunnery", "Navigation", "Rigging", "Engineering", "Combat", "Spotting"];

/// Named crew a race can field in skirmish (their cost is in victory points), in the given sort order (skills high to low).
pub fn roster(race: &str, sort: usize) -> Vec<String> {
    let mut ids: Vec<String> =
        crew_data().as_object().into_iter().flatten().filter(|(_, d)| d["race"].as_str() == Some(race) && d["points"].is_u64()).map(|(id, _)| id.clone()).collect();
    let key = |id: &String| -> i64 {
        match SORTS[sort.min(SORTS.len() - 1)] {
            "Name" => 0,
            "Victory Points" => -i64::from(crew_points(id)),
            skill => -(ability(id, &skill.to_lowercase()).unwrap_or(0.0) * 100.0) as i64,
        }
    };
    ids.sort_by(|a, b| key(a).cmp(&key(b)).then_with(|| name(a).cmp(name(b))));
    ids
}

/// The crew member's info panel: portrait, cost and the seven skills (N/A where the character has none).
pub fn crew_detail(id: &str) -> Detail {
    let strings = crate::menu::strings();
    let s = |k: &str| crate::menu::text_of(strings, k);
    let skills = [
        ("GUNNERY", "gunnery"),
        ("LEADERSHIP", "leadership"),
        ("NAVIGATION", "navigation"),
        ("ENGINEERING", "engineering"),
        ("RIGGING", "rigging"),
        ("SPOTTING", "spotting"),
        ("COMBAT", "combat"),
    ];
    Detail {
        title: name(id).to_string(),
        icon: crew_data()[id]["portrait"].as_str().unwrap_or_default().to_string(),
        cost: Some(crew_points(id)),
        lines: skills
            .iter()
            .map(|(label, skill)| {
                (s(&format!("IDGS_TPFRONTENDTEXT_CREWSKILL_{label}")), ability(id, skill).map_or_else(|| s("IDGS_TPFRONTENDTEXT01_NA_STRING"), |v| format!("{:.0}", v * 10.0)))
            })
            .collect(),
    }
}

/// The weapon's info panel: ranges, damage (PROVISIONAL: lowest to highest hit over the hull materials) and the point cost of one gun.
pub fn weapon_detail(lib: &Library, gun: &str) -> Option<Detail> {
    let g = lib.guns.get(gun)?;
    let strings = crate::menu::strings();
    let s = |k: &str| crate::menu::text_of(strings, k);
    let unit = s("IDGS_TPFRONTENDTEXT01_METERS");
    let damage = lib.bullets.get(&g.bullet).map(|b| {
        let (lo, hi) = b.damage.values().fold((f32::MAX, 0.0f32), |(lo, hi), d| (lo.min(d[0]), hi.max(d[1])));
        format!("{lo:.0} - {hi:.0}")
    });
    Some(Detail {
        title: gun_label(gun),
        icon: crate::combat::gun_icon(gun),
        cost: g.points,
        lines: [
            (s("IDGS_TPFRONTENDTEXT01_GUN_MAXIMUM_RANGE"), format!("{:.0} {unit}", g.range[2])),
            (s("IDGS_TPFRONTENDTEXT01_GUN_EFFECTIVE_RANGE"), format!("{:.0} {unit}", g.range[0])),
        ]
        .into_iter()
        .chain(damage.map(|d| (s("IDGS_TPFRONTENDTEXT01_GUN_DAMAGE"), d)))
        .collect(),
    })
}

/// Crew-post bars, mapped from the retail screen (docs/re/crew.md): Helmsman navigation, Engineers engineering, Engineers' engineering
/// with Riggers' rigging, Fighters combat; generic hands count as 0.1. PROVISIONAL: bar scale and the hull's base part are not drawn.
fn bars(posts: &[Vec<Option<String>>]) -> [f32; 4] {
    let skills = |role: usize, skill: &str| -> Vec<f32> { posts.get(role).into_iter().flatten().map(|p| p.as_deref().and_then(|id| ability(id, skill)).unwrap_or(0.1)).collect() };
    let mean = |v: Vec<f32>| if v.is_empty() { 0.0 } else { v.iter().sum::<f32>() / v.len() as f32 };
    [mean(skills(3, "navigation")), mean(skills(4, "engineering")), mean([skills(4, "engineering"), skills(5, "rigging")].concat()), mean(skills(6, "combat"))]
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

/// The selectable banks with the gun each carries: the fitted one from `fit`, else the class's own.
fn equipped_banks(lib: &Library, class: &crate::data::Class, fit: Option<&BTreeMap<usize, String>>) -> Vec<(usize, String)> {
    selectable_banks(lib, class).into_iter().map(|(bank, first)| (bank, fit.and_then(|f| f.get(&bank)).unwrap_or(&class.mounts[first].gun).clone())).collect()
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
        .add_systems(Update, (apply_outfit, apply_skirmish_weapons, apply_skirmish_crew).chain().after(CrewSet).before(crate::sail));
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
            Action::SelectPost(i) | Action::PostFace(i, _) => {
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
                            .ok_or_else(|| "No weapon bank.".into())
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
    let next = campaign_screen_model(&points, &lib, &fleet, &mut choice);
    if *model != next {
        *model = next;
    }
}

fn campaign_screen_model(
    points: &CampaignPoints,
    lib: &Library,
    fleet: &[(Entity, &crate::Ship, &crate::combat::Health, &crate::campaign::MissionShip)],
    choice: &mut Choice,
) -> ScreenModel {
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
            let banks = selectable_banks(lib, class);
            choice.post = choice.post.min(banks.len().saturating_sub(1));
            if let Some((bank, first)) = banks.get(choice.post) {
                let equipped = fit.and_then(|s| s.banks.get(bank)).unwrap_or(&class.mounts[*first].gun);
                next.detail = format!(
                    "{} — Bank {}\nEquipped: {}\nPrices are for the entire bank; no trade-in refund.",
                    ms.name,
                    bank,
                    equipped.trim_start_matches("Gun_").replace('_', " ")
                );
                rows = weapon_rows(lib, class, *bank, equipped, &points.arms, true);
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
            rows = crew_rows(points, class, role, assigned);
        }
        let (posts, bars) = posts_view(class, fit.map(|f| &f.crew));
        let banks = equipped_banks(lib, class, fit.map(|f| &f.banks));
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
            detail: if choice.arms { banks.get(choice.post).and_then(|(_, g)| weapon_detail(lib, g)) } else { None },
            banks,
            list,
            ..default()
        });
        let pages = rows.len().div_ceil(PAGE).max(1);
        choice.page = choice.page.min(pages - 1);
        next.detail += &format!("\nPage {}/{}", choice.page + 1, pages);
        next.rows = rows.into_iter().skip(choice.page * PAGE).take(PAGE).collect();
    } else {
        next.detail = "No surviving own ship.".into();
    }
    next.navigation.extend([("Previous page".into(), Action::Page(-1)), ("Next page".into(), Action::Page(1)), ("Continue (Enter)".into(), Action::Close)]);
    next
}

/// The campaign Crew rows of post `role`: send each `assigned` member back to the reserve, then assign hired crew or recruit new
/// ones of the ship's race (from the campaign's roster when it has one), by name.
fn crew_rows(points: &CampaignPoints, class: &crate::data::Class, role: usize, assigned: &[String]) -> Vec<(String, Action)> {
    let mut rows = vec![];
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
            let location = points.outfit.location(id).map_or_else(|| "reserve".to_string(), |(_, r)| format!("assigned: {}", ROLES[r]));
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
    rows
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

/// What the cursor carries between a list row and a post or mount: a click on a row picks it, a release or a click on a post places it.
#[derive(Clone, PartialEq)]
enum Carry {
    Crew(String),
    Gun(String),
}

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
    let pressed = if mouse.just_pressed(MouseButton::Left) { buttons.iter().find(|(_, i)| **i == Interaction::Pressed).map(|(a, _)| as_target(a, carrying)) } else { None };
    // Released over a post or mount while carrying: the drop of a drag.
    let released_on = if mouse.just_released(MouseButton::Left) && carry.is_some() {
        all.iter().find(|(a, i)| matches!(a, Action::SelectPost(_) | Action::PostFace(..)) && **i != Interaction::None).map(|(a, _)| as_target(a, true))
    } else {
        None
    };
    let action = if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Escape) { Some(Action::Close) } else { pressed.or(released_on) };
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
        match action {
            Action::Ship(d) => {
                choice.ship = cycle(choice.ship, d, slot.fleet.len());
                (choice.post, choice.page) = (0, 0);
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
                choice.message.clear();
            }
            Action::SelectShip(i) => {
                choice.ship = i.min(slot.fleet.len().saturating_sub(1));
                (choice.post, choice.page) = (0, 0);
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
                if let Some(id) = slot.crew.get(choice.ship).and_then(|c| c[r.min(7)].get(k)).cloned() {
                    *carry = Some(Carry::Crew(id));
                    *origin = Some((r, k));
                }
            }
            Action::PickWeapon(gun) => *carry = Some(Carry::Gun(gun)),
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
                        choice.post = i;
                        let cap = crate::boarding::counts(&class.name)[i.min(7)] as usize;
                        let crew = slot.crew_mut(choice.ship);
                        // Moving a posted crew member lifts them off their post first (their points come back).
                        let moved = origin.take();
                        if let Some((r, _)) = moved {
                            crew[r].retain(|c| c != &id);
                        }
                        let back = if moved.is_some() { i64::from(crew_points(&id)) } else { 0 };
                        let refund = if crew[i].len() >= cap { crew[i].last().map_or(0, |l| i64::from(crew_points(l))) } else { 0 };
                        choice.message = if cap == 0 {
                            "This ship has no such post.".into()
                        } else if i64::from(crew_points(&id)) > points + refund + back {
                            "You do not have enough victory points.".into()
                        } else {
                            if crew[i].len() >= cap {
                                crew[i].pop();
                            }
                            crew[i].push(id.clone());
                            String::new()
                        };
                        if !choice.message.is_empty()
                            && let Some((r, k)) = moved
                        {
                            let at = k.min(crew[r].len());
                            crew[r].insert(at, id);
                        }
                    }
                    // Fitting a weapon on a mount of the Arms tab.
                    Some(Carry::Gun(gun)) if choice.arms => {
                        choice.post = i;
                        choice.message = match selectable_banks(&lib, class).get(i) {
                            None => said("IDGS_TPFRONTENDTEXT02_WEAPON_WRONG_MOUNT"),
                            Some((bank, _)) if !bank_compatible(&lib, class, *bank, &gun) => said("IDGS_TPFRONTENDTEXT02_WEAPON_WRONG_MOUNT"),
                            Some((bank, first)) => {
                                slot.weapons.resize_with(slot.fleet.len(), BTreeMap::new);
                                let old = slot.weapons[choice.ship].get(bank).unwrap_or(&class.mounts[*first].gun).clone();
                                let delta = i64::from(bank_cost(&lib, class, *bank, &gun).unwrap_or(0)) - i64::from(bank_cost(&lib, class, *bank, &old).unwrap_or(0));
                                if delta > points {
                                    said("IDGS_TPFRONTENDTEXT02_WEAPON_NOT_ENOUGH_VICTORY_POINTS")
                                } else {
                                    slot.weapons[choice.ship].insert(*bank, gun);
                                    // Retail's message block: 'Total Cost: <change>' and 'Weapon Upgrade Successful'.
                                    format!("{} {}\n{}", said("IDGS_TPFRONTENDTEXT02_TOTAL_COST"), delta, said("IDGS_TPFRONTENDTEXT02_WEAPON_UPGRADE_SUCCESSFUL"))
                                }
                            }
                        };
                    }
                    _ => {
                        (choice.post, choice.page) = (i, 0);
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
    sk.show_picked_slot();
    if *model != next {
        *model = next;
    }
}

fn skirmish_screen_model(
    sk: &crate::menu::Skirmish,
    slot: &crate::menu::Slot,
    lib: &Library,
    choice: &mut Choice,
    carry: &Option<Carry>,
    sort: usize,
    sort_open: bool,
    points: i64,
) -> ScreenModel {
    let mut next = ScreenModel {
        open: true,
        title: "SKIRMISH — CREW & ARMS".into(),
        message: choice.message.clone(),
        navigation: vec![("Previous ship".into(), Action::Ship(-1)), ("Next ship".into(), Action::Ship(1)), ("Back to fleet (Enter/Esc)".into(), Action::Close)],
        ..default()
    };
    if let Some(ship_name) = slot.fleet.get(choice.ship) {
        let ci = lib.index(ship_name);
        let class = &lib.classes[ci];
        let banks = equipped_banks(lib, class, slot.weapons.get(choice.ship));
        let assigned = slot.crew.get(choice.ship);
        let (posts, bars) = posts_view(class, assigned);
        let (mut list, mut picked, mut detail) = (vec![], None, None);
        if choice.arms {
            choice.post = choice.post.min(banks.len().saturating_sub(1));
            if let Some((bank, equipped)) = banks.get(choice.post) {
                let mut guns: Vec<&String> = lib.guns.keys().filter(|g| bank_compatible(lib, class, *bank, g)).collect();
                guns.sort_by_key(|g| gun_label(g));
                let want = match carry {
                    Some(Carry::Gun(g)) => g,
                    _ => equipped,
                };
                picked = guns.iter().position(|g| *g == want);
                detail = weapon_detail(lib, want);
                list = guns.into_iter().map(|g| (gun_label(g), Action::PickWeapon(g.clone()))).collect();
            }
        } else {
            let used: Vec<&String> = slot.crew.iter().flatten().flatten().collect();
            let ids: Vec<String> = roster(&class.race, sort).into_iter().filter(|id| !used.contains(&id)).collect();
            if let Some(Carry::Crew(id)) = carry {
                picked = ids.iter().position(|x| x == id);
                detail = Some(crew_detail(id));
            }
            list = ids.into_iter().map(|id| (name(&id).to_string(), Action::PickCrew(id))).collect();
        }
        choice.page = choice.page.min(list.len().saturating_sub(8));
        let can_drop = !choice.arms && assigned.is_some_and(|c| c.get(choice.post).is_some_and(|r| !r.is_empty()));
        next.fit = Some(FitView {
            arms: choice.arms,
            fleet: slot.fleet.iter().map(|n| lib.index(n)).collect(),
            ship: choice.ship,
            ship_name: crate::menu::pretty(ship_name),
            race: class.race.clone(),
            points,
            posts,
            post: choice.post,
            bars,
            banks,
            list,
            detail,
            message: choice.message.clone(),
            can_drop,
            picked,
            sort: SORTS[sort],
            sort_open: sort_open && !choice.arms,
            carrying: carry.as_ref().map(|c| match c {
                Carry::Crew(id) => crew_data()[id]["portrait"].as_str().unwrap_or_default().to_string(),
                Carry::Gun(g) => crate::combat::gun_icon(g),
            }),
            scroll: choice.page,
            slot_scroll: sk.slot_scroll,
            limit: sk.fleet_size,
        });
    } else {
        next.detail = "Add a ship to the fleet first.".into();
    }
    next
}

/// The named crew a skirmish ship was assembled with; `apply_skirmish_crew` turns it into posts, handling and boarding crews.
#[derive(Component, Clone)]
pub struct SkirmishCrew(pub [Vec<String>; 8]);

fn apply_skirmish_crew(
    mut commands: Commands,
    lib: Res<Library>,
    mut ships: Query<(Entity, &mut crate::Ship, &SkirmishCrew, &mut crate::boarding::Boardable), Added<SkirmishCrew>>,
) {
    for (e, mut ship, crew, mut boarding) in &mut ships {
        let caps = crate::boarding::counts(&lib.classes[ship.class].name);
        let posts = CrewPosts(std::array::from_fn(|r| crew.0[r].iter().take(caps[r] as usize).cloned().collect()));
        (ship.crew_speed, ship.crew_turn) = posts.handling(&caps);
        boarding.apply_posts(&posts);
        commands.entity(e).insert(posts);
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
        &mut crate::Ship,
        &crate::combat::Health,
        Ref<crate::campaign::MissionShip>,
        &mut crate::combat::GunStates,
        &mut crate::boarding::Boardable,
        Option<&CrewPosts>,
    )>,
) {
    let Some(mission) = mission else { return };
    // The outfit only changes with the saved points or the mission; otherwise only ships that just spawned need fitting.
    let dirty = points.is_changed() || mission.is_changed();
    for (e, mut ship, health, ms, mut guns, mut boarding, posts) in &mut ships {
        if health.destroyed || !(dirty || ms.is_added()) {
            continue;
        }
        let class = &lib.classes[ship.class];
        let fit = (ms.owner == mission.script.player()).then(|| points.outfit.ships.get(&ship_key(class, &ms))).flatten();
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
            (ship.crew_speed, ship.crew_turn) = wanted.as_ref().map_or((1.0, 1.0), |p| p.handling(&crate::boarding::counts(&class.name)));
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
    fn campaign_screen_without_surviving_ships_still_offers_continue() {
        let lib = Library::load(crate::assets_dir());
        let model = campaign_screen_model(&CampaignPoints::default(), &lib, &[], &mut Choice::default());
        assert!(model.open);
        assert_eq!(model.detail, "No surviving own ship.");
        assert!(model.fit.is_none());
        assert!(model.navigation.iter().any(|(_, action)| *action == Action::Close));
    }

    #[test]
    fn crew_rows_list_the_assigned_member_once_as_a_return() {
        let lib = Library::load(crate::assets_dir());
        let class = &lib.classes[lib.index("Ship_Navy_Torpedo")];
        let roster = roster(&class.race, 0);
        let (first, second) = (roster[0].clone(), roster[1].clone());
        let mut points = CampaignPoints::default();
        points.outfit.hired.push(second.clone());
        let rows = crew_rows(&points, class, 1, std::slice::from_ref(&first));
        assert_eq!(rows[0].1, Action::Reserve(first.clone()));
        assert!(!rows[1..].iter().any(|(_, a)| matches!(a, Action::Assign(id) | Action::Recruit(id) if *id == first)));
        assert!(rows.iter().any(|(_, a)| *a == Action::Assign(second.clone())), "a hired member is offered for assignment");
        assert!(rows.iter().any(|(_, a)| matches!(a, Action::Recruit(_))), "the rest of the race's crew can be recruited");
    }
    /// Retail Man-O-War saves (docs/re/crew.md): helmsman Navigation 9 -> navigator factor 1.5, engineers 9/9/8 -> 1.491667,
    /// empty posts -> 1.125 (relative to an uncrewed ship: / 1.125).
    #[test]
    fn post_factors_match_retail_saves() {
        let mut posts = CrewPosts(Default::default());
        posts.0[3] = vec!["Crew_Canid_Navy_EngelbertF".into()];
        posts.0[4] = ["Crew_Navy_Gerspach", "Crew_Arc_Navy_JWhorfin", "Crew_Opto_Navy_Bretina"].map(String::from).to_vec();
        assert!((posts.post_factor(3, "navigation", 1) * 1.125 - 1.5).abs() < 1e-5);
        assert!((posts.post_factor(4, "engineering", 3) * 1.125 - 1.491667).abs() < 1e-5);
        assert!((CrewPosts(Default::default()).post_factor(4, "engineering", 3) - 1.0).abs() < 1e-6);
        assert!((skill_factor(0.7) * 1.125 - 1.425).abs() < 1e-5, "skill 7: the computer Carrack's saved factor");
    }
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
        let lib = Library::load(crate::assets_dir());
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
            .add_systems(Update, (apply_outfit, apply_skirmish_weapons, apply_skirmish_crew).chain());
        let assets = app.world().resource::<AssetServer>().clone();
        let lib = Library::load(crate::assets_dir());
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
                    crew_speed: 1.0,
                    crew_turn: 1.0,
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
        // Skirmish crews assembled in the front end become the ship's posts and handling.
        let mut crew: [Vec<String>; 8] = default();
        crew[4].push("Crew_Navy_BenH".into());
        app.world_mut().entity_mut(ship).remove::<CrewPosts>().insert(SkirmishCrew(crew));
        app.update();
        assert_eq!(app.world().get::<CrewPosts>(ship).unwrap().skill(4, "engineering"), Some(0.5));
        assert!(app.world().get::<crate::Ship>(ship).unwrap().crew_speed > 1.0 || ability("Crew_Navy_BenH", "engineering").unwrap() <= 0.1);
    }
    #[test]
    fn roster_lists_the_races_crew_with_costs_and_sorts() {
        let navy = roster("Navy", 0);
        assert!(navy.len() > 10 && navy.iter().all(|id| crew_data()[id]["race"] == "Navy"));
        assert!(navy.windows(2).all(|w| name(&w[0]) <= name(&w[1])), "sorted by name");
        let dear = roster("Navy", 1);
        assert!(dear.windows(2).all(|w| crew_points(&w[0]) >= crew_points(&w[1])), "Victory Points sort: dearest first");
        let gunners = roster("Navy", 3);
        assert!(gunners.windows(2).all(|w| ability(&w[0], "gunnery").unwrap_or(0.0) >= ability(&w[1], "gunnery").unwrap_or(0.0)));
        assert_eq!(crew_detail(&navy[0]).lines.len(), 7);
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
