//! What a mission's rules did during one play: the file `--rule-report <file>` writes when the battle ends (docs/modding.md,
//! "Authoring missions"). The editor and other tools read it with these types; the game builds it from the interpreter's own state
//! (`Script::rule_report`), so the file and the autoplay log's never-fired list cannot disagree.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How the play the report describes ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RuleReportOutcome {
    /// The mission's rules ended it as won.
    Victory,
    /// The mission's rules ended it as lost.
    Defeat,
    /// No rule ended the mission: the player left the battle or closed the game, or an unattended run stopped (time limit, stuck).
    Quit,
}

/// One rule of the mission, in script order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RuleReportEntry {
    /// The rule's `name`, without surrounding spaces.
    pub name: String,
    /// Its actions ran at least once (`runs` is above 0). Mission 1's opening NIS rules are played by the briefing, not the
    /// interpreter, so they stay false.
    pub fired: bool,
    /// How many times its actions ran: 1 for a run-once rule that fired, any number for a repeating one.
    pub runs: u32,
    /// Mission time in seconds of the first run; absent when it never ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_run: Option<f32>,
    /// Mission time in seconds of the latest run; absent when it never ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<f32>,
    /// The rule was still switched on at the end (its `Is Active` state, as `OP Set Rule Active` leaves it).
    pub active: bool,
}

/// The `--rule-report` file: the mission's rules and what each did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RuleReport {
    /// The map folder id of the mission or skirmish map (`mission01`, a custom mission's id, `red_rover`).
    pub mission: String,
    pub outcome: RuleReportOutcome,
    /// Mission time in seconds when the report was written.
    pub play_time: f32,
    /// Every rule of the mission, in script order.
    pub rules: Vec<RuleReportEntry>,
}

/// The JSON Schema (draft 2020-12) of [`RuleReport`]. Needs no assets.
pub fn rule_report_schema() -> serde_json::Value {
    serde_json::to_value(schemars::SchemaGenerator::default().into_root_schema_for::<RuleReport>()).expect("schema is JSON")
}
