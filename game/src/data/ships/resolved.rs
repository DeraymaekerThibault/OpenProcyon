//! The ship as the loader sees it (`authoring::resolved_ship`): the values and points `try_load_class` ends up with, each with where
//! it came from (a JSON pointer into `ship.json`, a node or layer of the model, or a PROVISIONAL fallback rule).
//!
//! Nothing is decided here. The loader (`ships::build`) merges `ship.json` and the model (`derive::complete`, `special_points`) and
//! hands back a `Parts` snapshot of the merged document; this file only says, per value, whether `ship.json` states it (the pointer
//! exists in the file) or the model supplied it. So the "which wins" rule exists once, in the loader.
use super::derive::{self, Facts};
use super::{ShipDoc, Special, build};
use bevy::math::Vec3;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

/// Where a resolved value comes from.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum Source {
    /// `ship.json` states it: an RFC 6901 JSON pointer into the file (`/guns/Gun_A/pos`, `/physics/MaxSpeed`).
    Json { pointer: String },
    /// The model supplies it: the name of the hardpoint node, or of the mesh node (the section's `layer`).
    Model { name: String },
    /// Neither does; the loader's default. `rule` says which (marked PROVISIONAL where the engine guessed).
    Fallback { rule: String },
}

impl Source {
    pub(super) fn json(pointer: &str) -> Source {
        Source::Json { pointer: pointer.into() }
    }
}

/// A value and where it came from.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Sourced<T> {
    /// The value the loader uses.
    pub value: T,
    /// Where it came from.
    pub source: Source,
}

/// What a hardpoint is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HardpointKind {
    /// A gun mount (`guns` entry or `GunPlacement` node).
    Gun,
    /// An engine port.
    Engine,
    /// A wake emitter.
    Wake,
    /// A flag attachment point.
    Flag,
    /// Where this ship holds a tow line.
    TowTower,
    /// Where a tow line fastens to this ship.
    TowTowee,
    /// Where homing torpedoes aim.
    Homing,
    /// A boarding effect point (the model only).
    Boarding,
    /// A crew place from `ship.json`'s `crew_nodes` (the model's own `Crew_*` nodes are not listed).
    Crew,
}

/// A point on the ship, in game axes (x starboard, y forward, z up).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Hardpoint {
    /// What it is.
    pub kind: HardpointKind,
    /// Its label (mount label, engine label, node name, crew node name), empty for the single points (tow, homing).
    pub name: String,
    /// Position, game axes.
    pub pos: [f64; 3],
    /// The `rot` triple of the mount, engine or crew node (a derived one is `[yaw, 0, 0]`); `None` for the others.
    pub rot: Option<[f64; 3]>,
    /// The effect of an engine or wake.
    pub fx: Option<String>,
    /// Where the position comes from.
    pub source: Source,
}

/// One hull section after derivation.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResolvedSection {
    /// The section's label.
    pub label: String,
    /// The model layer it covers.
    pub layer: String,
    /// Material.
    pub material: String,
    /// Hit points.
    pub hp: f64,
    /// `ship.json` states the section, or the model's layer made it.
    pub source: Source,
}

/// One gun mount after derivation (its position and facing are the `Gun` hardpoint of the same `name`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResolvedMount {
    /// The mount's label.
    pub label: String,
    /// The gun id fitted (`guns.json` key); not checked against `guns.json` here.
    pub gun: String,
    /// Firing arc in degrees.
    pub arc_deg: f64,
    /// The weapon-panel bank, stated or chosen by the loader.
    pub bank: Option<String>,
    /// The section layer it sits on, stated or the nearest hull layer.
    pub parent_layer: Option<String>,
}

/// A ship as the loader sees it: `authoring::resolved_ship`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResolvedShip {
    /// The ship id (its folder).
    pub id: String,
    /// The model file read (`ships/<id>/model.glb`, relative to the asset root), `None` when there is none or it is unreadable.
    pub model: Option<String>,
    /// Fleet cost in victory points.
    pub points: Sourced<u64>,
    /// `physics.MaxSpeed`.
    pub max_speed: Sourced<f64>,
    /// `physics.MaxThrust`.
    pub max_thrust: Sourced<f64>,
    /// `physics.Mass`.
    pub mass: Sourced<f64>,
    /// Core hit points.
    pub core_hp: Sourced<f64>,
    /// The sections, in order.
    pub sections: Vec<ResolvedSection>,
    /// The sum of the sections' hit points.
    pub section_hp: f64,
    /// The gun mounts, in order.
    pub mounts: Vec<ResolvedMount>,
    /// Every point: guns, engines, wakes, flags, tow tower and towee, homing, boarding, crew nodes (in that order).
    pub hardpoints: Vec<Hardpoint>,
}

struct GunPart {
    label: String,
    pos: Option<[f64; 3]>,
    rot: Option<[f64; 3]>,
    mount: ResolvedMount,
    node: Option<String>,
}
struct EnginePart {
    label: Option<String>,
    pos: [f64; 3],
    rot: Option<[f64; 3]>,
    fx: String,
    node: Option<String>,
}

/// The merged document of a loaded ship, in plain data (the loader moves out of its `ShipDoc`).
pub(in crate::data) struct Parts {
    pub(super) special: Special,
    model: Option<String>,
    has_facts: bool,
    points: u64,
    physics: [f64; 3],
    core_hp: f64,
    sections: Vec<ResolvedSection>,
    guns: Vec<GunPart>,
    /// The index in `ship.json`'s `engines` of each engine the loader keeps.
    engines: Vec<(usize, EnginePart)>,
    wakes: Vec<([f64; 3], String)>,
    wake_nodes: Vec<String>,
    flags: Vec<(String, [f64; 3])>,
    boarding: Vec<(String, [f64; 3])>,
    crew: Vec<(String, [f64; 3], Option<[f64; 3]>)>,
}

impl Parts {
    pub(super) fn new(j: &ShipDoc, facts: Option<&Facts>, special: Special, glb: &str) -> Parts {
        let nodes = |kind: &'static str| facts.into_iter().flat_map(move |f| f.points.iter().filter(move |p| p.kind == kind));
        let sections: Vec<_> = j
            .sections
            .iter()
            .map(|(label, s)| ResolvedSection { label: label.clone(), layer: s.layer.clone(), material: s.material.clone(), hp: s.hp, source: Source::json("") })
            .collect();
        let guns = j
            .guns
            .iter()
            .map(|(label, m)| GunPart {
                label: label.clone(),
                pos: m.pos,
                rot: m.rot,
                mount: ResolvedMount { label: label.clone(), gun: m.gun.clone(), arc_deg: m.arc_deg, bank: m.bank.clone(), parent_layer: m.parent_layer.clone() },
                node: facts.and_then(|f| derive::mount_node(f, label)).map(|p| p.name.clone()),
            })
            .collect();
        // The loader keeps an engine only with a position and an effect.
        let engines = j
            .engines
            .iter()
            .flatten()
            .enumerate()
            .filter_map(|(i, e)| {
                let node = e.label.as_deref().and_then(|l| nodes("EnginePortPlacement").find(|p| p.name.eq_ignore_ascii_case(l))).map(|p| p.name.clone());
                Some((i, EnginePart { label: e.label.clone(), pos: e.pos?, rot: e.rot, fx: e.fx.clone()?, node }))
            })
            .collect();
        Parts {
            special,
            model: facts.map(|_| glb.to_string()),
            has_facts: facts.is_some(),
            points: j.points,
            physics: [j.physics.mass, j.physics.max_thrust, j.physics.max_speed],
            core_hp: j.core_hp.unwrap_or(100.0),
            sections,
            guns,
            engines,
            wakes: j.wakes.iter().flatten().map(|w| (w.pos, w.fx.clone())).collect(),
            wake_nodes: nodes("WakePlacement").map(|p| p.name.clone()).collect(),
            flags: j.flags.iter().flatten().map(|f| (f.label.clone(), f.pos)).collect(),
            boarding: nodes("BoardingEffectPoint").map(|p| (p.name.clone(), p.pos)).collect(),
            crew: j.crew_nodes.iter().map(|c| (c.name.clone(), c.pos, c.rot)).collect(),
        }
    }

    /// Adds the sources: `raw` is the merged `ship.json` as written, so a pointer that exists in it is "stated".
    fn finish(self, id: &str, raw: &Value) -> ResolvedShip {
        let stated = |pointer: &str| raw.pointer(pointer).is_some_and(|v| !v.is_null());
        let escape = |s: &str| s.replace('~', "~0").replace('/', "~1");
        let sourced = |pointer: String, rule: &str| if stated(&pointer) { Source::Json { pointer } } else { Source::Fallback { rule: rule.into() } };
        let physics = |key: &str, value: f64, default: &str| Sourced { value, source: sourced(format!("/physics/{key}"), &format!("PROVISIONAL default {default}")) };
        let core_rule = if self.has_facts { "PROVISIONAL: a quarter of the hull sections' HP, at least 50" } else { "default 100" };
        let sections_stated = raw["sections"].as_object().is_some_and(|o| !o.is_empty());
        let sections: Vec<ResolvedSection> = self
            .sections
            .into_iter()
            .map(|mut s| {
                s.source = if sections_stated { Source::Json { pointer: format!("/sections/{}", escape(&s.label)) } } else { Source::Model { name: s.layer.clone() } };
                s
            })
            .collect();
        let mut hardpoints = vec![];
        for g in &self.guns {
            let (Some(pos), rot) = (g.pos, g.rot) else { continue };
            let pointer = format!("/guns/{}/pos", escape(&g.label));
            let source = match &g.node {
                Some(name) if !stated(&pointer) => Source::Model { name: name.clone() },
                _ => Source::Json { pointer },
            };
            hardpoints.push(Hardpoint { kind: HardpointKind::Gun, name: g.label.clone(), pos, rot, fx: None, source });
        }
        for (i, e) in &self.engines {
            let pointer = format!("/engines/{i}/pos");
            let source = match &e.node {
                Some(name) if !stated(&pointer) => Source::Model { name: name.clone() },
                _ => Source::Json { pointer },
            };
            hardpoints.push(Hardpoint { kind: HardpointKind::Engine, name: e.label.clone().unwrap_or_default(), pos: e.pos, rot: e.rot, fx: Some(e.fx.clone()), source });
        }
        let wakes_stated = raw["wakes"].is_array();
        for (i, (pos, fx)) in self.wakes.iter().enumerate() {
            let name = self.wake_nodes.get(i).filter(|_| !wakes_stated);
            let source = match name {
                Some(name) => Source::Model { name: name.clone() },
                None if wakes_stated => Source::Json { pointer: format!("/wakes/{i}") },
                None => Source::Fallback { rule: "PROVISIONAL: a wake at the stern's waterline when the model has no wake node".into() },
            };
            hardpoints.push(Hardpoint { kind: HardpointKind::Wake, name: name.cloned().unwrap_or_default(), pos: *pos, rot: None, fx: Some(fx.clone()), source });
        }
        let flags_stated = raw["flags"].is_array();
        for (i, (label, pos)) in self.flags.iter().enumerate() {
            let source = if flags_stated { Source::Json { pointer: format!("/flags/{i}") } } else { Source::Model { name: label.clone() } };
            hardpoints.push(Hardpoint { kind: HardpointKind::Flag, name: label.clone(), pos: *pos, rot: None, fx: None, source });
        }
        let at = |v: Vec3| crate::data::unconv(v);
        let [tower, towee] = self.special.tow;
        for (kind, (v, source)) in [(HardpointKind::TowTower, tower), (HardpointKind::TowTowee, towee), (HardpointKind::Homing, self.special.homing)] {
            hardpoints.push(Hardpoint { kind, name: String::new(), pos: at(v), rot: None, fx: None, source });
        }
        for (name, pos) in &self.boarding {
            hardpoints.push(Hardpoint { kind: HardpointKind::Boarding, name: name.clone(), pos: *pos, rot: None, fx: None, source: Source::Model { name: name.clone() } });
        }
        for (i, (name, pos, rot)) in self.crew.iter().enumerate() {
            hardpoints.push(Hardpoint {
                kind: HardpointKind::Crew,
                name: name.clone(),
                pos: *pos,
                rot: *rot,
                fx: None,
                source: Source::Json { pointer: format!("/crew_nodes/{i}") },
            });
        }
        let core_source = sourced("/core_hp".into(), core_rule);
        ResolvedShip {
            id: id.into(),
            model: self.model,
            points: Sourced { value: self.points, source: sourced("/points".into(), "default 0") },
            max_speed: physics("MaxSpeed", self.physics[2], "8"),
            max_thrust: physics("MaxThrust", self.physics[1], "60000"),
            mass: physics("Mass", self.physics[0], "4000"),
            core_hp: Sourced { value: self.core_hp, source: core_source },
            section_hp: sections.iter().map(|s| s.hp).sum(),
            sections,
            mounts: self.guns.into_iter().map(|g| g.mount).collect(),
            hardpoints,
        }
    }
}

/// The ship `id` of the asset root `root` as the loader sees it; see `authoring::resolved_ship`.
pub fn resolved_ship(root: &Path, id: &str) -> Result<ResolvedShip, crate::data::DataError> {
    let path = root.join(format!("ships/{id}/ship.json"));
    let bad_id = id.is_empty() || Path::new(id).components().count() != 1 || !matches!(Path::new(id).components().next(), Some(std::path::Component::Normal(_)));
    if bad_id {
        let error = std::io::Error::new(std::io::ErrorKind::InvalidInput, "a ship id is one folder name");
        return Err(crate::data::DataError::Read { path, source_mod: None, error });
    }
    let raw: Value = crate::data::load(&path)?;
    let (_, parts) =
        build(root, id, None).map_err(|message| crate::data::DataError::Read { path, source_mod: None, error: std::io::Error::new(std::io::ErrorKind::InvalidData, message) })?;
    Ok(parts.finish(id, &raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    /// A ship folder holding the synthetic model of `derive::tests` and the given `ship.json`.
    fn folder(tag: &str, ship: Value) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("procyon-resolved-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("ships/S")).unwrap();
        std::fs::write(root.join("ships/S/model.glb"), derive::tests::model()).unwrap();
        std::fs::write(root.join("ships/S/ship.json"), ship.to_string()).unwrap();
        root
    }
    fn of(r: &ResolvedShip, kind: HardpointKind) -> Vec<&Hardpoint> {
        r.hardpoints.iter().filter(|h| h.kind == kind).collect()
    }
    fn node(name: &str) -> Source {
        Source::Model { name: name.into() }
    }

    #[test]
    fn a_ship_of_stats_and_a_model_says_which_values_the_model_gave() {
        let root = folder("derived", json!({"physics": {"MaxSpeed": 12}, "guns": {"Gun_A": {"gun": "Gun", "arc_deg": 60}}}));
        let r = resolved_ship(&root, "S").unwrap();
        assert_eq!(r.model.as_deref(), Some("ships/S/model.glb"));
        assert_eq!(r.max_speed, Sourced { value: 12.0, source: Source::json("/physics/MaxSpeed") });
        assert_eq!(r.mass.value, 4000.0);
        assert!(matches!(&r.mass.source, Source::Fallback { rule } if rule.contains("PROVISIONAL")));
        assert!(r.core_hp.value >= 50.0 && matches!(r.core_hp.source, Source::Fallback { .. }));
        assert_eq!(
            r.sections.iter().map(|s| (s.layer.as_str(), s.source.clone())).collect::<Vec<_>>(),
            [("Bow", node("Bow")), ("Stern", node("Stern")), ("Main sail", node("Main sail"))]
        );
        assert_eq!(r.section_hp, 150.0 + 150.0 + 70.0);
        assert_eq!((r.mounts[0].gun.as_str(), r.mounts[0].arc_deg, r.mounts[0].parent_layer.as_deref()), ("Gun", 60.0, Some("Bow")));
        let gun = of(&r, HardpointKind::Gun)[0];
        assert_eq!((gun.name.as_str(), gun.pos, gun.source.clone()), ("Gun_A", [0.0, 5.0, 1.0], node("Gun_A")));
        assert!((gun.rot.unwrap()[0] - std::f64::consts::FRAC_PI_2).abs() < 1e-5, "the node's yaw");
        let engine = of(&r, HardpointKind::Engine)[0];
        assert_eq!((engine.pos, engine.source.clone(), engine.fx.as_deref()), ([0.0, -6.0, 1.0], node("Port engine"), Some("NavyEngine_Light")));
        assert_eq!(of(&r, HardpointKind::Wake)[0].source, node("Stern wake"));
        assert_eq!(of(&r, HardpointKind::Boarding)[0].pos, [1.0, -3.0, 2.0]);
        // No tow or homing node in the model: the loader's hull-end fallbacks.
        for kind in [HardpointKind::TowTower, HardpointKind::TowTowee, HardpointKind::Homing] {
            assert!(matches!(&of(&r, kind)[0].source, Source::Fallback { rule } if rule.contains("PROVISIONAL")), "{kind:?}");
        }
        assert!(serde_json::to_value(&r).unwrap()["hardpoints"][0]["source"]["from"] == "model", "serializes with a tag");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn what_ship_json_states_wins_and_points_to_its_own_key() {
        let ship = json!({
            "core_hp": 7, "points": 3, "physics": {"Mass": 9, "MaxThrust": 8, "MaxSpeed": 5},
            "guns": {"Gun/A": {"gun": "Gun", "arc_deg": 60, "pos": [1, 2, 3], "rot": [0, 0, 0]}},
            "engines": [{"label": "Port engine", "fx": "E"}, {"pos": [4, 5, 6], "rot": [0.5, 0, 0], "fx": "E2"}],
            "wakes": [{"pos": [7, 8, 9], "fx": "W"}], "flags": [{"label": "Flag_Top", "pos": [1, 1, 1]}],
            "tow": {"tower": [1, 2, 3], "towee": [4, 5, 6]}, "homing": [7, 8, 9],
            "crew_nodes": [{"name": "Crew_Post_Captain", "pos": [0, 1, 2], "rot": [0.5, 0, 0]}],
            "sections": {"Hull": {"layer": "Bow", "material": "Wood", "hp": 40, "core_pct": 1, "vital_ship": true, "vital_speed": false, "vital_turn": false}}
        });
        let root = folder("stated", ship);
        let r = resolved_ship(&root, "S").unwrap();
        assert_eq!((r.core_hp.clone(), r.points.value), (Sourced { value: 7.0, source: Source::json("/core_hp") }, 3));
        assert_eq!((r.mass.source.clone(), r.max_thrust.value), (Source::json("/physics/Mass"), 8.0));
        assert_eq!((r.sections[0].source.clone(), r.section_hp), (Source::json("/sections/Hull"), 40.0));
        let gun = of(&r, HardpointKind::Gun)[0];
        assert_eq!((gun.pos, gun.source.clone()), ([1.0, 2.0, 3.0], Source::json("/guns/Gun~1A/pos")), "the label is escaped like a JSON pointer");
        let engines = of(&r, HardpointKind::Engine);
        assert_eq!((engines[0].pos, engines[0].source.clone()), ([0.0, -6.0, 1.0], node("Port engine")), "an engine entry without pos takes its node's");
        assert_eq!((engines[1].pos, engines[1].source.clone()), ([4.0, 5.0, 6.0], Source::json("/engines/1/pos")));
        assert_eq!(of(&r, HardpointKind::Wake)[0].source, Source::json("/wakes/0"));
        assert_eq!(of(&r, HardpointKind::Flag)[0].source, Source::json("/flags/0"));
        assert_eq!((of(&r, HardpointKind::TowTower)[0].pos, of(&r, HardpointKind::TowTowee)[0].source.clone()), ([1.0, 2.0, 3.0], Source::json("/tow/towee")));
        assert_eq!((of(&r, HardpointKind::Homing)[0].pos, of(&r, HardpointKind::Homing)[0].source.clone()), ([7.0, 8.0, 9.0], Source::json("/homing")));
        let crew = of(&r, HardpointKind::Crew)[0];
        assert_eq!((crew.name.as_str(), crew.pos, crew.rot, crew.source.clone()), ("Crew_Post_Captain", [0.0, 1.0, 2.0], Some([0.5, 0.0, 0.0]), Source::json("/crew_nodes/0")));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_tow_or_homing_node_of_the_model_is_named_as_the_source() {
        let mut model = derive::tests::model();
        // Rewrite the model with two more nodes: the JSON chunk is the only text, so build it again from the synthetic one.
        let (mut doc, end) = crate::data::glb_json(&model).unwrap();
        let bin = model.split_off(end + 8);
        doc["scenes"][0]["nodes"].as_array_mut().unwrap().extend([json!(7), json!(8)]);
        doc["nodes"].as_array_mut().unwrap().extend([
            json!({"name": "Hook", "translation": [0.0, 2.0, -4.0], "extras": {"kind": "TowerPoint"}}),
            json!({"name": "Aim", "translation": [1.0, 0.0, 0.0], "extras": {"kind": "TorpedoHomingPoint"}}),
        ]);
        let root = folder("nodes", json!({}));
        std::fs::write(root.join("ships/S/model.glb"), derive::tests::glb(doc, &bin[4..])).unwrap();
        let r = resolved_ship(&root, "S").unwrap();
        assert_eq!((of(&r, HardpointKind::TowTower)[0].pos, of(&r, HardpointKind::TowTower)[0].source.clone()), ([0.0, 4.0, 2.0], node("Hook")));
        assert_eq!(of(&r, HardpointKind::Homing)[0].source, node("Aim"));
        assert!(matches!(of(&r, HardpointKind::TowTowee)[0].source, Source::Fallback { .. }));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_points_are_the_ones_the_loader_builds_the_class_from() {
        let root = folder("class", json!({"homing": [7, 8, 9]}));
        let r = resolved_ship(&root, "S").unwrap();
        let class = crate::data::try_load_class(&root, "S", &HashMap::new()).unwrap();
        let game = |v: bevy::math::Vec3| crate::data::unconv(v);
        assert_eq!(game(class.homing_point), of(&r, HardpointKind::Homing)[0].pos);
        assert_eq!([game(class.tow_points[0]), game(class.tow_points[1])], [of(&r, HardpointKind::TowTower)[0].pos, of(&r, HardpointKind::TowTowee)[0].pos]);
        assert_eq!(class.engines.iter().map(|e| game(e.0)).collect::<Vec<_>>(), of(&r, HardpointKind::Engine).iter().map(|h| h.pos).collect::<Vec<_>>());
        assert_eq!(class.wake.iter().map(|w| game(w.0)).collect::<Vec<_>>(), of(&r, HardpointKind::Wake).iter().map(|h| h.pos).collect::<Vec<_>>());
        assert_eq!(class.boarding_points.iter().map(|p| game(*p)).collect::<Vec<_>>(), of(&r, HardpointKind::Boarding).iter().map(|h| h.pos).collect::<Vec<_>>());
        assert_eq!((class.phys.max_speed as f64, class.phys.mass as f64, class.core_hp as f64), (r.max_speed.value, r.mass.value, r.core_hp.value));
        assert_eq!(class.sections.iter().map(|s| (s.hp as f64, s.layer.clone())).collect::<Vec<_>>(), r.sections.iter().map(|s| (s.hp, s.layer.clone())).collect::<Vec<_>>());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_ship_the_loader_refuses_and_a_bad_id_are_errors() {
        let root = folder("bad", json!({"guns": {"Nowhere": {"gun": "Gun", "arc_deg": 60}}}));
        let error = resolved_ship(&root, "S").err().unwrap().to_string();
        assert!(error.contains("Nowhere") && error.contains("no pos/rot"), "{error}");
        assert!(resolved_ship(&root, "../S").is_err() && resolved_ship(&root, "").is_err());
        assert!(resolved_ship(&root, "Missing").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
