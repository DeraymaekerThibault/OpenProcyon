//! The Mods screen's model: the installed mod folders and which are on (`modding.rs` owns discovery and `mods.json`), plus the
//! table's sort, category filter and scroll. The asset overlay is built once at launch, so a change takes effect on the next start.
use bevy::prelude::*;

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
}

/// Rows of the list the view draws; the view, the wheel and the scrollbar drag share it.
pub const MOD_ROWS: usize = 16;

impl ModList {
    pub fn load() -> Self {
        Self::from(crate::modding::installed(crate::bundled_mods_dir().as_deref()))
    }
    pub fn from(mods: Vec<crate::modding::Installed>) -> Self {
        ModList { mods, sort: (ModCol::Name, true), category: None, scroll: 0, picked: None, filter_open: false }
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
}
