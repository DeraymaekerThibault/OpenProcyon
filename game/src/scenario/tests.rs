use super::*;

mod ambient_tests {
    use super::ambient_effect as a;
    #[test]
    fn ambient_points_play_the_effect_their_world_object_names() {
        assert_eq!(a("PlayEffect", "FireOnShip"), Some("FireOnShip"));
        assert_eq!(a("PlayEffect", "volcanoSparks"), Some("volcanoSparks"));
        assert_eq!(a("VolcanoSmoke", ""), Some("VolcanoSmoke"));
        assert_eq!(a("PlayEffect", ""), None, "older imports name none");
    }
}

mod rule_pass_tests {
    use super::*;
    #[test]
    fn end_rules_are_tested_once_a_second_of_game_time() {
        let mut pass = RulePass::default();
        let ticks: Vec<usize> = (1..=100).filter(|_| pass.due(0.033)).collect();
        // 0.033 s ticks: a pass needs the clock more than 1.0 s past the last one, so every 31st tick (1.023 s).
        assert_eq!(ticks.len(), 3);
        let mut pass = RulePass::default();
        assert!(!pass.due(0.5) && !pass.due(0.5), "exactly one second is not more than one second");
        assert!(pass.due(0.01));
        assert!(!pass.due(0.5) && pass.due(0.51));
    }
}

mod end_line_tests {
    use super::*;
    #[test]
    fn game_won_for_the_winners_lost_for_the_rest_unless_the_rule_names_a_message() {
        assert_eq!(end_line_id(Outcome::Playing, None), None);
        assert_eq!(end_line_id(Outcome::Victory, None).as_deref(), Some("IDGS_TPINGAMEMESSAGE_GAME_WON"));
        assert_eq!(end_line_id(Outcome::Defeat, None).as_deref(), Some("IDGS_TPINGAMEMESSAGE_GAME_LOST"));
        // Border Dispute's no-human-ships rule: allies won (summary VICTORY!) but the rule's message is "Game Lost" for everyone.
        assert_eq!(end_line_id(Outcome::Victory, Some("IDGS_TPINGAMEMESSAGE_GAME_LOST")).as_deref(), Some("IDGS_TPINGAMEMESSAGE_GAME_LOST"));
    }
}

mod rotation_tests {
    use crate::scenario::Race::*;
    #[test]
    fn base_owners_are_read_from_their_names() {
        for (name, race) in [
            ("navybase", Navy),
            ("Piratebase", Pirate),
            ("proc base", Procyon),
            ("pirate base", Pirate),
            ("navy port", Navy),
            ("Base_Grant", Navy),
            ("Base_Procyon2", Procyon),
            ("Base_PirateMP_3", Pirate),
        ] {
            assert_eq!(super::faction(name), Some(race), "{name}");
        }
        assert_eq!(super::faction("asteroids"), None);
    }
    use super::*;
    #[test]
    #[ignore = "needs imported assets"]
    fn all_mission_two_base_docks_meet_the_authored_flight_plane() {
        let map: serde_json::Value = serde_json::from_slice(&std::fs::read(crate::assets_dir().join("maps/mission02/map.json")).unwrap()).unwrap();
        let docks = crate::docking::load_docks();
        for kind in ["Base_AbandonedCamp", "Base_SmugglersCamp", "Base_Safetytown"] {
            let object = map["objects"].as_array().unwrap().iter().find(|o| o["type"] == kind).unwrap();
            let tf = scenery_transform(object, game_rotation(&object["rot"]), &docks);
            let dock = crate::docking::dockable_for(&docks, kind).unwrap();
            assert!(tf.transform_point(dock.points[0].pos).y.abs() < 0.01, "{kind} dock must meet the ship's flight plane");
            for point in &dock.points {
                assert!(tf.transform_point(point.pos).y.abs() < 0.01, "{kind}: all its docks must be reachable at flight height");
            }
        }
        let custom = serde_json::json!({"pos": [1, 2, 3]});
        assert_eq!(scenery_transform(&custom, Quat::IDENTITY, &docks).translation, crate::game_to_bevy(&custom["pos"]));
    }

    #[test]
    fn scenery_datum_preserves_map_altitude_horizontal_coordinates_and_dock_spacing() {
        let docks = serde_json::json!({"Island_Test": {"points": [{"pos": [100, 200, 9]}, {"pos": [0, 0, 12]}]}});
        let mut object = serde_json::json!({"type": "Island_Test", "pos": [10, 20, 30]});
        let rotation = Quat::from_rotation_y(0.7);
        let tf = scenery_transform(&object, rotation, &docks);
        assert_eq!(tf.translation, Vec3::new(10.0, 21.0, -20.0));
        assert_eq!(tf.rotation, rotation);
        assert_eq!(tf.transform_point(Vec3::new(100.0, 9.0, -200.0)).y, 30.0);
        assert_eq!(tf.transform_point(Vec3::new(0.0, 12.0, 0.0)).y, 33.0);
        object["scene_origin"] = serde_json::json!([1, 2, 3]);
        assert_eq!(scenery_transform(&object, rotation, &docks).translation, game_to_bevy(&object["pos"]) - rotation * Vec3::new(1.0, 3.0, -2.0));
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn every_shipped_dockable_scenery_uses_its_first_dock_as_height_datum() {
        let docks = crate::docking::load_docks();
        let mut checked = std::collections::HashSet::new();
        for entry in std::fs::read_dir(crate::assets_dir().join("maps")).unwrap().flatten() {
            let Ok(bytes) = std::fs::read(entry.path().join("map.json")) else { continue };
            let map: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            for object in map["objects"].as_array().into_iter().flatten() {
                let kind = object["type"].as_str().unwrap_or_default();
                let Some(dock) = crate::docking::dockable_for(&docks, kind) else { continue };
                let tf = scenery_transform(object, game_rotation(&object["rot"]), &docks);
                assert!((tf.transform_point(dock.points[0].pos).y - game_to_bevy(&object["pos"]).y).abs() < 0.01, "{kind}: {}", entry.path().display());
                checked.insert(kind.to_owned());
            }
        }
        assert!(checked.len() >= 20, "must cover the base catalogue, not only Mission 2");
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn border_dispute_central_island_hangs_by_its_layer_offset_only() {
        // Island_Large01's mesh layer already sits 29.25 below the object (scene position minus pivot) and its dock comes
        // down with it to the flight plane, so the placement adds no second shift.
        let docks = crate::docking::load_docks();
        let map: serde_json::Value = serde_json::from_slice(&std::fs::read(crate::assets_dir().join("maps/border_dispute/map.json")).unwrap()).unwrap();
        let island = map["objects"].as_array().unwrap().iter().find(|o| o["type"] == "Island_Large01").unwrap();
        assert!(scenery_transform(island, game_rotation(&island["rot"]), &docks).translation.y.abs() < 0.05);
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn skirmish_bases_and_tenders_are_dockable() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<bevy::world_serialization::WorldAsset>()
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<StandardMaterial>()
            .init_asset::<AudioSource>()
            .init_asset::<crate::fx::EffectMaterial>()
            .insert_resource(crate::testutil::lib())
            .init_resource::<fx::FxLib>()
            .init_resource::<collision::Shapes>()
            .init_resource::<data::AsteroidDefs>()
            .init_resource::<ship::Fleets>()
            .init_resource::<crate::combat::ArmedBank>()
            .insert_resource(Orbit { yaw: 0.0, pitch: 0.0, dist: 60.0, focus: None })
            .insert_resource(Config { player: "Ship_Navy_Tender".into(), enemies: vec!["Ship_Navy_Cutter".into()], map: Some("red_rover".into()), speed: 1.0, ..default() });
        app.world_mut()
            .run_system_once(|mut c: Commands, a: Res<AssetServer>, l: Res<Library>, f: Res<fx::FxLib>, s: Res<collision::Shapes>, r: Res<data::AsteroidDefs>, cfg: Res<Config>| {
                spawn_scenario(&mut c, &a, &l, &f, &s, &r, &cfg)
            })
            .unwrap();
        let w = app.world_mut();
        let ships = w.query_filtered::<&crate::docking::Dockable, With<Ship>>().iter(w).filter(|d| d.enabled).count();
        let scenery = w.query_filtered::<&crate::docking::Dockable, Without<Ship>>().iter(w).filter(|d| d.enabled).count();
        assert_eq!(ships, 1, "the player's tender");
        assert_eq!(scenery, 2, "red_rover's Montgomery and pirate bases");
    }

    /// A skirmish fleet starts with its map slot's formation type but holds none: no ship follows a slot, so the captain drives them.
    #[test]
    #[ignore = "needs imported assets"]
    fn a_fleet_starts_with_its_formation_selected_and_no_ship_holding_it() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<bevy::world_serialization::WorldAsset>()
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<StandardMaterial>()
            .init_asset::<AudioSource>()
            .init_asset::<crate::fx::EffectMaterial>()
            .insert_resource(crate::testutil::lib())
            .init_resource::<fx::FxLib>()
            .init_resource::<collision::Shapes>()
            .init_resource::<data::AsteroidDefs>()
            .init_resource::<ship::Fleets>()
            .init_resource::<crate::combat::ArmedBank>()
            .insert_resource(Orbit { yaw: 0.0, pitch: 0.0, dist: 60.0, focus: None })
            .insert_resource(Config {
                player: "Ship_Navy_Cutter".into(),
                allies: vec!["Ship_Navy_Cutter".into(), "Ship_Navy_Cutter".into()],
                enemies: vec!["Ship_Navy_Cutter".into()],
                map: Some("red_rover".into()),
                speed: 1.0,
                ..default()
            });
        app.world_mut()
            .run_system_once(|mut c: Commands, a: Res<AssetServer>, l: Res<Library>, f: Res<fx::FxLib>, s: Res<collision::Shapes>, r: Res<data::AsteroidDefs>, cfg: Res<Config>| {
                spawn_scenario(&mut c, &a, &l, &f, &s, &r, &cfg)
            })
            .unwrap();
        let w = app.world_mut();
        let formed = w.resource::<ship::Fleets>().groups.values().filter(|f| f.formation != ship::Formation::None).count();
        assert!(formed > 0, "red_rover gives its fleets Line Abreast");
        let pilots = w.query::<&ship::FleetPilot>().iter(w).count();
        assert!(pilots >= 3, "the fleets have followers: {pilots}");
        assert!(w.query::<&ship::FleetPilot>().iter(w).all(|p| !p.hold), "no ship holds formation at the start");
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn native_column_matrix_preserves_every_authored_scenery_axis() {
        for map in ["storm_front", "mission13", "mission03", "basic_tutorial"] {
            let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(crate::assets_dir().join(format!("maps/{map}/map.json"))).unwrap()).unwrap();
            for object in doc["objects"].as_array().unwrap() {
                let r = object["rot"].as_array().unwrap();
                let q = game_rotation(&object["rot"]);
                for (axis, i) in [(Vec3::X, 0), (Vec3::NEG_Z, 3), (Vec3::Y, 6)] {
                    let expected = Vec3::new(r[i].as_f64().unwrap() as f32, r[i + 2].as_f64().unwrap() as f32, -r[i + 1].as_f64().unwrap() as f32);
                    assert!((q * axis).distance(expected) < 0.001, "{map}: {} axis {i}", object["type"]);
                }
            }
        }
    }

    /// A won skirmish stays won when the end flow removes the player's ship (it used to flip to DEFEAT two seconds later).
    #[test]
    fn decided_outcome_is_final() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.init_resource::<Time>().insert_resource(Outcome::Victory).init_resource::<EndMessage>().init_resource::<EndLine>().insert_resource(Config {
            player: "Ship_Navy_Cutter".into(),
            map: Some("red_rover".into()),
            speed: 1.0,
            ..default()
        });
        app.world_mut().run_system_once(outcome).unwrap(); // no PlayerShip left
        assert!(*app.world().resource::<Outcome>() == Outcome::Victory);
    }
    /// A mission's rules end it through the same decision as a skirmish: the outcome is set at once (the Game Summary countdown starts),
    /// and the battlefield shows no end line (the mission keeps its own end text). Asset-free.
    #[test]
    fn a_mission_end_sets_the_outcome_without_an_end_line() {
        use bevy::ecs::system::RunSystemOnce;
        for won in [true, false] {
            let mut app = App::new();
            let data = serde_json::json!({"rules": []});
            let mut script = crate::rules::Script::new(&data, serde_json::Value::Null);
            script.ended = Some(("Mission complete.".into(), won));
            app.init_resource::<Time>().insert_resource(Outcome::Playing).init_resource::<EndMessage>().init_resource::<EndLine>().insert_resource(Config {
                frame: 5,
                speed: 1.0,
                ..default()
            });
            app.insert_resource(campaign::Mission { id: "synthetic".into(), script, data, opening: None });
            app.world_mut().run_system_once(outcome).unwrap();
            assert_eq!(*app.world().resource::<Outcome>(), if won { Outcome::Victory } else { Outcome::Defeat });
            assert_eq!(app.world().resource::<EndLine>().0, None, "no end line");
            assert_eq!(app.world().resource::<Config>().ended, Some(5));
        }
    }
}

mod lighting_tests {
    use super::*;
    #[test]
    fn map_light_is_read_in_the_bevy_frame_and_absent_without_the_field() {
        let map = serde_json::json!({"lighting": {"ambient": [0, 0, 0, 1], "roof_direction": [0.0, 1.0, -1.0], "floor": [0.4, 0.45, 0.8, 1.0], "roof": [1.0, 1.0, 1.0, 1.0]}});
        let l = lighting_of(&map).unwrap();
        // game (x starboard, y forward, z up) -> Bevy (x, z, -y): forward-and-down travels toward -y and -z
        assert!((l.direction - Vec3::new(0.0, -1.0, -1.0).normalize()).length() < 1e-5, "{:?}", l.direction);
        assert_eq!(l.ambient, Color::srgb(0.4, 0.45, 0.8));
        assert!(lighting_of(&serde_json::json!({"sky": "x"})).is_none(), "older imports keep the default light");
        assert!(lighting_of(&serde_json::json!({"lighting": {"roof_direction": [0, 0, 0], "floor": [1, 1, 1], "roof": [1, 1, 1]}})).is_none(), "a zero direction is no light");
    }

    #[test]
    fn a_tower_belongs_to_its_map_player_unless_its_setup_names_another() {
        let players = [serde_json::json!({"name": "navy"}), serde_json::json!({"name": "pirate base"})];
        let tower = serde_json::json!({"id": 4, "player": 1});
        assert_eq!(base_owner(&tower, &players, None), "pirate base", "the map object's player, played or not");
        assert_eq!(base_owner(&tower, &players, Some(&(Some("navy".into()), None))), "navy", "a Setup Island's Player wins");
        assert_eq!(base_owner(&serde_json::json!({"id": 5, "player": -1}), &players, None), "", "no player: NO PLAYER");
        // Border Dispute's base players are slots nobody plays: with the map's pairs each fights for its own side, hostile to the
        // other team, so the fleets go for it; a tower of NO PLAYER is everyone's ally and never fires.
        let group = |player, side, team| crate::Group { player, side: Side(side), team, ..default() };
        let players = [serde_json::json!({"name": "navy", "team": 0}), serde_json::json!({"name": "pirates", "team": 1}), serde_json::json!({"name": "pirate base", "team": 1})];
        let t = battle_alliances(&[group(0, 0, 0), group(1, 1, 1)], &players, Some(&serde_json::json!([["pirates", "pirate base"]])));
        let base = t.side_or_neutral(&base_owner(&tower, &players, None));
        assert!(t.hostile(base, Side(0)) && t.allied(base, Side(1)), "the camp of an unplayed slot: the navy's foe, the pirates' ally");
        assert!(t.allied(t.side_or_neutral(""), Side(0)) && t.allied(t.side_or_neutral(""), Side(1)));
    }

    #[test]
    fn setup_island_owners_and_strengths_come_from_the_script() {
        let script = serde_json::json!({"rules": [{"actions": [
            {"Type": "*State Init* Setup Island", "World Object ID": 1, "Combat Strength": 60, "Player/Owner": "pirate base"},
            {"Type": "*State Init* Setup Island", "World Object ID": 133, "Combat Strength": 80},
            {"Type": "Play Music Track"}]}]});
        let s = setup_islands(&script);
        assert_eq!((s[&1].clone(), s[&133].clone(), s.len()), ((Some("pirate base".into()), Some(60)), (None, Some(80)), 2));
    }
}

mod console_reset_tests {
    use super::*;
    use crate::rules::console::{Cheats, DevView};

    #[test]
    fn a_respawned_scenario_clears_the_cheats_and_a_new_mission_clears_the_developer_view() {
        let mut cheats = Cheats { used: true, god: true };
        let mut dev = DevView { hud_hidden: true, ..Default::default() }; // the field only: the statics are shared by every test
        reset_console_state(&mut cheats, &mut dev, false);
        assert!(!cheats.used && !cheats.god, "restart: points count again and god is off");
        assert!(dev.hud_hidden, "the same mission keeps the developer view");
        cheats.used = true;
        reset_console_state(&mut cheats, &mut dev, true);
        assert!(!cheats.used && !dev.hud_hidden, "next mission: plain view");
    }
}

mod race_tests {
    use crate::scenario::Race::{self, *};
    #[test]
    fn twt_numbers_parse_and_round_trip() {
        for (n, r) in [(0, Navy), (2, Pirate), (3, Procyon), (4, Unchosen)] {
            assert_eq!(Race::from_twt(n), r);
            assert_eq!(u64::from(r.twt()), n);
        }
        assert_eq!(Race::from_twt(1), Unchosen, "numbers the lobby never offers are no fleet type");
        assert_eq!(Race::default(), Unchosen);
    }
    #[test]
    fn class_names_flags_and_texts_follow_the_fleet_type() {
        assert_eq!(Race::PLAYABLE.map(|r| (r.class_name(), r.flag_base())), [(Some("Navy"), 0), (Some("Pirate"), 10), (Some("Procyon"), 20)]);
        assert_eq!((Unchosen.class_name(), Unchosen.fleet_text(), Unchosen.flag_base()), (None, None, 0));
        for r in Race::PLAYABLE {
            assert_eq!(Race::from_class(r.class_name().unwrap()), Some(r));
        }
        assert_eq!((Race::from_class("Civilian"), Race::from_class("")), (None, None));
    }
}

/// #536: one seed gives one sequence, the named streams are independent of the battle stream, and a battle start (`reset_sim`) reseeds,
/// renumbers spawns from 0 and snapshots the difficulty.
#[test]
fn a_battle_start_reseeds_and_renumbers() {
    let draws = |r: &mut combat::SimRng| (0..4).map(|_| r.f()).collect::<Vec<_>>();
    let (mut a, mut b) = (combat::SimRng::new(42), combat::SimRng::new(42));
    assert_eq!(draws(&mut a), draws(&mut b));
    assert_ne!(draws(&mut combat::SimRng::new(43)), draws(&mut combat::SimRng::new(42)));
    let mut c = combat::SimRng::new(42);
    let nebula: Vec<f32> = (0..4).map(|_| c.nebula.f()).collect();
    assert_eq!(draws(&mut c), draws(&mut combat::SimRng::new(42)), "drawing from a named stream leaves the battle stream alone");
    assert_ne!(nebula, draws(&mut combat::SimRng::new(42)));
    // The default seed keeps the battle stream every battle used before seeds were settable.
    assert_eq!(combat::Rng::default().f(), combat::SimRng::default().f());

    #[derive(Component)]
    struct Thing;
    let mut app = App::new();
    number_spawns::<Thing>(&mut app);
    app.init_resource::<combat::SimRng>();
    let spawn = |app: &mut App| app.world_mut().spawn(Thing).id();
    let (x, y) = (spawn(&mut app), spawn(&mut app));
    app.world_mut().flush();
    let index = |app: &App, e| *app.world().get::<SpawnIndex>(e).unwrap();
    assert_eq!((index(&app, x), index(&app, y)), (SpawnIndex(0), SpawnIndex(1)));
    app.world_mut().resource_mut::<combat::SimRng>().f();
    reset_sim(app.world_mut());
    assert_eq!(app.world().resource::<combat::SimRng>().clone_state(), combat::SimRng::new(combat::SimRng::configured_seed()).clone_state());
    let z = spawn(&mut app);
    app.world_mut().flush();
    assert_eq!(index(&app, z), SpawnIndex(0), "numbering starts again at the next battle");
    assert_eq!(*app.world().resource::<Difficulty>(), Difficulty(menu::difficulty()));
}

#[test]
fn in_spawn_order_sorts_by_creation_number_unnumbered_first() {
    let mut world = World::new();
    let (late, early, none) = (world.spawn(SpawnIndex(5)).id(), world.spawn(SpawnIndex(1)).id(), world.spawn_empty().id());
    let mut state = bevy::ecs::system::SystemState::<Query<&SpawnIndex>>::new(&mut world);
    let order = state.get(&world).unwrap();
    assert_eq!(in_spawn_order(&order, [late, early, none], |e| *e), vec![none, early, late]);
}

/// #536: a system that keeps per-entity state in a `Local` learns of a battle start exactly once, and the effect detail is a
/// battle-start snapshot.
#[test]
fn a_battle_start_is_seen_once_per_system_and_snapshots_detail() {
    #[derive(Resource, Default)]
    struct News(Vec<bool>);
    let mut app = App::new();
    app.init_resource::<News>().init_resource::<combat::SimRng>().add_systems(Update, |mut start: BattleStart, mut news: ResMut<News>| news.0.push(start.is_new()));
    reset_sim(app.world_mut());
    app.update();
    app.update();
    reset_sim(app.world_mut());
    app.update();
    app.update();
    assert_eq!(app.world().resource::<News>().0, [true, false, true, false]);
    assert_eq!(*app.world().resource::<fx::DetailScale>(), fx::DetailScale(menu::detail_scale()));
}

/// Every battle start, a restart included, begins with fresh summary counts and without the last battle's lobby roster, skirmish
/// end rules or nebula state: a restart does not re-enter the Battle screen and a mission sets none of these itself.
#[test]
fn a_battle_start_forgets_the_last_battles_summary_roster_end_rules_and_nebula_state() {
    let mut world = World::new();
    let mut stats = crate::stats::GameStats::default();
    stats.players.insert("Hero".into(), crate::stats::Line { start: 3, ..default() });
    world.insert_resource(stats);
    world.insert_resource(crate::alliance::Roster::default());
    world.insert_resource(crate::victory::Victory::scripted(vec![]));
    world.insert_resource(crate::nebula::Storms(serde_json::json!([{ "name": "old" }])));
    world.insert_resource(crate::nebula::InNebula([Entity::from_raw_u32(7).unwrap()].into_iter().collect()));
    reset_sim(&mut world);
    assert!(world.resource::<crate::stats::GameStats>().players.is_empty(), "a restart counts the new battle only");
    assert!(!world.contains_resource::<crate::alliance::Roster>(), "a mission after a skirmish shows no player list");
    assert!(!world.contains_resource::<crate::victory::Victory>());
    assert!(!world.contains_resource::<crate::nebula::Storms>(), "a map without a nebula setup keeps none of the last one's");
    assert!(world.resource::<crate::nebula::InNebula>().0.is_empty());
}
