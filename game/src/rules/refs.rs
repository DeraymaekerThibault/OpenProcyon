//! `--check-mission` and the load-time check also look at what a mission points at: ship classes, sound files and the backdrop
//! picture. The lookups are passed in (the game's own asset lookup in `cli.rs`), so the check itself needs no assets.

use serde_json::Value;

use super::check::{Level, Problem};

/// Whether a thing a mission names exists. A lookup that is `None` is skipped (no imported assets to look in).
pub struct Lookups<'a> {
    /// A ship class id (`ships/<id>/ship.json`).
    pub ship: Option<&'a dyn Fn(&str) -> bool>,
    /// A sound by name, found like the game finds it (`audio::sound`).
    pub sound: Option<&'a dyn Fn(&str) -> bool>,
    /// A picture in the mission's own folder (`backdrop.png`).
    pub picture: Option<&'a dyn Fn(&str) -> bool>,
}

/// Whether a `Play Music Track` file name means "stop the music": `None`, with or without `.ogg`, in any case (sounds are looked up
/// case-insensitively, see `audio::sound`). The player and the mission check both use this, so they cannot disagree.
pub fn is_no_music(file: &str) -> bool {
    let name = file.strip_suffix(".ogg").or_else(|| file.strip_suffix(".OGG")).unwrap_or(file);
    name.eq_ignore_ascii_case("none")
}

/// Ship classes named `<class>_Wrecked` are scenery the spawner places by their TWT group, not ship classes with a `ship.json`.
pub fn is_wrecked_hull(class: &str) -> bool {
    class.ends_with("_Wrecked")
}

fn problem(rule: &str, at: String, field: &str, message: String) -> Problem {
    Problem::new(Level::Error, rule, at, field, message)
}

/// `problem` for action `item` of rule `index`.
fn in_action(index: usize, rule: &Value, item: usize, mut p: Problem) -> Problem {
    p.rule_index = Some(index);
    p.rule_name = rule["name"].as_str().filter(|n| !n.is_empty()).map(|n| n.trim().to_string());
    p.list = Some("actions".into());
    p.item = Some(item);
    p
}

/// Errors for every ship class, `.ogg` sound and `.png` picture the mission, its `map.json` and its `media.json` name that does not exist.
pub fn check_references(mission: &Value, map: &Value, media: &Value, look: &Lookups) -> Vec<Problem> {
    let mut out = vec![];
    if let Some(ship) = look.ship {
        for (i, s) in mission["ships"].as_array().into_iter().flatten().enumerate() {
            if let Some(class) = s["type"].as_str().filter(|c| !is_wrecked_hull(c) && !ship(c)) {
                let name = s["name"].as_str().filter(|n| !n.is_empty()).map_or(String::new(), |n| format!(" '{n}'"));
                let mut p = problem("(mission)", format!("ship #{}{name}", i + 1), "type", format!("ship class '{class}' does not exist (no ships/{class}/ship.json)"));
                p.set_path(super::check::JsonPath::from("ships").index(i).key("type"));
                out.push(p);
            }
        }
    }
    if let Some(sound) = look.sound {
        for (i, r) in mission["rules"].as_array().into_iter().flatten().enumerate() {
            let rule = r["name"].as_str().map_or_else(|| format!("#{}", i + 1), |n| n.trim().to_string());
            for (j, a) in r["actions"].as_array().into_iter().flatten().enumerate() {
                let ty = a["Type"].as_str().unwrap_or_default();
                let at = format!("action #{} '{ty}'", j + 1);
                match ty {
                    "Play Music Track" | "Play Special Effect" => {
                        let silence = ty == "Play Music Track";
                        if let Some(file) = a["File Name"].as_str().filter(|f| !f.is_empty() && !(silence && is_no_music(f)) && !sound(f)) {
                            out.push(in_action(i, r, j, problem(&rule, at, "File Name", format!("the sound '{file}.ogg' is not in any sound folder of the game or the mods"))));
                        }
                    }
                    "Play speech event" => {
                        let name = a["Speech Event Name"].as_str().unwrap_or_default();
                        let entry = &media["speech"][name];
                        // Like the interpreter: an entry without a duration is subtitle text only, with no sound.
                        if entry["duration"].as_f64().is_some() {
                            let file = entry["file"].as_str().unwrap_or(name);
                            if !sound(file) {
                                out.push(in_action(
                                    i,
                                    r,
                                    j,
                                    problem(
                                        &rule,
                                        at,
                                        "Speech Event Name",
                                        format!("the speech file '{file}.ogg' of '{name}' (media.json) is not in any sound or maps/<id>/speech folder"),
                                    ),
                                ));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if let Some(picture) = look.picture
        && (map["backdrop"] == true || map["backdrop"].as_str().is_some_and(|s| !s.is_empty()))
        && !picture("backdrop.png")
    {
        out.push(problem("(mission)", "files".into(), "backdrop", "map.json asks for a backdrop but the map folder has no backdrop.png".into()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(mission: &Value, map: &Value, media: &Value) -> Vec<String> {
        let ship = |c: &str| c == "Ship_Real";
        let sound = |f: &str| f == "real_track" || f == "real_line";
        let picture = |_: &str| false;
        let look = Lookups { ship: Some(&ship), sound: Some(&sound), picture: Some(&picture) };
        check_references(mission, map, media, &look).iter().map(ToString::to_string).collect()
    }

    #[test]
    fn missing_ship_classes_sounds_and_backdrops_are_errors_naming_their_field() {
        let mission = json!({
            "ships": [{"id": 1, "type": "Ship_Real", "name": "A"}, {"id": 2, "type": "Ship_Ghost", "name": "B"}],
            "rules": [
                {"name": "Music", "actions": [{"Type": "Play Music Track", "File Name": "real_track"}, {"Type": "Play Music Track", "File Name": "lost_track"}]},
                {"name": "Talk", "actions": [{"Type": "Play speech event", "Speech Event Name": "HELLO"}, {"Type": "Play speech event", "Speech Event Name": "TEXT"}]},
            ]
        });
        let media = json!({"speech": {"HELLO": {"duration": 2.0, "file": "lost_line"}, "TEXT": {"text": "no sound"}}});
        let lines = run(&mission, &json!({"backdrop": true}), &media);
        let has = |needle: &[&str]| lines.iter().any(|l| l.starts_with("ERROR") && needle.iter().all(|n| l.contains(n)));
        assert!(has(&["ship #2 'B'", "field \"type\"", "Ship_Ghost"]), "{lines:?}");
        assert!(has(&["rule \"Music\"", "action #2 'Play Music Track'", "File Name", "lost_track.ogg"]), "{lines:?}");
        assert!(has(&["rule \"Talk\"", "action #1 'Play speech event'", "Speech Event Name", "lost_line.ogg"]), "{lines:?}");
        assert!(has(&["backdrop.png"]), "{lines:?}");
        assert_eq!(lines.len(), 4, "the existing ship and sound and the text-only speech are fine: {lines:?}");
    }

    #[test]
    fn none_music_and_wrecked_hulls_are_not_missing_files() {
        let mission = json!({
            "ships": [{"type": "Galleon_Wrecked", "name": "W"}, {"type": "Ship_Ghost", "name": "G"}],
            "rules": [{"name": "M", "actions": [
                {"Type": "Play Music Track", "File Name": "None"},
                {"Type": "Play Music Track", "File Name": "None.ogg"},
                {"Type": "Play Music Track", "File Name": "none"},
                {"Type": "Play Music Track", "File Name": "lost_track"},
                {"Type": "Play Special Effect", "File Name": "None"},
            ]}]
        });
        let lines = run(&mission, &json!({}), &Value::Null);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("Ship_Ghost")) && lines.iter().any(|l| l.contains("lost_track")), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("Play Special Effect")), "only music may be None: {lines:?}");
    }

    #[test]
    fn without_lookups_nothing_is_checked() {
        let mission = json!({"ships": [{"type": "Ship_Ghost"}], "rules": [{"actions": [{"Type": "Play Music Track", "File Name": "x"}]}]});
        let none = Lookups { ship: None, sound: None, picture: None };
        assert!(check_references(&mission, &json!({"backdrop": true}), &Value::Null, &none).is_empty());
    }
}
