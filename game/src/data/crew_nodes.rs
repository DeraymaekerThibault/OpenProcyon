//! Not retail: crew places given in JSON instead of in a model. A ship's `crew_nodes` key (`ship.json`) and `data/crew_nodes.json`
//! (map scenery by type) list the same empty nodes `Crew_Walk_<lane>_<k>`, `Crew_Gun_*`, `Crew_Post_*`, `Crew_Work_*`,
//! `Crew_Climb_<shroud>_a`/`_b` and `Crew_Count_<n>` that a model may carry, so a mod can man a retail hull or a town without
//! shipping a GLB. They are turned into the node representation the deck crews already read (`render/deckhands.rs` collects the
//! model's nodes as `(name, Transform)`) and combined with the model's: a new name is added, the name of a model node replaces it.
//! Presentation only: nothing here reaches the simulation, and a profile with no `deckhands` block never draws them.
use super::ships::{mount_rotation, point};
use super::*;
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::BTreeMap;

/// The kinds `Crew_<Kind>_...` the deck crews understand (`render/deckhands.rs`).
const KINDS: [&str; 6] = ["Walk", "Gun", "Post", "Work", "Climb", "Count"];

/// One entry of a `crew_nodes` list.
#[derive(Deserialize, JsonSchema)]
pub(super) struct CrewNodeDoc {
    /// The node's name: `Crew_Walk_<lane>_<k>`, `Crew_Gun_*`, `Crew_Post_*`, `Crew_Work_*`, `Crew_Climb_<shroud>_a` / `_b` or `Crew_Count_<n>`.
    name: String,
    /// Ship frame, game axes (x starboard, y forward, z up): the convention of a gun's `pos`.
    pos: [f64; 3],
    /// Rotation, the convention of a gun's `rot`: the way the figure faces at the place.
    rot: Option<[f64; 3]>,
}

/// A crew place as the deck crews read it: the node's name and its transform in the model's frame.
pub type CrewNode = (String, Transform);

/// The entries as nodes. `context` names the file and the key, e.g. `ships/X/ship.json: crew_nodes`.
pub(super) fn nodes(docs: &[CrewNodeDoc], context: &str) -> Result<Vec<CrewNode>, String> {
    docs.iter()
        .enumerate()
        .map(|(i, d)| {
            let kind = d.name.strip_prefix("Crew_").map(|r| r.split_once('_').map_or(r, |s| s.0));
            if !kind.is_some_and(|k| KINDS.contains(&k)) {
                return Err(format!(
                    "{context}[{i}].name: {:?} is not a crew node name (Crew_Walk_<lane>_<k>, Crew_Gun_*, Crew_Post_*, Crew_Work_*, Crew_Climb_<shroud>_a/_b, Crew_Count_<n>)",
                    d.name
                ));
            }
            if !d.pos.iter().chain(d.rot.iter().flatten()).all(|v| v.is_finite()) {
                return Err(format!("{context}[{i}]: pos and rot must be finite numbers"));
            }
            let rotation = d.rot.map_or(Quat::IDENTITY, mount_rotation);
            Ok((d.name.clone(), Transform::from_translation(point(d.pos)).with_rotation(rotation)))
        })
        .collect()
}

/// The nodes of a model plus the ones from JSON: a JSON node with the name of a model node replaces it (the model's place in the
/// list), the others follow in the order given.
pub fn combine(mut model: Vec<CrewNode>, json: &[CrewNode]) -> Vec<CrewNode> {
    for n in json {
        match model.iter_mut().find(|m| m.0 == n.0) {
            Some(m) => *m = n.clone(),
            None => model.push(n.clone()),
        }
    }
    model
}

/// `data/crew_nodes.json`: the crew places of map scenery by type (`{"<scenery type>": [{name, pos, rot?}, ...]}`). A file that does
/// not read, or a type with a bad entry, is reported by file and field and left out; the game carries on without those crews.
pub fn load_scenery(root: &std::path::Path) -> HashMap<String, Vec<CrewNode>> {
    let rel = "data/crew_nodes.json";
    let Some(mut file) = load_or_base::<BTreeMap<String, Value>>(&root.join(rel), None, "no crews on scenery from it") else { return HashMap::new() };
    file.remove("$schema"); // an editor hint (`docs/modding.md` "JSON schemas"), not a scenery type
    let mut out = HashMap::new();
    for (ty, list) in file {
        let docs = match serde_json::from_value::<Vec<CrewNodeDoc>>(list) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("error: {rel}: {ty:?}: {e}; the crews of {ty:?} are left out");
                continue;
            }
        };
        match nodes(&docs, &format!("{rel}: {ty:?}")) {
            Ok(n) => {
                out.insert(ty, n);
            }
            Err(e) => eprintln!("error: {e}; the crews of {ty:?} are left out"),
        }
    }
    out
}

/// The schema of `data/crew_nodes.json`: an object of node lists by scenery type.
pub(super) fn schema() -> Value {
    super::weapons::table_schema::<Vec<CrewNodeDoc>>("data/crew_nodes.json")
}

/// The type of a piece of map scenery (`type` in the map's `objects`), keying its entry in `data/crew_nodes.json`.
#[derive(Component)]
pub struct SceneryType(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    fn docs(v: Value) -> Vec<CrewNodeDoc> {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn entries_become_the_nodes_a_model_gives() {
        let n = nodes(&docs(serde_json::json!([{"name": "Crew_Walk_P_0", "pos": [1, 2, 3]}, {"name": "Crew_Gun_0", "pos": [0, 0, 0], "rot": [0.5, 0, 0]}])), "f").unwrap();
        assert_eq!(n[0].0, "Crew_Walk_P_0");
        assert_eq!(n[0].1.translation, point([1.0, 2.0, 3.0]));
        assert_eq!(n[0].1.rotation, Quat::IDENTITY);
        assert_eq!(n[1].1.rotation, mount_rotation([0.5, 0.0, 0.0]));
    }

    #[test]
    fn a_json_node_replaces_the_model_node_of_its_name_and_others_are_added() {
        let at = |x: f32| Transform::from_xyz(x, 0.0, 0.0);
        let model = vec![("Crew_Walk_P_0".to_string(), at(1.0)), ("Crew_Walk_P_1".to_string(), at(2.0))];
        let all = combine(model, &[("Crew_Walk_P_1".to_string(), at(9.0)), ("Crew_Post_Captain".to_string(), at(5.0))]);
        assert_eq!(all.iter().map(|n| (n.0.as_str(), n.1.translation.x)).collect::<Vec<_>>(), [("Crew_Walk_P_0", 1.0), ("Crew_Walk_P_1", 9.0), ("Crew_Post_Captain", 5.0)]);
    }

    #[test]
    fn a_bad_name_is_named_by_its_place_in_the_list() {
        for bad in ["Walk_P_0", "Crew_Dance_0", "Crew_"] {
            let e = nodes(&docs(serde_json::json!([{"name": "Crew_Walk_P_0", "pos": [0, 0, 0]}, {"name": bad, "pos": [0, 0, 0]}])), "ships/X/ship.json: crew_nodes").err().unwrap();
            assert!(e.contains("ships/X/ship.json: crew_nodes[1].name") && e.contains(bad), "{e}");
        }
    }

    #[test]
    fn scenery_nodes_load_by_type_and_a_bad_type_is_left_out() {
        let dir = std::env::temp_dir().join(format!("procyon-crew-nodes-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("data")).unwrap();
        let file = r#"{"Town": [{"name": "Crew_Walk_A_0", "pos": [0, 0, 1]}], "Pier": [{"name": "Nope", "pos": [0, 0, 0]}]}"#;
        std::fs::write(dir.join("data/crew_nodes.json"), file).unwrap();
        let loaded = load_scenery(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(loaded.keys().collect::<Vec<_>>(), ["Town"]);
        assert_eq!(loaded["Town"][0].1.translation, point([0.0, 0.0, 1.0]));
    }
}
