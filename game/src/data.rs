//! Ship, gun and bullet definitions loaded from `assets/data` (format: docs/data-format.md).
use bevy::prelude::*;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

mod ships;
pub use ships::load_class;

fn asset_context(path: &std::path::Path) -> String {
    let source = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    format!("{} (source {})", path.display(), source.display())
}

/// Typed required asset with the resolved source path and serde's field/line/column diagnostic.
fn read_typed<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Result<T, String> {
    let context = asset_context(path);
    let bytes = std::fs::read(path).map_err(|e| format!("{context}: {e}; run tools/build_assets.sh if assets are missing"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{context}: {e}"))
}

/// Asset path of a converted model given its file name (`NavyCutterShip.glb`): GLBs live in `models/<category>/` (tools/models.py);
/// the folder is found by scanning once, so mods may add models anywhere. Unknown names fall back to the asset root.
pub fn model(file: &str) -> String {
    static INDEX: OnceLock<HashMap<String, String>> = OnceLock::new();
    let index = INDEX.get_or_init(|| {
        let mut files = vec![];
        crate::modding::walk(&crate::assets_dir().join("models"), std::path::Path::new("models"), &mut files);
        files.iter().filter_map(|p| Some((p.file_name()?.to_str()?.to_string(), p.to_string_lossy().replace('\\', "/")))).collect()
    });
    index.get(file).cloned().unwrap_or_else(|| file.to_string())
}

/// The scene of a converted model (`model(file)`), ready to spawn.
pub fn scene(assets: &AssetServer, file: &str) -> WorldAssetRoot {
    WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(model(file))))
}

/// Converted single-material mesh geometry; malformed or unsupported GLBs fail without indexing outside buffers.
pub fn mesh_geometry(root: &std::path::Path, name: &str) -> Option<(Vec<Vec3>, Vec<[f32; 2]>, Vec<u32>)> {
    let bytes = std::fs::read(root.join(model(&format!("{name}.glb")))).ok()?;
    let (doc, end) = glb_json(&bytes)?;
    let start = end.checked_add(8)?;
    let length = u32::from_le_bytes(bytes.get(end..end.checked_add(4)?)?.try_into().ok()?) as usize;
    if bytes.get(end.checked_add(4)?..start)? != b"BIN\0" {
        return None;
    }
    let bin = bytes.get(start..start.checked_add(length)?)?;
    let prims = doc["meshes"][0]["primitives"].as_array()?;
    if prims.len() != 1 {
        return None;
    }
    let acc = |index: u64, width: usize| -> Option<(&[u8], u64)> {
        let a = &doc["accessors"][index as usize];
        let view = &doc["bufferViews"][a["bufferView"].as_u64()? as usize];
        let local = a["byteOffset"].as_u64().unwrap_or(0) as usize;
        let offset = (view["byteOffset"].as_u64().unwrap_or(0) as usize).checked_add(local)?;
        let length = (a["count"].as_u64()? as usize).checked_mul(width)?;
        if view["buffer"].as_u64().unwrap_or(0) != 0
            || view["byteStride"].as_u64().is_some_and(|stride| stride != width as u64)
            || local.checked_add(length)? > view["byteLength"].as_u64()? as usize
        {
            return None;
        }
        Some((bin.get(offset..offset.checked_add(length)?)?, a["componentType"].as_u64()?))
    };
    let p = &prims[0];
    let (positions, pt) = acc(p["attributes"]["POSITION"].as_u64()?, 12)?;
    let (uvs, ut) = acc(p["attributes"]["TEXCOORD_0"].as_u64()?, 8)?;
    let index = p["indices"].as_u64()?;
    let it = doc["accessors"][index as usize]["componentType"].as_u64()?;
    if pt != 5126 || ut != 5126 || ![5123, 5125].contains(&it) {
        return None;
    }
    let (indices, _) = acc(index, if it == 5123 { 2 } else { 4 })?;
    let f = |b: &[u8]| f32::from_le_bytes(b.try_into().unwrap());
    let pos: Vec<_> = positions.as_chunks::<12>().0.iter().map(|b| Vec3::new(f(&b[0..4]), f(&b[4..8]), f(&b[8..12]))).collect();
    let uv: Vec<_> = uvs.as_chunks::<8>().0.iter().map(|b| [f(&b[0..4]), f(&b[4..8])]).collect();
    let idx: Vec<_> = indices
        .chunks_exact(if it == 5123 { 2 } else { 4 })
        .map(|b| if it == 5123 { u16::from_le_bytes(b.try_into().unwrap()) as u32 } else { u32::from_le_bytes(b.try_into().unwrap()) })
        .collect();
    (pos.len() == uv.len() && pos.iter().all(|p| p.is_finite()) && uv.iter().flatten().all(|v| v.is_finite()) && idx.iter().all(|i| (*i as usize) < pos.len()))
        .then_some((pos, uv, idx))
}

/// The JSON chunk of a binary glTF 2.0 file and the offset where it ends (the BIN chunk header follows); None unless the header is valid.
fn glb_json(bytes: &[u8]) -> Option<(Value, usize)> {
    if bytes.get(0..4)? != b"glTF" || bytes.get(4..8)? != 2u32.to_le_bytes() || bytes.get(16..20)? != b"JSON" {
        return None;
    }
    let size = u32::from_le_bytes(bytes.get(12..16)?.try_into().ok()?) as usize;
    let end = 20usize.checked_add(size)?;
    Some((serde_json::from_slice(bytes.get(20..end)?).ok()?, end))
}

pub struct AsteroidDef {
    pub scene: String,
    pub hp: f32,
    pub mass: f32,
    pub radius: f32,
    pub children: Vec<String>,
    pub effect: String,
}
#[derive(Resource)]
pub struct AsteroidDefs(pub HashMap<String, AsteroidDef>);
impl FromWorld for AsteroidDefs {
    fn from_world(_: &mut World) -> Self {
        let doc = read_json(&crate::assets_dir().join("data/asteroids.json"));
        let defs: HashMap<_, _> = doc
            .as_object()
            .expect("Asteroid type map")
            .iter()
            .map(|(name, d)| {
                let num = |key: &str| {
                    let v = d[key].as_f64().expect("Asteroid number") as f32;
                    assert!(v.is_finite() && v > 0.0, "Invalid asteroid {name}: {key}");
                    v
                };
                let text = |key: &str| d[key].as_str().expect("Asteroid string").to_string();
                let children: Vec<_> = d["children"].as_array().expect("Asteroid children").iter().map(|v| v.as_str().expect("Asteroid child").to_string()).collect();
                assert_eq!(children.len(), 10, "Asteroid death slots: {name}");
                (name.clone(), AsteroidDef { scene: text("scene"), hp: num("hp"), mass: num("mass"), radius: num("radius"), children, effect: text("effect") })
            })
            .collect();
        fn visit(name: &str, defs: &HashMap<String, AsteroidDef>, ancestors: &mut Vec<String>) {
            assert!(!ancestors.iter().any(|a| a == name), "Cyclic asteroid breakup: {name}");
            let d = defs.get(name).unwrap_or_else(|| panic!("Unknown asteroid child: {name}"));
            ancestors.push(name.into());
            for child in d.children.iter().filter(|c| !c.is_empty()) {
                visit(child, defs, ancestors);
            }
            ancestors.pop();
        }
        for name in defs.keys() {
            visit(name, &defs, &mut vec![]);
        }
        Self(defs)
    }
}

/// The retail torpedo bullet (WOT `BulletTypeName_Primary`); aiming, launch and crew calls treat it specially.
pub const TORPEDO_BULLET: &str = "Bullet_Torpedo";

pub struct GunDef {
    pub mount_type: Option<String>,
    pub points: Option<u32>,
    pub exclusion: Option<String>,
    pub point_defense: bool, // WOT `Is PointDefense Gun`: automatic anti-torpedo/mine gun, never fires at ships
    pub muzzle: f32,
    pub reload: f32,
    pub burst: u32,
    pub burst_reload: f32,
    pub range: [f32; 3], // effective, long, maximum
    /// Per range band (effective, long, maximum): the ricochet factor a shell of this gun takes when fired (0 when absent).
    pub ricochet: [f32; 3],
    pub deviation: [f32; 3], // per range band (effective, long, maximum): accuracy deviation (rad); PROVISIONAL: used as max cone half-angle
    pub gravity: bool,
    pub bullet: String,
    pub base: Option<String>,   // glb of the turning base
    pub barrel: Option<String>, // glb of the pitching barrel
    pub pivot: Vec3,            // barrel pivot in the base frame
    pub muzzles: Vec<Vec3>,     // muzzle points in the base frame, like the barrel mesh (see `combat::spawn_gun`)
    /// Turret base turn rate (rad/s, WOT HorizontalRotationSpeed).
    pub yaw_speed: f32,
    /// Barrel elevation rate (rad/s, WOT VerticalRotationSpeed).
    pub pitch_speed: f32,
    /// Barrel elevation limits (rad, WOT VerticalMin/MaxAngle).
    pub pitch_limits: [f32; 2],
    pub muzzle_fx: Option<String>,
    pub sound: Option<(String, f32, f32)>, // fire sound: name, volume, max distance
    /// WOT 'Is Lobbing Gun' with its 'LobAngle' (rad): mortars and gravity charges fire on this fixed high arc.
    pub lob: Option<f32>,
    /// WOT 'Can Be Fired While Cloaked' (torpedoes and gatlings).
    pub cloak_fire: bool,
}
impl GunDef {
    /// Gravity on this gun's shells (units/s^2): MuzzleSpeed^2 / (sqrt(2) * MaximumRange), so a shell fired at 45 degrees
    /// would carry sqrt(2) times the maximum range. Measured on retail shells of seven guns (docs/re/gunnery.md#shell-gravity);
    /// 0 for guns not flagged BulletAffectedByGravity.
    /// Time from the first shot of a burst to the next burst: TimeBetweenBursts (seconds) runs from the burst's last shot.
    /// Measured on retail gattling bursts (8 shots 0.15 s apart, a new burst every ~4.1 s) and medium cannons (9.0 s).
    pub fn burst_cycle(&self) -> f32 {
        self.reload + self.burst.saturating_sub(1) as f32 * self.burst_reload
    }
    /// The burst cycle with `cut` seconds taken off TimeBetweenBursts by the gunners (never below 0).
    pub fn burst_cycle_cut(&self, cut: f32) -> f32 {
        (self.reload - cut).max(0.0) + self.burst.saturating_sub(1) as f32 * self.burst_reload
    }

    pub fn shell_gravity(&self) -> f32 {
        if self.gravity && self.range[2] > 0.0 { self.muzzle * self.muzzle / (std::f32::consts::SQRT_2 * self.range[2]) } else { 0.0 }
    }
}
/// What a bullet does besides hitting a hull: its WOT custom-info factory (`kind`) and the fields that go with it.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Payload {
    /// Plain shell or torpedo: damage where it strikes.
    #[default]
    Shell,
    /// Star mortar (and the Nova mortar's secondaries, mission 9's dark matter): a blast sphere growing to `radius` over `duration`.
    Blast { radius: f32, duration: f32 },
    /// Nova mortar: bursts into secondary blasts where it lands.
    Nova,
    /// Gravity charge: a well of `radius` that holds ships for `duration` seconds.
    Grav { radius: f32, duration: f32 },
    /// Mine: drifts at `speed` toward ships within `sight` and bursts on contact.
    Mine { sight: f32, speed: f32 },
}
pub struct BulletDef {
    pub glb: Option<String>,
    pub physics: Option<Physics>,
    pub lifetime: f32,
    /// WOT bullet HitPoints: how much the projectile absorbs before it is destroyed (torpedo 16, light Lancer bolt 1).
    pub hitpoints: f32,
    pub impact_snd: HashMap<String, Vec<String>>, // material -> pool, one picked at random per hit
    pub impact_vol: f32,
    pub impact_max: f32,
    pub hit_fx: Option<String>,
    pub bullet_fx: Option<String>,
    pub damage: HashMap<String, [f32; 5]>, // material -> [lower, upper, crit chance, fire chance, initial fire strength]
    pub payload: Payload,
    /// Looping flight sound (name, volume, max distance) and per-material ricochet sound.
    pub travel_snd: Option<(String, f32, f32)>,
    pub ricochet_snd: HashMap<String, String>,
    /// 'Decal Damage Size' class of the scar it leaves (0 Small, 1 Medium, 2 Large).
    pub decal: Option<u8>,
}
pub struct Mount {
    pub pos: Vec3,
    pub rot: Quat, // mount orientation in the ship frame (yaw, plus roll for inverted mounts)
    pub arc: f32,  // radians; PROVISIONAL: WOT placement angle is interpreted as the total horizontal arc
    pub gun: String,
    pub bank: usize,           // weapon-panel slot (WOT `BankNN`; indexes WeaponBar_SlotOffset_From_TopLeft_NN)
    pub parent: Option<usize>, // damage section that carries this gun; destroyed section => gun disabled
}
#[derive(Default)]
pub struct Section {
    pub label: String,
    pub layer: String, // section mesh node, e.g. NavyCutterShip_Layer1
    pub material: String,
    pub hp: f32,
    pub core_pct: f32,
    pub min: Vec3,
    pub max: Vec3,
    pub vital: bool,
    pub speed: bool,   // vitalToMaxVelocity
    pub turn: bool,    // vitalToManeuverability
    pub mission: bool, // vitalToMission (bases: what the mission's 'Vital Section damage' rules watch)
    pub tris: Vec<[Vec3; 3]>,
    /// How the section breaks away when the ship is destroyed (WOT `destructFx`); None for imports that predate it.
    pub destruct: Option<Destruct>,
    /// The piece that breaks off when the section is destroyed mid-fight (WOT `debrisMeshScene`): its GLB file name (found with
    /// `model`, in the hull's model space) and its pivot (bevy axes). None for sections without one and for older imports.
    pub debris: Option<(String, Vec3)>,
    /// Effect trailing that piece (WOT `debrisFx`, e.g. Falling_Debris).
    pub debris_fx: Option<String>,
    /// Neighbouring sections (indices) a destroyed section passes damage on to (WOT `adjacent`).
    pub adjacent: Vec<usize>,
}
/// A section's part of a ship's final breakup (docs/re/effects.md "Ship destruction"): its mesh flies at `vel` (ship-local, bevy
/// axes, units/s) from the moment of death, bursts with effect `fx` (if any) at `start` seconds and is gone at `end`.
#[derive(Clone, Debug, PartialEq)]
pub struct Destruct {
    pub fx: Option<String>,
    pub start: f32,
    pub end: f32,
    pub vel: Vec3,
}
pub struct Physics {
    pub mass: f32,
    pub thrust: f32,
    pub max_speed: f32,
    pub rot_friction: f32,
    pub max_ang_acc: f32,
}
pub struct Class {
    pub name: String,
    pub race: String,   // "Navy", "Pirate", ...
    pub skirmish: bool, // offered in the skirmish ship pickers
    pub glb: String,
    pub mounts: Vec<Mount>,
    pub sections: Vec<Section>,
    pub core_hp: f32,
    pub phys: Physics,
    pub centre: Vec3,
    pub radius: f32,
    /// Farthest corner of any section box from the ship origin: a shell farther than this from the hull cannot hit.
    pub hull_bound: f32,
    pub engage: f32,                        // mean effective range of its guns; AI keeps about this distance
    pub flag: Option<(Vec3, Vec3)>,         // pole attachment points: top, bottom (Bevy ship frame)
    pub engines: Vec<(Vec3, Quat, String)>, // exhaust effect placements
    pub explosion_fx: Option<String>,
    pub engine_snd: [Option<String>; 3], // loops for Half, Full, Emergency
    /// WOT 'Selected Indicator Percentage Of Radius': the selection ring's radius as a fraction of `radius`.
    pub ring: f32,
    /// AI 'Default SightRange'; the original's dock/focus "in range" test is this sight test (docs/re/docking-and-orders.md §1).
    pub sight: f32,
    /// 'Repair Effect Name': picks the repair time class and the effect played while docked.
    pub repair_fx: Option<String>,
    /// WOT 'Is Cloakable' (only the Procyon submersible).
    pub cloakable: bool,
    /// Fleet-screen facts from the WOT: 'Victory Point Cost', total crew posts ('Max number of ...'), display-name string id and
    /// ship-bar icon sprite.
    pub points: u32,
    pub crew_posts: u32,
    pub display_name: Option<String>,
    pub icon: Option<String>,
    /// Bow-to-stern length of the WOT collision box (MaxExtents.y - MinExtents.y); the dock routine sizes its approach legs with it.
    pub length: f32,
    /// Authored collision box in Bevy's local frame; inherited ship mods retain/override these physics fields.
    pub collision_bounds: [Vec3; 2],
    pub mass_centre: Vec3,
    /// WOT `Number of lifeboats`: boats left by a destroyed ship that has Create Lifeboats set (docs/re/lifeboats.md).
    pub lifeboats: u32,
    /// WOT `Is Lifeboat`, the only lifeboat test (`Ship_Navy_Lifeboat_Mission9` is a combat ship despite its name).
    pub is_lifeboat: bool,
    /// Collision bounding sphere (Bevy-frame centre, radius): the lifeboat pickup contact test.
    pub sphere: (Vec3, f32),
    pub tow_points: [Vec3; 2], // TowerPoint (stern), ToweePoint (bow)
    pub homing_point: Vec3,
}
impl Class {
    /// The class's HUD icon of a bar ("ShipBar", "WeaponBar"): the WOT icon name (several classes share art, and names differ from
    /// class names, e.g. Ship_Navy_ManOWar -> Navy_Man_O_War), falling back to the class name.
    pub fn icon(&self, bar: &str) -> String {
        let art = self.icon.as_deref().and_then(|i| i.strip_prefix("ShipBar_ShipIcon_")).unwrap_or_else(|| self.name.trim_start_matches("Ship_"));
        format!("{bar}_ShipIcon_{art}")
    }
}

#[derive(Resource)]
pub struct Library {
    pub classes: Vec<Class>,
    pub guns: HashMap<String, GunDef>,
    pub bullets: HashMap<String, BulletDef>,
    /// Per ship type, the pool open skirmish maps name ships from (`ships/<id>/names.json`, WOT 'Available Unique Ship Name').
    pub names: HashMap<String, Vec<String>>,
}
impl Library {
    pub fn index(&self, name: &str) -> usize {
        self.classes
            .iter()
            .position(|c| c.name == name)
            .unwrap_or_else(|| panic!("unknown ship {name}: not built (run tools/build_assets.sh {name}), or a map/mod names a ship without a ships/{name}/ folder"))
    }
}
pub fn num(v: &Value) -> f32 {
    v.as_f64().expect("number in ship json") as f32
}
/// Game frame (x starboard, y bow, z up) -> Bevy (x, z, -y), the same map as the GLB export.
pub fn conv(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, z, -y)
}
pub fn conv_v(v: &Value) -> Vec3 {
    conv(num(&v[0]), num(&v[1]), num(&v[2]))
}

pub fn gun_def(g: &Value) -> GunDef {
    GunDef {
        mount_type: g["Mount Type"].as_str().map(|s| s.split_whitespace().collect::<Vec<_>>().join(" ")),
        points: g["Victory Point Cost"].as_u64().and_then(|v| u32::try_from(v).ok()),
        exclusion: g["Exclusion Race"].as_str().filter(|s| *s != "None").map(String::from),
        point_defense: g["Is PointDefense Gun"].as_bool().unwrap_or(false),
        muzzle: num(&g["MuzzleSpeed"]),
        reload: num(&g["TimeBetweenBursts"]),
        burst: g["BulletsPerBurst"].as_u64().unwrap() as u32,
        burst_reload: num(&g["DuringBurstReloadTime"]),
        range: [num(&g["EffectiveRange_Range"]), num(&g["LongRange_Range"]), num(&g["MaximumRange_Range"])],
        ricochet: ["EffectiveRange", "LongRange", "MaximumRange"].map(|b| g[format!("{b}_RicochetFactor")].as_f64().unwrap_or(0.0) as f32),
        deviation: [num(&g["EffectiveRange_AccuracyDeviation"]), num(&g["LongRange_AccuracyDeviation"]), num(&g["MaximumRange_AccuracyDeviation"])],
        gravity: g["BulletAffectedByGravity"].as_bool().unwrap(),
        bullet: g["BulletTypeName_Primary"].as_str().unwrap().into(),
        base: g["visual"]["base"].as_str().map(String::from),
        barrel: g["visual"]["barrel"].as_str().map(String::from),
        pivot: conv_v(&g["visual"]["pivot"]),
        muzzles: g["visual"]["muzzles"].as_array().unwrap().iter().map(conv_v).collect(),
        yaw_speed: num(&g["HorizontalRotationSpeed"]),
        pitch_speed: num(&g["VerticalRotationSpeed"]),
        pitch_limits: [num(&g["VerticalMinAngle"]), num(&g["VerticalMaxAngle"])],
        muzzle_fx: g["MuzzleFlashEffect"].as_str().map(String::from),
        sound: g["SoundName"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| (s.into(), g["SoundVolume"].as_f64().unwrap_or(1.0) as f32, g["SoundMaxDistance"].as_f64().unwrap_or(600.0) as f32)),
        lob: (g["Is Lobbing Gun"] == true).then(|| g["LobAngle"].as_f64().unwrap_or(0.785) as f32),
        cloak_fire: g["Can Be Fired While Cloaked"].as_bool().unwrap_or_else(|| {
            let b = g["BulletTypeName_Primary"].as_str().unwrap_or_default();
            b.starts_with(TORPEDO_BULLET) || b == "Bullet_Gattling"
        }),
    }
}

pub fn bullet_def(b: &Value) -> BulletDef {
    let physics = b["physics"].as_object().map(|p| {
        let get = |key: &str| p[key].as_f64().unwrap_or(f64::NAN) as f32;
        let physics = Physics {
            mass: get("Mass"),
            thrust: get("MaxThrust"),
            max_speed: get("MaxSpeed"),
            rot_friction: get("RotationalFriction"),
            max_ang_acc: get("MaxAngularAcceleration"),
        };
        assert!(
            [physics.mass, physics.thrust, physics.max_speed, physics.max_ang_acc].iter().all(|v| v.is_finite() && *v > 0.0)
                && physics.rot_friction.is_finite()
                && physics.rot_friction >= 0.0,
            "Invalid bullet physics"
        );
        physics
    });
    BulletDef {
        physics,
        glb: b["glb"].as_str().map(String::from),
        lifetime: num(&b["lifetime"]),
        hitpoints: b["hitpoints"].as_f64().unwrap_or(1.0) as f32,
        impact_vol: num(&b["impact_vol"]),
        impact_max: num(&b["impact_max"]),
        impact_snd: b["impact_snd"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(m, v)| (m.clone(), v.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect()))
            .collect(),
        hit_fx: b["hit_fx"].as_str().map(String::from),
        bullet_fx: b["bullet_fx"].as_str().map(String::from),
        payload: {
            let f = |v: &Value, k: &str| v[k].as_f64().unwrap_or(0.0) as f32;
            match (b["kind"].as_str().unwrap_or(""), &b["blast"], &b["mine"]) {
                (_, _, m) if m.is_object() => Payload::Mine { sight: f(m, "sight"), speed: f(m, "speed") },
                ("NovaMortar", ..) => Payload::Nova,
                ("GravCharge", bl, _) => Payload::Grav { radius: f(bl, "radius"), duration: f(bl, "duration") },
                (_, bl, _) if bl.is_object() => Payload::Blast { radius: f(bl, "radius"), duration: f(bl, "duration") },
                _ => Payload::Shell,
            }
        },
        travel_snd: b["travel_snd"].as_array().and_then(|a| Some((a.first()?.as_str()?.to_string(), a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32))),
        decal: b["decal"].as_str().and_then(|d| ["Small", "Medium", "Large"].iter().position(|x| *x == d)).map(|i| i as u8),
        ricochet_snd: b["ricochet_snd"].as_object().map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()).unwrap_or_default(),
        damage: {
            // Mod bullets may omit materials: missing ones deal no damage instead of panicking in `shells`.
            let mut d: HashMap<String, [f32; 5]> = crate::combat::MATERIALS.iter().map(|m| (m.to_string(), [0.0; 5])).collect();
            d.extend(b["damage"].as_object().unwrap().iter().map(|(m, d)| (m.clone(), [num(&d[0]), num(&d[1]), num(&d[2]), num(&d[3]), num(&d[4])])));
            d
        },
    }
}

/// A retail boolean field: the exporters keep WOT's "TRUE"/"FALSE" strings (any case); anything else is false.
pub fn wot_flag(v: &Value) -> bool {
    v.as_str().is_some_and(|x| x.eq_ignore_ascii_case("TRUE"))
}
/// A JSON file the game cannot run without: panics with the import hint when it is missing or invalid.
pub fn read_json(path: &std::path::Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_else(|_| panic!("missing {path:?}; run tools/build_assets.sh"))).unwrap_or_else(|e| panic!("{path:?}: {e}"))
}
/// An optional JSON file (settings, per-ship extras, data a feature can live without): `None` when missing or invalid.
pub fn try_json(path: &std::path::Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}
/// A map's `maps/<id>/map.json`, if that map was imported.
pub fn map_json(id: &str) -> Option<Value> {
    try_json(&crate::assets_dir().join(format!("maps/{id}/map.json")))
}

/// Every gun and bullet definition (`data/weapons/*.json`), shared by all ships.
pub fn load_weapons(root: &std::path::Path) -> (HashMap<String, GunDef>, HashMap<String, BulletDef>) {
    let table = |file: &str| read_json(&root.join("data/weapons").join(file)).as_object().unwrap().clone();
    (table("guns.json").iter().map(|(k, g)| (k.clone(), gun_def(g))).collect(), table("bullets.json").iter().map(|(k, b)| (k.clone(), bullet_def(b))).collect())
}

/// Ship ids: the folders of `ships/` that hold a `ship.json`, sorted.
pub fn ship_ids(root: &std::path::Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(root.join("ships"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("ship.json").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    ids.sort();
    ids
}

impl Library {
    /// Loads every `ships/<id>/` folder and `data/weapons` from the (mod-overlaid) asset root.
    pub fn load(root: &std::path::Path) -> Library {
        let ids = ship_ids(root);
        assert!(!ids.is_empty(), "no ships in {}/ships; run tools/build_assets.sh", root.display());
        let (guns, bullets) = load_weapons(root);
        let classes = ids.iter().map(|n| load_class(root, n, &guns)).collect();
        let names = ids.iter().filter_map(|n| Some((n.clone(), serde_json::from_value(try_json(&root.join(format!("ships/{n}/names.json")))?).ok()?))).collect();
        Library { classes, guns, bullets, names }
    }
}

#[cfg(test)]
mod bullet_tests {
    #[test]
    fn missing_materials_deal_no_damage_instead_of_panicking() {
        let b = serde_json::json!({"lifetime": 1, "impact_vol": 1, "impact_max": 1, "impact_snd": {}, "damage": {"Wood": [1, 2, 0, 0, 0]}});
        let d = super::bullet_def(&b).damage;
        assert_eq!(d["Wood"][1], 2.0);
        assert_eq!(d["Stone"], [0.0; 5]);
    }
}

#[cfg(test)]
mod lifeboat_tests {
    use super::*;

    #[test]
    #[ignore = "needs imported assets"]
    fn lifeboat_class_fields_load_and_default_for_mods_that_omit_them() {
        let lib = Library::load(crate::assets_dir());
        let c = |n: &str| &lib.classes[lib.index(n)];
        for race in ["Navy", "Civilian", "Pirate", "Procyon"] {
            assert!(c(&format!("Ship_{race}_Lifeboat")).is_lifeboat, "{race}");
        }
        assert!(!c("Ship_Navy_Lifeboat_Mission9").is_lifeboat, "flag, not name");
        assert_eq!([c("Ship_Pirate_Schooner").lifeboats, c("Ship_Pirate_Sloop").lifeboats, c("Ship_Procyon_Dreadnought").lifeboats], [2, 1, 0]);
        let (centre, radius) = c("Ship_Navy_Lifeboat").sphere;
        assert_eq!((centre, radius), (Vec3::new(0.0, 1.0, 0.0), 2.0)); // game (0,0,1) -> Bevy (0,1,0)
        assert_eq!(c("Ship_Navy_Cutter").sphere.1, 12.0);
        // A mod ship.json without the new keys is an ordinary ship with the old derived bounds.
        let root = std::env::temp_dir().join(format!("procyon-lifeboat-defaults-{}", std::process::id()));
        std::fs::create_dir_all(root.join("ships/Ship_Navy_Cutter")).unwrap();
        let mut j = read_json(&crate::assets_dir().join("ships/Ship_Navy_Cutter/ship.json"));
        for k in ["lifeboats", "is_lifeboat", "response", "sphere"] {
            j.as_object_mut().unwrap().remove(k);
        }
        std::fs::write(root.join("ships/Ship_Navy_Cutter/ship.json"), j.to_string()).unwrap();
        std::fs::copy(crate::assets_dir().join("ships/Ship_Navy_Cutter/geometry.json"), root.join("ships/Ship_Navy_Cutter/geometry.json")).unwrap();
        let old = load_class(&root, "Ship_Navy_Cutter", &lib.guns);
        assert_eq!((old.lifeboats, old.is_lifeboat, old.sphere), (0, false, (old.centre, old.radius)));
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glb(json: &[u8]) -> Vec<u8> {
        let mut b = b"glTF".to_vec();
        b.extend(2u32.to_le_bytes());
        b.extend(0u32.to_le_bytes()); // total length: not checked
        b.extend((json.len() as u32).to_le_bytes());
        b.extend(b"JSON");
        b.extend(json);
        b
    }

    #[test]
    fn glb_json_reads_the_json_chunk_and_rejects_bad_headers() {
        let ok = glb(br#"{"nodes":[]}"#);
        let (doc, end) = glb_json(&ok).unwrap();
        assert!(doc["nodes"].is_array());
        assert_eq!(end, ok.len());
        let mut wrong_magic = ok.clone();
        wrong_magic[0] = b'X';
        assert!(glb_json(&wrong_magic).is_none());
        assert!(glb_json(&ok[..ok.len() - 1]).is_none(), "truncated chunk");
        assert!(glb_json(b"glTF").is_none());
    }

    #[test]
    fn wot_flags_accept_any_case_and_nothing_else() {
        for (v, want) in
            [(serde_json::json!("TRUE"), true), (serde_json::json!("True"), true), (serde_json::json!("FALSE"), false), (serde_json::json!(true), false), (Value::Null, false)]
        {
            assert_eq!(wot_flag(&v), want, "{v}");
        }
    }

    #[test]
    fn game_axes_map_to_bevy_like_the_glb_export() {
        assert_eq!(conv(1.0, 2.0, 3.0), Vec3::new(1.0, 3.0, -2.0));
        assert_eq!(crate::game_to_bevy(&serde_json::json!([1.0, 2.0, 3.0])), conv(1.0, 2.0, 3.0));
        assert_eq!(crate::game_to_bevy(&serde_json::json!([1.0])), Vec3::new(1.0, 0.0, 0.0), "missing components read as 0");
    }
}
