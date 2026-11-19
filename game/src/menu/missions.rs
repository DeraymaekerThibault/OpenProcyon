//! Single Player > Custom Missions: the missions and campaigns that switched-on mods add (`campaign::custom_missions`). Not
//! retail: the original has no way to play a mission it does not ship. Playing one starts it like the campaign starts mission 1;
//! winning it offers the next mission of the same mod (`campaign::next`).
use bevy::prelude::*;

use crate::campaign::CustomMission;

/// Rows the list shows at once.
pub const MISSION_ROWS: usize = 16;

#[derive(Resource, Default)]
pub struct MissionList {
    pub list: Vec<CustomMission>,
    pub picked: Option<usize>,
    pub scroll: usize,
}

impl MissionList {
    pub fn load() -> Self {
        let list = crate::campaign::custom_missions().to_vec();
        Self { picked: (!list.is_empty()).then_some(0), list, scroll: 0 }
    }

    /// The picked mission's map id.
    pub fn id(&self, n: usize) -> Option<&str> {
        self.list.get(n).map(|m| m.id.as_str())
    }

    /// Where in its campaign mission `n` comes: (position from 1, missions in the campaign).
    pub fn place(&self, n: usize) -> (usize, usize) {
        let Some(m) = self.list.get(n) else { return (0, 0) };
        let same: Vec<usize> = (0..self.list.len()).filter(|&i| self.list[i].campaign == m.campaign).collect();
        (same.iter().position(|&i| i == n).map_or(0, |p| p + 1), same.len())
    }
}
