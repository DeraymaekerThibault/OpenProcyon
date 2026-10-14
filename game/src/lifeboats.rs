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
/// whether it starts the exe's lifeboat-spawning explosion state is unverified (docs/re/lifeboats.md §4.3).
#[derive(Message)]
pub struct ShipLost(pub Entity);

pub const PICKUP_SOUND: &str = "HUD_Lifeboat_Pickup";
/// Victory points for the local player collecting a lifeboat of a player it is not allied with.
pub const PICKUP_POINTS: u32 = 2;

/// The lifeboat class a ship of `race` leaves; unknown races leave Civilian boats, as the exe's jump table does.
pub fn class_for(race: &str) -> String {
    format!("Ship_{}_Lifeboat", if matches!(race, "Navy" | "Civilian" | "Pirate" | "Procyon") { race } else { "Civilian" })
}

/// One boat's random drift: the exe draws two uniform values in [-100, 100) as an offset (the path goal is 10000x that far away, i.e.
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
    app.add_message::<ShipLost>().add_systems(PreUpdate, smoke.after(bevy::ui::UiSystems::Focus)).add_systems(
        Update,
        // After the dev kill keys and before the rules, so a ship lost this frame has its boats when the rules next look.
        lifeboats.after(crate::debug_kill).before(crate::briefing::update).run_if(in_state(Screen::Battle)).run_if(crate::menu::overlay_closed),
    );
}

/// ECS smoke scenarios for `--script lifeboat-m1|lifeboat-m1-fallback|lifeboat-m2|lifeboat-m2-ladyj|lifeboat-m4|lifeboat-m9|lifeboat-kill` (needs `--shot`, which counts frames;
/// docs/re/lifeboats.md §7.10). SCRIPTED: it kills the pirate through its health (the combat-death path, not a fight), teleports the
/// collector onto a boat and, for mission 1, marks the NIS speech as played; everything else is the normal rules and systems.
/// Normal-combat validation is a manual play-through. Panics (non-zero exit) when an expectation fails.
fn smoke(world: &mut World, mut stage: Local<(u32, u32)>, mut score: Local<u32>, mut expect: Local<Option<(String, String, u32)>>) {
    let cfg = world.resource::<crate::Config>();
    let Some(script) = cfg.script.clone().filter(|s| s.starts_with("lifeboat-")) else { return };
    let (frame, shot) = (cfg.frame, cfg.shot.is_some());
    assert!(shot, "{script} counts frames: run it with --shot");
    if !world.contains_resource::<Mission>() || frame < 90 {
        return;
    }
    let pick = |w: &mut World, name: &str| w.query::<(Entity, &MissionShip)>().iter(w).find(|(_, m)| m.name == name).map(|(e, _)| e).unwrap_or_else(|| panic!("no ship {name}"));
    if script == "lifeboat-m4" || script == "lifeboat-m9" {
        // Placed boats: mission 4 has three real ones (class flag) that nothing collects at the start; mission 9's longboat is a combat ship.
        if frame == 200 {
            let found: Vec<_> = world.query::<(&Lifeboat, &Ship, &MissionShip)>().iter(world).map(|(l, s, m)| (l.owner.clone(), s.side, m.name.clone())).collect();
            if script == "lifeboat-m4" {
                assert_eq!(found.len(), 3, "mission 4 pre-placed lifeboats survive their first seconds");
                assert!(found.iter().all(|f| f.0 == "navy" && f.1 == Side::Player));
                assert_eq!(world.resource::<Mission>().script.lifeboats_saved_by("tourist", "navy"), 0);
            } else {
                assert!(found.is_empty());
                pick(world, "Longboat");
            }
            info!("{script}: PASS");
            std::process::exit(0);
        }
        return;
    }
    if script == "lifeboat-kill" {
        // Any mission: the first flagged ship that leaves boats is killed; its boats must match its race, count and owner.
        let classes: Vec<(String, String, u32)> = world.resource::<Library>().classes.iter().map(|c| (c.name.clone(), c.race.clone(), c.lifeboats)).collect();
        if frame == 200 {
            let (e, class, owner) = world
                .query::<(Entity, &Ship, &MissionShip, &Health)>()
                .iter(world)
                .find(|(_, s, m, h)| m.create_lifeboats && classes[s.class].2 > 0 && !h.destroyed)
                .map(|(e, s, m, _)| (e, &classes[s.class], m.owner.clone()))
                .unwrap_or_else(|| panic!("{script}: no flagged enemy ship that leaves boats"));
            *expect = Some((class_for(&class.1), owner.clone(), class.2));
            info!("{script}: killing {} of {owner}, expecting {} x {}", class.0, class.2, class_for(&class.1));
            world.get_mut::<Health>(e).unwrap().core = 0.0;
        } else if let Some((class, owner, n)) = expect.clone() {
            let found = world.query::<(&Lifeboat, &Ship)>().iter(world).map(|(l, s)| (l.owner.clone(), classes[s.class].0.clone())).collect::<Vec<_>>();
            if found.len() as u32 >= n {
                assert!(found.len() as u32 == n && found.iter().all(|f| f.0 == owner && f.1 == class), "{script}: wrong boats {found:?}");
                info!("{script}: PASS");
                std::process::exit(0);
            }
        }
        return;
    }
    let (m1, ladyj, fallback) = (script.starts_with("lifeboat-m1"), script.ends_with("ladyj"), script.ends_with("fallback"));
    let flagged = |w: &mut World, name: &str| {
        let e = pick(w, name);
        w.get::<MissionShip>(e).unwrap().create_lifeboats
    };
    let boats = |w: &mut World| w.query::<(Entity, &Lifeboat, &Transform)>().iter(w).map(|(e, l, t)| (e, l.owner.clone(), t.translation)).collect::<Vec<_>>();
    let (victim, owner) = if m1 { ("Warrior", "Pirate Schooner") } else { ("Ravage", "Pirates") };
    let wait = |s: &mut (u32, u32), ok: bool, what: &str| {
        s.1 += 1;
        assert!(s.1 < 3000, "{script}: timed out waiting for {what}");
        if ok {
            *s = (s.0 + 1, 0);
        }
        ok
    };
    match stage.0 {
        0 => {
            // Rules named the ships at world init (not the whole fleet).
            let names: &[(&str, bool)] = if m1 {
                &[("Warrior", true), ("RLS Providence", true), ("RLS Explorer", true), ("RLS Sovereign", false), ("RLS Bounty", false)]
            } else {
                &[("Ravage", true), ("SS Lady J", true), ("SS Coventry", true), ("SS Denman", false)]
            };
            for (n, want) in names {
                assert_eq!(flagged(world, n), *want, "Create Lifeboats flag of {n}");
            }
            assert!(boats(world).is_empty());
            let e = pick(world, victim);
            // The mission starts the player's ship beside the victim, which would collect the boats at once: move it away.
            let at = world.get::<Transform>(e).unwrap().translation;
            let player = world.query_filtered::<Entity, With<crate::PlayerShip>>().single(world).unwrap();
            world.get_mut::<Transform>(player).unwrap().translation = at + Vec3::new(-400.0, 0.0, 400.0);
            world.get_mut::<Health>(e).unwrap().core = 0.0; // `combat::deaths` then reports the loss
            info!("{script}: {victim} dies");
            *stage = (1, 0);
        }
        1 => {
            let b = boats(world);
            if wait(&mut stage, b.len() == 2, "two lifeboats") {
                assert!(b.iter().all(|x| x.1 == owner));
                let lib = world.resource::<Library>();
                let class = |e: Entity| lib.classes[world.get::<Ship>(e).unwrap().class].name.clone();
                assert!(b.iter().all(|x| class(x.0) == "Ship_Pirate_Lifeboat" && world.get::<crate::boarding::Boardable>(x.0).is_none()));
                if m1 {
                    world.resource_mut::<Mission>().script.played.insert("M01AMELIA17".into()); // scripted: skip the NIS speech
                }
                info!("{script}: boats spawned");
            }
        }
        2 => {
            // Boats exist: the rules see them, the exit does not fire, the objective shows.
            let sc = &world.resource::<Mission>().script;
            assert_eq!(sc.lifeboat_owners().len(), 2);
            assert!(sc.ended.is_none(), "no exit while boats are afloat");
            let objective = if m1 { "Collect Pirate Lifeboat" } else { "PickUpLifeboats" };
            let ready = sc.task_active(objective) && (m1 || sc.played.contains("M02ONYX18"));
            wait(&mut stage, ready, objective);
        }
        3 => {
            let before = world.resource::<Mission>().script.score;
            if fallback {
                // Nobody collects: the retail timers end the mission ~100 s after the kill (55 s + 45 s).
                let ended = world.resource::<Mission>().script.ended.clone();
                if wait(&mut stage, ended.is_some(), "the fallback exit") {
                    assert_eq!(world.resource::<Mission>().script.lifeboat_owners().len(), 2, "uncollected boats stay until the level changes");
                    info!("{script}: PASS (fallback exit)");
                    std::process::exit(0);
                }
                return;
            }
            let collector = if ladyj { pick(world, "SS Lady J") } else { world.query_filtered::<Entity, With<crate::PlayerShip>>().single(world).unwrap() };
            let at = boats(world)[0].2;
            world.get_mut::<Transform>(collector).unwrap().translation = at;
            world.get_mut::<Ship>(collector).unwrap().speed = 0.0;
            *score = before;
            *stage = (4, 0);
        }
        4 => {
            // Both boats of one wreck sit within a few units of each other, so one visit usually collects both.
            let b = boats(world);
            if wait(&mut stage, b.len() < 2, "the first pickup") {
                let sc = &world.resource::<Mission>().script;
                let (who, per_boat) = if ladyj {
                    ("Schooner2", 0)
                } else if m1 {
                    ("Player0 Group", 2)
                } else {
                    ("PlayerStart", 2)
                };
                let got = sc.lifeboats_saved_by(who, owner);
                assert_eq!(got as usize, 2 - b.len());
                assert_eq!(sc.score, *score + per_boat * got, "+2 per boat only for the local player collecting an enemy boat");
                if m1 {
                    assert!(sc.ended.is_none() || b.is_empty());
                } else {
                    assert!(sc.flag("LifeboatsPickedUp") || sc.ended.is_none(), "mission 2 reacts to the first pickup");
                }
                info!("{script}: {got} boat(s) collected");
                if let Some(rest) = b.first() {
                    let collector = world.query_filtered::<Entity, With<crate::PlayerShip>>().single(world).unwrap();
                    world.get_mut::<Transform>(collector).unwrap().translation = rest.2;
                }
            }
        }
        5 => {
            let ended = world.resource::<Mission>().script.ended.clone();
            if wait(&mut stage, ended.is_some(), "the next-level exit") {
                assert!(ended.unwrap().1, "mission won");
                let left = boats(world).len();
                let sc = &world.resource::<Mission>().script;
                if m1 {
                    assert_eq!((sc.lifeboats_saved_by("Player0 Group", owner), left), (2, 0));
                } else {
                    let got = sc.lifeboats_saved_by(if ladyj { "Schooner2" } else { "PlayerStart" }, owner) as usize;
                    assert_eq!(left + got, 2);
                    assert_eq!((sc.task_complete("PickUpLifeboats"), sc.task_active("PickUpLifeboats")), (!ladyj, false));
                }
                info!("{script}: PASS");
                std::process::exit(0);
            }
        }
        _ => {}
    }
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
                    side: Side::Player, // never an enemy: the exe's relation filter treats lifeboats as friendly to everyone
                    heading,
                    speed: 0.0,
                    ang_vel: 0.0,
                    order: 2,
                    waypoints: vec![tf.translation + to * 10000.0],
                    goal: None,
                    cap: 1.0,
                    throttle: Some(throttle),
                    sail_energy: 1.0,
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
            .insert_resource(Library::load(&crate::assets_dir()))
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
        let s = Ship { class: ci, side: Side::Player, heading: 0.0, speed: 0.0, ang_vel: 0.0, order: 0, waypoints: vec![], goal: None, cap: 1.0, throttle: None, sail_energy: 1.0 };
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
        // a ship on top of a boat is not pushed (Ship_vs_Lifeboat has no response), whichever way round
        let lib = app.world().resource::<Library>();
        let (cutter, boat) = (lib.index("Ship_Navy_Cutter"), lib.index("Ship_Navy_Lifeboat"));
        let mk =
            |class| Ship { class, side: Side::Player, heading: 0.0, speed: 0.0, ang_vel: 0.0, order: 0, waypoints: vec![], goal: None, cap: 1.0, throttle: None, sail_energy: 1.0 };
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs_f32(0.1));
        let mut w = App::new();
        w.insert_resource(time).insert_resource(Library::load(&crate::assets_dir())).add_systems(Update, crate::collision::sail);
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
    fn preplaced_mission4_boats_are_lifeboats_by_flag_and_mission9_longboat_is_not() {
        let lib = Library::load(&crate::assets_dir());
        let flagged = |mission: &str, id: i64| {
            let d = crate::campaign::load_id(mission).data;
            let s = d["ships"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap().clone();
            lib.classes.iter().any(|c| c.is_lifeboat && Some(c.name.as_str()) == s["type"].as_str())
        };
        assert!([220, 221, 222].iter().all(|id| flagged("mission04", *id)));
        assert!(!flagged("mission09", 303), "Ship_Navy_Lifeboat_Mission9 is a combat story ship");
    }
}
