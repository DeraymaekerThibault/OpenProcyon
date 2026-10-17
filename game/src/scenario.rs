//! Battle lifecycle: spawning ships/scenery from a map or mission, restart, leave, outcome.
use crate::*;

/// Names ships that no script names: a random not-yet-used entry of the ship type's pool, as open maps do (WOT 'Available Unique
/// Ship Name'); a used-up pool repeats names. PROVISIONAL: the original's selection (and its uniqueness scope) is unknown.
pub struct Namer {
    pub used: std::collections::HashSet<String>,
    pub seed: u64,
}

impl Namer {
    #[allow(clippy::new_without_default)] // time-seeded: a `Default` would hide the randomness
    pub fn new() -> Self {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
        Namer { used: Default::default(), seed: t | 1 }
    }
    pub fn pick(&mut self, lib: &Library, class: &str) -> Option<String> {
        let pool = lib.names.get(class)?;
        let free: Vec<&String> = pool.iter().filter(|n| !self.used.contains(*n)).collect();
        let list = if free.is_empty() { pool.iter().collect() } else { free };
        if list.is_empty() {
            return None;
        }
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        let n = list[(self.seed % list.len() as u64) as usize].clone();
        self.used.insert(n.clone());
        Some(n)
    }
}

#[derive(Component)]
pub struct Scenario;

/// One fleet of a battle: whose start point it uses (index in the map's player list), which side it fights for, whether the
/// human commands it, and its ships with an explicit start position (historical maps) or none (formation at the start point).
pub struct Group {
    pub player: usize,
    pub side: Side,
    pub human: bool,
    pub ships: Vec<(String, Option<[f32; 3]>, Option<String>)>, // class, start position, scripted name
    pub weapons: Vec<std::collections::BTreeMap<usize, String>>,
}

#[derive(Resource, PartialEq, Clone, Copy)]
pub enum Outcome {
    Playing,
    Victory,
    Defeat,
}

/// Battle start: the front end's choice (map, fleets) if any, else the command line.
pub fn enter_battle(
    mut commands: Commands,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    fxl: Res<fx::FxLib>,
    shapes: Res<collision::Shapes>,
    asteroids: Res<data::AsteroidDefs>,
    mut cfg: ResMut<Config>,
    sk: Res<menu::Skirmish>,
) {
    if sk.map.is_some() {
        cfg.map = sk.map.clone();
        cfg.groups = sk.plan(&lib);
        cfg.ended = None;
    }
    spawn_scenario(&mut commands, &assets, &lib, &fxl, &shapes, &asteroids, &cfg);
}

/// F10: back to the main menu (the retail in-game menu is not done).
pub fn leave_battle(keys: Res<ButtonInput<KeyCode>>, mut commands: Commands, old: Query<Entity, Or<(With<Scenario>, With<Transient>)>>, mut next: ResMut<NextState<menu::Screen>>) {
    if !keys.just_pressed(KeyCode::F10) {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    commands.remove_resource::<campaign::Mission>();
    commands.insert_resource(ship::Fleets::default());
    commands.queue(|world: &mut World| {
        world.resource_mut::<Orbit>().focus = None;
    });
    next.set(menu::Screen::Main);
}

pub fn spawn_ship(commands: &mut Commands, assets: &AssetServer, lib: &Library, fxl: &fx::FxLib, name: &str, side: Side, pos: Vec3, heading: f32, label: Option<String>) -> Entity {
    let ci = lib.index(name);
    let class = &lib.classes[ci];
    let mut e = commands.spawn((
        Scenario,
        Ship {
            class: ci,
            side,
            heading,
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
        },
        Health::new(class),
        collision::Drift::default(),
        combat::DamageFx::new(class),
        combat::Fires::default(),
        // Skirmish ships can be boarded; campaign missions override this with the Setup Ship flag (campaign.rs).
        boarding::Boardable::ship(&class.name, boarding::Skill::Average, true),
        WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(class.glb.clone()))),
        Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(heading)),
    ));
    if side == Side::Player {
        e.insert(PlayerShip);
    }
    if let Some(label) = label {
        e.insert(ShipName(label));
    }
    // Tenders carry dock points on every map type (skirmish, historical, campaign).
    let docks = serde_json::json!({ name: data::try_json(&assets_dir().join(format!("ships/{name}/docks.json"))) });
    if let Some(d) = crate::docking::dockable_for(&docks, name) {
        e.insert(d);
    }
    let id = e.id();
    let guns = combat::spawn_guns(commands, assets, lib, id, ci);
    commands.entity(id).insert(guns);
    combat::spawn_engines(commands, fxl, lib, id, ci, Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(heading)));
    id
}

/// Game axes (x starboard, y forward, z up) to Bevy (x, z, -y), as in the GLB export.
/// Missing components read as 0 (`data::conv_v` is the strict form for ship data).
pub fn game_to_bevy(v: &serde_json::Value) -> Vec3 {
    let f = |i: usize| v[i].as_f64().unwrap_or(0.0) as f32;
    data::conv(f(0), f(1), f(2))
}

/// A fresh scenario must not inherit pause from the battle it replaces; keep the chosen game speed.
pub fn reset_fleets(world: &mut World) {
    world.insert_resource(ship::Fleets::default());
    world.insert_resource(crate::environment::Sensors::default());
    if let Some(mut orbit) = world.get_resource_mut::<Orbit>() {
        orbit.focus = None;
    }
    world.insert_resource(Target::default());
    if let Some(mut hud) = world.get_resource_mut::<panels::FleetHud>() {
        *hud = default();
    }
    if let Some(menu) = world.get_resource::<options_menu::OptionsMenu>() {
        menu.reset();
    }
    if let Some(mut request) = world.get_resource_mut::<options_menu::BarMenuRequest>() {
        request.0 = None;
    }
}
pub fn resume_scenario(world: &mut World) {
    reset_fleets(world);
    world.resource_mut::<Time<Virtual>>().unpause();
}

/// Skirmish on a retail map (assets/maps/<id>.json from World/*.twt): scenery, skybox and music from the map; our fleet at
/// the first player's start point, the enemy fleet at the first player of another team, both in formation facing their
/// start vectors. Without a map: the test arena (player at the origin, enemies on a ring).
pub fn spawn_scenario(commands: &mut Commands, assets: &AssetServer, lib: &Library, fxl: &fx::FxLib, shapes: &collision::Shapes, asteroids: &data::AsteroidDefs, cfg: &Config) {
    commands.queue(resume_scenario);
    let map = cfg.map.as_deref().and_then(data::map_json);
    let sky = map.as_ref().and_then(|m| m["sky"].as_str()).map(|s| s.rsplit_once("_Layer").map_or(s, |x| x.0).to_string()).unwrap_or("skybox_07".into());
    commands.spawn((
        Scenario,
        Sky,
        WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(data::model(&format!("{sky}.glb"))))),
        Transform::from_scale(Vec3::splat(8000.0)),
        Visibility::default(),
    ));
    let track = map.as_ref().and_then(|m| m["music"].as_str()).unwrap_or("BTL_NavyBig_Full02");
    if !campaign::is_mission(cfg.map.as_deref()) {
        // the mission's rules choose its music
        let m = audio::music(commands, assets, track);
        commands.entity(m).insert(Scenario);
    }
    let mut bases: Vec<(Entity, serde_json::Value)> = vec![]; // scenery with an object id: candidates for rule-addressable bases
    let ours: Vec<String> = std::iter::once(cfg.player.clone()).chain(cfg.allies.iter().cloned()).collect();
    if let Some(map) = &map {
        let docks = crate::docking::load_docks();
        for o in map["objects"].as_array().into_iter().flatten() {
            let glb = format!("{}.glb", o["scene"].as_str().unwrap_or_default());
            let rotation = game_rotation(&o["rot"]);
            let tf = scenery_transform(o, rotation, &docks);
            let e = commands.spawn((Scenario, WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(data::model(&glb)))), tf)).id();
            if let Some(body) = shapes.obstacle(o["type"].as_str().unwrap_or_default(), &tf) {
                commands.entity(e).insert(body);
            }
            // Every base/island with dock points is dockable on every map type, not only campaign bases a rule sets up.
            if let Some(d) = o["type"].as_str().and_then(|t| crate::docking::dockable_for(&docks, t)) {
                commands.entity(e).insert(d);
            }
            if let Some(t) = o["type"].as_str().filter(|t| t.starts_with("Terrain_BlackHole")) {
                // The funnel mesh is not drawn: the reference video shows only the effect's glow, beam and ribbons.
                commands.entity(e).insert(Visibility::Hidden);
                blackhole::spawn(commands, fxl, t, game_to_bevy(&o["pos"]));
            }
            if let Some(mut rock) = combat::Asteroid::new(o["type"].as_str().unwrap_or(""), o["group"].as_str().unwrap_or(""), asteroids) {
                rock.id = o["id"].as_i64().unwrap_or(-1);
                commands.entity(e).insert(rock);
            }
            if o["id"].is_i64() {
                bases.push((e, o.clone()));
            }
        }
        nebula::spawn(commands, assets, fxl, map);
        if campaign::is_mission(cfg.map.as_deref()) {
            campaign::spawn(commands, assets, lib, fxl, cfg.map.as_deref().unwrap(), &bases);
            commands.insert_resource(Target::default());
            commands.insert_resource(Outcome::Playing);
            return;
        }
        commands.remove_resource::<campaign::Mission>();
        let players = map["players"].as_array().cloned().unwrap_or_default();
        assert!(!players.is_empty(), "map {:?} has no players", cfg.map);
        let mut groups = if cfg.groups.is_empty() {
            // Command line: our fleet at the first player, the enemy fleet at the first player of another team.
            let us = &players[0];
            let them = players.iter().position(|p| p["team"] != us["team"]).unwrap_or(players.len().min(2) - 1);
            let fleet = |f: &[String]| f.iter().map(|n| (n.clone(), None, None)).collect();
            vec![
                Group { player: 0, side: Side::Player, human: true, ships: fleet(&ours), weapons: vec![] },
                Group { player: them, side: Side::Enemy, human: false, ships: fleet(&cfg.enemies), weapons: vec![] },
            ]
        } else {
            cfg.groups.iter().map(|g| Group { player: g.player, side: g.side, human: g.human, ships: g.ships.clone(), weapons: g.weapons.clone() }).collect()
        };
        if cfg.script.as_deref() == Some("fleet")
            && let Some(player) = (0..players.len()).find(|i| !groups.iter().any(|g| g.player == *i))
        {
            groups.push(Group { player, side: Side::Player, human: false, ships: vec![(ship::DEFAULT_SHIP.into(), None, Some("Allied captain".into()))], weapons: vec![] });
        }
        let mut namer = Namer::new();
        for g in &groups {
            let Some(p) = players.get(g.player) else { continue };
            let f = game_to_bevy(&p["forward"]);
            let heading = (-f.x).atan2(-f.z);
            let radii: Vec<f32> = g.ships.iter().map(|(n, ..)| lib.classes[lib.index(n)].radius).collect();
            let kind = ship::Formation::from_index(p["formation"].as_u64().unwrap_or(0) as usize);
            let slots: Vec<_> = ship::formation_slots(kind, &radii).into_iter().map(|v| game_to_bevy(&p["start"]) + Quat::from_rotation_y(heading) * v).collect();
            let owner = format!("player{}", g.player);
            if g.human {
                let owner = owner.clone();
                commands.queue(move |w: &mut World| {
                    w.resource_mut::<ship::Fleets>().local = owner;
                });
            }
            for (i, ((name, at, label), slot)) in g.ships.iter().zip(slots).enumerate() {
                let pos = at.map_or(slot, |a| game_to_bevy(&serde_json::json!(a)));
                let e = spawn_ship(commands, assets, lib, fxl, name, g.side, pos, heading, label.clone().or_else(|| namer.pick(lib, name)));
                commands.entity(e).insert(ship::FleetMember(owner.clone(), i));
                if !g.human {
                    commands.entity(e).entry::<Ship>().and_modify(|mut s| (s.crew_speed, s.crew_turn) = (ship::COMPUTER_CREW, ship::COMPUTER_CREW));
                    // PROVISIONAL: the crew factor matching the measured speed (Average-to-Elite, retail save scale)
                    commands.entity(e).insert(crate::environment::ScanFactor(1.35));
                }
                if let Some(fit) = g.weapons.get(i).filter(|fit| !fit.is_empty()) {
                    commands.entity(e).insert(crate::crew_arms::WeaponFit(fit.clone()));
                }
                if g.side == Side::Player && !(g.human && i == 0) {
                    commands.entity(e).remove::<PlayerShip>();
                }
            }
            let owner = owner.clone();
            commands.queue(move |w: &mut World| ship::set_formation(w, &owner, Some(kind), kind != ship::Formation::None));
        }
    } else {
        commands.queue(|w: &mut World| {
            w.resource_mut::<ship::Fleets>().local = "Player".into();
        });
        let mut namer = Namer::new();
        for (i, name) in ours.iter().enumerate() {
            let e = spawn_ship(commands, assets, lib, fxl, name, Side::Player, Vec3::new(40.0 * i as f32, 0.0, 0.0), 0.0, namer.pick(lib, name));
            commands.entity(e).insert(ship::FleetMember("Player".into(), i));
            if i > 0 {
                commands.entity(e).remove::<PlayerShip>();
            }
        }
        for (i, name) in cfg.enemies.iter().enumerate() {
            let test = matches!(cfg.script.as_deref(), Some("fxtest") | Some("target"));
            let a = if test { 0.0 } else { 0.9 + i as f32 * 0.9 };
            let d = if test { 70.0 } else { 130.0 + 25.0 * i as f32 };
            let pos = Vec3::new(a.cos() * d, 0.0, a.sin() * d);
            spawn_ship(commands, assets, lib, fxl, name, Side::Enemy, pos, pos.x.atan2(pos.z), namer.pick(lib, name));
        }
    }
    commands.insert_resource(Target::default());
    commands.insert_resource(Outcome::Playing);
}

/// Dev cheats: K destroys all enemies, J destroys the player. `--script victory|defeat` presses one at frame 300 (demo).
/// Scripted runs (`--script target`): pick the first enemy so the target reticle can be screenshotted (`fxtest`: so it fires).
pub fn shot_target(
    cfg: Res<Config>,
    mut target: ResMut<Target>,
    mut orbit: ResMut<Orbit>,
    mut ships: Query<(Entity, &mut Ship)>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    time: Res<Time<Virtual>>,
) {
    // Runtime self-check: Pause key freezes simulation time, then resumes it while the HUD stays usable.
    if matches!(cfg.script.as_deref(), Some("pause" | "pause-restart")) {
        let resume_key = if cfg.script.as_deref() == Some("pause-restart") { KeyCode::KeyR } else { KeyCode::Pause };
        match cfg.frame {
            30 => {
                keys.press(KeyCode::Pause);
            }
            31 => {
                keys.release(KeyCode::Pause);
            }
            60 => {
                keys.press(resume_key);
            }
            61 => {
                keys.release(resume_key);
            }
            40 => {
                assert!(time.is_paused(), "Pause must pause the simulation");
                assert_eq!(time.delta_secs(), 0.0, "paused simulation time must not advance");
            }
            70 => assert!(!time.is_paused(), "resume/restart must unpause the simulation"),
            _ => {}
        }
    }
    if cfg.script.as_deref() == Some("speedtest") {
        for (_, mut s) in &mut ships {
            if s.side == Side::Player {
                s.order = ((cfg.frame / 90) % 4) as usize;
            }
        }
    }
    if cfg.script.as_deref() == Some("course") && cfg.frame == 3 {
        for (_, mut s) in &mut ships {
            if s.side == Side::Player {
                s.waypoints = vec![Vec3::new(-90.0, 0.0, -80.0), Vec3::new(40.0, 0.0, -200.0)];
                s.order = 2;
                s.goal = None;
            }
        }
    }
    // fxtest clicks the enemy too: the command ship only fires at a clicked target.
    if matches!(cfg.script.as_deref(), Some("target") | Some("fxtest")) && cfg.frame >= 5 && target.0.is_none() {
        target.0 = ships.iter().find(|(_, s)| s.side == Side::Enemy).map(|(e, _)| e);
        if cfg.script.as_deref() == Some("target") {
            orbit.focus = target.0; // screenshots look at the target
        }
    }
}

pub fn debug_kill(
    keys: Res<ButtonInput<KeyCode>>,
    cfg: Res<Config>,
    mut lost: MessageWriter<lifeboats::ShipLost>,
    mut ships: Query<(Entity, &Ship, &mut Health, &mut Visibility), Without<lifeboats::Lifeboat>>,
) {
    // Cheat keys only in debug builds or scripted runs, so a stray H/J/K never ends a real game.
    let keys_on = cfg!(debug_assertions) || cfg.script.is_some();
    let pressed = |k| keys_on && keys.just_pressed(k);
    let hurt = pressed(KeyCode::KeyH) || (cfg.shot.is_some() && cfg.script.as_deref() == Some("damage") && cfg.frame == 30);
    let s = cfg.script.as_deref();
    let at = cfg.shot.is_some() && cfg.frame == 300;
    let (kill_enemies, kill_player) = (pressed(KeyCode::KeyK) || (at && matches!(s, Some("victory") | Some("restart"))), pressed(KeyCode::KeyJ) || (at && s == Some("defeat")));
    for (e, ship, mut h, mut vis) in &mut ships {
        if hurt && ship.side == Side::Player {
            for hp in h.sections.iter_mut() {
                *hp *= 0.35;
            }
            h.sections[1] = 0.0; // stern destroyed: shows the Damage_ swap mesh
        }
        if (ship.side == Side::Enemy && kill_enemies) || (ship.side == Side::Player && kill_player) {
            if !h.destroyed {
                lost.write(lifeboats::ShipLost(e)); // a dev kill stands in for a combat death
            }
            h.core = 0.0;
            h.destroyed = true;
            *vis = Visibility::Hidden;
        }
    }
}

pub fn restart(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    fxl: Res<fx::FxLib>,
    mut cfg: ResMut<Config>,
    mission: Option<Res<campaign::Mission>>,
    shapes: Res<collision::Shapes>,
    asteroids: Res<data::AsteroidDefs>,
    old: Query<Entity, Or<(With<Scenario>, With<Transient>)>>,
) {
    let scripted = cfg.script.as_deref() == Some("restart") && cfg.frame == 400;
    // N after a won mission continues the campaign.
    let next = mission.filter(|m| keys.just_pressed(KeyCode::KeyN) && m.script.ended.as_ref().is_some_and(|e| e.1)).and_then(|m| campaign::next(&m.id));
    if !keys.just_pressed(KeyCode::KeyR) && !scripted && next.is_none() {
        return;
    }
    if next.is_some() {
        cfg.map = next;
    }
    cfg.ended = None;
    for e in &old {
        commands.entity(e).despawn();
    }
    spawn_scenario(&mut commands, &assets, &lib, &fxl, &shapes, &asteroids, &cfg);
}

pub fn outcome(mut out: ResMut<Outcome>, mut cfg: ResMut<Config>, player: Query<&Health, With<PlayerShip>>, ships: Query<(&Health, &Ship), Without<PlayerShip>>) {
    let enemies = || ships.iter().filter(|(_, s)| s.side == Side::Enemy).map(|(h, _)| h);
    if campaign::is_mission(cfg.map.as_deref()) {
        return;
    } // missions end through their own rules
    let new = if player.single().is_err() || player.single().is_ok_and(|h| h.destroyed) {
        Outcome::Defeat
    } else if enemies().count() > 0 && enemies().all(|h| h.destroyed) {
        Outcome::Victory
    } else {
        Outcome::Playing
    };
    if new != *out && new != Outcome::Playing {
        info!("{}", if new == Outcome::Victory { "VICTORY" } else { "DEFEAT" });
        cfg.ended = Some(cfg.frame);
    }
    *out = new;
}

/// Keep meshes, gun mounts, dock points and collision in the same placed scene frame.
// PROVISIONAL: anchor the first dock to the authored map altitude; native scene assembly is not decoded.
pub fn scenery_transform(object: &serde_json::Value, rotation: Quat, docks: &serde_json::Value) -> Transform {
    let origin = object.get("scene_origin").map_or_else(
        || {
            let kind = object["type"].as_str().unwrap_or_default();
            Vec3::Y * docks[kind]["points"][0]["pos"][2].as_f64().unwrap_or(0.0) as f32
        },
        game_to_bevy,
    );
    Transform::from_translation(game_to_bevy(&object["pos"]) - rotation * origin).with_rotation(rotation)
}

/// TWT stores native matrix columns without reordering; B = C M Cᵀ, C(x,y,z) = (x,z,-y).
pub fn game_rotation(r: &serde_json::Value) -> Quat {
    let m = |i: usize| r[i].as_f64().unwrap_or(if i.is_multiple_of(4) { 1.0 } else { 0.0 }) as f32;
    let game = Mat3::from_cols(Vec3::new(m(0), m(1), m(2)), Vec3::new(m(3), m(4), m(5)), Vec3::new(m(6), m(7), m(8)));
    let c = Mat3::from_cols(Vec3::X, Vec3::NEG_Z, Vec3::Y);
    Quat::from_mat3(&(c * game * c.transpose())).normalize()
}

#[cfg(test)]
mod rotation_tests {
    use super::*;
    #[test]
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
            .insert_resource(Library::load(&crate::assets_dir()))
            .init_resource::<fx::FxLib>()
            .init_resource::<collision::Shapes>()
            .init_resource::<data::AsteroidDefs>()
            .init_resource::<ship::Fleets>()
            .init_resource::<Target>()
            .insert_resource(Orbit { yaw: 0.0, pitch: 0.0, dist: 60.0, focus: None })
            .insert_resource(Config {
                groups: vec![],
                player: "Ship_Navy_Tender".into(),
                allies: vec![],
                enemies: vec!["Ship_Navy_Cutter".into()],
                map: Some("red_rover".into()),
                shot: None,
                frame: 0,
                speed: 1.0,
                ended: None,
                script: None,
                shot_frame: 0,
                late_frame: 0,
            });
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

    #[test]
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
}
