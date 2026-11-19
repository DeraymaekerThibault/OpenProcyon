//! The outfitting screens model: actions, rows, details and the campaign and skirmish screen models.
use super::*;

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
    /// Seat a crew member on the selected post (a click on a roster row while a post is selected); on the first free post that counts
    /// their best skill (a double-click with no post selected); take the member on a post back to the roster (right-click on the face).
    Hire(String),
    HireAuto(String),
    Unpost(usize, usize),
    /// Arms tab: fit a weapon to the selected bank (a double-click on its row); to every bank of the same mount type (shift-click).
    FitWeapon(String),
    FitAll(String),
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
    /// Extra columns of `list`, row for row (empty when the list has none).
    pub rows: Vec<RowInfo>,
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

/// The columns of a list row: a value (a skill rating) and a price or price change, and whether the row is out of reach.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct RowInfo {
    pub value: String,
    pub price: String,
    pub grey: bool,
}

/// The info panel of the Crew and Arms screens: a name, an icon, an optional point cost and label / value lines.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Detail {
    pub title: String,
    pub icon: String,
    pub cost: Option<u32>,
    pub lines: Vec<(String, String)>,
}

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

/// Every weapon bank the Arms screen lists and the gunner slots follow, fixed ones (torpedoes, mines) included so crew can be
/// assigned to them; point defence has no slot. A fixed bank shows its gun but offers no replacement (`bank_compatible`).
pub fn weapon_banks(lib: &Library, class: &crate::data::Class) -> Vec<(usize, usize)> {
    crate::combat::banks(lib, class)
}

/// The banks whose guns can be swapped: those whose mounts are not 'Non-Selectable'.
pub fn selectable_banks(lib: &Library, class: &crate::data::Class) -> Vec<(usize, usize)> {
    crate::combat::banks(lib, class)
        .into_iter()
        .filter(|(_, first)| lib.guns.get(&class.mounts[*first].gun).and_then(|g| g.mount_type.as_deref()).is_none_or(|t| !t.contains("Non-Selectable")))
        .collect()
}

/// The selectable banks with the gun each carries: the fitted one from `fit`, else the class's own.
fn equipped_banks(lib: &Library, class: &crate::data::Class, fit: Option<&BTreeMap<usize, String>>) -> Vec<(usize, String)> {
    weapon_banks(lib, class).into_iter().map(|(bank, first)| (bank, fit.and_then(|f| f.get(&bank)).unwrap_or(&class.mounts[first].gun).clone())).collect()
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
pub(super) fn campaign_screen_model(
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
            let banks = weapon_banks(lib, class);
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
pub(super) fn crew_rows(points: &CampaignPoints, class: &crate::data::Class, role: usize, assigned: &[String]) -> Vec<(String, Action)> {
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

pub(super) fn weapon_rows(lib: &Library, class: &crate::data::Class, bank: usize, equipped: &str, available: &[String], paid: bool) -> Vec<(String, Action)> {
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

pub(super) fn skirmish_screen_model(
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
        let (mut list, mut picked, mut detail, mut rows) = (vec![], None, None, vec![]);
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
                // The price change of each weapon on the selected bank; one the points left cannot pay for is dimmed.
                let fit = slot.weapons.get(choice.ship).cloned().unwrap_or_default();
                let first = weapon_banks(lib, class).into_iter().find(|(b, _)| b == bank).map_or(0, |(_, f)| f);
                rows = guns
                    .iter()
                    .map(|g| {
                        let delta = bank_delta(lib, class, &fit, (*bank, first), g).unwrap_or(0);
                        RowInfo { value: String::new(), price: signed(delta), grey: delta > points }
                    })
                    .collect();
                list = guns.into_iter().map(|g| (gun_label(g), Action::PickWeapon(g.clone()))).collect();
            }
        } else {
            let used: Vec<&String> = slot.crew.iter().flatten().flatten().collect();
            let ids: Vec<String> = roster(&class.race, sort).into_iter().filter(|id| !used.contains(&id)).collect();
            if let Some(Carry::Crew(id)) = carry {
                picked = ids.iter().position(|x| x == id);
                detail = Some(crew_detail(id));
            }
            // Columns: the rating in the selected post's skill (else the sort's skill) and the price; dearer than the points left is greyed.
            let skill: Option<&str> = if choice.post_picked {
                Some(post_skill_name(choice.post))
            } else if sort >= 2 {
                SKILLS.iter().copied().find(|k| k.eq_ignore_ascii_case(SORTS[sort]))
            } else {
                None
            };
            rows = ids
                .iter()
                .map(|id| RowInfo {
                    value: skill.map(|s| ability(id, s).map_or_else(|| "-".to_string(), |v| format!("{:.0}", v * 10.0))).unwrap_or_default(),
                    price: crew_points(id).to_string(),
                    grey: i64::from(crew_points(id)) > points,
                })
                .collect();
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
            rows,
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
