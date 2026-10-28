//! Ship JSON schemas; conversion keeps source ordering and the existing game-to-Bevy mapping.
use super::*;
use serde::Deserialize;
use serde::de::{MapAccess, Visitor};
use std::fmt;
use std::path::Path;

/// Preserve authored section/mount order: parent indices and weapon banks depend on it.
fn ordered<'de, D, T>(deserializer: D) -> Result<Vec<(String, T)>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Entries<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for Entries<T> {
        type Value = Vec<(String, T)>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an object keyed by label")
        }
        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
            let mut entries: Vec<(String, T)> = Vec::new();
            while let Some(label) = map.next_key::<String>()? {
                let value = map.next_value().map_err(|e| serde::de::Error::custom(format!("{label}: {e}")))?;
                if let Some((_, previous)) = entries.iter_mut().find(|(key, _)| *key == label) {
                    *previous = value;
                } else {
                    entries.push((label, value));
                }
            }
            Ok(entries)
        }
    }
    deserializer.deserialize_map(Entries(std::marker::PhantomData))
}

#[derive(Deserialize)]
struct ShipDoc {
    geometry: Option<String>,
    model: Option<String>,
    #[serde(default)]
    race: String,
    #[serde(default)]
    skirmish: bool,
    core_hp: f64,
    physics: ShipPhysics,
    #[serde(deserialize_with = "ordered")]
    sections: Vec<(String, SectionDoc)>,
    #[serde(deserialize_with = "ordered")]
    guns: Vec<(String, MountDoc)>,
    engines: Vec<EngineDoc>,
    flags: Option<Vec<FlagDoc>>,
    explosion_fx: Option<String>,
    engine_snd: Option<Vec<Option<String>>>,
    ring: Option<f64>,
    sight: Option<f64>,
    repair_fx: Option<String>,
    cloakable: Option<bool>,
    #[serde(default)]
    points: u64,
    crew_posts: Option<HashMap<String, u64>>,
    display_name: Option<String>,
    icon: Option<String>,
    #[serde(default)]
    lifeboats: u64,
    #[serde(default)]
    is_lifeboat: bool,
    sphere: Option<SphereDoc>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ShipPhysics {
    mass: f64,
    max_thrust: f64,
    max_speed: f64,
    rotational_friction: f64,
    max_angular_acceleration: f64,
    min_extents: Option<Vec<f64>>,
    max_extents: Option<Vec<f64>>,
    center_of_mass: Option<Vec<f64>>,
}

#[derive(Deserialize)]
struct SectionDoc {
    layer: String,
    material: String,
    hp: f64,
    core_pct: f64,
    vital_ship: bool,
    vital_speed: bool,
    vital_turn: bool,
    #[serde(default)]
    vital_mission: bool,
    destruct: Option<DestructDoc>,
    debris: Option<DebrisDoc>,
    debris_fx: Option<String>,
}
#[derive(Deserialize)]
struct DestructDoc {
    fx: Option<String>,
    start: f64,
    end: f64,
    vel: [f64; 3],
}
#[derive(Deserialize)]
struct DebrisDoc {
    model: String,
    pivot: [f64; 3],
}
#[derive(Deserialize)]
struct MountDoc {
    pos: [f64; 3],
    rot: [f64; 3],
    arc_deg: f64,
    gun: String,
    bank: Option<String>,
    parent_layer: Option<String>,
}
#[derive(Deserialize)]
struct EngineDoc {
    pos: [f64; 3],
    rot: [f64; 3],
    fx: String,
}
#[derive(Deserialize)]
struct FlagDoc {
    label: String,
    pos: [f64; 3],
}
#[derive(Deserialize)]
struct SphereDoc {
    center: [f64; 3],
    radius: f64,
}
#[derive(Deserialize)]
struct GeometryDoc {
    aabb: [[f64; 3]; 2],
    tris: Vec<f64>,
}

fn point(v: [f64; 3]) -> Vec3 {
    conv(v[0] as f32, v[1] as f32, v[2] as f32)
}

fn optional_point(v: Option<&Vec<f64>>) -> Vec3 {
    point(std::array::from_fn(|i| v.and_then(|v| v.get(i)).copied().unwrap_or(0.0)))
}

/// Ship definition and referenced geometry. Public API remains fail-fast like the other required loaders.
pub fn load_class(root: &Path, name: &str, guns: &HashMap<String, GunDef>) -> Class {
    try_load_class(root, name, guns).unwrap_or_else(|e| panic!("{e}"))
}

fn try_load_class(root: &Path, name: &str, guns: &HashMap<String, GunDef>) -> Result<Class, String> {
    let path = root.join(format!("ships/{name}/ship.json"));
    let j: ShipDoc = read_typed(&path)?;
    let geo_path = root.join(format!("ships/{}/geometry.json", j.geometry.as_deref().unwrap_or(name)));
    let geometry: HashMap<String, GeometryDoc> = read_typed(&geo_path)?;
    let context = asset_context(&path);
    let geo_context = asset_context(&geo_path);
    let sections = j
        .sections
        .iter()
        .map(|(label, s)| {
            let geo = geometry.get(&s.layer).ok_or_else(|| format!("{context}: sections.{label}.layer references missing layer {:?} in {geo_context}", s.layer))?;
            if geo.tris.len() % 9 != 0 {
                return Err(format!("{geo_context}: {}.tris must contain a multiple of 9 coordinates, got {}", s.layer, geo.tris.len()));
            }
            let (a, b) = (point(geo.aabb[0]), point(geo.aabb[1]));
            Ok(Section {
                label: label.clone(),
                layer: s.layer.clone(),
                material: s.material.clone(),
                hp: s.hp as f32,
                core_pct: s.core_pct as f32,
                min: a.min(b),
                max: a.max(b),
                vital: s.vital_ship,
                speed: s.vital_speed,
                turn: s.vital_turn,
                mission: s.vital_mission,
                destruct: s.destruct.as_ref().map(|d| Destruct { fx: d.fx.clone(), start: d.start as f32, end: d.end as f32, vel: point(d.vel) }),
                debris: s.debris.as_ref().map(|d| (d.model.clone(), point(d.pivot))),
                debris_fx: s.debris_fx.clone(),
                tris: geo.tris.as_chunks::<9>().0.iter().map(|c| [point([c[0], c[1], c[2]]), point([c[3], c[4], c[5]]), point([c[6], c[7], c[8]])]).collect(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mounts = j
        .guns
        .iter()
        .map(|(label, g)| {
            if !guns.contains_key(&g.gun) {
                return Err(format!("{context}: guns.{label}.gun references unknown weapon {:?}", g.gun));
            }
            let r = g.rot.map(|v| v as f32);
            Ok(Mount {
                pos: point(g.pos),
                // Preserve the rolled frame of inverted mounts.
                rot: Quat::from_rotation_z(-r[1]) * Quat::from_rotation_y(r[0]) * Quat::from_rotation_x(r[2]),
                arc: (g.arc_deg as f32).to_radians(),
                gun: g.gun.clone(),
                bank: g.bank.as_deref().and_then(|b| b.trim_start_matches("Bank").parse().ok()).unwrap_or(0),
                parent: g.parent_layer.as_deref().and_then(|l| j.sections.iter().position(|(_, s)| s.layer == l)),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let hull = |f: &dyn Fn(&Section) -> bool| sections.iter().filter(|s| f(s)).fold((Vec3::MAX, Vec3::MIN), |(a, b), s| (a.min(s.min), b.max(s.max)));
    let (hmn, hmx) = hull(&|s| s.vital);
    let (amn, amx) = hull(&|_| true);
    let ph = &j.physics;
    let engage = if mounts.is_empty() { 60.0 } else { mounts.iter().map(|m| guns[&m.gun].range[0]).sum::<f32>() / mounts.len() as f32 };
    let mut banks: Vec<(usize, f32)> = mounts.iter().map(|m| (m.bank, guns[&m.gun].range[1])).collect();
    banks.sort_by_key(|b| b.0);
    banks.dedup_by_key(|b| b.0);
    let reach = if banks.is_empty() { 125.0 } else { banks.iter().map(|b| b.1).sum::<f32>() / banks.len() as f32 };
    let bounds = if ph.min_extents.is_some() && ph.max_extents.is_some() {
        let (a, b) = (optional_point(ph.min_extents.as_ref()), optional_point(ph.max_extents.as_ref()));
        [a.min(b), a.max(b)]
    } else {
        [amn, amx]
    };
    let glb = format!("ships/{}/model.glb", j.model.as_deref().unwrap_or(name));
    let model_bytes = std::fs::read(root.join(&glb)).unwrap_or_default();
    let nodes = glb_json(&model_bytes).map(|(doc, _)| doc);
    let node_point = |kind: &str| {
        nodes.as_ref()?["nodes"]
            .as_array()?
            .iter()
            .find(|n| n["extras"]["kind"] == kind)
            .map(|n| Vec3::new(num(&n["translation"][0]), num(&n["translation"][1]), num(&n["translation"][2])))
    };
    let hull_bound = sections.iter().map(|s| s.min.abs().max(s.max.abs()).length()).fold(0.0, f32::max);
    let centre = if hmn.x <= hmx.x { (hmn + hmx) / 2.0 } else { (amn + amx) / 2.0 };
    let radius = 0.5 * (amx - amn).x.max((amx - amn).z) + 2.0;
    let flag_at = |label: &str| j.flags.as_ref()?.iter().find(|f| f.label == label).map(|f| point(f.pos));
    let flag = flag_at("Flag_Top").zip(flag_at("Flag_Bottom"));
    Ok(Class {
        name: name.into(),
        race: j.race,
        skirmish: j.skirmish,
        glb,
        mounts,
        sections,
        core_hp: j.core_hp as f32,
        phys: Physics {
            mass: ph.mass as f32,
            thrust: ph.max_thrust as f32,
            max_speed: ph.max_speed as f32,
            rot_friction: ph.rotational_friction as f32,
            max_ang_acc: ph.max_angular_acceleration as f32,
        },
        flag,
        engines: j.engines.into_iter().map(|e| (point(e.pos), Quat::from_rotation_y(e.rot[0] as f32), e.fx)).collect(),
        explosion_fx: j.explosion_fx,
        engine_snd: std::array::from_fn(|i| j.engine_snd.as_ref().and_then(|v| v.get(i)).cloned().flatten()),
        // PROVISIONAL fallbacks retained for older imports and mods.
        ring: j.ring.unwrap_or(0.65) as f32,
        sight: j.sight.unwrap_or(400.0) as f32,
        repair_fx: j.repair_fx,
        centre,
        radius,
        hull_bound,
        engage,
        reach,
        cloakable: j.cloakable.unwrap_or_else(|| name.contains("Submersible")),
        points: j.points as u32,
        crew_posts: j.crew_posts.map_or(0, |v| v.values().sum::<u64>() as u32),
        display_name: j.display_name,
        icon: j.icon,
        length: ph.max_extents.as_ref().and_then(|v| v.get(1)).zip(ph.min_extents.as_ref().and_then(|v| v.get(1))).map_or((amx - amn).z, |(a, b)| (a - b) as f32),
        collision_bounds: bounds,
        mass_centre: optional_point(ph.center_of_mass.as_ref()),
        lifeboats: j.lifeboats as u32,
        is_lifeboat: j.is_lifeboat,
        sphere: j.sphere.map_or((centre, radius), |s| (point(s.center), s.radius as f32)),
        // PROVISIONAL hull-end fallback for custom models without authored tow/homing nodes.
        tow_points: [node_point("TowerPoint").unwrap_or(Vec3::new(0.0, 0.0, bounds[1].z)), node_point("ToweePoint").unwrap_or(Vec3::new(0.0, 0.0, bounds[0].z))],
        homing_point: node_point("TorpedoHomingPoint").unwrap_or((hmn + hmx) * 0.5),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new(ship: Value, geometry: Value) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!("procyon-typed-ship-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(root.join("ships/Synthetic")).unwrap();
            std::fs::write(root.join("ships/Synthetic/ship.json"), ship.to_string()).unwrap();
            std::fs::write(root.join("ships/Synthetic/geometry.json"), geometry.to_string()).unwrap();
            Self(root)
        }
        fn load(&self) -> Result<Class, String> {
            try_load_class(&self.0, "Synthetic", &HashMap::new())
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn ship() -> Value {
        serde_json::json!({
            "core_hp":100, "physics":{"Mass":10,"MaxThrust":30,"MaxSpeed":10,"RotationalFriction":1,"MaxAngularAcceleration":0.2,"CenterOfMass":[1]},
            "sections":{
                "Z first":{"layer":"Hull","material":"Test","hp":20,"core_pct":0.5,"vital_ship":true,"vital_speed":false,"vital_turn":false},
                "A second":{"layer":"Hull","material":"Test","hp":30,"core_pct":0.5,"vital_ship":false,"vital_speed":false,"vital_turn":false}
            },
            "guns":{}, "engines":[], "ignored_future_field":true
        })
    }
    fn geometry() -> Value {
        serde_json::json!({"Hull":{"aabb":[[-1,-2,-3],[1,2,3]],"tris":[1,2,3,4,5,6,7,8,9]}})
    }
    #[test]
    fn ordered_maps_keep_first_position_and_last_value_like_json_values() {
        #[derive(Deserialize)]
        struct Labels(#[serde(deserialize_with = "ordered")] Vec<(String, u32)>);
        let Labels(labels) = serde_json::from_str(r#"{"z":1,"a":2,"z":3}"#).unwrap();
        assert_eq!(labels, [("z".into(), 3), ("a".into(), 2)]);
    }
    #[test]
    fn synthetic_ship_preserves_order_axes_and_older_mod_defaults() {
        let f = Fixture::new(ship(), geometry());
        let c = f.load().unwrap();
        assert_eq!(c.sections.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["Z first", "A second"]);
        assert_eq!(c.sections[0].min, Vec3::new(-1.0, -3.0, -2.0));
        assert_eq!(c.sections[0].tris[0], [Vec3::new(1.0, 3.0, -2.0), Vec3::new(4.0, 6.0, -5.0), Vec3::new(7.0, 9.0, -8.0)]);
        assert_eq!(c.mass_centre, Vec3::X, "partial optional physics vectors retain zero-fill");
        assert_eq!((c.ring, c.sight, c.lifeboats, c.is_lifeboat), (0.65, 400.0, 0, false));
        assert_eq!(c.sphere, (c.centre, c.radius));
        assert_eq!(c.collision_bounds, [c.sections[0].min, c.sections[0].max]);
        assert_eq!(c.engine_snd, [None, None, None]);
        assert!(c.sections[0].destruct.is_none() && !c.sections[0].mission && c.sections[0].debris.is_none());
    }
    #[test]
    fn invalid_ship_reports_file_section_and_field_or_location() {
        let mut missing = ship();
        missing["sections"]["Z first"].as_object_mut().unwrap().remove("hp");
        let error = Fixture::new(missing, geometry()).load().err().unwrap();
        assert!(error.contains("ship.json") && error.contains("Z first") && error.contains("hp"), "{error}");
        let mut wrong = ship();
        wrong["sections"]["Z first"]["hp"] = serde_json::json!("not a number");
        let error = Fixture::new(wrong, geometry()).load().err().unwrap();
        assert!(error.contains("ship.json") && error.contains("Z first") && error.contains("line") && error.contains("column"), "{error}");
    }
    #[test]
    fn geometry_and_weapon_references_fail_with_context_instead_of_index_panics() {
        let mut geo = geometry();
        geo["Hull"]["tris"] = serde_json::json!([1, 2]);
        let error = Fixture::new(ship(), geo).load().err().unwrap();
        assert!(error.contains("geometry.json") && error.contains("Hull.tris") && error.contains("multiple of 9"), "{error}");
        let error = Fixture::new(ship(), serde_json::json!({})).load().err().unwrap();
        assert!(error.contains("sections.Z first.layer") && error.contains("Hull"), "{error}");
        let mut doc = ship();
        doc["guns"]["Test mount"] = serde_json::json!({"pos":[0,0,0],"rot":[0,0,0],"arc_deg":90,"gun":"Missing"});
        let error = Fixture::new(doc, geometry()).load().err().unwrap();
        assert!(error.contains("guns.Test mount.gun") && error.contains("Missing"), "{error}");
    }
    #[test]
    fn missing_file_and_mod_symlink_errors_identify_the_source() {
        let f = Fixture::new(ship(), geometry());
        let path = f.0.join("ships/Synthetic/ship.json");
        std::fs::remove_file(&path).unwrap();
        let error = f.load().err().unwrap();
        assert!(error.contains("ships/Synthetic/ship.json") && error.contains("build_assets"), "{error}");
        #[cfg(unix)]
        {
            let source = f.0.join("broken-mod.json");
            std::fs::write(&source, r#"{"core_hp":"bad"}"#).unwrap();
            std::os::unix::fs::symlink(&source, &path).unwrap();
            let error = f.load().err().unwrap();
            assert!(error.contains("ship.json") && error.contains("broken-mod.json") && error.contains("line"), "{error}");
        }
    }
}
