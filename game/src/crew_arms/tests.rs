use super::fitting::{FitError, family};
use super::view::{crew_rows, weapon_rows};
use super::*;
#[test]
#[allow(deprecated)] // Interaction, see pointer.rs
fn widgets_play_rollover_once_and_select_on_press() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_message::<crate::audio::Sfx>().add_systems(Update, button_sounds);
    let tab = app.world_mut().spawn((Action::Tab, Interaction::None)).id();
    let add = app.world_mut().spawn((Action::Assign("x".into()), Interaction::None)).id();
    let mut heard = |e: Entity, i: Interaction| -> Vec<String> {
        *app.world_mut().get_mut::<Interaction>(e).unwrap() = i;
        app.update();
        app.world_mut().resource_mut::<Messages<crate::audio::Sfx>>().drain().map(|s| s.name).collect()
    };
    assert_eq!(heard(tab, Interaction::Hovered), ["FE_Button_Rollover"]);
    assert_eq!(heard(tab, Interaction::Pressed), ["FE_Button_Select"]);
    assert!(heard(tab, Interaction::Hovered).is_empty(), "releasing the press is silent");
    assert_eq!(heard(add, Interaction::Pressed), ["FE_List_Rollover", "FE_Add"]);
}
#[test]
#[ignore = "needs imported assets"]
fn a_gun_reads_its_own_gunner_slot() {
    let mut posts = CrewPosts(Default::default());
    posts.0[0] = vec!["Crew_Navy_BenH".into()];
    assert_eq!(posts.gunner_skill(Some(0)), 0.5);
    assert_eq!(posts.gunner_skill(Some(1)), 0.1, "an empty slot is a generic hand");
    assert_eq!(posts.gunner_skill(None), 0.1, "a bank without a slot is a generic hand");
}
#[test]
#[ignore = "needs imported assets"]
fn campaign_screen_without_surviving_ships_still_offers_continue() {
    let lib = Library::load(crate::assets_dir());
    let model = campaign_screen_model(&CampaignPoints::default(), &lib, &[], &mut Choice::default());
    assert!(model.open);
    assert_eq!(model.detail, "No surviving own ship.");
    assert!(model.fit.is_none());
    assert!(model.navigation.iter().any(|(_, action)| *action == Action::Close));
}

#[test]
#[ignore = "needs imported assets"]
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
#[ignore = "needs imported assets"]
fn post_factors_match_retail_saves() {
    let mut posts = CrewPosts(Default::default());
    posts.0[3] = vec!["Crew_Canid_Navy_EngelbertF".into()];
    posts.0[4] = ["Crew_Navy_Gerspach", "Crew_Arc_Navy_JWhorfin", "Crew_Opto_Navy_Bretina"].map(String::from).to_vec();
    assert!((posts.post_factor(3, "navigation", 1) * 1.125 - 1.5).abs() < 1e-5);
    assert!((posts.post_factor(4, "engineering", 3) * 1.125 - 1.491667).abs() < 1e-5);
    assert!((CrewPosts(Default::default()).post_factor(4, "engineering", 3) - 1.0).abs() < 1e-6);
    assert!((skill_factor(0.7) * 1.125 - 1.425).abs() < 1e-5, "skill 7: the computer Carrack's saved factor");
}
/// A class with no engineer post is rated on its captain: 1 + 0.05 x the captain's rating (retail Sloops: 1.05 with an empty
/// captain post, 1.30 under a rating-6 captain), scan range likewise on the lookouts.
#[test]
fn a_post_a_class_lacks_is_rated_on_its_captain() {
    let posts = CrewPosts(Default::default());
    assert!((posts.post_factor(4, "engineering", 0) * 1.125 - 1.05).abs() < 1e-6);
    assert!((posts.scan(&[0; 8]) - 1.05).abs() < 1e-6, "no lookout post, no captain: sight x 1.05");
    assert!((posts.scan(&[0, 1, 0, 1, 1, 0, 0, 1]) - 1.125).abs() < 1e-6, "an empty lookout post: sight x 1.125");
}
#[test]
#[ignore = "needs imported assets"]
fn a_rated_captain_sets_the_factor_of_a_missing_post() {
    let mut posts = CrewPosts(Default::default());
    posts.0[1] = vec!["Crew_Navy_JimHawkins".into()]; // engineering 2
    assert!((posts.post_factor(4, "engineering", 0) * 1.125 - 1.10).abs() < 1e-5);
}
#[test]
#[ignore = "needs imported assets"]
fn the_first_mate_stands_in_for_a_missing_helm_and_the_captain_for_missing_engineers() {
    let mut posts = CrewPosts(Default::default());
    posts.0[1] = vec!["Crew_Navy_JimHawkins".into()]; // navigation 7, engineering 2
    posts.0[2] = vec!["Crew_Navy_Gerspach".into()]; // navigation 8
    assert!((posts.post_factor(3, "navigation", 0) * 1.125 - 1.40).abs() < 1e-5);
    assert!((posts.post_factor(4, "engineering", 0) * 1.125 - 1.10).abs() < 1e-5);
}
#[test]
#[ignore = "needs imported assets"]
fn a_leadership_ten_captain_always_lifts_the_crew_and_no_officers_never_do() {
    let mut posts = CrewPosts(Default::default());
    for i in 0..50 {
        assert_eq!(leadership_bonus(crate::scenario::SpawnIndex(i), &posts), 0.0);
    }
    posts.0[1] = vec!["Crew_Navy_Amelia".into()]; // leadership 10
    for i in 0..50 {
        assert!(leadership_bonus(crate::scenario::SpawnIndex(i), &posts) >= 0.1);
    }
}
#[test]
#[ignore = "needs imported assets"]
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
#[ignore = "needs imported assets"]
fn fixed_banks_are_listed_for_crew_but_cannot_be_swapped() {
    let lib = Library::load(crate::assets_dir());
    let class = &lib.classes[lib.index("Ship_Navy_Torpedo")];
    let all = weapon_banks(&lib, class);
    let swappable = selectable_banks(&lib, class);
    assert!(all.len() > swappable.len(), "the torpedo tubes are listed too");
    let (bank, first) = *all.iter().find(|b| !swappable.contains(b)).unwrap();
    assert!(!weapon_rows(&lib, class, bank, &class.mounts[first].gun, &[], true).iter().any(|_| true), "nothing replaces a fixed bank");
}

#[test]
#[ignore = "needs imported assets"]
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
/// A mod gun on a non-selectable mount is built into its ship's hull: no retail ship can fit it.
#[test]
#[ignore = "needs imported assets"]
fn a_built_in_mod_gun_fits_no_other_ship() {
    let mut lib = Library::load(crate::assets_dir());
    let gun = serde_json::json!({
        "MuzzleSpeed": 5000.0, "TimeBetweenBursts": 9.0, "BulletsPerBurst": 1, "DuringBurstReloadTime": 0.0,
        "MaximumRange_Range": 380.0, "LongRange_Range": 280.0, "EffectiveRange_Range": 150.0, "BulletAffectedByGravity": false,
        "HorizontalRotationSpeed": 0.35, "VerticalMinAngle": -0.12, "VerticalMaxAngle": 0.3, "VerticalRotationSpeed": 0.5,
        "MuzzleFlashEffect": "PlasmaCannon_Heavy_Muzzle", "MaximumRange_AccuracyDeviation": 0.01, "LongRange_AccuracyDeviation": 0.006,
        "EffectiveRange_AccuracyDeviation": 0.003, "BulletTypeName_Primary": "Bullet_Mod_Beam", "Is PointDefense Gun": false,
        "Is TorpedoLauncher Gun": false, "SoundName": "Weapon_Mod_Beam", "SoundVolume": 1.0, "SoundMaxDistance": 700.0, "LobAngle": 0.0,
        "Is Lobbing Gun": false, "Can Be Fired While Cloaked": false, "Victory Point Cost": 16, "Exclusion Race": "None",
        "Mount Type": "Non-Selectable Heavy Mount", "EffectiveRange_RicochetFactor": -100.0, "LongRange_RicochetFactor": -100.0,
        "MaximumRange_RicochetFactor": -100.0,
        "beam": {"charge": 2.0, "duration": 3.0, "tick": 0.25, "ramp": 0.6, "max_power": 2.8, "width": 0.32, "color": [0.45, 1.6, 2.2]},
        "visual": {"pivot": [0.0, 0.0, 0.4], "muzzles": [[0.0, 4.4, 0.4]]}
    });
    lib.guns.insert("Gun_Mod_Beam".into(), crate::data::gun_def(&gun));
    for class in &lib.classes {
        for (bank, _) in weapon_banks(&lib, class) {
            assert!(!bank_compatible(&lib, class, bank, "Gun_Mod_Beam"), "{} bank {bank}", class.name);
        }
    }
    let beam = &lib.guns["Gun_Mod_Beam"];
    assert!(beam.mount_type.as_deref().is_some_and(|t| t.contains("Non-Selectable") && t.contains("Heavy")), "fixed, and still counted as a heavy mount");
}
#[test]
#[ignore = "needs imported assets"]
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
    let ship =
        app.world_mut().spawn((crate::Ship { heading: 0.0, ..crate::Ship::new(class, crate::Side::Player) }, crate::combat::Health::new(&lib.classes[class]), ms, boardable)).id();
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
#[ignore = "needs imported assets"]
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
#[ignore = "needs imported assets"]
fn best_gunner_sets_the_legacy_crewed_accuracy() {
    let mut p = CampaignPoints::default();
    p.crew = vec!["Crew_Navy_BenH".into()];
    assert_eq!(best(&p, "gunnery"), Some(0.5));
    assert!((accuracy(Some(&p)) - 0.6).abs() < 1e-6);
    assert_eq!(accuracy(Some(&CampaignPoints::default())), 0.5);
}

#[test]
fn selecting_a_post_sorts_the_roster_by_its_skill() {
    let want = ["Gunnery", "Leadership", "Leadership", "Navigation", "Engineering", "Rigging", "Combat", "Spotting"];
    for (role, skill) in want.iter().enumerate() {
        assert_eq!(SORTS[sort_for_post(role)], *skill, "{}", ROLES[role]);
    }
}

#[test]
#[ignore = "needs imported assets"]
fn a_roster_click_seats_the_member_within_the_points_and_a_right_click_returns_them() {
    let navy = roster("Navy", 3); // gunnery first
    let id = navy[0].clone();
    let price = i64::from(crew_points(&id));
    let mut crew: [Vec<String>; 8] = Default::default();
    let broke = place_crew(&mut crew, "Ship_Navy_Frigate", id.clone(), 0, None, price - 1);
    assert_eq!(broke, "You do not have enough victory points.");
    assert!(crew[0].is_empty(), "a refused hire seats nobody");
    assert_eq!(place_crew(&mut crew, "Ship_Navy_Frigate", id.clone(), 0, None, price), "");
    assert_eq!(crew[0].len(), 1);
    assert_eq!(crew[0][0], id);
    assert_eq!(unpost(&mut crew, 0, 0), Some(id));
    assert!(crew[0].is_empty());
    assert_eq!(unpost(&mut crew, 0, 0), None, "nothing on that seat");
}

#[test]
#[ignore = "needs imported assets"]
fn a_double_click_seats_the_member_on_the_first_free_post_for_their_best_skill() {
    let id = roster("Navy", 3)[0].clone();
    let mut crew: [Vec<String>; 8] = Default::default();
    let post = auto_post(&crew, "Ship_Navy_Frigate", &id).expect("a free post");
    let top = SKILLS.iter().filter_map(|k| ability(&id, k)).fold(0.0, f32::max);
    assert_eq!(ability(&id, SKILLS[post]), Some(top), "the post rates their best skill");
    // Once every seat of that skill is taken, no post suits.
    let caps = crate::boarding::counts("Ship_Navy_Frigate");
    for r in (0..8).filter(|&r| SKILLS[r] == SKILLS[post]) {
        crew[r] = vec!["x".into(); caps[r] as usize];
    }
    assert_eq!(auto_post(&crew, "Ship_Navy_Frigate", &id), None);
}

#[test]
fn price_changes_read_signed() {
    assert_eq!((signed(8), signed(-8), signed(0)), ("+8".to_string(), "-8".to_string(), "0".to_string()));
}

/// A class with a gun that fits at least two of its banks, and that gun.
#[cfg(test)]
fn class_with_twin_banks(lib: &Library) -> (usize, String) {
    for (ci, class) in lib.classes.iter().enumerate() {
        for gun in lib.guns.keys() {
            if banks_for(lib, class, gun).len() >= 2 && class.mounts.iter().all(|m| lib.guns.contains_key(&m.gun)) {
                return (ci, gun.clone());
            }
        }
    }
    panic!("no class has two banks of one mount type");
}

#[test]
#[ignore = "needs imported assets"]
fn fitting_every_bank_of_a_mount_type_is_all_or_nothing_against_the_points() {
    let lib = Library::load(crate::assets_dir());
    let (ci, gun) = class_with_twin_banks(&lib);
    let class = &lib.classes[ci];
    let targets = banks_for(&lib, class, &gun);
    let need: i64 = targets.iter().map(|&t| bank_delta(&lib, class, &BTreeMap::new(), t, &gun).unwrap()).sum();
    let mut fit = BTreeMap::new();
    assert_eq!(fit_guns(&lib, class, &mut fit, &targets, &gun, need - 1), Err(FitError::NotEnough));
    assert!(fit.is_empty(), "a refused fit changes no bank");
    assert_eq!(fit_guns(&lib, class, &mut fit, &targets, &gun, need), Ok(need));
    assert!(targets.iter().all(|(b, _)| fit.get(b) == Some(&gun)), "every bank got the gun");
    // Fitting it again changes nothing and costs nothing.
    assert_eq!(fit_guns(&lib, class, &mut fit, &targets, &gun, 0), Ok(0));
    assert_eq!(fit_guns(&lib, class, &mut fit, &[], &gun, 0), Err(FitError::WrongMount), "no bank takes it");
}

#[test]
#[ignore = "needs imported assets"]
fn the_price_change_is_the_difference_times_the_guns_in_the_bank() {
    let lib = Library::load(crate::assets_dir());
    let (ci, gun) = class_with_twin_banks(&lib);
    let class = &lib.classes[ci];
    let target = banks_for(&lib, class, &gun)[0];
    let stock = &class.mounts[target.1].gun;
    let guns = class.mounts.iter().filter(|m| m.bank == target.0).count() as i64;
    let (a, b) = (i64::from(lib.guns[&gun].points.unwrap_or(0)), i64::from(lib.guns[stock].points.unwrap_or(0)));
    assert_eq!(bank_delta(&lib, class, &BTreeMap::new(), target, &gun), Some((a - b) * guns));
    assert_eq!(bank_delta(&lib, class, &BTreeMap::new(), target, stock), Some(0));
}
#[test]
fn navigation_actions_move_only_the_choice() {
    let mut c = Choice { ship: 1, post: 3, page: 2, post_picked: true, message: "x".into(), ..default() };
    assert!(navigate(&mut c, &Action::Ship(1), 3, 0));
    assert_eq!((c.ship, c.post, c.page, c.post_picked), (2, 0, 0, false));
    assert!(navigate(&mut c, &Action::Ship(1), 3, 0), "the ship list wraps");
    assert_eq!(c.ship, 0);
    assert!(navigate(&mut c, &Action::Post(-1), 3, 4));
    assert_eq!(c.post, 3, "the post list wraps backwards");
    assert!(navigate(&mut c, &Action::Page(-5), 3, 4));
    assert_eq!(c.page, 0, "a page never goes below the first");
    assert!(navigate(&mut c, &Action::Tab, 3, 4));
    assert!(c.arms && c.message.is_empty());
    assert!(navigate(&mut c, &Action::SelectShip(9), 3, 4));
    assert_eq!(c.ship, 2, "a picked ship is clamped to the fleet");
    assert!(!navigate(&mut c, &Action::Sort, 3, 4), "other actions are not navigation");
}
