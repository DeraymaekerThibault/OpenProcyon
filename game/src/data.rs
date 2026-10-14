//! Ship, gun and bullet definitions loaded from `assets/data` (format: docs/data-format.md).
use bevy::prelude::*;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

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

/// Converted single-material mesh geometry; malformed or unsupported GLBs fail without indexing outside buffers.
pub fn mesh_geometry(root: &std::path::Path, name: &str) -> Option<(Vec<Vec3>, Vec<[f32; 2]>, Vec<u32>)> {
    let bytes = std::fs::read(root.join(model(&format!("{name}.glb")))).ok()?;
    if bytes.get(0..4)? != b"glTF" || bytes.get(4..8)? != 2u32.to_le_bytes() || bytes.get(16..20)? != b"JSON" {
        return None;
    }
    let size = u32::from_le_bytes(bytes.get(12..16)?.try_into().ok()?) as usize;
    let end = 20usize.checked_add(size)?;
    let doc: Value = serde_json::from_slice(bytes.get(20..end)?).ok()?;
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
    pub dev: [f32; 3],   // accuracy deviation (rad); PROVISIONAL: used as max cone half-angle
    pub gravity: bool,
    pub bullet: String,
    pub base: Option<String>,   // glb of the turning base
    pub barrel: Option<String>, // glb of the pitching barrel
    pub pivot: Vec3,            // barrel pivot in the base frame
    pub muzzles: Vec<Vec3>,     // muzzle points in the barrel frame
    pub hspeed: f32,
    pub vspeed: f32,
    pub vlim: [f32; 2],
    pub muzzle_fx: Option<String>,
    pub snd: Option<(String, f32, f32)>, // fire sound: name, volume, max distance
    /// WOT 'Is Lobbing Gun' with its 'LobAngle' (rad): mortars and gravity charges fire on this fixed high arc.
    pub lob: Option<f32>,
    /// WOT 'Can Be Fired While Cloaked' (torpedoes and gatlings).
    pub cloak_fire: bool,
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
    /// AI 'Default SightRange'; the exe's dock/focus "in range" test is this sight test (docs/re/docking-and-orders.md §1).
    pub sight: f32,
    /// 'Repair Effect Name': picks the repair time class and the effect played while docked.
    pub repair_fx: Option<String>,
    /// WOT 'Is Cloakable' (only the Procyon submersible).
    pub cloakable: bool,
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
        dev: [num(&g["EffectiveRange_AccuracyDeviation"]), num(&g["LongRange_AccuracyDeviation"]), num(&g["MaximumRange_AccuracyDeviation"])],
        gravity: g["BulletAffectedByGravity"].as_bool().unwrap(),
        bullet: g["BulletTypeName_Primary"].as_str().unwrap().into(),
        base: g["visual"]["base"].as_str().map(String::from),
        barrel: g["visual"]["barrel"].as_str().map(String::from),
        pivot: conv_v(&g["visual"]["pivot"]),
        muzzles: g["visual"]["muzzles"].as_array().unwrap().iter().map(conv_v).collect(),
        hspeed: num(&g["HorizontalRotationSpeed"]),
        vspeed: num(&g["VerticalRotationSpeed"]),
        vlim: [num(&g["VerticalMinAngle"]), num(&g["VerticalMaxAngle"])],
        muzzle_fx: g["MuzzleFlashEffect"].as_str().map(String::from),
        snd: g["SoundName"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| (s.into(), g["SoundVolume"].as_f64().unwrap_or(1.0) as f32, g["SoundMaxDistance"].as_f64().unwrap_or(600.0) as f32)),
        lob: (g["Is Lobbing Gun"] == true).then(|| g["LobAngle"].as_f64().unwrap_or(0.785) as f32),
        cloak_fire: g["Can Be Fired While Cloaked"].as_bool().unwrap_or_else(|| {
            let b = g["BulletTypeName_Primary"].as_str().unwrap_or_default();
            b.starts_with("Bullet_Torpedo") || b == "Bullet_Gattling"
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

pub fn read_json(path: &std::path::Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_else(|_| panic!("missing {path:?}; run tools/build_assets.sh"))).unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

/// Every gun and bullet definition (`data/weapons/*.json`), shared by all ships.
pub fn load_weapons(root: &std::path::Path) -> (HashMap<String, GunDef>, HashMap<String, BulletDef>) {
    let table = |file: &str| read_json(&root.join("data/weapons").join(file)).as_object().unwrap().clone();
    (table("guns.json").iter().map(|(k, g)| (k.clone(), gun_def(g))).collect(), table("bullets.json").iter().map(|(k, b)| (k.clone(), bullet_def(b))).collect())
}

/// Ship definition `ships/<name>/ship.json` plus its hit meshes (`geometry.json` of the ship named by "geometry"); see docs/data-format.md.
pub fn load_class(root: &std::path::Path, name: &str, guns: &HashMap<String, GunDef>) -> Class {
    let j = read_json(&root.join(format!("ships/{name}/ship.json")));
    let geometry = read_json(&root.join(format!("ships/{}/geometry.json", j["geometry"].as_str().unwrap_or(name))));
    let sections: Vec<Section> = j["sections"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(label, s)| {
            let geo = &geometry[s["layer"].as_str().unwrap()];
            let (a, b) = (conv_v(&geo["aabb"][0]), conv_v(&geo["aabb"][1]));
            let t = geo["tris"].as_array().unwrap();
            Section {
                label: label.clone(),
                layer: s["layer"].as_str().unwrap().into(),
                material: s["material"].as_str().unwrap().into(),
                hp: num(&s["hp"]),
                core_pct: num(&s["core_pct"]),
                min: a.min(b),
                max: a.max(b),
                vital: s["vital_ship"].as_bool().unwrap(),
                speed: s["vital_speed"].as_bool().unwrap(),
                turn: s["vital_turn"].as_bool().unwrap(),
                mission: s["vital_mission"].as_bool().unwrap_or(false),
                tris: t
                    .chunks(9)
                    .map(|c| {
                        let p = |i: usize| conv(num(&c[i]), num(&c[i + 1]), num(&c[i + 2]));
                        [p(0), p(3), p(6)]
                    })
                    .collect(),
            }
        })
        .collect();
    let mounts: Vec<Mount> = j["guns"]
        .as_object()
        .unwrap()
        .values()
        .map(|g| {
            let r = &g["rot"];
            let p = &g["pos"];
            // Game rotation (yaw about up, then angle about the bow axis for inverted mounts, then pitch) -> Bevy axes.
            let rot = Quat::from_rotation_y(num(&r[0])) * Quat::from_rotation_z(-num(&r[1])) * Quat::from_rotation_x(num(&r[2]));
            Mount {
                pos: conv(num(&p[0]), num(&p[1]), num(&p[2])),
                rot,
                arc: num(&g["arc_deg"]).to_radians(),
                gun: g["gun"].as_str().unwrap().into(),
                bank: g["bank"].as_str().and_then(|b| b.trim_start_matches("Bank").parse().ok()).unwrap_or(0),
                parent: g["parent_layer"].as_str().and_then(|l| j["sections"].as_object().unwrap().values().position(|s| s["layer"].as_str() == Some(l))),
            }
        })
        .collect();
    let hull = |f: &dyn Fn(&Section) -> bool| sections.iter().filter(|s| f(s)).fold((Vec3::MAX, Vec3::MIN), |(a, b), s| (a.min(s.min), b.max(s.max)));
    let (hmn, hmx) = hull(&|s| s.vital);
    let (amn, amx) = hull(&|_| true);
    let ph = &j["physics"];
    let p = |k: &str| num(&ph[k]);
    let engage = if mounts.is_empty() { 60.0 } else { mounts.iter().map(|m| guns[&m.gun].range[0]).sum::<f32>() / mounts.len() as f32 };
    let bounds = if ph["MinExtents"].is_array() && ph["MaxExtents"].is_array() {
        let a = crate::game_to_bevy(&ph["MinExtents"]);
        let b = crate::game_to_bevy(&ph["MaxExtents"]);
        [a.min(b), a.max(b)]
    } else {
        [amn, amx]
    };
    let glb = format!("ships/{}/model.glb", j["model"].as_str().unwrap_or(name));
    let model_bytes = std::fs::read(root.join(&glb)).unwrap_or_default();
    let nodes: Option<Value> = model_bytes.get(12..16).and_then(|b| {
        let end = 20usize.checked_add(u32::from_le_bytes(b.try_into().ok()?) as usize)?;
        serde_json::from_slice(model_bytes.get(20..end)?).ok()
    });
    let point = |kind: &str| {
        nodes.as_ref()?["nodes"]
            .as_array()?
            .iter()
            .find(|n| n["extras"]["kind"] == kind)
            .map(|n| Vec3::new(num(&n["translation"][0]), num(&n["translation"][1]), num(&n["translation"][2])))
    };
    let hull_bound = sections.iter().map(|s| s.min.abs().max(s.max.abs()).length()).fold(0.0, f32::max);
    let centre = if hmn.x <= hmx.x { (hmn + hmx) / 2.0 } else { (amn + amx) / 2.0 };
    let radius = 0.5 * (amx - amn).x.max((amx - amn).z) + 2.0;
    Class {
        name: name.into(),
        race: j["race"].as_str().unwrap_or("").into(),
        skirmish: j["skirmish"].as_bool().unwrap_or(false),
        glb,
        mounts,
        sections,
        core_hp: num(&j["core_hp"]),
        phys: Physics { mass: p("Mass"), thrust: p("MaxThrust"), max_speed: p("MaxSpeed"), rot_friction: p("RotationalFriction"), max_ang_acc: p("MaxAngularAcceleration") },
        flag: {
            let at = |l: &str| j["flags"].as_array()?.iter().find(|f| f["label"] == l).map(|f| conv(num(&f["pos"][0]), num(&f["pos"][1]), num(&f["pos"][2])));
            at("Flag_Top").zip(at("Flag_Bottom"))
        },
        engines: j["engines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                let (p, r) = (&e["pos"], &e["rot"]);
                (conv(num(&p[0]), num(&p[1]), num(&p[2])), Quat::from_rotation_y(num(&r[0])), e["fx"].as_str().unwrap().to_string())
            })
            .collect(),
        explosion_fx: j["explosion_fx"].as_str().map(String::from),
        engine_snd: std::array::from_fn(|i| j["engine_snd"][i].as_str().map(String::from)),
        centre,
        radius,
        hull_bound,
        engage,
        sight: j["sight"].as_f64().unwrap_or(400.0) as f32,
        repair_fx: j["repair_fx"].as_str().map(String::from),
        // Older exports lack the flag; the submersible is the only cloakable hull in the retail data.
        cloakable: j["cloakable"].as_bool().unwrap_or_else(|| name.contains("Submersible")),
        length: ph["MaxExtents"][1].as_f64().zip(ph["MinExtents"][1].as_f64()).map_or((amx - amn).z, |(a, b)| (a - b) as f32),
        collision_bounds: bounds,
        mass_centre: crate::game_to_bevy(&ph["CenterOfMass"]),
        lifeboats: j["lifeboats"].as_u64().unwrap_or(0) as u32,
        is_lifeboat: j["is_lifeboat"].as_bool().unwrap_or(false),
        sphere: j["sphere"]["radius"].as_f64().map_or((centre, radius), |r| (conv_v(&j["sphere"]["center"]), r as f32)),
        // PROVISIONAL hull-end fallback for custom models without authored tow/homing nodes.
        tow_points: [point("TowerPoint").unwrap_or(Vec3::new(0.0, 0.0, bounds[1].z)), point("ToweePoint").unwrap_or(Vec3::new(0.0, 0.0, bounds[0].z))],
        homing_point: point("TorpedoHomingPoint").unwrap_or((hmn + hmx) * 0.5),
    }
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
        let names = ids.iter().filter_map(|n| Some((n.clone(), serde_json::from_str(&std::fs::read_to_string(root.join(format!("ships/{n}/names.json"))).ok()?).ok()?))).collect();
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
    fn lifeboat_class_fields_load_and_default_for_mods_that_omit_them() {
        let lib = Library::load(&crate::assets_dir());
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
