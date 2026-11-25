//! Mods: `--mod <dir>` (repeatable) overlays a folder onto the base assets. The merged tree is built once per launch under
//! `<assets>/../local/overlay` as symlinks (hard links, or copies, off Unix) so every loader, Bevy's included, sees a single root.
//! Rules: a mod file replaces the base file of the same path (a new `ships/<id>/` or `maps/<id>/` folder is new content);
//! a JSON with `"extends": "Ship_Navy_Cutter"` is deep-merged over the same file of that base ship/map, so a new ship is a few lines.
//! Not retail: a mod's `textures/<retail texture name>.png` swaps that texture in every model of the overlay that uses it; only those
//! models are rewritten (`retexture_models`), the rest stay links.
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

pub fn walk(dir: &Path, rel: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let r = rel.join(e.file_name());
        if e.file_type().is_ok_and(|t| t.is_dir()) { walk(&e.path(), &r, out) } else { out.push(r) }
    }
}

fn link(from: &Path, to: &Path) {
    let from = std::fs::canonicalize(from).expect("canonical asset path");
    let _ = std::fs::remove_file(to);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&from, to).expect("symlink");
    // A hard link costs nothing (symlinks need a privilege on Windows); a copy when the overlay is on another volume.
    // Safe because the overlay never writes through a file: merged ones are removed first and written anew.
    #[cfg(not(unix))]
    if std::fs::hard_link(&from, to).is_err() {
        std::fs::copy(&from, to).expect("copy");
    }
}

static ACTIVE: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();

/// The `--mod <folder>` arguments of a command line, in order. A missing value, a path that does not exist and a path that is a file
/// are errors naming the path: a mod that silently loads nothing is the worst way to fail.
pub fn mod_args(args: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut mods = vec![];
    for i in (1..args.len()).filter(|&i| args[i] == "--mod") {
        let Some(path) = args.get(i + 1).filter(|p| !p.starts_with("--")) else {
            return Err("--mod takes the mod's folder".into());
        };
        let dir = PathBuf::from(path);
        if dir.is_file() {
            return Err(format!("--mod {path}: that is a file, not a mod folder"));
        }
        if !dir.is_dir() {
            return Err(format!(
                "--mod {path}: no such folder (a relative path is read from {})",
                std::env::current_dir().map_or_else(|_| ".".into(), |d| d.display().to_string())
            ));
        }
        mods.push(dir);
    }
    Ok(mods)
}

/// `--only-mod`: this launch loads the `--mod` folders and none of the player's installed mods (a mod editor play-testing one mod
/// must not see the others). The bundled `content/` overlay is the engine's own and always applies.
pub const ONLY_MOD: &str = "--only-mod";

/// The mods of this launch: `(installed mods to check `requires` against, mod folders to overlay in order)`. Normally the switched-on
/// installed mods (their `mods.json` order) and then the `--mod` folders; with `--only-mod` the installed list is dropped and only the
/// `--mod` folders remain. Pure: it never reads or writes `mods.json`, which belongs to the player. `--only-mod` without a `--mod`
/// is an error (a mistyped launch must not quietly run the bare game).
pub fn pick_mods(args: &[String], installed: Vec<Installed>, folders: Vec<PathBuf>) -> Result<(Vec<Installed>, Vec<PathBuf>), String> {
    if !args.iter().skip(1).any(|a| a == ONLY_MOD) {
        let dirs = installed.iter().filter(|m| m.enabled).map(|m| m.dir.clone()).chain(folders).collect();
        return Ok((installed, dirs));
    }
    if folders.is_empty() {
        return Err(format!("{ONLY_MOD} needs at least one --mod <folder>"));
    }
    Ok((vec![], folders))
}

/// Records this launch's mod folders (switched-on installed mods, then `--mod` folders, in overlay order). Set once, at startup.
pub fn set_active(dirs: Vec<PathBuf>) {
    let _ = ACTIVE.set(dirs);
}

/// The mod folders overlaid in this launch (empty before `set_active`, as in unit tests).
pub fn active() -> &'static [PathBuf] {
    ACTIVE.get().map_or(&[], Vec::as_slice)
}

/// Overlay file (canonical path) -> (the mod folder, the mod's own file) that provided it. A merged or extended file is a copy in the
/// overlay, so its path says nothing about where it came from; errors name the mod's file instead (`origin_of`).
static ORIGINS: std::sync::Mutex<Vec<(PathBuf, PathBuf, PathBuf)>> = std::sync::Mutex::new(Vec::new());

/// The mod folder and the mod's own file behind a file of a built overlay; `None` for a base file or one no overlay built here knows.
pub(crate) fn origin_of(path: &Path) -> Option<(PathBuf, PathBuf)> {
    let key = std::fs::canonicalize(path).ok()?;
    ORIGINS.lock().ok()?.iter().find(|(k, _, _)| *k == key).map(|(_, dir, file)| (dir.clone(), file.clone()))
}

fn record_origin(overlay_file: &Path, dir: &Path, file: &Path) {
    let (Ok(key), Ok(mut all)) = (std::fs::canonicalize(overlay_file), ORIGINS.lock()) else { return };
    all.retain(|(k, _, _)| *k != key);
    // Canonical like the key, so errors name the same path on every platform (Windows adds `\\?\` and expands short names).
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    all.push((key, canonical(dir), canonical(file)));
}

/// Objects merge key by key, anything else is replaced.
pub fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                merge(b.entry(k.clone()).or_insert(Value::Null), v)
            }
        }
        (b, o) => *b = o.clone(),
    }
}

/// The slot a JSON pointer names in `v`, walking object keys only (a missing key is created as `null`); `None` when the path meets a
/// non-object or is not a pointer. Part of the keyed merge (`merge_keyed`).
fn object_slot<'a>(v: &'a mut Value, ptr: &str) -> Option<&'a mut Value> {
    let rest = ptr.strip_prefix('/')?;
    let mut at = v;
    for token in rest.split('/') {
        let key = token.replace("~1", "/").replace("~0", "~");
        at = at.as_object_mut()?.entry(key).or_insert(Value::Null);
    }
    Some(at)
}

/// The list at `ptr` in the diff, taken out of it (so the plain merge leaves it alone); `Ok(None)` when the diff has none there.
fn take_list(diff: &mut Value, ptr: &str) -> Result<Option<Vec<Value>>, String> {
    let Some((parent, last)) = ptr.rsplit_once('/').filter(|(_, last)| !last.is_empty()) else {
        return Err(format!("merges '{ptr}': not a JSON pointer to a key (like \"/objects\")"));
    };
    let last = last.replace("~1", "/").replace("~0", "~");
    let mut at = &mut *diff;
    for token in parent.split('/').skip(1) {
        let key = token.replace("~1", "/").replace("~0", "~");
        at = match at.as_object_mut().and_then(|o| o.get_mut(&key)) {
            Some(next) => next,
            None => return Ok(None),
        };
    }
    match at.as_object_mut().and_then(|o| o.remove(&last)) {
        None => Ok(None),
        Some(Value::Array(list)) => Ok(Some(list)),
        Some(_) => Err(format!("merges '{ptr}': the file's value there is not a list")),
    }
}

/// The text a key value is matched by: strings, numbers and booleans only.
fn key_text(v: Option<&Value>) -> Option<String> {
    v.filter(|v| matches!(v, Value::String(_) | Value::Number(_) | Value::Bool(_))).map(Value::to_string)
}

/// Keyed merge (the `$remove` spelling is PROVISIONAL, docs/modding.md "Changing one entry of a list"): `entries` are matched to the base list at
/// `ptr` by the field `field`. A matching entry is deep-merged (`merge`) into the base entry; one the base has not is appended, in
/// order; `{"<field>": k, "$remove": true}` drops the base entry (a missing one adds a line to `warnings`). Base entries that are not
/// mentioned keep their place. Errors (the whole file is then skipped, as for a bad `set`): the base is not a list, a base entry has
/// no key or two share one, a diff entry is no object or has no key.
pub(crate) fn merge_keyed(base: &mut Value, ptr: &str, field: &str, entries: &[Value], warnings: &mut Vec<String>) -> Result<(), String> {
    let err = |m: String| format!("merges '{ptr}': {m}");
    let slot = object_slot(base, ptr).ok_or_else(|| err("the file being extended has no object on that path".into()))?;
    if slot.is_null() {
        *slot = Value::Array(vec![]);
    }
    let Value::Array(list) = slot else { return Err(err("the file being extended has no list there".into())) };
    let mut items: Vec<Option<Value>> = std::mem::take(list).into_iter().map(Some).collect();
    let mut index: HashMap<String, usize> = HashMap::new();
    for (i, e) in items.iter().flatten().enumerate() {
        let Some(k) = key_text(e.get(field)) else {
            let first = e.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>().join(", ")).unwrap_or_default();
            return Err(err(format!("entry {i} of the list being extended has no '{field}' (unknown key field? its fields: {first})")));
        };
        if let Some(j) = index.insert(k.clone(), i) {
            return Err(err(format!("entries {j} and {i} of the list being extended have the same '{field}' ({k}), so they cannot be told apart")));
        }
    }
    for (n, entry) in entries.iter().enumerate() {
        let Some(obj) = entry.as_object() else { return Err(err(format!("entry {n} is not an object"))) };
        let Some(k) = key_text(obj.get(field)) else { return Err(err(format!("entry {n} has no '{field}'"))) };
        let remove = obj.get("$remove") == Some(&Value::Bool(true));
        let mut change = entry.clone();
        if let Some(o) = change.as_object_mut() {
            o.remove("$remove");
        }
        match (index.get(&k).copied(), remove) {
            (Some(i), true) => {
                items[i] = None;
                index.remove(&k);
            }
            (None, true) => warnings.push(format!("merges '{ptr}': removes {field} {k}, which is not in the list being extended")),
            (Some(i), false) => merge(items[i].as_mut().expect("indexed entries are present"), &change),
            (None, false) => {
                index.insert(k, items.len());
                items.push(Some(change));
            }
        }
    }
    if let Some(Value::Array(out)) = object_slot(base, ptr) {
        *out = items.into_iter().flatten().collect();
    }
    Ok(())
}

/// The lists of a map or mission file whose entries have a stable key, merged by that key by default (PROVISIONAL, each from what the
/// loader reads: docs/modding.md "Changing one entry of a list"). `name` is the file's path inside the assets. Lists of other files,
/// and every list not named here (a ship's `engines`, a rule's `actions`), replace whole as before.
pub(crate) fn keyed_defaults(name: &str) -> &'static [(&'static str, &'static str)] {
    let parts: Vec<&str> = name.split('/').collect();
    match parts[..] {
        ["maps", _, "map.json"] => &[("/objects", "id"), ("/ships", "id"), ("/mines", "id"), ("/nebulae", "name"), ("/players", "name")],
        ["maps", _, "mission.json"] => &[("/ships", "id"), ("/rules", "name"), ("/points", "Name")],
        ["maps", _, "script.json"] => &[("/rules", "name")],
        _ => &[],
    }
}

/// What an `extends` file does to its parent: `diff` merged over `parent` (objects deep, everything else replaced), except the lists
/// of `keyed_defaults(file)` and those named by `"merge": {"<pointer>": "<key field>"}`, which merge by key (`merge_keyed`); then
/// `"set"`. `"merge": {"<pointer>": false}` makes a default list replace whole instead. `extends`, `merge` and `set` are dropped from
/// the result. Order: the whole merge (keyed lists included) first, then `set`, so a `set` pointer counts the entries after the keyed
/// merge. Returns the result and the warnings.
pub(crate) fn extend(mut parent: Value, diff: &Value, file: &str) -> Result<(Value, Vec<String>), String> {
    let mut diff = diff.clone();
    // A `merge` that is not an object is not this feature: it stays data.
    let explicit = match diff.as_object_mut().filter(|o| o.get("merge").is_some_and(Value::is_object)).and_then(|o| o.remove("merge")) {
        Some(Value::Object(m)) => m,
        _ => Default::default(),
    };
    let mut lists = vec![];
    for (ptr, field) in keyed_defaults(file) {
        if !explicit.contains_key(*ptr) && diff.pointer(ptr).is_some_and(Value::is_array) {
            let list = take_list(&mut diff, ptr)?.unwrap_or_default();
            lists.push((ptr.to_string(), field.to_string(), list));
        }
    }
    for (ptr, field) in &explicit {
        if *field == Value::Bool(false) {
            continue;
        }
        let field =
            field.as_str().filter(|f| !f.is_empty()).ok_or_else(|| format!("merges '{ptr}': the key field must be a field name (a string), or false to replace the list"))?;
        match take_list(&mut diff, ptr)? {
            Some(list) => lists.push((ptr.clone(), field.to_string(), list)),
            None => return Err(format!("merges '{ptr}': the file has no list there")),
        }
    }
    merge(&mut parent, &diff);
    let obj = parent.as_object_mut().ok_or("the merged file is not a JSON object")?;
    obj.remove("extends");
    let set = obj.remove("set");
    let mut warnings = vec![];
    for (ptr, field, list) in lists {
        merge_keyed(&mut parent, &ptr, &field, &list, &mut warnings)?;
    }
    if let Some(Value::Object(set)) = set {
        for (ptr, value) in set {
            match parent.pointer_mut(&ptr) {
                Some(slot) => *slot = value,
                None => return Err(format!("sets '{ptr}', which does not exist")),
            }
        }
    }
    Ok((parent, warnings))
}

/// The file-format version of `mod.json` and of the mod layout this engine reads. A `mod.json` without `format` is version 1.
pub const MOD_FORMAT: u32 = 1;

/// `mod.json` (docs/modding.md "Sharing a mod"). Every field is read leniently: a value of the wrong type counts as absent, and
/// unknown keys are allowed and ignored, so a hand-written file never stops a mod from loading.
#[derive(Clone, Debug, Default, serde::Deserialize, schemars::JsonSchema)]
#[schemars(title = "mod.json")]
pub struct ModJson {
    /// Editor hint: the schema this file follows. Ignored by the loader.
    #[serde(rename = "$schema", default, deserialize_with = "crate::data::lenient")]
    pub schema: Option<String>,
    /// The mod's id, which `mods.json` and other mods' `requires` use. Default: the folder name.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub id: Option<String>,
    /// The name shown in the Mods screen. Default: the id.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub name: Option<String>,
    /// The mod's own version, shown in the Mods screen.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub version: Option<String>,
    /// A line shown in the Mods screen.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub description: Option<String>,
    /// The Mods screen's filter (`Ships`, `Visuals`, ...). Default `Other`.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub category: Option<String>,
    /// The order of a mod campaign's missions (map ids); missions not listed follow, by id.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub missions: Option<Vec<String>>,
    /// Bundled mods only: on until the player switches it off. Ignored in user mods.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub default: Option<bool>,
    /// The mod file-format version (an integer). The engine reads format 1; absent means 1.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub format: Option<u32>,
    /// The oldest engine version the mod was made for, as `"x.y.z"`. A newer requirement logs a warning naming the mod; the mod still loads.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub game_version: Option<String>,
    /// Ids of mods this one needs. A missing or switched-off one logs a warning naming both; the mod still loads.
    #[serde(default, deserialize_with = "crate::data::lenient")]
    pub requires: Option<Vec<String>>,
}

/// `x.y.z` (missing parts are 0, a leading `v` and anything after `-` or `+` are ignored) as numbers; `None` when it is not a version.
pub fn parse_version(text: &str) -> Option<[u64; 3]> {
    let core = text.trim().trim_start_matches('v').split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let mut out = [0; 3];
    for slot in &mut out {
        match parts.next() {
            Some(p) => *slot = p.trim().parse().ok()?,
            None => break,
        }
    }
    parts.next().is_none().then_some(out)
}

/// The schema of `mod.json`.
pub fn schema() -> Value {
    serde_json::to_value(schemars::SchemaGenerator::default().into_root_schema_for::<ModJson>()).expect("schema is JSON")
}

/// A mod folder found in `<user dir>/mods/`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// `mod.json` `category` ("Ships", "Visuals", ...); "Other" when absent.
    pub category: String,
    pub dir: PathBuf,
    pub enabled: bool,
    /// A bundled mod whose `mod.json` has `"default": true`: on until the player switches it off.
    pub default: bool,
    /// `mod.json` `format` (1 when absent).
    pub format: u32,
    /// `mod.json` `game_version`: the oldest engine version the mod asks for (empty when absent).
    pub game_version: String,
    /// `mod.json` `requires`: ids of the mods this one needs.
    pub requires: Vec<String>,
}

impl Installed {
    /// Warnings about this mod's `game_version` and `format` against this engine, and about `requires` against the other `mods`
    /// (only switched-on ones count). Nothing here stops a mod from loading.
    pub fn warnings(&self, mods: &[Installed], engine: &str) -> Vec<String> {
        let mut out = vec![];
        let label = format!("mod {:?}", self.id);
        if !self.game_version.is_empty() {
            match (parse_version(&self.game_version), parse_version(engine)) {
                (Some(want), Some(have)) if want > have => out.push(format!("{label} asks for OpenProcyon {} or newer; this is {engine}, so it may not work", self.game_version)),
                (None, _) => out.push(format!("{label}: game_version {:?} is not a version like \"0.2.0\"; ignored", self.game_version)),
                _ => {}
            }
        }
        if self.format > MOD_FORMAT {
            out.push(format!("{label} is mod format {}; this engine reads format {MOD_FORMAT}, so some of it may be ignored", self.format));
        }
        for need in &self.requires {
            match mods.iter().find(|m| m.id == *need) {
                None => out.push(format!("{label} requires mod {need:?}, which is not installed")),
                Some(m) if !m.enabled => out.push(format!("{label} requires mod {need:?}, which is installed but switched off")),
                Some(_) => {}
            }
        }
        out
    }
}

/// Mods are installed by unzipping into this folder; `mods.json` beside it (`{"enabled": ["id", ...], "disabled": ["id", ...]}`, load order,
/// later wins) lists the ones that are on. A mod never listed stays off, so dropping a folder in changes nothing until the player enables it;
/// only a bundled mod marked `"default": true` starts on, and `disabled` remembers that the player switched it off.
pub fn mods_dir() -> Option<PathBuf> {
    Some(crate::campaign::user_dir()?.join("mods"))
}

/// Every mod folder in `<user>/mods/` and in `bundled` (the `mods/` that ships with the game; a user folder of the same id wins), with
/// its `mod.json` (`id` defaults to the folder name, `name` to the id). Enabled ones first in `mods.json` order, the rest by id.
pub fn installed_in(user: &Path, bundled: Option<&Path>) -> Vec<Installed> {
    let saved = read(&user.join("mods.json")).unwrap_or_default();
    let list = |k: &str| -> Vec<String> { serde_json::from_value(saved[k].clone()).unwrap_or_default() };
    let (on, off) = (list("enabled"), list("disabled"));
    let mut found: Vec<Installed> = vec![];
    for (dir, is_bundled) in [(Some(user.join("mods")), false), (bundled.map(Path::to_path_buf), true)] {
        let Some(dir) = dir else { continue };
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.path().is_dir()) {
            let meta: ModJson = read(&e.path().join("mod.json")).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
            let text = |v: &Option<String>| v.clone().unwrap_or_default();
            let id = meta.id.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| e.file_name().to_string_lossy().into_owned());
            if found.iter().any(|m| m.id == id) {
                continue;
            }
            let name = meta.name.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| id.clone());
            let category = meta.category.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "Other".into());
            let default = is_bundled && meta.default == Some(true);
            let enabled = on.contains(&id) || (default && !off.contains(&id));
            found.push(Installed {
                enabled,
                default,
                id,
                name,
                version: text(&meta.version),
                description: text(&meta.description),
                category,
                dir: e.path(),
                format: meta.format.unwrap_or(MOD_FORMAT),
                game_version: text(&meta.game_version),
                requires: meta.requires.clone().unwrap_or_default(),
            });
        }
    }
    found.sort_by_key(|m| (on.iter().position(|i| *i == m.id).unwrap_or(usize::MAX), m.id.clone()));
    found
}

/// Prints the `game_version` / `format` / `requires` warnings of the switched-on mods (`Installed::warnings`) to stderr, one line each.
/// `folders` are the `--mod` folders: they count as installed and on for `requires` (by folder name, or the `id` of their `mod.json`).
pub fn log_warnings(all: &[Installed], folders: &[PathBuf]) {
    let mut every = all.to_vec();
    for dir in folders {
        let meta: ModJson = read(&dir.join("mod.json")).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        let id = meta.id.filter(|s| !s.is_empty()).unwrap_or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        if !every.iter().any(|m| m.id == id && m.enabled) {
            every.push(Installed {
                enabled: true,
                id,
                format: meta.format.unwrap_or(MOD_FORMAT),
                game_version: meta.game_version.unwrap_or_default(),
                requires: meta.requires.unwrap_or_default(),
                ..Installed::default()
            });
        }
    }
    for m in every.iter().filter(|m| m.enabled) {
        for w in m.warnings(&every, crate::authoring::ENGINE_VERSION) {
            eprintln!("warning: {w}");
        }
    }
}

pub fn installed(bundled: Option<&Path>) -> Vec<Installed> {
    crate::campaign::user_dir().map(|d| installed_in(&d, bundled)).unwrap_or_else(|| installed_in(Path::new(""), bundled))
}

/// Writes `mods.json` from the list's enabled mods, in list order, and the default mods switched off.
pub fn save_enabled_in(dir: &Path, mods: &[Installed]) {
    let ids = |keep: fn(&Installed) -> bool| -> Vec<&str> { mods.iter().filter(|m| keep(m)).map(|m| m.id.as_str()).collect() };
    let (on, off) = (ids(|m| m.enabled), ids(|m| m.default && !m.enabled));
    let _ = std::fs::create_dir_all(dir);
    let json = serde_json::json!({ "enabled": on, "disabled": off });
    if let Err(e) = std::fs::write(dir.join("mods.json"), serde_json::to_string_pretty(&json).unwrap_or_default()) {
        eprintln!("could not save mods.json: {e}");
    }
}

/// Object files that mods add keys to instead of replacing: the one list (`render/mod.rs` reads the two render files it names,
/// `docs/modding.md` lists them all; a test keeps the doc in step).
pub const MERGED: &[&str] = &[
    "hud/scales.json",
    "data/collision.json",
    "data/docks.json",
    "data/crew_nodes.json",
    "data/weapons/guns.json",
    "data/weapons/bullets.json",
    "data/ai/weapons.json",
    "menu/strings.json",
    crate::render::SKIES,
    crate::render::NEBULAE,
];

fn read(p: &Path) -> Option<Value> {
    crate::data::try_json(p)
}

/// What one mod file did to the overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Applied {
    /// `mod.json`: metadata only.
    Meta,
    /// A plain file put in place: an override when the overlay already had one, else a new file (the caller tells which).
    Placed,
    /// Keys merged into a `MERGED` object file.
    Merged,
    /// Deep-merged over its `extends` parent.
    Extends,
}

/// One mod layer's part in the overlay: counts per category come from the lists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModEntry {
    pub name: String,
    pub version: String,
    pub dir: PathBuf,
    pub overrides: Vec<String>,
    pub merged: Vec<String>,
    pub extends: Vec<String>,
    pub added: Vec<String>,
    /// `textures/<name>.png` files: retail textures this mod replaces by name (not placed as files; they are written into models).
    pub textures: Vec<String>,
    /// The `textures` files that won over earlier mods' but that no model of the overlay uses (a typo, as a rule).
    pub unused_textures: Vec<String>,
    /// (file, why): the same error text the loader reports, so a mod author sees one wording.
    pub skipped: Vec<(String, String)>,
    /// (file, why): a file that applied with something to look at (a keyed `$remove` of an entry the list has not).
    pub warnings: Vec<(String, String)>,
}

/// What `overlay` did, as data: which mods loaded and what each did to which file. `log` prints it; a mod screen can read it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModReport {
    pub mods: Vec<ModEntry>,
}

/// Files listed per category before "+K more".
const REPORT_LIST: usize = 8;

impl ModReport {
    /// Drops the entry for `dir` (the game's own `content/` layer) unless it skipped something, which the player or developer must see.
    pub fn without(mut self, dir: &Path) -> Self {
        self.mods.retain(|m| m.dir != dir || !m.skipped.is_empty() || !m.warnings.is_empty() || !m.unused_textures.is_empty());
        self
    }

    /// The report as log lines: one short line with no mods, else a block per mod with counts and up to `REPORT_LIST` files per category.
    pub fn lines(&self) -> Vec<String> {
        if self.mods.is_empty() {
            return vec!["mods: none (vanilla)".into()];
        }
        let list = |names: &[String]| -> String {
            let mut s = names.iter().take(REPORT_LIST).cloned().collect::<Vec<_>>().join(", ");
            if names.len() > REPORT_LIST {
                s += &format!(", +{} more", names.len() - REPORT_LIST);
            }
            s
        };
        let mut out = vec![format!("mods: {} loaded", self.mods.len())];
        for m in &self.mods {
            let version = if m.version.is_empty() { String::new() } else { format!(" {}", m.version) };
            out.push(format!("  {}{version} ({})", m.name, m.dir.display()));
            for (label, files) in [("overrides", &m.overrides), ("merges", &m.merged), ("extends", &m.extends), ("adds", &m.added), ("texture overrides", &m.textures)] {
                if !files.is_empty() {
                    out.push(format!("    {label} {}: {}", files.len(), list(files)));
                }
            }
            if !m.unused_textures.is_empty() {
                out.push(format!("    warning: no model uses {} texture override(s), check the names: {}", m.unused_textures.len(), list(&m.unused_textures)));
            }
            for (f, why) in &m.warnings {
                out.push(format!("    warning: {f}: {why}"));
            }
            if !m.skipped.is_empty() {
                out.push(format!("    skipped {}:", m.skipped.len()));
                for (f, why) in m.skipped.iter().take(REPORT_LIST) {
                    out.push(format!("      {f}: {why}"));
                }
                if m.skipped.len() > REPORT_LIST {
                    out.push(format!("      +{} more", m.skipped.len() - REPORT_LIST));
                }
            }
        }
        out
    }

    pub fn log(&self) {
        for l in self.lines() {
            eprintln!("{l}");
        }
    }
}

/// Marks a `PROCYON_OVERLAY` folder as one the game built, and so may delete and rebuild.
const OVERLAY_MARK: &str = ".openprocyon-overlay";

/// True when the game may delete `dir` to rebuild an overlay there: missing, empty, or marked as an overlay it built earlier.
fn disposable(dir: &Path) -> bool {
    dir.join(OVERLAY_MARK).exists() || std::fs::read_dir(dir).map_or(true, |mut d| d.next().is_none())
}

/// Builds the overlay and returns its root; with no mods returns `base` unchanged. `own`: a folder of the launch's own
/// (PROCYON_OVERLAY, read by the caller: the tools give each checkout and each parallel run one, so runs sharing an import do not
/// rebuild each other's tree); else `<base>/../local/overlay`.
pub fn overlay(base: &Path, mods: &[PathBuf], own: Option<PathBuf>) -> PathBuf {
    let (root, report) = overlay_report(base, mods, own);
    report.log();
    root
}

/// `overlay` that returns what it did instead of printing it.
pub fn overlay_report(base: &Path, mods: &[PathBuf], own: Option<PathBuf>) -> (PathBuf, ModReport) {
    if mods.is_empty() {
        return (base.to_path_buf(), ModReport::default());
    }
    if let Some(dir) = &own {
        assert!(disposable(dir), "PROCYON_OVERLAY {} holds files that are not an overlay; refusing to delete it", dir.display());
    }
    let root = own.clone().unwrap_or_else(|| base.join("../local/overlay"));
    let report = build_overlay(base, mods, &root, own.is_some());
    (root, report)
}

/// Builds the overlay of `mods` over `base` into `out`, a folder the caller chose (a tool's own scratch folder, so it never touches the
/// game's shared `local/overlay`, #136). `out` is rebuilt whole; it must be missing, empty or an overlay built earlier, else an error
/// and nothing is deleted. With no mods nothing is built (the base is the root). See `authoring::overlay_into`.
pub fn overlay_into(base: &Path, mods: &[PathBuf], out: &Path) -> Result<ModReport, String> {
    if mods.is_empty() {
        return Ok(ModReport::default());
    }
    if !disposable(out) {
        return Err(format!("{} holds files that are not an overlay; refusing to delete it", out.display()));
    }
    Ok(build_overlay(base, mods, out, true))
}

/// The shared core: replaces `root` with base links plus the mods' files. `marked` writes the overlay mark (a folder the caller chose).
fn build_overlay(base: &Path, mods: &[PathBuf], root: &Path, marked: bool) -> ModReport {
    let mut report = ModReport::default();
    if let (Ok(old), Ok(mut all)) = (std::fs::canonicalize(root), ORIGINS.lock()) {
        all.retain(|(k, _, _)| !k.starts_with(&old));
    }
    let _ = std::fs::remove_dir_all(root);
    if marked {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join(OVERLAY_MARK), "").unwrap();
    }
    let mut files = vec![];
    walk(base, Path::new(""), &mut files);
    for rel in &files {
        let to = root.join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        link(&base.join(rel), &to);
    }
    // Retail texture key -> the PNG bytes, and who gave them (index of the mod in the report, its file); a later mod replaces an earlier one.
    let mut images: HashMap<String, Vec<u8>> = HashMap::new();
    let mut owners: HashMap<String, (usize, String)> = HashMap::new();
    let mut models: BTreeSet<PathBuf> = files.iter().filter(|r| is_model(r)).cloned().collect();
    for m in mods {
        let modname = m.file_name().map_or_else(|| m.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
        let mut entry = ModEntry { name: modname.clone(), dir: m.clone(), ..Default::default() };
        let mut mf = vec![];
        walk(m, Path::new(""), &mut mf);
        // A JSON file that does not parse is skipped with one error line naming file and mod (the base file stays); the
        // rest of the mod still loads. Parsed once here: extends detection and the merge below reuse it.
        let mut docs: HashMap<PathBuf, Value> = HashMap::new();
        mf.retain(|rel| {
            if rel.extension().is_none_or(|e| e != "json") {
                return true;
            }
            match crate::data::load_as::<Value>(&m.join(rel), Some(&modname)) {
                Ok(v) => {
                    docs.insert(rel.clone(), v);
                    true
                }
                Err(e) => {
                    entry.skipped.push((rel.to_string_lossy().replace('\\', "/"), e.to_string()));
                    false
                }
            }
        });
        // Files that extend another go last (stable sort over sorted names), so they see sibling mod files, not only the base.
        // shortcut: one level; a chain of extends inside one mod needs a topological order.
        mf.sort();
        mf.retain(|rel| {
            let Some(k) = texture_override(rel) else { return true };
            let name = rel.to_string_lossy().replace('\\', "/");
            match std::fs::read(m.join(rel)) {
                Ok(png) if png.starts_with(b"\x89PNG\r\n\x1a\n") && png.len() > 26 => {
                    entry.textures.push(name.clone());
                    owners.insert(k.clone(), (report.mods.len(), name));
                    images.insert(k, png);
                }
                Ok(_) => entry.skipped.push((name, "not a PNG picture".into())),
                Err(e) => entry.skipped.push((name, format!("cannot read: {e}"))),
            }
            false
        });
        models.extend(mf.iter().filter(|r| is_model(r)).cloned());
        mf.sort_by_key(|r| docs.get(r).is_some_and(|v| v.get("extends").is_some()));
        for rel in mf {
            let (from, to) = (m.join(&rel), root.join(&rel));
            let name = rel.to_string_lossy().replace('\\', "/");
            let existed = to.exists();
            let mut warned = vec![];
            let applied = apply_file(&name, &from, &to, base, docs.get(&rel), &mut warned);
            entry.warnings.extend(warned.into_iter().map(|w| (name.clone(), w)));
            if applied.as_ref().is_ok_and(|a| *a != Applied::Meta) {
                record_origin(&to, m, &from);
            }
            match applied {
                Ok(Applied::Meta) => {
                    let meta = docs.get(&rel);
                    let text = |k: &str| meta.and_then(|v| v[k].as_str()).filter(|s| !s.is_empty()).map(str::to_string);
                    entry.name = text("name").unwrap_or(entry.name);
                    entry.version = text("version").unwrap_or_default();
                }
                Ok(Applied::Merged) => entry.merged.push(name),
                Ok(Applied::Extends) => entry.extends.push(name),
                Ok(Applied::Placed) if existed => entry.overrides.push(name),
                Ok(Applied::Placed) => entry.added.push(name),
                Err(e) => entry.skipped.push((name, e)),
            }
        }
        report.mods.push(entry);
    }
    retexture_models(root, &models, &images, &owners, &mut report);
    // Ships the mods bring get the UI art their model implies, unless they ship their own (shipart.rs).
    let mut ids: Vec<String> =
        mods.iter().flat_map(|m| std::fs::read_dir(m.join("ships")).into_iter().flatten().flatten()).filter_map(|e| e.file_name().into_string().ok()).collect();
    ids.sort();
    ids.dedup();
    crate::shipart::generate(root, &ids);
    report
}

/// A model file the engine loads textures for: the overlay's `ships/` and `models/` GLBs.
fn is_model(rel: &Path) -> bool {
    rel.extension().is_some_and(|e| e == "glb") && (rel.starts_with("ships") || rel.starts_with("models"))
}

/// The retail texture key a mod file `textures/<name>.png` stands for (any case of name and extension, `.tga` optional); `None` for any
/// other path, `textures/surface/...` (baked surface maps) included.
fn texture_override(rel: &Path) -> Option<String> {
    let mut parts = rel.components();
    let (dir, file) = (parts.next()?, parts.next()?);
    let file = Path::new(file.as_os_str());
    if parts.next().is_some() || dir.as_os_str() != "textures" || !file.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) {
        return None;
    }
    Some(crate::retail_tex::key(&file.file_stem()?.to_string_lossy()))
}

/// Not retail: writes, in place of the link, a copy of each model in `models` that uses a texture in `textures`, with that texture
/// replaced (retail_tex.rs). A model that uses none stays a link, and finding that out reads only its JSON chunk. A texture no model
/// uses is reported on the mod whose file won.
fn retexture_models(root: &Path, models: &BTreeSet<PathBuf>, images: &HashMap<String, Vec<u8>>, owners: &HashMap<String, (usize, String)>, report: &mut ModReport) {
    if images.is_empty() {
        return;
    }
    let mut used: BTreeSet<String> = BTreeSet::new();
    for rel in models {
        let path = root.join(rel);
        if !crate::retail_tex::uses_any(&path, images) {
            continue;
        }
        let Some((bytes, keys)) = std::fs::read(&path).ok().and_then(|b| crate::retail_tex::retexture(&b, images)) else { continue };
        let _ = std::fs::remove_file(&path);
        if let Err(e) = std::fs::write(&path, bytes) {
            eprintln!("mod textures: cannot write {}: {e}", rel.display());
        }
        used.extend(keys);
    }
    let mut unused: Vec<_> = owners.iter().filter(|(k, _)| !used.contains(*k)).map(|(_, (i, name))| (*i, name.clone())).collect();
    unused.sort();
    for (i, name) in unused {
        report.mods[i].unused_textures.push(name);
    }
}

/// How a mod file combines with what the layers below it left at its path (docs/modding.md "What a mod can change").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeRule {
    /// `mod.json`: read for the mod's details, never placed.
    Meta,
    /// One of [`MERGED`]: objects combine key by key, arrays and scalars replace.
    Merged,
    /// A `.json` file with `"extends"`: merged over its parent ([`extends_parent`]), then `set` pointers; `extends` and `set` dropped.
    Extends,
    /// `textures/<retail texture name>.png`: swapped into every model that uses that texture, not placed as a file.
    Texture,
    /// Anything else: the file replaces the one below it whole.
    Replaced,
}

/// The rule for the mod file `rel` (relative to the mod folder, `/` separators). `doc` is the file's JSON when it is a `.json` file
/// (whether it holds `"extends"` decides between [`MergeRule::Extends`] and [`MergeRule::Replaced`]); `None` reads as no `extends`.
/// The same decision the overlay makes when it lays the file down.
pub fn merge_rule(rel: &str, doc: Option<&Value>) -> MergeRule {
    if rel == "mod.json" {
        MergeRule::Meta
    } else if MERGED.contains(&rel) {
        MergeRule::Merged
    } else if rel.ends_with(".json") && doc.is_some_and(|v| v.get("extends").is_some()) {
        MergeRule::Extends
    } else if texture_override(Path::new(rel)).is_some() {
        MergeRule::Texture
    } else {
        MergeRule::Replaced
    }
}

/// The file (relative to the asset root, `/` separators) that `rel` with `"extends": parent` is laid over: for `ships/<id>/<file>`
/// and `maps/<id>/<file>` the same file of ship or map `parent`, for any other file the sibling `<parent>.json`. `None` when `rel` has
/// no file name.
pub fn extends_parent(rel: &str, parent: &str) -> Option<String> {
    let parts: Vec<&str> = rel.split('/').collect();
    match parts.as_slice() {
        [top @ ("ships" | "maps"), _, file] => Some(format!("{top}/{parent}/{file}")),
        [dirs @ .., file] if !file.is_empty() => Some(dirs.iter().map(|d| format!("{d}/")).collect::<String>() + &format!("{parent}.json")),
        _ => None,
    }
}

/// Puts one mod file into the overlay at `to`. An `Err` leaves the base file in place; the caller logs it and goes on.
/// `doc`: the file's parsed JSON, for a `.json` file. `warnings`: what went wrong without stopping the file (a keyed removal of an
/// entry the list has not).
fn apply_file(name: &str, from: &Path, to: &Path, base: &Path, doc: Option<&Value>, warnings: &mut Vec<String>) -> Result<Applied, String> {
    fn io(what: &'static str) -> impl Fn(std::io::Error) -> String {
        move |e| format!("{what}: {e}")
    }
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(io("cannot create the overlay folder"))?;
    }
    let write = |v: &Value| -> Result<(), String> {
        let text = serde_json::to_string(v).map_err(|e| format!("cannot write the merged file: {e}"))?;
        let _ = std::fs::remove_file(to);
        std::fs::write(to, text).map_err(io("cannot write the merged file"))
    };
    if name == "mod.json" {
        return Ok(Applied::Meta);
    }
    if MERGED.contains(&name) {
        // The file earlier layers left in the overlay; a missing one starts empty, an unreadable one is an error.
        let mut b = if to.exists() { crate::data::load_as::<Value>(to, None).map_err(|e| e.to_string())? } else { Value::Null };
        if !b.is_object() {
            b = Value::Object(Default::default())
        }
        merge(&mut b, doc.unwrap_or(&Value::Null));
        return write(&b).map(|()| Applied::Merged);
    }
    if let (true, Some(v)) = (name.ends_with(".json"), doc.filter(|v| v.get("extends").is_some())) {
        let parent = v["extends"].as_str().unwrap_or_default();
        // The overlay's root is `to` minus `name`; the parent is found there (`extends_parent` has the rule).
        let root = name.split('/').try_fold(to, |p, _| p.parent()).ok_or_else(|| format!("cannot place the file extended by '{parent}'"))?;
        let parent_path = extends_parent(name, parent).map(|rel| root.join(rel)).ok_or_else(|| format!("cannot place the file extended by '{parent}'"))?;
        if !parent_path.exists() {
            return Err(format!("extends unknown '{parent}' (no {})", parent_path.display()));
        }
        let b = crate::data::load_as::<Value>(&parent_path, None).map_err(|e| format!("extends '{parent}': {e}"))?;
        let (b, found) = extend(b, v, name)?;
        warnings.extend(found);
        return write(&b).map(|()| Applied::Extends);
    }
    if name.ends_with(".glb") && (name.starts_with("ships/") || name.starts_with("models/")) {
        // A model that names retail textures (`retail:<name>`) gets them from the player's import (retail_tex.rs).
        match std::fs::read(from).ok().and_then(|bytes| crate::retail_tex::bake(&bytes, base)) {
            Some(baked) => {
                let _ = std::fs::remove_file(to);
                std::fs::write(to, baked).map_err(io("cannot write the baked model"))?;
            }
            None => link(from, to),
        }
    } else {
        link(from, to);
    }
    Ok(Applied::Placed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_rule_and_extends_parent_name_the_overlays_own_decisions() {
        let ext = serde_json::json!({"extends": "P"});
        assert_eq!(merge_rule("mod.json", None), MergeRule::Meta);
        assert_eq!(merge_rule("data/weapons/guns.json", Some(&ext)), MergeRule::Merged, "a merged path merges even with extends");
        assert_eq!(merge_rule("ships/S/ship.json", Some(&ext)), MergeRule::Extends);
        assert_eq!(merge_rule("ships/S/ship.json", Some(&serde_json::json!({"points": 1}))), MergeRule::Replaced);
        assert_eq!(merge_rule("ships/S/ship.json", None), MergeRule::Replaced);
        assert_eq!(merge_rule("textures/Hull_Planks.PNG", None), MergeRule::Texture);
        assert_eq!(merge_rule("textures/surface/x.png", None), MergeRule::Replaced);
        assert_eq!(merge_rule("ships/S/model.glb", None), MergeRule::Replaced);
        for m in MERGED {
            assert_eq!(merge_rule(m, None), MergeRule::Merged, "{m}");
        }
        assert_eq!(extends_parent("ships/New/ship.json", "Ship_Old").as_deref(), Some("ships/Ship_Old/ship.json"));
        assert_eq!(extends_parent("maps/m2/map.json", "m1").as_deref(), Some("maps/m1/map.json"));
        assert_eq!(extends_parent("fx/Red_Engine.json", "Engine").as_deref(), Some("fx/Engine.json"));
        assert_eq!(extends_parent("data/render/look.json", "base").as_deref(), Some("data/render/base.json"));
        assert_eq!(extends_parent("top.json", "other").as_deref(), Some("other.json"));
        assert_eq!(extends_parent("ships/S/sub/x.json", "y").as_deref(), Some("ships/S/sub/y.json"), "only depth 3 is by id");
    }

    #[test]
    fn mod_json_new_keys_default_and_read_leniently() {
        let none: ModJson = serde_json::from_str("{}").unwrap();
        assert_eq!((none.format, none.game_version, none.requires), (None, None, None));
        let all: ModJson = serde_json::from_str(r#"{"$schema": "x", "format": 1, "game_version": "0.2.0", "requires": ["a", "b"], "future": 1}"#).unwrap();
        assert_eq!((all.format, all.game_version.as_deref(), all.requires), (Some(1), Some("0.2.0"), Some(vec!["a".into(), "b".into()])));
        let wrong: ModJson = serde_json::from_str(r#"{"format": "one", "game_version": 2, "requires": "a", "name": 5}"#).unwrap();
        assert_eq!((wrong.format, wrong.game_version, wrong.requires, wrong.name), (None, None, None, None), "wrong types count as absent");
        let dir = std::env::temp_dir().join(format!("procyon-modjson-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (f, meta) in [("plain", "{}"), ("new", r#"{"format": 2, "game_version": "9.0.0", "requires": ["plain", "ghost"]}"#)] {
            std::fs::create_dir_all(dir.join("mods").join(f)).unwrap();
            std::fs::write(dir.join("mods").join(f).join("mod.json"), meta).unwrap();
        }
        let list = installed_in(&dir, None);
        let by = |id: &str| list.iter().find(|m| m.id == id).unwrap().clone();
        assert_eq!((by("plain").format, by("plain").game_version.as_str(), by("plain").requires.len()), (1, "", 0));
        assert_eq!((by("new").format, by("new").game_version.as_str(), by("new").requires.clone()), (2, "9.0.0", vec!["plain".to_string(), "ghost".to_string()]));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn mod_arguments_must_name_folders() {
        let dir = std::env::temp_dir().join(format!("procyon-modargs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("m")).unwrap();
        std::fs::write(dir.join("f.json"), "{}").unwrap();
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (m, f, gone) = (dir.join("m").to_string_lossy().into_owned(), dir.join("f.json").to_string_lossy().into_owned(), dir.join("gone").to_string_lossy().into_owned());
        assert_eq!(mod_args(&args(&["game", "--map", "x"])).unwrap(), Vec::<PathBuf>::new());
        assert_eq!(mod_args(&args(&["game", "--mod", &m, "--mod", &m])).unwrap(), vec![dir.join("m"), dir.join("m")]);
        let error = mod_args(&args(&["game", "--mod", &m, "--mod", &gone])).unwrap_err();
        assert!(error.contains(&gone) && error.contains("no such folder"), "{error}");
        let error = mod_args(&args(&["game", "--mod", &f])).unwrap_err();
        assert!(error.contains(&f) && error.contains("file"), "{error}");
        assert!(mod_args(&args(&["game", "--mod"])).unwrap_err().contains("takes"));
        assert!(mod_args(&args(&["game", "--mod", "--map", "x"])).unwrap_err().contains("takes"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn versions_compare_as_numbers() {
        assert_eq!(parse_version("0.2.0"), Some([0, 2, 0]));
        assert_eq!(parse_version("v1.10"), Some([1, 10, 0]));
        assert_eq!(parse_version("0.3.1-beta+x"), Some([0, 3, 1]));
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("soon"), None);
        assert!(parse_version("0.10.0") > parse_version("0.9.9"), "not as text");
    }

    #[test]
    fn warnings_name_the_mod_for_a_newer_engine_requirement_and_missing_or_disabled_requirements() {
        let mod_ = |id: &str, enabled: bool, game_version: &str, requires: &[&str]| Installed {
            id: id.into(),
            enabled,
            format: 1,
            game_version: game_version.into(),
            requires: requires.iter().map(|s| s.to_string()).collect(),
            ..Installed::default()
        };
        let all = [
            mod_("base", true, "", &[]),
            mod_("off", false, "", &[]),
            mod_("fancy", true, "0.10.0", &["base", "off", "ghost"]),
            mod_("old", true, "0.2.0", &[]),
            mod_("odd", true, "x", &[]),
        ];
        assert!(all[0].warnings(&all, "0.2.0").is_empty());
        let w = all[2].warnings(&all, "0.9.0");
        assert_eq!(w.len(), 3, "{w:?}");
        assert!(w[0].contains("\"fancy\"") && w[0].contains("0.10.0") && w[0].contains("0.9.0"), "{w:?}");
        assert!(w[1].contains("\"off\"") && w[1].contains("switched off"), "{w:?}");
        assert!(w[2].contains("\"ghost\"") && w[2].contains("not installed"), "{w:?}");
        assert!(all[3].warnings(&all, "0.2.0").is_empty(), "the same version is enough");
        assert_eq!(all[4].warnings(&all, "0.2.0").len(), 1, "an unreadable version is reported, not fatal");
        let newer = Installed { format: MOD_FORMAT + 1, ..mod_("f", true, "", &[]) };
        assert!(newer.warnings(&[], "0.2.0")[0].contains("format"));
    }

    #[test]
    fn the_merged_list_in_docs_modding_md_is_this_one() {
        let doc = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/modding.md")).unwrap();
        let row = doc.lines().find(|l| l.contains("**merged**, deep")).expect("the merge rule row of docs/modding.md");
        let mut listed: Vec<&str> = row.split('|').nth(1).unwrap().split('`').skip(1).step_by(2).collect();
        let mut code: Vec<&str> = MERGED.to_vec();
        listed.sort();
        code.sort();
        assert_eq!(listed, code, "docs/modding.md and modding::MERGED list different files");
    }

    #[test]
    fn only_an_empty_or_marked_folder_may_be_rebuilt_as_an_overlay() {
        let dir = std::env::temp_dir().join(format!("procyon_overlay_guard_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(disposable(&dir), "a missing folder");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(disposable(&dir), "an empty folder");
        std::fs::write(dir.join("notes.txt"), "mine").unwrap();
        assert!(!disposable(&dir), "somebody's files are never deleted");
        std::fs::write(dir.join(OVERLAY_MARK), "").unwrap();
        assert!(disposable(&dir), "an overlay the game built");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn extends_sees_a_sibling_mod_file_whatever_the_directory_order() {
        let dir = std::env::temp_dir().join(format!("procyon-mod-test-{}", std::process::id()));
        let (base, m) = (dir.join("assets"), dir.join("mod"));
        for d in [base.join("data"), m.join("data")] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(base.join("data/a.json"), r#"{"x": 1, "y": 1}"#).unwrap();
        std::fs::write(m.join("data/a.json"), r#"{"x": 2}"#).unwrap();
        std::fs::write(m.join("data/b.json"), r#"{"extends": "a", "y": 3}"#).unwrap();
        let root = overlay(&base, &[m], None);
        let b: Value = read(&root.join("data/b.json")).unwrap();
        assert_eq!((b["x"].as_i64(), b["y"].as_i64()), (Some(2), Some(3)), "b extends the mod's own a.json, not the base one");
        // overlay files may be links to the base: replacing one must never write through to the original
        assert_eq!(std::fs::read_to_string(base.join("data/a.json")).unwrap(), r#"{"x": 1, "y": 1}"#);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn only_mod_drops_the_installed_mods() {
        let inst = |id: &str, enabled: bool| Installed { id: id.into(), enabled, dir: PathBuf::from(id), ..Installed::default() };
        let list = || vec![inst("a", true), inst("b", false), inst("c", true)];
        let folder = || vec![PathBuf::from("x"), PathBuf::from("y")];
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (shown, dirs) = pick_mods(&args(&["game", "--mod", "x"]), list(), folder()).unwrap();
        assert_eq!(dirs, [PathBuf::from("a"), "c".into(), "x".into(), "y".into()], "default: switched-on installed mods, then --mod");
        assert_eq!(shown.len(), 3);
        let (shown, dirs) = pick_mods(&args(&["game", "--only-mod", "--mod", "x"]), list(), folder()).unwrap();
        assert_eq!(dirs, [PathBuf::from("x"), "y".into()], "--only-mod: the --mod folders alone, in order");
        assert!(shown.is_empty(), "installed mods are not offered to `requires` either");
        assert!(pick_mods(&args(&["game", "--only-mod"]), list(), vec![]).unwrap_err().contains("--mod"));
        assert_eq!(pick_mods(&args(&["game"]), list(), vec![]).unwrap().1.len(), 2, "no flag, no --mod: the installed mods as before");
        assert!(pick_mods(&args(&["--only-mod"]), list(), folder()).unwrap().1.len() == 4, "argv[0] is the program, not a flag");
    }

    #[test]
    fn discovery_lists_folders_and_keeps_enabled_order() {
        let dir = std::env::temp_dir().join(format!("procyon-mods-list-{}", std::process::id()));
        for (f, meta) in [("a", r#"{"name": "Alpha", "version": "1"}"#), ("b", r#"{"id": "bee"}"#), ("c", "")] {
            std::fs::create_dir_all(dir.join("mods").join(f)).unwrap();
            if !meta.is_empty() {
                std::fs::write(dir.join("mods").join(f).join("mod.json"), meta).unwrap();
            }
        }
        std::fs::write(dir.join("mods.json"), r#"{"enabled": ["c", "bee", "gone"]}"#).unwrap();
        let bundled = dir.join("bundled");
        std::fs::create_dir_all(bundled.join("a")).unwrap();
        std::fs::create_dir_all(bundled.join("zz")).unwrap();
        std::fs::write(bundled.join("a/mod.json"), r#"{"name": "Shadowed"}"#).unwrap();
        let mut list = installed_in(&dir, Some(&bundled));
        let ids: Vec<_> = list.iter().map(|m| (m.id.as_str(), m.enabled)).collect();
        assert_eq!(ids, [("c", true), ("bee", true), ("a", false), ("zz", false)], "enabled first in file order, unknown ids ignored, bundled listed too");
        assert_eq!(list[2].name, "Alpha", "the user folder wins over a bundled mod of the same id");
        list[2].enabled = true;
        save_enabled_in(&dir, &list);
        assert_eq!(installed_in(&dir, Some(&bundled)).iter().filter(|m| m.enabled).count(), 3);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn default_bundled_mods_start_on_until_switched_off() {
        let dir = std::env::temp_dir().join(format!("procyon-mods-default-{}", std::process::id()));
        let bundled = dir.join("bundled");
        for (root, f) in [(&bundled, "speed"), (&bundled, "plain"), (&dir.join("mods"), "mine")] {
            std::fs::create_dir_all(root.join(f)).unwrap();
            std::fs::write(root.join(f).join("mod.json"), if f == "plain" { "{}" } else { r#"{"default": true}"# }).unwrap();
        }
        let on = |list: &[Installed]| list.iter().filter(|m| m.enabled).map(|m| m.id.clone()).collect::<Vec<_>>();
        let mut list = installed_in(&dir, Some(&bundled));
        assert_eq!(on(&list), ["speed"], "no mods.json yet: only the bundled default is on; a user folder cannot switch itself on");
        let i = list.iter().position(|m| m.id == "speed").unwrap();
        list[i].enabled = false;
        save_enabled_in(&dir, &list);
        assert!(on(&installed_in(&dir, Some(&bundled))).is_empty(), "switching a default mod off is remembered");
        std::fs::write(dir.join("mods.json"), r#"{"enabled": ["plain"]}"#).unwrap();
        assert_eq!(on(&installed_in(&dir, Some(&bundled))), ["plain", "speed"], "an older mods.json without `disabled` keeps the default on");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn collision_shapes_merge_by_type() {
        let dir = std::env::temp_dir().join(format!("procyon-mod-collision-{}", std::process::id()));
        let (base, m) = (dir.join("assets"), dir.join("mod"));
        for d in [base.join("data"), m.join("data")] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(base.join("data/collision.json"), r#"{"Island01": {"min": [0, 0, 0], "max": [1, 1, 1]}}"#).unwrap();
        std::fs::write(m.join("data/collision.json"), r#"{"Island_Ours": {"min": [0, 0, 0], "max": [2, 2, 2]}}"#).unwrap();
        let c: Value = read(&overlay(&base, &[m], None).join("data/collision.json")).unwrap();
        assert!(c["Island01"].is_object() && c["Island_Ours"].is_object(), "a mod's scenery type is added beside the retail ones");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn dock_points_merge_by_type() {
        let dir = std::env::temp_dir().join(format!("procyon-mod-docks-{}", std::process::id()));
        let (base, m) = (dir.join("assets"), dir.join("mod"));
        for d in [base.join("data"), m.join("data")] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(base.join("data/docks.json"), r#"{"Base_X": {"points": [{"label": "DockPoint1", "pos": [0, 0, 0], "yaw": 0}], "tender": true}}"#).unwrap();
        std::fs::write(m.join("data/docks.json"), r#"{"FerrumTown": {"points": [{"label": "Berth1", "pos": [1, 2, 0], "yaw": 1.5}], "tender": true}}"#).unwrap();
        let c: Value = read(&overlay(&base, &[m], None).join("data/docks.json")).unwrap();
        assert!(c["Base_X"]["points"].is_array() && c["FerrumTown"]["points"].is_array(), "a mod's dock points are added beside the retail ones");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn crew_nodes_merge_by_scenery_type_across_mods() {
        let dir = std::env::temp_dir().join(format!("procyon-mod-crew-nodes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (base, a, b) = (dir.join("assets"), dir.join("a"), dir.join("b"));
        for (d, text) in [
            (&base, r#"{"Town": [{"name": "Crew_Walk_A_0", "pos": [0, 0, 0]}]}"#),
            (&a, r#"{"Pier": [{"name": "Crew_Work_0", "pos": [1, 0, 0]}]}"#),
            (&b, r#"{"Town": [{"name": "Crew_Walk_B_0", "pos": [2, 0, 0]}]}"#),
        ] {
            std::fs::create_dir_all(d.join("data")).unwrap();
            std::fs::write(d.join("data/crew_nodes.json"), text).unwrap();
        }
        let c: Value = read(&overlay(&base, &[a, b], None).join("data/crew_nodes.json")).unwrap();
        assert!(c["Pier"].is_array(), "the first mod's type is added");
        assert_eq!(c["Town"][0]["name"], "Crew_Walk_B_0", "a type a later mod gives replaces its list (arrays replace)");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn a_ship_extending_a_retail_hull_with_crew_nodes_keeps_the_hulls_other_keys() {
        let (dir, root) = bad_mod(
            "crew-nodes-extends",
            &[("ships/Ship_X/ship.json", r#"{"race": "Navy", "core_hp": 80, "guns": {"G": {"arc_deg": 90, "gun": "Y"}}}"#)],
            &[("ships/Ship_Crewed/ship.json", r#"{"extends": "Ship_X", "crew_nodes": [{"name": "Crew_Walk_P_0", "pos": [0, 1, 2]}]}"#)],
        );
        let s: Value = read(&root.join("ships/Ship_Crewed/ship.json")).unwrap();
        assert_eq!((s["race"].as_str(), s["core_hp"].as_i64(), s["guns"]["G"]["gun"].as_str()), (Some("Navy"), Some(80), Some("Y")));
        assert_eq!(s["crew_nodes"][0]["name"], "Crew_Walk_P_0");
        assert!(s.get("extends").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn set_replaces_one_value_inside_an_array() {
        let dir = std::env::temp_dir().join(format!("procyon-mod-set-{}", std::process::id()));
        let (base, m) = (dir.join("assets"), dir.join("mod"));
        for d in [base.join("data"), m.join("data")] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(base.join("data/a.json"), r#"{"list": [{"fx": "x", "pos": 1}, {"fx": "y"}]}"#).unwrap();
        std::fs::write(m.join("data/a.json"), r#"{"extends": "a", "set": {"/list/1/fx": "z"}}"#).unwrap();
        let root = overlay(&base, &[m], None);
        let a: Value = read(&root.join("data/a.json")).unwrap();
        assert_eq!(a, serde_json::json!({"list": [{"fx": "x", "pos": 1}, {"fx": "z"}]}));
        std::fs::remove_dir_all(dir).unwrap();
    }
    /// A synthetic base + one mod in a temp dir; `files` are the mod's (path, text). Returns the overlay root and the stderr-free state.
    fn bad_mod(tag: &str, base_files: &[(&str, &str)], files: &[(&str, &str)]) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("procyon-mod-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (root, list) in [("assets", base_files), ("mod", files)] {
            for (f, text) in list {
                let p = dir.join(root).join(f);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, text).unwrap();
            }
        }
        let root = overlay(&dir.join("assets"), &[dir.join("mod")], None);
        (dir, root)
    }
    #[test]
    fn bad_json_in_a_merged_file_is_skipped_and_the_base_file_survives() {
        let (dir, root) = bad_mod(
            "bad-merged",
            &[("data/collision.json", r#"{"Island01": 1}"#), ("data/a.json", r#"{"x": 1}"#)],
            &[("data/collision.json", "Island_Ours: not json"), ("data/a.json", r#"{"x": 2}"#)],
        );
        let c: Value = read(&root.join("data/collision.json")).unwrap();
        assert_eq!(c, serde_json::json!({"Island01": 1}), "neither an empty value nor a half merge");
        assert_eq!(read(&root.join("data/a.json")).unwrap()["x"], 2, "the mod's other files still load");
        let e = crate::data::load_as::<Value>(&dir.join("mod/data/collision.json"), Some("mod")).unwrap_err().to_string();
        assert!(e.contains("collision.json") && e.contains("mod \"mod\"") && e.contains("line 1 column"), "{e}");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn bad_json_in_mod_json_does_not_stop_the_mod_or_panic() {
        let (dir, root) = bad_mod("bad-modjson", &[("data/a.json", r#"{"x": 1}"#)], &[("mod.json", "not json"), ("data/a.json", r#"{"x": 2}"#)]);
        assert_eq!(read(&root.join("data/a.json")).unwrap()["x"], 2);
        assert!(!root.join("mod.json").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn bad_json_in_an_extends_file_and_a_bad_extends_leave_the_base_file() {
        let (dir, root) = bad_mod(
            "bad-extends",
            &[("data/a.json", r#"{"x": 1}"#), ("data/c.json", r#"{"x": 1}"#), ("data/d.json", r#"{"list": [1]}"#)],
            &[
                ("data/a.json", "extends a, not json"),
                ("data/b.json", r#"{"extends": "missing", "y": 1}"#),
                ("data/c.json", r#"{"extends": "c", "y": 3}"#),
                ("data/d.json", r#"{"extends": "d", "set": {"/list/5": 1}}"#),
            ],
        );
        assert_eq!(read(&root.join("data/a.json")).unwrap(), serde_json::json!({"x": 1}), "unparsable extends file: base stays");
        assert!(!root.join("data/b.json").exists(), "unknown parent: nothing added");
        assert_eq!(read(&root.join("data/c.json")).unwrap(), serde_json::json!({"x": 1, "y": 3}), "a good extends file in the same mod still applies");
        assert_eq!(read(&root.join("data/d.json")).unwrap(), serde_json::json!({"list": [1]}), "a set path that does not exist leaves the base");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn merge_overrides_nested_keys_only() {
        let mut b = serde_json::json!({"physics": {"MaxSpeed": 10.0, "Mass": 5}, "name": "a"});
        merge(&mut b, &serde_json::json!({"physics": {"MaxSpeed": 20.0}}));
        assert_eq!(b["physics"]["MaxSpeed"], 20.0);
        assert_eq!(b["physics"]["Mass"], 5);
    }
    /// Temp dir with a base and a mod from (path, text) lists; returns (dir, report).
    fn report_for(tag: &str, base_files: &[(&str, &str)], files: &[(&str, &str)]) -> (PathBuf, ModReport) {
        let dir = std::env::temp_dir().join(format!("procyon-mod-report-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (root, list) in [("assets", base_files), ("mod", files)] {
            for (f, text) in list {
                let p = dir.join(root).join(f);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, text).unwrap();
            }
        }
        let (_, report) = overlay_report(&dir.join("assets"), &[dir.join("mod")], None);
        (dir, report)
    }
    #[test]
    fn report_sorts_files_into_override_merge_extends_added_and_skipped() {
        let (dir, r) = report_for(
            "all",
            &[("data/a.json", r#"{"x": 1}"#), ("data/collision.json", "{}"), ("data/base.json", r#"{"y": 1}"#)],
            &[
                ("mod.json", r#"{"name": "Demo", "version": "2.1"}"#),
                ("data/a.json", r#"{"x": 2}"#),
                ("data/collision.json", r#"{"K": 1}"#),
                ("data/child.json", r#"{"extends": "base", "y": 2}"#),
                ("data/new.json", "{}"),
                ("data/broken.json", "not json"),
            ],
        );
        let m = &r.mods[0];
        assert_eq!((m.name.as_str(), m.version.as_str()), ("Demo", "2.1"));
        assert_eq!(m.overrides, ["data/a.json"]);
        assert_eq!(m.merged, ["data/collision.json"]);
        assert_eq!(m.extends, ["data/child.json"]);
        assert_eq!(m.added, ["data/new.json"]);
        assert_eq!(m.skipped.len(), 1);
        assert!(m.skipped[0].0 == "data/broken.json" && m.skipped[0].1.contains("line 1 column"), "{:?}", m.skipped);
        let text = r.lines().join("\n");
        assert!(text.contains("Demo 2.1") && text.contains("overrides 1: data/a.json") && text.contains("skipped 1:"), "{text}");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn report_skips_a_bad_extends_with_the_loader_error() {
        let (dir, r) = report_for("badext", &[], &[("data/c.json", r#"{"extends": "nope"}"#)]);
        assert!(r.mods[0].skipped[0].1.contains("extends unknown 'nope'"), "{:?}", r.mods[0].skipped);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn report_caps_long_lists_and_names_the_rest() {
        let files: Vec<(String, String)> = (0..20).map(|i| (format!("data/f{i:02}.json"), "{}".into())).collect();
        let base: Vec<(&str, &str)> = files.iter().map(|(f, t)| (f.as_str(), t.as_str())).collect();
        let (dir, r) = report_for("cap", &base, &base);
        assert_eq!(r.mods[0].overrides.len(), 20);
        let lines = r.lines();
        assert!(lines.len() < 8, "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("overrides 20:") && l.contains("+12 more")), "{lines:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn report_for_no_mods_is_one_short_line() {
        let (root, r) = overlay_report(Path::new("nowhere"), &[], None);
        assert_eq!(root, Path::new("nowhere"));
        assert_eq!(r.lines(), ["mods: none (vanilla)"]);
        // a clean layer of the game's own is dropped from the report; one that skipped something stays
        let own = PathBuf::from("content");
        let clean = ModReport { mods: vec![ModEntry { dir: own.clone(), ..Default::default() }] };
        assert!(clean.without(&own).mods.is_empty());
        let bad = ModReport { mods: vec![ModEntry { dir: own.clone(), skipped: vec![("a".into(), "b".into())], ..Default::default() }] };
        assert_eq!(bad.without(&own).mods.len(), 1);
    }
    /// A tiny GLB with one material named `name` and `image` as its texture.
    fn model_with_texture(name: &str, image: &[u8]) -> Vec<u8> {
        let doc = serde_json::json!({"asset": {"version": "2.0"}, "buffers": [{"byteLength": image.len()}], "bufferViews": [{"buffer": 0, "byteLength": image.len()}],
            "images": [{"bufferView": 0, "mimeType": "image/png"}], "textures": [{"source": 0}],
            "materials": [{"name": name, "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}}]});
        crate::data::derive::tests::glb(doc, image)
    }
    fn has(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }
    #[test]
    fn a_mods_texture_png_replaces_that_retail_texture_in_every_model_that_uses_it() {
        let dir = std::env::temp_dir().join(format!("procyon-mod-textures-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (base, a, b) = (dir.join("assets"), dir.join("a"), dir.join("b"));
        let old = crate::shipart::png(2, 2, &[7u8; 16]);
        let (first, second) = (crate::shipart::png(4, 4, &[1u8; 64]), crate::shipart::png(8, 8, &[2u8; 256]));
        let put = |p: PathBuf, bytes: &[u8]| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        };
        put(base.join("ships/Ship_A/model.glb"), &model_with_texture("Hull_Planks.tga", &old));
        put(base.join("ships/Ship_B/model.glb"), &model_with_texture("Other.tga", &old));
        put(base.join("models/scenery/Pier.glb"), &model_with_texture("hull_planks.tga", &old));
        // a mod model asks for the retail texture, which the import has
        put(
            a.join("ships/Ship_Mine/model.glb"),
            &crate::data::derive::tests::glb(
                serde_json::json!({"asset": {"version": "2.0"}, "buffers": [{"byteLength": 4}], "materials": [{"name": "retail:hull_planks"}]}),
                &[0; 4],
            ),
        );
        put(a.join("textures/HULL_PLANKS.PNG"), &first);
        put(a.join("textures/typo_name.png"), &first);
        put(b.join("textures/Hull_Planks.tga.png"), &second);
        put(b.join("textures/surface/Other.png"), &first);
        let (root, report) = overlay_report(&base, &[a, b], Some(dir.join("overlay")));
        for rel in ["ships/Ship_A/model.glb", "models/scenery/Pier.glb", "ships/Ship_Mine/model.glb"] {
            let out = std::fs::read(root.join(rel)).unwrap();
            assert!(has(&out, &second) && !has(&out, &first), "{rel}: the later mod's picture, and only it");
            assert!(!std::fs::symlink_metadata(root.join(rel)).unwrap().file_type().is_symlink(), "{rel} is a rewritten copy");
        }
        #[cfg(unix)]
        assert!(std::fs::symlink_metadata(root.join("ships/Ship_B/model.glb")).unwrap().file_type().is_symlink(), "a model using no overridden texture stays a link");
        assert!(!root.join("textures/Hull_Planks.tga.png").exists() && !root.join("textures/HULL_PLANKS.PNG").exists(), "texture files are not placed");
        assert!(root.join("textures/surface/Other.png").exists(), "textures/surface/ keeps its own meaning");
        assert_eq!(report.mods[0].textures, ["textures/HULL_PLANKS.PNG", "textures/typo_name.png"]);
        assert_eq!(report.mods[0].unused_textures, ["textures/typo_name.png"], "a name no model uses is a warning on its mod");
        assert!(report.mods[1].unused_textures.is_empty() && report.mods[1].textures.len() == 1);
        let text = report.lines().join("\n");
        assert!(text.contains("texture overrides 2") && text.contains("warning: no model uses 1") && text.contains("typo_name.png"), "{text}");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn a_texture_file_that_is_not_a_png_is_skipped_with_a_reason() {
        let (dir, report) = report_for("badpng", &[], &[("textures/x.png", "not a picture")]);
        assert_eq!(report.mods[0].skipped[0].0, "textures/x.png");
        assert!(report.mods[0].textures.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
