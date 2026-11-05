//! The Mods screen's model: the installed mod folders and which are on (`modding.rs` owns discovery and `mods.json`), plus the
//! table's sort, category filter and scroll. The asset overlay is built once at launch, so a change takes effect on the next start.
//! Also the HD skies bake (`SkyBake`), which the screen starts for players who have no Python: the skies are made from each
//! player's own copy, so they cannot ship and every player bakes them once.
use bevy::prelude::*;
use std::path::Path;
use std::process::Child;
use std::sync::{Arc, Mutex, mpsc};

use crate::modding::Installed;
use crate::setup::ToolMsg;

/// The mod folder the sky baker writes (`MOD["id"]` in `tools/bake_sky.py`).
pub const HD_SKIES: &str = "hd-skies";
/// Mods whose panel offers the bake: the skies themselves, and the remaster look that is "best with HD skies".
const BAKE_FROM: [&str; 2] = [HD_SKIES, "remaster"];

/// The HD skies bake: the importer's `--bake-skies` (`tools/bake_sky.py`) on the imported assets, written into the player's
/// mods folder. Not retail. The new skies show from the next start, like any mod change.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum SkyBake {
    #[default]
    Idle,
    /// `step` of `total` pieces done (0 before the first), `percent` of the whole when the baker says.
    Running {
        step: u32,
        total: u32,
        percent: Option<u32>,
    },
    Done,
    /// Why it failed (or could not start).
    Failed(String),
}

/// The running bake's process and output. Dropping it (stopping the bake, quitting the game) stops the process.
#[derive(Resource, Default)]
pub struct BakeJob(Option<Job>);

struct Job {
    rx: Mutex<mpsc::Receiver<ToolMsg>>,
    child: Arc<Mutex<Child>>,
    /// The bake's standard input, held open: the baker (`--watch-stdin`) stops when it closes, with its workers. Killing the
    /// child alone is not enough: the release's one-file importer runs Python under a launcher that a kill leaves behind.
    _stdin: Option<std::process::ChildStdin>,
    /// The last line that was not progress: the reason when the bake fails.
    last: String,
}

impl Drop for Job {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

/// Quitting stops the bake (its workers follow their parent, `stop_with_parent` in the baker).
pub fn stop_bake_on_exit(mut exit: MessageReader<AppExit>, mut job: ResMut<BakeJob>) {
    if exit.read().count() > 0 && job.0.is_some() {
        job.0 = None;
    }
}

/// Applies the running bake's output to the Mods screen (`ModList::bake`); on success the list is read again so the new
/// mod shows, and switched on.
pub fn poll_bake(mut job: ResMut<BakeJob>, mut mods: ResMut<ModList>) {
    let Some(running) = job.bypass_change_detection().0.as_mut() else { return };
    let msgs: Vec<ToolMsg> = running.rx.lock().map(|rx| rx.try_iter().collect()).unwrap_or_default();
    for msg in msgs {
        let finished = match msg {
            ToolMsg::Line(line) => {
                mods.bake_line(&line, &mut running.last);
                false
            }
            ToolMsg::Exit(Ok(())) => {
                let fresh = crate::modding::installed(crate::bundled_mods_dir().as_deref());
                mods.baked(fresh, crate::campaign::user_dir().as_deref());
                true
            }
            ToolMsg::Exit(Err(reason)) => {
                mods.bake = SkyBake::Failed(if running.last.is_empty() { reason } else { format!("{reason} {}", running.last) });
                true
            }
        };
        if finished {
            job.0 = None;
            return;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModCol {
    Enabled,
    Name,
    Category,
    Version,
}

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct ModList {
    pub mods: Vec<crate::modding::Installed>,
    pub sort: (ModCol, bool),
    /// Only this category is shown (`None`: all).
    pub category: Option<String>,
    pub scroll: usize,
    /// The row picked for the right-hand panel (an index into `mods`, so sorting and filtering keep it).
    pub picked: Option<usize>,
    /// The category pull-down is open.
    pub filter_open: bool,
    pub bake: SkyBake,
}

/// Rows of the list the view draws; the view, the wheel and the scrollbar drag share it.
pub const MOD_ROWS: usize = 16;

impl ModList {
    pub fn load() -> Self {
        Self::from(crate::modding::installed(crate::bundled_mods_dir().as_deref()))
    }
    pub fn from(mods: Vec<crate::modding::Installed>) -> Self {
        ModList { mods, sort: (ModCol::Name, true), category: None, scroll: 0, picked: None, filter_open: false, bake: SkyBake::Idle }
    }
    /// Whether mod `i`'s panel offers to bake the HD skies.
    pub fn offers_bake(&self, i: usize) -> bool {
        self.mods.get(i).is_some_and(|m| BAKE_FROM.contains(&m.id.as_str()))
    }
    /// Whether the bake writes the stars as their own cube: for the remaster look's `sky` block, which twinkles them, when it is
    /// on. Without the block a separate stars cube is not drawn, so the stars stay in the sky picture.
    /// PROVISIONAL: switching the remaster look on or off after baking needs a new bake for the stars to match.
    fn split_stars(&self) -> bool {
        self.mods.iter().any(|m| m.id == "remaster" && m.enabled)
    }
    /// Starts the bake, or stops the one running.
    pub fn bake_or_stop(&mut self, job: &mut BakeJob) {
        if job.0.take().is_some() {
            self.bake = SkyBake::Idle;
            return;
        }
        let Some(out) = crate::modding::mods_dir().map(|d| d.join(HD_SKIES)) else {
            self.bake = SkyBake::Failed("There is no user folder to put the skies in.".into());
            return;
        };
        let assets = crate::base_assets();
        let assets = std::path::absolute(&assets).unwrap_or(assets);
        let mut args = vec!["--bake-skies".into(), "--watch-stdin".into(), "--assets".into(), assets.into_os_string(), "--out".into(), out.into_os_string()];
        if self.split_stars() {
            args.push("--split-stars".into());
        }
        let (tx, rx) = mpsc::channel();
        self.bake = match crate::setup::spawn_tool(&args, true, tx) {
            Ok((_, child)) => {
                let stdin = child.lock().ok().and_then(|mut c| c.stdin.take());
                job.0 = Some(Job { rx: Mutex::new(rx), child, _stdin: stdin, last: String::new() });
                SkyBake::Running { step: 0, total: 0, percent: None }
            }
            Err(reason) => SkyBake::Failed(reason),
        };
    }
    /// One output line of the bake: `[n/total] what (p%)` moves the progress on, anything else is kept in `last`.
    fn bake_line(&mut self, line: &str, last: &mut String) {
        match crate::setup::parse_progress(line) {
            Some((step, total, _, percent)) => {
                let next = SkyBake::Running { step, total, percent };
                if self.bake != next {
                    self.bake = next;
                }
            }
            None if !line.trim().is_empty() => *last = line.trim().to_owned(),
            None => {}
        }
    }
    /// The bake finished: `fresh` is the mod list read again; the skies are switched on (saved in `user`'s `mods.json`) and the
    /// picked row stays on the same mod.
    fn baked(&mut self, fresh: Vec<Installed>, user: Option<&Path>) {
        let picked = self.picked.and_then(|i| self.mods.get(i)).map(|m| m.id.clone());
        self.mods = fresh;
        self.picked = picked.and_then(|id| self.mods.iter().position(|m| m.id == id));
        match self.mods.iter_mut().find(|m| m.id == HD_SKIES) {
            Some(skies) => {
                if !skies.enabled {
                    skies.enabled = true;
                    if let Some(dir) = user {
                        crate::modding::save_enabled_in(dir, &self.mods);
                    }
                }
                self.bake = SkyBake::Done;
            }
            None => self.bake = SkyBake::Failed("The baker finished but no HD skies mod is installed.".into()),
        }
    }
    /// Flips mod `i` (an index into `mods`) and saves `mods.json`.
    pub fn toggle(&mut self, i: usize) {
        let Some(m) = self.mods.get_mut(i) else { return };
        m.enabled = !m.enabled;
        if let Some(dir) = crate::campaign::user_dir() {
            crate::modding::save_enabled_in(&dir, &self.mods);
        }
    }
    /// Sorting by the same column again reverses it.
    pub fn sort_by(&mut self, col: ModCol) {
        self.sort = (col, if self.sort.0 == col { !self.sort.1 } else { true });
        self.scroll = 0;
    }
    pub fn categories(&self) -> Vec<String> {
        let mut c: Vec<String> = self.mods.iter().map(|m| m.category.clone()).collect();
        c.sort();
        c.dedup();
        c
    }
    /// Filter by `n`: 0 is All, then each of `categories()`.
    pub fn set_filter(&mut self, n: usize) {
        self.category = n.checked_sub(1).and_then(|i| self.categories().get(i).cloned());
        self.filter_open = false;
        self.scroll = 0;
    }
    /// Indices into `mods` of the rows shown, filtered and sorted.
    pub fn rows(&self) -> Vec<usize> {
        let mut rows: Vec<usize> = (0..self.mods.len()).filter(|&i| self.category.as_ref().is_none_or(|c| *c == self.mods[i].category)).collect();
        let key = |i: usize| {
            let m = &self.mods[i];
            match self.sort.0 {
                ModCol::Enabled => (!m.enabled).to_string(),
                ModCol::Name => m.name.to_lowercase(),
                ModCol::Category => m.category.to_lowercase(),
                ModCol::Version => m.version.clone(),
            }
        };
        rows.sort_by_key(|&i| (key(i), self.mods[i].name.to_lowercase()));
        if !self.sort.1 {
            rows.reverse();
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn m(name: &str, category: &str, enabled: bool) -> crate::modding::Installed {
        crate::modding::Installed {
            id: name.into(),
            name: name.into(),
            version: "1".into(),
            description: String::new(),
            category: category.into(),
            dir: Default::default(),
            enabled,
            default: false,
        }
    }
    #[test]
    fn rows_sort_filter_and_reverse() {
        let mut l = ModList::from(vec![m("b", "Ships", false), m("A", "Visuals", true), m("c", "Ships", true)]);
        assert_eq!(l.rows(), [1, 0, 2], "by name, case-insensitive");
        l.sort_by(ModCol::Name);
        assert_eq!(l.rows(), [2, 0, 1], "same column again reverses");
        l.sort_by(ModCol::Enabled);
        assert_eq!(l.rows()[2], 0, "enabled first, the disabled mod last");
        l.set_filter(1);
        assert_eq!(l.category.as_deref(), Some("Ships"));
        assert_eq!(l.rows().len(), 2);
        l.set_filter(0);
        assert_eq!(l.category, None, "0 is All");
    }
    #[test]
    fn pick_survives_sort_and_filter() {
        let mut l = ModList::from(vec![m("b", "Ships", false), m("A", "Visuals", true)]);
        l.picked = Some(0);
        l.sort_by(ModCol::Name);
        l.set_filter(1);
        assert_eq!(l.picked, Some(0));
        assert_eq!(l.rows(), [0]);
    }
    #[test]
    fn bake_progress_and_the_reason_it_failed() {
        let mut l = ModList::from(vec![]);
        let mut last = String::new();
        l.bake_line("[3/30] face 2 band 0: 19 skies shaded (10%)", &mut last);
        assert_eq!(l.bake, SkyBake::Running { step: 3, total: 30, percent: Some(10) });
        l.bake_line("no sky models in C:/x/models/sky: import the game first", &mut last);
        l.bake_line("", &mut last);
        assert_eq!(l.bake, SkyBake::Running { step: 3, total: 30, percent: Some(10) }, "other lines leave the progress");
        assert_eq!(last, "no sky models in C:/x/models/sky: import the game first");
    }
    #[test]
    fn a_finished_bake_switches_the_skies_on_and_keeps_the_pick() {
        let mut l = ModList::from(vec![m("remaster", "Visuals", true)]);
        l.picked = Some(0);
        assert!(l.offers_bake(0));
        l.baked(vec![m(HD_SKIES, "Visuals", false), m("remaster", "Visuals", true)], None);
        assert_eq!(l.bake, SkyBake::Done);
        assert!(l.mods[0].enabled, "the new skies are on");
        assert_eq!(l.picked, Some(1), "still the remaster row");
        l.baked(vec![m("remaster", "Visuals", true)], None);
        assert!(matches!(l.bake, SkyBake::Failed(_)), "no skies mod after the bake");
        assert!(!ModList::from(vec![m("game-speed", "Rules", false)]).offers_bake(0));
        assert!(l.split_stars(), "the remaster look is on: its sky block twinkles a separate stars cube");
        l.mods[0].enabled = false;
        assert!(!l.split_stars());
    }
}
