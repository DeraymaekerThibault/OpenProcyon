//! Ships that stand on their model alone: what `ship.json` leaves out is derived from the GLB (docs/modding.md "A ship from a model").
//!
//! The model supplies the hit meshes (every mesh node is a layer) and its hardpoints: empty nodes whose glTF extra `kind` (or, failing
//! that, whose name) says what they are, the way the retail hulls carry them (`GunPlacement`, `EnginePortPlacement`, `WakePlacement`,
//! `FlagAttachmentPoint`, `BoardingEffectPoint`, `ToweePoint`, `TowerPoint`, `TorpedoHomingPoint`). Anything `ship.json` states wins.
//! Every default here is PROVISIONAL: the retail data has a value for each, the engine only needs a sensible one.
use super::*;
use bevy::math::{EulerRot, Mat4, Vec3};
use std::path::Path;

pub struct Layer {
    pub name: String,
    /// Corners of the layer's box and its triangles (nine numbers each), game axes.
    pub aabb: [[f64; 3]; 2],
    pub tris: Vec<f64>,
}
/// A hardpoint in game axes; `yaw` is the heading about the up axis, 0 forward and positive to port (the retail `rot_raw[0]`).
pub struct Point {
    pub kind: String,
    pub name: String,
    pub pos: [f64; 3],
    pub yaw: f64,
}
#[derive(Default)]
pub struct Facts {
    pub layers: Vec<Layer>,
    pub points: Vec<Point>,
}

/// glTF axes (x, y up, z back) to game axes (x, y forward, z up).
fn to_game(v: Vec3) -> [f64; 3] {
    [v.x as f64, -v.z as f64, v.y as f64]
}

pub fn read(bytes: &[u8]) -> Option<Facts> {
    let (doc, end) = glb_json(bytes)?;
    let start = end.checked_add(8)?;
    let length = u32::from_le_bytes(bytes.get(end..end.checked_add(4)?)?.try_into().ok()?) as usize;
    let bin = bytes.get(start..start.checked_add(length)?).unwrap_or(&[]);
    let mut facts = Facts::default();
    let roots = doc["scenes"][doc["scene"].as_u64().unwrap_or(0) as usize]["nodes"].as_array()?;
    for r in roots.iter().filter_map(Value::as_u64) {
        walk(&doc, bin, r as usize, Mat4::IDENTITY, &mut facts, 0);
    }
    Some(facts)
}

fn walk(doc: &Value, bin: &[u8], i: usize, parent: Mat4, out: &mut Facts, depth: usize) {
    let Some(n) = doc["nodes"].get(i).filter(|_| depth < 32) else { return };
    let f = |v: &Value, k: usize, d: f32| v.get(k).and_then(Value::as_f64).map_or(d, |x| x as f32);
    let local = match n["matrix"].as_array() {
        Some(m) if m.len() == 16 => Mat4::from_cols_array(&std::array::from_fn(|k| f(&n["matrix"], k, 0.0))),
        _ => Mat4::from_scale_rotation_translation(
            Vec3::new(f(&n["scale"], 0, 1.0), f(&n["scale"], 1, 1.0), f(&n["scale"], 2, 1.0)),
            bevy::math::Quat::from_xyzw(f(&n["rotation"], 0, 0.0), f(&n["rotation"], 1, 0.0), f(&n["rotation"], 2, 0.0), f(&n["rotation"], 3, 1.0)).normalize(),
            Vec3::new(f(&n["translation"], 0, 0.0), f(&n["translation"], 1, 0.0), f(&n["translation"], 2, 0.0)),
        ),
    };
    let world = parent * local;
    let name = n["name"].as_str().unwrap_or_default().to_string();
    if let Some(mesh) = n["mesh"].as_u64() {
        if let Some(g) = mesh_triangles(doc, bin, mesh as usize, world) {
            match out.layers.iter_mut().find(|l| l.name == name) {
                Some(l) => {
                    l.tris.extend(g.tris);
                    for k in 0..3 {
                        l.aabb[0][k] = l.aabb[0][k].min(g.aabb[0][k]);
                        l.aabb[1][k] = l.aabb[1][k].max(g.aabb[1][k]);
                    }
                }
                None => out.layers.push(Layer { name: name.clone(), aabb: g.aabb, tris: g.tris }),
            }
        }
    } else if let Some(kind) = point_kind(n, &name) {
        let (_, rotation, translation) = world.to_scale_rotation_translation();
        let yaw = n["extras"]["rot_raw"][0].as_f64().unwrap_or_else(|| rotation.to_euler(EulerRot::YXZ).0 as f64);
        out.points.push(Point { kind, name, pos: to_game(translation), yaw });
    }
    for c in n["children"].as_array().into_iter().flatten().filter_map(Value::as_u64) {
        walk(doc, bin, c as usize, world, out, depth + 1);
    }
}

/// What a non-mesh node is: its `extras.kind`, else a guess from its name. Nodes that are neither stay unused.
fn point_kind(n: &Value, name: &str) -> Option<String> {
    if let Some(k) = n["extras"]["kind"].as_str() {
        return Some(k.to_string());
    }
    let l = name.to_lowercase();
    let kind = if l.contains("mount") || l.contains("lancer") || l.contains("turret") {
        "GunPlacement"
    } else if l.contains("engine") {
        "EnginePortPlacement"
    } else if l.contains("wake") {
        "WakePlacement"
    } else if l.contains("flag") {
        "FlagAttachmentPoint"
    } else if l.contains("boarding") {
        "BoardingEffectPoint"
    } else if l.contains("homing") {
        "TorpedoHomingPoint"
    } else if l.contains("tow") && (l.contains("front") || l.contains("bow")) {
        "ToweePoint"
    } else if l.contains("tow") {
        "TowerPoint"
    } else {
        return None;
    };
    Some(kind.into())
}

fn view<'a>(doc: &Value, bin: &'a [u8], accessor: u64, component: u64, kind: &str, width: usize) -> Option<(&'a [u8], usize, usize)> {
    let a = &doc["accessors"][accessor as usize];
    if a["componentType"].as_u64()? != component || a["type"].as_str()? != kind || a["sparse"].is_object() {
        return None;
    }
    let v = &doc["bufferViews"][a["bufferView"].as_u64()? as usize];
    let count = a["count"].as_u64()? as usize;
    let stride = v["byteStride"].as_u64().map_or(width, |s| s as usize).max(width);
    let offset = (v["byteOffset"].as_u64().unwrap_or(0) as usize).checked_add(a["byteOffset"].as_u64().unwrap_or(0) as usize)?;
    let end = offset.checked_add(count.checked_sub(1)?.checked_mul(stride)?)?.checked_add(width)?;
    Some((bin.get(offset..end)?, stride, count))
}

fn mesh_triangles(doc: &Value, bin: &[u8], mesh: usize, world: Mat4) -> Option<Layer> {
    let mut tris = vec![];
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in doc["meshes"].get(mesh)?["primitives"].as_array()? {
        if p["mode"].as_u64().unwrap_or(4) != 4 {
            continue;
        }
        let Some((data, stride, count)) = view(doc, bin, p["attributes"]["POSITION"].as_u64()?, 5126, "VEC3", 12) else { continue };
        let f = |b: &[u8]| f32::from_le_bytes(b.try_into().unwrap());
        let verts: Vec<[f64; 3]> = (0..count)
            .map(|i| {
                let b = &data[i * stride..];
                to_game(world.transform_point3(Vec3::new(f(&b[0..4]), f(&b[4..8]), f(&b[8..12]))))
            })
            .collect();
        let indices: Vec<usize> = match p["indices"].as_u64() {
            None => (0..count).collect(),
            Some(ix) => {
                let c = doc["accessors"][ix as usize]["componentType"].as_u64()?;
                let w = match c {
                    5121 => 1,
                    5123 => 2,
                    5125 => 4,
                    _ => continue,
                };
                let Some((d, s, n)) = view(doc, bin, ix, c, "SCALAR", w) else { continue };
                (0..n).map(|i| d[i * s..i * s + w].iter().rev().fold(0usize, |a, b| a << 8 | *b as usize)).collect()
            }
        };
        for t in indices.as_chunks::<3>().0 {
            let Some(vs) = t.iter().map(|&i| verts.get(i)).collect::<Option<Vec<_>>>() else { continue };
            for v in vs {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
                tris.extend_from_slice(v);
            }
        }
    }
    (!tris.is_empty() && lo.iter().chain(&hi).all(|v| v.is_finite())).then_some(Layer { name: String::new(), aabb: [lo, hi], tris })
}

fn is_sail(layer: &str) -> bool {
    let l = layer.to_lowercase();
    ["sail", "mast", "cloth", "canvas"].iter().any(|w| l.contains(w))
}

/// Distance from a point to a box.
fn gap(g: &Layer, p: [f64; 3]) -> f64 {
    (0..3).map(|k| (g.aabb[0][k] - p[k]).max(p[k] - g.aabb[1][k]).max(0.0).powi(2)).sum::<f64>().sqrt()
}

/// The hull section a hardpoint sits on: the nearest hull (non-sail) layer among `layers`.
fn carrier(facts: &Facts, layers: &[&str], p: [f64; 3]) -> Option<String> {
    let hull = |l: &&Layer| layers.contains(&l.name.as_str()) && !is_sail(&l.name);
    facts.layers.iter().filter(hull).min_by(|a, b| gap(a, p).total_cmp(&gap(b, p))).map(|l| l.name.clone())
}

/// Hull length along y of the intact layers.
fn length(facts: &Facts) -> f64 {
    let (lo, hi) = facts.layers.iter().filter(|l| !l.name.contains("Damage_")).fold((f64::MAX, f64::MIN), |(a, b), l| (a.min(l.aabb[0][1]), b.max(l.aabb[1][1])));
    (hi - lo).max(0.0)
}

fn near(a: &Layer, b: &Layer) -> bool {
    (0..3).all(|k| a.aabb[0][k] - 0.3 <= b.aabb[1][k] && b.aabb[0][k] - 0.3 <= a.aabb[1][k])
}

fn points<'a>(facts: &'a Facts, kind: &'a str) -> impl Iterator<Item = &'a Point> {
    facts.points.iter().filter(move |p| p.kind == kind)
}

/// Fill in every part of `j` the document did not state, from the model's `facts`.
pub(super) fn complete(j: &mut ShipDoc, facts: &Facts, root: &Path) {
    let intact: Vec<&Layer> = facts.layers.iter().filter(|l| !l.name.contains("Damage_")).collect();
    if j.sections.is_empty() {
        let hulls = intact.iter().filter(|l| !is_sail(&l.name)).count().max(1) as f64;
        j.sections = intact
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let sail = is_sail(&l.name);
                let centre = std::array::from_fn(|k| (l.aabb[0][k] + l.aabb[1][k]) / 2.0);
                let adjacent = intact.iter().filter(|o| o.name != l.name && is_sail(&o.name) == sail && near(l, o)).map(|o| o.name.clone()).collect();
                let section = SectionDoc {
                    layer: l.name.clone(),
                    material: if sail { "Sail Cloth" } else { "Hull Wood" }.into(),
                    hp: if sail { 70.0 } else { 150.0 },
                    core_pct: if sail { 0.0 } else { 100.0 / hulls },
                    vital_ship: !sail,
                    vital_speed: sail,
                    vital_turn: false,
                    vital_mission: false,
                    destruct: Some(DestructDoc { fx: Some("Final_Ship_Chunk".into()), start: 1.2 + 0.2 * i as f64, end: 1.4 + 0.2 * i as f64, point: centre }),
                    debris: None,
                    debris_fx: None,
                    adjacent,
                    criticals: vec![],
                };
                (l.name.clone(), section)
            })
            .collect();
    }
    let layers: Vec<&str> = j.sections.iter().map(|(_, s)| s.layer.as_str()).collect();
    let layers: Vec<String> = layers.into_iter().map(String::from).collect();
    let layer_refs: Vec<&str> = layers.iter().map(String::as_str).collect();
    let size = length(facts);
    let hull_hp: f64 = j.sections.iter().filter(|(_, s)| !s.material.contains("Cloth")).map(|(_, s)| s.hp).sum();
    j.core_hp.get_or_insert((0.25 * hull_hp).max(50.0));

    for (label, m) in &mut j.guns {
        let node = points(facts, "GunPlacement").find(|p| p.name.eq_ignore_ascii_case(label));
        if m.pos.is_none() {
            m.pos = node.map(|p| p.pos);
        }
        if m.rot.is_none() {
            m.rot = node.map(|p| [p.yaw, 0.0, 0.0]);
        }
        if m.parent_layer.is_none() {
            m.parent_layer = m.pos.and_then(|p| carrier(facts, &layer_refs, p));
        }
    }
    // A gun that names no bank takes the weapon-panel slot nearest its place on the generated outline (shipart.rs), so its icon lands
    // over it; guns that do name one keep it. One bank per gun: a mod that wants guns to fire together names the same bank on each.
    if j.guns.iter().any(|(_, m)| m.bank.is_none()) {
        let (slots, outline) = (crate::shipart::slot_centres(root), crate::shipart::TopDown::of(&facts.layers));
        let mut taken: Vec<usize> = j.guns.iter().filter_map(|(_, m)| m.bank.as_deref()?.trim_start_matches("Bank").parse().ok()).collect();
        for (_, m) in j.guns.iter_mut().filter(|(_, m)| m.bank.is_none()) {
            let want = m.pos.zip(outline.as_ref()).map(|(p, o)| o.panel(p[0], p[1]));
            let d2 = |i: usize| want.map_or(0.0, |w| (slots[i][0] - w[0]).powi(2) + (slots[i][1] - w[1]).powi(2));
            let slot = (0..if slots.is_empty() { 80 } else { slots.len() })
                .filter(|i| !taken.contains(i))
                .min_by(|&a, &b| if slots.is_empty() { a.cmp(&b) } else { d2(a).total_cmp(&d2(b)) });
            if let Some(slot) = slot {
                taken.push(slot);
                m.bank = Some(format!("Bank{slot:02}"));
            }
        }
    }
    let race = if j.race.is_empty() { "Navy" } else { j.race.as_str() };
    let (engine_prefix, sound) = match race {
        "Pirate" => ("PirateEngine", "Pirate"),
        "Procyon" => ("ProcEngine", "Proc"),
        "Civilian" => ("CivillianEngine", "Navy"),
        _ => ("NavyEngine", "Navy"),
    };
    let weight = [(14.0, "Light"), (22.0, "Medium"), (32.0, "Heavy")].iter().find(|(l, _)| size < *l).map_or("ExtraHeavy", |(_, w)| w);
    if j.engines.is_none() {
        j.engines = Some(
            points(facts, "EnginePortPlacement")
                .map(|p| EngineDoc {
                    label: Some(p.name.clone()),
                    parent_layer: carrier(facts, &layer_refs, p.pos),
                    pos: Some(p.pos),
                    rot: Some([p.yaw, 0.0, 0.0]),
                    fx: Some(format!("{engine_prefix}_{weight}")),
                })
                .collect(),
        );
    }
    for e in j.engines.iter_mut().flatten() {
        let node = e.label.as_deref().and_then(|l| points(facts, "EnginePortPlacement").find(|p| p.name.eq_ignore_ascii_case(l)));
        e.pos = e.pos.or(node.map(|p| p.pos));
        e.rot = e.rot.or(node.map(|p| [p.yaw, 0.0, 0.0]));
        e.fx.get_or_insert_with(|| format!("{engine_prefix}_{weight}"));
        if e.parent_layer.is_none() {
            e.parent_layer = e.pos.and_then(|p| carrier(facts, &layer_refs, p));
        }
    }
    if j.wakes.is_none() {
        let fx = if size < 14.0 {
            "Wake_Light"
        } else if size < 28.0 {
            "Wake_Medium"
        } else {
            "Wake_Heavy"
        };
        let mut wakes: Vec<WakeDoc> = points(facts, "WakePlacement").map(|p| WakeDoc { pos: p.pos, fx: fx.into() }).collect();
        if wakes.is_empty() && !intact.is_empty() {
            // PROVISIONAL: a wake at the stern's waterline when the model has no wake node.
            let low = intact.iter().map(|l| l.aabb[0][2]).fold(f64::MAX, f64::min);
            let back = intact.iter().map(|l| l.aabb[0][1]).fold(f64::MAX, f64::min);
            wakes.push(WakeDoc { pos: [0.0, back, low + 0.5], fx: fx.into() });
        }
        j.wakes = Some(wakes);
    }
    if j.flags.is_none() {
        j.flags = Some(points(facts, "FlagAttachmentPoint").map(|p| FlagDoc { label: p.name.clone(), pos: p.pos }).collect());
    }
    j.explosion_fx.get_or_insert_with(|| "ShipDebris_Explosion_Medium".into());
    j.engine_snd.get_or_insert_with(|| ["Med", "Full", "EmergFull"].iter().map(|s| Some(format!("Engine_{sound}_{s}"))).collect());
    j.repair_fx.get_or_insert_with(|| {
        if size < 22.0 {
            "Repair_Small"
        } else if size < 32.0 {
            "Repair_Medium"
        } else {
            "Repair_Large"
        }
        .into()
    });
    if j.crew_posts.is_none() {
        let sails = j.sections.iter().filter(|(_, s)| s.material.contains("Cloth")).count() as u64;
        let gunners = (j.guns.len() as u64).div_ceil(2).max(1);
        let posts = [
            ("Gunners", gunners),
            ("Captains", 1),
            ("First Mates", 1),
            ("Navigators", 1),
            ("Engineers", 1 + (size >= 30.0) as u64),
            ("Riggers", sails.max(1)),
            ("Fighters", 1 + (size / 20.0) as u64),
            ("Lookouts", 0),
        ];
        j.crew_posts = Some(posts.iter().map(|(k, v)| (k.to_string(), *v)).collect());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A binary glTF of the given JSON and binary payload.
    pub(crate) fn glb(json: Value, bin: &[u8]) -> Vec<u8> {
        let mut j = json.to_string().into_bytes();
        while !j.len().is_multiple_of(4) {
            j.push(b' ');
        }
        let mut b = bin.to_vec();
        while !b.len().is_multiple_of(4) {
            b.push(0);
        }
        let total = 12 + 8 + j.len() + 8 + b.len();
        let mut out = b"glTF".to_vec();
        out.extend(2u32.to_le_bytes());
        out.extend((total as u32).to_le_bytes());
        out.extend((j.len() as u32).to_le_bytes());
        out.extend(b"JSON");
        out.extend(&j);
        out.extend((b.len() as u32).to_le_bytes());
        out.extend(b"BIN\0");
        out.extend(&b);
        out
    }

    /// Two hull boxes' worth of triangles (one triangle each is enough) plus a sail, and hardpoints in glTF axes.
    pub(crate) fn model() -> Vec<u8> {
        let tri = |dy: f32, dx: f32| [[dx, 0.0, dy], [dx + 1.0, 0.0, dy], [dx, 1.0, dy + 1.0]]; // lies in the deck plane's slant: it has area from above too
        let mut bin = vec![];
        for t in [tri(-6.0, 0.0), tri(6.0, 0.0), tri(0.0, 3.0)] {
            for v in t {
                for c in v {
                    bin.extend(c.to_le_bytes());
                }
            }
        }
        let acc = |first: usize| serde_json::json!({"bufferView": 0, "byteOffset": first * 36, "componentType": 5126, "count": 3, "type": "VEC3"});
        let json = serde_json::json!({
            "asset": {"version": "2.0"}, "scene": 0, "scenes": [{"nodes": [0, 1, 2, 3, 4, 5, 6]}],
            "buffers": [{"byteLength": bin.len()}], "bufferViews": [{"buffer": 0, "byteLength": bin.len()}],
            "accessors": [acc(0), acc(1), acc(2)],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}, {"primitives": [{"attributes": {"POSITION": 1}}]}, {"primitives": [{"attributes": {"POSITION": 2}}]}],
            "nodes": [
                {"name": "Bow", "mesh": 0}, {"name": "Stern", "mesh": 1}, {"name": "Main sail", "mesh": 2},
                {"name": "Gun_A", "translation": [0.0, 1.0, -5.0], "rotation": [0.0, std::f32::consts::FRAC_1_SQRT_2, 0.0, std::f32::consts::FRAC_1_SQRT_2], "extras": {"kind": "GunPlacement"}},
                {"name": "Port engine", "translation": [0.0, 1.0, 6.0]},
                {"name": "Stern wake", "translation": [0.0, 0.0, 6.0], "extras": {"kind": "WakePlacement"}},
                {"name": "Crew entry", "translation": [1.0, 2.0, 3.0], "extras": {"kind": "BoardingEffectPoint"}},
            ]
        });
        glb(json, &bin)
    }

    #[test]
    fn reads_layers_and_hardpoints_in_game_axes() {
        let f = read(&model()).unwrap();
        assert_eq!(f.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Bow", "Stern", "Main sail"]);
        // glTF z is game -y, glTF y is game z.
        assert_eq!(f.layers[0].aabb, [[0.0, 5.0, 0.0], [1.0, 6.0, 1.0]]);
        let kinds: Vec<_> = f.points.iter().map(|p| (p.kind.as_str(), p.name.as_str())).collect();
        assert_eq!(kinds, [("GunPlacement", "Gun_A"), ("EnginePortPlacement", "Port engine"), ("WakePlacement", "Stern wake"), ("BoardingEffectPoint", "Crew entry")]);
        assert_eq!(f.points[0].pos, [0.0, 5.0, 1.0]);
        assert!((f.points[0].yaw - std::f64::consts::FRAC_PI_2).abs() < 1e-5, "a quarter turn about up faces port: {}", f.points[0].yaw);
    }

    #[test]
    fn a_ship_with_only_stats_gets_sections_mounts_engines_and_crew_from_its_model() {
        let f = read(&model()).unwrap();
        let mut j: ShipDoc = serde_json::from_value(serde_json::json!({"race": "Pirate", "guns": {"Gun_A": {"gun": "Gun", "arc_deg": 60}}})).unwrap();
        complete(&mut j, &f, Path::new(""));
        let s: Vec<_> = j.sections.iter().map(|(_, s)| (s.layer.as_str(), s.material.as_str(), s.vital_ship, s.vital_speed)).collect();
        assert_eq!(s, [("Bow", "Hull Wood", true, false), ("Stern", "Hull Wood", true, false), ("Main sail", "Sail Cloth", false, true)]);
        assert_eq!(j.sections[0].1.core_pct + j.sections[1].1.core_pct, 100.0, "the hull sections share the core");
        let m = &j.guns[0].1;
        assert_eq!((m.pos, m.parent_layer.as_deref()), (Some([0.0, 5.0, 1.0]), Some("Bow")), "the mount sits on the hull section nearest it");
        let e = &j.engines.as_ref().unwrap()[0];
        assert_eq!((e.pos, e.fx.as_deref(), e.parent_layer.as_deref()), (Some([0.0, -6.0, 1.0]), Some("PirateEngine_Light"), Some("Stern")));
        assert_eq!(j.wakes.as_ref().unwrap()[0].pos, [0.0, -6.0, 0.0]);
        assert_eq!(j.crew_posts.as_ref().unwrap()["Gunners"], 1);
        assert_eq!(j.crew_posts.as_ref().unwrap()["Riggers"], 1);
        assert!(j.core_hp.unwrap() >= 50.0);
    }

    #[test]
    fn what_ship_json_states_is_kept() {
        let f = read(&model()).unwrap();
        let mut j: ShipDoc = serde_json::from_value(serde_json::json!({"core_hp": 7, "engines": [], "crew_posts": {"Gunners": 9},
            "guns": {"Gun_A": {"gun": "Gun", "arc_deg": 60, "pos": [1, 2, 3], "rot": [0, 0, 0], "parent_layer": "Stern"}}}))
        .unwrap();
        complete(&mut j, &f, Path::new(""));
        assert_eq!(j.core_hp, Some(7.0));
        assert!(j.engines.unwrap().is_empty());
        assert_eq!(j.crew_posts.unwrap()["Gunners"], 9);
        assert_eq!((j.guns[0].1.pos, j.guns[0].1.parent_layer.as_deref()), (Some([1.0, 2.0, 3.0]), Some("Stern")));
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("procyon-derive-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("ships/Model_Only")).unwrap();
        root
    }

    #[test]
    fn a_ship_folder_with_only_a_model_and_a_few_stats_loads_like_any_other() {
        let root = scratch("load");
        std::fs::write(root.join("ships/Model_Only/model.glb"), model()).unwrap();
        std::fs::write(root.join("ships/Model_Only/ship.json"), r#"{"race": "Pirate", "physics": {"MaxSpeed": 12}}"#).unwrap();
        let c = try_load_class(&root, "Model_Only", &HashMap::new()).unwrap();
        assert_eq!(c.sections.len(), 3);
        assert!(c.sections.iter().all(|s| !s.tris.is_empty()), "hit meshes come from the model");
        assert_eq!((c.engines.len(), c.wake.len(), c.boarding_points.len()), (1, 1, 1));
        assert_eq!(c.phys.max_speed, 12.0);
        assert_eq!(c.phys.mass, 4000.0, "unstated physics take the provisional defaults");
        assert!(c.crew.iter().sum::<u32>() >= 5 && c.crew_posts == c.crew.iter().sum::<u32>());
        assert!(c.engines[0].3.is_some(), "the engine hangs on a section");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn guns_without_a_bank_take_distinct_panel_slots_nearest_their_place() {
        let root = scratch("banks");
        std::fs::create_dir_all(root.join("hud")).unwrap();
        // Three slots down the panel's centre line: top, middle, bottom of the outline's box.
        let hud: Value = serde_json::json!({"Offset_WeaponBar_GunIcon": {"Navy": {"TopLeft X": 0, "TopLeft Y": 0}},
            "WeaponBar_SlotOffset_From_TopLeft_00": {"Navy": {"TopLeft X": 60, "TopLeft Y": 20}},
            "WeaponBar_SlotOffset_From_TopLeft_01": {"Navy": {"TopLeft X": 60, "TopLeft Y": 120}},
            "WeaponBar_SlotOffset_From_TopLeft_02": {"Navy": {"TopLeft X": 60, "TopLeft Y": 220}}});
        std::fs::write(root.join("hud/hud.json"), hud.to_string()).unwrap();
        let f = read(&model()).unwrap();
        let mut j: ShipDoc = serde_json::from_value(serde_json::json!({"guns": {
            "Bow": {"gun": "G", "arc_deg": 60, "pos": [0, 5, 1], "rot": [0, 0, 0]},
            "Stern": {"gun": "G", "arc_deg": 60, "pos": [0, -5, 1], "rot": [0, 0, 0]},
            "Named": {"gun": "G", "arc_deg": 60, "pos": [0, 0, 1], "rot": [0, 0, 0], "bank": "Bank01"}}}))
        .unwrap();
        complete(&mut j, &f, &root);
        let banks: Vec<_> = j.guns.iter().map(|(_, m)| m.bank.clone().unwrap()).collect();
        assert_eq!(banks, ["Bank00", "Bank02", "Bank01"], "bow up, stern down, the named bank untouched");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn the_retail_cutter_model_derives_what_its_ship_json_states() {
        let root = crate::assets_dir();
        let f = read(&std::fs::read(root.join("ships/Ship_Navy_Cutter/model.glb")).unwrap()).unwrap();
        let retail: ShipDoc = read_typed(&root.join("ships/Ship_Navy_Cutter/ship.json")).unwrap();
        let geometry: HashMap<String, GeometryDoc> = read_typed(&root.join("ships/Ship_Navy_Cutter/geometry.json")).unwrap();
        for (label, m) in &retail.guns {
            let node = f.points.iter().find(|p| p.kind == "GunPlacement" && p.name == *label).unwrap_or_else(|| panic!("no node for {label}"));
            let (want, got) = (m.pos.unwrap(), node.pos);
            assert!((0..3).all(|k| (want[k] - got[k]).abs() < 1e-3), "{label}: {want:?} vs {got:?}");
            assert!((m.rot.unwrap()[0] - node.yaw).abs() < 1e-3, "{label} yaw");
        }
        for e in retail.engines.iter().flatten() {
            let want = e.pos.unwrap();
            assert!(f.points.iter().any(|p| p.kind == "EnginePortPlacement" && (0..3).all(|k| (want[k] - p.pos[k]).abs() < 1e-3)));
        }
        for (name, g) in &geometry {
            let l = f.layers.iter().find(|l| l.name == *name).unwrap_or_else(|| panic!("no layer {name}"));
            assert!((0..3).all(|k| (g.aabb[0][k] - l.aabb[0][k]).abs() < 0.05 && (g.aabb[1][k] - l.aabb[1][k]).abs() < 0.05), "{name}: {:?} vs {:?}", g.aabb, l.aabb);
        }
    }
}
