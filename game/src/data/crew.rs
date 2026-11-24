//! Typed crew roster (`data/crew.json`: id -> race, species, name, points, skills, portrait; layout: docs/data-format.md).
//! The file is optional (no named crew without it). Each entry must be an object; its fields were read leniently before
//! (`as_str`, `as_u64`, `as_f64` with a default) and still are, so valid data behaves exactly as before. A file that does not
//! parse gives one `error:` line naming the file, its mod and the entry, then the base file, else no named crew. Unknown fields allowed.
use super::weapons::lenient;
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A skill's ability value (0..10); anything that is not a number reads as absent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Rating(pub Option<f64>);
impl<'de> Deserialize<'de> for Rating {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        lenient(d).map(Rating)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct CrewMember {
    #[serde(default, deserialize_with = "lenient")]
    pub race: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub species: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub name: Option<String>,
    /// Victory-point price; a crew member without one cannot be recruited.
    #[serde(default, deserialize_with = "lenient")]
    pub points: Option<u64>,
    /// `TalkingHead_Character_*` sprite.
    #[serde(default, deserialize_with = "lenient")]
    pub portrait: Option<String>,
    /// Active skills only, by lower-case name (leadership, navigation, spotting, engineering, rigging, combat, gunnery).
    #[serde(default, deserialize_with = "lenient")]
    pub skills: Option<BTreeMap<String, Rating>>,
}

impl CrewMember {
    /// The ability value of `skill` (0..10), when the character has it.
    pub fn rating(&self, skill: &str) -> Option<f64> {
        self.skills.as_ref()?.get(skill)?.0
    }
}

/// The roster of a parsed file; an entry that is not an object names itself.
pub(super) fn from_value(doc: Value) -> Result<BTreeMap<String, CrewMember>, String> {
    let Value::Object(entries) = doc else { return Err("the file must be an object of crew members".into()) };
    entries.into_iter().map(|(id, v)| serde_json::from_value(v).map(|m| (id.clone(), m)).map_err(|e| format!("crew member {id}: {e}"))).collect()
}

/// `data/crew.json` as typed entries, through the same file-or-base fallback as the other optional data (`load_asset`).
pub fn roster() -> &'static BTreeMap<String, CrewMember> {
    static ROSTER: std::sync::OnceLock<BTreeMap<String, CrewMember>> = std::sync::OnceLock::new();
    ROSTER.get_or_init(|| {
        load_asset::<Value>("data/crew.json", "no named crew")
            .and_then(|v| from_value(v).map_err(|e| eprintln!("error: data/crew.json: {e}; no named crew")).ok())
            .unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_member_reads_as_before_and_unknown_fields_are_allowed() {
        let r = from_value(json!({"Crew_A": {"race": "Navy", "species": "Human_Male", "name_id": "X1", "name": "Ann", "points": 12, "portrait": "TalkingHead_A",
            "skills": {"gunnery": 8, "combat": "high", "rigging": 2.5}, "extra": [1]}}))
        .unwrap();
        let a = &r["Crew_A"];
        assert_eq!(
            (a.race.as_deref(), a.species.as_deref(), a.name.as_deref(), a.points, a.portrait.as_deref()),
            (Some("Navy"), Some("Human_Male"), Some("Ann"), Some(12), Some("TalkingHead_A"))
        );
        assert_eq!((a.rating("gunnery"), a.rating("rigging"), a.rating("combat"), a.rating("spotting")), (Some(8.0), Some(2.5), None, None));
    }

    #[test]
    fn fields_of_the_wrong_type_read_as_absent_like_the_untyped_reader() {
        let r = from_value(json!({"Crew_B": {"race": 3, "points": "cheap", "name": null, "skills": 7}, "Crew_C": {}})).unwrap();
        assert_eq!(r["Crew_B"], CrewMember::default());
        assert_eq!(r["Crew_C"], CrewMember::default());
        assert_eq!(r["Crew_B"].rating("gunnery"), None);
    }

    #[test]
    fn an_entry_that_is_not_an_object_or_a_file_that_is_not_a_map_names_itself() {
        let e = from_value(json!({"Crew_D": 5})).unwrap_err();
        assert!(e.contains("crew member Crew_D"), "{e}");
        assert!(from_value(json!([])).unwrap_err().contains("must be an object"));
    }

    #[test]
    fn an_unparsable_file_gives_a_named_error() {
        let dir = std::env::temp_dir().join(format!("procyon-crew-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("crew.json"), "not json").unwrap();
        let e = load::<Value>(&dir.join("crew.json")).unwrap_err().to_string();
        assert!(e.contains("crew.json"), "{e}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Typed and untyped agree on the imported roster: the same entries, and every accessor of the old reader gives the same answer.
    #[test]
    #[ignore = "needs imported assets"]
    fn the_typed_roster_equals_the_untyped_reader_on_retail_and_mod_data() {
        let Some(retail) = try_json(&crate::assets_dir().join("data/crew.json")) else {
            eprintln!("skipped: no imported data/crew.json");
            return;
        };
        let check = |doc: &Value, what: &str| {
            let typed = from_value(doc.clone()).unwrap_or_else(|e| panic!("{what}: {e}"));
            assert_eq!(typed.len(), doc.as_object().unwrap().len(), "{what}");
            for (id, m) in &typed {
                let d = &doc[id];
                assert_eq!(m.species.as_deref(), d["species"].as_str(), "{what} {id}");
                assert_eq!(m.race.as_deref(), d["race"].as_str(), "{what} {id}");
                assert_eq!(m.name.as_deref(), d["name"].as_str(), "{what} {id}");
                assert_eq!(m.points, d["points"].as_u64(), "{what} {id}");
                assert_eq!(m.portrait.as_deref(), d["portrait"].as_str(), "{what} {id}");
                for skill in ["leadership", "navigation", "spotting", "engineering", "rigging", "combat", "gunnery"] {
                    assert_eq!(m.rating(skill), d["skills"][skill].as_f64(), "{what} {id} {skill}");
                }
                // The Value the other crew screens still read is the typed entry's own serialisation.
                let v = serde_json::to_value(m).unwrap();
                assert_eq!(
                    (v["race"].as_str(), v["points"].as_u64(), v["portrait"].as_str(), v["name"].as_str()),
                    (d["race"].as_str(), d["points"].as_u64(), d["portrait"].as_str(), d["name"].as_str()),
                    "{what} {id}"
                );
            }
        };
        check(&retail, "retail");
        assert!(!retail.as_object().unwrap().is_empty());
        let mods = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mods");
        for m in std::fs::read_dir(mods).unwrap().flatten() {
            let Some(over) = try_json(&m.path().join("data/crew.json")) else { continue };
            let mut merged = retail.clone();
            crate::modding::merge(&mut merged, &over);
            check(&merged, &m.file_name().to_string_lossy());
        }
    }
}
