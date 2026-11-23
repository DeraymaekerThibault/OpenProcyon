//! Mission 1 setup: retail ship placements and the HUD text nodes. All behaviour comes from the data-driven rule interpreter in
//! rules.rs (World/SinglePlayer/Mission01.twt); this file only spawns the units and holds their mission state.
use crate::{PlayerShip, Side, assets_dir, combat::Library, game_to_bevy};
use bevy::prelude::*;
use serde_json::Value;
use std::collections::HashMap;

/// Mission state of one unit. `owner` is the rules' player name (changes with 'Set Group/Unit Owner'); `id` its TWT world object id.
#[derive(Component, Default)]
pub struct MissionShip {
    pub id: i64,
    pub name: String,
    pub owner: crate::names::Owner,
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
    /// The course it was on when a tow took it; it sails on from there once the tow ends (`rules::systems` AI).
    pub towed_course: Option<Vec<Vec3>>,
    /// Not retail: the standing order of the mission rule `OP Follow Group` (`rules::systems` mission AI keeps the station).
    pub follow: Option<rules::FollowOrder>,
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
        rules::Unit { owner: self.owner.as_str().into(), damage, ..rules::Unit::new(e, self.id, tf.translation) }
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
/// True when `map` names a scripted world (a campaign mission "mission01"... or a tutorial), as opposed to a skirmish map.
pub fn is_mission(map: Option<&str>) -> bool {
    map.is_some_and(|m| assets_dir().join(format!("maps/{m}/mission.json")).exists())
}

/// The retail campaign's missions "mission01".."mission13": the only ones whose victory points count toward the campaign total.
pub fn is_campaign(id: &str) -> bool {
    id.strip_prefix("mission").is_some_and(|n| n.len() == 2 && n.parse::<u32>().is_ok())
}

/// The mission after `id`, if it is exported: the next retail mission ("mission02" -> "mission03"), or the next custom mission of
/// the same mod campaign (`custom_missions`).
pub fn next(id: &str) -> Option<String> {
    if is_campaign(id) {
        let n: u32 = id["mission".len()..].parse().ok()?;
        return Some(format!("mission{:02}", n + 1)).filter(|m| is_mission(Some(m)));
    }
    next_custom(custom_missions(), id)
}

/// Not retail: the campaign counters (`OP Set Campaign Counter`) each custom mission handed on when it was won, kept in memory for the
/// session (not saved). A mission starts with what the mission before it in the same mod campaign handed on; a mission that was lost or
/// restarted hands on nothing, so a replay starts from the same values as the first try.
#[derive(Resource, Default)]
pub struct CampaignValues {
    handed_on: HashMap<String, HashMap<String, f32>>,
}
impl CampaignValues {
    /// What a mission starts with: the values the previous mission of its campaign handed on (none for the first mission, a retail
    /// mission, or when that one was never won in this session).
    pub fn incoming(&self, id: &str) -> HashMap<String, f32> {
        self.incoming_in(custom_missions(), id)
    }
    fn incoming_in(&self, list: &[CustomMission], id: &str) -> HashMap<String, f32> {
        let Some(i) = list.iter().position(|m| m.id == id).filter(|i| *i > 0) else { return HashMap::new() };
        let before = &list[i - 1];
        if before.campaign != list[i].campaign {
            return HashMap::new();
        }
        self.handed_on.get(&before.id).cloned().unwrap_or_default()
    }
    /// A won mission hands its values on (replacing what it handed on before).
    pub fn commit(&mut self, id: &str, values: HashMap<String, f32>) {
        self.handed_on.insert(id.into(), values);
    }
}

/// Not retail: a mission a mod adds. Its map id, the map's name and description, and the campaign it belongs to (its mod).
#[derive(Clone, Debug, PartialEq)]
pub struct CustomMission {
    pub id: String,
    pub name: String,
    pub description: String,
    /// The mod's `name` (folder name when it has none): each mod is one campaign.
    pub campaign: String,
}

/// Not retail: `id` is a mission one of this launch's mods adds (`custom_missions`): the places that read keys only custom missions have
/// (a map's `mines`) ask this, so retail missions and the tutorials are never affected.
pub fn is_custom(id: &str) -> bool {
    custom_missions().iter().any(|m| m.id == id)
}

/// The custom missions of this launch's mods (`modding::active`), read once.
pub fn custom_missions() -> &'static [CustomMission] {
    static LIST: std::sync::OnceLock<Vec<CustomMission>> = std::sync::OnceLock::new();
    LIST.get_or_init(|| custom_missions_in(crate::modding::active(), &assets_dir().join("maps")))
}

/// Every `maps/<id>/mission.json` in `mods` (in load order), grouped by mod. A mod's missions play in the order of its `mod.json`
/// `"missions": [ids]` (the rest after, by id). Retail ids (`missionNN`, the tutorials) are left out: a mod's copy replaces the
/// retail mission in the retail campaign. When two mods add the same id the later one wins, as in the overlay. Names and
/// descriptions come from the overlaid `maps` root (`map.json` `name`, `description`).
pub fn custom_missions_in(mods: &[std::path::PathBuf], maps: &std::path::Path) -> Vec<CustomMission> {
    let mut out: Vec<CustomMission> = vec![];
    for dir in mods {
        let meta = crate::data::try_json(&dir.join("mod.json")).unwrap_or_default();
        let folder = dir.file_name().map_or(String::new(), |f| f.to_string_lossy().into_owned());
        let campaign = meta["name"].as_str().filter(|s| !s.is_empty()).map_or(folder, String::from);
        let order = string_list(&meta["missions"]);
        let mut ids: Vec<String> = std::fs::read_dir(dir.join("maps"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().join("mission.json").is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|id| !is_campaign(id) && !matches!(id.as_str(), "tutorial" | "basic_tutorial"))
            .collect();
        ids.sort_by_key(|id| (order.iter().position(|o| o == id).unwrap_or(usize::MAX), id.clone()));
        for id in ids {
            out.retain(|m| m.id != id);
            let map = crate::data::try_json(&maps.join(&id).join("map.json")).unwrap_or_default();
            let text = |k: &str| map[k].as_str().unwrap_or_default().to_string();
            let name = Some(text("name")).filter(|s| !s.is_empty()).unwrap_or_else(|| id.clone());
            out.push(CustomMission { id, name, description: text("description"), campaign: campaign.clone() });
        }
    }
    out
}

/// The custom mission after `id` in the same campaign, if any.
fn next_custom(list: &[CustomMission], id: &str) -> Option<String> {
    let i = list.iter().position(|m| m.id == id)?;
    list.get(i + 1).filter(|n| n.campaign == list[i].campaign).map(|n| n.id.clone())
}

#[cfg(test)]
pub fn load() -> Mission {
    load_id("mission01")
}

/// Not retail: debug launch options (`--fire-rule`, `--start-at`), set once by `run` and applied to every mission loaded.
static START: std::sync::OnceLock<(Vec<String>, Option<(f32, f32)>)> = std::sync::OnceLock::new();
pub fn set_start(rules: Vec<String>, at: Option<(f32, f32)>) {
    let _ = START.set((rules, at));
}

pub fn load_id(id: &str) -> Mission {
    let read = |name: &str| {
        std::fs::read_to_string(assets_dir().join(format!("maps/{id}/{name}.json"))).unwrap_or_else(|_| panic!("missing maps/{id}/{name}.json: run tools.export_campaign"))
    };
    let data: Value = serde_json::from_str(&read("mission")).expect("invalid mission JSON");
    let media: Value = serde_json::from_str(&read("media")).expect("invalid media JSON");
    let mut script = rules::Script::new(&data, media);
    // The scripted opening NIS (briefing.rs) only exists for mission 1; later missions start at once and run their own NIS rules.
    script.briefing_opening = id == "mission01";
    if let Some((rules, at)) = START.get() {
        (script.start_rules, script.start_at) = (rules.clone(), *at);
    }
    Mission { id: id.into(), data, opening: script.briefing_opening.then(default), script }
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
                // Green/Average/Elite scale the spread by 0.7/0.4/0.1; the "CREW SKILL LEVEL" placeholder leaves it at 1.
                accuracy: match a["Gunnery Level"].as_str().unwrap_or_default() {
                    "Green" => 0.7,
                    "Average" => 0.4,
                    "Elite" => 0.1,
                    _ => 1.0,
                },
                silent: stance.contains("dummy"),
                neutral: stance.contains("neutral"),
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
        let setup = islands.iter().find(|(i, ..)| Some(*i) == o["id"].as_i64()).map(|(_, o, c, g)| (o.clone(), *c, *g));
        // A base no rule sets up (mission 5's Grant and pirate base, 7's Regina, 8's Mining Camp) is still a rules unit with its class's guns
        // and hit points: the rules address it by its TWT group for docking, owner and dockable changes, and mission 5's pirate base must be
        // destroyed. With no owner until a rule gives one it shoots nobody. PROVISIONAL: default gunnery (no skill level).
        let unset_base = || (String::new(), 0, crate::combat::IslandGunnery { accuracy: 1.0, silent: false, neutral: false });
        let Some((owner, combat, gunnery)) = setup.or_else(|| o["type"].as_str().is_some_and(|t| t.starts_with("Base_")).then(unset_base)) else {
            // Most volcanoes have no setup rule; they still fire (docs/re/volcanoes.md).
            if let Some(class) = o["type"].as_str().filter(|t| t.starts_with("Island_Volcano")).and_then(|t| lib.classes.iter().position(|c| c.name == t)) {
                commands.entity(*e).insert(MissionBase { id: o["id"].as_i64().unwrap_or(-1), owner: String::new() });
                let states = crate::combat::spawn_guns(commands, assets, lib, *e, class);
                commands.entity(*e).insert((
                    crate::combat::ArmedBase { class },
                    crate::combat::DamageFx::new(&lib.classes[class]),
                    crate::combat::Health::new(&lib.classes[class]),
                    states,
                    crate::combat::IslandGunnery { accuracy: 0.0, silent: false, neutral: false },
                ));
            }
            continue;
        };
        commands.entity(*e).insert(MissionBase { id: o["id"].as_i64().unwrap_or(-1), owner });
        // Bases are boarded only once a rule sets them Boardable (their 'Combat Strength' is what the boarders must wear down).
        commands.entity(*e).insert(crate::boarding::Boardable::base(o["type"].as_str().unwrap_or_default(), combat, false));
        // Island gun towers: a `Base_*` class with mounts makes the base a shooter and a target (combat::base_fire).
        if let Some(class) = lib.classes.iter().position(|c| Some(c.name.as_str()) == o["type"].as_str()) {
            let states = crate::combat::spawn_guns(commands, assets, lib, *e, class);
            commands.entity(*e).insert((
                crate::combat::ArmedBase { class },
                crate::combat::Health::new(&lib.classes[class]),
                states,
                crate::combat::DamageFx::new(&lib.classes[class]),
                gunnery,
            ));
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
    let local_owner: crate::names::Owner = player_group.as_str().unwrap_or_default().into();
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
        let owner: crate::names::Owner = s["group"].as_str().unwrap_or("").into();
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
    // Not retail: the campaign counters the mission before this one in the mod's campaign handed on.
    let id = mission.id.clone();
    commands.insert_resource(mission);
    commands.queue(move |w: &mut World| {
        let values = w.get_resource::<CampaignValues>().map(|c| c.incoming(&id)).unwrap_or_default();
        if let Some(mut m) = w.get_resource_mut::<Mission>() {
            m.script.set_campaign_values(values);
        }
    });
}

/// Position of the rules' current objective point (None for 'NO OBJECTIVE POINT').
pub fn objective_point(m: &Mission) -> Option<Vec3> {
    m.script.point_pos.or_else(|| m.script.point.map(|i| game_to_bevy(&m.data["points"][i]["Position"])))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mission 5's Waystation Grant has no 'Setup Island' rule. It must still be a rules unit, or docking there never satisfies the
    /// opening 'Group/Unit Docked' condition and the mission cannot advance. Its pirate base has none either.
    #[test]
    #[ignore = "needs imported assets"]
    fn a_base_without_a_setup_rule_is_a_dock_target_of_the_rules() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<bevy::world_serialization::WorldAsset>()
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<StandardMaterial>();
        app.insert_resource(Library::load(assets_dir())).init_resource::<crate::fx::FxLib>().init_resource::<crate::ship::Fleets>();
        let map: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("maps/mission05/map.json")).unwrap()).unwrap();
        let objects: Vec<Value> = map["objects"].as_array().unwrap().iter().filter(|o| o["type"] == "Base_Grant" || o["type"] == "Base_Pirate").cloned().collect();
        let bases: Vec<(Entity, Value)> = objects.into_iter().map(|o| (app.world_mut().spawn(Transform::default()).id(), o)).collect();
        let grant = bases.iter().find(|(_, o)| o["type"] == "Base_Grant").unwrap().0;
        let pirate = bases.iter().find(|(_, o)| o["type"] == "Base_Pirate").unwrap().0;
        app.world_mut()
            .run_system_once(move |mut commands: Commands, assets: Res<AssetServer>, lib: Res<Library>, fxl: Res<crate::fx::FxLib>| {
                spawn(&mut commands, &assets, &lib, &fxl, "mission05", &bases)
            })
            .unwrap();
        let base = app.world().get::<MissionBase>(grant).expect("Base_Grant is a MissionBase");
        let mission = load_id("mission05");
        let mut player = base.unit(Entity::PLACEHOLDER, &Transform::default(), 0.0);
        (player.primary, player.owner, player.id) = (true, mission.script.player().into(), -2);
        let mut w = vec![player, base.unit(grant, &Transform::default(), 0.0)];
        mission.script.tag(&mut w);
        assert!(mission.script.dock_targets(&w).contains(&grant), "the opening dock rule waits for Grant");
        // The pirate base has no setup rule either, and the mission is won by destroying its mission section: it needs its guns and hit points.
        assert!(app.world().get::<crate::combat::ArmedBase>(pirate).is_some() && app.world().get::<crate::combat::Health>(pirate).is_some());
        assert_eq!(app.world().get::<MissionBase>(pirate).unwrap().owner, "", "no owner until 'Set Player Owners & AI'");
    }

    /// Mission 11's pirate asteroids are island objects with guns and hit points (read from the original's data), owned by their own player.
    #[test]
    #[ignore = "needs imported assets"]
    fn mission_11_armed_asteroids_become_armed_bases_owned_by_the_armed_asteroids_player() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<bevy::world_serialization::WorldAsset>()
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<StandardMaterial>();
        app.insert_resource(Library::load(assets_dir())).init_resource::<crate::fx::FxLib>().init_resource::<crate::ship::Fleets>();
        let map: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("maps/mission11/map.json")).unwrap()).unwrap();
        let objects: Vec<Value> = map["objects"].as_array().unwrap().iter().filter(|o| o["type"].as_str().is_some_and(|t| t.ends_with("_Armed"))).cloned().collect();
        assert_eq!(objects.len(), 32);
        let bases: Vec<(Entity, Value)> = objects.into_iter().map(|o| (app.world_mut().spawn(Transform::default()).id(), o)).collect();
        let ids: Vec<Entity> = bases.iter().map(|b| b.0).collect();
        app.world_mut()
            .run_system_once(move |mut commands: Commands, assets: Res<AssetServer>, lib: Res<Library>, fxl: Res<crate::fx::FxLib>| {
                spawn(&mut commands, &assets, &lib, &fxl, "mission11", &bases)
            })
            .unwrap();
        for e in ids {
            let base = app.world().get::<MissionBase>(e).expect("an armed asteroid is a MissionBase");
            assert_eq!(base.owner, "Armed Asteroids");
            assert!(app.world().get::<crate::combat::ArmedBase>(e).is_some() && app.world().get::<crate::combat::Health>(e).is_some());
        }
    }

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

    #[test]
    fn custom_missions_are_listed_per_mod_in_campaign_order_and_chain_within_their_mod() {
        let dir = std::env::temp_dir().join(format!("procyon-custom-missions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let write = |rel: &str, text: &str| {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        // Saga: a two-mission campaign in mod.json order, plus a retail override and a map with no mission (both left out).
        write("saga/mod.json", r#"{"name": "Saga", "missions": ["b_second", "a_first"]}"#);
        for id in ["a_first", "b_second", "mission03", "tutorial"] {
            write(&format!("saga/maps/{id}/mission.json"), "{}");
        }
        write("saga/maps/skirmish_only/map.json", "{}");
        write("merged/b_second/map.json", r#"{"name": "The Second", "description": "Two."}"#);
        // Lone: no name, one mission.
        write("lone/maps/solo/mission.json", "{}");
        let list = custom_missions_in(&[dir.join("saga"), dir.join("lone")], &dir.join("merged"));
        let rows: Vec<_> = list.iter().map(|m| (m.id.as_str(), m.name.as_str(), m.campaign.as_str())).collect();
        assert_eq!(rows, [("b_second", "The Second", "Saga"), ("a_first", "a_first", "Saga"), ("solo", "solo", "lone")]);
        assert_eq!(list[0].description, "Two.");
        assert_eq!(next_custom(&list, "b_second").as_deref(), Some("a_first"));
        assert_eq!(next_custom(&list, "a_first"), None, "the campaign ends at its mod's last mission");
        assert_eq!(next_custom(&list, "solo"), None);
        assert!(is_campaign("mission03") && !is_campaign("mission_x") && !is_campaign("mission"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Not retail: what a won custom mission hands to the next one of its mod campaign (`OP Set Campaign Counter`).
    #[test]
    fn campaign_values_pass_from_a_won_mission_to_the_next_one_of_the_same_campaign_only() {
        let m = |id: &str, campaign: &str| CustomMission { id: id.into(), name: id.into(), description: String::new(), campaign: campaign.into() };
        let list = [m("one", "Saga"), m("two", "Saga"), m("three", "Saga"), m("other", "Elsewhere")];
        let mut values = CampaignValues::default();
        assert!(values.incoming_in(&list, "one").is_empty(), "the first mission starts with nothing");
        assert!(values.incoming_in(&list, "two").is_empty(), "the first one was not won yet");
        values.commit("one", HashMap::from([("rescued".to_string(), 3.0)]));
        assert_eq!(values.incoming_in(&list, "two").get("rescued"), Some(&3.0), "the next mission starts with what the won one handed on");
        assert_eq!(values.incoming_in(&list, "two").get("rescued"), Some(&3.0), "a replay starts from the same values");
        assert!(values.incoming_in(&list, "three").is_empty(), "only the mission before it hands values on");
        assert!(values.incoming_in(&list, "other").is_empty(), "another campaign does not see them, even when listed next");
        assert!(values.incoming_in(&list, "not listed").is_empty());
        values.commit("one", HashMap::from([("rescued".to_string(), 5.0)]));
        assert_eq!(values.incoming_in(&list, "two").get("rescued"), Some(&5.0), "a mission won again hands on its new values");
    }
}
