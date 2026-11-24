//! A ship as the game resolves it: [`resolved_ship`] loads `ships/<id>/ship.json` of a root exactly as the game does (the same loader,
//! the same model derivation, the guns of the same root) and returns what a modder wants to see, each value with where it came from.
use std::collections::BTreeMap;
use std::path::Path;

use bevy::math::{EulerRot, Vec3};
use serde::Serialize;

use super::{bevy_to_game, mount_rot};
use crate::data::{CREW_POSTS, Class, Fill, derive, fill_of, glb_json};

/// Where a resolved value came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The ship.json chain (the file, with its `extends` and mod layers merged) sets it.
    File,
    /// Derived from the model's nodes and geometry (the ship needs a readable `model.glb`).
    Model,
    /// A race and size choice, the Navy Cutter numbers or a fixed default.
    Default,
}

/// One hull section.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResolvedSection {
    pub label: String,
    pub layer: String,
    pub hp: f64,
}

/// One gun mount; `pos` and `rot` are as the file holds them (game axes).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResolvedGun {
    /// The mount's label (its key in `guns`).
    pub name: String,
    /// The gun fitted: a key of `guns.json`.
    pub gun: String,
    pub pos: [f64; 3],
    pub rot: [f64; 3],
    pub arc_deg: f64,
    /// The weapon-panel slot (`BankNN` is bank NN; 0 when none was given or derived).
    pub bank: usize,
    /// The label of the section the mount sits on.
    pub section: Option<String>,
}

/// One engine port.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResolvedEngine {
    pub pos: [f64; 3],
    /// Yaw about the up axis, as the first number of `rot`.
    pub yaw: f64,
    pub fx: String,
    pub section: Option<String>,
}

/// One wake emitter.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResolvedWake {
    pub pos: [f64; 3],
    pub fx: String,
}

/// Where the flag pole's ends are.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResolvedFlag {
    pub top: [f64; 3],
    pub bottom: [f64; 3],
}

/// Posts of one crew role.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResolvedPost {
    pub role: String,
    pub count: u32,
}

/// What a modder sees of a ship after the game's own loader and derivations (positions in the files' game axes).
///
/// `sources` says where each value came from, keyed by the `ship.json` key (`core_hp`, `physics`, `sections`, `guns`, `engines`,
/// `wakes`, `flags`, `crew_posts`, `tow`, `homing`, `sphere`, `explosion_fx`, `repair_fx`, `engine_snd`, `sight`, `race`, `skirmish`,
/// `points`) and, for what a file may state per entry, by JSON pointer: `/physics/Mass`, `/physics/MaxThrust`, `/physics/MaxSpeed`,
/// `/guns/<label>/pos`, `/guns/<label>/rot`, `/guns/<label>/bank`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResolvedShip {
    pub id: String,
    pub race: String,
    pub skirmish: bool,
    pub points: u32,
    pub max_speed: f64,
    pub max_thrust: f64,
    pub mass: f64,
    pub sight: f64,
    pub core_hp: f64,
    pub sections: Vec<ResolvedSection>,
    /// The sum of the sections' hit points.
    pub section_hp: f64,
    pub guns: Vec<ResolvedGun>,
    pub engines: Vec<ResolvedEngine>,
    pub wakes: Vec<ResolvedWake>,
    /// `None` when the hull has no flag pole.
    pub flags: Option<ResolvedFlag>,
    pub crew_posts: Vec<ResolvedPost>,
    pub tower: [f64; 3],
    pub towee: [f64; 3],
    pub homing: [f64; 3],
    pub explosion_fx: Option<String>,
    pub repair_fx: Option<String>,
    /// The engine loops for Half, Full and Emergency.
    pub engine_snd: [Option<String>; 3],
    pub sources: BTreeMap<String, Source>,
}

fn game(v: Vec3) -> [f64; 3] {
    bevy_to_game(v)
}

/// The ship `id` of `root` (the base assets or a folder `overlay_into` built) as the game loads it. The loader is
/// `data::try_load_class` with the guns of `data/weapons` of the same root, so an error is the loader's own and names the file.
///
/// `sources` is read off the merged `ships/<id>/ship.json` and what the loader does for a key left out (the `SHIP_FILLS` table of
/// `data/ships.rs`): a key the file states is `file`; a missing one is `model` when its fill is derived from the model and the ship has
/// a readable `model.glb` (for `tow` / `homing` only when the model carries the node), else `default`. PROVISIONAL: it is a reading of
/// the file against that table, not a record the loader keeps, so a derivation the loader skips for another reason still reads `model`.
pub fn resolved_ship(root: &Path, id: &str) -> Result<ResolvedShip, String> {
    let (guns, _) = crate::data::try_load_weapons(root)?;
    let class = crate::data::try_load_class(root, id, &guns)?;
    let doc = super::read_merged(root, &format!("ships/{id}/ship.json")).map_err(|e| e.to_string())?;
    let model = doc["model"].as_str().unwrap_or(id);
    let bytes = std::fs::read(root.join(format!("ships/{model}/model.glb"))).unwrap_or_default();
    let has_model = derive::read(&bytes).is_some();
    let glb = glb_json(&bytes).map(|(j, _)| j);
    let has_node = |kinds: &[&str]| glb.as_ref().and_then(|j| j["nodes"].as_array()).is_some_and(|ns| ns.iter().any(|n| kinds.iter().any(|k| n["extras"]["kind"] == *k)));
    let source = |present: bool, key: &str| match (present, fill_of(key)) {
        (true, _) => Source::File,
        (false, Some(Fill::Model | Fill::WeaponPanel)) if has_model => Source::Model,
        (false, Some(Fill::Hull)) if has_node(&["TowerPoint", "ToweePoint", "TorpedoHomingPoint"]) => Source::Model,
        _ => Source::Default,
    };
    let mut sources = BTreeMap::new();
    for key in [
        "race",
        "skirmish",
        "points",
        "sight",
        "core_hp",
        "physics",
        "sections",
        "guns",
        "engines",
        "wakes",
        "flags",
        "crew_posts",
        "sphere",
        "explosion_fx",
        "repair_fx",
        "engine_snd",
        "tow",
        "homing",
    ] {
        sources.insert(key.to_string(), source(!doc[key].is_null(), key));
    }
    // `tow` and `homing` each look for their own node.
    sources.insert("tow".into(), if doc["tow"].is_null() && has_node(&["TowerPoint", "ToweePoint"]) { Source::Model } else { source(!doc["tow"].is_null(), "tow") });
    sources.insert("homing".into(), if doc["homing"].is_null() && has_node(&["TorpedoHomingPoint"]) { Source::Model } else { source(!doc["homing"].is_null(), "homing") });
    for key in ["Mass", "MaxThrust", "MaxSpeed"] {
        sources.insert(format!("/physics/{key}"), source(!doc["physics"][key].is_null(), &format!("physics.{key}")));
    }
    // Mounts keep the order of the file's `guns` object (the loader's order).
    let labels: Vec<&String> = doc["guns"].as_object().map(|m| m.keys().collect()).unwrap_or_default();
    let mut gun_list = vec![];
    for (i, m) in class.mounts.iter().enumerate() {
        let label = labels.get(i).map_or_else(|| format!("#{i}"), |l| l.to_string());
        for (field, fill) in [("pos", "guns.pos"), ("rot", "guns.rot"), ("bank", "guns.bank")] {
            sources.insert(format!("/guns/{label}/{field}"), source(!doc["guns"][label.as_str()][field].is_null(), fill));
        }
        gun_list.push(ResolvedGun {
            name: label,
            gun: m.gun.clone(),
            pos: game(m.pos),
            rot: mount_rot(m.rot),
            arc_deg: m.arc.to_degrees() as f64,
            bank: m.bank,
            section: m.parent.and_then(|s| class.sections.get(s)).map(|s| s.label.clone()),
        });
    }
    Ok(assemble(id, &class, gun_list, sources))
}

fn assemble(id: &str, class: &Class, guns: Vec<ResolvedGun>, sources: BTreeMap<String, Source>) -> ResolvedShip {
    let section_of = |i: &Option<usize>| i.and_then(|i| class.sections.get(i)).map(|s| s.label.clone());
    ResolvedShip {
        id: id.to_string(),
        race: class.race.clone(),
        skirmish: class.skirmish,
        points: class.points,
        max_speed: class.phys.max_speed as f64,
        max_thrust: class.phys.thrust as f64,
        mass: class.phys.mass as f64,
        sight: class.sight as f64,
        core_hp: class.core_hp as f64,
        sections: class.sections.iter().map(|s| ResolvedSection { label: s.label.clone(), layer: s.layer.clone(), hp: s.hp as f64 }).collect(),
        section_hp: class.sections.iter().map(|s| s.hp as f64).sum(),
        guns,
        engines: class
            .engines
            .iter()
            .map(|(p, r, fx, s)| ResolvedEngine { pos: game(*p), yaw: r.to_euler(EulerRot::YXZ).0 as f64, fx: fx.clone(), section: section_of(s) })
            .collect(),
        wakes: class.wake.iter().map(|(p, fx)| ResolvedWake { pos: game(*p), fx: fx.clone() }).collect(),
        flags: class.flag.map(|(top, bottom)| ResolvedFlag { top: game(top), bottom: game(bottom) }),
        crew_posts: CREW_POSTS.iter().zip(class.crew).map(|(role, count)| ResolvedPost { role: role.to_string(), count }).collect(),
        tower: game(class.tow_points[0]),
        towee: game(class.tow_points[1]),
        homing: game(class.homing_point),
        explosion_fx: class.explosion_fx.clone(),
        repair_fx: class.repair_fx.clone(),
        engine_snd: class.engine_snd.clone(),
        sources,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::derive::tests::model;
    use serde_json::{Value, json};

    fn root(tag: &str, ship: Value, with_model: bool) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("procyon-resolved-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("ships/Test_Ship")).unwrap();
        std::fs::create_dir_all(root.join("data/weapons")).unwrap();
        let gun = json!({"Gun": {"MuzzleSpeed": 1, "TimeBetweenBursts": 1, "BulletsPerBurst": 1, "DuringBurstReloadTime": 1, "EffectiveRange_Range": 100, "LongRange_Range": 1,
            "MaximumRange_Range": 1, "EffectiveRange_AccuracyDeviation": 1, "LongRange_AccuracyDeviation": 1, "MaximumRange_AccuracyDeviation": 1, "BulletAffectedByGravity": false,
            "BulletTypeName_Primary": "B", "visual": {"pivot": [0, 0, 0], "muzzles": [[0, 0, 0]]}, "HorizontalRotationSpeed": 1, "VerticalRotationSpeed": 1, "VerticalMinAngle": 0, "VerticalMaxAngle": 1}});
        std::fs::write(root.join("data/weapons/guns.json"), gun.to_string()).unwrap();
        std::fs::write(root.join("data/weapons/bullets.json"), "{}").unwrap();
        std::fs::write(root.join("ships/Test_Ship/ship.json"), ship.to_string()).unwrap();
        if with_model {
            std::fs::write(root.join("ships/Test_Ship/model.glb"), model()).unwrap();
        } else {
            let geometry = json!({"Hull": {"aabb": [[-1, -2, -3], [1, 2, 3]], "tris": [1, 2, 3, 4, 5, 6, 7, 8, 9]}});
            std::fs::write(root.join("ships/Test_Ship/geometry.json"), geometry.to_string()).unwrap();
        }
        root
    }

    #[test]
    fn a_ship_that_states_little_is_filled_from_its_model_and_says_so() {
        let dir = root("model", json!({"race": "Pirate", "physics": {"MaxSpeed": 12}, "guns": {"Gun_A": {"gun": "Gun", "arc_deg": 60}}}), true);
        let s = resolved_ship(&dir, "Test_Ship").unwrap();
        assert_eq!((s.race.as_str(), s.max_speed, s.mass, s.sight), ("Pirate", 12.0, 4000.0, 400.0));
        assert_eq!(s.sections.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(), ["Bow", "Stern", "Main sail"]);
        assert_eq!(s.section_hp, 150.0 + 150.0 + 70.0);
        assert_eq!(s.core_hp, 75.0, "a quarter of the hull sections' HP: (150 + 150) / 4");
        let g = &s.guns[0];
        assert_eq!((g.name.as_str(), g.gun.as_str(), g.section.as_deref()), ("Gun_A", "Gun", Some("Bow")));
        assert!((g.pos[1] - 5.0).abs() < 1e-5 && (g.arc_deg - 60.0).abs() < 1e-4, "{g:?}");
        assert_eq!((s.engines.len(), s.engines[0].fx.as_str(), s.engines[0].section.as_deref()), (1, "PirateEngine_Light", Some("Stern")));
        assert_eq!(s.wakes.len(), 1);
        assert_eq!(s.crew_posts[0], ResolvedPost { role: "Gunners".into(), count: 1 });
        // the sources
        let src = |k: &str| s.sources[k];
        assert_eq!((src("race"), src("/physics/MaxSpeed"), src("/physics/Mass")), (Source::File, Source::File, Source::Default));
        assert_eq!((src("core_hp"), src("sections"), src("engines"), src("wakes"), src("flags")), (Source::Model, Source::Model, Source::Model, Source::Model, Source::Model));
        assert_eq!((src("crew_posts"), src("explosion_fx"), src("sight"), src("guns")), (Source::Default, Source::Default, Source::Default, Source::File));
        assert_eq!((src("/guns/Gun_A/pos"), src("/guns/Gun_A/rot"), src("/guns/Gun_A/bank")), (Source::Model, Source::Model, Source::Model));
        assert_eq!((src("tow"), src("homing")), (Source::Default, Source::Default), "the model has neither node: the hull ends and the vital centre");
        assert!(serde_json::to_value(&s).unwrap()["sources"]["race"] == "file");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn what_the_file_states_wins_and_a_ship_without_a_model_has_no_model_sources() {
        let ship = json!({"core_hp": 9, "points": 20, "skirmish": true, "physics": {"Mass": 10, "MaxThrust": 30, "MaxSpeed": 10, "RotationalFriction": 1, "MaxAngularAcceleration": 0.2},
            "sections": {"Hull": {"layer": "Hull", "material": "Test", "hp": 20, "core_pct": 0.5, "vital_ship": true, "vital_speed": false, "vital_turn": false}},
            "guns": {"Fwd": {"gun": "Gun", "arc_deg": 90, "pos": [1, 2, 3], "rot": [0, 0, 0], "bank": "Bank07"}},
            "tow": {"tower": [1, 2, 3], "towee": [4, 5, 6]}, "homing": [7, 8, 9], "explosion_fx": "Boom"});
        let dir = root("file", ship, false);
        let s = resolved_ship(&dir, "Test_Ship").unwrap();
        assert_eq!((s.core_hp, s.points, s.skirmish, s.mass, s.max_thrust, s.max_speed), (9.0, 20, true, 10.0, 30.0, 10.0));
        assert_eq!((s.guns[0].bank, s.guns[0].pos, s.tower, s.towee, s.homing), (7, [1.0, 2.0, 3.0], [1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]));
        assert_eq!(s.explosion_fx.as_deref(), Some("Boom"));
        assert_eq!(s.section_hp, 20.0);
        let src = |k: &str| s.sources[k];
        for k in ["core_hp", "physics", "sections", "guns", "tow", "homing", "explosion_fx", "/physics/Mass", "/guns/Fwd/pos", "/guns/Fwd/bank"] {
            assert_eq!(src(k), Source::File, "{k}");
        }
        // nothing is derived from a model that is not there
        for k in ["engines", "wakes", "flags", "sphere", "crew_posts"] {
            assert_eq!(src(k), Source::Default, "{k}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_error_is_the_loaders_and_a_missing_ship_is_an_error() {
        let dir = root("err", json!({"guns": {"X": {"gun": "Nope", "arc_deg": 1, "pos": [0, 0, 0], "rot": [0, 0, 0]}}}), true);
        let e = resolved_ship(&dir, "Test_Ship").unwrap_err();
        assert!(e.contains("guns.X.gun") && e.contains("Nope"), "{e}");
        assert!(resolved_ship(&dir, "Nowhere").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
