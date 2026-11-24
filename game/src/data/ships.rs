//! Ship JSON schemas; conversion keeps source ordering and the existing game-to-Bevy mapping.
use super::*;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::de::{MapAccess, Visitor};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

pub mod derive;
use super::crew_nodes;

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

/// `ships/<Id>/ship.json` (layout and defaults: docs/data-format.md "ship.json keys"). Unknown keys are allowed and ignored.
#[derive(Deserialize, JsonSchema)]
#[schemars(title = "ship.json")]
pub(super) struct ShipDoc {
    /// Editor hint: the schema this file follows. Ignored by the loader.
    #[serde(rename = "$schema", default)]
    _schema: Option<String>,
    /// Not retail: the ship folder whose `geometry.json` this ship uses (default: its own folder).
    geometry: Option<String>,
    /// Not retail: the ship folder whose `model.glb` this ship uses (default: its own folder).
    model: Option<String>,
    /// Name of the scene the ship appears in on the fleet screen; none when absent.
    scene: Option<String>,
    /// Race of the hull (empty when absent).
    #[serde(default)]
    race: String,
    /// `true`: the ship is offered on the fleet screen (default `false`).
    #[serde(default)]
    skirmish: bool,
    /// Core hit points. Default: with a model, a quarter of the hull sections' HP (at least 50); without one, 100.
    core_hp: Option<f64>,
    /// Physics; a missing value is the PROVISIONAL default (about a Navy Cutter's: Mass 4000, MaxThrust 60000, MaxSpeed 8, RotationalFriction 20, MaxAngularAcceleration 3).
    #[serde(default)]
    physics: ShipPhysics,
    /// Hull sections by label, in order. Default: built from the model's layers. Each needs `layer`, `material`, `hp`, `core_pct`, `vital_ship`, `vital_speed`, `vital_turn`.
    #[serde(default, deserialize_with = "ordered")]
    #[schemars(with = "BTreeMap<String, SectionDoc>")]
    sections: Vec<(String, SectionDoc)>,
    /// Gun mounts by label, in order. Each needs `gun` (a key of `guns.json`) and `arc_deg`; `pos`/`rot` default to the model's hardpoint node of that name.
    #[serde(default, deserialize_with = "ordered")]
    #[schemars(with = "BTreeMap<String, MountDoc>")]
    guns: Vec<(String, MountDoc)>,
    /// Engine ports. Default: from the model's `EnginePortPlacement` nodes.
    engines: Option<Vec<EngineDoc>>,
    /// Wake emitters. Default: from the model's `WakePlacement` nodes.
    wakes: Option<Vec<WakeDoc>>,
    /// Flag attachment points. Default: from the model's `FlagAttachmentPoint` nodes.
    flags: Option<Vec<FlagDoc>>,
    /// Effect played when the ship blows up. Default: by race and size.
    explosion_fx: Option<String>,
    /// Engine sound names. Default: by race and size.
    engine_snd: Option<Vec<Option<String>>>,
    /// Selection ring size (default 0.65).
    ring: Option<f64>,
    /// AI sight range (default 400).
    sight: Option<f64>,
    /// Effect played while the ship is repaired. Default: by race and size.
    repair_fx: Option<String>,
    /// Whether the ship can cloak. Default: `true` only for ids containing `Submersible`.
    cloakable: Option<bool>,
    /// Not retail: `true` for a hull that burns fuel, so a nebula's drain does not slow it (default `false`).
    fuel: Option<bool>,
    /// Fleet cost in victory points (default 0).
    #[serde(default)]
    points: u64,
    /// Crew posts by role (`Gunners`, `Captains`, `First Mates`, `Navigators`, `Engineers`, `Riggers`, `Fighters`, `Lookouts`). Default: one gunner per two guns, a captain, mate, navigator, engineer, a rigger per sail, a fighter or two.
    crew_posts: Option<HashMap<String, u64>>,
    /// Name shown for the hull. Default: the hull's name string.
    display_name: Option<String>,
    /// Icon name. Default: named from the ship id.
    icon: Option<String>,
    /// Number of lifeboats (default 0).
    #[serde(default)]
    lifeboats: u64,
    /// `true` for a lifeboat hull (default `false`).
    #[serde(default)]
    is_lifeboat: bool,
    /// Bounding sphere. Default: from the model.
    sphere: Option<SphereDoc>,
    /// Not retail: crew places for deck crews, added to the model's `Crew_*` nodes (a node of the same name replaces the model's).
    #[serde(default)]
    crew_nodes: Vec<crew_nodes::CrewNodeDoc>,
    /// Not retail: where a tow line attaches, ship frame, game axes. When present it wins over the model's `TowerPoint` / `ToweePoint` nodes.
    tow: Option<TowDoc>,
    /// Not retail: where homing torpedoes aim, ship frame, game axes (x starboard, y forward, z up). When present it wins over the model's `TorpedoHomingPoint` node.
    homing: Option<[f64; 3]>,
}

/// The schema of `ship.json`.
pub(super) fn schema() -> Value {
    serde_json::to_value(schemars::SchemaGenerator::default().into_root_schema_for::<ShipDoc>()).expect("schema is JSON")
}

/// Not retail: the two ends of a tow, ship frame, game axes (the convention of a gun's `pos`).
#[derive(Deserialize, JsonSchema)]
struct TowDoc {
    /// Where this ship holds the line when it tows another (the tower).
    tower: [f64; 3],
    /// Where the line fastens when another ship tows this one (the towee).
    towee: [f64; 3],
}

/// PROVISIONAL defaults, about a Navy Cutter's, for a ship that states no physics. Keys are PascalCase (`Mass`, not `mass`).
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "PascalCase", default)]
struct ShipPhysics {
    /// Mass (default 4000).
    mass: f64,
    /// Maximum engine thrust (default 60000).
    max_thrust: f64,
    /// Top speed (default 8).
    max_speed: f64,
    /// Rotational friction (default 20).
    rotational_friction: f64,
    /// Maximum angular acceleration (default 3).
    max_angular_acceleration: f64,
    /// Collision box minimum corner, up to 3 numbers (x, y, z; missing ones are 0). Default: from the model.
    min_extents: Option<Vec<f64>>,
    /// Collision box maximum corner. Default: from the model.
    max_extents: Option<Vec<f64>>,
    /// Centre of mass. Default: from the model.
    center_of_mass: Option<Vec<f64>>,
}
impl Default for ShipPhysics {
    fn default() -> Self {
        Self {
            mass: 4000.0,
            max_thrust: 60000.0,
            max_speed: 8.0,
            rotational_friction: 20.0,
            max_angular_acceleration: 3.0,
            min_extents: None,
            max_extents: None,
            center_of_mass: None,
        }
    }
}

/// One hull section.
#[derive(Deserialize, JsonSchema)]
struct SectionDoc {
    /// The model layer (geometry.json key) this section covers.
    layer: String,
    /// Material, which picks the damage multiplier.
    material: String,
    /// Hit points of the section.
    hp: f64,
    /// Share of the section's damage that also hits the core.
    core_pct: f64,
    /// Losing it sinks the ship.
    vital_ship: bool,
    /// Losing it stops the ship.
    vital_speed: bool,
    /// Losing it stops the ship turning.
    vital_turn: bool,
    /// Losing it fails a mission that asks for the section (default `false`).
    #[serde(default)]
    vital_mission: bool,
    /// Absent in imports that predate it: every section swapped then (default `true`).
    #[serde(default = "yes")]
    swappable: bool,
    /// Break-up effect when the section is destroyed.
    destruct: Option<DestructDoc>,
    /// Debris model and pivot.
    debris: Option<DebrisDoc>,
    /// Effect played by the debris.
    debris_fx: Option<String>,
    /// Layers of the neighbouring sections (empty when absent).
    #[serde(default)]
    adjacent: Vec<String>,
    /// Critical hits this section can suffer (empty when absent).
    #[serde(default)]
    criticals: Vec<String>,
}
fn yes() -> bool {
    true
}
/// How a destroyed section breaks up.
#[derive(Deserialize, JsonSchema)]
struct DestructDoc {
    /// Effect name.
    fx: Option<String>,
    /// Seconds after destruction when the effect starts.
    start: f64,
    /// Seconds after destruction when the effect ends.
    end: f64,
    /// Where it plays, ship frame, game axes.
    point: [f64; 3],
}
/// Debris a destroyed section leaves.
#[derive(Deserialize, JsonSchema)]
struct DebrisDoc {
    /// Debris model name.
    model: String,
    /// Pivot point, ship frame, game axes.
    pivot: [f64; 3],
}
/// One gun mount.
#[derive(Deserialize, JsonSchema)]
struct MountDoc {
    /// Position, ship frame, game axes (x starboard, y forward, z up). Default: the model's hardpoint node of the mount's label.
    pos: Option<[f64; 3]>,
    /// Rotation, as a crew node's `rot`. Default: the model's hardpoint node.
    rot: Option<[f64; 3]>,
    /// Firing arc in degrees.
    arc_deg: f64,
    /// The gun fitted: a key of `data/weapons/guns.json`.
    gun: String,
    /// Weapon-panel bank (`Bank1`, ...). Default: the nearest weapon-panel slot.
    bank: Option<String>,
    /// The section layer the mount sits on (none when absent).
    parent_layer: Option<String>,
}
/// One engine port.
#[derive(Deserialize, JsonSchema)]
struct EngineDoc {
    /// Name of the port.
    label: Option<String>,
    /// The section layer it belongs to.
    parent_layer: Option<String>,
    /// Position, ship frame, game axes.
    pos: Option<[f64; 3]>,
    /// Rotation; the first number is the yaw about the up axis.
    rot: Option<[f64; 3]>,
    /// Exhaust effect.
    fx: Option<String>,
}
/// One wake emitter.
#[derive(Deserialize, JsonSchema)]
struct WakeDoc {
    /// Position, ship frame, game axes.
    pos: [f64; 3],
    /// Effect name.
    fx: String,
}
/// One flag attachment point.
#[derive(Deserialize, JsonSchema)]
struct FlagDoc {
    /// Name; the pole's ends contain `flag` and `top` / `bot`.
    label: String,
    /// Position, ship frame, game axes.
    pos: [f64; 3],
}
/// A bounding sphere.
#[derive(Deserialize, JsonSchema)]
struct SphereDoc {
    /// Centre, ship frame, game axes.
    center: [f64; 3],
    /// Radius.
    radius: f64,
}
#[derive(Deserialize)]
struct GeometryDoc {
    aabb: [[f64; 3]; 2],
    tris: Vec<f64>,
}

pub(super) fn point(v: [f64; 3]) -> Vec3 {
    conv(v[0] as f32, v[1] as f32, v[2] as f32)
}

/// A `rot` triple of a gun (or a crew node) as the node's rotation; preserves the rolled frame of inverted mounts.
pub(crate) fn mount_rotation(rot: [f64; 3]) -> Quat {
    let r = rot.map(|v| v as f32);
    Quat::from_rotation_z(-r[1]) * Quat::from_rotation_y(r[0]) * Quat::from_rotation_x(r[2])
}

fn optional_point(v: Option<&Vec<f64>>) -> Vec3 {
    point(std::array::from_fn(|i| v.and_then(|v| v.get(i)).copied().unwrap_or(0.0)))
}

/// Ship definition and referenced geometry. Public API remains fail-fast like the other required loaders.
pub fn load_class(root: &Path, name: &str, guns: &HashMap<String, GunDef>) -> Class {
    try_load_class(root, name, guns).unwrap_or_else(|e| panic!("{e}"))
}

/// `load_class` that returns the error (it names the file and the field) instead of panicking; used by `--check-mod`.
pub fn try_load_class(root: &Path, name: &str, guns: &HashMap<String, GunDef>) -> Result<Class, String> {
    let path = root.join(format!("ships/{name}/ship.json"));
    let mut j: ShipDoc = read_typed(&path)?;
    let glb = format!("ships/{}/model.glb", j.model.as_deref().unwrap_or(name));
    let model_bytes = std::fs::read(root.join(&glb)).unwrap_or_default();
    let facts = derive::read(&model_bytes);
    let geo_path = root.join(format!("ships/{}/geometry.json", j.geometry.as_deref().unwrap_or(name)));
    // A ship with no geometry.json of its own is built from its model (docs/modding.md "A ship from a model").
    let (geometry, geo_context): (HashMap<String, GeometryDoc>, String) = match &facts {
        Some(f) if j.geometry.is_none() && !geo_path.is_file() => {
            (f.layers.iter().map(|l| (l.name.clone(), GeometryDoc { aabb: l.aabb, tris: l.tris.clone() })).collect(), format!("{glb} (derived)"))
        }
        _ => (read_typed(&geo_path)?, asset_context(&geo_path)),
    };
    if let Some(f) = &facts {
        derive::complete(&mut j, f, root);
    }
    let context = asset_context(&path);
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
                swappable: s.swappable,
                destruct: s.destruct.as_ref().map(|d| Destruct { fx: d.fx.clone(), start: d.start as f32, end: d.end as f32, point: point(d.point) }),
                debris: s.debris.as_ref().map(|d| (d.model.clone(), point(d.pivot))),
                debris_fx: s.debris_fx.clone(),
                criticals: s.criticals.clone(),
                adjacent: s.adjacent.iter().filter_map(|l| j.sections.iter().position(|(_, o)| o.layer == *l)).collect(),
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
            let (Some(pos), Some(rot)) = (g.pos, g.rot) else {
                return Err(format!("{context}: guns.{label} has no pos/rot and the model has no hardpoint node named {label:?}"));
            };
            Ok(Mount {
                pos: point(pos),
                rot: mount_rotation(rot),
                arc: (g.arc_deg as f32).to_radians(),
                gun: g.gun.clone(),
                bank: g.bank.as_deref().and_then(|b| b.trim_start_matches("Bank").parse().ok()).unwrap_or(0),
                parent: g.parent_layer.as_deref().and_then(|l| j.sections.iter().position(|(_, s)| s.layer == l)),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let crew_nodes = crew_nodes::nodes(&j.crew_nodes, &format!("{context}: crew_nodes"))?;
    let hull = |f: &dyn Fn(&Section) -> bool| sections.iter().filter(|s| f(s)).fold((Vec3::MAX, Vec3::MIN), |(a, b), s| (a.min(s.min), b.max(s.max)));
    let (hmn, hmx) = hull(&|s| s.vital);
    let (amn, amx) = hull(&|_| true);
    let ph = &j.physics;
    let engage = if mounts.is_empty() { 60.0 } else { mounts.iter().map(|m| guns[&m.gun].range[0]).sum::<f32>() / mounts.len() as f32 };
    let bounds = if ph.min_extents.is_some() && ph.max_extents.is_some() {
        let (a, b) = (optional_point(ph.min_extents.as_ref()), optional_point(ph.max_extents.as_ref()));
        [a.min(b), a.max(b)]
    } else {
        [amn, amx]
    };
    let nodes = glb_json(&model_bytes).map(|(doc, _)| doc);
    let node = |kind: &str| nodes.as_ref()?["nodes"].as_array()?.iter().find(|n| n["extras"]["kind"] == kind);
    let translation = |n: &Value| Vec3::new(num(&n["translation"][0]), num(&n["translation"][1]), num(&n["translation"][2]));
    let node_point = |kind: &str| node(kind).map(translation);
    // The cloak's two effect points (MSB 'CloakingEffectPoint' / 'FullyCloakedEffectPoint'): where Cloaking/DeCloaking and the
    // FullyCloaked periscope trail play; the second faces aft (rot_raw is a yaw about the hull's up axis).
    let effect_point = |kind: &str| node(kind).map(|n| (translation(n), Quat::from_rotation_y(num(&n["extras"]["rot_raw"][0]))));
    let cloak_points = effect_point("CloakingEffectPoint").zip(effect_point("FullyCloakedEffectPoint")).map(|(a, b)| [a, b]);
    let hull_bound = sections.iter().map(|s| s.min.abs().max(s.max.abs()).length()).fold(0.0, f32::max);
    let centre = if hmn.x <= hmx.x { (hmn + hmx) / 2.0 } else { (amn + amx) / 2.0 };
    let radius = 0.5 * (amx - amn).x.max((amx - amn).z) + 2.0;
    // The retail hulls spell the pole's two points many ways (Flag_Top, Top_Flag, flag_top, Top_FlagPoint, Flag_Botom ...).
    let flag_at = |end: &str| {
        let flags = j.flags.as_ref()?;
        flags.iter().find(|f| f.label.to_lowercase().contains("flag") && f.label.to_lowercase().contains(end)).map(|f| point(f.pos))
    };
    let flag = flag_at("top").zip(flag_at("bot"));
    let engines = j
        .engines
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let section = e.parent_layer.as_deref().and_then(|l| sections.iter().position(|s| s.layer == l));
            Some((point(e.pos?), Quat::from_rotation_y(e.rot.map_or(0.0, |r| r[0] as f32)), e.fx?, section))
        })
        .collect();
    Ok(Class {
        name: name.into(),
        race: j.race,
        skirmish: j.skirmish,
        glb,
        scene: j.scene,
        mounts,
        sections,
        core_hp: j.core_hp.unwrap_or(100.0) as f32,
        phys: Physics {
            mass: ph.mass as f32,
            thrust: ph.max_thrust as f32,
            max_speed: ph.max_speed as f32,
            rot_friction: ph.rotational_friction as f32,
            max_ang_acc: ph.max_angular_acceleration as f32,
        },
        flag,
        wake: j.wakes.iter().flatten().map(|w| (point(w.pos), w.fx.clone())).collect(),
        engines,
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
        cloakable: j.cloakable.unwrap_or_else(|| name.contains("Submersible")),
        fuel: j.fuel.unwrap_or(false),
        cloak_points,
        points: j.points as u32,
        crew_posts: j.crew_posts.as_ref().map_or(0, |v| v.values().sum::<u64>() as u32),
        crew: ["Gunners", "Captains", "First Mates", "Navigators", "Engineers", "Riggers", "Fighters", "Lookouts"]
            .map(|k| j.crew_posts.as_ref().and_then(|v| v.get(k)).map_or(0, |n| *n as u32)),
        crew_nodes,
        boarding_points: facts.iter().flat_map(|f| f.points.iter()).filter(|p| p.kind == "BoardingEffectPoint").map(|p| point(p.pos)).collect(),
        display_name: j.display_name,
        icon: j.icon,
        length: ph.max_extents.as_ref().and_then(|v| v.get(1)).zip(ph.min_extents.as_ref().and_then(|v| v.get(1))).map_or((amx - amn).z, |(a, b)| (a - b) as f32),
        collision_bounds: bounds,
        mass_centre: optional_point(ph.center_of_mass.as_ref()),
        lifeboats: j.lifeboats as u32,
        is_lifeboat: j.is_lifeboat,
        sphere: j.sphere.map_or((centre, radius), |s| (point(s.center), s.radius as f32)),
        // PROVISIONAL hull-end fallback for custom models without authored tow/homing nodes.
        // Not retail: `tow` and `homing` in ship.json win over the model's nodes.
        tow_points: match &j.tow {
            Some(t) => [point(t.tower), point(t.towee)],
            None => [node_point("TowerPoint").unwrap_or(Vec3::new(0.0, 0.0, bounds[1].z)), node_point("ToweePoint").unwrap_or(Vec3::new(0.0, 0.0, bounds[0].z))],
        },
        homing_point: j.homing.map(point).or_else(|| node_point("TorpedoHomingPoint")).unwrap_or((hmn + hmx) * 0.5),
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
    fn crew_nodes_in_ship_json_reach_the_class_and_a_bad_one_names_file_and_field() {
        let mut doc = ship();
        doc["crew_nodes"] = serde_json::json!([{"name": "Crew_Walk_P_0", "pos": [1, 2, 3]}, {"name": "Crew_Post_Captain", "pos": [0, 0, 0], "rot": [0.5, 0, 0]}]);
        let c = Fixture::new(doc.clone(), geometry()).load().unwrap();
        assert_eq!(c.crew_nodes.iter().map(|n| n.0.as_str()).collect::<Vec<_>>(), ["Crew_Walk_P_0", "Crew_Post_Captain"]);
        assert_eq!(c.crew_nodes[0].1.translation, Vec3::new(1.0, 3.0, -2.0), "game axes, as a gun's pos");
        assert!(Fixture::new(ship(), geometry()).load().unwrap().crew_nodes.is_empty());
        doc["crew_nodes"][1].as_object_mut().unwrap().remove("pos");
        let error = Fixture::new(doc.clone(), geometry()).load().err().unwrap();
        assert!(error.contains("ship.json") && error.contains("pos"), "{error}");
        doc["crew_nodes"][1] = serde_json::json!({"name": "Crew_Dance_0", "pos": [0, 0, 0]});
        let error = Fixture::new(doc, geometry()).load().err().unwrap();
        assert!(error.contains("ship.json") && error.contains("crew_nodes[1].name") && error.contains("Crew_Dance_0"), "{error}");
    }
    #[test]
    fn a_bad_ship_in_a_mod_is_an_error_naming_the_mods_own_file_not_a_panic() {
        let dir = std::env::temp_dir().join(format!("procyon-badmod-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (base, m, out) = (dir.join("assets"), dir.join("bad_mod"), dir.join("overlay"));
        for d in ["ships/Synthetic", "data/weapons"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        std::fs::create_dir_all(m.join("ships/Synthetic")).unwrap();
        std::fs::write(base.join("ships/Synthetic/ship.json"), ship().to_string()).unwrap();
        std::fs::write(base.join("ships/Synthetic/geometry.json"), geometry().to_string()).unwrap();
        std::fs::write(base.join("data/weapons/guns.json"), "{}").unwrap();
        std::fs::write(base.join("data/weapons/bullets.json"), "{}").unwrap();
        let mod_file = m.join("ships/Synthetic/ship.json");
        std::fs::write(&mod_file, r#"{"extends": "Synthetic", "crew_nodes": [{"name": "Crew_Bogus_1", "pos": [0, 0, 0]}]}"#).unwrap();
        crate::modding::overlay_into(&base, &[m], &out).unwrap();
        let source = std::fs::canonicalize(&mod_file).unwrap();
        let error = Library::try_load(&out).err().expect("bad data is an Err, not a panic");
        assert!(error.contains("crew_nodes[0].name") && error.contains("Crew_Bogus_1"), "{error}");
        assert!(error.contains(&format!("(source {})", source.display())), "the source is the mod's own file: {error}");
        assert!(!error.contains(&format!("(source {}", out.display())), "{error}");
        // the same mod without the bad entry loads
        std::fs::write(&mod_file, r#"{"extends": "Synthetic", "core_hp": 7}"#).unwrap();
        crate::modding::overlay_into(&base, &[dir.join("bad_mod")], &out).unwrap();
        assert_eq!(Library::try_load(&out).ok().map(|l| l.classes.len()), Some(1));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn tow_and_homing_points_in_ship_json_win_over_the_hull_ends_and_absent_keys_change_nothing() {
        let plain = Fixture::new(ship(), geometry()).load().unwrap();
        let mut doc = ship();
        doc["tow"] = serde_json::json!({"tower": [1, 2, 3], "towee": [4, 5, 6]});
        doc["homing"] = serde_json::json!([7, 8, 9]);
        let c = Fixture::new(doc, geometry()).load().unwrap();
        assert_eq!(c.tow_points, [Vec3::new(1.0, 3.0, -2.0), Vec3::new(4.0, 6.0, -5.0)], "game axes, as a gun's pos");
        assert_eq!(c.homing_point, Vec3::new(7.0, 9.0, -8.0));
        // Only one given: the other keeps its fallback.
        let mut half = ship();
        half["homing"] = serde_json::json!([1, 1, 1]);
        let h = Fixture::new(half, geometry()).load().unwrap();
        assert_eq!(h.tow_points, plain.tow_points);
        assert_ne!(h.homing_point, plain.homing_point);
        // A tow without both ends is an error naming the file.
        let mut bad = ship();
        bad["tow"] = serde_json::json!({"tower": [1, 2, 3]});
        let error = Fixture::new(bad, geometry()).load().err().unwrap();
        assert!(error.contains("ship.json") && error.contains("towee"), "{error}");
    }
    #[test]
    fn a_schema_key_in_ship_json_is_ignored() {
        let mut doc = ship();
        doc["$schema"] = serde_json::json!("https://example.invalid/ship.schema.json");
        assert!(Fixture::new(doc, geometry()).load().is_ok());
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
