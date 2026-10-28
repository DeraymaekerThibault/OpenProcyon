use super::*;

/// Gunners take 10 x gunnery seconds off TimeBetweenBursts (docs/re/gunnery.md#gunners): a gunnery-8.5 crew fires the
/// 12 s heavy cannon every 3.5 s; the cut never goes below zero and leaves in-burst spacing alone.
#[test]
fn gunners_shorten_reload() {
    let (guns, _) = load_weapons(assets_dir());
    let heavy = &guns["Gun_CannonBall_Heavy"];
    assert!((heavy.burst_cycle_cut(8.5) - 3.5).abs() < 1e-5);
    assert_eq!(heavy.burst_cycle_cut(0.0), heavy.burst_cycle());
    assert_eq!(guns["Gun_CannonBall_Med_2Deck"].burst_cycle_cut(9.5), 0.0);
    let gat = &guns["Gun_Gattling_Light"];
    assert!((gat.burst_cycle_cut(9.0) - 7.0 * 0.15).abs() < 1e-5);
}

#[test]
fn damage_scales_with_side_and_difficulty() {
    assert_eq!(side_damage_scale(Side::Player, Side::Enemy, 1), 2.0);
    assert_eq!(side_damage_scale(Side::Enemy, Side::Player, 1), 0.5);
    assert_eq!(side_damage_scale(Side::Enemy, Side::Player, 0), 0.25);
    assert_eq!(side_damage_scale(Side::Player, Side::Enemy, 2), 1.0);
    assert_eq!(side_damage_scale(Side::Enemy, Side::Enemy, 0), 1.0);
    assert_eq!(side_damage_scale(Side::Player, Side(2), 1), 2.0, "every computer team takes the difficulty factor");
    assert_eq!(side_damage_scale(Side::Enemy, Side(2), 0), 1.0);
}

#[test]
fn destroyed_sail_does_not_absorb_shots_but_destroyed_vital_sections_do() {
    let (guns, _) = load_weapons(assets_dir());
    let mut class = load_class(assets_dir(), "Ship_Navy_Torpedo", &guns);
    class.sections.retain(|s| !s.vital);
    assert_eq!(class.sections.len(), 1);
    let [a, b, c] = class.sections[0].tris[0];
    let centre = (a + b + c) / 3.0;
    let normal = (b - a).cross(c - a).normalize();
    let (start, end) = (centre - normal, centre + normal);
    let mut hp = Health::new(&class);
    assert!(ship_segment_hit(&class, &hp, start, end).is_some());
    hp.sections[0] = 0.0;
    assert!(ship_segment_hit(&class, &hp, start, end).is_none());
    class.sections[0].vital = true;
    assert!(ship_segment_hit(&class, &hp, start, end).is_some());
}

#[test]
fn destroyed_torpedo_boat_sail_redirects_aim_to_hull() {
    let lib = Library::load(assets_dir());
    let class = &lib.classes[lib.index("Ship_Navy_Torpedo")];
    let k = class.sections.iter().position(|s| !s.vital).unwrap();
    let sail = &class.sections[k];
    let point = Vec3::new((sail.min.x + sail.max.x) / 2.0, sail.max.y - 0.01, (sail.min.z + sail.max.z) / 2.0);
    let mut hp = Health::new(class);
    assert_eq!(ship_aim_point(class, &hp, Some(point)), point);
    hp.sections[k] = 0.0;
    assert_eq!(ship_aim_point(class, &hp, Some(point)), class.centre);
    let hull = class.sections.iter().find(|s| s.vital && s.label == "Bow_wood").unwrap();
    let hull_point = (hull.min + hull.max) / 2.0;
    assert_eq!(ship_aim_point(class, &hp, Some(hull_point)), hull_point);
}

#[test]
fn breakup_chunks_fly_then_burst_inside_their_window() {
    let chunk = crate::data::Destruct { fx: Some("Final_Ship_Chunk".into()), start: 2.4, end: 2.5, vel: Vec3::new(-1.8, 5.6, -10.2) };
    let burst = burst_time(&chunk, 0.5);
    assert!((burst - 2.45).abs() < 1e-5, "the burst falls inside the window");
    assert_eq!(chunk_offset(&chunk, burst, 0.0), Some(Vec3::ZERO), "every piece starts on the hull");
    assert_eq!(chunk_offset(&chunk, burst, 1.0), Some(chunk.vel), "then flies at its velocity");
    assert_eq!(chunk_offset(&chunk, burst, 2.46), None, "and is gone after its burst");
    // a section without an effect (retail mid hulls) drifts until its window closes
    let drift = crate::data::Destruct { fx: None, start: 0.0, end: 2.0, vel: Vec3::X * 4.0 };
    assert_eq!(burst_time(&drift, 0.0), 2.0);
}

#[test]
fn cutter_masts_break_off_as_debris_pieces() {
    let lib = Library::load(assets_dir());
    let class = &lib.classes[lib.index("Ship_Navy_Cutter")];
    let by = |label: &str| class.sections.iter().find(|s| s.label == label).unwrap();
    let (model, pivot) = by("Mast_2_cloth").debris.clone().expect("imported debrisMeshScene");
    assert!(model.ends_with(".glb") && crate::data::model(&model).starts_with("models/debris/"), "{model}");
    assert!(pivot.y > 10.0, "the fore-mast piece pivots high up the mast (bevy y up)");
    assert_eq!(by("Mast_2_cloth").debris_fx.as_deref(), Some("Falling_Debris"));
    assert!(by("Port_Mid_Hull_wood").debris.is_none());
}

#[test]
fn debris_drifts_with_its_ship_sinks_tumbles_and_goes() {
    let mut app = App::new();
    app.init_resource::<Time>().add_systems(Update, debris);
    let start = Transform::from_xyz(0.0, 10.0, 0.0);
    let e = app.world_mut().spawn((Debris { vel: Vec3::new(0.0, 0.0, -4.0), axis: Vec3::X, spin: DEBRIS_SPIN, age: 0.0 }, start)).id();
    for _ in 0..20 {
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_millis(50));
        app.update();
    }
    let tf = *app.world().get::<Transform>(e).unwrap();
    assert!((tf.translation.z + 4.0).abs() < 0.3, "keeps the ship's velocity: {tf:?}");
    assert!(tf.translation.y < 10.0 - DEBRIS_FALL * 0.8, "and sinks");
    assert!(tf.rotation.angle_between(start.rotation) > 0.3, "and tumbles");
    app.world_mut().get_mut::<Debris>(e).unwrap().age = DEBRIS_LIFE;
    app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_millis(50));
    app.update();
    assert!(app.world().get_entity(e).is_err(), "gone after its life");
}

#[test]
fn cutter_breakup_uses_the_retail_section_data() {
    let lib = Library::load(assets_dir());
    let class = &lib.classes[lib.index("Ship_Navy_Cutter")];
    let d: Vec<_> = class.sections.iter().map(|s| s.destruct.clone().expect("imported destructFx")).collect();
    // bow: Final_Ship_Chunk at 2.9-3.0 s, flying forward and up (game y forward = bevy -z, game z up = bevy +y)
    let bow = &d[0];
    assert_eq!((bow.fx.as_deref(), bow.start, bow.end), (Some("Final_Ship_Chunk"), 2.9, 3.0));
    assert!(bow.vel.y > 0.0 && bow.vel.z < 0.0);
    assert!(d.iter().all(|c| c.end <= 3.0), "the whole breakup is over within 3 s");
}

#[test]
fn island_gun_towers_load_and_mission_damage_follows_their_vital_sections() {
    let lib = Library::load(assets_dir());
    let class = &lib.classes[lib.index("Base_SmugglersCamp")];
    assert_eq!(class.mounts.len(), 2);
    for mount in &class.mounts {
        let section = &class.sections[mount.parent.expect("gun belongs to a tower")];
        assert!(mount.pos.x >= section.min.x && mount.pos.x <= section.max.x);
        assert!(mount.pos.z >= section.min.z && mount.pos.z <= section.max.z);
        assert!((mount.pos.y - section.max.y).abs() < 1.0, "gun sits on its own tower roof");
    }
    assert!(!class.skirmish);
    let mut hp = Health::new(class);
    assert_eq!(base_mission_damage(class, &hp), 0.0);
    let point = base_aim_point(class, &hp, Some(class.mounts[0].pos)).unwrap();
    let ray = Ray3d::new(point + Vec3::Y * 20.0, Dir3::NEG_Y);
    assert!(base_ray_hit(class, &hp, &Transform::default(), ray).is_some());
    for (s, c) in hp.sections.iter_mut().zip(&class.sections) {
        if c.mission {
            *s = 0.0;
        }
    }
    assert_eq!(base_mission_damage(class, &hp), 1.0);
    assert!(base_aim_point(class, &hp, Some(point)).is_none());
    assert!(base_ray_hit(class, &hp, &Transform::default(), ray).is_none(), "destroyed towers are no longer attack targets");
}

#[test]
fn resting_muzzles_sit_at_their_authored_points() {
    // Barrel meshes and muzzles are authored in the base frame; the pivot only moves the pitch axis (Dreadnought hull guns
    // were drawn 0.9 too high and 1.2 too far forward when the pivot was added on top).
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default(), TransformPlugin)).init_asset::<bevy::world_serialization::WorldAsset>();
    let assets = app.world().resource::<AssetServer>().clone();
    let lib = Library::load(assets_dir());
    let class = lib.index("Ship_Procyon_Dreadnought");
    let ship = app.world_mut().spawn(Transform::default()).id();
    let states = spawn_guns(&mut app.world_mut().commands(), &assets, &lib, ship, class);
    app.update();
    let mut checked = 0;
    for (m, st) in lib.classes[class].mounts.iter().zip(&states.0) {
        let g = &lib.guns[&m.gun];
        for (p, e) in g.muzzles.iter().zip(&st.muzzles) {
            let at = app.world().get::<GlobalTransform>(*e).unwrap().translation();
            assert!(at.distance(m.pos + m.rot * *p) < 1e-4, "{}: muzzle at {at}, authored {}", m.gun, m.pos + m.rot * *p);
            checked += (g.pivot != Vec3::ZERO) as usize;
        }
    }
    assert!(checked > 0, "the Dreadnought has pivoted barrels");
}

#[test]
fn hostile_island_towers_shoot_the_player_and_neutral_ones_do_not() {
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default(), TransformPlugin))
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .init_asset::<bevy::prelude::Image>()
        .add_message::<Sfx>()
        .add_message::<Fired>()
        .add_message::<Hit>()
        .add_message::<Destroyed>()
        .add_message::<Detonate>()
        .add_message::<crate::decals::Scar>()
        .init_resource::<Time>()
        .init_resource::<FxLib>()
        .insert_resource(Library::load(assets_dir()))
        .insert_resource(Rng::new(7))
        .insert_resource(Look { mesh: default(), shell: default(), enemy_shell: default() })
        .init_resource::<crate::environment::Sensors>()
        .add_systems(PreUpdate, crate::environment::refresh_sensors)
        .add_systems(Update, base_fire);
    let mission = crate::campaign::load_id("mission02");
    let player = mission.script.player().to_string();
    assert!(mission.script.hostile(&player, "SmugglerCampEnemy"), "the camp's enemy owner is hostile to the player");
    assert!(!mission.script.hostile(&player, "NO PLAYER"));
    app.insert_resource(mission);
    let assets = app.world().resource::<AssetServer>().clone();
    let lib = Library::load(assets_dir());
    let class = lib.index("Base_SmugglersCamp");
    let spawn_base = |app: &mut App, owner: &str, x: f32| {
        let id = app.world_mut().spawn((Transform::from_xyz(x, -27.0, 0.0), crate::campaign::MissionBase { id: 1, owner: owner.into() })).id();
        let states = spawn_guns(&mut app.world_mut().commands(), &assets, &lib, id, class);
        app.world_mut().entity_mut(id).insert((ArmedBase { class }, Health::new(&lib.classes[class]), states));
        app.world_mut().flush();
        id
    };
    let hostile = spawn_base(&mut app, "SmugglerCampEnemy", 0.0);
    spawn_base(&mut app, "NO PLAYER", 40.0);
    let c = lib.index("Ship_Navy_Torpedo");
    let ship = Ship {
        class: c,
        side: Side::Player,
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
    };
    let ms = crate::campaign::MissionShip {
        id: 1,
        name: "p".into(),
        owner: player,
        hold_fire: false,
        invuln: 1.0,
        visible: true,
        stance: crate::rules::Stance::Neutral,
        target: None,
        attack: vec![],
        flag: String::new(),
        banner: String::new(),
        looping: None,
        stop_at_end: false,
        create_lifeboats: false,
        ..Default::default()
    };
    let player_ship = app.world_mut().spawn((Transform::from_xyz(0.0, 0.0, -120.0), ship, Health::new(&lib.classes[c]), Fires::default(), ms, PlayerShip)).id();
    for _ in 0..300 {
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.05));
        app.update();
    }
    let shooters: Vec<Entity> = app.world_mut().query::<&Shell>().iter(app.world()).map(|s| s.shooter).collect();
    assert!(!shooters.is_empty() && shooters.iter().all(|s| *s == hostile), "only the hostile camp opened fire");
    let states = spawn_guns(&mut app.world_mut().commands(), &assets, &lib, player_ship, c);
    app.world_mut().entity_mut(player_ship).insert(states);
    app.world_mut().flush();
    let old_shells: Vec<Entity> = app.world_mut().query_filtered::<Entity, With<Shell>>().iter(app.world()).collect();
    for e in old_shells {
        app.world_mut().despawn(e);
    }
    app.insert_resource(Target::default()).add_systems(Update, (fire.after(base_fire), shells.after(fire)));
    let mut lancer_fired = false;
    for mount in &lib.classes[class].mounts {
        let tower = mount.parent.unwrap();
        let aim = mount.pos;
        let world_aim = app.world().get::<Transform>(hostile).unwrap().transform_point(aim);
        app.world_mut().get_mut::<Transform>(player_ship).unwrap().translation = (world_aim + Vec3::Z * 100.0).with_y(0.0);
        *app.world_mut().resource_mut::<Target>() = Target(Some(hostile), Some(aim), default());
        for _ in 0..1200 {
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.05));
            app.update();
            for shell in app.world_mut().query::<&Shell>().iter(app.world()).filter(|s| s.shooter == player_ship) {
                assert_eq!(shell.bullet, "Bullet_Lancer_Light", "only the front Lancer should fire at base towers");
                lancer_fired = true;
            }
            if app.world().get::<Health>(hostile).unwrap().sections[tower] == 0.0 {
                break;
            }
        }
        assert_eq!(app.world().get::<Health>(hostile).unwrap().sections[tower], 0.0, "Mission 2's torpedo boat must actually destroy {}", lib.classes[class].sections[tower].label);
    }
    assert!(lancer_fired, "the front Lancer must engage the selected towers");
    assert_eq!(base_mission_damage(&lib.classes[class], app.world().get::<Health>(hostile).unwrap()), 1.0);
}

#[test]
fn skirmish_bases_shoot_ships_of_other_sides_only() {
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default(), TransformPlugin))
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .init_asset::<bevy::prelude::Image>()
        .add_message::<Sfx>()
        .add_message::<Fired>()
        .init_resource::<Time>()
        .init_resource::<FxLib>()
        .insert_resource(Library::load(assets_dir()))
        .insert_resource(Rng::new(7))
        .insert_resource(Look { mesh: default(), shell: default(), enemy_shell: default() })
        .init_resource::<crate::environment::Sensors>()
        .add_systems(PreUpdate, crate::environment::refresh_sensors)
        .add_systems(Update, base_fire);
    let assets = app.world().resource::<AssetServer>().clone();
    let lib = Library::load(assets_dir());
    let class = lib.index("Base_SmugglersCamp");
    let spawn_base = |app: &mut App, side: Side, x: f32| {
        let id = app.world_mut().spawn((Transform::from_xyz(x, -27.0, 0.0), crate::campaign::MissionBase { id: 1, owner: format!("base{x}") }, BaseSide(side))).id();
        let states = spawn_guns(&mut app.world_mut().commands(), &assets, &lib, id, class);
        app.world_mut().entity_mut(id).insert((ArmedBase { class }, Health::new(&lib.classes[class]), states));
        app.world_mut().flush();
        id
    };
    let hostile = spawn_base(&mut app, Side::Enemy, 0.0);
    spawn_base(&mut app, Side::Player, 40.0);
    let c = lib.index("Ship_Navy_Torpedo");
    let ship = Ship {
        class: c,
        side: Side::Player,
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
    };
    app.world_mut().spawn((Transform::from_xyz(0.0, 0.0, -120.0), ship, Health::new(&lib.classes[c]), Fires::default(), PlayerShip));
    for _ in 0..300 {
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.05));
        app.update();
    }
    let shooters: Vec<Entity> = app.world_mut().query::<&Shell>().iter(app.world()).map(|s| s.shooter).collect();
    assert!(!shooters.is_empty() && shooters.iter().all(|s| *s == hostile), "only the base of the other side opened fire");
}

#[test]
fn a_volcano_lobs_one_aimed_shell_at_a_time() {
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default(), TransformPlugin))
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .init_asset::<bevy::prelude::Image>()
        .add_message::<Sfx>()
        .add_message::<Fired>()
        .init_resource::<Time>()
        .init_resource::<FxLib>()
        .insert_resource(Library::load(assets_dir()))
        .insert_resource(Rng::new(7))
        .insert_resource(Look { mesh: default(), shell: default(), enemy_shell: default() })
        .init_resource::<crate::environment::Sensors>()
        .add_systems(PreUpdate, crate::environment::refresh_sensors)
        .add_systems(Update, base_fire);
    let assets = app.world().resource::<AssetServer>().clone();
    let lib = Library::load(assets_dir());
    let class = lib.index("Island_Volcano_01");
    let spawn_base = |app: &mut App, side: Side, x: f32| {
        let id = app.world_mut().spawn((Transform::from_xyz(x, -27.0, 0.0), crate::campaign::MissionBase { id: 1, owner: format!("base{x}") }, BaseSide(side))).id();
        let states = spawn_guns(&mut app.world_mut().commands(), &assets, &lib, id, class);
        app.world_mut().entity_mut(id).insert((ArmedBase { class }, Health::new(&lib.classes[class]), states));
        app.world_mut().flush();
        id
    };
    spawn_base(&mut app, Side::Enemy, 0.0);
    spawn_base(&mut app, Side::Player, 40.0);
    let c = lib.index("Ship_Navy_Torpedo");
    let ship = Ship {
        class: c,
        side: Side::Player,
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
    };
    app.world_mut().spawn((Transform::from_xyz(0.0, 0.0, -120.0), ship, Health::new(&lib.classes[c]), Fires::default(), PlayerShip));
    for _ in 0..200 {
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.05));
        app.update();
    }
    let shells: Vec<Option<Vec3>> = app.world_mut().query::<&Shell>().iter(app.world()).map(|s| s.aim).collect();
    assert_eq!(shells.len(), 1, "six mounts take turns: one shell per cycle");
    assert!(shells[0].is_some(), "a lob shell bursts at its aim height, not through the sea");
}

#[test]
fn the_command_ship_holds_fire_until_an_enemy_is_clicked() {
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default(), TransformPlugin))
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .init_asset::<bevy::prelude::Image>()
        .add_message::<Sfx>()
        .add_message::<Fired>()
        .add_message::<Hit>()
        .add_message::<Destroyed>()
        .add_message::<Detonate>()
        .add_message::<crate::decals::Scar>()
        .init_resource::<Time>()
        .init_resource::<FxLib>()
        .insert_resource(Library::load(assets_dir()))
        .insert_resource(Rng::new(7))
        .insert_resource(Look { mesh: default(), shell: default(), enemy_shell: default() })
        .insert_resource(Target::default())
        .init_resource::<crate::environment::Sensors>()
        .add_systems(PreUpdate, crate::environment::refresh_sensors)
        .add_systems(Update, (fire, shells.after(fire)));
    let assets = app.world().resource::<AssetServer>().clone();
    let lib = Library::load(assets_dir());
    let c = lib.index("Ship_Navy_Frigate");
    let ship = |side| Ship {
        class: c,
        side,
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
    };
    let spawn = |app: &mut App, side, x: f32| {
        let e = app.world_mut().spawn((Transform::from_xyz(x, 0.0, 0.0), ship(side), Health::new(&lib.classes[c]), Fires::default())).id();
        let states = spawn_guns(&mut app.world_mut().commands(), &assets, &lib, e, c);
        app.world_mut().entity_mut(e).insert(states);
        app.world_mut().flush();
        e
    };
    let me = spawn(&mut app, Side::Player, 0.0);
    app.world_mut().entity_mut(me).insert(PlayerShip);
    let enemy = spawn(&mut app, Side::Enemy, 60.0);
    let run = |app: &mut App| {
        for _ in 0..200 {
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.05));
            app.update();
        }
        app.world_mut().query::<&Shell>().iter(app.world()).any(|s| s.shooter == me)
    };
    assert!(!run(&mut app), "no auto-engage: an enemy in range is not a target until clicked");
    *app.world_mut().resource_mut::<Target>() = Target(Some(enemy), None, default());
    assert!(run(&mut app), "the clicked enemy is engaged");
}

#[test]
fn the_command_ship_damages_a_clicked_hostile_skirmish_base() {
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default(), TransformPlugin))
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .init_asset::<bevy::prelude::Image>()
        .add_message::<Sfx>()
        .add_message::<Fired>()
        .add_message::<Hit>()
        .add_message::<Destroyed>()
        .add_message::<Detonate>()
        .add_message::<crate::decals::Scar>()
        .init_resource::<Time>()
        .init_resource::<FxLib>()
        .insert_resource(Library::load(assets_dir()))
        .insert_resource(Rng::new(7))
        .insert_resource(Look { mesh: default(), shell: default(), enemy_shell: default() })
        .insert_resource(Target::default())
        .init_resource::<crate::environment::Sensors>()
        .add_systems(PreUpdate, crate::environment::refresh_sensors)
        .add_systems(Update, (fire, shells.after(fire)));
    let assets = app.world().resource::<AssetServer>().clone();
    let lib = Library::load(assets_dir());
    let c = lib.index("Ship_Navy_Frigate");
    let ship = |side| Ship {
        class: c,
        side,
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
    };
    let spawn = |app: &mut App, side, x: f32| {
        let e = app.world_mut().spawn((Transform::from_xyz(x, 0.0, 0.0), ship(side), Health::new(&lib.classes[c]), Fires::default())).id();
        let states = spawn_guns(&mut app.world_mut().commands(), &assets, &lib, e, c);
        app.world_mut().entity_mut(e).insert(states);
        app.world_mut().flush();
        e
    };
    let me = spawn(&mut app, Side::Player, 0.0);
    app.world_mut().entity_mut(me).insert(PlayerShip);
    let class = lib.index("Base_PirateMP");
    let base = app.world_mut().spawn((Transform::from_xyz(120.0, -27.0, 0.0), crate::campaign::MissionBase { id: 1, owner: "base1".into() }, BaseSide(Side::Enemy))).id();
    let states = spawn_guns(&mut app.world_mut().commands(), &assets, &lib, base, class);
    app.world_mut().entity_mut(base).insert((ArmedBase { class }, Health::new(&lib.classes[class]), states));
    app.world_mut().flush();
    *app.world_mut().resource_mut::<Target>() = Target(Some(base), None, default());
    for _ in 0..600 {
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.05));
        app.update();
    }
    let hp = app.world().get::<Health>(base).unwrap();
    assert!(base_mission_damage(&lib.classes[class], hp) > 0.0, "shells at the clicked base must damage it");
}

#[test]
fn ship_hit_rules() {
    let lib = Library::load(assets_dir());
    let class = &lib.classes[lib.index("Ship_Navy_Cutter")];
    let stern = class.sections.iter().position(|s| s.vital && s.core_pct == 45.0).unwrap();
    let sure_fire = [10.0, 10.0, 0.0, 1.0, 3.0];
    let mut rng = Rng::new(7);
    // Invulnerable: no damage, no fire, no core loss.
    let mut hp = Health::new(class);
    let out = apply_ship_hit(class, &mut hp, stern, sure_fire, 1.0, true, &mut rng);
    assert!(out.fire.is_none() && !out.lost && hp.sections[stern] == class.sections[stern].hp && hp.core == class.core_hp);
    // A live section takes damage and starts one fire, but not a second while one burns there.
    let out = apply_ship_hit(class, &mut hp, stern, sure_fire, 0.0, true, &mut rng);
    assert_eq!((out.fire, out.lost, hp.sections[stern]), (Some(3.0), false, class.sections[stern].hp - 10.0));
    assert!(apply_ship_hit(class, &mut hp, stern, sure_fire, 0.0, false, &mut rng).fire.is_none());
    // The killing hit reports the loss, and a hit on the destroyed vital section bleeds the core (10 x 45% = 4.5, rounded).
    hp.sections[stern] = 5.0;
    let out = apply_ship_hit(class, &mut hp, stern, sure_fire, 0.0, true, &mut rng);
    assert!(out.lost && out.fire.is_none() && hp.sections[stern] == 0.0);
    let core = hp.core;
    let out = apply_ship_hit(class, &mut hp, stern, sure_fire, 0.0, true, &mut rng);
    assert!(!out.lost && hp.core < core);
}

#[test]
fn segment_distance_clamps_to_the_ends() {
    let (a, b) = (Vec3::ZERO, Vec3::X * 10.0);
    assert_eq!(segment_distance(Vec3::new(5.0, 3.0, 0.0), a, b), 3.0);
    assert_eq!(segment_distance(Vec3::new(-4.0, 3.0, 0.0), a, b), 5.0);
    assert_eq!(segment_distance(Vec3::Y, a, a), 1.0);
}

#[test]
fn core_bleed_scales_with_destroyed_vital_sections() {
    let lib = Library::load(assets_dir());
    let class = &lib.classes[lib.index("Ship_Navy_Cutter")];
    let mut hp = Health::new(class);
    assert_eq!(core_bleed(class, &hp, 40.0), 0.0);
    let stern = class.sections.iter().position(|s| s.vital && s.core_pct == 45.0).unwrap();
    hp.sections[stern] = 0.0;
    assert_eq!(core_bleed(class, &hp, 40.0), 18.0); // 40 x 45%
}

#[test]
fn bank_recharge_follows_the_slowest_live_gun() {
    let lib = Library::load(assets_dir());
    let ci = lib.index("Ship_Navy_Cutter");
    let class = &lib.classes[ci];
    let bank = class.mounts[0].bank;
    let gun = |name: &str, cooldown| GunState {
        gun: name.into(),
        root: Entity::PLACEHOLDER,
        cooldown,
        burst_left: 0,
        burst_timer: 0.0,
        base: Entity::PLACEHOLDER,
        barrel: Entity::PLACEHOLDER,
        muzzles: vec![],
        next_muzzle: 0,
        yaw: 0.0,
        pitch: 0.0,
        aligned: false,
        in_range: false,
        dev: 0.0,
    };
    let health = Health::new(class);
    let mut g = GunStates(class.mounts.iter().map(|m| gun(&m.gun, 0.0)).collect());
    assert_eq!(bank_recharge(&lib, &g, class, &health, bank), 1.0);
    let reload = lib.guns[&class.mounts[0].gun].reload;
    g.0[0].cooldown = reload / 4.0;
    assert!((bank_recharge(&lib, &g, class, &health, bank) - 0.75).abs() < 1e-4);
    assert_eq!(bank_recharge(&lib, &g, class, &health, 99), 0.0, "no live gun in the bank");
}
#[test]
fn every_gun_has_a_panel_icon_and_pd_is_hidden() {
    let hud: serde_json::Value = crate::data::read_json(&assets_dir().join("hud/hud.json"));
    let (g, _) = load_weapons(assets_dir());
    for name in g.keys() {
        assert!(hud.get(gun_icon(name)).is_some(), "{name} -> {} missing", gun_icon(name));
    }
    assert!(is_indirect("Gun_NovaMortar") && !is_indirect("Gun_CannonBall_Med"));
    assert!(g["Gun_Lancer_PD_Light"].point_defense);
}
#[test]
fn port_and_starboard_sections_sit_on_their_own_side() {
    let (g, _) = load_weapons(assets_dir());
    for name in ["Ship_Pirate_Schooner", "Ship_Navy_Frigate", "Ship_Navy_Cutter"] {
        let c = load_class(assets_dir(), name, &g);
        for s in &c.sections {
            let cx = (s.min.x + s.max.x) / 2.0;
            println!("{name} {} x {:.1}..{:.1}", s.label, s.min.x, s.max.x);
            if s.label.to_lowercase().contains("port") && !s.label.to_lowercase().contains("starboard") {
                assert!(cx < 0.0, "{name} {} on the wrong side", s.label);
            }
            if s.label.to_lowercase().contains("starboard") {
                assert!(cx > 0.0, "{name} {} on the wrong side", s.label);
            }
        }
    }
}
#[test]
fn slow_projectile_intercepts_crossing_and_receding_targets() {
    let d = Vec3::new(0.0, 0.0, -100.0);
    for velocity in [Vec3::ZERO, Vec3::X * 10.0, Vec3::NEG_Z * 10.0, Vec3::Z * 22.0] {
        let t = intercept_time(d, velocity, 22.0).unwrap();
        assert!(((d + velocity * t).length() - 22.0 * t).abs() < 0.001);
    }
    assert_eq!(intercept_time(d, Vec3::NEG_Z * 30.0, 22.0), None);
    assert_eq!(intercept_time(Vec3::ZERO, Vec3::ZERO, 22.0), Some(0.0));
}
#[test]
fn point_defence_wears_a_torpedo_down_instead_of_one_shot_kills() {
    let lib = Library::load(crate::assets_dir());
    let torpedo = lib.bullets["Bullet_Torpedo"].hitpoints;
    let bolts = |gun: &str| {
        let bolt = lib.bullets[&lib.guns[gun].bullet].hitpoints;
        let mut hp = torpedo;
        (1..).find(|_| wear_down(&mut hp, bolt)).unwrap()
    };
    assert_eq!(torpedo, 16.0);
    assert_eq!(bolts("Gun_Lancer_PD_Light"), 16, "light PD bolts (1 HP) need sixteen hits");
    assert_eq!(bolts("Gun_Lancer_PD_Med"), 8, "medium PD bolts (2 HP) need eight hits");
}

#[test]
fn torpedo_guidance_launch_coast_and_moving_target_intercept() {
    let lib = Library::load(assets_dir());
    let physics = lib.bullets["Bullet_Torpedo"].physics.as_ref().unwrap();
    assert!((physics.max_speed - 20.833334).abs() < 0.001);
    let (mut p, mut v, mut spin) = (Vec3::ZERO, Vec3::NEG_Z * 22.0, 0.0);
    let target_class = &lib.classes[lib.index("Ship_Pirate_Schooner")];
    let target_rotation = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2);
    let mut target = Vec3::new(15.0, target_class.homing_point.y, -80.0);
    let target_velocity = Vec3::X * 4.0;
    p = guide_torpedo(p, &mut v, &mut spin, 0.0, Some((target, target_velocity)), physics, 0.5);
    assert_eq!(p.x, 0.0, "no steering during launch coast");
    assert_eq!(p.y, 0.0);
    let mut closest = f32::INFINITY;
    let mut hit = false;
    for i in 0..900 {
        let dt = 1.0 / 60.0;
        target += target_velocity * dt;
        let previous = p;
        p = guide_torpedo(p, &mut v, &mut spin, 0.5 + i as f32 * dt, Some((target, target_velocity)), physics, dt);
        let inverse = Transform::from_translation(target - target_rotation * target_class.homing_point).with_rotation(target_rotation).to_matrix().inverse();
        hit |= target_class.sections.iter().any(|s| seg_mesh(inverse.transform_point3(previous), inverse.transform_point3(p), s).is_some());
        closest = closest.min(p.distance(target));
        assert!(p.is_finite() && v.is_finite() && spin.is_finite());
    }
    assert!(hit, "torpedo must intersect the moving target's authored hull; closest homing-point distance: {closest}");
    let old_y = p.y;
    let next = guide_torpedo(p, &mut v, &mut spin, 16.0, None, physics, 0.1);
    assert_eq!(next.y, old_y, "lost target must stop altitude tracking");
}

#[test]
fn model_hardpoints_are_loaded_in_bevy_axes() {
    let lib = Library::load(assets_dir());
    let class = &lib.classes[lib.index("Ship_Navy_Torpedo")];
    assert!((class.homing_point.y - 0.2275).abs() < 0.001);
    assert!(class.tow_points[0].z > 0.0 && class.tow_points[1].z < 0.0);
}

#[test]
fn torpedo_aim_and_launch_do_not_inherit_shooter_motion() {
    let offset = Vec3::new(15.0, 3.0, -80.0);
    let target_velocity = Vec3::X * 5.0;
    let aimed = aim_offset(offset, target_velocity, 22.0, true);
    let velocity = launch_velocity(aimed.normalize(), 22.0, true);
    assert!((velocity.length() - 22.0).abs() < 1e-5);
    assert_eq!(velocity.y, 0.0);
    let time = intercept_time(offset.with_y(0.0), target_velocity, 22.0).unwrap();
    assert!((velocity * time - (offset.with_y(0.0) + target_velocity * time)).length() < 1e-4);
    let unreachable = aim_offset(offset, Vec3::NEG_Z * 30.0, 22.0, true);
    assert_eq!(unreachable, offset.with_y(0.0), "unreachable targets do not produce runaway lead");
}

/// Retail damage control (refit-resupply.md#crew-damage-control): riggers mend the most damaged sail +3 per successful
/// roll; engineers mend the most damaged hull by floor(10 x mean engineering x material factor).
#[test]
fn crew_damage_control_follows_riggers_and_engineers() {
    let lib = Library::load(assets_dir());
    let class = &lib.classes[lib.index("Ship_Navy_ManOWar")];
    let full: Vec<f32> = class.sections.iter().map(|s| s.hp).collect();
    let sails: Vec<usize> = (0..full.len()).filter(|&i| class.sections[i].material.contains("Cloth")).collect();
    let hulls: Vec<usize> = (0..full.len()).filter(|i| !sails.contains(i)).collect();
    let mut hp = full.clone();
    hp[sails[0]] -= 30.0;
    hp[sails[1]] *= 0.5; // most damaged sail
    hp[sails[2]] = 0.0; // destroyed sails stay down
    hp[hulls[1]] -= 50.0;
    hp[hulls[2]] *= 0.5; // most damaged hull
    let mut always = || 0.0;
    // Run 10: eight riggers averaging 8.25 of 10, seven successes -> +21 on one sail.
    damage_control(class, &mut hp, &[], &[0.9, 0.9, 0.8, 0.8, 0.8, 0.8, 0.8, 0.0], &mut always);
    assert_eq!(hp[sails[1]], full[sails[1]] * 0.5 + 21.0);
    assert_eq!((hp[sails[0]], hp[sails[2]]), (full[sails[0]] - 30.0, 0.0));
    // Run 9: engineers 9, 7, 7 -> c = 0.767 -> +7 on the most damaged wood section.
    damage_control(class, &mut hp, &[0.9, 0.7, 0.7], &[], &mut always);
    assert_eq!(hp[hulls[2]], full[hulls[2]] * 0.5 + 7.0);
    assert_eq!(hp[hulls[1]], full[hulls[1]] - 50.0);
    // A failed roll mends nothing; repairs stop at the maximum.
    let before = hp.clone();
    damage_control(class, &mut hp, &[0.5], &[0.5], &mut || 0.99);
    assert_eq!(hp, before);
    let mut nearly = full.clone();
    nearly[sails[3]] -= 1.0;
    damage_control(class, &mut nearly, &[], &[1.0; 8], &mut always);
    assert_eq!(nearly, full);
}

/// Retail fire-fighting (docs/re/effects.md#fire): each tick the crew puts out the sum of every member's leadership; an
/// unmanned gunner, engineer or rigger post counts the first mates again; other empty posts add nothing.
#[test]
fn fire_fighting_sums_crew_leadership() {
    let hands = |n: usize| vec![0.1; n];
    // Default Cutter (5 gunners, captain, first mate, navigator, engineer, 2 riggers, fighter, no lookout): 12 x 0.1.
    let cutter = [hands(5), hands(1), hands(1), hands(1), hands(1), hands(2), hands(1), vec![]];
    assert!((fire_fighting(&cutter) - 1.2).abs() < 1e-5);
    // Empty gunner, engineer and rigger posts count the first mate (0.9) again; empty navigator/fighter/lookout posts add nothing.
    let short = [vec![], vec![0.5], vec![0.9], vec![], vec![], vec![], vec![], vec![]];
    assert!((fire_fighting(&short) - (0.5 + 0.9 * 4.0)).abs() < 1e-5);
    assert_eq!(fire_fighting(&[vec![1.5], vec![], vec![], vec![], vec![], vec![], vec![], vec![]]), 1.0);
}

/// Retail shells (docs/re/gunnery.md#shell-gravity): gravity MuzzleSpeed^2 / (sqrt(2) * MaximumRange) per gun, launch speed
/// MuzzleSpeed in the world frame whatever the shooter's motion, lead from the target's velocity alone.
#[test]
fn shells_match_measured_retail_ballistics() {
    let (guns, _) = load_weapons(assets_dir());
    // gun, measured gravity (median of repeated retail shells)
    for (gun, measured) in [
        ("Gun_StarMortar", 1.263),
        ("Gun_CannonBall_Heavy", 3.231),
        ("Gun_CannonBall_Med", 4.184),
        ("Gun_Carronade_Med", 4.908),
        ("Gun_CannonBall_Light", 6.269),
        ("Gun_Carronade_Light", 6.355),
        ("Gun_Gattling_Light", 28.5),
    ] {
        let g = guns[gun].shell_gravity();
        assert!((g / measured - 1.0).abs() < 0.015, "{gun}: {g} vs measured {measured}");
    }
    assert_eq!(guns["Gun_Lancer_PD_Light"].shell_gravity(), 0.0, "lancers fly straight");
    // Burst cycle (start to start): gattling 3 s + 7 x 0.15 s, measured 3.98-4.17 s; medium cannon 9.0 s, measured 8.96-8.99 s.
    assert!((guns["Gun_Gattling_Light"].burst_cycle() - 4.05).abs() < 1e-4);
    assert_eq!(guns["Gun_CannonBall_Med"].burst_cycle(), 9.0);
    let v = launch_velocity(Vec3::NEG_Z, 40.0, false);
    assert_eq!(v, Vec3::NEG_Z * 40.0, "no shooter velocity added");
    let d = Vec3::new(0.0, 0.0, -100.0);
    assert_eq!(aim_offset(d, Vec3::X * 5.0, 40.0, false), d + Vec3::X * 12.5);
    // A medium cannon aimed by `aim` lands on a point 200 units off at the measured gravity.
    let g = guns["Gun_CannonBall_Med"].shell_gravity();
    let target = Vec3::new(120.0, -3.0, -160.0);
    let (mut p, mut vel, dt) = (Vec3::ZERO, aim(target, 40.0, g) * 40.0, 0.0005);
    while Vec2::new(p.x, p.z).length() < Vec2::new(target.x, target.z).length() {
        vel.y -= g * dt;
        p += vel * dt;
    }
    assert!(p.distance(target) < 0.3, "landed {p} for {target}");
}

#[test]
fn retail_torpedo_arc_body_and_hull_collision() {
    let (guns, bullets) = load_weapons(assets_dir());
    let torpedo = load_class(assets_dir(), "Ship_Navy_Torpedo", &guns);
    let launcher = torpedo.mounts.iter().find(|m| m.gun == "Gun_Torpedo").unwrap();
    assert!((launcher.arc / 2.0 - 22.5f32.to_radians()).abs() < 0.00001);
    assert!(10.0f32.to_radians() < launcher.arc / 2.0);
    let glb = bullets["Bullet_Torpedo"].glb.as_ref().unwrap();
    assert!(assets_dir().join(crate::data::model(glb)).is_file());
    let target = load_class(assets_dir(), "Ship_Pirate_Schooner", &guns);
    let start = target.centre + Vec3::Z * 100.0;
    assert!(target.sections.iter().any(|s| seg_mesh(start, target.centre, s).is_some()));
}

/// The lay-mines routine (docs/re/gunnery.md#mine-laying): 10 s spells, 20 s apart while moving, back to back with an enemy
/// within 50 units, never without a hostile ship.
#[test]
fn mine_laying_spells() {
    let run = |hostile: bool, speed: f32, nearest: f32, secs: f32| {
        let mut m = MineLaying::default();
        (0..(secs * 10.0) as usize).map(|_| m.step(0.1, hostile, speed, nearest)).collect::<Vec<_>>()
    };
    let at = |v: &[bool], s: f32| v[(s * 10.0) as usize - 1];
    let moving = run(true, 5.0, f32::MAX, 70.0);
    assert!(!at(&moving, 19.8) && at(&moving, 20.3) && at(&moving, 29.8) && !at(&moving, 30.5));
    assert!(!at(&moving, 49.8) && at(&moving, 50.3) && !at(&moving, 61.0));
    assert!(run(true, 0.5, f32::MAX, 70.0).iter().all(|on| !on), "a still ship with no enemy close lays nothing");
    let close = run(true, 0.0, 30.0, 25.0);
    assert!(at(&close, 0.1) && at(&close, 10.0) && at(&close, 10.3) && at(&close, 25.0));
    assert!(run(false, 5.0, 30.0, 70.0).iter().all(|on| !on));
}

/// A ship frozen by an NIS (speed 0) keeps its engine order but shows no exhaust; it shows once the ship moves.
#[test]
fn engine_exhaust_needs_motion_not_just_an_order() {
    let mut app = App::new();
    app.init_resource::<FxLib>().insert_resource(Library::load(assets_dir())).add_systems(Update, engine_rate);
    let lib = Library::load(assets_dir());
    let c = lib.index("Ship_Navy_Torpedo");
    let ship = Ship {
        class: c,
        side: Side::Player,
        heading: 0.0,
        speed: 0.0,
        ang_vel: 0.0,
        order: 2,
        waypoints: vec![],
        goal: None,
        cap: 1.0,
        throttle: None,
        sail_energy: 1.0,
        crew_speed: 1.0,
        crew_turn: 1.0,
    };
    let s = app.world_mut().spawn((Transform::default(), ship, Health::new(&lib.classes[c]))).id();
    let fx = app.world_mut().resource_scope(|world, fxl: Mut<FxLib>| {
        spawn_engines(&mut world.commands(), &fxl, &lib, s, c, Transform::default());
        world.flush();
        world.query_filtered::<Entity, With<Engine>>().iter(world).next().expect("engine effect")
    });
    app.update();
    assert!(!app.world().get::<fx::Fx>(fx).unwrap().emitting, "stopped ship with an engine order");
    app.world_mut().get_mut::<Ship>(s).unwrap().speed = 5.0;
    app.update();
    assert!(app.world().get::<fx::Fx>(fx).unwrap().emitting);
}

/// Side-facing point-defence lancers face away from the keel: they hang upside down (rolled about the bow axis) and used to
/// face inward.
#[test]
fn side_point_defence_faces_outward() {
    let (guns, _) = load_weapons(assets_dir());
    for ship in ["Ship_Navy_ManOWar", "Ship_Navy_Cutter", "Ship_Navy_Heavy_Escort", "Ship_Pirate_Galleon"] {
        let class = load_class(assets_dir(), ship, &guns);
        let mut side_lancers = 0;
        for m in class.mounts.iter().filter(|m| m.gun.contains("PD") && m.pos.x.abs() > 0.2 && (m.rot * Vec3::NEG_Z).x.abs() > 0.5) {
            side_lancers += 1;
            assert!((m.rot * Vec3::NEG_Z).x * m.pos.x > 0.0, "{ship}: {} at x {} faces {:?}", m.gun, m.pos.x, m.rot * Vec3::NEG_Z);
        }
        assert!(side_lancers >= 2, "{ship}");
    }
}
