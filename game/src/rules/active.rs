//! The script a battle runs, whichever kind of battle it is: a campaign mission (`campaign::Mission`) or a skirmish map's script
//! (`skirmish::SkirmishScript`). The two resources stay (a `Mission` also means "this is the campaign"); consumers that only need the
//! script, its `paths` or its map text read it here instead of branching on both.
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde_json::Value;
use std::collections::HashMap;

use super::Script;
use super::skirmish::SkirmishScript;
use crate::Side;
use crate::campaign::Mission;

/// What a script's effects need besides the script itself (`systems::apply::FxParam::env`).
pub struct ScriptParts<'a> {
    pub script: &'a mut Script,
    /// The mission's or the map's `paths`.
    pub paths: &'a Value,
    /// A skirmish map's player -> side table; none in the campaign.
    pub sides: Option<&'a HashMap<String, Side>>,
    /// The mission's id; none in a skirmish.
    pub mission_id: Option<&'a str>,
}

/// Read access to the battle's script.
#[derive(SystemParam)]
pub struct ActiveScript<'w> {
    mission: Option<Res<'w, Mission>>,
    skirmish: Option<Res<'w, SkirmishScript>>,
}

impl ActiveScript<'_> {
    /// The mission's script, else the skirmish map's; none without one.
    pub fn script(&self) -> Option<&Script> {
        match (&self.mission, &self.skirmish) {
            (Some(m), _) => Some(&m.script),
            (None, Some(s)) => Some(&s.script),
            _ => None,
        }
    }
    pub fn is_mission(&self) -> bool {
        self.mission.is_some()
    }
    /// The script's map text (`script.json` / `mission.json` `map_text`: the place names the star map labels).
    pub fn map_text(&self) -> &Value {
        match (&self.mission, &self.skirmish) {
            (Some(m), _) => &m.data["map_text"],
            (None, Some(s)) => &s.map_text,
            _ => &Value::Null,
        }
    }
}

/// Write access to the battle's script.
#[derive(SystemParam)]
pub struct ActiveScriptMut<'w> {
    mission: Option<ResMut<'w, Mission>>,
    skirmish: Option<ResMut<'w, SkirmishScript>>,
}

impl ActiveScriptMut<'_> {
    pub fn is_mission(&self) -> bool {
        self.mission.is_some()
    }
    /// The mission's script, else the skirmish map's; none without one.
    pub fn script_mut(&mut self) -> Option<&mut Script> {
        self.parts().map(|p| p.script)
    }
    /// The script with what applying its effects needs.
    pub fn parts(&mut self) -> Option<ScriptParts<'_>> {
        if let Some(m) = self.mission.as_deref_mut() {
            let Mission { script, data, id, .. } = m;
            Some(ScriptParts { script, paths: &data["paths"], sides: None, mission_id: Some(id.as_str()) })
        } else if let Some(sk) = self.skirmish.as_deref_mut() {
            let SkirmishScript { script, paths, sides, .. } = sk;
            Some(ScriptParts { script, paths: &*paths, sides: Some(&*sides), mission_id: None })
        } else {
            None
        }
    }
}

/// The script a world holds now (the report writer's view), with the mission's id when it is one.
pub fn script_of(world: &World) -> Option<(&Script, Option<&str>)> {
    if let Some(m) = world.get_resource::<Mission>() { Some((&m.script, Some(m.id.as_str()))) } else { world.get_resource::<SkirmishScript>().map(|s| (&s.script, None)) }
}
