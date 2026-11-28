use super::ai::{AI_BOARD_CHANCE, AI_MIN_ATTACK};
use super::grapple::{REEL_MIN, SHIP_RANGE, bank_limit, reeled};
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
struct Got(Vec<(crate::names::Owner, String)>);
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
/// (Elite crew); with a boarding strength of 10 it grapples but never sends boarders; with an empty target slot it leaves the cutter alone.
#[test]
#[ignore = "needs imported assets"]
fn computer_captain_grapples_and_boards_its_target() {
    // (its target slot holds the cutter, its boarding strength, it grapples, it boards). An empty slot never grapples: the original has
    // no "the player's ship" fallback (#699).
    for (slot, attack, grapples, boards) in [(true, None, true, true), (true, Some(AI_MIN_ATTACK), true, false), (false, None, false, false)] {
        let mut app = app();
        app.add_message::<crate::combat::Fired>().add_systems(Update, ai_boarding.after(winch_pull).before(fights));
        let me = spawn(&mut app, "Ship_Navy_Cutter", Side::Player, Vec3::ZERO, Skill::Elite);
        app.world_mut().entity_mut(me).insert(PlayerShip);
        let foe = spawn(&mut app, "Ship_Pirate_Barque", Side::Enemy, Vec3::new(40.0, 0.0, 0.0), Skill::Elite);
        app.world_mut().entity_mut(foe).insert(crate::ship::FleetPilot::default()); // skirmish fleets give every ship a pilot
        if slot {
            app.world_mut().entity_mut(foe).insert(crate::ship::TargetSlot::order(Some(me), None));
        }
        if let Some(a) = attack {
            app.world_mut().get_mut::<Boardable>(foe).unwrap().strength.attack = a;
        }
        for _ in 0..50 {
            tick(&mut app, 0.1);
        }
        assert!(held(&mut app, foe).is_empty(), "an untouched computer ship leaves its target alone");
        app.world_mut().write_message(crate::combat::Fired { shooter: me, target: Some(foe), bullet: String::new(), torpedo: false, mine: false });
        let (mut grappled, mut boarded) = (false, false);
        for _ in 0..600 {
            tick(&mut app, 0.1);
            grappled |= held(&mut app, foe).iter().any(|g| g.0 == me);
            boarded |= app.world().get::<Boarding>(foe).is_some_and(|f| f.target() == me);
        }
        assert_eq!(grappled, grapples, "slot {slot}: the Barque harpoons the target in its slot, and only that");
        assert_eq!(boarded, boards, "attack {attack:?}: boarding needs an attacking strength above {AI_MIN_ATTACK}");
    }
}
fn held(app: &mut App, ship: Entity) -> Vec<(Entity, f32)> {
    app.world_mut().query::<&Grapple>().iter(app.world()).filter(|g| g.grappler == ship).map(|g| (g.target, g.flight)).collect()
}
fn drop_grapples(app: &mut App, ship: Entity) {
    let hooks: Vec<Entity> = app.world_mut().query::<(Entity, &Grapple)>().iter(app.world()).filter(|(_, g)| g.grappler == ship).map(|(e, _)| e).collect();
    for h in hooks {
        app.world_mut().despawn(h);
    }
}
#[test]
fn the_grapple_limit_counts_the_guns_of_every_bank_whose_first_working_gun_is_a_harpoon() {
    // (bank, working, harpoon)
    assert_eq!(bank_limit([(0, true, true), (0, true, true)].into_iter()), 2, "one bank of two harpoon guns");
    assert_eq!(bank_limit([(0, true, true)].into_iter()), 1, "a single harpoon");
    assert_eq!(bank_limit([(0, true, false), (0, true, true)].into_iter()), 0, "the first working gun is not a harpoon");
    assert_eq!(bank_limit([(0, false, false), (0, true, true), (1, true, true)].into_iter()), 3, "a dead first gun is skipped; banks add up");
    assert_eq!(bank_limit([(0, false, true), (0, true, false)].into_iter()), 0, "a dead harpoon does not count as first working");
    assert_eq!(bank_limit(std::iter::empty()), 0);
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
        .insert_resource(crate::testutil::lib())
        .insert_resource(SimRng::new(7))
        .init_resource::<crate::scenario::Difficulty>()
        .add_systems(Update, (handle_messages, set_flags, grapple_orders, hook_flight, winch_upkeep, winch_pull, drive, fights, settled, status, collect).chain());
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
    let ship = Ship { heading: 0.0, ..Ship::new(ci, side) };
    app.world_mut().spawn((Transform::from_translation(at), ship, health, Drift::default(), Boardable::ship(class, skill, true))).id()
}

#[test]
#[ignore = "needs imported assets"]
fn boarding_a_hostile_skirmish_base_changes_its_side() {
    use crate::combat::BaseFlag;
    let mut app = app();
    let mut table = crate::alliance::Alliances::default();
    table.add_player("Player", Side::Player);
    table.add_player("Enemy", Side::Enemy);
    app.insert_resource(table);
    let pts = points("Base_Procyon1");
    assert!(!pts.is_empty(), "the Procyon base exports boarding points");
    let base =
        app.world_mut().spawn((Transform::default(), MissionBase { id: 225, owner: "Enemy".into() }, BaseFlag::new("Enemy"), Boardable::base("Base_Procyon1", 2, true))).id();
    let me = spawn(&mut app, "Ship_Navy_Cutter", Side::Player, pts[0], Skill::Elite);
    app.world_mut().entity_mut(me).insert((PlayerShip, crate::ship::FleetMember("Player".into(), 0)));
    tick(&mut app, 0.1);
    app.world_mut().write_message(BoardMsg::Board { boarder: me, target: base });
    for _ in 0..600 {
        tick(&mut app, 0.1);
    }
    assert_eq!(app.world().get::<MissionBase>(base).unwrap().owner, "Player", "captured: the base fights for the captor now");
    assert_eq!(app.world().get::<BaseFlag>(base).unwrap(), &BaseFlag { shows: "Player".into(), start: "Enemy".into() });
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
    let (towee, other) = (app.world_mut().spawn_empty().id(), app.world_mut().spawn_empty().id());
    let mut mission = crate::campaign::load();
    mission.script.set_tow(me, towee, true);
    mission.script.set_tow(other, towee, true);
    app.insert_resource(mission);
    tick(&mut app, 0.1);
    assert_eq!(app.world().resource::<BoardStatus>().board(foe), (false, false), "no grapple yet: Board greyed");
    assert_eq!(app.world().resource::<BoardStatus>().grapple(foe), (false, true), "the cutter has a harpoon");
    app.world_mut().write_message(BoardMsg::Grapple { grappler: me, target: foe });
    tick(&mut app, 0.1);
    assert!(held(&mut app, me).iter().any(|g| g.1 > 0.0), "harpoon in the air");
    let surface = |app: &App| {
        let lib = app.world().resource::<Library>();
        let r = |e: Entity| lib.classes[app.world().get::<Ship>(e).unwrap().class].radius;
        app.world().get::<Transform>(me).unwrap().translation.distance(app.world().get::<Transform>(foe).unwrap().translation) - r(me) - r(foe)
    };
    while held(&mut app, me).iter().any(|g| g.1 > 0.0) {
        tick(&mut app, 0.1);
    }
    let bit = surface(&app);
    for _ in 0..20 {
        tick(&mut app, 0.1);
    }
    assert!((surface(&app) - bit).abs() < 0.5, "a grapple alone does not reel the ships together");
    assert!(app.world().resource::<BoardStatus>().board(foe).1, "grappled: Board enabled");
    app.world_mut().write_message(BoardMsg::Board { boarder: me, target: foe });
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
        .init_resource::<crate::combat::ArmedBank>();
    crate::ship::reconcile_fleets(app.world_mut());
    assert_eq!(app.world().resource::<crate::ship::Fleets>().roster(), &[me, foe], "capture joins the captor's own roster");
    assert!(held(&mut app, me).is_empty(), "grapple released");
    assert_eq!(app.world().resource::<Mission>().script.tows(), &[(other, towee)], "the captor's tow links are dropped, others kept");
    assert_eq!(app.world().resource::<Got>().0, vec![("Player".into(), "Enemy".to_string())]);
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
    app.world_mut().spawn(Grapple::new(me, foe, 0.0));
    tick(&mut app, 0.1);
    assert!(app.world().resource::<BoardStatus>().board(foe).1);
    app.world_mut().write_message(BoardMsg::Board { boarder: me, target: foe });
    tick(&mut app, 0.1);
    assert!(app.world().get::<Boarding>(me).is_some());
    app.world_mut().get_mut::<Ship>(foe).unwrap().speed = 8.0; // sails away faster than the relative-speed limit
    drop_grapples(&mut app, me);
    tick(&mut app, 0.1);
    assert!(app.world().get::<Boarding>(me).is_none(), "no grapple, no fight");
    app.world_mut().get_mut::<Boardable>(foe).unwrap().on = false;
    app.world_mut().spawn(Grapple::new(me, foe, 0.0));
    tick(&mut app, 0.1);
    assert!(!app.world().resource::<BoardStatus>().board(foe).1, "a ship that is not Boardable cannot be boarded");
}

#[test]
#[ignore = "needs imported assets"]
fn a_grapple_is_a_rope_that_pulls_both_hulls_back_and_snaps_only_past_a_jump_or_a_crossed_side() {
    let mut app = app();
    let me = spawn(&mut app, "Ship_Navy_Cutter", Side::Player, Vec3::ZERO, Skill::Average);
    app.world_mut().entity_mut(me).insert(PlayerShip);
    let foe = spawn(&mut app, "Ship_Pirate_Schooner", Side::Enemy, Vec3::new(0.0, 0.0, -40.0), Skill::Average);
    app.world_mut().spawn(Grapple::new(foe, me, 0.0));
    tick(&mut app, 0.1);
    let apart = |app: &App| app.world().get::<Transform>(me).unwrap().translation.distance(app.world().get::<Transform>(foe).unwrap().translation);
    let (m_me, m_foe) = {
        let lib = app.world().resource::<Library>();
        (lib.classes[app.world().get::<Ship>(me).unwrap().class].phys.mass, lib.classes[app.world().get::<Ship>(foe).unwrap().class].phys.mass)
    };
    app.world_mut().get_mut::<Ship>(me).unwrap().speed = -4.0; // we sail away from the schooner
    let start = (app.world().get::<Transform>(me).unwrap().translation, app.world().get::<Transform>(foe).unwrap().translation);
    for _ in 0..20 {
        tick(&mut app, 0.1);
    }
    assert!(!held(&mut app, foe).is_empty(), "sailing away at 4 units/s does not snap the rope");
    assert!((apart(&app) - 40.0).abs() < 2.0, "the rope holds the centres at the length it bit at, not {}", apart(&app));
    let moved = (app.world().get::<Transform>(me).unwrap().translation.distance(start.0), app.world().get::<Transform>(foe).unwrap().translation.distance(start.1));
    assert!((moved.0 < moved.1) == (m_me > m_foe), "the heavier hull gives way less ({moved:?}, masses {m_me} and {m_foe})");
    // A jump of more than 2 units past the rope's length in one tick snaps it.
    app.world_mut().get_mut::<Ship>(me).unwrap().speed = 0.0;
    app.world_mut().get_mut::<Transform>(me).unwrap().translation += Vec3::Z * 30.0;
    tick(&mut app, 0.1);
    assert!(held(&mut app, foe).is_empty(), "carried 2 units further than the rope in one tick: the line snapped");
    // Grappled on our bow, the schooner circles round to our stern: the line snaps.
    app.world_mut().get_mut::<Ship>(me).unwrap().speed = 0.0;
    app.world_mut().get_mut::<Transform>(me).unwrap().translation = Vec3::ZERO;
    app.world_mut().get_mut::<Transform>(foe).unwrap().translation = Vec3::new(0.0, 0.0, -30.0);
    app.world_mut().spawn(Grapple::new(foe, me, 0.0));
    tick(&mut app, 0.1);
    assert!(!held(&mut app, foe).is_empty());
    app.world_mut().get_mut::<Transform>(foe).unwrap().translation = Vec3::new(5.0, 0.0, 25.0);
    tick(&mut app, 0.1);
    assert!(held(&mut app, foe).is_empty(), "crossed to the other side of our hull: the line snapped");
}

/// Both players becoming allied lets the line go, whichever end holds it.
#[test]
#[ignore = "needs imported assets"]
fn becoming_allied_releases_the_grapple() {
    let mut app = app();
    let me = spawn(&mut app, "Ship_Navy_Cutter", Side::Player, Vec3::ZERO, Skill::Average);
    let foe = spawn(&mut app, "Ship_Pirate_Schooner", Side::Enemy, Vec3::new(0.0, 0.0, -40.0), Skill::Average);
    app.world_mut().spawn(Grapple::new(foe, me, 0.0));
    tick(&mut app, 0.1);
    assert!(!held(&mut app, foe).is_empty(), "hostile ships stay grappled");
    app.world_mut().get_mut::<Ship>(foe).unwrap().side = Side::Player;
    tick(&mut app, 0.1);
    assert!(held(&mut app, foe).is_empty(), "allied now: the line is let go");
    // The other direction: the hook is on our ship.
    app.world_mut().get_mut::<Ship>(foe).unwrap().side = Side::Enemy;
    app.world_mut().spawn(Grapple::new(me, foe, 0.0));
    tick(&mut app, 0.1);
    assert!(!held(&mut app, me).is_empty());
    app.world_mut().get_mut::<Ship>(foe).unwrap().side = Side::Player;
    tick(&mut app, 0.1);
    assert!(held(&mut app, me).is_empty(), "allied: released from the other end too");
}

/// The computer's harpoon test against the retail runs (docs/re/boarding.md §5): a Barque's beam harpoons reach a ship on its
/// beam inside 45 units but not at the 55.7-unit circle it held round an idle Cutter; a Carrack's bow-facing harpoons never
/// bear on a ship it circles on its beam.
#[test]
#[ignore = "needs imported assets"]
fn ai_harpoons_bear_by_arc_and_long_range() {
    let lib = crate::testutil::lib();
    let bears = |class: &str, at: Vec3| {
        let ci = lib.index(class);
        let ship = Ship { heading: 0.0, ..Ship::new(ci, Side::Enemy) };
        harpoon_bears(&lib, &ship, &Health::new(&lib.classes[ci]), None, &Transform::IDENTITY, at)
    };
    assert!(bears("Ship_Pirate_Barque", Vec3::X * 40.0) && bears("Ship_Pirate_Barque", Vec3::NEG_X * 40.0));
    assert!(!bears("Ship_Pirate_Barque", Vec3::X * 55.7) && !bears("Ship_Pirate_Barque", Vec3::NEG_Z * 40.0));
    assert!(!bears("Ship_Pirate_Carrack", Vec3::X * 30.0) && bears("Ship_Pirate_Carrack", Vec3::NEG_Z * 30.0));
    assert!((per_tick(AI_BOARD_CHANCE, crate::TICK_SECS) - 0.2).abs() < 1e-5 && (per_tick(AI_BOARD_CHANCE, 2.0 * crate::TICK_SECS) - 0.36).abs() < 1e-5);
}

#[test]
#[ignore = "needs imported assets"]
fn base_boarding_needs_a_boarding_point_in_reach_and_a_slow_boarder() {
    let lib = crate::testutil::lib();
    let ci = lib.index("Ship_Navy_Cutter");
    let ship = Ship { heading: 0.0, ..Ship::new(ci, Side::Enemy) };
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

#[test]
fn the_reel_takes_a_tenth_of_a_unit_per_0_033_s_tick() {
    assert!((reeled(50.0, crate::TICK_SECS) - 49.9).abs() < 1e-5);
    assert!((reeled(50.0, 2.0 * crate::TICK_SECS) - 49.8).abs() < 1e-5);
    assert_eq!(reeled(12.05, 1.0), REEL_MIN);
}

// ---- `OP Board Ship` (boarding/order.rs): the order a mission rule gives, with synthetic ships (no imported assets).

/// Two ships 40 units apart, `a` (side Player) with or without a harpoon, `b` (side `foe`) a boardable hull.
fn order_world(harpoon: bool, foe: Side) -> (App, Entity, Entity) {
    let mut class = crate::fixed_tests::synthetic_class();
    if harpoon {
        class.mounts = vec![crate::data::Mount { pos: Vec3::ZERO, rot: Quat::IDENTITY, arc: std::f32::consts::TAU, gun: "Gun_Harpoon".into(), bank: 0, parent: None }];
    }
    let mut app = App::new();
    app.add_message::<BoardMsg>()
        .add_message::<Captured>()
        .add_message::<Settled>()
        .add_message::<Sfx>()
        .insert_resource(Time::<()>::default())
        .insert_resource(Library { classes: vec![class], guns: default(), bullets: default(), names: default(), clips: default() })
        .insert_resource(SimRng::new(7))
        .add_systems(Update, (handle_messages, set_flags, grapple_orders, board_orders, hook_flight).chain());
    let hull = |app: &mut App, at: Vec3, side: Side| {
        let health = Health::new(&app.world().resource::<Library>().classes[0]);
        let boardable = Boardable { on: true, strength: Strength::ship(SCHOONER, Skill::Average.member()), skill: Skill::Average, counts: SCHOONER, points: vec![] };
        app.world_mut().spawn((Transform::from_translation(at), Ship::new(0, side), health, Drift::default(), boardable)).id()
    };
    let a = hull(&mut app, Vec3::ZERO, Side::Player);
    let b = hull(&mut app, Vec3::new(0.0, 0.0, -40.0), foe);
    (app, a, b)
}

#[test]
fn a_board_order_grapples_and_boards_when_the_hook_holds() {
    let (mut app, a, b) = order_world(true, Side::Enemy);
    app.world_mut().write_message(BoardMsg::Order { boarder: a, target: b });
    tick(&mut app, 0.1);
    assert!(app.world().get::<order::BoardOrder>(a).is_some(), "waiting for the hook");
    assert!(held(&mut app, a).iter().any(|g| g.0 == b && g.1 > 0.0), "the harpoon is in the air: the Grapple order a player gives");
    assert!(app.world().get::<Boarding>(a).is_none(), "no boarding before the hook bites");
    for _ in 0..30 {
        tick(&mut app, 0.1);
        if app.world().get::<Boarding>(a).is_some() {
            break;
        }
    }
    assert!(app.world().get::<Boarding>(a).is_some_and(|f| f.target() == b), "the boarding the Board order of the menu makes");
    assert!(app.world().get::<order::BoardOrder>(a).is_none(), "the order is spent");
}

#[test]
fn a_ship_without_a_harpoon_or_a_friendly_target_cannot_be_ordered_to_board() {
    let (mut app, a, b) = order_world(false, Side::Enemy);
    app.world_mut().write_message(BoardMsg::Order { boarder: a, target: b });
    tick(&mut app, 0.1);
    assert!(app.world().get::<order::BoardOrder>(a).is_none() && app.world().get::<GrappleOrder>(a).is_none() && held(&mut app, a).is_empty(), "no harpoon: nothing happens");
    let (mut app, a, b) = order_world(true, Side::Player);
    app.world_mut().write_message(BoardMsg::Order { boarder: a, target: b });
    tick(&mut app, 0.1);
    assert!(app.world().get::<order::BoardOrder>(a).is_none() && held(&mut app, a).is_empty(), "an ally is not boarded");
}

#[test]
fn a_board_order_ends_when_the_grapple_is_lost_and_waits_while_the_target_is_not_boardable() {
    let (mut app, a, b) = order_world(true, Side::Enemy);
    app.world_mut().get_mut::<Boardable>(b).unwrap().on = false;
    app.world_mut().write_message(BoardMsg::Order { boarder: a, target: b });
    for _ in 0..30 {
        tick(&mut app, 0.1);
    }
    assert!(app.world().get::<order::BoardOrder>(a).is_some() && app.world().get::<Boarding>(a).is_none(), "grappled, but the target may not be boarded: it waits");
    app.world_mut().get_mut::<Boardable>(b).unwrap().on = true;
    tick(&mut app, 0.1);
    tick(&mut app, 0.1);
    assert!(app.world().get::<Boarding>(a).is_some(), "once Boardable is set the boarding starts");
    // A lost grapple drops the order.
    let (mut app, a, b) = order_world(true, Side::Enemy);
    app.world_mut().get_mut::<Boardable>(b).unwrap().on = false;
    app.world_mut().write_message(BoardMsg::Order { boarder: a, target: b });
    tick(&mut app, 0.1);
    drop_grapples(&mut app, a);
    tick(&mut app, 0.1);
    assert!(app.world().get::<order::BoardOrder>(a).is_none(), "the hook is gone: the order lapses");
}

#[test]
fn a_board_order_ends_when_the_target_is_no_longer_an_enemy_and_an_abandoned_ship_takes_none() {
    // The hook holds on a target that may not be boarded yet; someone else captures it meanwhile (it now fights for the boarder's side).
    let (mut app, a, b) = order_world(true, Side::Enemy);
    app.world_mut().get_mut::<Boardable>(b).unwrap().on = false;
    app.world_mut().write_message(BoardMsg::Order { boarder: a, target: b });
    for _ in 0..30 {
        tick(&mut app, 0.1);
    }
    assert!(app.world().get::<order::BoardOrder>(a).is_some() && held(&mut app, a).iter().any(|g| g.0 == b && g.1 <= 0.0), "hooked and waiting");
    app.world_mut().get_mut::<Ship>(b).unwrap().side = Side::Player;
    tick(&mut app, 0.1);
    assert!(app.world().get::<order::BoardOrder>(a).is_none(), "not an enemy any more: the order does not wait forever");
    // A ship whose crew left cannot be told to board, and loses an order it had.
    let (mut app, a, b) = order_world(true, Side::Enemy);
    app.world_mut().entity_mut(a).insert(crate::lifeboats::Abandoned);
    app.world_mut().write_message(BoardMsg::Order { boarder: a, target: b });
    tick(&mut app, 0.1);
    assert!(app.world().get::<order::BoardOrder>(a).is_none() && app.world().get::<GrappleOrder>(a).is_none() && held(&mut app, a).is_empty());
    let (mut app, a, b) = order_world(true, Side::Enemy);
    app.world_mut().get_mut::<Transform>(b).unwrap().translation = Vec3::new(0.0, 0.0, -5000.0); // out of harpoon range: the order steers
    app.world_mut().write_message(BoardMsg::Order { boarder: a, target: b });
    tick(&mut app, 0.1);
    assert!(app.world().get::<GrappleOrder>(a).is_some());
    app.world_mut().entity_mut(a).insert(crate::lifeboats::Abandoned);
    tick(&mut app, 0.1);
    tick(&mut app, 0.1);
    assert!(app.world().get::<order::BoardOrder>(a).is_none() && app.world().get::<GrappleOrder>(a).is_none(), "abandoned on the way: both orders end");
}

/// Two fights at once draw their casualty coins from the one `SimRng` in the boarders' spawn order, whatever order the entities were
/// created (and the query lists them) in: the same battle gives the same losses on every run (#369).
#[test]
fn two_fights_at_once_draw_in_spawn_order_not_query_order() {
    let run = |first_pair_first: bool| {
        let class = crate::fixed_tests::synthetic_class();
        let mut app = App::new();
        app.add_message::<Settled>()
            .add_message::<Sfx>()
            .insert_resource(Time::<()>::default())
            .insert_resource(Library { classes: vec![class], guns: default(), bullets: default(), names: default(), clips: default() })
            .insert_resource(SimRng::new(11))
            .add_systems(Update, fights);
        // A fight: boarder `n` (spawn number n) on its grappled target, both hulls at one spot, the crews already fighting.
        let pair = |app: &mut App, n: u32, at: Vec3| {
            let hull = |app: &mut App, side: Side, index: u32| {
                let health = Health::new(&app.world().resource::<Library>().classes[0]);
                let boardable = Boardable { on: true, strength: Strength::ship(SCHOONER, Skill::Average.member()), skill: Skill::Average, counts: SCHOONER, points: vec![] };
                app.world_mut().spawn((Transform::from_translation(at), Ship::new(0, side), health, Drift::default(), boardable, crate::scenario::SpawnIndex(index))).id()
            };
            let (b, t) = (hull(app, Side::Player, n), hull(app, Side::Enemy, 10 + n));
            app.world_mut().spawn(Grapple::new(b, t, 0.0));
            app.world_mut().entity_mut(b).insert(Boarding { target: t, interval: 0.0, since: 0.0, live: true, fx: vec![] });
            (b, t)
        };
        let (a, c) = if first_pair_first {
            let a = pair(&mut app, 0, Vec3::ZERO);
            (a, pair(&mut app, 1, Vec3::X * 500.0))
        } else {
            let c = pair(&mut app, 1, Vec3::X * 500.0);
            (pair(&mut app, 0, Vec3::ZERO), c)
        };
        for _ in 0..12 {
            tick(&mut app, 0.033);
        }
        let left = |e: Entity| app.world().get::<Boardable>(e).map(|b| (b.strength.attack, b.strength.defend));
        [left(a.0), left(a.1), left(c.0), left(c.1)]
    };
    let (one, other) = (run(true), run(false));
    assert_eq!(one, other);
    assert!(one[0] != one[2] || one[1] != one[3], "the two fights drew different coins, so their order shows: {one:?}");
}

/// A won fight for a base hands it to the captor (its side and guns follow the owner) and repaints its flag. Asset-free.
#[test]
fn a_captured_base_changes_owner_and_shows_the_captors_flag() {
    use crate::combat::BaseFlag;
    let mut app = App::new();
    app.add_message::<Settled>().add_message::<Captured>().add_message::<Sfx>().insert_resource(SimRng::new(7)).add_systems(Update, fight::settled);
    let boarder = app.world_mut().spawn((Ship::new(0, Side::Player), Boardable::base("x", 2, true), crate::ship::FleetMember("player0".into(), 0))).id();
    let base = app.world_mut().spawn((MissionBase { id: 4, owner: "player1".into() }, BaseFlag::new("player1"), Boardable::base("x", 2, true))).id();
    app.world_mut().write_message(Settled { boarder, target: base, outcome: Outcome::Won });
    app.update();
    assert_eq!(app.world().get::<MissionBase>(base).unwrap().owner, "player0", "the captor's fleet owner: its side now");
    assert_eq!(app.world().get::<BaseFlag>(base).unwrap().shows, "player0", "a capture repaints the flag");
}
