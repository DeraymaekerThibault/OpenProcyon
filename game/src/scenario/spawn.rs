//! Spawning a battle: ships, map scenery, fleets, bases and the test arena.
use super::*;
use crate::scenario::Race;

/// Names ships that no script names: a random not-yet-used entry of the ship type's pool, as open maps do (WOT 'Available Unique
/// Ship Name'); a used-up pool repeats names. PROVISIONAL: the original's selection (and its uniqueness scope) is unknown.
pub struct Namer {
    pub used: std::collections::HashSet<String>,
    pub seed: u64,
}

impl Namer {
    #[allow(clippy::new_without_default)] // time-seeded: a `Default` would hide the randomness
    pub fn new() -> Self {
        Namer { used: Default::default(), seed: crate::clock_seed() | 1 }
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

/// The colour of the map player that owns a ship (`players[].color` of the map): its name banner is tinted with it, and the
/// roster lists the player's name in it.
#[derive(Component, Clone, Copy)]
pub struct PlayerColor(pub Color);

/// A skirmish ship's map identity for the end rules: the world-object id the map gave it (historical maps) and the side it started on.
#[derive(Component, Clone, Copy)]
pub struct MapObject {
    pub id: Option<i64>,
    pub start: Side,
}

/// The fleet type a base or player name stands for: "navybase", "proc base", "Piratebase",
/// `Base_Grant`... PROVISIONAL: read from the retail names, the TWT carries no explicit owner side.
pub fn faction(name: &str) -> Option<Race> {
    let n = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if has(&["pirate", "privateer"]) {
        Some(Race::Pirate)
    } else if has(&["proc"]) {
        Some(Race::Procyon)
    } else if has(&["navy", "grant", "montgomery", "shipyard", "port", "regina"]) {
        Some(Race::Navy)
    } else {
        None
    }
}

pub fn spawn_ship(commands: &mut Commands, assets: &AssetServer, lib: &Library, fxl: &fx::FxLib, name: &str, side: Side, pos: Vec3, heading: f32, label: Option<String>) -> Entity {
    let ci = lib.index(name);
    let class = &lib.classes[ci];
    let mut e = commands.spawn((
        Scenario,
        Ship { heading, ..Ship::new(ci, side) },
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

/// The map's scenery (`objects`): models with their collision bodies, dock points, island sounds, black holes and asteroids.
/// Returns the objects with an id (candidates for rule-addressable bases).
fn spawn_map_objects(
    commands: &mut Commands,
    assets: &AssetServer,
    fxl: &fx::FxLib,
    shapes: &collision::Shapes,
    asteroids: &data::AsteroidDefs,
    map: &serde_json::Value,
) -> Vec<(Entity, serde_json::Value)> {
    let mut bases = vec![];
    let docks = crate::docking::load_docks();
    let island_sounds = data::try_json(&crate::assets_dir().join("data/island_sounds.json")).unwrap_or_default();
    let mesh_effects = data::try_json(&crate::assets_dir().join("data/mesh_effects.json")).unwrap_or_default();
    for (i, o) in map["objects"].as_array().into_iter().flatten().enumerate() {
        let ty = o["type"].as_str().unwrap_or_default();
        let glb = format!("{}.glb", o["scene"].as_str().unwrap_or_default());
        let tf = scenery_transform(o, game_rotation(&o["rot"]), &docks);
        let e = commands.spawn((Scenario, crate::data::scene(assets, &glb), tf, super::SceneryIndex(i as u32))).id();
        commands.entity(e).insert((crate::clips::SceneLoop(o["scene"].as_str().unwrap_or_default().into()), data::crew_nodes::SceneryType(ty.into())));
        if let Some(clip) = crate::environment::Clip::scenery(assets, &glb) {
            commands.entity(e).insert(clip);
        }
        if let Some(body) = shapes.obstacle(ty, &tf) {
            commands.entity(e).insert(body);
        }
        // Every base/island with dock points is dockable on every map type, not only campaign bases a rule sets up.
        if let Some(d) = crate::docking::dockable_for(&docks, ty) {
            commands.entity(e).insert(d);
        }
        if let Some(i) = island_sounds.get(ty) {
            commands.entity(e).insert(crate::audio::IslandLoop { sound: i["sound"].as_str().unwrap_or_default().into(), max: i["distance"].as_f64().unwrap_or(200.0) as f32 });
        }
        for p in mesh_effects[o["scene"].as_str().unwrap_or_default()].as_array().into_iter().flatten() {
            let name = |k: &str| p[k].as_str().unwrap_or_default();
            if let Some(effect) = ambient_effect(name("kind"), name("effect")) {
                let local = Transform::from_translation(game_to_bevy(&p["pos"]));
                fx::spawn(commands, fxl, effect, Some(e), local, tf * local, false);
            }
        }
        if ty.starts_with("Terrain_BlackHole") {
            // The funnel mesh is not drawn: the reference video shows only the effect's glow, beam and ribbons.
            commands.entity(e).insert(Visibility::Hidden);
            blackhole::spawn(commands, fxl, ty, game_to_bevy(&o["pos"]));
        }
        if let Some(mut rock) = combat::Asteroid::new(ty, o["group"].as_str().unwrap_or(""), asteroids) {
            rock.id = o["id"].as_i64().unwrap_or(-1);
            commands.entity(e).insert(rock);
        }
        if o["id"].is_i64() {
            bases.push((e, o.clone()));
        }
    }
    bases
}

/// The effect file of one ambient point of a mesh scene (`data/mesh_effects.json`): the effect its world object names for the
/// point (`effect`), or the volcano smoke for `VolcanoSmoke` points.
pub(super) fn ambient_effect<'a>(kind: &str, effect: &'a str) -> Option<&'a str> {
    if kind == "VolcanoSmoke" {
        return Some("VolcanoSmoke");
    }
    Some(effect).filter(|e| !e.is_empty())
}

/// Skirmish on a retail map (assets/maps/<id>.json from World/*.twt): scenery, skybox and music from the map; our fleet at
/// the first player's start point, the enemy fleet at the first player of another team, both in formation facing their
/// start vectors. Without a map: the test arena (player at the origin, enemies on a ring).
pub fn spawn_scenario(commands: &mut Commands, assets: &AssetServer, lib: &Library, fxl: &fx::FxLib, shapes: &collision::Shapes, asteroids: &data::AsteroidDefs, cfg: &Config) {
    info!("battle start: map={:?} player={} allies={:?} enemies={:?}", cfg.map, cfg.player, cfg.allies, cfg.enemies);
    commands.queue(super::reset_sim);
    commands.queue(resume_scenario);
    commands.insert_resource(crate::loading::Loading::default());
    commands.remove_resource::<crate::rules::skirmish::SkirmishScript>();
    let map = cfg.map.as_deref().and_then(data::map_json);
    commands.insert_resource(MapLighting(map.as_ref().and_then(lighting_of)));
    let sky = map.as_ref().and_then(|m| m["sky"].as_str()).map_or_else(|| "skybox_07".into(), |s| s.rsplit_once("_Layer").map_or(s, |x| x.0).to_string());
    let mut sky_entity = commands.spawn((Scenario, Sky, Transform::from_scale(Vec3::splat(8000.0)), Visibility::default()));
    match view::hd_sky_file(assets_dir(), &sky) {
        Some(file) => sky_entity.insert(view::HdSky(assets.load(file))),
        // The retail sphere and the original's generated stars, a new pattern each battle; a baked sky has its own stars.
        None => sky_entity.insert(crate::data::scene(assets, &format!("{sky}.glb"))).with_child((
            Mesh3d(assets.add(view::star_mesh(&view::starfield(crate::clock_seed() as u32)))),
            MeshMaterial3d(assets.add(StandardMaterial { unlit: true, ..default() })),
            bevy::light::NotShadowCaster,
            bevy::light::NotShadowReceiver,
        )),
    };
    let track = map.as_ref().and_then(|m| m["music"].as_str()).unwrap_or("BTL_NavyBig_Full02");
    let mission = campaign::is_mission(cfg.map.as_deref());
    if !mission {
        // the mission's rules choose its music
        let m = audio::music(commands, assets, track);
        commands.entity(m).insert(Scenario);
    }
    let ours: Vec<String> = std::iter::once(cfg.player.clone()).chain(cfg.allies.iter().cloned()).collect();
    // Each battle starts with its own alliance table (the mission's or the lobby's replace this one).
    commands.insert_resource(alliance::Alliances::default());
    if let Some(map) = &map {
        let bases = spawn_map_objects(commands, assets, fxl, shapes, asteroids, map);
        nebula::spawn(commands, assets, fxl, map);
        if mission {
            campaign::spawn(commands, assets, lib, fxl, cfg.map.as_deref().unwrap(), &bases);
            // Not retail: a custom mission's map places mines like a skirmish map does. Retail missions never read their `mines` key.
            if campaign::is_custom(cfg.map.as_deref().unwrap()) {
                spawn_mines(commands, map);
            }
        } else {
            spawn_skirmish(commands, assets, lib, fxl, cfg, map, &bases, &ours);
        }
    } else {
        spawn_arena(commands, assets, lib, fxl, cfg, &ours);
    }
    commands.insert_resource(Target::default());
    commands.insert_resource(Outcome::Playing);
}

/// The alliance table of a lobby's battle: each group is its own side, allied with the groups of its lobby team.
pub fn lobby_alliances(groups: &[crate::Group]) -> alliance::Alliances {
    let mut table = alliance::Alliances::default();
    for (a, b) in groups.iter().flat_map(|a| groups.iter().map(move |b| (a, b))) {
        if a.team >= 0 && a.team == b.team {
            table.offer(a.side, b.side, true);
        }
    }
    table
}

/// A skirmish map's battle: the fleets of its players at their start points, the victory rules, the map script, mines, animals and bases.
#[allow(clippy::too_many_arguments)]
fn spawn_skirmish(
    commands: &mut Commands,
    assets: &AssetServer,
    lib: &Library,
    fxl: &fx::FxLib,
    cfg: &Config,
    map: &serde_json::Value,
    bases: &[(Entity, serde_json::Value)],
    ours: &[String],
) {
    commands.remove_resource::<campaign::Mission>();
    let players = map["players"].as_array().cloned().unwrap_or_default();
    assert!(!players.is_empty(), "map {:?} has no players", cfg.map);
    let mut groups = if cfg.groups.is_empty() {
        // Command line: our fleet at the first player, the enemy fleet at the first player of another team.
        let us = &players[0];
        let them = players.iter().position(|p| p["team"] != us["team"]).unwrap_or(players.len().min(2) - 1);
        let fleet = |f: &[String]| f.iter().map(|n| (n.clone(), None, None)).collect();
        vec![
            Group {
                player: 0,
                side: Side::Player,
                human: true,
                race: Race::from_twt(players[0]["race"].as_u64().unwrap_or(4)),
                team: players[0]["team"].as_i64().unwrap_or(-1) as i32,
                row: None,
                ids: vec![],
                ships: fleet(ours),
                weapons: vec![],
                crew: vec![],
            },
            Group {
                player: them,
                side: Side::Enemy,
                human: false,
                race: Race::from_twt(players[them]["race"].as_u64().unwrap_or(4)),
                team: players[them]["team"].as_i64().unwrap_or(-1) as i32,
                row: None,
                ids: vec![],
                ships: fleet(&cfg.enemies),
                weapons: vec![],
                crew: vec![],
            },
        ]
    } else {
        cfg.groups
            .iter()
            .map(|g| Group {
                player: g.player,
                side: g.side,
                human: g.human,
                race: g.race,
                team: g.team,
                row: g.row.clone(),
                ids: g.ids.clone(),
                ships: g.ships.clone(),
                weapons: g.weapons.clone(),
                crew: g.crew.clone(),
            })
            .collect()
    };
    if cfg.script.as_deref() == Some("fleet")
        && let Some(player) = (0..players.len()).find(|i| !groups.iter().any(|g| g.player == *i))
    {
        groups.push(Group {
            player,
            side: Side::Player,
            human: false,
            race: Race::Unchosen,
            team: -1,
            row: None,
            ids: vec![],
            ships: vec![(ship::DEFAULT_SHIP.into(), None, Some("Allied captain".into()))],
            weapons: vec![],
            crew: vec![],
        });
    }
    // A map with its own script (Convoy Raid's convoys, ...): the ships of its non-playable players join the battle on their
    // team's side, and every ship carries its map player and rule name for the script (rules::skirmish).
    let map_id = cfg.map.clone().unwrap_or_default();
    // The map's script, read once for the ship names, end rules, rule script, animals and island strengths below.
    let script_json = data::try_json(&crate::assets_dir().join(format!("maps/{map_id}/script.json")));
    let script_names = script_json.as_ref().and_then(crate::rules::skirmish::ship_names);
    if script_names.is_some() {
        for (p, pl) in players.iter().enumerate() {
            if pl["playable"] == true || groups.iter().any(|g| g.player == p) {
                continue;
            }
            let placed: Vec<&serde_json::Value> = map["ships"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|o| o["player"].as_u64() == Some(p as u64) && o["type"].as_str().is_some_and(|t| lib.classes.iter().any(|c| c.name == t)))
                .collect();
            if placed.is_empty() {
                continue;
            }
            let team = pl["team"].as_i64().unwrap_or(-1) as i32;
            let side = groups.iter().find(|g| team >= 0 && g.team == team).map_or(Side(groups.iter().map(|g| g.side.0).max().unwrap_or(0) + 1), |g| g.side);
            groups.push(Group {
                player: p,
                side,
                human: false,
                race: Race::Unchosen,
                team,
                row: None,
                ids: placed.iter().map(|o| o["id"].as_i64()).collect(),
                ships: placed
                    .iter()
                    .map(|o| {
                        (o["type"].as_str().unwrap_or_default().into(), Some([0, 1, 2].map(|i| o["pos"][i].as_f64().unwrap_or(0.0) as f32)), o["name"].as_str().map(String::from))
                    })
                    .collect(),
                weapons: vec![],
                crew: vec![],
            });
        }
    }
    let mut namer = Namer::new();
    let mut roster = alliance::Roster { changeable: map["alliance_change"].as_bool().unwrap_or(false), ..default() };
    let mut alliances = lobby_alliances(&groups);
    for g in &groups {
        let Some(p) = players.get(g.player) else { continue };
        let f = game_to_bevy(&p["forward"]);
        let heading = crate::geom::heading_of(f);
        let radii: Vec<f32> = g.ships.iter().map(|(n, ..)| lib.classes[lib.index(n)].sphere.1).collect();
        let kind = ship::Formation::from_index(p["formation"].as_u64().unwrap_or(0) as usize);
        let slots: Vec<_> = ship::formation_slots(kind, &radii).into_iter().map(|v| game_to_bevy(&p["start"]) + Quat::from_rotation_y(heading) * v).collect();
        // A scripted map's ships answer to their map player's name (`MissionShip::owner`), so the fleet is keyed the same way, or the
        // fleet bookkeeping drops the human's command ship (no control, no HUD).
        let owner: crate::names::Owner = if script_names.is_some() { p["name"].as_str().unwrap_or_default().to_string() } else { format!("player{}", g.player) }.into();
        if let Some((name, flag)) = &g.row {
            let colour = p["color"]
                .as_array()
                .filter(|c| c.len() >= 3)
                .map_or(Color::WHITE, |c| Color::srgb(c[0].as_f64().unwrap_or(1.0) as f32, c[1].as_f64().unwrap_or(1.0) as f32, c[2].as_f64().unwrap_or(1.0) as f32));
            roster.rows.push(alliance::Row::new(owner.clone(), name.clone(), *flag, colour, g.side));
        }
        alliances.add_player(&owner, g.side);
        if g.human {
            let owner = owner.clone();
            commands.queue(move |w: &mut World| {
                w.resource_mut::<ship::Fleets>().local = owner;
            });
        }
        for (i, ((name, at, label), slot)) in g.ships.iter().zip(slots).enumerate() {
            let pos = at.map_or(slot, |a| game_to_bevy(&serde_json::json!(a)));
            let e = spawn_ship(commands, assets, lib, fxl, name, g.side, pos, heading, label.clone().or_else(|| namer.pick(lib, name)));
            let id = g.ids.get(i).copied().flatten();
            commands.entity(e).insert((ship::FleetMember(owner.clone(), i), MapObject { id, start: g.side }));
            if let Some(c) = p["color"].as_array().filter(|c| c.len() >= 3) {
                let k = |i: usize| c[i].as_f64().unwrap_or(0.0) as f32;
                commands.entity(e).insert(PlayerColor(Color::srgb(k(0), k(1), k(2))));
            }
            if let Some(names) = &script_names {
                commands.entity(e).insert(campaign::MissionShip::placed(
                    id.unwrap_or(-1),
                    id.and_then(|id| names.get(&id).cloned()).unwrap_or_default(),
                    p["name"].as_str().unwrap_or_default().into(),
                ));
            }
            // A computer fleet built in the lobby carries its crew (`SkirmishCrew` below); its ships left without crew have empty posts
            // like ours (retail saves: factor 1.125, scan x1.125). Pre-placed computer ships keep the measured crewed factor.
            let built = g.crew.iter().any(|c| c.iter().any(|r| !r.is_empty()));
            if !g.human && !built {
                commands.entity(e).entry::<Ship>().and_modify(|mut s| (s.crew_speed, s.crew_turn) = (ship::COMPUTER_CREW, ship::COMPUTER_CREW));
                // PROVISIONAL: the crew factor matching the measured speed (Average-to-Elite, retail save scale)
                commands.entity(e).insert(crate::environment::ScanFactor(1.35));
            }
            if let Some(fit) = g.weapons.get(i).filter(|fit| !fit.is_empty()) {
                commands.entity(e).insert(crate::crew_arms::WeaponFit(fit.clone()));
            }
            if let Some(crew) = g.crew.get(i).filter(|c| c.iter().any(|r| !r.is_empty())) {
                commands.entity(e).insert(crate::crew_arms::SkirmishCrew(crew.clone()));
            } else if g.human || built {
                // No crew assigned: every post holds the rating-1 hand, which still gives the lookout factor 1.125 (retail saves).
                let scan = crate::crew_arms::CrewPosts(Default::default()).scan(&crate::boarding::counts(name));
                commands.entity(e).insert(crate::environment::ScanFactor(scan));
            }
            if g.side == Side::Player && !(g.human && i == 0) {
                commands.entity(e).remove::<PlayerShip>();
            }
        }
        let owner = owner.clone();
        commands.queue(move |w: &mut World| ship::set_formation(w, &owner, Some(kind), kind != ship::Formation::None));
    }
    if roster.rows.is_empty() {
        commands.remove_resource::<alliance::Roster>();
    } else {
        commands.insert_resource(roster);
    }
    // The map's own end rules, when we model all of them (else the plain "no enemy ships left" outcome stays).
    let map_teams = map["teams"].as_array().cloned().unwrap_or_default();
    let teams: Vec<(String, Vec<Side>)> = if map_teams.is_empty() {
        // Without map teams: one per lobby team (a slot on no team is its own).
        let mut teams: Vec<(String, Vec<Side>)> = vec![];
        for g in &groups {
            let name = if g.team >= 0 { format!("team{}", g.team) } else { format!("side{}", g.side.0) };
            match teams.iter_mut().find(|t| t.0 == name) {
                Some(t) if !t.1.contains(&g.side) => t.1.push(g.side),
                Some(_) => {}
                None => teams.push((name, vec![g.side])),
            }
        }
        teams
    } else {
        map_teams
            .iter()
            .enumerate()
            .map(|(i, t)| (t["name_id"].as_str().unwrap_or_default().to_string(), groups.iter().filter(|g| g.team == i as i32).map(|g| g.side).collect()))
            .collect()
    };
    // A condition of the map's end rules may name a group of its script instead of listing object ids. A map that lets its script
    // decide (`"victory": "script"`) needs the victory rules even without usable map rules of its own.
    let script_groups = script_json.as_ref().map_or(serde_json::Value::Null, |s| s["groups"].clone());
    let scripted = script_json.as_ref().is_some_and(crate::rules::skirmish::script_decides);
    match victory::Victory::from_map_in(map, teams.clone(), &script_groups) {
        Some(v) => commands.insert_resource(v),
        None if scripted => commands.insert_resource(victory::Victory::scripted(teams)),
        None => commands.remove_resource::<victory::Victory>(),
    }
    // Every map player in the alliance table (a base player such as 'pirate base' fights for the fleet of its type, as the bases
    // below do), for the script's owner changes and alliance actions.
    for (i, p) in players.iter().enumerate() {
        let team = p["team"].as_i64().unwrap_or(-1) as i32;
        let Some(name) = p["name"].as_str() else { continue };
        let side = groups.iter().find(|g| g.player == i).or_else(|| groups.iter().find(|g| team >= 0 && g.team == team)).map(|g| g.side);
        if let Some(side) = side.or_else(|| faction(name).and_then(|f| groups.iter().find(|g| g.race == f)).map(|g| g.side)) {
            alliances.add_player(name, side);
        }
    }
    commands.insert_resource(alliances);
    let rosters = map_teams
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let names = players.iter().filter(|p| p["team"].as_i64() == Some(i as i64)).filter_map(|p| p["name"].as_str().map(String::from)).collect();
            (t["name_id"].as_str().unwrap_or_default().to_string(), names)
        })
        .collect();
    match script_json.as_ref().map(|data| crate::rules::skirmish::SkirmishScript::load(&map_id, data, rosters)) {
        Some(s) => commands.insert_resource(s),
        None => {
            if campaign::start_requested() {
                warn!("--fire-rule / --start-at: map '{map_id}' has no script.json, so there are no rules to fire and nothing runs them; they are ignored");
            }
            commands.remove_resource::<crate::rules::skirmish::SkirmishScript>();
        }
    }
    // Mines the map placed (Border Dispute, Red Rover): the script gives each field to a player.
    spawn_mines(commands, map);
    // The map's space whales and dragons (named in its script's environment).
    if let Some(script) = &script_json {
        crate::environment::spawn(commands, assets, script, true);
    }
    // The map script's Setup Island 'Combat Strength' is what boarders must wear down (retail: it seeds the base's strength).
    let strengths = script_json.as_ref().map(setup_strengths).unwrap_or_default();
    spawn_bases(commands, assets, lib, bases, &players, &groups, &strengths);
}

/// The mines a map places (`mines`: `[{type, id, pos}]`), resting where the map put them until a script hands them to a player.
fn spawn_mines(commands: &mut Commands, map: &serde_json::Value) {
    for m in map["mines"].as_array().into_iter().flatten() {
        let (Some(bullet), Some(id)) = (m["type"].as_str(), m["id"].as_i64()) else { continue };
        commands.spawn((Scenario, combat::MapMine { id, bullet: bullet.into(), owner: String::new(), side: None }, Transform::from_translation(game_to_bevy(&m["pos"]))));
    }
}

/// Armed bases and islands of a skirmish map: each fights for the side whose fleet type is theirs, with guns, damage and boarding state.
fn spawn_bases(
    commands: &mut Commands,
    assets: &AssetServer,
    lib: &Library,
    bases: &[(Entity, serde_json::Value)],
    players: &[serde_json::Value],
    groups: &[Group],
    strengths: &std::collections::HashMap<i64, i32>,
) {
    // Bases fight for the side whose fleet type is theirs; a base no fleet claims stays neutral (and silent). Armed islands (the
    // volcanoes) belong to nobody: they fire on every ship and cannot be boarded. The armed asteroids (Convoy Raid's belong to the
    // pirate base's player) fight for their object's owner like a base.
    for (e, o) in bases {
        let Some(class) = o["type"]
            .as_str()
            .filter(|t| t.starts_with("Base_") || t.starts_with("Island_") || (t.starts_with("Asteroid_") && t.ends_with("_Armed")))
            .and_then(|t| lib.classes.iter().position(|c| c.name == t))
        else {
            continue;
        };
        // PROVISIONAL: a gunless base (Convoy Raid's pirate stockade) stays scenery in skirmish, as before it had a class: whether the
        // original lets skirmish fleets shoot or board it is not observed. Missions use its vitalToMission section (mission 6).
        if lib.classes[class].mounts.is_empty() {
            continue;
        }
        let island = lib.classes[class].name.starts_with("Island_");
        let owner = o["player"].as_u64().and_then(|i| players.get(i as usize)).and_then(|p| p["name"].as_str()).and_then(faction);
        let Some(side) =
            (if island { Some(HOSTILE_TO_ALL) } else { owner.or_else(|| o["type"].as_str().and_then(faction)).and_then(|f| groups.iter().find(|g| g.race == f)).map(|g| g.side) })
        else {
            continue;
        };
        let id = o["id"].as_i64().unwrap_or(-1);
        let states = combat::spawn_guns(commands, assets, lib, *e, class);
        commands.entity(*e).insert((
            campaign::MissionBase { id, owner: format!("base{id}") },
            combat::ArmedBase { class },
            combat::DamageFx::new(&lib.classes[class]),
            combat::BaseSide(side),
            combat::BaseOrigin(side),
            combat::Health::new(&lib.classes[class]),
            states,
        ));
        if island {
            // An island no setup rule touches shoots with perfect aim (docs/re/volcanoes.md).
            commands.entity(*e).insert(combat::IslandGunnery { accuracy: 0.0, silent: false, neutral: false });
        }
        if !island {
            commands.entity(*e).insert(boarding::Boardable::base(&lib.classes[class].name, strengths.get(&id).copied().filter(|s| *s > 0).unwrap_or(SKIRMISH_BASE_STRENGTH), true));
        }
    }
}

/// The test arena (a battle without a map): the player at the origin, the enemies on a ring.
fn spawn_arena(commands: &mut Commands, assets: &AssetServer, lib: &Library, fxl: &fx::FxLib, cfg: &Config, ours: &[String]) {
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

/// Setup Island 'Combat Strength' by world-object id, from a map script.
pub(super) fn setup_strengths(script: &serde_json::Value) -> std::collections::HashMap<i64, i32> {
    let actions = script["rules"].as_array().into_iter().flatten().flat_map(|r| r["actions"].as_array().into_iter().flatten());
    actions.filter(|a| a["Type"] == "*State Init* Setup Island").filter_map(|a| Some((a["World Object ID"].as_i64()?, a["Combat Strength"].as_i64()? as i32))).collect()
}

/// The side of an armed island: no ship belongs to it, so it fires on all of them.
pub const HOSTILE_TO_ALL: Side = Side(u8::MAX);

/// Boarding strength of a skirmish base whose map script gives none (the retail strengths are the script's 'Combat Strength'). PROVISIONAL.
const SKIRMISH_BASE_STRENGTH: i32 = 50;

/// Keep meshes, gun mounts, dock points and collision in the same placed scene frame.
// PROVISIONAL: anchor the first dock to the authored map altitude; docks already carry their layer offset, so this shifts only
// scenery whose dock stays off the flight plane after it (docs/re/scenery-placement.md section 2).
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
