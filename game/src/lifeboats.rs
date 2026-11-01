//! Lifeboats (docs/re/lifeboats.md): a destroyed ship whose Create Lifeboats flag is set leaves `Number of lifeboats` boats of its race,
//! which drift away until any live ship touches one; the nearest such ship collects it. Lifeboats are identified by the class flag
//! `Is Lifeboat` only, never by name (`Ship_Navy_Lifeboat_Mission9` is a combat ship). Boarding captures are a separate feature.
use crate::campaign::{Mission, MissionShip};
use crate::combat::{Health, Library};
use crate::{PlayerShip, Ship, Side};
use bevy::prelude::*;

/// A lifeboat. `owner` is the rules' player name of the ship that left it; pre-placed boats (mission 4) also carry a `MissionShip`,
/// whose owner then wins so 'Set Group/Unit Owner' keeps working. Spawned boats have no `MissionShip`: rules address groups, not them.
#[derive(Component)]
pub struct Lifeboat {
    pub owner: String,
}

/// A ship was destroyed in combat (core hit points gone, or the dev kill keys). Scripted `Destroy Group/Unit` is not reported:
/// whether it starts the original's lifeboat-spawning explosion state is unverified (docs/re/lifeboats.md §4.3).
#[derive(Message)]
pub struct ShipLost(pub Entity);

pub const PICKUP_SOUND: &str = "HUD_Lifeboat_Pickup";
/// Victory points for the local player collecting a lifeboat of a player it is not allied with.
pub const PICKUP_POINTS: u32 = 2;

/// The lifeboat class a ship of `race` leaves; unknown races leave Civilian boats, as in the original.
pub fn class_for(race: &str) -> String {
    format!("Ship_{}_Lifeboat", if matches!(race, "Navy" | "Civilian" | "Pirate" | "Procyon") { race } else { "Civilian" })
}

/// One boat's random drift: the original draws two uniform values in [-100, 100) as an offset (the path goal is 10000x that far away, i.e.
/// "sail that way forever") and a throttle `0.2 + 0.8u`. Returns the game-axes offset (x starboard, y forward) and the throttle.
pub fn drift(rng: &mut crate::combat::Rng) -> (Vec2, f32) {
    let mut u = || rng.f() * 200.0 - 100.0;
    let offset = Vec2::new(u(), u());
    (offset, (0.2 + 0.8 * rng.f()).clamp(0.0, 1.0))
}

/// Surface distance between two bounding spheres; <= 0 means contact.
pub fn surface_distance(a: (Vec3, f32), b: (Vec3, f32)) -> f32 {
    a.0.distance(b.0) - a.1 - b.1
}

fn world_sphere(lib: &Library, ship: &Ship, tf: &Transform) -> (Vec3, f32) {
    let (c, r) = lib.classes[ship.class].sphere;
    (tf.transform_point(c), r)
}

pub fn plugin(app: &mut App) {
    use crate::menu::Screen;
    app.add_message::<ShipLost>().add_systems(
        FixedUpdate,
        // Consume combat deaths before the next rules tick.
        lifeboats.in_set(crate::BattleSet::Cleanup).run_if(in_state(Screen::Battle)).run_if(crate::menu::overlay_closed),
    );
}

/// Spawns the boats of lost ships, runs the pickup test and keeps the rules' view of which boats exist.
fn lifeboats(
    mut commands: Commands,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    fxl: Res<crate::fx::FxLib>,
    mut rng: ResMut<crate::combat::Rng>,
    mut lost: MessageReader<ShipLost>,
    mut sfx: MessageWriter<crate::audio::Sfx>,
    mission: Option<ResMut<Mission>>,
    dying: Query<(&Ship, &Transform, Option<&MissionShip>), Without<Lifeboat>>,
    boats: Query<(Entity, &Ship, &Transform, &Lifeboat, Option<&MissionShip>)>,
    ships: Query<(&Ship, &Transform, &Health, Option<&MissionShip>), Without<Lifeboat>>,
) {
    let owner = |l: &Lifeboat, m: Option<&MissionShip>| m.map_or_else(|| l.owner.clone(), |m| m.owner.clone());
    let mut present: Vec<(Option<Entity>, String)> = boats.iter().map(|(e, _, _, l, m)| (Some(e), owner(l, m))).collect();
    for ShipLost(e) in lost.read() {
        let Ok((ship, tf, ms)) = dying.get(*e) else { continue };
        let class = &lib.classes[ship.class];
        let Some(ms) = ms.filter(|m| m.create_lifeboats && class.lifeboats > 0 && !class.is_lifeboat) else { continue };
        let Some(boat) = lib.classes.iter().find(|c| c.name == class_for(&class.race)) else {
            warn!("no {} class: {} leaves no lifeboats", class_for(&class.race), class.name);
            continue;
        };
        for _ in 0..class.lifeboats {
            let (offset, throttle) = drift(&mut rng);
            let to = Vec3::new(offset.x, 0.0, -offset.y);
            let heading = if to.length_squared() > 1e-6 { (-to.x).atan2(-to.z) } else { ship.heading };
            let id = crate::spawn_ship(&mut commands, &assets, &lib, &fxl, &boat.name, Side::Player, tf.translation, heading, None);
            commands.entity(id).remove::<PlayerShip>().remove::<crate::boarding::Boardable>().insert((
                Lifeboat { owner: ms.owner.clone() },
                Ship {
                    class: lib.index(&boat.name),
                    side: Side::Player, // never an enemy: the original treats lifeboats as friendly to everyone
                    heading,
                    speed: 0.0,
                    ang_vel: 0.0,
                    order: 2,
                    waypoints: vec![tf.translation + to * 10000.0],
                    goal: None,
                    cap: 1.0,
                    throttle: Some(throttle),
                    sail_energy: 1.0,
                    crew_speed: 1.0,
                    crew_turn: 1.0,
                    net: 1.0,
                    avoid: 1.0,
                },
            ));
            present.push((None, ms.owner.clone()));
        }
    }
    let mut mission = mission;
    // Each boat is tested against the nearest live non-lifeboat ship (any owner, human or AI); contact of the bounding spheres collects it.
    for (boat, bship, btf, l, bm) in &boats {
        let sphere = world_sphere(&lib, bship, btf);
        let Some((d, cm)) =
            ships.iter().filter(|(_, _, h, _)| !h.destroyed).map(|(s, tf, _, m)| (surface_distance(sphere, world_sphere(&lib, s, tf)), m)).min_by(|a, b| a.0.total_cmp(&b.0))
        else {
            continue;
        };
        if d > 0.0 {
            continue;
        }
        info!("lifeboat picked up: owner {} by {:?} at {:?} d={d}", owner(l, bm), cm.map(|m| (&m.name, &m.owner)), btf.translation);
        commands.entity(boat).despawn();
        present.retain(|p| p.0 != Some(boat));
        sfx.write(crate::audio::Sfx::ui(PICKUP_SOUND));
        if let Some(m) = mission.as_mut() {
            m.script.lifeboat_saved(cm.map_or("", |m| m.owner.as_str()), &owner(l, bm));
        }
    }
    if let Some(m) = mission.as_mut() {
        m.script.set_lifeboat_owners(present.into_iter().map(|p| p.1).collect());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::Rng;

    fn app(mission: &str) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<WorldAsset>()
            .init_resource::<crate::fx::FxLib>()
            .insert_resource(Library::load(crate::assets_dir()))
            .insert_resource(Rng::new(5))
            .insert_resource(crate::campaign::load_id(mission))
            .add_message::<ShipLost>()
            .add_message::<crate::audio::Sfx>()
            .add_systems(Update, lifeboats);
        app
    }
    fn ship(app: &mut App, class: &str, owner: &str, at: Vec3, flag: bool) -> Entity {
        let lib = app.world().resource::<Library>();
        let ci = lib.index(class);
        let health = Health::new(&lib.classes[ci]);
        let s = Ship {
            class: ci,
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
            net: 1.0,
            avoid: 1.0,
        };
        let ms = MissionShip {
            id: -1,
            name: class.into(),
            owner: owner.into(),
            hold_fire: false,
            invuln: 0.0,
            visible: true,
            stance: crate::rules::Stance::Neutral,
            target: None,
            attack: vec![],
            flag: String::new(),
            banner: String::new(),
            looping: None,
            stop_at_end: false,
            create_lifeboats: flag,
            ..Default::default()
        };
        app.world_mut().spawn((s, health, Transform::from_translation(at), ms)).id()
    }
    fn boats(app: &mut App) -> Vec<(String, String, Vec3)> {
        let mut q = app.world_mut().query::<(&Lifeboat, &Ship, &Transform)>();
        let lib = app.world().resource::<Library>();
        q.iter(app.world()).map(|(l, s, t)| (l.owner.clone(), lib.classes[s.class].name.clone(), t.translation)).collect()
    }
    fn lose(app: &mut App, e: Entity) {
        app.world_mut().write_message(ShipLost(e));
        app.update();
    }

    #[test]
    fn drift_is_uniform_in_the_exe_ranges_and_deterministic() {
        let (mut a, mut b) = (Rng::new(9), Rng::new(9));
        for _ in 0..200 {
            let (o, t) = drift(&mut a);
            assert!(o.abs().max_element() <= 100.0 && (0.2..=1.0).contains(&t));
            assert_eq!((o, t), drift(&mut b));
        }
        assert_eq!([class_for("Navy"), class_for("Procyon"), class_for("Weird")], ["Ship_Navy_Lifeboat", "Ship_Procyon_Lifeboat", "Ship_Civilian_Lifeboat"]);
        assert!(surface_distance((Vec3::ZERO, 2.0), (Vec3::X * 3.0, 1.0)) <= 0.0 && surface_distance((Vec3::ZERO, 2.0), (Vec3::X * 3.1, 1.0)) > 0.0);
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn creation_follows_flag_count_race_and_current_owner() {
        let mut app = app("mission02");
        let at = Vec3::new(10.0, 0.0, 20.0);
        let plain = ship(&mut app, "Ship_Pirate_Schooner", "Pirates", at, false);
        lose(&mut app, plain);
        assert!(boats(&mut app).is_empty(), "a ship no rule named leaves nothing");
        for (class, owner, n, boat) in [
            ("Ship_Pirate_Schooner", "Pirates", 2, "Ship_Pirate_Lifeboat"),
            ("Ship_Pirate_Sloop", "Pirates", 1, "Ship_Pirate_Lifeboat"),
            ("Ship_Civilian_Merchant", "Schooner1", 6, "Ship_Civilian_Lifeboat"),
            (
                "Ship_Navy_Tender",
                "Pirates",
                3.min(app.world().resource::<Library>().classes[app.world().resource::<Library>().index("Ship_Navy_Tender")].lifeboats as usize),
                "Ship_Navy_Lifeboat",
            ),
            ("Ship_Procyon_Dreadnought", "Pirates", 0, "Ship_Procyon_Lifeboat"),
        ] {
            let e = ship(&mut app, class, owner, at, true);
            lose(&mut app, e);
            let got = boats(&mut app);
            assert_eq!(got.len(), n, "{class}");
            assert!(got.iter().all(|b| b.0 == owner && b.1 == boat && b.2 == at), "{class}: owner of the moment, race class, at the wreck");
            let all: Vec<_> = app.world_mut().query_filtered::<Entity, With<Lifeboat>>().iter(app.world()).collect();
            all.into_iter().for_each(|b| {
                app.world_mut().despawn(b);
            });
        }
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn boats_sail_straight_at_a_fraction_of_class_speed_and_never_collide_with_ships() {
        let mut app = app("mission02");
        let parent = ship(&mut app, "Ship_Pirate_Schooner", "Pirates", Vec3::ZERO, true);
        lose(&mut app, parent);
        let mut q = app.world_mut().query_filtered::<&Ship, With<Lifeboat>>();
        for s in q.iter(app.world()) {
            assert_eq!((s.order, s.waypoints.len(), s.side), (2, 1, Side::Player));
            assert!((0.2..=1.0).contains(&s.throttle.unwrap()) && s.waypoints[0].length() > 1.0e3);
            let to = s.waypoints[0].normalize();
            assert!((Vec3::new(-s.heading.sin(), 0.0, -s.heading.cos()) - to).length() < 1e-3, "heading faces its goal");
        }
        // a ship on top of a boat is not pushed (a ship-lifeboat contact has no response), whichever way round
        let lib = app.world().resource::<Library>();
        let (cutter, boat) = (lib.index("Ship_Navy_Cutter"), lib.index("Ship_Navy_Lifeboat"));
        let mk = |class| Ship {
            class,
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
            net: 1.0,
            avoid: 1.0,
        };
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs_f32(0.1));
        let mut w = App::new();
        w.insert_resource(time).insert_resource(Library::load(crate::assets_dir())).add_systems(Update, crate::collision::sail);
        let health = |w: &App, c: usize| Health::new(&w.world().resource::<Library>().classes[c]);
        let (h1, h2) = (health(&w, cutter), health(&w, boat));
        let a = w.world_mut().spawn((mk(cutter), h1, Transform::from_xyz(0.0, 0.0, 0.0), crate::collision::Drift::default())).id();
        let b = w.world_mut().spawn((mk(boat), h2, Transform::from_xyz(0.5, 0.0, 0.5), crate::collision::Drift::default(), Lifeboat { owner: "x".into() })).id();
        for _ in 0..10 {
            w.update();
        }
        assert_eq!(w.world().get::<Transform>(a).unwrap().translation, Vec3::ZERO);
        assert_eq!(w.world().get::<Transform>(b).unwrap().translation, Vec3::new(0.5, 0.0, 0.5));
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn pickup_by_nearest_live_non_lifeboat_ship_in_sphere_contact() {
        let mut app = app("mission02");
        let boat = |app: &mut App, at| {
            let e = ship(app, "Ship_Pirate_Lifeboat", "Pirates", at, false);
            app.world_mut().entity_mut(e).insert(Lifeboat { owner: "Pirates".into() }).remove::<MissionShip>();
            e
        };
        let b = boat(&mut app, Vec3::ZERO);
        let far = ship(&mut app, "Ship_Navy_Cutter", "PlayerStart", Vec3::new(0.0, 0.0, 15.5), false); // just outside the 12 + boat sphere
        app.update();
        assert!(app.world().get_entity(b).is_ok(), "just outside contact: not collected");
        assert!(!app.world().resource::<Mission>().script.lifeboat_owners().is_empty());
        let dead = ship(&mut app, "Ship_Navy_Cutter", "Schooner2", Vec3::ZERO, false);
        app.world_mut().get_mut::<Health>(dead).unwrap().destroyed = true;
        let other_boat = boat(&mut app, Vec3::ZERO);
        app.update();
        assert!(app.world().get_entity(b).is_ok() && app.world().get_entity(other_boat).is_ok(), "dead ships and other lifeboats never collect");
        app.world_mut().get_mut::<Transform>(far).unwrap().translation.z = 6.0; // inside contact
        let nearer = ship(&mut app, "Ship_Navy_Cutter", "Schooner2", Vec3::new(0.0, 0.0, 3.0), false);
        app.update();
        app.update();
        let m = &app.world().resource::<Mission>().script;
        assert_eq!((m.lifeboats_saved_by("Schooner2", "Pirates"), m.lifeboats_saved_by("PlayerStart", "Pirates")), (2, 0), "an AI ally collects both, only the nearest collects");
        assert!(app.world().get_entity(b).is_err() && app.world().get_entity(other_boat).is_err() && app.world().get_entity(nearer).is_ok());
        assert_eq!(m.score, 0, "AI collection scores nothing");
        assert!(m.lifeboat_owners().is_empty());
        assert_eq!(app.world().resource::<Messages<crate::audio::Sfx>>().len(), 2, "one pickup sound per boat");
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn preplaced_mission4_boats_are_lifeboats_by_flag_and_mission9_longboat_is_not() {
        let lib = Library::load(crate::assets_dir());
        let flagged = |mission: &str, id: i64| {
            let d = crate::campaign::load_id(mission).data;
            let s = d["ships"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap().clone();
            lib.classes.iter().any(|c| c.is_lifeboat && Some(c.name.as_str()) == s["type"].as_str())
        };
        assert!([220, 221, 222].iter().all(|id| flagged("mission04", *id)));
        assert!(!flagged("mission09", 303), "Ship_Navy_Lifeboat_Mission9 is a combat story ship");
    }
}
