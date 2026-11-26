//! Not retail: a skirmish map's own script (`maps/<id>/script.json`), the same interpreter as the campaign, driving the map's convoys, timers and
//! team points. It runs in the campaign's rule update (`systems::update`): the same units, events and effects. Retail maps keep their end rules with
//! victory.rs (`without_end_rules`); a map that opts in with `"victory": "script"` lets its own rules end the game (`script_end`).
use bevy::prelude::*;
use serde_json::Value;
use std::collections::HashMap;

use super::eval::schema;
use super::*;
#[derive(Resource)]
pub struct SkirmishScript {
    pub script: Script,
    pub(in crate::rules) paths: Value,
    /// The script's environment block (space animal and dragon definitions).
    pub environment: Value,
    /// The script's map text (place names the star map labels).
    pub map_text: Value,
}

impl SkirmishScript {
    /// The map's script (`script.json`, already read: `data`) as a running script. `teams`: (team name id, its players).
    pub fn load(map: &str, data: &Value, teams: Vec<(String, Vec<String>)>) -> Self {
        let dir = crate::assets_dir().join(format!("maps/{map}"));
        // Optional speech lines, end messages and ship names, as a mission's `media.json`.
        let media = crate::data::try_json(&dir.join("media.json")).unwrap_or(Value::Null);
        let decides = script_decides(data);
        warn_campaign_only(map, data);
        // A retail map's end rules stay with victory.rs; a map that opts in lets its own end rules (and team objectives) decide.
        let mut script = Script::new(&if decides { data.clone() } else { without_end_rules(data) }, media);
        script.set_teams(teams);
        script.decide_game(decides);
        // `--fire-rule` / `--start-at` work in a skirmish as in a mission: the script's first step runs them.
        crate::campaign::apply_start(&mut script);
        SkirmishScript { script, paths: data["paths"].clone(), environment: data["environment"].clone(), map_text: data["map_text"].clone() }
    }
    /// Team points so far (victory.rs 'Team has X points').
    pub fn points(&self) -> Vec<(String, i32)> {
        self.script.team_points.iter().map(|(t, p)| (t.clone(), *p)).collect()
    }
}

/// The rules' names of the map-placed ships ('Setup Ship' names) by world-object id; none when the script lists no ships.
pub fn ship_names(data: &Value) -> Option<HashMap<i64, String>> {
    Some(data["ships"].as_array()?.iter().filter_map(|s| Some((s["id"].as_i64()?, s["name"].as_str()?.to_string()))).collect())
}

/// `"victory": "script"` (or `"objectives"`) in a map's `script.json`: the script's own end rules decide the game.
pub fn script_decides(data: &Value) -> bool {
    matches!(data["victory"].as_str(), Some("script" | "objectives"))
}

/// A skirmish has no cutscene camera, mission HUD or player-ship handoff: a rule that uses a campaign-only Type (the rule schema marks
/// them, `--check-mission` reports them as errors) changes nothing there. Say so once per rule and Type when the map loads.
fn warn_campaign_only(map: &str, data: &Value) {
    let mut seen = std::collections::HashSet::new();
    for r in data["rules"].as_array().into_iter().flatten() {
        for (role, key) in [(schema::Role::Condition, "conditions"), (schema::Role::Action, "actions")] {
            for e in r[key].as_array().into_iter().flatten() {
                let (rule, ty) = (r["name"].as_str().unwrap_or("").trim(), e["Type"].as_str().unwrap_or(""));
                if schema::lookup(role, ty).is_some_and(|d| d.campaign_only) && seen.insert((rule.to_string(), ty.to_string())) {
                    warn!("map '{map}': rule '{rule}' uses '{ty}', which only campaign missions support; it does nothing in a skirmish");
                }
            }
        }
    }
}
