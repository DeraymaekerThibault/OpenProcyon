//! `--check-mod <dir>`: everything that can be said about a mod folder without starting a game, as a list of [`Finding`]s.
//!
//! The mod is laid over the imported assets (and the mods it `requires`, when installed) in a temporary overlay, exactly as the game
//! does, and then looked at from three sides: what the loader refuses (skipped files, `mod.json`), what the author wrote that nothing
//! reads (unknown keys, from the same schemas the editor gets, `authoring::schemas`), and what the files name that is not there
//! (layers, hardpoints, guns, bullets, effects, sounds, scenery models). Missions go through the mission checker (`--check-mission`)
//! with a few extra warnings. Without imported assets only what the mod's own files can show is checked.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

use crate::data::derive;
use crate::modding::{self, Installed};
use crate::rules::check::{Finding, JsonPath, Level, did_you_mean, missing_scenery, nearest, outside_world, unused_definitions};

/// Checks the mod in `dir` against the imported assets in `base` (`None`: nothing imported, so only the mod's own files are looked
/// at and one warning says what was skipped). The mods `dir` lists in `requires` are laid under it when they are installed. Writes
/// nothing but a scratch folder under the system's temp folder, which it removes; starts no window. Findings come in the order the
/// checks run: `mod.json`, files the loader skipped, keys, ships, weapons, missions.
pub fn check_mod(dir: &Path, base: Option<&Path>) -> Vec<Finding> {
    check_mod_with(dir, base, &modding::installed(crate::bundled_mods_dir().as_deref()))
}

/// `check_mod` with the installed mods given (the `requires` of the mod are looked up among them).
pub fn check_mod_with(dir: &Path, base: Option<&Path>, installed: &[Installed]) -> Vec<Finding> {
    // A folder with no `ships/` is not an import (an empty user folder): treat it as no assets.
    let base = base.filter(|b| b.join("ships").is_dir());
    let mut c = Checker { dir, root: PathBuf::new(), base: base.map(Path::to_path_buf), have_base: base.is_some(), out: vec![], unchecked: 0 };
    if !dir.is_dir() {
        c.error("", "", format!("{} is not a folder", dir.display()));
        return c.out;
    }
    let meta = std::fs::read(dir.join("mod.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let deps = c.mod_json(meta.as_ref(), installed);
    // The overlay: the base (or an empty folder), the required mods, the mod.
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let scratch = std::env::temp_dir().join(format!("procyon-check-mod-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&scratch);
    let (empty, root) = (scratch.join("base"), scratch.join("overlay"));
    if std::fs::create_dir_all(&empty).is_err() {
        c.error("", "", format!("cannot create a scratch folder in {}", std::env::temp_dir().display()));
        return c.out;
    }
    let layers: Vec<PathBuf> = deps.iter().cloned().chain([dir.to_path_buf()]).collect();
    match modding::overlay_into(base.unwrap_or(&empty), &layers, &root) {
        Ok(report) => {
            c.root = root;
            if let Some(entry) = report.mods.last() {
                for (file, why) in &entry.skipped {
                    // Without the game's files a mod that extends a retail file cannot be merged: not the mod's fault.
                    if !c.have_base && why.starts_with("extends unknown '") {
                        c.unchecked += 1;
                    } else {
                        let why = c.tidy(why);
                        let ptr = merge_pointer(&why);
                        c.error(file, ptr, why);
                    }
                }
                for (file, why) in &entry.warnings {
                    let why = c.tidy(why);
                    let ptr = merge_pointer(&why);
                    c.warn(file, ptr, why);
                }
                for file in &entry.unused_textures {
                    c.warn(file, "", "no model uses this texture override; check the name".into());
                }
            }
            c.run();
        }
        Err(e) => c.error("", "", e),
    }
    let _ = std::fs::remove_dir_all(&scratch);
    if !c.have_base {
        let files = if c.unchecked > 0 { format!(", and {} file(s) that extend a retail file", c.unchecked) } else { String::new() };
        c.out.push(Finding::in_file(
            Level::Warning,
            "",
            "",
            format!("no imported assets found: ships, models, weapons, effects and sounds the mod does not bring itself were not looked up{files}; import the game (make assets) for the full check"),
        ));
    }
    c.out
}

/// The place a keyed-merge message names (`merges '/objects': ...`, docs/modding.md "Changing one entry of a list"), else the
/// whole file. The message holds a JSON pointer; its `~1` / `~0` escapes are undone so the finding's `pointer` is that pointer.
fn merge_pointer(why: &str) -> JsonPath {
    let ptr = why.strip_prefix("merges '").and_then(|r| r.split_once("':")).map_or("", |(ptr, _)| ptr);
    ptr.split('/').skip(1).fold(JsonPath::default(), |path, seg| path.key(seg.replace("~1", "/").replace("~0", "~")))
}

struct Checker<'a> {
    dir: &'a Path,
    /// The overlay folder (base + mods); empty until built.
    root: PathBuf,
    /// The imported assets, when there are any.
    base: Option<PathBuf>,
    have_base: bool,
    out: Vec<Finding>,
    /// Files that could not be merged without the game's own files.
    unchecked: usize,
}

impl Checker<'_> {
    fn error(&mut self, file: &str, path: impl Into<JsonPath>, message: String) {
        self.out.push(Finding::in_file(Level::Error, file, path, message));
    }
    fn warn(&mut self, file: &str, path: impl Into<JsonPath>, message: String) {
        self.out.push(Finding::in_file(Level::Warning, file, path, message));
    }
    /// A finding with a "did you mean" tail and the suggestion as data.
    fn with_hint(&mut self, level: Level, file: &str, path: impl Into<JsonPath>, message: String, suggestion: Option<&str>) {
        let mut f = Finding::in_file(level, file, path, format!("{message}{}", did_you_mean(suggestion)));
        f.suggestion = suggestion.map(String::from);
        self.out.push(f);
    }

    /// `mod.json`: value types, `game_version` against this engine, `format`, `requires` and `missions`. Returns the folders of the
    /// installed mods `requires` names (what they require first), to be laid under this one.
    fn mod_json(&mut self, meta: Option<&Value>, installed: &[Installed]) -> Vec<PathBuf> {
        let file = "mod.json";
        if !self.dir.join(file).is_file() {
            self.warn(file, "", "no mod.json: the mod's id is its folder name and the Mods screen has no name or description for it".into());
            return vec![];
        }
        // A file that does not parse is reported with the files the overlay skipped.
        let Some(meta) = meta.and_then(Value::as_object) else { return vec![] };
        for key in ["id", "name", "version", "description", "category", "game_version"] {
            if meta.get(key).is_some_and(|v| !v.is_string()) {
                self.error(file, key, format!("{} is not a string; the game ignores it", meta[key]));
            }
        }
        if meta.get("default").is_some_and(|v| !v.is_boolean()) {
            self.error(file, "default", format!("{} is not true or false; the game ignores it", meta["default"]));
        }
        match meta.get("format") {
            Some(v) if v.as_u64().is_none_or(|n| n > u64::from(u32::MAX)) => self.error(file, "format", format!("{v} is not a whole number; the game ignores it")),
            Some(v) if v.as_u64().is_some_and(|n| n > u64::from(modding::MOD_FORMAT)) => {
                self.warn(file, "format", format!("mod format {v}; this engine reads format {}, so some of it may be ignored", modding::MOD_FORMAT));
            }
            _ => {}
        }
        if let Some(text) = meta.get("game_version").and_then(Value::as_str) {
            match (modding::parse_version(text), modding::parse_version(crate::authoring::ENGINE_VERSION)) {
                (None, _) => self.error(file, "game_version", format!("{text:?} is not a version like \"0.2.0\"; the game ignores it")),
                (Some(want), Some(have)) if want > have => {
                    self.warn(file, "game_version", format!("the mod asks for OpenProcyon {text} or newer; this is {}, so it may not work", crate::authoring::ENGINE_VERSION));
                }
                _ => {}
            }
        }
        let list = |key: &str| -> Option<Vec<String>> {
            let items = meta.get(key)?.as_array()?;
            let strings: Vec<String> = items.iter().filter_map(|x| x.as_str().map(String::from)).collect();
            (strings.len() == items.len()).then_some(strings)
        };
        for key in ["missions", "requires"] {
            if meta.get(key).is_some() && list(key).is_none() {
                self.error(file, key, format!("{} is not a list of strings; the game ignores it", meta[key]));
            }
        }
        for id in list("missions").unwrap_or_default() {
            if !self.dir.join("maps").join(&id).join("mission.json").is_file() {
                self.warn(file, "missions", format!("'{id}' has no maps/{id}/mission.json in this mod"));
            }
        }
        // `requires`, and what those require, laid under this mod when installed (dependencies first).
        let own_id = meta.get("id").and_then(Value::as_str).map(String::from).or_else(|| self.dir.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
        let mut deps: Vec<PathBuf> = vec![];
        let mut seen: HashSet<String> = HashSet::from([own_id]);
        let mut wanted: Vec<String> = list("requires").unwrap_or_default();
        wanted.reverse();
        let mut order: Vec<&Installed> = vec![];
        while let Some(id) = wanted.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            match installed.iter().find(|m| m.id == id) {
                Some(m) => {
                    order.push(m);
                    wanted.extend(m.requires.iter().rev().cloned());
                }
                None => self.warn(file, "requires", format!("requires mod {id:?}, which is not installed; its files are not part of this check")),
            }
        }
        // Whatever was reached last is the deepest dependency: lay those first.
        deps.extend(order.iter().rev().map(|m| m.dir.clone()).filter(|d| std::fs::canonicalize(d).ok() != std::fs::canonicalize(self.dir).ok()));
        deps
    }

    fn run(&mut self) {
        let mut files = vec![];
        modding::walk(self.dir, Path::new(""), &mut files);
        files.sort();
        let rel = |p: &PathBuf| p.to_string_lossy().replace('\\', "/");
        let schemas = crate::authoring::schemas();
        for f in &files {
            let name = rel(f);
            let Some(schema) = schema_of(&name).and_then(|s| schemas.iter().find(|(n, _)| *n == s)).map(|(_, s)| s) else { continue };
            let Some(mut doc) = crate::data::try_json(&self.dir.join(f)).filter(Value::is_object) else { continue };
            if !name.starts_with("ships/")
                && let Some(o) = doc.as_object_mut()
            {
                // Merge keywords of any file extended over a sibling.
                o.remove("extends");
                o.remove("set");
                if o.get("merge").is_some_and(Value::is_object) {
                    o.remove("merge");
                }
            }
            self.out.extend(unknown_key_findings(schema, &doc, &name));
        }
        if self.have_base {
            self.references(&files);
        }
        let reported = self.weapons();
        self.ships(&files, reported);
        self.missions();
        self.skirmish_maps();
    }

    /// Every skirmish map the mod brings (`map.json` `"skirmish": true`), against what the lobby needs to start it.
    fn skirmish_maps(&mut self) {
        let maps = self.dir.join("maps");
        let mut ids: Vec<String> =
            std::fs::read_dir(&maps).into_iter().flatten().flatten().filter(|e| e.path().is_dir()).filter_map(|e| e.file_name().into_string().ok()).collect();
        ids.sort();
        for id in ids {
            let Some(map) = crate::data::try_json(&self.root.join("maps").join(&id).join("map.json")) else { continue };
            let file = format!("maps/{id}/map.json");
            for (path, message) in skirmish_team_problems(&map) {
                self.error(&file, path, message);
            }
        }
    }

    /// Gun, bullet, effect and sound names the mod's own files use, against the merged tables and folders. Needs the base.
    fn references(&mut self, files: &[PathBuf]) {
        let sounds = sound_stems(&self.root);
        let fx_exists = |n: &str| self.root.join(format!("fx/{n}.json")).is_file();
        let mut findings = vec![];
        let mut fx = |file: &str, path: JsonPath, name: Option<&str>, is_sound: bool| {
            let Some(name) = name.filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case("none")) else { return };
            let found = if is_sound { sounds.contains(&name.to_lowercase()) } else { fx_exists(name) };
            if !found {
                let what = if is_sound { "sound (no sound/**/<name>.ogg)" } else { "effect (no fx/<name>.json)" };
                findings.push(Finding::in_file(Level::Warning, file, path, format!("'{name}' is not a {what} in the game or the mods")));
            }
        };
        for f in files {
            let name = f.to_string_lossy().replace('\\', "/");
            let Some(doc) = crate::data::try_json(&self.dir.join(f)).filter(Value::is_object) else { continue };
            let s = |v: &Value| v.as_str().map(String::from);
            if schema_of(&name) == Some("ship.schema.json") {
                for k in ["explosion_fx", "repair_fx"] {
                    fx(&name, JsonPath::from(k), s(&doc[k]).as_deref(), false);
                }
                for (i, n) in doc["engine_snd"].as_array().into_iter().flatten().enumerate() {
                    fx(&name, JsonPath::from("engine_snd").index(i), n.as_str(), true);
                }
                for key in ["engines", "wakes"] {
                    for (i, e) in doc[key].as_array().into_iter().flatten().enumerate() {
                        fx(&name, JsonPath::from(key).index(i).key("fx"), e["fx"].as_str(), false);
                    }
                }
                for (label, sec) in doc["sections"].as_object().into_iter().flatten() {
                    fx(&name, JsonPath::from("sections").key(label.as_str()).key("destruct").key("fx"), sec["destruct"]["fx"].as_str(), false);
                    fx(&name, JsonPath::from("sections").key(label.as_str()).key("debris_fx"), sec["debris_fx"].as_str(), false);
                }
            } else if name == "data/weapons/guns.json" {
                for (gun, g) in doc.as_object().into_iter().flatten().filter(|(_, g)| g.is_object()) {
                    fx(&name, JsonPath::default().key(gun.as_str()).key("MuzzleFlashEffect"), g["MuzzleFlashEffect"].as_str(), false);
                    fx(&name, JsonPath::default().key(gun.as_str()).key("SoundName"), g["SoundName"].as_str(), true);
                }
            } else if name == "data/weapons/bullets.json" {
                for (bullet, b) in doc.as_object().into_iter().flatten().filter(|(_, b)| b.is_object()) {
                    for k in ["hit_fx", "bullet_fx"] {
                        fx(&name, JsonPath::default().key(bullet.as_str()).key(k), b[k].as_str(), false);
                    }
                    for (material, list) in b["impact_snd"].as_object().into_iter().flatten() {
                        for (i, n) in list.as_array().into_iter().flatten().enumerate() {
                            fx(&name, JsonPath::default().key(bullet.as_str()).key("impact_snd").key(material.as_str()).index(i), n.as_str(), true);
                        }
                    }
                    fx(&name, JsonPath::default().key(bullet.as_str()).key("travel_snd").index(0), b["travel_snd"][0].as_str(), true);
                    for (material, n) in b["ricochet_snd"].as_object().into_iter().flatten() {
                        fx(&name, JsonPath::default().key(bullet.as_str()).key("ricochet_snd").key(material.as_str()), n.as_str(), true);
                    }
                }
            }
        }
        self.out.extend(findings);
        // Every gun entry the mod touches fires a bullet that exists.
        let merged = |rel: &str| crate::data::try_json(&self.root.join(rel)).unwrap_or_default();
        let bullets: Vec<String> = merged("data/weapons/bullets.json").as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
        let guns = merged("data/weapons/guns.json");
        if let Some(mine) = crate::data::try_json(&self.dir.join("data/weapons/guns.json")).filter(Value::is_object) {
            for gun in mine.as_object().into_iter().flatten().map(|(k, _)| k).filter(|k| *k != "$schema") {
                if let Some(bullet) = guns[gun.as_str()]["BulletTypeName_Primary"].as_str().filter(|b| !bullets.iter().any(|x| x == b)) {
                    let best = nearest(bullet, bullets.iter().map(String::as_str)).map(String::from);
                    self.with_hint(
                        Level::Error,
                        "data/weapons/guns.json",
                        JsonPath::default().key(gun.as_str()).key("BulletTypeName_Primary"),
                        format!("bullet '{bullet}' is not in data/weapons/bullets.json"),
                        best.as_deref(),
                    );
                }
            }
        }
    }

    /// The gun and bullet entries the mod brings or changes, in the merged tables: every field the loader needs and the entry lacks (all
    /// of them, not the first), each at its own place (`/Gun_X/visual/pivot` in `data/weapons/guns.json`), then whatever else the loader
    /// refuses in the entry. Needs the base: without it a partial entry may be a change of a retail one. Returns whether it reported.
    fn weapons(&mut self) -> bool {
        if !self.have_base {
            return false;
        }
        let schemas = crate::data::schemas();
        let before = self.out.iter().filter(|f| f.level == Level::Error).count();
        for (file, schema_file, is_bullet) in [("data/weapons/guns.json", "guns.schema.json", false), ("data/weapons/bullets.json", "bullets.schema.json", true)] {
            let (Some(mine), Some(merged)) = (crate::data::try_json(&self.dir.join(file)).filter(Value::is_object), crate::data::try_json(&self.root.join(file))) else { continue };
            let Some((_, schema)) = schemas.iter().find(|(n, _)| *n == schema_file) else { continue };
            for key in mine.as_object().into_iter().flatten().map(|(k, _)| k).filter(|k| *k != "$schema") {
                let entry = &merged[key.as_str()];
                if !entry.is_object() {
                    self.error(file, JsonPath::default().key(key.as_str()), format!("the entry must be an object, not {entry}"));
                    continue;
                }
                let mut missing = vec![];
                missing_required(schema, &schema["$defs"]["entry"], entry, &mut vec![], &mut missing);
                for path in &missing {
                    let at = path.iter().fold(JsonPath::default().key(key.as_str()), |p, k| p.key(k.as_str()));
                    self.error(file, at, format!("the game needs `{}` in this entry (a new entry must state every required field)", path.join(".")));
                }
                if missing.is_empty() {
                    let loaded =
                        if is_bullet { crate::data::bullet_from_value(key, entry.clone()).map(|_| ()) } else { crate::data::gun_from_value(key, entry.clone()).map(|_| ()) };
                    if let Err(e) = loaded {
                        self.error(file, JsonPath::default().key(key.as_str()), e);
                    }
                }
            }
        }
        self.out.iter().filter(|f| f.level == Level::Error).count() > before
    }

    /// The ships the mod brings or changes: what their sections, mounts and engines name, against the model and the weapon tables.
    fn ships(&mut self, files: &[PathBuf], weapons_reported: bool) {
        let ids: BTreeSet<String> = files
            .iter()
            .filter_map(|f| {
                let parts: Vec<_> = f.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                (parts.len() == 3 && parts[0] == "ships" && parts[2] == "ship.json").then(|| parts[1].clone())
            })
            .collect();
        let loaded = self.have_base.then(|| crate::data::try_load_weapons(&self.root));
        if let Some(Err(e)) = &loaded {
            let weapons = ["guns", "bullets"].iter().any(|k| self.dir.join(format!("data/weapons/{k}.json")).is_file());
            if weapons && !weapons_reported {
                // The loader names the file of the entry it stopped at; the entry itself is not known here.
                let file = if e.contains("bullets.json") { "data/weapons/bullets.json" } else { "data/weapons/guns.json" };
                self.error(file, "", self.tidy(e));
            }
        }
        for id in ids {
            let file = format!("ships/{id}/ship.json");
            let Some(ship) = crate::data::try_json(&self.root.join(&file)).filter(Value::is_object) else { continue };
            let before = self.out.iter().filter(|f| f.level == Level::Error).count();
            self.ship_names(&id, &file, &ship);
            let new = self.base.as_ref().is_some_and(|b| !b.join(&file).is_file()) && !crate::data::try_json(&self.dir.join(&file)).is_some_and(|m| m.get("extends").is_some());
            if new {
                self.new_ship_fields(&file, &ship);
            }
            let errors_here = self.out.iter().filter(|f| f.level == Level::Error).count() > before;
            // What the checks above did not see: whatever else the loader refuses (a wrong type, a bad `crew_nodes` name). When the
            // weapon tables do not load, the ship is still loaded, without looking its guns up, so its own errors show too.
            let result = match &loaded {
                Some(Ok((guns, _))) => Some(crate::data::try_load_class(&self.root, &id, guns)),
                Some(Err(_)) => Some(crate::data::try_load_class_without_weapons(&self.root, &id)),
                None => None,
            };
            if let (false, Some(Err(e))) = (errors_here, result) {
                let message = self.tidy(&e);
                self.error(&file, "", format!("the game cannot load this ship: {message}"));
            }
        }
    }

    /// PROVISIONAL: a ship that extends nothing (`data::SHIP_REQUIRED_NEW`) must state the fields the loader would otherwise fill in
    /// silently, each missing one at its own place, and a non-empty `race`. `ship` is the merged file.
    fn new_ship_fields(&mut self, file: &str, ship: &Value) {
        for key in crate::authoring::required_new(file) {
            let path = key.split('.').fold(JsonPath::default(), |p, k| p.key(k));
            let value = key.split('.').try_fold(ship, |v, k| v.get(k));
            match value {
                None => self.error(file, path, format!("a new ship must state `{key}` (the loader would silently default it; a ship that extends another one inherits it)")),
                Some(v) if key == "race" && v.as_str().is_some_and(|r| r.trim().is_empty()) => {
                    self.error(
                        file,
                        path,
                        "`race` is empty: the race picks the ship's crew faces and voices, outfits, sails and trails, so a new ship must name one (such as Navy or Pirate)".into(),
                    );
                }
                Some(_) => {}
            }
        }
    }

    /// An error text of the loader with the folders taken out of its paths (the scratch overlay and the mod), and the
    /// `(source <path>)` the loader adds to a path it has already named.
    fn tidy(&self, text: &str) -> String {
        let mut text = text.replace(&format!("{}/", self.root.display()), "").replace(&format!("{}/", self.dir.display()), "");
        while let Some(i) = text.find(" (source ") {
            let Some(len) = text[i..].find(')') else { break };
            text.replace_range(i..=i + len, "");
        }
        text
    }

    fn ship_names(&mut self, id: &str, file: &str, ship: &Value) {
        let model_id = ship["model"].as_str().unwrap_or(id);
        let model_file = format!("ships/{model_id}/model.glb");
        let facts = std::fs::read(self.root.join(&model_file)).ok().and_then(|b| derive::read(&b));
        let geometry = ship["geometry"].as_str().unwrap_or(id);
        // The layers a section may name: a `geometry.json` when there is one, else the model's meshes.
        let layers: Option<Vec<String>> = crate::data::try_json(&self.root.join(format!("ships/{geometry}/geometry.json")))
            .and_then(|g| g.as_object().map(|m| m.keys().cloned().collect()))
            .or_else(|| facts.as_ref().map(|f| f.layers.iter().map(|l| l.name.clone()).collect()));
        let explicit: Vec<(&String, &Value)> = ship["sections"].as_object().map(|m| m.iter().collect()).unwrap_or_default();
        let section_layers: Vec<String> = if explicit.is_empty() {
            facts.iter().flat_map(|f| f.layers.iter()).filter(|l| !derive::is_damage_layer(&l.name)).map(|l| l.name.clone()).collect()
        } else {
            explicit.iter().filter_map(|(_, s)| s["layer"].as_str().map(String::from)).collect()
        };
        if let Some(layers) = &layers {
            for (label, s) in &explicit {
                if let Some(layer) = s["layer"].as_str().filter(|l| !layers.contains(&l.to_string())) {
                    let best = nearest(layer, layers.iter().map(String::as_str)).map(String::from);
                    self.with_hint(
                        Level::Error,
                        file,
                        JsonPath::from("sections").key(label.as_str()).key("layer"),
                        format!("layer '{layer}' is not a layer of the model {model_file}"),
                        best.as_deref(),
                    );
                }
            }
        }
        let known = |l: &str| section_layers.iter().any(|x| x == l);
        for (label, s) in &explicit {
            for (i, adj) in s["adjacent"].as_array().into_iter().flatten().filter_map(Value::as_str).enumerate() {
                if !known(adj) {
                    let best = nearest(adj, section_layers.iter().map(String::as_str)).map(String::from);
                    self.with_hint(
                        Level::Warning,
                        file,
                        JsonPath::from("sections").key(label.as_str()).key("adjacent").index(i),
                        format!("'{adj}' is the layer of no section; the game ignores it"),
                        best.as_deref(),
                    );
                }
            }
        }
        // `parent_layer` of mounts and engines names a section's layer; anything else leaves them attached to no section.
        let mut parents = vec![];
        for (label, g) in ship["guns"].as_object().into_iter().flatten() {
            parents.push((JsonPath::from("guns").key(label.as_str()).key("parent_layer"), g["parent_layer"].as_str()));
        }
        for (i, e) in ship["engines"].as_array().into_iter().flatten().enumerate() {
            parents.push((JsonPath::from("engines").index(i).key("parent_layer"), e["parent_layer"].as_str()));
        }
        for (path, parent) in parents {
            if let Some(l) = parent.filter(|l| !known(l)) {
                let best = nearest(l, section_layers.iter().map(String::as_str)).map(String::from);
                self.with_hint(Level::Warning, file, path, format!("'{l}' is the layer of no section; the mount is attached to none"), best.as_deref());
            }
        }
        // A damaged variant is shown only for a swappable section of its layer (the rule of `combat::damage`'s swap); else never.
        // Sections derived from the model are all swappable.
        let swappable: Vec<&String> = if explicit.is_empty() {
            section_layers.iter().collect()
        } else {
            explicit.iter().filter(|(_, s)| s["swappable"] != false).filter_map(|(_, s)| s["layer"].as_str()).filter_map(|l| section_layers.iter().find(|x| *x == l)).collect()
        };
        for l in facts.iter().flat_map(|f| f.layers.iter()).filter(|l| !l.name.contains('.')) {
            let Some(base) = derive::damaged_layer(&l.name) else { continue };
            if !swappable.iter().any(|s| s.eq_ignore_ascii_case(&base)) {
                let best = nearest(&base, swappable.iter().map(|s| s.as_str())).map(String::from);
                self.with_hint(
                    Level::Warning,
                    &model_file,
                    l.name.as_str(),
                    format!("'{}' is a damaged variant of '{base}', which is the layer of no swappable section, so it is never shown", l.name),
                    best.as_deref(),
                );
            }
        }
        // Mounts: the gun, and a hardpoint node for a mount that gives no place.
        let guns: Option<Vec<String>> = self
            .have_base
            .then(|| crate::data::try_json(&self.root.join("data/weapons/guns.json")).and_then(|g| g.as_object().map(|m| m.keys().cloned().collect())).unwrap_or_default());
        let nodes: Vec<&derive::Point> = facts.iter().flat_map(|f| f.points.iter()).filter(|p| p.kind == "GunPlacement").collect();
        for (label, g) in ship["guns"].as_object().into_iter().flatten() {
            if let (Some(guns), Some(gun)) = (&guns, g["gun"].as_str())
                && !guns.iter().any(|x| x == gun)
            {
                let best = nearest(gun, guns.iter().map(String::as_str)).map(String::from);
                self.with_hint(Level::Error, file, JsonPath::from("guns").key(label.as_str()).key("gun"), format!("gun '{gun}' is not in data/weapons/guns.json"), best.as_deref());
            }
            if g["pos"].is_array() && g["rot"].is_array() {
                continue;
            }
            if facts.is_none() && !self.have_base {
                continue; // the model may be a retail one this check cannot read
            }
            match facts.as_ref().and_then(|f| derive::mount_node(f, label)) {
                Some(node) if node.inferred => self.warn(
                    file,
                    JsonPath::from("guns").key(label.as_str()),
                    format!("the node '{}' of {model_file} is taken as this mount's hardpoint only because its name contains 'mount', 'lancer' or 'turret'; give the node the extra `kind` = \"GunPlacement\" so it does not depend on its name", node.name),
                ),
                Some(_) => {}
                None => {
                    let like = nodes.iter().find(|p| {
                        let (a, b) = (p.name.to_lowercase(), label.to_lowercase());
                        a.contains(&b) || b.contains(&a)
                    });
                    let tail = like.map_or_else(String::new, |p| format!(" (the node '{}' only contains the name; the match is by the whole name, ignoring case)", p.name));
                    self.error(file, JsonPath::from("guns").key(label.as_str()), format!("has no pos/rot and the model has no hardpoint node named '{label}'{tail}"));
                }
            }
        }
    }

    /// Every `maps/<id>/` the mod brings, through the mission checker, with the checks only `--check-mod` adds.
    fn missions(&mut self) {
        let maps = self.dir.join("maps");
        let mut ids: Vec<String> =
            std::fs::read_dir(&maps).into_iter().flatten().flatten().filter(|e| e.path().is_dir()).filter_map(|e| e.file_name().into_string().ok()).collect();
        ids.sort();
        let sounds = self.have_base.then(|| sound_stems(&self.root));
        let models = self.have_base.then(|| model_files(&self.root));
        let base_sounds = std::cell::OnceCell::new();
        let base_models = std::cell::OnceCell::new();
        for id in ids {
            let dir = self.root.join("maps").join(&id);
            let Some(rules) = crate::rules::cli::rules_file(&dir) else { continue };
            let rules_name = rules.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            // The same map in the base game, when the mod touches a retail one: its own warnings are not the mod's.
            let base_dir = self.base.as_ref().map(|b| b.join("maps").join(&id)).filter(|d| d.is_dir());
            if let Some(bd) = &base_dir
                && ["map.json", "media.json", rules_name.as_str()].iter().all(|f| same_file(&dir.join(f), &bd.join(f)))
            {
                continue; // the merged files are the base's own: nothing of the mod to check
            }
            let mut found = match map_findings(&self.root, &dir, &id, &rules_name, sounds.as_ref(), models.as_ref()) {
                Ok(found) => found,
                Err(e) => {
                    self.error(&format!("maps/{id}/{rules_name}"), "", e);
                    continue;
                }
            };
            if let (Some(bd), Some(base)) = (&base_dir, &self.base) {
                let (bs, bm) = (base_sounds.get_or_init(|| sound_stems(base)), base_models.get_or_init(|| model_files(base)));
                if let Ok(inherited) = map_findings(base, bd, &id, &rules_name, Some(bs), Some(bm)) {
                    mark_inherited(&mut found, &inherited);
                }
            }
            self.out.extend(found);
        }
    }
}

/// Everything `--check-mod` says about the map folder `dir` (`maps/<id>` under `root`, which is where ships, sounds and models are looked
/// up; `sounds`/`models` are `None` without imported assets): the mission checker's findings and the extra warnings.
fn map_findings(root: &Path, dir: &Path, id: &str, rules_name: &str, sounds: Option<&HashSet<String>>, models: Option<&HashSet<String>>) -> Result<Vec<Finding>, String> {
    let have_sound = |f: &str| sounds.is_some_and(|s| s.contains(&f.to_lowercase()));
    let lookups: Option<(&Path, &dyn Fn(&str) -> bool)> = sounds.map(|_| (root, &have_sound as &dyn Fn(&str) -> bool));
    let mut out: Vec<Finding> = crate::rules::cli::check_map_in(dir, lookups)?
        .into_iter()
        .map(|mut f| {
            f.file = format!("maps/{}", f.file);
            f
        })
        .collect();
    let rules = dir.join(rules_name);
    let (Some(mission), map) = (crate::data::try_json(&rules), crate::data::try_json(&dir.join("map.json")).unwrap_or_default()) else { return Ok(out) };
    let mission_file = format!("maps/{id}/{rules_name}");
    out.extend(unused_definitions(&mission).into_iter().map(|f| f.with_file(&mission_file)));
    // Findings of `rules::check` name `map.json` or nothing (the rules file); here they get the map folder.
    let map_file = format!("maps/{id}/map.json");
    let placed = outside_world(&mission, &map).into_iter().chain(models.map(|m| missing_scenery(&map, m)).unwrap_or_default());
    out.extend(placed.map(|f| {
        let file = if f.file == "map.json" { map_file.clone() } else { mission_file.clone() };
        f.with_file(&file)
    }));
    Ok(out)
}

/// Both files are missing, or both are there with the same bytes.
fn same_file(a: &Path, b: &Path) -> bool {
    std::fs::read(a).ok() == std::fs::read(b).ok()
}

/// What makes two findings the same one for inheritance: the file, the place (JSON pointer, or the rule by name, its list and field,
/// never by index: indices shift when a mod adds rules), and the message.
fn inherit_key(f: &Finding) -> (String, String, String, String, Option<String>, String) {
    (f.file.clone(), f.pointer.clone(), f.rule.clone(), f.field.clone(), f.list.clone(), f.message.clone())
}

/// Sets `inherited` on each warning of `found` that the base game's own run (`base`) has too. Each base warning covers one mod warning,
/// so a mod that repeats a retail warning gets the extra copy as its own. Errors are never marked.
fn mark_inherited(found: &mut [Finding], base: &[Finding]) {
    let mut left: std::collections::HashMap<_, usize> = std::collections::HashMap::new();
    for f in base.iter().filter(|f| f.level == Level::Warning) {
        *left.entry(inherit_key(f)).or_default() += 1;
    }
    for f in found.iter_mut().filter(|f| f.level == Level::Warning) {
        if let Some(n) = left.get_mut(&inherit_key(f)).filter(|n| **n > 0) {
            *n -= 1;
            f.inherited = true;
        }
    }
}

/// What the skirmish lobby cannot start on a map's `teams` (the team list of a listed, open map whose fleet types are not free):
/// `(path, message)` for each team without a numeric `race`. The lobby takes the human's fleet type from the team and, with the
/// team's fleet type left open, cannot launch (`PLEASE_SELECT_A_RACE`) because the pull-down only works on free-for-all maps. Free-for-all
/// maps (`alliance_change` true, or no teams) and historical maps (`assemble` false, which place their fleets) are not affected.
fn skirmish_team_problems(map: &Value) -> Vec<(JsonPath, String)> {
    let doc = crate::data::MapDoc::from_value(map);
    let free = doc.alliance_change.unwrap_or(doc.teams.is_empty());
    if doc.skirmish != Some(true) || doc.assemble != Some(true) || free {
        return vec![];
    }
    let raw = map["teams"].as_array().cloned().unwrap_or_default();
    raw.iter()
        .enumerate()
        .filter(|(_, t)| t["race"].as_u64().is_none())
        .map(|(i, t)| {
            let name = t["name_id"].as_str().map_or(String::new(), |n| format!(" ({n})"));
            (JsonPath::default().key("teams").index(i).key("race"), format!("team {i}{name} has no `race`: the skirmish lobby takes each team's fleet type from it and cannot start the map (set the team's `race`, or make the map free-for-all with `alliance_change`)"))
        })
        .collect()
}

/// The schema file (`authoring::schemas`) that describes the mod file at `rel`, if any.
/// The paths (below the entry) of every `required` field of `node` that `value` lacks, in the schema of `root`, going into the objects
/// that are there; an object that is missing is reported by itself.
fn missing_required(root: &Value, node: &Value, value: &Value, at: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    let Some(object) = value.as_object() else { return };
    for key in node["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        if !object.contains_key(key) {
            out.push(at.iter().cloned().chain([key.to_string()]).collect());
        }
    }
    for (key, sub) in node["properties"].as_object().into_iter().flatten() {
        let Some(child) = object.get(key).filter(|c| c.is_object()) else { continue };
        let target = super::annotate::def_name(sub).map_or(sub, |n| &root["$defs"][n.as_str()]);
        if target["properties"].is_object() {
            at.push(key.clone());
            missing_required(root, target, child, at, out);
            at.pop();
        }
    }
}

fn schema_of(rel: &str) -> Option<&'static str> {
    match rel {
        "mod.json" => Some("mod.schema.json"),
        "data/weapons/guns.json" => Some("guns.schema.json"),
        "data/weapons/bullets.json" => Some("bullets.schema.json"),
        "data/crew_nodes.json" => Some("crew_nodes.schema.json"),
        _ => {
            let parts: Vec<&str> = rel.split('/').collect();
            (parts.len() == 3 && parts[0] == "ships" && parts[2] == "ship.json").then_some("ship.schema.json")
        }
    }
}

/// Lower-case names of every `.ogg` under `sound/` and `maps/*/speech/` of `root`: what a sound is looked up by (`audio::sound`).
fn sound_stems(root: &Path) -> HashSet<String> {
    let mut files = vec![];
    modding::walk(&root.join("sound"), Path::new(""), &mut files);
    for m in std::fs::read_dir(root.join("maps")).into_iter().flatten().flatten() {
        modding::walk(&m.path().join("speech"), Path::new(""), &mut files);
    }
    files.iter().filter(|p| p.extension().is_some_and(|e| e == "ogg")).filter_map(|p| Some(p.file_stem()?.to_str()?.to_lowercase())).collect()
}

/// File names of every model under `models/` of `root` (`data::model` finds a scenery mesh by its file name).
fn model_files(root: &Path) -> HashSet<String> {
    let mut files = vec![];
    modding::walk(&root.join("models"), Path::new(""), &mut files);
    files.iter().filter_map(|p| Some(p.file_name()?.to_str()?.to_string())).collect()
}

/// The keys of `value` that the JSON Schema `root` does not list, as `(path, keys the schema lists there)`, in document order. A path
/// is `sections.Hull.layer` / `engines[0].fx`. Follows `$ref` into `$defs`; an optional type (`anyOf [T, null]`) is read as the variant
/// the value fits best. Maps with `additionalProperties` accept any key. [`unknown_key_findings`] gives the same as findings, with a
/// JSON pointer and the nearest known key.
pub fn unknown_keys(root: &Value, value: &Value) -> Vec<(String, Vec<String>)> {
    unknown_key_paths(root, value).into_iter().map(|(path, known)| (path.dotted(), known)).collect()
}

/// The unknown keys of `value` (see [`unknown_keys`]) as the warnings `--check-mod` prints for `file`: `path` and `pointer` name the
/// key, `suggestion` is the nearest key the schema lists there, when one is near.
pub fn unknown_key_findings(root: &Value, value: &Value, file: &str) -> Vec<Finding> {
    unknown_key_paths(root, value)
        .into_iter()
        .map(|(path, known)| {
            let leaf = match path.0.last() {
                Some(crate::rules::check::Segment::Key(k)) => k.clone(),
                _ => String::new(),
            };
            let best = nearest(&leaf, known.iter().map(String::as_str)).map(String::from);
            let mut f = Finding::in_file(Level::Warning, file, path, format!("unknown key '{leaf}'; the game ignores it{}", did_you_mean(best.as_deref())));
            f.suggestion = best;
            f
        })
        .collect()
}

fn unknown_key_paths(root: &Value, value: &Value) -> Vec<(JsonPath, Vec<String>)> {
    let mut out = vec![];
    walk(root, root, value, &JsonPath::default(), &mut out);
    out
}

fn walk(root: &Value, node: &Value, value: &Value, at: &JsonPath, out: &mut Vec<(JsonPath, Vec<String>)>) {
    if let Some(r) = node["$ref"].as_str() {
        if let Some(target) = root.pointer(r.trim_start_matches('#')) {
            walk(root, target, value, at, out);
        }
        return;
    }
    if let Some(variants) = node["anyOf"].as_array() {
        let best = variants
            .iter()
            .filter(|v| v["type"] != "null")
            .map(|v| {
                let mut o = vec![];
                walk(root, v, value, at, &mut o);
                o
            })
            .min_by_key(Vec::len)
            .unwrap_or_default();
        out.extend(best);
        return;
    }
    let child = |key: &str| at.clone().key(key);
    match (value, node["properties"].as_object()) {
        (Value::Object(obj), Some(props)) => {
            for (k, v) in obj {
                match props.get(k) {
                    Some(p) => walk(root, p, v, &child(k), out),
                    None if node["additionalProperties"].is_object() => walk(root, &node["additionalProperties"], v, &child(k), out),
                    None => out.push((child(k), props.keys().filter(|k| *k != "$schema").cloned().collect())),
                }
            }
        }
        (Value::Object(obj), None) if node["additionalProperties"].is_object() => {
            for (k, v) in obj {
                walk(root, &node["additionalProperties"], v, &child(k), out);
            }
        }
        (Value::Array(items), _) if node.get("items").is_some() => {
            for (i, v) in items.iter().enumerate() {
                walk(root, &node["items"], v, &at.clone().index(i), out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::derive::tests::glb;
    use serde_json::json;

    fn scratch(tag: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("procyon-checkmod-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(root: &Path, rel: &str, text: impl AsRef<[u8]>) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn put_json(root: &Path, rel: &str, v: Value) {
        put(root, rel, v.to_string());
    }

    /// The findings as `file: level path: message` lines (the file and path are the data a tool would use).
    fn lines(found: &[Finding]) -> Vec<String> {
        found.iter().map(|f| format!("{}: {:?} {}: {}", f.file, f.level, f.path, f.message)).collect()
    }

    fn has(found: &[Finding], level: Level, file: &str, path: &str, needle: &str) -> bool {
        found.iter().any(|f| f.level == level && f.file == file && f.path == path && f.message.contains(needle))
    }

    fn gun() -> Value {
        json!({"MuzzleSpeed": 1, "TimeBetweenBursts": 1, "BulletsPerBurst": 1, "DuringBurstReloadTime": 1, "EffectiveRange_Range": 1, "LongRange_Range": 1,
            "MaximumRange_Range": 1, "EffectiveRange_AccuracyDeviation": 1, "LongRange_AccuracyDeviation": 1, "MaximumRange_AccuracyDeviation": 1, "BulletAffectedByGravity": false,
            "BulletTypeName_Primary": "B", "visual": {"base": "a", "pivot": [0, 0, 0], "muzzles": [[0, 0, 0]]}, "HorizontalRotationSpeed": 1, "VerticalRotationSpeed": 1,
            "VerticalMinAngle": 0, "VerticalMaxAngle": 1, "Mount Type": "Medium Mount"})
    }

    /// A new ship's `ship.json`: every field of `SHIP_REQUIRED_NEW`, then `extra` over it.
    fn new_ship(extra: Value) -> Value {
        let mut ship =
            json!({"race": "Navy", "core_hp": 10, "points": 5, "physics": {"Mass": 1, "MaxThrust": 1, "MaxSpeed": 1, "RotationalFriction": 1, "MaxAngularAcceleration": 1}});
        ship.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        ship
    }
    fn bullet() -> Value {
        json!({"lifetime": 1, "impact_vol": 1, "impact_max": 1, "impact_snd": {"Wood": ["real"]}, "damage": {"Wood": [1, 2, 0, 0, 0]}, "hit_fx": "Real"})
    }

    /// A model with a hull, a damaged variant of a layer no section has, a node that is a gun mount by its name alone and one that
    /// merely contains a mount's name.
    fn small_model(damaged: bool) -> Vec<u8> {
        let mut bin = vec![];
        for c in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0] {
            bin.extend(c.to_le_bytes());
        }
        let json = json!({
            "asset": {"version": "2.0"}, "scene": 0, "scenes": [{"nodes": [0, 1, 2, 3]}],
            "buffers": [{"byteLength": bin.len()}], "bufferViews": [{"buffer": 0, "byteLength": bin.len()}],
            "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
            "nodes": [{"name": "Hull", "mesh": 0}, {"name": if damaged { "HullDamage_Two" } else { "Plank" }, "mesh": 0}, {"name": "Turret_1", "translation": [0.0, 1.0, 0.0]}, {"name": "Mount_B_aft", "translation": [0.0, 1.0, 1.0]}]
        });
        glb(json, &bin)
    }

    #[test]
    fn an_open_team_map_whose_teams_lack_a_race_is_an_error_the_lobby_would_have_hit() {
        let map = |extra: Value| {
            let mut m = json!({"skirmish": true, "assemble": true, "players": [{"playable": true, "team": 0}, {"playable": true, "team": 1}],
                "teams": [{"name_id": "TEAM_A", "lock": 0, "race": 0}, {"name_id": "TEAM_B", "lock": 0}]});
            m.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            m
        };
        let run = |tag: &str, m: Value| {
            let mod_dir = scratch(tag);
            put_json(&mod_dir, "mod.json", json!({"id": "m"}));
            put_json(&mod_dir, "maps/arena/map.json", m);
            check_mod_with(&mod_dir, None, &[])
        };
        let found = run("teamrace", map(json!({})));
        let f = found.iter().find(|f| f.pointer == "/teams/1/race").unwrap_or_else(|| panic!("{found:#?}"));
        assert!(f.level == Level::Error && f.file == "maps/arena/map.json" && f.message.contains("TEAM_B") && f.message.contains("race"), "{f:?}");
        assert_eq!(found.iter().filter(|f| f.pointer.ends_with("/race")).count(), 1, "the team with a race is fine");
        // not skirmish-listed, historical, free-for-all or complete: nothing
        for (tag, extra) in [("t1", json!({"skirmish": false})), ("t2", json!({"assemble": false})), ("t3", json!({"alliance_change": true}))] {
            assert!(run(tag, map(extra)).iter().all(|f| !f.pointer.ends_with("/race")), "{tag}");
        }
        let mut fixed = map(json!({}));
        fixed["teams"][1]["race"] = json!(1);
        assert!(run("t4", fixed).iter().all(|f| !f.pointer.ends_with("/race")));
    }

    #[test]
    fn a_finding_carries_a_json_pointer_that_names_keys_holding_dots_slashes_and_digits() {
        let mod_dir = scratch("pointer-mod");
        put_json(&mod_dir, "mod.json", json!({"id": "m"}));
        put_json(&mod_dir, "ships/Ship_P/ship.json", json!({"guns": {"Mount.1/a": {"arc_dge": 9}}, "sections": {"7": {"layr": "A"}}}));
        let found = check_mod_with(&mod_dir, None, &[]);
        let f = found.iter().find(|f| f.path.ends_with(".arc_dge")).unwrap_or_else(|| panic!("{found:#?}"));
        assert_eq!((f.path.as_str(), f.pointer.as_str()), ("guns.Mount.1/a.arc_dge", "/guns/Mount.1~1a/arc_dge"), "the dotted path is ambiguous, the pointer is not");
        assert_eq!(f.suggestion.as_deref(), Some("arc_deg"));
        let ship: Value = serde_json::from_str(&std::fs::read_to_string(mod_dir.join("ships/Ship_P/ship.json")).unwrap()).unwrap();
        assert!(ship.pointer(&f.pointer).is_some(), "the pointer finds the key in the file");
        let g = found.iter().find(|f| f.path.ends_with(".layr")).unwrap();
        assert_eq!(g.pointer, "/sections/7/layr", "a key made of digits stays a key");
        assert!(ship.pointer(&g.pointer).is_some());
        // the library form: the same warnings for any document and schema
        let schema = &crate::authoring::schemas()[0].1;
        let direct = unknown_key_findings(schema, &json!({"sigth": 1}), "ships/S/ship.json");
        assert_eq!((direct[0].pointer.as_str(), direct[0].suggestion.as_deref(), direct[0].file.as_str()), ("/sigth", Some("sight"), "ships/S/ship.json"));
        // --check-mod --json prints the field
        assert!(serde_json::to_value(&direct[0]).unwrap()["pointer"] == "/sigth");
        std::fs::remove_dir_all(mod_dir).unwrap();
    }

    #[test]
    fn without_assets_the_mods_own_files_are_checked_and_what_needs_the_game_is_one_notice() {
        let (dir, mod_dir) = (scratch("noassets"), scratch("noassets-mod"));
        put_json(
            &mod_dir,
            "mod.json",
            json!({"id": "m", "name": 5, "default": "yes", "game_version": "99.0.0", "format": 2, "requires": ["ghost"], "missions": ["m1", "nope"], "colour": "red"}),
        );
        // extends a retail ship (cannot be merged here), a ship with typos, a file that is not JSON
        put_json(&mod_dir, "ships/Ship_X/ship.json", json!({"extends": "Ship_Retail", "points": 100}));
        put_json(&mod_dir, "ships/Ship_Y/ship.json", json!({"sigth": 1, "sections": {"Hull": {"layr": "A"}}, "tow": {"tower": [0, 0, 0], "tow": 1}, "engines": [{"lable": "E"}]}));
        put(&mod_dir, "ships/Ship_Z/ship.json", "not json");
        put_json(&mod_dir, "data/weapons/guns.json", json!({"$schema": "x", "Gun_A": {"Muzzlespeed": 2, "MuzzleSpeed": 1}}));
        put_json(&mod_dir, "data/weapons/bullets.json", json!({"B": {"liftime": 1}}));
        put_json(&mod_dir, "data/crew_nodes.json", json!({"Town": [{"name": "Crew_Walk_A_0", "pos": [0, 0, 0], "pso": 1}]}));
        let rule = |n: &str| json!({"name": n, "Run Once": true, "conditions": [{"Type": "World Initialize"}], "actions": [{"Type": "OP Wait", "Seconds": 1}]});
        put_json(
            &mod_dir,
            "maps/m1/mission.json",
            json!({"players": ["P"], "ships": [{"id": 1, "type": "Anything", "group": "P", "pos": [5000, 0, 0]}], "groups": {"Spare": [1]}, "rules": [rule("Twice"), rule("Twice")]}),
        );
        put_json(&mod_dir, "maps/m1/map.json", json!({"world_half": 100, "objects": [{"type": "Rock", "scene": "Rock", "pos": [0, -300, 0]}]}));
        put_json(&mod_dir, "maps/m1/media.json", json!({}));
        let found = check_mod_with(&mod_dir, None, &[]);
        let all = lines(&found).join("\n");
        for (level, file, path, needle) in [
            (Level::Error, "mod.json", "name", "not a string"),
            (Level::Error, "mod.json", "default", "not true or false"),
            (Level::Warning, "mod.json", "game_version", "asks for OpenProcyon 99.0.0 or newer"),
            (Level::Warning, "mod.json", "format", "mod format 2"),
            (Level::Warning, "mod.json", "requires", "\"ghost\", which is not installed"),
            (Level::Warning, "mod.json", "missions", "'nope' has no maps/nope/mission.json"),
            (Level::Warning, "mod.json", "colour", "unknown key 'colour'"),
            (Level::Warning, "ships/Ship_Y/ship.json", "sigth", "unknown key 'sigth'"),
            (Level::Warning, "ships/Ship_Y/ship.json", "sections.Hull.layr", "unknown key 'layr'"),
            (Level::Warning, "ships/Ship_Y/ship.json", "tow.tow", "unknown key 'tow'"),
            (Level::Warning, "ships/Ship_Y/ship.json", "engines[0].lable", "unknown key 'lable'"),
            (Level::Warning, "data/weapons/guns.json", "Gun_A.Muzzlespeed", "unknown key 'Muzzlespeed'"),
            (Level::Warning, "data/weapons/bullets.json", "B.liftime", "unknown key 'liftime'"),
            (Level::Warning, "data/crew_nodes.json", "Town[0].pso", "unknown key 'pso'"),
            (Level::Error, "ships/Ship_Z/ship.json", "", "expected"),
            (Level::Warning, "maps/m1/mission.json", "", "another rule has the same name"),
            (Level::Warning, "maps/m1/mission.json", "groups.Spare", "group 'Spare' is defined but no rule uses it"),
            (Level::Warning, "maps/m1/mission.json", "ships[0].pos", "outside world_half 100"),
            (Level::Warning, "maps/m1/map.json", "objects[0].pos", "Rock is at"),
        ] {
            assert!(has(&found, level, file, path, needle), "missing {level:?} {file} {path} {needle:?} in\n{all}");
        }
        let pointer = |path: &str| found.iter().find(|f| f.path == path).unwrap().pointer.clone();
        assert_eq!(
            [pointer("sections.Hull.layr"), pointer("engines[0].lable"), pointer("Town[0].pso"), pointer("ships[0].pos"), pointer("objects[0].pos"), pointer("groups.Spare")],
            ["/sections/Hull/layr", "/engines/0/lable", "/Town/0/pso", "/ships/0/pos", "/objects/0/pos", "/groups/Spare"]
        );
        let hint = |path: &str| found.iter().find(|f| f.path == path).unwrap().suggestion.clone();
        assert_eq!((hint("sigth").as_deref(), hint("sections.Hull.layr").as_deref(), hint("Gun_A.Muzzlespeed").as_deref()), (Some("sight"), Some("layer"), Some("MuzzleSpeed")));
        assert!(!all.contains("/tmp") && !all.contains("(source"), "paths in messages are the mod's own: {all}");
        // the retail-extending ship is not an error without the game's files; neither are ships, sounds and models
        assert!(!all.contains("Ship_X"), "{all}");
        assert!(!all.contains("does not exist"), "{all}");
        let notices: Vec<_> = found.iter().filter(|f| f.message.contains("no imported assets")).collect();
        assert_eq!(notices.len(), 1, "{all}");
        assert!(notices[0].message.contains("1 file(s) that extend a retail file"), "{}", notices[0].message);
        assert_eq!(found.last().unwrap().message, notices[0].message, "the notice comes last");
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_dir_all(mod_dir).unwrap();
    }

    #[test]
    fn a_required_mod_that_is_installed_is_laid_under_the_mod() {
        let (dep, mod_dir) = (scratch("dep"), scratch("dep-mod"));
        put_json(&dep, "mod.json", json!({"id": "base-pack"}));
        put_json(&dep, "ships/Ship_Base/ship.json", json!({"core_hp": 10, "points": 5}));
        put_json(&mod_dir, "mod.json", json!({"id": "m", "requires": ["base-pack"]}));
        put_json(&mod_dir, "ships/Ship_Child/ship.json", json!({"extends": "Ship_Base", "points": 100}));
        let installed = [Installed { id: "base-pack".into(), dir: dep.clone(), enabled: false, ..Installed::default() }];
        let found = check_mod_with(&mod_dir, None, &installed);
        assert!(!found.iter().any(|f| f.level == Level::Error || f.file == "mod.json"), "{:?}", lines(&found));
        let alone = check_mod_with(&mod_dir, None, &[]);
        assert!(has(&alone, Level::Warning, "mod.json", "requires", "base-pack"), "{:?}", lines(&alone));
        std::fs::remove_dir_all(dep).unwrap();
        std::fs::remove_dir_all(mod_dir).unwrap();
    }

    #[test]
    fn a_new_weapon_entry_reports_every_missing_field_at_its_own_place_and_a_change_of_a_retail_entry_is_fine() {
        let (base, mod_dir) = (scratch("weapons-base"), scratch("weapons-mod"));
        put_json(&base, "data/weapons/guns.json", json!({"Gun": gun()}));
        put_json(&base, "data/weapons/bullets.json", json!({"B": bullet()}));
        put(&base, "ships/readme.txt", "x");
        let mut bad_type = gun();
        bad_type["MuzzleSpeed"] = json!("fast");
        put_json(
            &mod_dir,
            "data/weapons/guns.json",
            json!({"Gun": {"MuzzleSpeed": 5}, "Gun_Scratch": {"BulletTypeName_Primary": "B", "visual": {"base": "a"}}, "Gun_Bad": bad_type, "Gun_Ok": gun()}),
        );
        put_json(&mod_dir, "data/weapons/bullets.json", json!({"B": {"lifetime": 2}, "B_New": {"lifetime": 1, "hit_fx": "H"}}));
        let found = check_mod_with(&mod_dir, Some(&base), &[]);
        let all = lines(&found).join("\n");
        let guns = "data/weapons/guns.json";
        let errors = |file: &str| found.iter().filter(|f| f.level == Level::Error && f.file == file).collect::<Vec<_>>();
        // every missing field of the new gun at once, with a pointer to it (and nothing for the entries that are fine)
        let scratch_errors: Vec<_> = errors(guns).into_iter().filter(|f| f.pointer.starts_with("/Gun_Scratch")).collect();
        let pointers: Vec<&str> = scratch_errors.iter().map(|f| f.pointer.as_str()).collect();
        for want in
            ["/Gun_Scratch/MuzzleSpeed", "/Gun_Scratch/HorizontalRotationSpeed", "/Gun_Scratch/visual/pivot", "/Gun_Scratch/visual/muzzles", "/Gun_Scratch/VerticalMaxAngle"]
        {
            assert!(pointers.contains(&want), "{want} missing from {pointers:?}\n{all}");
        }
        assert_eq!(scratch_errors.len(), 17, "the 15 top-level fields it lacks and the 2 inside `visual`: {pointers:?}");
        assert!(has(&found, Level::Error, guns, "Gun_Scratch.visual.pivot", "`visual.pivot`"), "{all}");
        assert!(errors(guns).iter().all(|f| !f.pointer.starts_with("/Gun/") && f.pointer != "/Gun_Ok" && !f.pointer.starts_with("/Gun_Ok/")), "{all}");
        // a field of the wrong type is the loader's error, at its entry
        assert!(has(&found, Level::Error, guns, "Gun_Bad", "invalid type: string \"fast\""), "{all}");
        // bullets: a new entry lacks impact_vol, impact_max, impact_snd, damage; the change to the retail `B` is fine
        let bullets = "data/weapons/bullets.json";
        let mut got: Vec<&str> = errors(bullets).iter().map(|f| f.pointer.as_str()).collect();
        got.sort();
        assert_eq!(got, ["/B_New/damage", "/B_New/impact_max", "/B_New/impact_snd", "/B_New/impact_vol"], "{all}");
        // no finding is filed under the bare directory any more
        assert!(found.iter().all(|f| f.file != "data/weapons"), "{all}");
        std::fs::remove_dir_all(base).unwrap();
        std::fs::remove_dir_all(mod_dir).unwrap();
    }

    #[test]
    fn with_assets_names_are_checked_against_the_model_the_weapons_and_the_folders() {
        let (base, mod_dir) = (scratch("assets"), scratch("assets-mod"));
        put_json(&base, "data/weapons/guns.json", json!({"Gun": gun()}));
        put_json(&base, "data/weapons/bullets.json", json!({"B": bullet()}));
        put(&base, "sound/ship/real.ogg", "x");
        put(&base, "fx/Real.json", "x");
        put(&base, "ships/readme.txt", "x");
        put(&base, "models/scenery/Island.glb", "x");
        put(&mod_dir, "ships/Ship_M/model.glb", small_model(true));
        put_json(
            &mod_dir,
            "ships/Ship_M/ship.json",
            new_ship(json!({
                "explosion_fx": "Nofx", "engine_snd": ["real", "gone"],
                "guns": {"Turret_1": {"gun": "Gun", "arc_deg": 60}, "Mount_B": {"gun": "Gunn", "arc_deg": 60}, "Free": {"gun": "Gun", "arc_deg": 1, "pos": [0, 0, 0], "rot": [0, 0, 0], "parent_layer": "Nowhere"}}
            })),
        );
        put_json(
            &mod_dir,
            "ships/Ship_S/ship.json",
            new_ship(
                json!({"sections": {"Deck": {"layer": "Hulll", "material": "Wood", "hp": 1, "core_pct": 1, "vital_ship": true, "vital_speed": false, "vital_turn": false, "adjacent": ["Hull", "Ghost"]}}}),
            ),
        );
        put(&mod_dir, "ships/Ship_S/model.glb", small_model(false));
        put(&mod_dir, "ships/Ship_Ok/model.glb", small_model(false));
        put_json(&mod_dir, "ships/Ship_Ok/ship.json", new_ship(json!({})));
        put(&mod_dir, "ships/Ship_Bad/model.glb", small_model(false));
        put_json(&mod_dir, "ships/Ship_Bad/ship.json", new_ship(json!({"core_hp": "lots"})));
        put_json(
            &mod_dir,
            "data/weapons/guns.json",
            json!({"Gun_New": {"BulletTypeName_Primary": "Bb", "MuzzleFlashEffect": "Nope", "SoundName": "real", "MuzzleSpeed": 1, "TimeBetweenBursts": 1, "BulletsPerBurst": 1, "DuringBurstReloadTime": 1, "EffectiveRange_Range": 1, "LongRange_Range": 1,
            "MaximumRange_Range": 1, "EffectiveRange_AccuracyDeviation": 1, "LongRange_AccuracyDeviation": 1, "MaximumRange_AccuracyDeviation": 1, "BulletAffectedByGravity": false,
            "visual": {"pivot": [0, 0, 0], "muzzles": [[0, 0, 0]]}, "HorizontalRotationSpeed": 1, "VerticalRotationSpeed": 1, "VerticalMinAngle": 0, "VerticalMaxAngle": 1}}),
        );
        put_json(
            &mod_dir,
            "data/weapons/bullets.json",
            json!({"B2": {"hit_fx": "Real", "impact_snd": {"Wood": ["real", "lost"]}, "lifetime": 1, "impact_vol": 1, "impact_max": 1, "damage": {"Wood": [1, 2, 0, 0, 0]}}}),
        );
        put_json(&mod_dir, "maps/m1/mission.json", json!({"players": ["P"], "ships": [{"id": 1, "type": "Ship_Ghost", "group": "P", "pos": [0, 0, 0]}], "rules": []}));
        put_json(&mod_dir, "maps/m1/map.json", json!({"objects": [{"type": "Island", "scene": "Island", "pos": [0, 0, 0]}, {"type": "Isle", "scene": "Isel", "pos": [0, 0, 0]}]}));
        put_json(&mod_dir, "maps/m1/media.json", json!({}));
        let found = check_mod_with(&mod_dir, Some(&base), &[]);
        let all = lines(&found).join("\n");
        let m = "ships/Ship_M/ship.json";
        for (level, file, path, needle) in [
            (Level::Warning, m, "explosion_fx", "'Nofx' is not a effect"),
            (Level::Warning, m, "engine_snd[1]", "'gone' is not a sound"),
            (Level::Warning, m, "guns.Turret_1", "only because its name contains"),
            (Level::Error, m, "guns.Mount_B", "no hardpoint node named 'Mount_B' (the node 'Mount_B_aft' only contains the name"),
            (Level::Error, m, "guns.Mount_B.gun", "gun 'Gunn' is not in data/weapons/guns.json (did you mean 'Gun'?)"),
            (Level::Warning, m, "guns.Free.parent_layer", "'Nowhere' is the layer of no section"),
            (Level::Warning, "ships/Ship_M/model.glb", "HullDamage_Two", "is a damaged variant of 'Hull_Two'"),
            (Level::Error, "ships/Ship_S/ship.json", "sections.Deck.layer", "layer 'Hulll' is not a layer of the model ships/Ship_S/model.glb"),
            (Level::Warning, "ships/Ship_S/ship.json", "sections.Deck.adjacent[1]", "'Ghost' is the layer of no section"),
            (Level::Error, "ships/Ship_Bad/ship.json", "", "the game cannot load this ship"),
            (Level::Error, "data/weapons/guns.json", "Gun_New.BulletTypeName_Primary", "bullet 'Bb' is not in data/weapons/bullets.json"),
            (Level::Warning, "data/weapons/guns.json", "Gun_New.MuzzleFlashEffect", "'Nope' is not a effect"),
            (Level::Warning, "data/weapons/bullets.json", "B2.impact_snd.Wood[1]", "'lost' is not a sound"),
            (Level::Error, "maps/m1/mission.json", "ships[0].type", "ship class 'Ship_Ghost' does not exist"),
            (Level::Warning, "maps/m1/map.json", "objects[1].scene", "the scenery model 'Isel.glb' is not under models/"),
        ] {
            assert!(has(&found, level, file, path, needle), "missing {level:?} {file} {path} {needle:?} in\n{all}");
        }
        assert!(!all.contains("Ship_Ok"), "a ship that loads has no findings: {all}");
        assert!(!all.contains("no imported assets"), "{all}");
        assert!(!all.contains("'real'") && !all.contains("'Real'"), "names that exist are fine: {all}");
        assert!(!all.contains("'Island.glb'"), "{all}");
        let ship_m_errors = found.iter().filter(|f| f.level == Level::Error && f.file == m && f.message.contains("cannot load")).count();
        assert_eq!(ship_m_errors, 0, "the loader is asked only about a ship the other checks found no error in");
        std::fs::remove_dir_all(base).unwrap();
        std::fs::remove_dir_all(mod_dir).unwrap();
    }

    /// A base with `maps/m1`: an error (a ship class nobody has) and a warning (a scenery model that is not there).
    fn base_with_a_map(tag: &str) -> PathBuf {
        let base = scratch(tag);
        put(&base, "ships/readme.txt", "x");
        put(&base, "models/scenery/Island.glb", "x");
        put(&base, "sound/ship/real.ogg", "x");
        put_json(&base, "maps/m1/mission.json", json!({"players": ["P"], "ships": [{"id": 1, "type": "Ship_Ghost", "group": "P", "pos": [0, 0, 0]}], "rules": []}));
        put_json(&base, "maps/m1/map.json", json!({"objects": [{"type": "Isle", "scene": "Isel", "pos": [0, 0, 0]}]}));
        put_json(&base, "maps/m1/media.json", json!({}));
        base
    }

    #[test]
    fn warnings_the_base_game_has_for_the_same_map_are_inherited_and_its_errors_and_the_mods_own_warnings_are_not() {
        let (base, mod_dir) = (base_with_a_map("inherit"), scratch("inherit-mod"));
        // the mod keeps the retail object and adds one more scenery model that is not there
        put_json(&mod_dir, "maps/m1/map.json", json!({"objects": [{"type": "Isle", "scene": "Isel", "pos": [0, 0, 0]}, {"type": "Rock", "scene": "Rok", "pos": [5, 5, 0]}]}));
        let found = check_mod_with(&mod_dir, Some(&base), &[]);
        let all = lines(&found).join("\n");
        let map = "maps/m1/map.json";
        let at = |file: &str, needle: &str| found.iter().filter(|f| f.file == file && f.message.contains(needle)).collect::<Vec<_>>();
        let retail = at(map, "'Isel.glb'");
        assert_eq!(retail.len(), 1, "{all}");
        assert!(retail[0].inherited, "the retail warning is the base game's: {all}");
        let own = at(map, "'Rok.glb'");
        assert_eq!(own.len(), 1, "{all}");
        assert!(!own[0].inherited, "the mod's own warning is its own: {all}");
        let ghost = at("maps/m1/mission.json", "'Ship_Ghost'");
        assert!(ghost.iter().all(|f| f.level == Level::Error && !f.inherited) && !ghost.is_empty(), "errors always stay the mod's: {all}");
        assert!(found.iter().filter(|f| f.inherited).all(|f| f.level == Level::Warning), "{all}");
        // without a base nothing is inherited
        let alone = check_mod_with(&mod_dir, None, &[]);
        assert!(alone.iter().all(|f| !f.inherited), "{:?}", lines(&alone));
        std::fs::remove_dir_all(base).unwrap();
        std::fs::remove_dir_all(mod_dir).unwrap();
    }

    #[test]
    fn rule_warnings_match_by_rule_field_and_message_not_by_index_and_a_repeat_is_the_mods() {
        let rule_warning = |idx: usize, msg: &str| {
            let mut f = Finding::new(Level::Warning, "\"Spawn\"", format!("condition #{idx}"), "Group Name", msg);
            f.rule_index = Some(idx);
            f.with_file("maps/m1/mission.json")
        };
        let base = vec![rule_warning(2, "group 'A' is empty"), Finding::new(Level::Error, "\"Spawn\"", "rule", "", "bad").with_file("maps/m1/mission.json")];
        let mut found = vec![rule_warning(5, "group 'A' is empty"), rule_warning(6, "group 'A' is empty"), rule_warning(7, "group 'B' is empty"), base[1].clone()];
        mark_inherited(&mut found, &base);
        let flags: Vec<bool> = found.iter().map(|f| f.inherited).collect();
        assert_eq!(flags, [true, false, false, false], "the shifted index still matches once; an error is never marked");
    }

    #[test]
    fn a_map_the_mod_leaves_as_the_base_has_it_is_not_checked_at_all() {
        let (base, mod_dir) = (base_with_a_map("same"), scratch("same-mod"));
        put(&mod_dir, "maps/m1/map.json", std::fs::read(base.join("maps/m1/map.json")).unwrap());
        let found = check_mod_with(&mod_dir, Some(&base), &[]);
        assert!(found.iter().all(|f| !f.file.starts_with("maps/")), "{:?}", lines(&found));
        std::fs::remove_dir_all(base).unwrap();
        std::fs::remove_dir_all(mod_dir).unwrap();
    }

    #[test]
    fn a_new_ship_must_state_what_the_loader_would_default_and_one_that_extends_or_changes_a_retail_ship_need_not() {
        let (base, mod_dir) = (scratch("newship"), scratch("newship-mod"));
        put_json(&base, "data/weapons/guns.json", json!({"Gun": gun()}));
        put_json(&base, "data/weapons/bullets.json", json!({"B": bullet()}));
        put(&base, "ships/readme.txt", "x");
        put_json(&base, "ships/Ship_Retail/ship.json", new_ship(json!({})));
        for id in ["Ship_Empty", "Ship_Extends", "Ship_Retail", "Ship_NoRace", "Ship_Full"] {
            put(&mod_dir, &format!("ships/{id}/model.glb"), small_model(false));
        }
        // the tester's file: a brand-new ship that says only race "", points and skirmish
        put_json(&mod_dir, "ships/Ship_Empty/ship.json", json!({"race": "", "points": 0, "skirmish": false}));
        put_json(&mod_dir, "ships/Ship_Extends/ship.json", json!({"extends": "Ship_Retail", "points": 100}));
        put_json(&mod_dir, "ships/Ship_Retail/ship.json", json!({"points": 7}));
        let mut no_race = new_ship(json!({}));
        no_race.as_object_mut().unwrap().remove("race");
        put_json(&mod_dir, "ships/Ship_NoRace/ship.json", no_race);
        put_json(&mod_dir, "ships/Ship_Full/ship.json", new_ship(json!({})));
        let found = check_mod_with(&mod_dir, Some(&base), &[]);
        let all = lines(&found).join("\n");
        let mut empty: Vec<&str> = found.iter().filter(|f| f.file == "ships/Ship_Empty/ship.json" && f.level == Level::Error).map(|f| f.pointer.as_str()).collect();
        empty.sort();
        let mut want = vec!["/race", "/core_hp", "/physics/Mass", "/physics/MaxThrust", "/physics/MaxSpeed", "/physics/RotationalFriction", "/physics/MaxAngularAcceleration"];
        want.sort();
        assert_eq!(empty, want, "{all}");
        assert!(has(&found, Level::Error, "ships/Ship_Empty/ship.json", "race", "`race` is empty"), "{all}");
        assert!(has(&found, Level::Error, "ships/Ship_Empty/ship.json", "core_hp", "a new ship must state `core_hp`"), "{all}");
        assert!(has(&found, Level::Error, "ships/Ship_NoRace/ship.json", "race", "a new ship must state `race`"), "{all}");
        for ok in ["Ship_Extends", "Ship_Retail", "Ship_Full"] {
            assert!(!found.iter().any(|f| f.file == format!("ships/{ok}/ship.json") && f.level == Level::Error), "{ok}: {all}");
        }
        assert_eq!(crate::authoring::required_new("ships/X/ship.json"), crate::data::SHIP_REQUIRED_NEW);
        std::fs::remove_dir_all(base).unwrap();
        std::fs::remove_dir_all(mod_dir).unwrap();
    }

    #[test]
    fn ship_errors_show_while_the_weapon_tables_fail() {
        let (base, mod_dir) = (scratch("both"), scratch("both-mod"));
        put_json(&base, "data/weapons/guns.json", json!({"Gun": gun()}));
        put_json(&base, "data/weapons/bullets.json", json!({"B": bullet()}));
        put(&base, "ships/readme.txt", "x");
        put(&mod_dir, "ships/Ship_A/model.glb", small_model(false));
        put_json(&mod_dir, "ships/Ship_A/ship.json", new_ship(json!({"guns": {"Mount_1": {"gun": "Gun", "pos": [0, 0, 0], "rot": [0, 0, 0]}}})));
        put_json(&mod_dir, "data/weapons/bullets.json", json!({"B_New": {"lifetime": 1}}));
        let found = check_mod_with(&mod_dir, Some(&base), &[]);
        let all = lines(&found).join("\n");
        assert!(found.iter().any(|f| f.level == Level::Error && f.file == "data/weapons/bullets.json" && f.pointer == "/B_New/damage"), "{all}");
        assert!(has(&found, Level::Error, "ships/Ship_A/ship.json", "", "arc_deg"), "the ship's own error shows with the bullets: {all}");
        assert!(!all.contains("unknown weapon"), "{all}");
        std::fs::remove_dir_all(base).unwrap();
        std::fs::remove_dir_all(mod_dir).unwrap();
    }

    #[test]
    fn a_damaged_variant_of_a_section_that_is_not_swappable_is_never_shown() {
        let mod_dir = scratch("swappable");
        // The model's `HullDamage_Two` stands in for a section of layer `Hull_Two`.
        let section = |swappable: bool| json!({"Two": {"layer": "Hull_Two", "material": "Wood", "hp": 1, "core_pct": 1, "vital_ship": true, "vital_speed": false, "vital_turn": false, "swappable": swappable}});
        put(&mod_dir, "ships/Ship_W/model.glb", small_model(true));
        put_json(&mod_dir, "ships/Ship_W/ship.json", json!({"sections": section(true)}));
        let warned = |found: &[Finding]| has(found, Level::Warning, "ships/Ship_W/model.glb", "HullDamage_Two", "no swappable section");
        assert!(!warned(&check_mod_with(&mod_dir, None, &[])), "a swappable section shows it");
        put_json(&mod_dir, "ships/Ship_W/ship.json", json!({"sections": section(false)}));
        let found = check_mod_with(&mod_dir, None, &[]);
        assert!(warned(&found), "{:?}", lines(&found));
        std::fs::remove_dir_all(mod_dir).unwrap();
    }

    #[test]
    fn a_folder_that_is_not_there_is_one_error() {
        let found = check_mod(Path::new("/nonexistent/procyon/mod"), None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].level, Level::Error);
    }

    #[test]
    fn the_retail_style_mods_of_the_project_have_no_errors_without_assets() {
        let mods = Path::new(env!("CARGO_MANIFEST_DIR")).join("../mods");
        for entry in std::fs::read_dir(&mods).unwrap().flatten().filter(|e| e.path().is_dir()) {
            let found = check_mod_with(&entry.path(), None, &[]);
            let errors: Vec<_> = lines(&found).into_iter().zip(&found).filter(|(_, f)| f.level == Level::Error).map(|(l, _)| l).collect();
            assert!(errors.is_empty(), "{}: {errors:#?}", entry.path().display());
        }
    }
}
