use super::*;

#[test]
fn difficulty_toggle_cycles_normal_hard_easy() {
    let mut s = Settings::default();
    let mut seen = vec![s.difficulty];
    for _ in 0..3 {
        s.step(Setting::Difficulty);
        seen.push(s.difficulty);
    }
    assert_eq!(seen, [1, 2, 0, 1]);
}

#[test]
#[ignore = "needs imported assets"]
fn auto_assign_fills_the_fleet_within_the_points() {
    let lib = Library::load(assets_dir());
    let mut sk = skirmish("red_rover");
    sk.slots[0].race = 0;
    sk.fleet_size = 5;
    apply(&mut sk, &lib, &Action::AutoAssign);
    let fleet = &sk.human().unwrap().fleet;
    assert!(!fleet.is_empty() && fleet.len() <= 5);
    assert!(sk.points_left(&lib) >= 0, "never overspends");
    let navy = offered(&lib, 0);
    assert_eq!(lib.classes[navy[0]].name, "Ship_Navy_StarHammer", "the original lists the StarHammer first");
}

#[test]
#[ignore = "needs imported assets"]
fn frigate_stats_match_the_original_fleet_screen() {
    let lib = Library::load(assets_dir());
    let st = ship_stats(&lib, lib.index("Ship_Navy_Frigate"));
    assert_eq!(st, ShipStats { hit_points: 3150, max_speed_kmh: 21, turning: "IDGS_TPFRONTENDTEXT_TURNINGRATE_AVERAGE", crew: 23, heavy: 6, medium: 6, light: 9, points: 131 });
}

#[test]
#[ignore = "needs imported assets"]
fn fleet_spends_victory_points() {
    let lib = Library::load(assets_dir());
    let mut sk = skirmish("red_rover");
    sk.slots[0].race = 0;
    sk.ship_pick = offered(&lib, 0).iter().position(|&c| lib.classes[c].name == "Ship_Navy_Frigate").unwrap();
    sk.fleet_size = 5;
    assert_eq!(sk.points_left(&lib), 300);
    apply(&mut sk, &lib, &Action::AddShip);
    assert_eq!(sk.points_left(&lib), 169, "the original shows 169 after adding a frigate");
    apply(&mut sk, &lib, &Action::AddShip);
    apply(&mut sk, &lib, &Action::AddShip);
    assert_eq!(sk.human().unwrap().fleet.len(), 2, "a third frigate would overspend 300 points");
}

#[test]
#[ignore = "needs imported assets"]
fn map_info_opens_and_closes_the_lobby_dialog() {
    let lib = Library::load(assets_dir());
    let mut sk = skirmish("ambush");
    apply(&mut sk, &lib, &Action::MapInfo(true));
    assert!(sk.map_info);
    apply(&mut sk, &lib, &Action::MapInfo(false));
    assert!(!sk.map_info);
}

#[test]
#[ignore = "needs imported assets"]
fn skirmish_list_counts_playable_players_like_the_original() {
    let maps = skirmish_maps();
    let get = |id: &str| maps.iter().find(|m| m.0 == id).unwrap_or_else(|| panic!("{id}"));
    // Original 'Historical Maps' list: The Ambush 4, Bayle's Stand 2, Iron War 3, Locusts 4, Storm Front 2, The Maw 3.
    for (id, n) in [("ambush", 4), ("bayles_stand", 2), ("iron_war", 3), ("locusts", 4), ("storm_front", 2), ("the_maw", 3)] {
        assert_eq!((get(id).2, get(id).3), (false, n), "{id}");
    }
}

#[test]
fn menu_widgets_play_rollover_once_and_their_own_select_sound() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_message::<crate::audio::Sfx>().add_systems(Update, button_sounds);
    let row = app.world_mut().spawn((Action::PickMap(0), Interaction::None)).id();
    let next = app.world_mut().spawn((Action::Goto(Screen::Lobby), ArrowButton { forward: true }, Interaction::None)).id();
    let heard = |app: &mut App, e: Entity, i: Interaction| -> Vec<String> {
        *app.world_mut().get_mut::<Interaction>(e).unwrap() = i;
        app.update();
        app.world_mut().resource_mut::<Messages<crate::audio::Sfx>>().drain().map(|s| s.name).collect()
    };
    assert_eq!(heard(&mut app, row, Interaction::Hovered), ["FE_List_Rollover"]);
    assert_eq!(heard(&mut app, row, Interaction::Pressed), ["FE_List_Select"]);
    assert!(heard(&mut app, row, Interaction::Hovered).is_empty(), "releasing the press is silent");
    assert_eq!(heard(&mut app, next, Interaction::Pressed), ["FE_Button_Rollover", "FE_Button_Forward_Select"]);
}

#[test]
#[ignore = "needs imported assets"]
fn every_menu_sound_is_a_retail_file() {
    let back = ArrowButton { forward: false };
    let actions = [Action::Quit, Action::PickMap(0), Action::FleetSize(1), Action::Launch, Action::AddShip, Action::DropShip];
    for a in &actions {
        for (arrow, select) in [(None, false), (None, true), (Some(&back), true)] {
            let name = button_sound(a, arrow, select);
            assert!(crate::assets_dir().join(crate::audio::sound(name)).is_file(), "{name}");
        }
    }
}

fn map(id: &str) -> Value {
    crate::data::read_json(&assets_dir().join(format!("maps/{id}/map.json")))
}
fn skirmish(id: &str) -> Skirmish {
    let mut sk = Skirmish {
        map: None,
        slots: vec![],
        maps: vec![(id.into(), id.into(), false, 0)],
        selected: 0,
        historical: false,
        fleet_size: 5,
        victory_limit: 300,
        map_json: Value::Null,
        error: None,
        dropdown: None,
        name_open: false,
        flag_picker: false,
        ship_pick: 0,
        ship_chosen: false,
        view_slot: false,
        fleet_pick: 0,
        slot_scroll: 0,
        map_scroll: 0,
        scroll: 0,
        arms_open: false,
        arms_tab: false,
        map_info: false,
        profile: None,
    };
    sk.load_map();
    sk
}
#[test]
#[ignore = "needs imported assets"]
fn skirmish_ship_names_default_to_the_pool_and_follow_the_pull_down() {
    let lib = Library::load(assets_dir());
    let mut sk = skirmish("diablo_straight");
    sk.fleet_size = 2;
    sk.choose(Cell::Player(1), Choice::Computer);
    let human = sk.slots.iter().position(|s| s.kind == Kind::Human).unwrap();
    sk.ship_pick = offered(&lib, 0).iter().position(|&i| lib.classes[i].name == "Ship_Navy_Cutter").unwrap();
    apply(&mut sk, &lib, &Action::AddShip);
    apply(&mut sk, &lib, &Action::AddShip);
    let pool = lib.names["Ship_Navy_Cutter"].clone();
    let names = |sk: &Skirmish| sk.slots[human].ship_names(&lib);
    assert_eq!(names(&sk), vec![Some(pool[0].clone()), Some(pool[1].clone())], "same-hull ships get different default names");
    assert!(!sk.slots[human].name_choices(&lib, 0).contains(&pool[0]), "the pull-down omits the name the ship bears");
    sk.fleet_pick = 1;
    apply(&mut sk, &lib, &Action::NameList(true));
    assert!(sk.name_open);
    apply(&mut sk, &lib, &Action::PickName(pool[5].clone()));
    assert!(!sk.name_open);
    assert_eq!(names(&sk), vec![Some(pool[0].clone()), Some(pool[5].clone())]);
    let plan = sk.plan(&lib);
    let human_group = plan.iter().find(|g| g.human).unwrap();
    assert_eq!(human_group.ships[1].2.as_deref(), Some(pool[5].as_str()), "the chosen name reaches the battle");
    apply(&mut sk, &lib, &Action::DropShip);
    assert_eq!(sk.slots[human].names.len(), 1, "dropping a ship drops its name");
}
#[test]
#[ignore = "needs imported assets"]
fn border_dispute_keeps_the_humans_command_ship() {
    use crate::*;
    use bevy::ecs::system::RunSystemOnce;
    let lib = Library::load(assets_dir());
    let mut sk = skirmish("border_dispute");
    let other = sk.slots.iter().position(|s| s.kind != Kind::Human).unwrap();
    sk.choose(Cell::Player(other), Choice::Computer);
    if sk.open() {
        sk.ship_pick = 0;
        apply(&mut sk, &lib, &Action::AddShip);
    }
    let plan = sk.plan(&lib);
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .init_asset::<AudioSource>()
        .init_asset::<crate::fx::EffectMaterial>()
        .insert_resource(Library::load(crate::assets_dir()))
        .init_resource::<fx::FxLib>()
        .init_resource::<collision::Shapes>()
        .init_resource::<data::AsteroidDefs>()
        .init_resource::<ship::Fleets>()
        .init_resource::<Target>()
        .insert_resource(Orbit { yaw: 0.0, pitch: 0.0, dist: 60.0, focus: None })
        .insert_resource(Config {
            groups: plan,
            player: String::new(),
            allies: vec![],
            enemies: vec![],
            map: Some("border_dispute".into()),
            shot: None,
            frame: 0,
            speed: 1.0,
            ended: None,
            script: None,
            autolaunch: false,
            shot_frame: 0,
            late_frame: 0,
        });
    app.world_mut()
        .run_system_once(|mut c: Commands, a: Res<AssetServer>, l: Res<Library>, f: Res<fx::FxLib>, s: Res<collision::Shapes>, r: Res<data::AsteroidDefs>, cfg: Res<Config>| {
            crate::scenario::spawn_scenario(&mut c, &a, &l, &f, &s, &r, &cfg)
        })
        .unwrap();
    let w = app.world_mut();
    let n = w.query_filtered::<&Ship, With<PlayerShip>>().iter(w).count();
    assert_eq!(n, 1, "the human's command ship must stay the player's ship once the scenario has been reconciled");
}
#[test]
#[ignore = "needs imported assets"]
fn skirmish_weapon_choices_follow_fleet_entries_and_survive_launch_planning() {
    let lib = Library::load(assets_dir());
    let mut sk = skirmish("diablo_straight");
    sk.fleet_size = 3;
    sk.choose(Cell::Player(1), Choice::Computer);
    let human = sk.slots.iter().position(|s| s.kind == Kind::Human).unwrap();
    sk.ship_pick = offered(&lib, 0).iter().position(|&i| lib.classes[i].name == "Ship_Navy_Cutter").unwrap();
    apply(&mut sk, &lib, &Action::AddShip);
    apply(&mut sk, &lib, &Action::AddShip);
    let class = &lib.classes[lib.index("Ship_Navy_Cutter")];
    let bank = class.mounts.iter().find(|m| m.gun == "Gun_CannonBall_Med").unwrap().bank;
    let before = sk.points_left(&lib);
    sk.slots[human].weapons[0].insert(bank, "Gun_Carronade_Med".into());
    let mounts = class.mounts.iter().filter(|m| m.bank == bank).count() as i64;
    let price = |g: &str| i64::from(lib.guns[g].points.unwrap());
    assert_eq!(before - sk.points_left(&lib), (price("Gun_Carronade_Med") - price("Gun_CannonBall_Med")) * mounts, "a refit costs the price difference per gun");
    let plan = sk.plan(&lib);
    let group = plan.iter().find(|g| g.human).unwrap();
    assert_eq!(group.ships[0].0, group.ships[1].0);
    assert_eq!(group.weapons[0][&bank], "Gun_Carronade_Med");
    assert!(group.weapons[1].is_empty(), "duplicate hulls must not share fittings");
    sk.fleet_pick = 0;
    apply(&mut sk, &lib, &Action::OpenArms);
    assert!(sk.arms_open);
    apply(&mut sk, &lib, &Action::DropShip);
    assert_eq!(sk.slots[human].weapons.len(), 1);
    assert!(sk.slots[human].weapons[0].is_empty(), "dropping a ship also drops its fitting");
    sk.slots[human].weapons[0].insert(bank, "Gun_GravCharge_Med".into());
    sk.choose(Cell::Fleet(human), Choice::Fleet(2));
    assert!(sk.slots[human].fleet.is_empty());
    assert!(sk.slots[human].weapons.is_empty());
}

#[test]
#[ignore = "needs imported assets"]
fn historical_map_commands_the_ship_its_rules_mark_primary() {
    // Iron War lists Navy 1's two torpedo boats first, but its Setup Ship rule makes the fast frigate the primary.
    let lib = Library::load(assets_dir());
    let navy = &skirmish("iron_war").plan(&lib)[0].ships;
    assert_eq!((navy[0].0.as_str(), navy[0].2.as_deref()), ("Ship_Navy_FastFrigate", Some("RLS Star Finder")));
    assert_eq!(navy.len(), 3);
}

#[test]
#[ignore = "needs imported assets"]
fn historical_map_keeps_preplaced_fleets_and_fixed_fleet_types() {
    let slots = new_slots(&map("ambush"));
    assert_eq!(slots.iter().map(|s| (s.race, s.team)).collect::<Vec<_>>(), [(0, 0), (2, 1), (2, 1), (2, 1)]);
    assert!(slots.iter().all(|s| !s.fleet.is_empty() && s.kind != Kind::Closed));
    assert_eq!(slots[0].kind, Kind::Human);
    // Taking a pirate slot makes the other pirates allies and the Navy hostile.
    let lib = Library::load(assets_dir());
    let mut sk = skirmish("ambush");
    let navy = &sk.plan(&lib)[0].ships;
    assert_eq!((navy[0].0.as_str(), navy[0].2.as_deref()), ("Ship_Navy_ManOWar", Some("RLS Indefatigable")), "historical ships keep their set names");
    sk.slots[0].kind = Kind::Computer;
    sk.slots[2].kind = Kind::Human;
    let sides: Vec<_> = sk.plan(&lib).iter().map(|g| (g.player, g.side, g.human, !g.ships.is_empty() && g.ships.iter().all(|s| s.1.is_some()))).collect();
    assert_eq!(sides, [(0, crate::Side::Enemy, false, true), (1, crate::Side::Player, false, true), (2, crate::Side::Player, true, true), (3, crate::Side::Player, false, true)]);
}
#[test]
#[ignore = "needs imported assets"]
fn three_team_historical_map_is_a_three_way_battle() {
    // The Maw: Navy, Procyon and Pirates are three teams; the two computer fleets are hostile to each other as well.
    let lib = Library::load(assets_dir());
    let sides: Vec<_> = skirmish("the_maw").plan(&lib).iter().map(|g| g.side).collect();
    assert_eq!(sides, [crate::Side::Player, crate::Side::Enemy, crate::Side(2)]);
}
#[test]
#[ignore = "needs imported assets"]
fn open_map_locks_fleet_type_to_team_and_needs_a_fleet() {
    let mut sk = skirmish("red_rover");
    assert_eq!((sk.slots[0].race, sk.slots[1].race), (0, 2));
    assert!(sk.slots.iter().skip(1).all(|s| s.kind == Kind::Open), "retail: you alone, every other slot Open");
    assert_eq!(sk.launch_error(), Some("No opponent: open another slot on a different team."));
    sk.choose(Cell::Player(1), Choice::Computer);
    assert_eq!(sk.launch_error(), Some("IDGS_TPFRONTENDTEXT_SCREENS_ERROR_FLEET"));
    assert!(sk.choices(Cell::Fleet(1)).is_empty() && sk.choices(Cell::Team(0)).is_empty(), "team maps lock fleet type and team");
    sk.slots[0].fleet.push("Ship_Navy_Cutter".into());
    assert_eq!(sk.launch_error(), None);
    let lib = Library::load(assets_dir());
    let plan = sk.plan(&lib);
    assert_eq!(plan.len(), 2);
    assert_eq!(plan[1].side, crate::Side::Enemy);
    assert!(!plan[1].ships.is_empty() && plan[1].ships.iter().all(|s| s.0.contains("Pirate")), "computer fleet is built from its fleet type");
    // Free-for-all map: you choose the fleet type yourself.
    let mut sk = skirmish("diablo_straight");
    assert_eq!((sk.slots[0].race, sk.slots[0].team, sk.slots[1].kind), (0, 0, Kind::Open), "retail: you are Navy, Team 1");
    sk.choose(Cell::Player(1), Choice::Computer);
    assert_eq!((sk.slots[1].race, sk.slots[1].team), (2, 1), "a new computer: Pirate, Team 2");
    assert_eq!(sk.choices(Cell::Team(1))[..2], [Choice::Team(1), Choice::Team(0)], "current team first");
    assert_eq!(sk.choices(Cell::Fleet(0)), [Choice::Fleet(0), Choice::Fleet(2), Choice::Fleet(3)]);
    sk.choose(Cell::Player(1), Choice::Close);
    assert_eq!((sk.slots[1].kind, sk.slots[1].race, sk.slots[1].team), (Kind::Closed, 4, -1));
}
#[test]
#[ignore = "needs imported assets"]
fn lobby_actions_edit_slots_and_fleet() {
    let lib = Library::load(assets_dir());
    let mut sk = skirmish("diablo_straight"); // free-for-all
    sk.fleet_size = 2;
    assert_eq!(sk.slots[0].race, 0);
    let cheapest = offered(&lib, 0).len() - 1; // the list is most expensive first; points never limit cheap ships here
    apply(&mut sk, &lib, &Action::PickShip(cheapest));
    for _ in 0..3 {
        apply(&mut sk, &lib, &Action::AddShip);
    }
    assert_eq!(sk.slots[0].fleet.len(), 2, "fleet size limit");
    apply(&mut sk, &lib, &Action::DropShip);
    assert_eq!(sk.slots[0].fleet.len(), 1);
    sk.choose(Cell::Fleet(0), Choice::Fleet(2));
    assert!(sk.slots[0].fleet.is_empty() && sk.slots[0].race == 2, "a new fleet type clears the fleet");
    apply(&mut sk, &lib, &Action::PickShip(0));
    apply(&mut sk, &lib, &Action::AddShip);
    sk.choose(Cell::Team(0), Choice::Team(4));
    assert!(sk.slots[0].fleet.is_empty() && sk.slots[0].team == 4, "the original resets the fleet when the team changes");
    sk.choose(Cell::Player(2), Choice::Computer);
    assert_eq!(sk.slots[2].kind, Kind::Computer);
}
#[test]
#[ignore = "needs imported assets"]
fn named_crew_cost_victory_points_and_leave_with_their_ship() {
    let lib = Library::load(assets_dir());
    let mut sk = skirmish("diablo_straight");
    apply(&mut sk, &lib, &Action::PickShip(1));
    apply(&mut sk, &lib, &Action::AddShip);
    let before = sk.points_left(&lib);
    let id = crate::crew_arms::roster("Navy", 0)[0].clone();
    let cost = i64::from(crate::crew_arms::crew_points(&id));
    sk.slots[0].crew_mut(0)[1].push(id.clone());
    assert_eq!(before - sk.points_left(&lib), cost);
    assert_eq!(sk.slots[0].crew_at(&id), Some((0, 1)));
    apply(&mut sk, &lib, &Action::DropShip);
    assert!(sk.slots[0].crew_at(&id).is_none(), "dropping a ship returns its crew");
    assert_eq!(sk.points_left(&lib), 300);
}
#[test]
#[ignore = "needs imported assets"]
fn auto_assign_mixes_hulls_that_fit_and_spends_the_rest_on_crew() {
    let lib = Library::load(assets_dir());
    let mut sk = skirmish("diablo_straight");
    auto_assign(&mut sk, &lib);
    let h = &sk.slots[0];
    assert_eq!(h.fleet.len(), 5, "a 300-point budget fills five slots");
    assert!(h.fleet.iter().any(|n| n != &h.fleet[0]), "classes are mixed");
    assert!(h.crew.iter().flatten().flatten().count() > 0, "leftover points buy named crew");
    assert!(sk.points_left(&lib) >= 0, "weapon swaps stay inside the budget");
    let left = sk.points_left(&lib);
    assert!((0..30).contains(&left), "nearly everything is spent: {left}");
    // A tiny budget buys the few cheap hulls that fit.
    let mut sk = skirmish("diablo_straight");
    sk.victory_limit = 50;
    auto_assign(&mut sk, &lib);
    assert!(!sk.slots[0].fleet.is_empty() && sk.points_left(&lib) >= 0);
}
#[test]
#[ignore = "needs imported assets"]
fn computers_cycle_fleet_types_by_slot_and_fleets_never_share_a_flag() {
    let mut sk = skirmish("diablo_straight");
    for i in 1..=4 {
        sk.choose(Cell::Player(i), Choice::Computer);
    }
    assert_eq!(sk.slots.iter().take(5).map(|s| s.race).collect::<Vec<_>>(), [0, 2, 3, 0, 2], "Navy, Pirate, Procyon, then round again");
    // Two Navy fleets: the second gets the next flag, and the first cannot take it.
    assert_eq!((sk.slots[0].flag, sk.slots[3].flag), (0, 1));
    assert!(sk.flag_taken(0, 0, 1));
    let lib = Library::load(assets_dir());
    apply(&mut sk, &lib, &Action::PickFlag(1));
    assert_eq!(sk.slots[0].flag, 0, "a taken flag is refused");
    apply(&mut sk, &lib, &Action::PickFlag(5));
    assert_eq!(sk.slots[0].flag, 5);
    // Changing a fleet type moves it off a flag the new type's fleets already fly.
    sk.choose(Cell::Fleet(3), Choice::Fleet(2));
    assert_ne!(sk.slots[3].flag, sk.slots[4].flag);
}
#[test]
#[ignore = "needs imported assets"]
fn computer_fleets_use_auto_assign_inside_the_victory_limit() {
    let lib = Library::load(assets_dir());
    let mut sk = skirmish("diablo_straight");
    sk.choose(Cell::Player(1), Choice::Computer);
    sk.slots[0].push_ship("Ship_Navy_Cutter");
    let cost = |sk: &Skirmish| -> (usize, i64) {
        let g = &sk.plan(&lib)[1];
        let ships: i64 = g.ships.iter().map(|(n, ..)| i64::from(lib.classes[lib.index(n)].points)).sum();
        (g.ships.len(), ships)
    };
    let (n, points) = cost(&sk);
    assert!(n >= 1 && (120..=180).contains(&points), "retail: hulls take 40-60 % of the 300 points: {n} ships cost {points}");
    let plan = sk.plan(&lib);
    let crew: Vec<&String> = plan[1].crew.iter().flatten().flatten().collect();
    assert!(crew.iter().all(|id| id.contains("_Navy_")), "computer crews come from the Navy roster: {crew:?}");
    sk.victory_limit = 60;
    let (n, points) = cost(&sk);
    assert!(n > 0 && points <= 60, "a small limit buys cheap hulls: {n} ships cost {points}");
    assert!(sk.plan(&lib)[1].crew.iter().flatten().any(|post| !post.is_empty()), "computer ships carry named crew (retail saves), so their factors follow the posts");
    // Any gun a computer swapped in comes from the retail list (no harpoons, lancers, energy nets).
    // Retail at 1000 points / limit 10 fielded 3-8 ships, not the 1-3 seen at 300.
    (sk.victory_limit, sk.fleet_size) = (1000, 10);
    let (n, points) = cost(&sk);
    assert!(n >= 3 && (400..=600).contains(&points), "1000 points: {n} ships cost {points}");
    assert!(sk.plan(&lib)[1].ships.iter().all(|s| !s.0.contains("Tender") && !s.0.contains("Tug")), "retail computers never field support ships");
    let swapped: Vec<String> = sk.plan(&lib)[1].weapons.iter().flat_map(|w| w.values().cloned()).collect();
    assert!(!swapped.is_empty(), "a big budget buys some swaps");
    assert!(swapped.iter().all(|g| super::skirmish::RETAIL_COMPUTER_GUNS.contains(&g.as_str())), "{swapped:?}");
}
#[test]
#[ignore = "needs imported assets"]
fn slot_bar_shows_five_slots_and_follows_the_picked_one() {
    let lib = Library::load(assets_dir());
    let mut sk = skirmish("diablo_straight");
    sk.fleet_size = 8;
    sk.scroll_slots(10);
    assert_eq!(sk.slot_scroll, 3, "eight slots: the last window starts at slot 4");
    sk.scroll_slots(-10);
    assert_eq!(sk.slot_scroll, 0);
    apply(&mut sk, &lib, &Action::PickShip(offered(&lib, 0).len() - 1));
    for _ in 0..7 {
        apply(&mut sk, &lib, &Action::AddShip);
    }
    assert_eq!((sk.fleet_pick, sk.slot_scroll), (6, 2), "adding the seventh ship scrolls it into view");
    apply(&mut sk, &lib, &Action::PickFleet(0));
    assert_eq!(sk.slot_scroll, 0);
}
#[test]
#[ignore = "needs imported assets"]
fn create_skirmish_sliders_use_the_retail_ranges() {
    let lib = Library::load(assets_dir());
    let mut sk = skirmish("diablo_straight");
    assert_eq!((sk.victory_limit, sk.fleet_size), (300, 5));
    apply(&mut sk, &lib, &Action::VictoryLimit(-100));
    assert_eq!(sk.victory_limit, 50);
    apply(&mut sk, &lib, &Action::VictoryLimit(1));
    assert_eq!(sk.victory_limit, 60);
    apply(&mut sk, &lib, &Action::VictoryLimit(500));
    assert_eq!(sk.victory_limit, 1000);
    apply(&mut sk, &lib, &Action::FleetSize(-20));
    assert_eq!(sk.fleet_size, 1);
    apply(&mut sk, &lib, &Action::FleetSize(20));
    assert_eq!(sk.fleet_size, 10);
    // Dragging the knob: the end stops and the step rounding.
    assert_eq!((VICTORY_LIMIT.at(0.0), VICTORY_LIMIT.at(1.0), VICTORY_LIMIT.at(0.5)), (50, 1000, 530));
    assert_eq!((FLEET_SIZE.at(-3.0), FLEET_SIZE.at(0.5), FLEET_SIZE.at(9.0)), (1, 6, 10));
}
#[test]
#[ignore = "needs imported assets"]
fn fleet_screen_starts_blank_and_greys_crew_arms_drop() {
    let lib = Library::load(assets_dir());
    let mut sk = skirmish("diablo_straight");
    assert!(sk.shown_class(&lib).is_none() && !sk.slot_filled() && !sk.ship_chosen);
    apply(&mut sk, &lib, &Action::PickShip(1));
    assert!(sk.shown_class(&lib).is_some() && !sk.slot_filled(), "a picked hull shows, Crew/Arms/Drop stay greyed");
    apply(&mut sk, &lib, &Action::AddShip);
    assert!(sk.slot_filled());
    apply(&mut sk, &lib, &Action::DropShip);
    assert!(!sk.slot_filled());
}
#[test]
#[ignore = "needs imported assets"]
fn fleet_screen_offers_every_fleet_type_without_story_hulls() {
    let lib = Library::load(assets_dir());
    for (race, name) in [(0, "Navy"), (2, "Pirate"), (3, "Procyon")] {
        let list = offered(&lib, race);
        assert!(list.len() >= 5, "{name}: {}", list.len());
        assert!(list.iter().all(|&i| lib.classes[i].race == name && !NOT_OFFERED.iter().any(|n| lib.classes[i].name.contains(n))));
    }
    assert!(offered(&lib, 4).is_empty());
}
#[test]
fn battle_entry_removes_menu_and_descendants_only() {
    for source in [Screen::SinglePlayer, Screen::Lobby] {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin)).init_state::<Screen>().add_systems(OnEnter(Screen::Battle), clear_menu);
        // This test exercises state transitions/cleanup without renderer-dependent menu building.
        app.update();
        app.world_mut().resource_mut::<NextState<Screen>>().set(source);
        app.update();
        let root = app.world_mut().spawn(MenuRoot).id();
        let child = app.world_mut().spawn(ChildOf(root)).id();
        let grandchild = app.world_mut().spawn(ChildOf(child)).id();
        let battle_ui = app.world_mut().spawn_empty().id();
        app.world_mut().resource_mut::<NextState<Screen>>().set(Screen::Battle);
        app.update();
        assert_eq!(*app.world().resource::<State<Screen>>().get(), Screen::Battle);
        for e in [root, child, grandchild] {
            assert!(app.world().get_entity(e).is_err());
        }
        assert!(app.world().get_entity(battle_ui).is_ok());
        // Returning to the same screen must not leave a new menu behind on relaunch.
        app.world_mut().resource_mut::<NextState<Screen>>().set(source);
        app.update();
        let root = app.world_mut().spawn(MenuRoot).id();
        app.world_mut().resource_mut::<NextState<Screen>>().set(Screen::Battle);
        app.update();
        assert!(app.world().get_entity(root).is_err());
    }
}

#[test]
#[ignore = "needs imported assets"]
fn loading_a_fleet_crew_or_arms_profile_restores_what_was_saved_and_refuses_what_does_not_fit() {
    use crate::profiles::Kind as K;
    let lib = Library::load(assets_dir());
    let mut sk = skirmish("red_rover");
    sk.slots[0].race = 0;
    sk.fleet_size = 5;
    sk.victory_limit = 1000;
    apply(&mut sk, &lib, &Action::AutoAssign);
    let human = sk.slots.iter().position(|s| s.kind == Kind::Human).unwrap();
    assert!(sk.slots[human].fleet.len() >= 2);
    let id = crate::crew_arms::roster("Navy", 0)[0].clone();
    sk.slots[human].crew_mut(0)[0] = vec![id.clone()];
    let before = sk.slots[human].clone();
    let fleet = before.profile_of(K::Fleet, 0);
    // Save, drop the fleet, load: the same hulls come back, with stock guns and no crew.
    sk.slots[human].clear_fleet();
    assert_eq!(sk.slots[human].apply_profile(K::Fleet, &fleet, 0, &lib, 1000, 5), Ok(()));
    let after = &sk.slots[human];
    assert_eq!(after.fleet, before.fleet);
    assert!(after.crew.iter().flatten().all(Vec::is_empty) && after.weapons.iter().all(|w| w.is_empty()), "a fleet is hulls only");
    // Arms saved from one Man-O-War load onto another.
    let mut two = sk.slots[human].clone();
    two.clear_fleet();
    two.push_ship("Ship_Navy_ManOWar");
    two.push_ship("Ship_Navy_ManOWar");
    let class = &lib.classes[lib.index("Ship_Navy_ManOWar")];
    let (bank, first) = crate::crew_arms::selectable_banks(&lib, class)[0];
    let swap = |g: &&String| *g != &class.mounts[first].gun && crate::crew_arms::bank_compatible(&lib, class, bank, g);
    let gun = lib.guns.keys().find(swap).unwrap().clone();
    two.weapons[0].insert(bank, gun);
    let arms = two.profile_of(K::Arms, 0);
    assert_eq!(two.apply_profile(K::Arms, &arms, 1, &lib, 1000, 5), Ok(()));
    assert_eq!(two.weapons[1], two.weapons[0]);
    // Crew belongs to its hull class and may not be posted twice.
    let crew = before.profile_of(K::Crew, 0);
    sk.slots[human].crew_mut(0)[0] = vec![id.clone()];
    assert!(sk.slots[human].apply_profile(K::Crew, &crew, 1, &lib, 1000, 5).is_err() || before.fleet[0] == before.fleet[1]);
    let mut twice = crew.clone();
    twice.ships[0].class = before.fleet[1].clone();
    assert!(sk.slots[human].apply_profile(K::Crew, &twice, 1, &lib, 1000, 5).is_err(), "the crew member is posted on ship 0");
    // The budget and the fleet size limit are kept.
    assert_eq!(sk.slots[human].apply_profile(K::Fleet, &fleet, 0, &lib, 10, 5), Err("Not enough victory points."));
    assert!(sk.slots[human].apply_profile(K::Fleet, &fleet, 0, &lib, 1000, 1).is_err());
    assert_eq!(sk.slots[human].fleet, before.fleet, "a refused load leaves the fleet alone");
}
