//! Fleet, crew and arms profiles: what the skirmish screens' Load / Save buttons read and write. One JSON file per profile in
//! `<user data dir>/profiles/{fleets,crews,arms}/<name>.json`: `{"ships": [{"class", "crew": [8 post lists], "arms": {"bank": "gun"}}]}`.
//! A fleet file holds every ship with its crew and guns; a crew or arms file holds the one ship it was saved from (and only that part).
//! The original keeps three binary files per fleet; this format is ours. PROVISIONAL: which files the lists offer (a ship's crew and
//! arms by hull class, a fleet by race) is not measured.
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::data::Library;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Fleet,
    Crew,
    Arms,
}

impl Kind {
    fn folder(self) -> &'static str {
        match self {
            Kind::Fleet => "fleets",
            Kind::Crew => "crews",
            Kind::Arms => "arms",
        }
    }
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct ShipProfile {
    pub class: String,
    pub crew: [Vec<String>; 8],
    /// Guns swapped in, by bank.
    pub arms: BTreeMap<usize, String>,
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct Profile {
    pub name: String,
    pub ships: Vec<ShipProfile>,
}

impl Profile {
    pub fn to_json(&self, kind: Kind) -> Value {
        let ships: Vec<Value> = self
            .ships
            .iter()
            .map(|s| {
                let mut v = json!({ "class": s.class });
                if kind != Kind::Arms {
                    v["crew"] = json!(s.crew);
                }
                if kind != Kind::Crew {
                    v["arms"] = Value::Object(s.arms.iter().map(|(b, g)| (b.to_string(), json!(g))).collect());
                }
                v
            })
            .collect();
        json!({ "ships": ships })
    }

    pub fn from_json(name: &str, v: &Value) -> Option<Self> {
        let ships = v["ships"].as_array()?.iter().map(ship_from_json).collect::<Option<Vec<_>>>()?;
        (!ships.is_empty()).then(|| Self { name: name.into(), ships })
    }

    /// Victory points the profile's ships cost: hulls, gun swaps and named crew.
    pub fn points(&self, lib: &Library) -> i64 {
        self.ships
            .iter()
            .filter_map(|s| {
                let class = lib.classes.iter().find(|c| c.name == s.class)?;
                let crew: u32 = s.crew.iter().flatten().map(|id| crate::crew_arms::crew_points(id)).sum();
                Some(i64::from(class.points) + crate::crew_arms::fitting_delta(lib, class, &s.arms) + i64::from(crew))
            })
            .sum()
    }
}

fn ship_from_json(v: &Value) -> Option<ShipProfile> {
    let mut ship = ShipProfile { class: v["class"].as_str()?.into(), ..Default::default() };
    for (post, list) in ship.crew.iter_mut().zip(v["crew"].as_array().into_iter().flatten()) {
        *post = list.as_array()?.iter().filter_map(|id| id.as_str().map(String::from)).collect();
    }
    for (bank, gun) in v["arms"].as_object().into_iter().flatten() {
        ship.arms.insert(bank.parse().ok()?, gun.as_str()?.into());
    }
    Some(ship)
}

fn dir(kind: Kind) -> Option<PathBuf> {
    Some(crate::campaign::user_dir()?.join("profiles").join(kind.folder()))
}

/// Every readable profile of `kind`, by name.
pub fn list(kind: Kind) -> Vec<Profile> {
    let mut all: Vec<Profile> = dir(kind)
        .and_then(|d| std::fs::read_dir(d).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.path().file_stem()?.to_str()?.to_string();
            Profile::from_json(&name, &crate::data::try_json(&e.path())?)
        })
        .collect();
    all.sort_by_key(|a| a.name.to_lowercase());
    all
}

pub fn write(kind: Kind, p: &Profile) -> Result<(), String> {
    let name = crate::campaign::clean_name(&p.name);
    let dir = dir(kind).filter(|_| !name.is_empty()).ok_or("invalid profile name")?;
    let (path, temp) = (dir.join(format!("{name}.json")), dir.join(format!("{name}.tmp")));
    std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(&temp, serde_json::to_string_pretty(&p.to_json(kind)).unwrap()))
        .and_then(|()| std::fs::rename(&temp, &path))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_keeps_only_the_part_of_its_kind() {
        let mut ship = ShipProfile { class: "Ship_Navy_ManOWar".into(), ..Default::default() };
        ship.crew[0] = vec!["Crew_Navy_AlexG".into()];
        ship.arms.insert(3, "Gun_Harpoon_Light".into());
        let p = Profile { name: "x".into(), ships: vec![ship] };
        assert_eq!(Profile::from_json("x", &p.to_json(Kind::Fleet)), Some(p.clone()));
        let crew = Profile::from_json("x", &p.to_json(Kind::Crew)).unwrap();
        assert_eq!((crew.ships[0].crew[0].len(), crew.ships[0].arms.len()), (1, 0));
        let arms = Profile::from_json("x", &p.to_json(Kind::Arms)).unwrap();
        assert_eq!((arms.ships[0].crew[0].len(), arms.ships[0].arms.len()), (0, 1));
        assert_eq!(Profile::from_json("x", &json!({ "ships": [] })), None);
    }
}
