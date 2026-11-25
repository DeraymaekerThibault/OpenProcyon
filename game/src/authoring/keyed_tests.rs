//! The keyed merge of an `extends` file (docs/modding.md "Changing one entry of a list"), end to end through
//! `authoring::overlay_into` and `read_merged`, on synthetic JSON (no assets).
use super::*;
use serde_json::json;

/// A base with `maps/m/map.json` = `base`, one mod whose `maps/m/map.json` is `diff` (it extends its own id), built into a scratch
/// overlay. Returns the report, the merged file (if it was written) and the scratch folder to remove.
fn run(tag: &str, base: Value, diff: Value) -> (crate::modding::ModReport, Option<Value>, PathBuf) {
    let dir = std::env::temp_dir().join(format!("procyon-keyed-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (assets, m, out) = (dir.join("assets"), dir.join("mod"), dir.join("out"));
    for d in [assets.join("maps/m"), m.join("maps/m")] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(assets.join("maps/m/map.json"), base.to_string()).unwrap();
    std::fs::write(m.join("maps/m/map.json"), diff.to_string()).unwrap();
    let report = overlay_into(&assets, &[m], &out).unwrap();
    let merged = read_merged(&out, "maps/m/map.json").ok();
    (report, merged, dir)
}

fn base() -> Value {
    json!({
        "name": "Made Up",
        "objects": [
            {"id": 1, "type": "Rock", "pos": [0, 0, 0], "rot": {"yaw": 1, "pitch": 2}},
            {"id": 2, "type": "Reef", "pos": [5, 5, 0]},
            {"id": 3, "type": "Buoy", "pos": [9, 9, 0]}
        ]
    })
}

fn ids(v: &Value) -> Vec<i64> {
    v["objects"].as_array().unwrap().iter().map(|o| o["id"].as_i64().unwrap()).collect()
}

#[test]
fn changes_one_entry_and_leaves_the_rest_in_base_order() {
    let diff = json!({"extends": "m", "objects": [{"id": 2, "pos": [6, 6, 0]}]});
    let (report, merged, dir) = run("change", base(), diff);
    let merged = merged.unwrap();
    assert_eq!(merged["objects"][1], json!({"id": 2, "type": "Reef", "pos": [6, 6, 0]}), "the key and untouched fields stay");
    assert_eq!(merged["objects"][0], base()["objects"][0]);
    assert_eq!(merged["objects"][2], base()["objects"][2]);
    assert_eq!(ids(&merged), [1, 2, 3]);
    assert!(merged.get("merge").is_none() && merged.get("extends").is_none() && merged["name"] == "Made Up");
    assert!(report.mods[0].skipped.is_empty() && report.mods[0].warnings.is_empty(), "{report:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn adds_in_diff_order_and_removes_by_key() {
    let diff = json!({"extends": "m", "objects": [
        {"id": 9, "type": "Wreck"}, {"id": 1, "$remove": true}, {"id": 7, "type": "Gull"}]});
    let (_, merged, dir) = run("add-remove", base(), diff);
    let merged = merged.unwrap();
    assert_eq!(ids(&merged), [2, 3, 9, 7], "base order, then new entries as the diff lists them");
    assert!(merged["objects"].as_array().unwrap().iter().all(|o| o.get("$remove").is_none()));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_entry_removed_and_added_again_is_a_new_entry_at_the_end() {
    let diff = json!({"extends": "m", "objects": [{"id": 1, "$remove": true}, {"id": 1, "type": "Fresh"}]});
    let (_, merged, dir) = run("readd", base(), diff);
    let merged = merged.unwrap();
    assert_eq!(ids(&merged), [2, 3, 1]);
    assert_eq!(merged["objects"][2], json!({"id": 1, "type": "Fresh"}));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn nested_objects_of_an_entry_merge_deep_and_lists_inside_replace() {
    let mut b = base();
    b["objects"][0]["tags"] = json!(["a", "b"]);
    let diff = json!({"extends": "m", "objects": [{"id": 1, "rot": {"yaw": 9}, "tags": ["c"]}]});
    let (_, merged, dir) = run("deep", b, diff);
    let merged = merged.unwrap();
    assert_eq!(merged["objects"][0]["rot"], json!({"yaw": 9, "pitch": 2}));
    assert_eq!(merged["objects"][0]["tags"], json!(["c"]), "a list inside an entry is replaced as before");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn merge_false_replaces_the_whole_list_and_other_files_replace_by_default() {
    let diff = json!({"extends": "m", "merge": {"/objects": false}, "objects": [{"id": 2, "pos": [6, 6, 0]}]});
    let (report, merged, dir) = run("optin", base(), diff);
    assert_eq!(merged.unwrap()["objects"], json!([{"id": 2, "pos": [6, 6, 0]}]));
    assert!(report.mods[0].warnings.is_empty());
    // a `merge` that is not an object is plain data, as before
    // a list that is not a default one (a rule's actions) replaces, and so does any list of a file that is not a map or mission
    let diff = json!({"extends": "m", "merge": "x", "tags": ["z"]});
    let (_, merged, dir2) = run("optin2", base(), diff);
    let merged = merged.unwrap();
    assert_eq!((merged["merge"].as_str(), &merged["tags"]), (Some("x"), &json!(["z"])));
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(dir2).unwrap();
}

#[test]
fn a_removal_of_an_absent_entry_is_a_warning_naming_the_pointer_and_the_file_still_applies() {
    let diff = json!({"extends": "m", "objects": [{"id": 42, "$remove": true}, {"id": 3, "type": "Lamp"}]});
    let (report, merged, dir) = run("absent", base(), diff);
    assert_eq!(merged.unwrap()["objects"][2]["type"], "Lamp");
    let w = &report.mods[0].warnings;
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].0, "maps/m/map.json");
    assert!(w[0].1.starts_with("merges '/objects':") && w[0].1.contains("id 42"), "{w:?}");
    assert!(report.lines().iter().any(|l| l.contains("warning: maps/m/map.json: merges '/objects'")));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn misuse_skips_the_file_with_a_message_naming_the_pointer() {
    let merge = json!({"/objects": "id"});
    let cases = [
        (
            "dup",
            {
                let mut b = base();
                b["objects"][2]["id"] = json!(1);
                b
            },
            json!({"extends": "m", "merge": merge, "objects": [{"id": 1}]}),
            "the same 'id'",
        ),
        ("nokey-base", json!({"objects": [{"id": 1}, {"type": "x"}]}), json!({"extends": "m", "merge": merge, "objects": []}), "entry 1 of the list being extended has no 'id'"),
        ("nokey-diff", base(), json!({"extends": "m", "merge": merge, "objects": [{"type": "x"}]}), "entry 0 has no 'id'"),
        ("field", base(), json!({"extends": "m", "merge": {"/objects": "ident"}, "objects": [{"ident": 1}]}), "unknown key field"),
        ("field-type", base(), json!({"extends": "m", "merge": {"/objects": 3}, "objects": []}), "must be a field name"),
        ("not-list", json!({"objects": {"a": 1}}), json!({"extends": "m", "merge": merge, "objects": []}), "no list there"),
        ("diff-not-list", base(), json!({"extends": "m", "merge": merge, "objects": {"id": 1}}), "not a list"),
        ("diff-missing", base(), json!({"extends": "m", "merge": merge}), "no list there"),
        ("bad-pointer", base(), json!({"extends": "m", "merge": {"objects": "id"}}), "not a JSON pointer"),
    ];
    for (tag, b, diff, want) in cases {
        let (report, merged, dir) = run(tag, b.clone(), diff);
        let skipped = &report.mods[0].skipped;
        assert_eq!(skipped.len(), 1, "{tag}: {skipped:?}");
        assert!(skipped[0].1.contains(want) && skipped[0].1.starts_with("merges '"), "{tag}: {skipped:?}");
        assert_eq!(merged.unwrap(), b, "{tag}: the base file stays");
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn set_runs_after_the_keyed_merge_and_plain_set_files_mean_what_they_did() {
    // Without `merge`, `set` is as before.
    let diff = json!({"extends": "m", "set": {"/objects/1/type": "Cairn"}});
    let (_, merged, dir) = run("set-plain", base(), diff);
    assert_eq!(merged.unwrap()["objects"][1]["type"], "Cairn");
    std::fs::remove_dir_all(dir).unwrap();
    // With it, the pointer counts the list after the keyed merge: object 1 is gone, so index 1 is object 3.
    let diff = json!({"extends": "m", "objects": [{"id": 1, "$remove": true}, {"id": 8, "type": "New"}],
        "set": {"/objects/1/type": "Cairn", "/objects/2/type": "Moved"}});
    let (_, merged, dir) = run("set-keyed", base(), diff);
    let merged = merged.unwrap();
    assert_eq!(ids(&merged), [2, 3, 8]);
    assert_eq!(merged["objects"][1]["type"], "Cairn");
    assert_eq!(merged["objects"][2]["type"], "Moved", "set reaches an entry the keyed merge appended");
    assert!(merged.get("set").is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_mission_merges_its_rules_by_name_and_ships_by_id_without_saying_so() {
    let dir = std::env::temp_dir().join(format!("procyon-keyed-mission-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (assets, m, out) = (dir.join("assets"), dir.join("mod"), dir.join("out"));
    for d in [assets.join("maps/m"), m.join("maps/m"), assets.join("data"), m.join("data")] {
        std::fs::create_dir_all(d).unwrap();
    }
    let base = json!({"ships": [{"id": 1, "type": "A"}, {"id": 2, "type": "B"}], "rules": [{"name": "Start", "actions": [1, 2]}, {"name": "End", "actions": [3]}]});
    std::fs::write(assets.join("maps/m/mission.json"), base.to_string()).unwrap();
    std::fs::write(
        m.join("maps/m/mission.json"),
        json!({"extends": "m", "rules": [{"name": "End", "actions": [9]}, {"name": "Start", "$remove": true}, {"name": "Late", "actions": []}]}).to_string(),
    )
    .unwrap();
    // another file keeps the whole-list rule, even with the same shape
    std::fs::write(assets.join("data/a.json"), base.to_string()).unwrap();
    std::fs::write(m.join("data/a.json"), json!({"extends": "a", "rules": [{"name": "End"}]}).to_string()).unwrap();
    overlay_into(&assets, &[m], &out).unwrap();
    let merged = read_merged(&out, "maps/m/mission.json").unwrap();
    assert_eq!(merged["rules"], json!([{"name": "End", "actions": [9]}, {"name": "Late", "actions": []}]), "a rule's own actions list replaces");
    assert_eq!(merged["ships"], base["ships"], "ships untouched: not in the diff");
    assert_eq!(read_merged(&out, "data/a.json").unwrap()["rules"], json!([{"name": "End"}]));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_extending_mission_changes_one_task_by_name_and_repeated_names_are_refused() {
    let tasks = json!([{"Name": "Reach", "text": "Reach the gate", "Active": true}, {"Name": "Clear", "text": "Clear it", "Active": false}]);
    let diff = json!({"extends": "m", "tasks": [{"Name": "Clear", "required": true, "complete_when": [{"Type": "Group Destroyed", "Group Name": "Raiders"}]}, {"Name": "Reach", "$remove": true}, {"Name": "Escape", "text": "Leave"}]});
    let (merged, warnings) = crate::authoring::extend("maps/m/mission.json", json!({"tasks": tasks}), &diff).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(merged["tasks"][0]["Name"], "Clear");
    assert_eq!((merged["tasks"][0]["text"].as_str(), merged["tasks"][0]["required"].as_bool()), (Some("Clear it"), Some(true)), "an entry of the parent is merged, not replaced");
    assert_eq!(merged["tasks"].as_array().unwrap().len(), 2);
    assert_eq!(merged["tasks"][1]["Name"], "Escape", "a new name is appended");
    assert!(crate::authoring::keyed_lists("maps/m/mission.json").contains(&("/tasks", "Name")));
    assert!(crate::authoring::keyed_lists("maps/m/script.json").iter().all(|(p, _)| *p != "/tasks"), "a skirmish script keeps its tasks whole");
    // a parent that repeats a task name cannot be keyed: the error says which entries, and `merge: false` is the way out
    let twice = json!({"tasks": [{"Name": "A"}, {"Name": "B"}, {"Name": "A"}]});
    let err = crate::authoring::extend("maps/m/mission.json", twice.clone(), &diff).unwrap_err();
    assert!(err.contains("merges '/tasks'") && err.contains("same 'Name'"), "{err}");
    let whole = json!({"extends": "m", "merge": {"/tasks": false}, "tasks": [{"Name": "Only"}]});
    assert_eq!(crate::authoring::extend("maps/m/mission.json", twice, &whole).unwrap().0["tasks"], json!([{"Name": "Only"}]));
}

#[test]
fn string_keys_and_a_missing_base_list_work() {
    let b = json!({"rules": [{"name": "A", "n": 1}, {"name": "B", "n": 2}]});
    let diff =
        json!({"extends": "m", "merge": {"/rules": "name", "/extra/list": "k"}, "rules": [{"name": "B", "n": 3}, {"name": "A", "$remove": true}], "extra": {"list": [{"k": "x"}]}});
    let (report, merged, dir) = run("strings", b, diff);
    let merged = merged.unwrap();
    assert_eq!(merged["rules"], json!([{"name": "B", "n": 3}]));
    assert_eq!(merged["extra"]["list"], json!([{"k": "x"}]), "no list in the base: the entries are all new");
    assert!(report.mods[0].skipped.is_empty(), "{report:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn check_mod_reports_misuse_with_file_and_pointer() {
    let dir = std::env::temp_dir().join(format!("procyon-keyed-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let m = dir.join("mod");
    std::fs::create_dir_all(m.join("data")).unwrap();
    std::fs::write(m.join("data/a.json"), r#"{"x": 1}"#).unwrap();
    std::fs::write(m.join("data/b.json"), r#"{"extends": "a", "merge": {"/list": "id"}, "list": [{"id": 1, "$remove": true}]}"#).unwrap();
    std::fs::write(m.join("data/c.json"), r#"{"extends": "a", "merge": {"/list": "id"}, "list": [{"n": 1}]}"#).unwrap();
    let found = check_mod_with(&m, None, &[]);
    let at = |file: &str| found.iter().find(|f| f.file == file).unwrap_or_else(|| panic!("{file}: {found:#?}"));
    let (warn, error) = (at("data/b.json"), at("data/c.json"));
    assert_eq!((warn.level, warn.path.as_str(), warn.pointer.as_str()), (Level::Warning, "list", "/list"), "{warn:?}");
    assert!(warn.message.contains("id 1"));
    assert_eq!((error.level, error.path.as_str(), error.pointer.as_str()), (Level::Error, "list", "/list"), "{error:?}");
    assert!(error.message.contains("entry 0 has no 'id'"));
    assert!(!found.iter().any(|f| f.message.contains("unknown key 'merge'")), "{found:#?}");
    std::fs::remove_dir_all(dir).unwrap();
}

/// The first retail skirmish map, one object moved by a keyed diff; every other object is the same. Needs the player's import:
/// `cargo test --manifest-path game/Cargo.toml keyed_diff_over_a_retail_map -- --ignored`
#[test]
#[ignore = "needs imported assets"]
fn keyed_diff_over_a_retail_map_moves_one_object_and_leaves_the_rest() {
    let assets = base_assets().expect("imported assets (make assets)");
    let mut maps: Vec<String> = std::fs::read_dir(assets.join("maps")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    maps.sort();
    let (id, original) = maps
        .iter()
        .filter_map(|id| read_map(&assets, id).ok().map(|m| (id.clone(), m)))
        .find(|(_, m)| m["skirmish"] == true && m["objects"].as_array().is_some_and(|o| o.len() > 1))
        .expect("a skirmish map with objects");
    let objects = original["objects"].as_array().unwrap();
    let target = objects[objects.len() / 2].clone();
    let moved = json!([target["pos"][0].as_f64().unwrap() + 25.0, target["pos"][1], target["pos"][2]]);
    let dir = std::env::temp_dir().join(format!("procyon-keyed-retail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let m = dir.join("mod");
    std::fs::create_dir_all(m.join(format!("maps/{id}"))).unwrap();
    let diff = json!({"extends": id, "objects": [{"id": target["id"], "pos": moved}]});
    std::fs::write(m.join(format!("maps/{id}/map.json")), diff.to_string()).unwrap();
    let report = overlay_into(&assets, &[m], &dir.join("out")).unwrap();
    assert!(report.mods[0].skipped.is_empty() && report.mods[0].warnings.is_empty(), "{report:?}");
    let merged = read_map(&dir.join("out"), &id).unwrap();
    let mut expected = original.clone();
    expected["objects"][objects.len() / 2]["pos"] = moved;
    assert_eq!(merged, expected, "map {id}: only the one object's pos differs");
    std::fs::remove_dir_all(dir).unwrap();
}
