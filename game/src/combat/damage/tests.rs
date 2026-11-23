use super::*;

mod swap_tests {
    use super::*;
    use crate::combat::{ArmedBase, Health};

    #[test]
    #[ignore = "needs imported assets"]
    fn guns_of_a_destroyed_section_are_hidden_with_its_intact_mesh() {
        let lib = Library::load(crate::assets_dir());
        let swapped = |c: &crate::data::Class| c.mounts.iter().find_map(|m| m.parent.filter(|p| c.sections[*p].swappable));
        let class = lib.classes.iter().position(|c| swapped(c).is_some()).expect("a class with a mounted swappable section");
        let lost = swapped(&lib.classes[class]).unwrap();
        let mut app = App::new();
        app.insert_resource(Library::load(crate::assets_dir())).add_systems(Update, swap_meshes);
        let mut hp = Health::new(&lib.classes[class]);
        hp.sections[lost] = 0.0;
        let state = |root| crate::combat::gunnery::GunState {
            gun: String::new(),
            root,
            cooldown: 0.0,
            burst_left: 0,
            burst_timer: 0.0,
            base: Entity::PLACEHOLDER,
            barrel: Entity::PLACEHOLDER,
            muzzles: vec![],
            yaw: 0.0,
            pitch: 0.0,
            aligned: false,
            was_aligned: false,
            lob_wait: None,
            in_range: false,
            dev: 0.0,
            beam: Default::default(),
            request: Default::default(),
            rock_ask: false,
        };
        let roots: Vec<Entity> = lib.classes[class].mounts.iter().map(|_| app.world_mut().spawn((crate::combat::gunnery::GunPart, Visibility::default())).id()).collect();
        let guns = crate::combat::gunnery::GunStates(roots.iter().map(|r| state(*r)).collect());
        app.world_mut().spawn((ArmedBase { class }, hp, guns));
        app.update();
        for (m, r) in lib.classes[class].mounts.iter().zip(&roots) {
            let want = if m.parent == Some(lost) { Visibility::Hidden } else { Visibility::Inherited };
            assert_eq!(*app.world().get::<Visibility>(*r).unwrap(), want);
        }
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn a_destroyed_base_part_shows_its_damaged_mesh() {
        let lib = Library::load(crate::assets_dir());
        let class = lib.index("Base_Procyon1");
        let mut app = App::new();
        app.insert_resource(Library::load(crate::assets_dir())).add_systems(Update, swap_meshes);
        let mut hp = Health::new(&lib.classes[class]);
        hp.sections[0] = 0.0; // BaseProcyons_Layer2
        let node = |app: &mut App, parent, name: &str| app.world_mut().spawn((Name::new(name.to_string()), Visibility::default(), ChildOf(parent))).id();
        let base = app.world_mut().spawn((ArmedBase { class }, hp)).id();
        let [whole, hurt, other, other_hurt] =
            ["BaseProcyons_Layer2", "BaseProcyonsDamage_Layer2", "BaseProcyons_Layer3", "BaseProcyonsDamage_Layer3"].map(|n| node(&mut app, base, n));
        let stray = node(&mut app, base, "BaseProcyonsDamage_Layer1");
        let hurt_primitive = node(&mut app, hurt, "BaseProcyonsDamage_Layer2.damage_cutter1.tga");
        app.update();
        let vis = |app: &App, e| *app.world().get::<Visibility>(e).unwrap();
        assert_eq!([vis(&app, whole), vis(&app, hurt)], [Visibility::Hidden, Visibility::Inherited]);
        assert_eq!([vis(&app, other), vis(&app, other_hurt)], [Visibility::Inherited, Visibility::Hidden]);
        assert_eq!(vis(&app, stray), Visibility::Hidden);
        assert_eq!(vis(&app, hurt_primitive), Visibility::Inherited, "a damaged layer's primitives are never hidden on their own");
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn late_nodes_are_swapped_at_once_and_a_repair_swaps_back() {
        let lib = Library::load(crate::assets_dir());
        let class = lib.index("Base_Procyon1");
        let mut app = App::new();
        app.insert_resource(Library::load(crate::assets_dir())).add_systems(Update, swap_meshes);
        let mut hp = Health::new(&lib.classes[class]);
        hp.sections[0] = 0.0; // BaseProcyons_Layer2
        let base = app.world_mut().spawn((ArmedBase { class }, hp)).id();
        app.update();
        app.update();
        // The scene finishes loading after the section was already destroyed: the new nodes show the right mesh straight away.
        let node = |app: &mut App, name: &str| app.world_mut().spawn((Name::new(name.to_string()), Visibility::default(), ChildOf(base))).id();
        let [whole, hurt] = ["BaseProcyons_Layer2", "BaseProcyonsDamage_Layer2"].map(|n| node(&mut app, n));
        app.update();
        let vis = |app: &App, e| *app.world().get::<Visibility>(e).unwrap();
        assert_eq!([vis(&app, whole), vis(&app, hurt)], [Visibility::Hidden, Visibility::Inherited]);
        let full = lib.classes[class].sections[0].hp;
        app.world_mut().get_mut::<Health>(base).unwrap().sections[0] = full;
        app.update();
        assert_eq!([vis(&app, whole), vis(&app, hurt)], [Visibility::Inherited, Visibility::Hidden]);
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn a_destroyed_unswappable_section_keeps_its_mesh() {
        // Station Montgomery's island is a 10 HP section with swappable=0: shot down, the island stays.
        let lib = Library::load(crate::assets_dir());
        let class = lib.index("Base_Montgomery");
        let island = lib.classes[class].sections.iter().position(|s| s.layer == "WayStationMontgomery_Layer1").unwrap();
        assert!(!lib.classes[class].sections[island].swappable, "re-run the asset importer");
        let mut app = App::new();
        app.insert_resource(Library::load(crate::assets_dir())).add_systems(Update, swap_meshes);
        let mut hp = Health::new(&lib.classes[class]);
        hp.sections[island] = 0.0;
        let base = app.world_mut().spawn((ArmedBase { class }, hp)).id();
        let [whole, hurt] =
            ["WayStationMontgomery_Layer1", "WayStationMontgomeryDamage_Layer1"].map(|n| app.world_mut().spawn((Name::new(n), Visibility::default(), ChildOf(base))).id());
        app.update();
        let vis = |e| *app.world().get::<Visibility>(e).unwrap();
        assert_eq!([vis(whole), vis(hurt)], [Visibility::Inherited, Visibility::Hidden]);
    }
}

mod level_tests {
    use super::damage_level;

    #[test]
    fn smoke_bands_follow_the_originals_remaining_share() {
        let levels = [1.0, 0.8, 0.7, 0.5, 0.4, 0.25, 0.1, 0.0].map(damage_level);
        assert_eq!(levels, [0, 0, 1, 1, 2, 2, 3, 4]);
        assert_eq!(damage_level(0.75), 3, "exactly 0.75 is in no band and falls to Heavy");
    }
}

mod fire_tests {
    use super::*;

    #[test]
    fn a_fire_on_the_humans_ship_takes_the_difficulty_factor_and_whole_hit_points() {
        assert_eq!(fire_damage(1.0, true, 1), 0.0, "measured on the original: a 1.0 fire takes nothing on Normal");
        assert_eq!([0, 1, 2].map(|d| fire_damage(8.0, true, d)), [2.0, 4.0, 8.0]);
        assert_eq!(fire_damage(5.0, true, 1), 2.0, "half to even");
        assert_eq!(fire_damage(8.0, false, 1), 8.0, "other ships: not scaled");
        assert_eq!(fire_damage(0.5, false, 1), 0.0, "whole hit points");
    }

    /// A fire whose section is gone stops on the 6 s tick that finds it, is cut loose on the next and deleted on the one after.
    #[test]
    #[ignore = "needs imported assets"]
    fn the_scripted_percent_hit_takes_sections_through_the_hit_routine_and_leaves_the_core() {
        let lib = Library::load(crate::assets_dir());
        let class = &lib.classes[lib.index("Ship_Navy_Cutter")];
        let lost = |victim: Side, p: f32| {
            let mut hp = Health::new(class);
            fraction_hit(class, &mut hp, p, victim, 0.0);
            (class.sections.iter().map(|s| s.hp).sum::<f32>() - hp.sections.iter().sum::<f32>(), hp.core)
        };
        let (enemy, core) = lost(Side::Enemy, 0.5);
        assert_eq!(core, class.core_hp, "half of every section leaves the core whole");
        assert!(enemy > 0.0);
        assert!(lost(Side::Player, 0.5).0 < enemy, "the human's ship takes the difficulty factor, computer ships do not");
        assert!(lost(Side::Enemy, 1.0).1 <= 0.0, "every section hit for all its hit points takes the core down through the vital ones");
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn a_dead_fire_changes_state_only_on_the_six_second_ticks() {
        let lib = Library::load(crate::assets_dir());
        let class = lib.index("Ship_Navy_Cutter");
        let mut app = App::new();
        app.init_resource::<Time>().add_systems(Update, fires);
        let mut hp = Health::new(&lib.classes[class]);
        hp.sections[0] = 0.0;
        let ship = crate::Ship { heading: 0.0, ..crate::Ship::new(class, crate::Side::Enemy) };
        app.insert_resource(lib);
        let fire = Fire { sec: 0, intensity: 5.0, state: FireState::Burning, fx: None };
        let e = app.world_mut().spawn((Transform::default(), ship, hp, Fires { list: vec![fire], tick: 0.0 })).id();
        let states = |app: &App| app.world().get::<Fires>(e).unwrap().list.iter().map(|f| f.state).collect::<Vec<_>>();
        let step = |app: &mut App, secs: f32| {
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(secs));
            app.update();
        };
        step(&mut app, 5.0);
        assert_eq!(states(&app), [FireState::Burning], "no tick yet: nothing changes");
        step(&mut app, 1.5);
        assert_eq!(states(&app), [FireState::Out], "the tick finds the dead fire");
        step(&mut app, 1.0);
        assert_eq!(states(&app), [FireState::Out], "between ticks it stays put");
        step(&mut app, 5.5);
        assert_eq!(states(&app), [FireState::Loose]);
        step(&mut app, 6.0);
        assert!(states(&app).is_empty(), "deleted on the third tick");
    }
}
