//! The Mods screen's model: the installed mod folders and which are on (`modding.rs` owns discovery and `mods.json`).
//! The asset overlay is built once at launch, so a change takes effect on the next start.
use bevy::prelude::*;

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct ModList(pub Vec<crate::modding::Installed>);

impl ModList {
    pub fn load() -> Self {
        ModList(crate::modding::installed())
    }
    /// Flips mod `i` and saves `mods.json`.
    pub fn toggle(&mut self, i: usize) {
        let Some(m) = self.0.get_mut(i) else { return };
        m.enabled = !m.enabled;
        if let Some(dir) = crate::campaign::user_dir() {
            crate::modding::save_enabled_in(&dir, &self.0);
        }
    }
}
