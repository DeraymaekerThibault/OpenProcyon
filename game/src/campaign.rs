//! Mission 1 setup: retail ship placements and the HUD text nodes. All behaviour comes from the data-driven rule interpreter in
//! rules.rs (World/SinglePlayer/Mission01.twt); this file only spawns the units and holds their mission state.
use crate::{PlayerShip, Side, assets_dir, combat::Library, game_to_bevy};
use bevy::prelude::*;
use serde_json::Value;

/// Mission state of one unit. `owner` is the rules' player name (changes with 'Set Group/Unit Owner'); `id` its TWT world object id.
#[derive(Component, Default)]
pub struct MissionShip {
    pub id: i64,
    pub name: String,
    pub owner: String,
    pub hold_fire: bool,
    pub invuln: f32,
    pub visible: bool,
    pub stance: rules::Stance,
    pub target: Option<Entity>,
    pub attack: Vec<String>,
    pub flag: String,
    pub banner: String,
    pub looping: Option<Vec<Vec3>>,
    /// Moving on a script order ('to end' path or chase): halt once it runs out instead of sailing on (docs/re/movement.md).
    pub stop_at_end: bool,
    /// Per-ship 'Create Lifeboats' flag (rule `Set Lifeboat Creation State`): false until a rule sets it. Mission-start saves restart
    /// the mission, so the rules re-apply it and nothing else needs to persist it.
    pub create_lifeboats: bool,
    /// 'Set Group/Unit Border Zone': the volume the ship is leashed to, and the 'Return Point' it heads for when outside.
    pub border: Option<String>,
    pub return_point: Option<Vec3>,
    /// 'Set Group/Unit AI Captain' Captain_Human: no captain AI of its own; it takes the human fleet's orders.
    pub human: bool,
    /// 'Set Group/Unit Warning Shot Mode': its shells do no damage (mission 5's dummy charge).
    pub warning_shots: bool,
    /// 'GroupA to Ram GroupB': its attack order is flown bow-on at emergency speed.
    pub ram: bool,
}
use crate::rules;
/// A mission base/island that rules can address ('*State Init* Setup Island'): `owner` is its group, e.g. 'AbandonedCamp'.
#[derive(Component)]
pub struct MissionBase {
    pub id: i64,
    pub owner: String,
}
impl MissionBase {
    pub fn unit(&self, e: Entity, tf: &Transform, damage: f32) -> rules::Unit {
        rules::Unit {
            e,
            id: self.id,
            name: String::new(),
            owner: self.owner.clone(),
            owner_is_named_group: false,
            groups: vec![],
            pos: tf.translation,
            alive: true,
            visible: true,
            primary: false,
            damage,
            flag: String::new(),
            attack: vec![],
        }
    }
}
#[derive(Resource, Default)]
pub struct CampaignPoints {
    pub total: u32,
    /// Retail availability lists, distinct from purchased crew and equipped guns.
    pub crew: Vec<String>,
    pub arms: Vec<String>,
    pub outfit: crate::crew_arms::Outfit,
    /// Mission-start saves may carry purchases funded by points that the replay has not earned yet.
    replay_debt: u32,
    mission: String,
    last_score: u32,
}

impl CampaignPoints {
    pub fn load() -> Self {
        campaign_points_path().map_or_else(Self::default, Self::load_from)
    }

    fn load_from(path: std::path::PathBuf) -> Self {
        match std::fs::read_to_string(path) {
            Ok(json) => match serde_json::from_str::<Value>(&json) {
                Ok(v) if v.is_u64() => Self { total: v.as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0), ..Self::default() },
                Ok(v) if v.is_object() => Self {
                    total: v["points"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
                    crew: string_list(&v["crew"]),
                    arms: string_list(&v["arms"]),
                    outfit: crate::crew_arms::Outfit::from_json(&v["outfit"]),
                    replay_debt: v["replay_debt"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
                    ..Self::default()
                },
                Ok(_) => Self::default(),
                Err(err) => {
                    warn!("invalid campaign points save: {err}");
                    Self::default()
                }
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(err) => {
                warn!("could not read campaign points save: {err}");
                Self::default()
            }
        }
    }

    /// Total at the start of `mission`: what a save made now must restore, so replaying does not count the mission's points twice.
    pub fn snapshot(&self, name: &str, mission: &str) -> CampaignSave {
        let earned = if self.mission == mission { self.last_score } else { 0 };
        CampaignSave {
            name: name.into(),
            mission: mission.into(),
            points: self.total.saturating_sub(earned),
            crew: self.crew.clone(),
            arms: self.arms.clone(),
            outfit: self.outfit.clone(),
            replay_debt: earned.saturating_add(self.replay_debt).saturating_sub(self.total),
        }
    }

    pub fn restore(&mut self, s: &CampaignSave) {
        *self = Self { total: s.points, crew: s.crew.clone(), arms: s.arms.clone(), outfit: s.outfit.clone(), replay_debt: s.replay_debt, ..Self::default() };
        self.save();
    }

    pub fn record(&mut self, mission: &str, score: u32) {
        if self.record_score(mission, score) > 0 {
            self.save();
        }
    }

    fn record_score(&mut self, mission: &str, score: u32) -> u32 {
        if self.mission != mission {
            self.mission = mission.to_owned();
            self.last_score = 0;
        }
        let added = score.saturating_sub(self.last_score);
        let repaid = added.min(self.replay_debt);
        self.replay_debt -= repaid;
        self.total = self.total.saturating_add(added - repaid);
        self.last_score = score;
        added
    }

    fn save(&self) {
        if let Err(err) = self.persist() {
            warn!("could not save campaign points: {err}");
        }
    }

    pub fn persist(&self) -> Result<(), String> {
        self.save_to(campaign_points_path().ok_or("no user data directory")?).map_err(|e| e.to_string())
    }

    fn save_to(&self, path: std::path::PathBuf) -> std::io::Result<()> {
        let parent = path.parent().ok_or_else(|| std::io::Error::other("no parent directory"))?;
        std::fs::create_dir_all(parent)?;
        let temp = path.with_extension("json.tmp");
        let v = serde_json::json!({"points": self.total, "crew": self.crew, "arms": self.arms, "outfit": self.outfit.to_json(), "replay_debt": self.replay_debt});
        std::fs::write(&temp, serde_json::to_string_pretty(&v)?)?;
        std::fs::rename(temp, path)
    }
}

/// Per-user data (imported assets, saves, settings): `$XDG_DATA_HOME/openprocyon` (Linux default `~/.local/share`),
/// `%APPDATA%\openprocyon` on Windows.
pub fn user_dir() -> Option<std::path::PathBuf> {
    let var = |name| std::env::var_os(name).map(std::path::PathBuf::from);
    user_dir_from(var("XDG_DATA_HOME"), var("HOME"), var("APPDATA"), cfg!(windows), |p| p.is_dir())
}

/// `user_dir` from its environment. On Windows `%APPDATA%` comes before `HOME`, which shells such as Git Bash set; a
/// `~/.local/share/openprocyon` made that way stays in use while no `%APPDATA%\openprocyon` exists (assets and saves kept).
fn user_dir_from(
    xdg: Option<std::path::PathBuf>,
    home: Option<std::path::PathBuf>,
    appdata: Option<std::path::PathBuf>,
    windows: bool,
    exists: impl Fn(&std::path::Path) -> bool,
) -> Option<std::path::PathBuf> {
    let home = home.map(|h| h.join(".local/share").join("openprocyon"));
    let appdata = appdata.map(|a| a.join("openprocyon"));
    if let Some(xdg) = xdg {
        return Some(xdg.join("openprocyon"));
    }
    if !windows {
        return home.or(appdata);
    }
    match (appdata, home) {
        (Some(a), Some(h)) if !exists(&a) && exists(&h) => Some(h),
        (a, h) => a.or(h),
    }
}

fn campaign_points_path() -> Option<std::path::PathBuf> {
    Some(user_dir()?.join("campaign-points.json"))
}

/// A campaign save: a mission-start checkpoint plus the campaign counters carried into it (retail saves are full world snapshots,
/// see docs/re/scoring-saves.md; the rule/ship state of a running mission is not snapshotted yet, loading restarts the mission).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CampaignSave {
    pub name: String,
    pub mission: String,
    pub points: u32,
    pub crew: Vec<String>,
    pub arms: Vec<String>,
    pub outfit: crate::crew_arms::Outfit,
    pub replay_debt: u32,
}

pub fn string_list(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect()
}

impl CampaignSave {
    fn from_json(v: &Value) -> Option<Self> {
        let list = |k: &str| v[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let mission = v["mission"].as_str()?.to_owned();
        if !is_mission(Some(&mission)) {
            return None;
        }
        Some(Self {
            name: v["name"].as_str()?.into(),
            mission,
            points: u32::try_from(v["points"].as_u64()?).ok()?,
            crew: list("crew"),
            arms: list("arms"),
            outfit: crate::crew_arms::Outfit::from_json(&v["outfit"]),
            replay_debt: v["replay_debt"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
        })
    }
    fn to_json(&self) -> Value {
        serde_json::json!({"name": self.name, "mission": self.mission, "points": self.points, "crew": self.crew, "arms": self.arms, "outfit": self.outfit.to_json(), "replay_debt": self.replay_debt})
    }
}

/// Saves the Load screen offers: the shipped mission-start saves (`assets/saves`, read-only) then the player's own
/// (`<data dir>/openprocyon/saves`). The flag is true for shipped ones.
pub fn list_saves() -> Vec<(CampaignSave, bool)> {
    let read = |dir: std::path::PathBuf, shipped| {
        let mut v: Vec<_> =
            std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|e| Some((CampaignSave::from_json(&crate::data::try_json(&e.path())?)?, shipped))).collect();
        v.sort_by(|a, b| a.0.name.cmp(&b.0.name));
        v
    };
    let mut all = read(assets_dir().join("saves"), true);
    all.extend(user_dir().into_iter().flat_map(|d| read(d.join("saves"), false)));
    all
}

/// A file name from user input: only letters, digits, space, `-`, `_` and brackets survive, so a name cannot escape its folder.
pub fn clean_name(name: &str) -> String {
    let clean: String = name.chars().filter(|c| c.is_alphanumeric() || " -_()".contains(*c)).collect();
    clean.trim().to_string()
}

/// Player save file for `name`.
fn save_path(name: &str) -> Option<std::path::PathBuf> {
    let clean = clean_name(name);
    (!clean.is_empty()).then(|| user_dir().map(|d| d.join("saves").join(format!("{clean}.json"))))?
}

pub fn write_save(s: &CampaignSave) -> Result<(), String> {
    let path = save_path(&s.name).ok_or("invalid save name")?;
    let temp = path.with_extension("tmp"); // write then rename: a crash never leaves a truncated save
    std::fs::create_dir_all(path.parent().unwrap())
        .and_then(|()| std::fs::write(&temp, serde_json::to_string_pretty(&s.to_json()).unwrap()))
        .and_then(|()| std::fs::rename(&temp, &path))
        .map_err(|e| e.to_string())
}

pub fn delete_save(name: &str) {
    if let Some(p) = save_path(name) {
        let _ = std::fs::remove_file(p);
    }
}

#[derive(Resource)]
pub struct Mission {
    pub id: String,
    pub data: Value,
    pub opening: Option<crate::briefing::Opening>,
    pub script: rules::Script,
}
/// Text widget a view spawns for the rules' current objectives (`rules::Ui` holds the other mission widgets).
#[derive(Component)]
pub struct ObjectiveText;

/// True when `map` names a scripted world (a campaign mission "mission01"... or a tutorial), as opposed to a skirmish map.
pub fn is_mission(map: Option<&str>) -> bool {
    map.is_some_and(|m| assets_dir().join(format!("maps/{m}/mission.json")).exists())
}

/// The mission after `id` ("mission02" -> "mission03"), if it is exported.
pub fn next(id: &str) -> Option<String> {
    let n: u32 = id.strip_prefix("mission")?.parse().ok()?;
    Some(format!("mission{:02}", n + 1)).filter(|m| is_mission(Some(m)))
}

#[cfg(test)]
pub fn load() -> Mission {
    load_id("mission01")
}

pub fn load_id(id: &str) -> Mission {
    let read = |name: &str| {
        std::fs::read_to_string(assets_dir().join(format!("maps/{id}/{name}.json"))).unwrap_or_else(|_| panic!("missing maps/{id}/{name}.json: run tools.export_campaign"))
    };
    let data: Value = serde_json::from_str(&read("mission")).expect("invalid mission JSON");
    let media: Value = serde_json::from_str(&read("media")).expect("invalid media JSON");
    let script = rules::Script::new(&data, media);
    // The scripted opening NIS (briefing.rs) only exists for mission 1; later missions start at once and run their own NIS rules.
    Mission { id: id.into(), data, opening: (id == "mission01").then(default), script }
}

/// `bases`: the map's scenery entities with their exported object records (id, type); those that a rule sets up become `MissionBase`s.
pub fn spawn(commands: &mut Commands, assets: &AssetServer, lib: &Library, fxl: &crate::fx::FxLib, id: &str, bases: &[(Entity, Value)]) {
    let mission = load_id(id);
    let setups =
        |kind: &'static str| mission.data["rules"].as_array().into_iter().flatten().flat_map(|r| r["actions"].as_array().into_iter().flatten()).filter(move |a| a["Type"] == kind);
    let islands: Vec<(i64, String, i32, crate::combat::IslandGunnery)> = setups("*State Init* Setup Island")
        .filter_map(|a| {
            let stance = a["AI Stance"].as_str().unwrap_or_default().to_lowercase();
            let gunnery = crate::combat::IslandGunnery {
                accuracy: crate::boarding::Skill::parse(a["Gunnery Level"].as_str().unwrap_or_default()).gunnery_vs_average(),
                silent: stance.contains("dummy") || stance.contains("neutral"),
            };
            Some((a["World Object ID"].as_i64()?, a["Player/Owner"].as_str()?.to_string(), a["Combat Strength"].as_i64().unwrap_or(0) as i32, gunnery))
        })
        .collect();
    for (e, o) in bases {
        // Wrecked hulls are scenery the rules address by their TWT group (mission 6 teleports them and turns their collision off).
        if o["type"].as_str().is_some_and(|t| t.ends_with("_Wrecked")) {
            commands.entity(*e).insert(MissionBase { id: o["id"].as_i64().unwrap_or(-1), owner: String::new() });
            continue;
        }
        let Some((_, owner, combat, gunnery)) = islands.iter().find(|(i, ..)| Some(*i) == o["id"].as_i64()) else {
            // Most volcanoes have no setup rule; they still fire (docs/re/volcanoes.md).
            if let Some(class) = o["type"].as_str().filter(|t| t.starts_with("Island_Volcano")).and_then(|t| lib.classes.iter().position(|c| c.name == t)) {
                commands.entity(*e).insert(MissionBase { id: o["id"].as_i64().unwrap_or(-1), owner: String::new() });
                let states = crate::combat::spawn_guns(commands, assets, lib, *e, class);
                commands.entity(*e).insert((crate::combat::ArmedBase { class }, crate::combat::Health::new(&lib.classes[class]), states));
            }
            continue;
        };
        commands.entity(*e).insert(MissionBase { id: o["id"].as_i64().unwrap_or(-1), owner: owner.clone() });
        // Bases are boarded only once a rule sets them Boardable (their 'Combat Strength' is what the boarders must wear down).
        commands.entity(*e).insert(crate::boarding::Boardable::base(o["type"].as_str().unwrap_or_default(), *combat, false));
        // Island gun towers: a `Base_*` class with mounts makes the base a shooter and a target (combat::base_fire).
        if let Some(class) = lib.classes.iter().position(|c| Some(c.name.as_str()) == o["type"].as_str()) {
            let states = crate::combat::spawn_guns(commands, assets, lib, *e, class);
            commands.entity(*e).insert((crate::combat::ArmedBase { class }, crate::combat::Health::new(&lib.classes[class]), states, *gunnery));
        }
    }
    let mut ships: Vec<Value> = mission.data["ships"].as_array().expect("mission ships").clone();
    // Wrecks and other hulls without a ship class are not simulated yet (docs/re/campaign.md).
    ships.retain(|s| lib.classes.iter().any(|c| Some(c.name.as_str()) == s["type"].as_str()));
    let player_group = mission.data["player_start"]["name"].clone();
    // The human's ship is the one the TWT flags primary; if none is (missions 9, 13) use the player group's first, else a stand-in cutter.
    let primary_id = [|s: &Value, g: &Value| s["primary"] == true && s["group"] == *g, |s: &Value, g: &Value| s["group"] == *g]
        .iter()
        .find_map(|p| ships.iter().find(|s| p(s, &player_group)))
        .map(|s| s["id"].as_i64().unwrap());
    if primary_id.is_none() {
        warn!("mission has no player ship; spawning a stand-in Ship_Navy_Cutter at the start point");
        let st = &mission.data["player_start"];
        ships.push(serde_json::json!({"id": -1, "type": crate::ship::DEFAULT_SHIP, "pos": st["start"], "rot": [st["forward"][1], -st["forward"][0].as_f64().unwrap_or(0.0), 0.0, st["forward"][0], st["forward"][1], 0.0, 0.0, 0.0, 1.0], "name": "RLS Stand-in", "group": player_group}));
    }
    let local_owner = player_group.as_str().unwrap_or_default().to_string();
    commands.queue(move |w: &mut World| {
        w.resource_mut::<crate::ship::Fleets>().local = local_owner;
    });
    for (slot, s) in ships.iter().enumerate() {
        let id = s["id"].as_i64().unwrap();
        let primary = Some(id) == primary_id || (primary_id.is_none() && id == -1);
        // Jim spawns at his placed position and drives the retail 'Jim Arriving @ Start of Mission' path (rule 1).
        let pos = game_to_bevy(&s["pos"]);
        let forward = crate::scenario::game_rotation(&s["rot"]) * Vec3::NEG_Z;
        let heading = (-forward.x).atan2(-forward.z);
        let owner = s["group"].as_str().unwrap_or("").to_string();
        let e = crate::spawn_ship(commands, assets, lib, fxl, s["type"].as_str().unwrap(), Side::Player, pos, heading, None);
        commands.entity(e).insert(crate::ship::FleetMember(owner.clone(), slot));
        commands.entity(e).insert(MissionShip {
            id,
            name: s["name"].as_str().unwrap_or_default().into(),
            owner,
            hold_fire: false,
            invuln: 0.0,
            visible: true,
            stance: rules::Stance::Neutral,
            target: None,
            attack: vec![],
            flag: String::new(),
            banner: String::new(),
            looping: None,
            stop_at_end: false,
            create_lifeboats: false,
            ..Default::default()
        });
        // A pre-placed lifeboat (mission 4) is a real one, found by the class flag; it is not part of any fleet roster.
        if lib.classes.iter().any(|c| c.is_lifeboat && Some(c.name.as_str()) == s["type"].as_str()) {
            commands.entity(e).insert(crate::lifeboats::Lifeboat { owner: s["group"].as_str().unwrap_or("").into() }).remove::<crate::ship::FleetMember>();
        }
        // Setup Ship: 'Crew Skill Level' and 'Boardable' (ships without one cannot be boarded).
        let setup = setups("*State Init* Setup Ship").find(|a| a["World Object ID"].as_i64() == Some(id));
        let skill = crate::boarding::Skill::parse(setup.and_then(|a| a["Crew Skill Level"].as_str()).unwrap_or_default());
        let boardable = setup.is_some_and(|a| crate::data::wot_flag(&a["Boardable"]));
        commands.entity(e).insert(crate::boarding::Boardable::ship(s["type"].as_str().unwrap_or_default(), skill, boardable));
        let f = skill.handling();
        commands.entity(e).entry::<crate::Ship>().and_modify(move |mut ship| (ship.crew_speed, ship.crew_turn) = (f, f));
        commands.entity(e).insert(crate::environment::ScanFactor(skill.scan()));
        if !primary {
            commands.entity(e).remove::<PlayerShip>();
        }
    }
    crate::environment::spawn(commands, assets, &mission.data, false);
    commands.insert_resource(mission);
}

/// Position of the rules' current objective point (None for 'NO OBJECTIVE POINT').
pub fn objective_point(m: &Mission) -> Option<Vec3> {
    m.script.point_pos.or_else(|| m.script.point.map(|i| game_to_bevy(&m.data["points"][i]["Position"])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn victory_points_accumulate_across_missions_without_double_counting() {
        let mut points = CampaignPoints::default();
        points.record_score("mission01", 2);
        points.record_score("mission01", 2);
        points.record_score("mission01", 5);
        points.record_score("mission02", 3);
        assert_eq!(points.total, 8);
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn checkpoint_preserves_outfitting_without_refunding_current_mission_spending() {
        let mut p = CampaignPoints { total: 4, mission: "mission03".into(), last_score: 10, ..default() };
        p.outfit.hired.push("Crew_Navy_BenH".into());
        p.outfit.ships.entry("test".into()).or_default().crew[0].push("Crew_Navy_BenH".into());
        let save = p.snapshot("outfit", "mission03");
        assert_eq!((save.points, save.replay_debt), (0, 6));
        assert_eq!(CampaignSave::from_json(&save.to_json()), Some(save.clone()));
        let mut replay = CampaignPoints { total: save.points, replay_debt: save.replay_debt, outfit: save.outfit.clone(), ..default() };
        replay.record_score("mission03", 2);
        let partial = replay.snapshot("partial", "mission03");
        assert_eq!((partial.points, partial.replay_debt), (0, 6));
        assert_eq!(partial.outfit, save.outfit);
        replay.record_score("mission03", 10);
        assert_eq!((replay.total, replay.replay_debt), (4, 0));
        let legacy = serde_json::json!({"name": "old", "mission": "mission03", "points": 29});
        let legacy = CampaignSave::from_json(&legacy).unwrap();
        assert_eq!(legacy.outfit, crate::crew_arms::Outfit::default());
        assert_eq!(legacy.replay_debt, 0);
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn shipped_saves_cover_every_mission_and_snapshot_excludes_this_missions_points() {
        let saves = list_saves();
        let shipped: Vec<_> = saves.iter().filter(|s| s.1).map(|s| s.0.mission.as_str()).collect();
        assert_eq!(shipped.len(), 13);
        assert!(shipped.contains(&"mission13"));
        let m5 = saves.iter().find(|s| s.0.mission == "mission05").unwrap();
        assert_eq!((m5.0.points, m5.0.crew.len(), m5.0.arms.len()), (94, 21, 12));
        let p = CampaignPoints { total: 94 + 7, crew: m5.0.crew.clone(), mission: "mission05".into(), last_score: 7, ..default() };
        let snap = p.snapshot("x", "mission05");
        assert_eq!((snap.points, snap.crew.len()), (94, 21), "mission 5's 7 points are not part of its start");
        assert_eq!(CampaignSave::from_json(&snap.to_json()), Some(snap));
        assert_eq!(save_path("../a/b"), user_dir().map(|d| d.join("saves/ab.json")));
        assert_eq!(save_path("//"), None);
    }

    #[test]
    fn user_dir_prefers_appdata_on_windows_but_keeps_an_existing_home_folder() {
        let p = |s: &str| Some(std::path::PathBuf::from(s));
        let (home, appdata) = (p("/h"), p("/a"));
        let old = std::path::PathBuf::from("/h/.local/share/openprocyon");
        let new = std::path::PathBuf::from("/a/openprocyon");
        let none = |_: &std::path::Path| false;
        assert_eq!(user_dir_from(p("/x"), home.clone(), appdata.clone(), true, none), p("/x/openprocyon"));
        assert_eq!(user_dir_from(None, home.clone(), appdata.clone(), true, none), Some(new.clone()));
        assert_eq!(user_dir_from(None, home.clone(), appdata.clone(), true, |d| d == old), Some(old.clone()));
        assert_eq!(user_dir_from(None, home.clone(), appdata.clone(), true, |_| true), Some(new), "both exist: APPDATA");
        assert_eq!(user_dir_from(None, home.clone(), None, true, none), Some(old.clone()));
        assert_eq!(user_dir_from(None, home, appdata, false, none), Some(old), "Linux/macOS: HOME first as before");
        assert_eq!(user_dir_from(None, None, None, true, none), None);
    }

    #[test]
    fn campaign_points_round_trip_to_user_save() {
        let dir = std::env::temp_dir().join(format!("procyon-campaign-test-{}", std::process::id()));
        let path = dir.join("campaign-points.json");
        let points = CampaignPoints { total: 27, ..Default::default() };
        points.save_to(path.clone()).unwrap();
        assert_eq!(CampaignPoints::load_from(path).total, 27);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
