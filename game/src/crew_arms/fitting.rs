//! Guns and banks: fits, prices, compatibility and applying an outfit to ships.
use super::*;

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
    pub(super) fn location(&self, id: &str) -> Option<(&str, usize)> {
        self.ships.iter().find_map(|(key, fit)| fit.crew.iter().position(|r| r.iter().any(|c| c == id)).map(|r| (key.as_str(), r)))
    }
    pub(super) fn assign(&mut self, key: &str, role: usize, capacity: u32, id: &str) -> Result<(), String> {
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

/// Campaign: Jim Hawkins captains the command ship and Mister Onyx is its first mate, always (posts by `ROLES` index). Neither serves
/// on another ship, and a ship that stops being the command ship goes back to its default captain and first mate; other posts and
/// guns carry over between missions. From play (the retail Crew & Arms screen shows the two fixed on the command ship).
pub const COMMAND_CREW: [(usize, &str); 2] = [(1, "Crew_Navy_JimHawkins"), (2, "Crew_Navy_Onyx")];

impl Outfit {
    /// This outfit with the command crew on the ship `command` names (`ship_key`), alone at their posts, and on no other ship.
    pub fn pinned(&self, command: Option<&str>) -> Outfit {
        let mut out = self.clone();
        for fit in out.ships.values_mut() {
            for slot in &mut fit.crew {
                slot.retain(|c| !is_command_crew(c));
            }
        }
        let Some(key) = command else { return out };
        for (role, id) in COMMAND_CREW {
            out.ships.entry(key.into()).or_default().crew[role] = vec![id.into()];
            if !out.hired.iter().any(|c| c == id) {
                out.hired.push(id.into());
            }
        }
        out
    }
}

/// Whether `id` is one of the command ship's fixed crew.
pub fn is_command_crew(id: &str) -> bool {
    COMMAND_CREW.iter().any(|(_, c)| *c == id)
}

/// Ship identity, not ECS IDs: survives mission restart/save, without applying one hull's mounts to another hull.
pub(super) fn ship_key(class: &crate::data::Class, mission: &crate::campaign::MissionShip) -> String {
    format!("{}:{}", class.name, if mission.name.is_empty() { mission.id.to_string() } else { mission.name.clone() })
}

/// Why a weapon fit was refused.
#[derive(Debug, PartialEq)]
pub(super) enum FitError {
    WrongMount,
    NotEnough,
}

/// Price change of fitting `gun` on `bank` (`first` is its first mount) over what it carries now (`fit`, else the stock gun): the price
/// difference times the guns in the bank. `None` when the gun does not fit the bank.
pub(super) fn bank_delta(lib: &Library, class: &crate::data::Class, fit: &BTreeMap<usize, String>, (bank, first): (usize, usize), gun: &str) -> Option<i64> {
    let old = fit.get(&bank).unwrap_or(&class.mounts[first].gun);
    Some(i64::from(bank_cost(lib, class, bank, gun)?) - i64::from(bank_cost(lib, class, bank, old).unwrap_or(0)))
}

/// Fits `gun` to every bank in `targets` ((bank, first mount) pairs), all or nothing against `points`: the summed price change is
/// charged at once, or nothing changes. Returns the change.
pub(super) fn fit_guns(lib: &Library, class: &crate::data::Class, fit: &mut BTreeMap<usize, String>, targets: &[(usize, usize)], gun: &str, points: i64) -> Result<i64, FitError> {
    let deltas: Option<Vec<i64>> = targets.iter().map(|&t| bank_delta(lib, class, fit, t, gun)).collect();
    let deltas = deltas.filter(|d| !d.is_empty()).ok_or(FitError::WrongMount)?;
    let total: i64 = deltas.iter().sum();
    if total > points {
        return Err(FitError::NotEnough);
    }
    for &(bank, _) in targets {
        fit.insert(bank, gun.to_string());
    }
    Ok(total)
}

/// The banks of the ship `gun` can be fitted to (the same mount type): what shift-click fits at once.
pub(super) fn banks_for(lib: &Library, class: &crate::data::Class, gun: &str) -> Vec<(usize, usize)> {
    weapon_banks(lib, class).into_iter().filter(|(bank, _)| bank_compatible(lib, class, *bank, gun)).collect()
}

/// A price change as the lists show it: `+8`, `-8`, `0`.
pub(super) fn signed(d: i64) -> String {
    if d == 0 { "0".into() } else { format!("{d:+}") }
}

/// The Arms screen's message for a fit: retail's 'Total Cost: <change>' block, or why it was refused.
pub(super) fn fit_message(said: &dyn Fn(&str) -> String, result: Result<i64, FitError>) -> String {
    match result {
        Err(FitError::WrongMount) => said("IDGS_TPFRONTENDTEXT02_WEAPON_WRONG_MOUNT"),
        Err(FitError::NotEnough) => said("IDGS_TPFRONTENDTEXT02_WEAPON_NOT_ENOUGH_VICTORY_POINTS"),
        Ok(delta) => format!("{} {}\n{}", said("IDGS_TPFRONTENDTEXT02_TOTAL_COST"), delta, said("IDGS_TPFRONTENDTEXT02_WEAPON_UPGRADE_SUCCESSFUL")),
    }
}

pub(super) fn family(gun: &str) -> &str {
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
pub(super) fn upgrade(points: &mut CampaignPoints, lib: &Library, class: &crate::data::Class, key: &str, bank: usize, gun: &str) -> Result<(), String> {
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

/// Skirmish equipment belongs to a fleet entry, not the campaign's currency or a shared hull class.
#[derive(Component)]
pub struct WeaponFit(pub BTreeMap<usize, String>);

/// What the cursor carries between a list row and a post or mount: a click on a row picks it, a release or a click on a post places it.
#[derive(Clone, PartialEq)]
pub(super) enum Carry {
    Crew(String),
    Gun(String),
}

/// The named crew a skirmish ship was assembled with; `apply_skirmish_crew` turns it into posts, handling and boarding crews.
#[derive(Component, Clone)]
pub struct SkirmishCrew(pub [Vec<String>; 8]);

pub(super) fn apply_skirmish_crew(
    spawn_order: Query<&crate::scenario::SpawnIndex>,
    mut commands: Commands,
    lib: Res<Library>,
    mut ships: Query<(Entity, &mut crate::Ship, &SkirmishCrew, &mut crate::boarding::Boardable), Added<SkirmishCrew>>,
) {
    for (e, mut ship, crew, mut boarding) in &mut ships {
        let caps = crate::boarding::counts(&lib.classes[ship.class].name);
        let posts = CrewPosts(std::array::from_fn(|r| crew.0[r].iter().take(caps[r] as usize).cloned().collect()));
        (ship.crew_speed, ship.crew_turn) = posts.handling(&caps);
        // Sight follows the lookouts, for every ship (retail: scan range = sight x the lookout post's factor, ours included).
        commands.entity(e).insert(crate::environment::ScanFactor(posts.scan(&caps)));
        boarding.apply_posts(&posts, leadership_bonus(crate::scenario::SpawnIndex::of(&spawn_order, e), &posts));
        commands.entity(e).insert(posts);
    }
}

pub(super) fn apply_skirmish_weapons(
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

pub(super) fn apply_outfit(
    spawn_order: Query<&crate::scenario::SpawnIndex>,
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
    command: Query<(), With<crate::commands::Commander>>,
    new_command: Query<(), Added<crate::commands::Commander>>,
) {
    let Some(mission) = mission else { return };
    // The outfit only changes with the saved points, the mission or the command ship; otherwise only ships that just spawned need fitting.
    let dirty = points.is_changed() || mission.is_changed() || !new_command.is_empty();
    if !dirty && !ships.iter().any(|(.., ms, _, _, _)| ms.is_added()) {
        return;
    }
    let command_key = ships
        .iter()
        .find(|(e, _, h, ms, ..)| command.contains(*e) && !h.destroyed && ms.owner == mission.script.player())
        .map(|(_, ship, _, ms, ..)| ship_key(&lib.classes[ship.class], &ms));
    let outfit = points.outfit.pinned(command_key.as_deref());
    for (e, mut ship, health, ms, mut guns, mut boarding, posts) in &mut ships {
        if health.destroyed || !(dirty || ms.is_added()) {
            continue;
        }
        let class = &lib.classes[ship.class];
        let fit = (ms.owner == mission.script.player()).then(|| outfit.ships.get(&ship_key(class, &ms))).flatten();
        let wanted = fit.map(|fit| {
            let mut crew: [Vec<String>; 8] = default();
            let mut used = std::collections::HashSet::new();
            for (role, slot) in crew.iter_mut().enumerate() {
                *slot = fit.crew[role]
                    .iter()
                    .filter(|id| outfit.hired.contains(id) && crew_data()[id.as_str()].is_object() && used.insert((*id).clone()))
                    .take(crate::boarding::counts(&class.name)[role] as usize)
                    .cloned()
                    .collect();
            }
            CrewPosts(crew)
        });
        if posts != wanted.as_ref() {
            (ship.crew_speed, ship.crew_turn) = wanted.as_ref().map_or((1.0, 1.0), |p| p.handling(&crate::boarding::counts(&class.name)));
            if ms.owner == mission.script.player() {
                // The player's ships see as far as their lookouts allow (an empty post still gives 1.125).
                let caps = crate::boarding::counts(&class.name);
                commands.entity(e).insert(crate::environment::ScanFactor(wanted.as_ref().unwrap_or(&CrewPosts(Default::default())).scan(&caps)));
            }
            if let Some(posts) = &wanted {
                boarding.apply_posts(posts, leadership_bonus(crate::scenario::SpawnIndex::of(&spawn_order, e), posts));
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
