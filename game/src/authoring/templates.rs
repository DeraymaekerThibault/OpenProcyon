//! Mission templates: data files that write ordinary rules and tasks with a few names filled in (docs/modding.md "Mission templates").
//! The files are `content/templates/*.json`, our own hand-written text with no game content; they are compiled in, so a tool gets the
//! same list with no assets and no folders (a mod adding templates is not read: PROVISIONAL, not needed yet).
//!
//! Nothing remembers a template once it is expanded: [`expand_template`] returns plain rules and tasks for the caller to insert into
//! a mission document, where they are edited like any others.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The files of `content/templates/`, in the order [`templates`] lists them: the three blocks, then the mission templates.
const FILES: [&str; 7] = [
    include_str!("../../../content/templates/dialogue.json"),
    include_str!("../../../content/templates/cutscene.json"),
    include_str!("../../../content/templates/failsafe.json"),
    include_str!("../../../content/templates/reach-point.json"),
    include_str!("../../../content/templates/ambush-on-arrival.json"),
    include_str!("../../../content/templates/escort-to-gate.json"),
    include_str!("../../../content/templates/sink-group.json"),
];

/// One template file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Template {
    /// Stable id (the file name without `.json`).
    pub id: String,
    pub title: String,
    /// `"block"` (a piece to put into a mission: dialogue, cutscene, failsafe) or `"mission"` (a whole objective).
    pub kind: String,
    pub description: String,
    pub parameters: Vec<Parameter>,
    /// Keys of the mission document the template needs (`"victory": "objectives"`): to be written only where the mission has no such
    /// key yet. Optional.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub mission: Map<String, Value>,
    /// Objective tasks (`mission.json` `tasks`), with `{parameter}` placeholders in their string values. Optional.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<Value>,
    /// Rules (`mission.json` `rules`), with `{parameter}` placeholders in their string values.
    pub rules: Vec<Value>,
}

/// A name the author fills in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Parameter {
    /// The name inside `{}` in the template's strings.
    pub name: String,
    /// What it holds: `text`, `number`, or a kind of the rule schema the mission defines (`group`, `volume`, `point`, `point_set`,
    /// `speech_event`, `rule`). A tool offers the matching picker; the value is always the name as text (a `number` as its digits).
    pub kind: String,
    /// The label a form shows.
    pub label: String,
    /// The value used when the author gives none; without one the parameter is required. Always text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// What [`expand_template`] gives: ordinary entries to add to a mission document. The caller owns the insertion (and that it is one
/// undo step); names that are already taken are the caller's to refuse or to rename (`check_mission` reports a repeated rule name).
#[derive(Clone, Debug, PartialEq, Serialize, Default)]
pub struct Expansion {
    pub rules: Vec<Value>,
    pub tasks: Vec<Value>,
    /// The template's `mission` keys, to write where the mission lacks the key.
    pub mission: Map<String, Value>,
}

/// The kinds a parameter may have.
pub const PARAMETER_KINDS: [&str; 8] = ["text", "number", "group", "volume", "point", "point_set", "speech_event", "rule"];

/// Every template, blocks first. The files are compiled in and tested to parse, so this does not fail.
pub fn templates() -> Vec<Template> {
    FILES.iter().map(|f| serde_json::from_str(f).expect("content/templates/*.json parses (tested)")).collect()
}

/// The JSON Schema (draft 2020-12) of a template file, for a tool that validates or offers templates written by others.
pub fn template_schema() -> Value {
    serde_json::to_value(schemars::schema_for!(Template)).expect("a schema is JSON")
}

/// The template with `params` filled in. `params` maps a parameter name to its value; a parameter left out takes its default.
/// Errors: a name in `params` the template does not declare, a parameter with neither a value nor a default (or an empty one), a
/// `number` that is not a number, or a placeholder the template does not declare (a broken file).
///
/// A string that is exactly `{name}` of a `number` parameter becomes a JSON number; in any other string each `{name}` is replaced by
/// the text. Object keys are never changed. A value is inserted as it is: braces in it are not expanded again.
pub fn expand_template(template: &Template, params: &BTreeMap<String, String>) -> Result<Expansion, String> {
    let declared: BTreeSet<&str> = template.parameters.iter().map(|p| p.name.as_str()).collect();
    if let Some(unknown) = params.keys().find(|k| !declared.contains(k.as_str())) {
        return Err(format!("template '{}' has no parameter '{unknown}'", template.id));
    }
    let mut values: BTreeMap<&str, (&Parameter, String)> = BTreeMap::new();
    for p in &template.parameters {
        let value = params.get(&p.name).cloned().or_else(|| p.default.clone()).unwrap_or_default();
        if value.trim().is_empty() {
            return Err(format!("template '{}': parameter '{}' ({}) needs a value", template.id, p.name, p.label));
        }
        if p.kind == "number" && value.trim().parse::<f64>().is_err() {
            return Err(format!("template '{}': parameter '{}' ({}) is not a number: '{value}'", template.id, p.name, p.label));
        }
        values.insert(&p.name, (p, value));
    }
    let fill = |list: &[Value]| list.iter().map(|v| substitute(v, &values, &template.id)).collect::<Result<Vec<_>, _>>();
    Ok(Expansion { rules: fill(&template.rules)?, tasks: fill(&template.tasks)?, mission: template.mission.clone() })
}

fn substitute(v: &Value, values: &BTreeMap<&str, (&Parameter, String)>, id: &str) -> Result<Value, String> {
    match v {
        Value::String(s) => {
            if let Some(name) = s.strip_prefix('{').and_then(|r| r.strip_suffix('}')).filter(|n| is_name(n))
                && let Some((p, value)) = values.get(name)
                && p.kind == "number"
            {
                let n: f64 = value.trim().parse().expect("checked above");
                return Ok(if n.fract() == 0.0 && n.abs() < 1e15 { Value::from(n as i64) } else { Value::from(n) });
            }
            let mut out = String::new();
            let mut rest = s.as_str();
            while let Some(i) = rest.find('{') {
                out.push_str(&rest[..i]);
                let after = &rest[i + 1..];
                match after.find('}').map(|j| (&after[..j], j)).filter(|(n, _)| is_name(n)) {
                    Some((name, j)) => {
                        let (_, value) = values.get(name).ok_or_else(|| format!("template '{id}': placeholder '{{{name}}}' is not a parameter"))?;
                        out.push_str(value);
                        rest = &after[j + 1..];
                    }
                    None => {
                        out.push('{');
                        rest = after;
                    }
                }
            }
            out.push_str(rest);
            Ok(Value::String(out))
        }
        Value::Array(a) => a.iter().map(|x| substitute(x, values, id)).collect::<Result<Vec<_>, _>>().map(Value::Array),
        Value::Object(m) => m.iter().map(|(k, x)| Ok((k.clone(), substitute(x, values, id)?))).collect::<Result<Map<_, _>, String>>().map(Value::Object),
        other => Ok(other.clone()),
    }
}

fn is_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::{Level, check_mission, rule_schema};
    use serde_json::json;

    fn params(list: &[(&str, &str)]) -> BTreeMap<String, String> {
        list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    /// A value for each required parameter that the map and media below define.
    fn test_params(t: &Template) -> BTreeMap<String, String> {
        let pick = |kind: &str, name: &str| match kind {
            "group" if name == "escorted" || name == "target" || name == "ambushers" => "Enemies",
            "group" => "Hero",
            "volume" => "Gate Area",
            "point" => "The Gate",
            "point_set" => "Arrival",
            "speech_event" => "LINE",
            "rule" => "Other Rule",
            _ => "Text",
        };
        t.parameters.iter().filter(|p| p.default.is_none()).map(|p| (p.name.clone(), pick(&p.kind, &p.name).to_string())).collect()
    }

    /// An empty mission plus what the templates refer to: the groups, area, point, point set, speech and two rules to switch.
    fn world() -> (Value, Value, Value) {
        let mission = json!({
            "players": ["Hero", "Raiders"], "alliances": [], "groups": {"Enemies": [2], "Hero": [1]},
            "ships": [
                {"id": 1, "type": "Ship_Navy_Cutter", "group": "Hero", "name": "Dauntless", "primary": true, "pos": [0.0, 0.0, 0.0]},
                {"id": 2, "type": "Ship_Pirate_Sloop", "group": "Raiders", "name": "Raider", "pos": [400.0, 600.0, 0.0]}
            ],
            "volumes": {"Gate Area": [[300.0, 500.0], [500.0, 500.0], [500.0, 700.0], [300.0, 700.0]]},
            "points": [{"Name": "The Gate", "Position": [400.0, 600.0, 0.0]}],
            "environment": {"point_sets": {"Arrival": [{"pos": [10.0, 10.0, 0.0]}]}},
            "tasks": [],
            "rules": [
                {"name": "Other Rule", "Run Once": true, "Is Active": false, "conditions": [{"Type": "World Initialize"}], "actions": [{"Type": "OP Wait", "Seconds": 1}]}
            ]
        });
        let media = json!({"speech": {"LINE": {"text": "Ahoy."}}, "messages": {}, "ship_names": {}});
        (mission, json!({"id": "t", "world_half": 1200.0}), media)
    }

    #[test]
    fn every_template_parses_and_is_listed() {
        let all = templates();
        let ids: Vec<&str> = all.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["dialogue", "cutscene", "failsafe", "reach-point", "ambush-on-arrival", "escort-to-gate", "sink-group"]);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../content/templates");
        let on_disk = std::fs::read_dir(dir).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "json")).count();
        assert_eq!(on_disk, all.len(), "a file in content/templates/ is not listed in FILES");
        for t in &all {
            assert!(["block", "mission"].contains(&t.kind.as_str()), "{}", t.id);
            assert!(!t.title.is_empty() && !t.description.is_empty() && !t.rules.is_empty(), "{}", t.id);
            for p in &t.parameters {
                assert!(PARAMETER_KINDS.contains(&p.kind.as_str()), "{}: kind {}", t.id, p.kind);
                assert!(is_name(&p.name) && !p.label.is_empty(), "{}: {}", t.id, p.name);
            }
        }
        assert!(template_schema()["properties"]["parameters"].is_object());
    }

    #[test]
    fn every_template_expands_into_a_mission_that_checks_clean() {
        let schema = rule_schema();
        for t in templates() {
            let (mut mission, map, media) = world();
            let x = expand_template(&t, &test_params(&t)).unwrap_or_else(|e| panic!("{}: {e}", t.id));
            assert!(!x.rules.is_empty(), "{}", t.id);
            mission["rules"].as_array_mut().unwrap().extend(x.rules);
            mission["tasks"].as_array_mut().unwrap().extend(x.tasks);
            for (k, v) in x.mission {
                mission.as_object_mut().unwrap().entry(k).or_insert(v);
            }
            let problems = check_mission(&mission, &map, &media, false);
            let errors: Vec<_> = problems.iter().filter(|p| p.level == Level::Error).collect();
            assert!(errors.is_empty(), "{}: {errors:#?}", t.id);
            // Every Type a template writes is one the rule schema lists.
            let known: BTreeSet<&str> = schema["types"].as_array().unwrap().iter().filter_map(|x| x["name"].as_str()).collect();
            for rule in mission["rules"].as_array().unwrap() {
                for item in rule["conditions"].as_array().into_iter().chain(rule["actions"].as_array()).flatten() {
                    assert!(known.contains(item["Type"].as_str().unwrap()), "{}: {item}", t.id);
                }
            }
        }
    }

    #[test]
    fn placeholders_are_all_substituted_and_all_declared() {
        for t in templates() {
            let doc = serde_json::to_string(&json!([t.rules, t.tasks])).unwrap();
            for p in &t.parameters {
                assert!(doc.contains(&format!("{{{}}}", p.name)), "{}: parameter {} is never used", t.id, p.name);
            }
            let x = expand_template(&t, &test_params(&t)).unwrap();
            let out = serde_json::to_string(&json!([x.rules, x.tasks])).unwrap();
            for p in &t.parameters {
                assert!(!out.contains(&format!("{{{}}}", p.name)), "{}: {{{}}} left", t.id, p.name);
            }
        }
    }

    #[test]
    fn expansion_errors_name_the_problem() {
        let t = templates().into_iter().find(|t| t.id == "reach-point").unwrap();
        let ok = test_params(&t);
        let mut unknown = ok.clone();
        unknown.insert("colour".into(), "red".into());
        assert!(expand_template(&t, &unknown).unwrap_err().contains("no parameter 'colour'"));
        let mut missing = ok.clone();
        missing.remove("volume");
        assert!(expand_template(&t, &missing).unwrap_err().contains("'volume'"));
        let mut blank = ok;
        blank.insert("group".into(), "  ".into());
        assert!(expand_template(&t, &blank).is_err());
        let f = templates().into_iter().find(|t| t.id == "failsafe").unwrap();
        let mut bad = test_params(&f);
        bad.insert("seconds".into(), "soon".into());
        assert!(expand_template(&f, &bad).unwrap_err().contains("not a number"));
        let broken = Template { rules: vec![json!({"name": "{nope}"})], ..t };
        assert!(expand_template(&broken, &test_params(&broken)).unwrap_err().contains("'{nope}'"));
    }

    #[test]
    fn numbers_become_numbers_and_values_are_not_expanded_again() {
        let f = templates().into_iter().find(|t| t.id == "failsafe").unwrap();
        let x = expand_template(&f, &{
            let mut p = test_params(&f);
            p.insert("seconds".into(), "90".into());
            p.insert("timer".into(), "{seconds}".into());
            p
        })
        .unwrap();
        let cond = &x.rules[1]["conditions"][0]["Conditions"][0];
        assert_eq!(cond["Time in seconds"], json!(90));
        assert_eq!(cond["Timer Name"], "{seconds}");
        let half = expand_template(&f, &{
            let mut p = test_params(&f);
            p.insert("seconds".into(), "7.5".into());
            p
        })
        .unwrap();
        assert_eq!(half.rules[1]["conditions"][0]["Conditions"][0]["Time in seconds"], json!(7.5));
    }

    /// With its own names the ambush template is rule 2 and rule 3 of the `gate-ambush` example, and its second task.
    #[test]
    fn ambush_on_arrival_matches_the_gate_ambush_example() {
        let t = templates().into_iter().find(|t| t.id == "ambush-on-arrival").unwrap();
        let x = expand_template(&t, &params(&[("group", "Hero"), ("volume", "Gate Area"), ("ambushers", "RaiderShips")])).unwrap();
        let example = include_str!("../../../docs/examples/gate-ambush.mission.json");
        let example: Value = serde_json::from_str(example).unwrap();
        assert_eq!(x.rules[0], example["rules"][1]);
        assert_eq!(x.rules[1], example["rules"][2]);
        assert_eq!(x.tasks[0], example["tasks"][1]);
    }
}
