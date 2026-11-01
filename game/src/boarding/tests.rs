use super::*;

const SCHOONER: [u32; 8] = [4, 1, 1, 1, 0, 2, 2, 1]; // Ship_Pirate_Schooner 'Max number of ...'

#[test]
fn strengths_follow_the_crew_counts_and_skill() {
    let green = Strength::ship(SCHOONER, Skill::Green.member());
    assert_eq!((green.fighters, green.crew, green.attack, green.defend), (2, 10, 7, 12));
    let elite = Strength::ship(SCHOONER, Skill::Elite.member());
    assert_eq!((elite.fighters, elite.crew, elite.attack, elite.defend), (8, 40, 28, 48));
    assert_eq!(Strength::ship([0; 8], 0.3).defend, 0, "no crew, no defence");
    assert_eq!(Strength::base(100).defend, 100);
}

#[test]
fn casualties_alternate_by_side_and_end_when_a_side_is_out() {
    let (mut b, mut t) = (Strength::ship(SCHOONER, 0.3), Strength::base(2));
    let attack = b.attack;
    assert!(!casualty(&mut b, &mut t, false), "target 2 -> 1");
    assert_eq!(t.defend, 1);
    assert_eq!(b.attack, attack, "a base has no attack to lose");
    assert!(!casualty(&mut b, &mut t, true));
    assert_eq!((b.attack, b.defend), (attack - 1, 36 - 1));
    assert!(casualty(&mut b, &mut t, false), "target 1 -> 0 ends the fight");
    let mut lost = Strength { attack: 1, ..b };
    assert!(casualty(&mut lost, &mut t, true), "the boarder's last attack point");
    assert_eq!(settle(&mut lost, &mut Strength::ship(SCHOONER, 0.3)), Outcome::Lost);
}

#[test]
fn settling_reads_the_totals_before_normalising_and_takes_losses_from_fighters_first() {
    let (mut b, mut t) = (Strength::ship(SCHOONER, 0.3), Strength::ship(SCHOONER, 0.1)); // defend 36 vs 12
    t.defend = 0; // target wiped out
    b.defend -= 4; // the boarder lost four hands: 3 fighters... fighters 6, crew 30 -> loss 4 from the fighters
    assert_eq!(settle(&mut b, &mut t), Outcome::Won);
    assert_eq!((b.fighters, b.crew, b.defend, b.attack), (2, 30, 32, 17));
    // more losses than fighters spill into the crew
    let mut c = Strength::ship(SCHOONER, 0.3);
    c.defend -= 10;
    c.normalise();
    assert_eq!((c.fighters, c.crew), (0, 26));
    // a draw
    assert_eq!(settle(&mut Strength { attack: 5, ..Strength::base(0) }, &mut Strength::base(5)), Outcome::Draw);
}

#[test]
fn interval_scales_with_the_weaker_side() {
    let (b, t) = (Strength::ship(SCHOONER, 0.3), Strength::base(10));
    assert!((interval(&b, &t, 0.0) - 9.0 / 20.0).abs() < 1e-6);
    assert!((interval(&b, &t, 1.0) - 10.5 / 20.0).abs() < 1e-6);
    assert_eq!(interval(&b, &Strength::base(0), 0.5), 0.0);
}

#[test]
#[ignore = "needs imported assets"]
fn exported_data_has_crews_and_base_points() {
    assert_eq!(counts("Ship_Pirate_Schooner"), SCHOONER);
    assert_eq!(points("Ship_Pirate_Schooner").len(), 2);
    assert_eq!(points("Base_QueensPalace").len(), 4);
    assert_eq!(counts("Ship_Nope"), [0; 8]);
}

#[derive(Resource, Default)]
struct Got(Vec<(String, String)>);
fn collect(mut msgs: MessageReader<Captured>, mut got: ResMut<Got>) {
    got.0.extend(msgs.read().map(|c| (c.by.clone(), c.victim.clone())));
}
/// Stand-in for `sail`: moves every ship by its velocity (no engine drag, collisions or turning).
fn drive(time: Res<Time>, mut ships: Query<(&Ship, &mut Transform, &Drift)>) {
    for (s, mut tf, d) in &mut ships {
        let v = tf.rotation * Vec3::NEG_Z * s.speed + d.0;
        tf.translation += v * time.delta_secs();
    }
}
/// A computer Barque with the player's ship on its beam inside harpoon reach, once fired at, grapples it on its own, winches in and boards
/// (Elite crew); with a boarding strength of 10 it grapples but never sends boarders.
#[test]
#[ignore = "needs imported assets"]
fn computer_captain_grapples_and_boards_its_target() {
    for (attack, boards) in [(None, true), (Some(AI_MIN_ATTACK), false)] {
        let mut app = app();
        app.add_message::<crate::combat::Fired>().add_systems(Update, ai_boarding.after(winch).before(fights));
        let me = spawn(&mut app, "Ship_Navy_Cutter", Side::Player, Vec3::ZERO, Skill::Elite);
        app.world_mut().entity_mut(me).insert(PlayerShip);
        let foe = spawn(&mut app, "Ship_Pirate_Barque", Side::Enemy, Vec3::new(40.0, 0.0, 0.0), Skill::Elite);
        app.world_mut().entity_mut(foe).insert(crate::ship::FleetPilot::default()); // skirmish fleets give every ship a pilot
        if let Some(a) = attack {
            app.world_mut().get_mut::<Boardable>(foe).unwrap().strength.attack = a;
        }
        for _ in 0..50 {
            tick(&mut app, 0.1);
        }
        assert!(app.world().get::<Grapple>(foe).is_none(), "an untouched computer ship leaves its target alone");
        app.world_mut().write_message(crate::combat::Fired { shooter: me, target: Some(foe), bullet: String::new() });
        let (mut grappled, mut boarded) = (false, false);
        for _ in 0..600 {
            tick(&mut app, 0.1);
            grappled |= app.world().get::<Grapple>(foe).is_some_and(|g| g.target == me);
            boarded |= app.world().get::<Boarding>(foe).is_some_and(|f| f.target() == me);
        }
        assert!(grappled, "the Barque harpooned its target");
        assert_eq!(boarded, boards, "attack {attack:?}: boarding needs an attacking strength above {AI_MIN_ATTACK}");
    }
}
fn app() -> App {
    let mut app = App::new();
    app.add_message::<BoardMsg>()
        .add_message::<Captured>()
        .add_message::<Settled>()
        .add_message::<Sfx>()
        .insert_resource(Time::<()>::default())
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<BoardStatus>()
        .init_resource::<Got>()
        .insert_resource(Library::load(assets_dir()))
        .insert_resource(Rng::new(7))
        .add_systems(Update, (handle_messages, set_flags, grapple_orders, winch, drive, fights, settled, status, collect).chain());
    app
}
fn tick(app: &mut App, secs: f32) {
    app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(secs));
    app.update();
}
fn spawn(app: &mut App, class: &str, side: Side, at: Vec3, skill: Skill) -> Entity {
    let ci = app.world().resource::<Library>().index(class);
    let c = &app.world().resource::<Library>().classes[ci];
    let health = Health::new(c);
    let ship = Ship {
        class: ci,
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
        net: 1.0,
        avoid: 1.0,
    };
    app.world_mut().spawn((Transform::from_translation(at), ship, health, Drift::default(), Boardable::ship(class, skill, true))).id()
}

#[test]
#[ignore = "needs imported assets"]
fn boarding_a_hostile_skirmish_base_changes_its_side() {
    use crate::combat::{BaseOrigin, BaseSide};
    let mut app = app();
    let pts = points("Base_Procyon1");
    assert!(!pts.is_empty(), "the Procyon base exports boarding points");
    let base = app
        .world_mut()
        .spawn((Transform::default(), MissionBase { id: 225, owner: "base225".into() }, BaseSide(Side::Enemy), BaseOrigin(Side::Enemy), Boardable::base("Base_Procyon1", 2, true)))
        .id();
    let me = spawn(&mut app, "Ship_Navy_Cutter", Side::Player, pts[0], Skill::Elite);
    app.world_mut().entity_mut(me).insert((PlayerShip, crate::ship::FleetMember("Player".into(), 0)));
    tick(&mut app, 0.1);
    app.world_mut().write_message(BoardMsg::Board { boarder: None, target: base });
    for _ in 0..600 {
        tick(&mut app, 0.1);
    }
    assert_eq!(app.world().get::<BaseSide>(base).unwrap().0, Side::Player, "captured: the base fights for the captor now");
    assert_eq!(app.world().get::<BaseOrigin>(base).unwrap().0, Side::Enemy);
    assert!(app.world().get::<crate::ship::FleetMember>(base).is_none(), "a base does not join the captor's fleet roster");
}

#[test]
fn generic_crews_speed_ships_up_by_level() {
    let f: Vec<f32> = [Skill::Green, Skill::Average, Skill::Elite].map(Skill::handling).to_vec();
    assert!(f[0] > 1.0 && f[0] < f[1] && f[1] < f[2] && f[2] < 1.35, "{f:?}");
    // the skirmish computer's measured run-in factor (1.267) lies between Average and Elite
    assert!(f[1] < 1.267 && 1.267 < f[2]);
    assert_eq!(Skill::Players.handling(), 1.0);
    // a Green Barque (sight 500) scans 575, as in the retail save
    assert!((500.0 * Skill::Green.scan() - 575.0).abs() < 1e-3);
}

#[test]
#[ignore = "needs imported assets"]
fn grapple_then_board_captures_the_ship_and_tells_the_rules() {
    let mut app = app();
    let me = spawn(&mut app, "Ship_Navy_Cutter", Side::Player, Vec3::ZERO, Skill::Elite);
    app.world_mut().entity_mut(me).insert(PlayerShip);
    let foe = spawn(&mut app, "Ship_Pirate_Schooner", Side::Enemy, Vec3::new(0.0, 0.0, -60.0), Skill::Green);
    app.world_mut().entity_mut(me).insert(crate::ship::FleetMember("Player".into(), 0));
    app.world_mut().entity_mut(foe).insert(crate::ship::FleetMember("Enemy".into(), 0));
    app.world_mut().get_mut::<Boardable>(foe).unwrap().strength.defend = 3; // a thin crew, so the seeded fight is short
    tick(&mut app, 0.1);
    assert_eq!(app.world().resource::<BoardStatus>().board(foe), (false, false), "no grapple yet: Board greyed");
    assert_eq!(app.world().resource::<BoardStatus>().grapple(foe), (false, true), "the cutter has a harpoon");
    app.world_mut().write_message(BoardMsg::Grapple { grappler: None, target: foe });
    tick(&mut app, 0.1);
    assert!(app.world().get::<Grapple>(me).is_some_and(|g| g.flight > 0.0), "harpoon in the air");
    let surface = |app: &App| {
        let lib = app.world().resource::<Library>();
        let r = |e: Entity| lib.classes[app.world().get::<Ship>(e).unwrap().class].radius;
        app.world().get::<Transform>(me).unwrap().translation.distance(app.world().get::<Transform>(foe).unwrap().translation) - r(me) - r(foe)
    };
    while app.world().get::<Grapple>(me).is_some_and(|g| g.flight > 0.0) {
        tick(&mut app, 0.1);
    }
    let bit = surface(&app);
    for _ in 0..20 {
        tick(&mut app, 0.1);
    }
    assert!((surface(&app) - bit).abs() < 0.5, "a grapple alone does not reel the ships together");
    assert!(app.world().resource::<BoardStatus>().board(foe).1, "grappled: Board enabled");
    app.world_mut().write_message(BoardMsg::Board { boarder: None, target: foe });
    tick(&mut app, 0.1);
    assert!(app.world().get::<Boarding>(me).is_some_and(|f| f.target() == foe && !f.live()), "fight registered, crews not yet across");
    assert_eq!(app.world().resource::<BoardStatus>().board(foe), (true, true), "Board shows Stop");
    for _ in 0..600 {
        tick(&mut app, 0.1);
        if app.world().get::<Boarding>(me).is_some_and(|f| f.live()) {
            break;
        }
    }
    assert!(surface(&app) < SHIP_RANGE, "the registered boarding reeled the hulls together (surface {})", surface(&app));
    for _ in 0..200 {
        tick(&mut app, 0.1);
    }
    assert!(app.world().get::<Boarding>(me).is_none(), "fight over");
    assert_eq!(app.world().get::<Ship>(foe).unwrap().side, Side::Player, "captured");
    assert_eq!(app.world().get::<crate::ship::FleetMember>(foe).unwrap().0, "Player");
    app.insert_resource(crate::ship::Fleets { local: "Player".into(), selected: Some(me), ..default() })
        .insert_resource(crate::Orbit { yaw: 0.0, pitch: 0.0, dist: 60.0, focus: None })
        .init_resource::<crate::combat::Target>();
    crate::ship::reconcile_fleets(app.world_mut());
    assert_eq!(app.world().resource::<crate::ship::Fleets>().roster(), &[me, foe], "capture joins the captor's own roster");
    assert!(app.world().get::<Grapple>(me).is_none(), "grapple released");
    assert_eq!(app.world().resource::<Got>().0, vec![("Player".to_string(), "Enemy".to_string())]);
    assert!(app.world().get::<Boardable>(foe).unwrap().strength.defend > 3, "crewed afresh");
}

#[test]
#[ignore = "needs imported assets"]
fn boarding_needs_a_grapple_and_ends_when_the_ships_drift_apart() {
    let mut app = app();
    let me = spawn(&mut app, "Ship_Navy_Cutter", Side::Player, Vec3::ZERO, Skill::Average);
    app.world_mut().entity_mut(me).insert(PlayerShip);
    let foe = spawn(&mut app, "Ship_Pirate_Schooner", Side::Enemy, Vec3::new(0.0, 0.0, -14.0), Skill::Average);
    tick(&mut app, 0.1);
    assert!(!app.world().resource::<BoardStatus>().board(foe).1, "close, but not grappled");
    app.world_mut().entity_mut(me).insert(Grapple::new(foe, 0.0));
    tick(&mut app, 0.1);
    assert!(app.world().resource::<BoardStatus>().board(foe).1);
    app.world_mut().write_message(BoardMsg::Board { boarder: None, target: foe });
    tick(&mut app, 0.1);
    assert!(app.world().get::<Boarding>(me).is_some());
    app.world_mut().get_mut::<Ship>(foe).unwrap().speed = 8.0; // sails away faster than the relative-speed limit
    app.world_mut().entity_mut(me).remove::<Grapple>();
    tick(&mut app, 0.1);
    assert!(app.world().get::<Boarding>(me).is_none(), "no grapple, no fight");
    app.world_mut().get_mut::<Boardable>(foe).unwrap().on = false;
    app.world_mut().entity_mut(me).insert(Grapple::new(foe, 0.0));
    tick(&mut app, 0.1);
    assert!(!app.world().resource::<BoardStatus>().board(foe).1, "a ship that is not Boardable cannot be boarded");
}

#[test]
#[ignore = "needs imported assets"]
fn a_grapple_does_not_hold_the_ships_and_snaps_when_they_pull_apart() {
    let mut app = app();
    let me = spawn(&mut app, "Ship_Navy_Cutter", Side::Player, Vec3::ZERO, Skill::Average);
    app.world_mut().entity_mut(me).insert(PlayerShip);
    let foe = spawn(&mut app, "Ship_Pirate_Schooner", Side::Enemy, Vec3::new(0.0, 0.0, -40.0), Skill::Average);
    app.world_mut().entity_mut(foe).insert(Grapple::new(me, 0.0));
    tick(&mut app, 0.1);
    app.world_mut().get_mut::<Ship>(me).unwrap().speed = 4.0; // we sail on, bow (-Z) toward the schooner's side
    tick(&mut app, 0.1);
    assert!(app.world().get::<Ship>(me).unwrap().speed > 3.9, "the grapple neither stops nor drags us");
    app.world_mut().get_mut::<Ship>(me).unwrap().speed = -4.0; // and away again
    for _ in 0..20 {
        tick(&mut app, 0.1);
    }
    assert!(app.world().get::<Grapple>(foe).is_none(), "pulled more than 2 units past its length: the line snapped");
    // Grappled on our bow, the schooner circles round to our stern: the line snaps.
    app.world_mut().get_mut::<Ship>(me).unwrap().speed = 0.0;
    app.world_mut().get_mut::<Transform>(me).unwrap().translation = Vec3::ZERO;
    app.world_mut().get_mut::<Transform>(foe).unwrap().translation = Vec3::new(0.0, 0.0, -30.0);
    app.world_mut().entity_mut(foe).insert(Grapple::new(me, 0.0));
    tick(&mut app, 0.1);
    assert!(app.world().get::<Grapple>(foe).is_some());
    app.world_mut().get_mut::<Transform>(foe).unwrap().translation = Vec3::new(5.0, 0.0, 25.0);
    tick(&mut app, 0.1);
    assert!(app.world().get::<Grapple>(foe).is_none(), "crossed to the other side of our hull: the line snapped");
}

/// The computer's harpoon test against the retail runs (docs/re/boarding.md §5): a Barque's beam harpoons reach a ship on its
/// beam inside 45 units but not at the 55.7-unit circle it held round an idle Cutter; a Carrack's bow-facing harpoons never
/// bear on a ship it circles on its beam.
#[test]
#[ignore = "needs imported assets"]
fn ai_harpoons_bear_by_arc_and_long_range() {
    let lib = Library::load(assets_dir());
    let bears = |class: &str, at: Vec3| {
        let ci = lib.index(class);
        let ship = Ship {
            class: ci,
            side: Side::Enemy,
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
            net: 1.0,
            avoid: 1.0,
        };
        harpoon_bears(&lib, &ship, &Health::new(&lib.classes[ci]), None, &Transform::IDENTITY, at)
    };
    assert!(bears("Ship_Pirate_Barque", Vec3::X * 40.0) && bears("Ship_Pirate_Barque", Vec3::NEG_X * 40.0));
    assert!(!bears("Ship_Pirate_Barque", Vec3::X * 55.7) && !bears("Ship_Pirate_Barque", Vec3::NEG_Z * 40.0));
    assert!(!bears("Ship_Pirate_Carrack", Vec3::X * 30.0) && bears("Ship_Pirate_Carrack", Vec3::NEG_Z * 30.0));
    assert!((per_tick(AI_BOARD_CHANCE, 1.0 / 30.0) - 0.2).abs() < 1e-5 && (per_tick(AI_BOARD_CHANCE, 2.0 / 30.0) - 0.36).abs() < 1e-5);
}

#[test]
#[ignore = "needs imported assets"]
fn base_boarding_needs_a_boarding_point_in_reach_and_a_slow_boarder() {
    let lib = Library::load(assets_dir());
    let ci = lib.index("Ship_Navy_Cutter");
    let ship = Ship {
        class: ci,
        side: Side::Enemy,
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
        net: 1.0,
        avoid: 1.0,
    };
    let base_tf = Transform::from_xyz(100.0, 0.0, 0.0);
    let point = Vec3::new(-10.0, 0.0, 0.0); // world (90, 0, 0)
    let at = |x: f32| Transform::from_xyz(x, 0.0, 0.0);
    let range = |x: f32, speed: f32| {
        let mut s = ship.clone();
        s.speed = speed;
        in_range(&lib, &(&at(x), Some(&s), None), &(&base_tf, None, None), &[point])
    };
    let r = lib.classes[ci].radius;
    assert!(range(90.0 - r - 14.0, 0.0), "within 15 of the point");
    assert!(!range(90.0 - r - 16.0, 0.0), "too far");
    assert!(!range(90.0 - r - 14.0, 2.5), "too fast (limit 2)");
}

#[test]
fn island_gunners_aim_by_the_generic_crews_gunnery() {
    // Crew_General_* gunnery 3 / 6 / 8: 1 - 0.8 x gunnery relative to Average.
    assert!((Skill::Green.gunnery_vs_average() - 0.76 / 0.52).abs() < 1e-5);
    assert_eq!(Skill::Average.gunnery_vs_average(), 1.0);
    assert!((Skill::Elite.gunnery_vs_average() - 0.36 / 0.52).abs() < 1e-5);
}
