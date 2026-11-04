//! Reading the exported effects: curves, primitives (emitters) and the library of loaded effects.

use bevy::prelude::*;
use serde_json::Value;
use std::collections::HashMap;

use super::sim::TICK;
use crate::assets_dir;

/// The `curve` key list (time, value) of an exported interpolator; missing numbers read as 0.
pub fn curve_keys(v: &Value) -> Vec<(f32, f32)> {
    v["curve"].as_array().map(|a| a.iter().map(|k| (k[0].as_f64().unwrap_or(0.0) as f32, k[1].as_f64().unwrap_or(0.0) as f32)).collect()).unwrap_or_default()
}

#[derive(Clone, Default)]
pub(super) struct Curve(pub(super) Vec<(f32, f32)>, pub(super) Option<(f32, f32, u32)>, pub(super) bool);
impl Curve {
    pub(super) fn from(v: Option<&Value>) -> Curve {
        let keys = v.map(curve_keys);
        let looping = v.and_then(|v| v.get("loop")).and_then(|l| {
            let (start, end, count) = (l["StartLoopingT"].as_f64()? as f32, l["EndLoopingT"].as_f64()? as f32, l["NumberLoops"].as_u64()?);
            (start.is_finite() && end.is_finite() && end > start && count > 0).then_some((start, end, count.min(u32::MAX as u64) as u32))
        });
        Curve(keys.unwrap_or_default(), looping, v.is_some_and(|v| v["keys_only"] == true))
    }
    /// Index of the key interval time `t` falls in (the last key at or before it, after the loop mapping).
    pub(super) fn key_index(&self, t: f32) -> usize {
        let t = self.looped(t);
        self.0.iter().rposition(|k| k.0 <= t).unwrap_or(0)
    }
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub(super) fn last_t(&self) -> f32 {
        self.0.last().map_or(0.0, |k| k.0)
    }
    /// `t` mapped through the authored loop; once the repeats are used up time carries on from the loop's end (decoded).
    pub(super) fn looped(&self, mut t: f32) -> f32 {
        if let Some((start, end, count)) = self.1 {
            let period = end - start;
            let done = ((t - start) / period).floor();
            if done >= count as f32 {
                t = end + (t - start - count as f32 * period);
            } else if done > 0.0 {
                t -= done * period;
            }
        }
        t
    }
    /// Linear between keys, with authored interval repeats; held outside the keys, `def` when empty.
    pub(super) fn at(&self, t: f32, def: f32) -> f32 {
        let t = self.looped(t);
        let k = &self.0;
        match k.len() {
            0 => def,
            1 => k[0].1,
            _ => {
                if t <= k[0].0 {
                    return k[0].1;
                }
                for w in k.windows(2) {
                    if t <= w[1].0 {
                        let f = (t - w[0].0) / (w[1].0 - w[0].0).max(1e-6);
                        return w[0].1 + (w[1].1 - w[0].1) * f;
                    }
                }
                k[k.len() - 1].1
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Blend {
    Add,
    Alpha,
    Multiply,
}

/// Effect meshes drawn without a morph (exported by tools/export_effects.py too).
const STATIC_MESHES: [&str; 1] = ["Periscope_Layer1"];

pub(super) struct MorphMesh {
    pub(super) source: Vec<Vec3>,
    pub(super) destination: Vec<Vec3>,
    pub(super) uv: Vec<[f32; 2]>,
    pub(super) indices: Vec<u32>,
    pub(super) model: String,
    pub(super) weight: Curve,
}

pub(super) struct Prim {
    pub(super) mesh: Option<MorphMesh>,
    pub(super) duration: f32,
    pub(super) delay: f32,
    pub(super) ever: bool,
    pub(super) world: bool,
    pub(super) rate: Curve,
    pub(super) start: u32,
    pub(super) pos_t: [Curve; 3],
    pub(super) pos_r: Curve,
    pub(super) pos_l: Curve,
    pub(super) pos_le: Curve,
    pub(super) vel_t: [Curve; 3],
    pub(super) vel_r: Curve,
    pub(super) vel_l: Curve,
    pub(super) vel_le: Curve,
    pub(super) target_on: bool,
    pub(super) target_pos_velocity: bool,
    pub(super) angle_on: bool,
    pub(super) angle_pos_velocity: bool,
    pub(super) vel_ang_dir: [Curve; 3],
    pub(super) vel_ang_min: Curve,
    pub(super) vel_ang_max: Curve,
    pub(super) vel_ang_len: Curve,
    pub(super) vel_ang_len_err: Curve,
    pub(super) ang_dir: [Curve; 3],
    pub(super) ang_min: Curve,
    pub(super) ang_max: Curve,
    pub(super) ang_len: Curve,
    pub(super) ang_len_err: Curve,
    pub(super) tex: Option<usize>,
    /// Pseudo-billboards carry a second, "front" texture: seen along its axis the streak turns into this round glow.
    pub(super) tex2: Option<usize>,
    pub(super) blend: Blend,
    pub(super) size: Curve,
    pub(super) rgb: [Curve; 3],
    pub(super) alpha: Curve,
    pub(super) rect: Option<[Curve; 4]>,
    pub(super) rect_scale: f32, // PROVISIONAL: 0.1 for a rectangle with no size entry (planks, boarding sprites), else 1; see docs/re/effects.md
    pub(super) flat: bool,
    pub(super) along_velocity: bool,
    pub(super) pseudo_dir: Option<Vec3>, // fixed streak axis in effect space (PseudoBillboardChunk)

    pub(super) life: Option<Curve>,
    pub(super) attract: Option<([Curve; 3], [Curve; 3])>,
    pub(super) fallback_life: f32,
    pub(super) low_rate: f32,       // LowDetailParticleRateMultiply
    pub(super) budget: f32,         // emission scale keeping the steady live count under MAX_LIVE
    pub(super) src: Option<usize>,  // spawn at the positions of this primitive's particles (invisible "source" emitters)
    pub(super) mesh_vertices: bool, // effect variable ASSIGN_RANDOM_POSITIONS_FROM_MESH_VERTEXES: add a random vertex of the owner's mesh to each spawn
}
pub(super) struct Effect {
    pub(super) prims: Vec<Prim>,
    /// Particles that drift farther than this from the effect's origin are removed. PROVISIONAL (owner's choice): only the
    /// Star mortar impact, whose attractor pushes its glow outward without limit (docs/re/effects.md); its authored
    /// `EffectMaxRadius` (200) is the bound. No retail rule that removes them was found.
    pub(super) bound: Option<f32>,
}

#[derive(Resource)]
pub struct FxLib {
    pub(super) effects: HashMap<String, usize>,
    pub(super) list: Vec<Effect>,
    pub(super) tex_names: Vec<String>,
}

impl FxLib {
    /// Every loaded effect: its file name and the index `Fx::effect` reports.
    pub fn names(&self) -> impl Iterator<Item = (&str, usize)> {
        self.effects.iter().map(|(n, &i)| (n.as_str(), i))
    }

    /// A library of empty effects with these names (index = position), for tests that need effect names but no game data.
    #[cfg(test)]
    pub fn with_names(names: &[&str]) -> Self {
        Self {
            effects: names.iter().enumerate().map(|(i, n)| (n.to_string(), i)).collect(),
            list: names.iter().map(|_| Effect { prims: vec![], bound: None }).collect(),
            tex_names: vec![],
        }
    }
}

/// Game frame (x right, y bow, z up) -> Bevy (x, z, -y).
pub(super) fn conv(v: Vec3) -> Vec3 {
    crate::data::conv(v.x, v.y, v.z)
}

fn first(v: Option<&Value>) -> Option<&Value> {
    match v {
        Some(Value::Array(a)) => a.first(),
        other => other,
    }
}
fn get<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cur = v;
    for k in path {
        cur = first(Some(cur.get(*k)?))?;
    }
    Some(cur)
}
fn c3(v: Option<&Value>) -> [Curve; 3] {
    ["Value1Interpolator", "Value2Interpolator", "Value3Interpolator"].map(|k| Curve::from(v.and_then(|v| v.get(k))))
}

pub(super) fn parse_prim(p: &Value, tex_names: &mut Vec<String>) -> Prim {
    let flag = |k: &str| p[k].as_bool().unwrap_or(false);
    let num = |k: &str| p[k].as_f64().unwrap_or(0.0) as f32;
    let pe = p.get("PositionEmitter");
    let ve = p.get("VelocityEmitter");
    let pa = p.get("CHUNKDATA_POSITION_ANGLE_EMITTER");
    let va = p.get("CHUNKDATA_VELOCITY_ANGLE_EMITTER");
    // Per-spawn chunks (colour fader, size, rectangle, rotation) override the common ones; the original creates them per spawned
    // batch, so their curves run over the particle's own age (e.g. the engine's orange fade lives only here).
    let mut merged = p["CommonPropertiesChunk"].clone();
    if let (Some(m), Some(sp)) = (merged.as_object_mut(), p["SpawnedEachTimePropertiesChunk"].as_object()) {
        for (k, v) in sp {
            if k != "RenderNumInfos" && k != "MovementNumInfos" {
                m.insert(k.clone(), v.clone());
            }
        }
    }
    let cp = &merged;
    let mut texture = |t: &str| {
        let n = std::path::Path::new(t).file_stem().unwrap().to_string_lossy().to_lowercase();
        tex_names.iter().position(|x| *x == n).unwrap_or_else(|| {
            tex_names.push(n);
            tex_names.len() - 1
        })
    };
    let tex = get(cp, &["SimpleTextureChunk", "TextureName"]).and_then(|t| t.as_str()).map(&mut texture);
    let tex2 = cp.get("SimpleTextureChunk").and_then(|c| c.as_array()).and_then(|a| a.get(1)).and_then(|c| c["TextureName"].as_str()).map(&mut texture);
    let src = get(cp, &["BlendChunk", "BlendSource"]).and_then(|v| v.as_str()).unwrap_or("SRCALPHA");
    let dst = get(cp, &["BlendChunk", "BlendDestination"]).and_then(|v| v.as_str()).unwrap_or("INVSRCALPHA");
    let blend = if dst == "ONE" {
        Blend::Add
    } else if dst == "SRCCOLOR" || dst == "ZERO" {
        Blend::Multiply
    } else {
        let _ = src;
        Blend::Alpha
    };
    let cc = get(cp, &["ColorFaderChunk", "ColorInterpolator3Chunk"]);
    let rgb = c3(cc);
    let alpha = Curve::from(get(cp, &["ColorFaderChunk", "AlphaComponentChunk"]));
    let size = Curve::from(get(cp, &["SizeChunk", "SizeInterpolatorChunk"]));
    let rect_scale = if get(cp, &["Rectangle2dChunk"]).is_some() && size.is_empty() { 0.1 } else { 1.0 };
    let rect = get(cp, &["Rectangle2dChunk"])
        .map(|r| ["TopLeftXInterpolatorChunk", "TopLeftYInterpolatorChunk", "BottomRightXInterpolatorChunk", "BottomRightYInterpolatorChunk"].map(|k| Curve::from(r.get(k))));
    let life = get(cp, &["ExpirationChunk", "ExpirationInterpolateChunk"]).map(|c| Curve::from(Some(c))).filter(|c| !c.is_empty());
    let attract = get(cp, &["AttractionChunk"]).map(|a| {
        (
            c3(a.get("PositionInterpolator3Chunk")),
            ["InterpolatorComponentAttractionConst", "InterpolatorComponentAttractionWithDist", "InterpolatorComponentAttractionWithDistSquared"].map(|k| Curve::from(a.get(k))),
        )
    });
    let rate = Curve::from(p.get("ParticleRateInterpolator"));
    let fl = [rgb[0].last_t(), rgb[1].last_t(), rgb[2].last_t(), alpha.last_t(), size.last_t()].into_iter().fold(0.0, f32::max);
    // PROVISIONAL: native no-expiration lifetime is unresolved.
    // shortcut: bound continuous, non-fading particles by authored duration; replace after recovering native expiry.
    // A start-only looping glow is a fixed pool, not a new emission every animation cycle.
    let start_only_loop = !rate.0.iter().any(|(_, r)| *r > 0.0) && [&rgb[0], &rgb[1], &rgb[2], &alpha, &size].iter().any(|c| c.1.is_some());
    let fallback_life = if start_only_loop {
        f32::INFINITY
    } else if fl > 0.0 {
        fl.max(0.05)
    } else if rate.0.iter().any(|(_, r)| *r > 0.0) {
        num("Duration").max(0.05)
    } else {
        f32::INFINITY
    };
    let live = rate.0.iter().map(|(_, r)| *r).fold(0.0, f32::max) / TICK * if flag("Everlasting") { fallback_life } else { fallback_life.min(num("Duration")) };
    let budget = if live.is_finite() && live > MAX_LIVE { MAX_LIVE / live } else { 1.0 };
    let src = p
        .get("CHUNKDATA_SOURCE_EMITTER")
        .filter(|s| s["CHUNKDATA_PARTICLE_SYSTEM_SOURCE_EMITTER_ENABLED"].as_bool() == Some(true))
        .and_then(|s| s["CHUNKDATA_PARTICLE_SYSTEM_SOURCE_EMITTER_INDEX"].as_u64())
        .map(|i| i as usize);
    // A mesh that does not morph is drawn as it is (the cloaked ship's periscope); the nebula meshes are drawn by nebula.rs.
    let mesh =
        get(p, &["CommonPropertiesChunk", "MeshChunk"]).filter(|m| m["UseMorph"] == true || m["SourceMeshName"].as_str().is_some_and(|n| STATIC_MESHES.contains(&n))).map(|m| {
            let src = m["SourceMeshName"].as_str().expect("Morph source mesh");
            let dst = if m["UseMorph"] == true { m["DestinationMeshName"].as_str().expect("Morph destination mesh") } else { src };
            let (source, uv, indices) = crate::data::mesh_geometry(assets_dir(), src).unwrap_or_else(|| panic!("Missing/invalid effect mesh {src}; regenerate effects"));
            let (destination, _, other_indices) = crate::data::mesh_geometry(assets_dir(), dst).unwrap_or_else(|| panic!("Missing/invalid effect mesh {dst}; regenerate effects"));
            assert!(source.len() == destination.len() && indices == other_indices, "Morph topology mismatch: {src} / {dst}");
            MorphMesh { source, destination, uv, indices, model: crate::data::model(&format!("{src}.glb")), weight: Curve::from(m.get("MeshWeightInterpolator")) }
        });
    Prim {
        mesh,
        tex2,
        duration: num("Duration"),
        delay: num("StartDelay"),
        ever: flag("Everlasting"),
        world: flag("UseWorldCoordInternaly"),
        rate,
        start: p["NumberParticlesAtStart"].as_u64().unwrap_or(0) as u32,
        pos_t: c3(pe.and_then(|e| e.get("EmitterTargetPositionInterpolator3"))),
        pos_r: Curve::from(pe.and_then(|e| e.get("RadiusInterpolator"))),
        pos_l: Curve::from(pe.and_then(|e| e.get("LengthInterpolator"))),
        pos_le: Curve::from(pe.and_then(|e| e.get("LengthErrorInterpolator"))),
        vel_t: c3(ve.and_then(|e| e.get("EmitterTargetPositionInterpolator3"))),
        vel_r: Curve::from(ve.and_then(|e| e.get("RadiusInterpolator"))),
        vel_l: Curve::from(ve.and_then(|e| e.get("LengthInterpolator"))),
        vel_le: Curve::from(ve.and_then(|e| e.get("LengthErrorInterpolator"))),
        target_on: flag("CHUNKDATA_ENABLE_TARGET_EMITTER"),
        target_pos_velocity: flag("CHUNKDATA_TARGET_EMITTER_USE_POSITION_FOR_VELOCITY"),
        angle_on: flag("CHUNKDATA_ENABLE_ANGLE_EMITTER"),
        angle_pos_velocity: flag("CHUNKDATA_ANGLE_EMITTER_USE_POSITION_FOR_VELOCITY"),
        vel_ang_dir: c3(va.and_then(|e| e.get("CHUNK_ANGLEEMITTER_TARGET_POSITION_INTERPOLATOR3"))),
        vel_ang_min: Curve::from(va.and_then(|e| e.get("CHUNK_ANGLEEMITTER_MIN_ANGLE"))),
        vel_ang_max: Curve::from(va.and_then(|e| e.get("CHUNK_ANGLEEMITTER_MAX_ANGLE"))),
        vel_ang_len: Curve::from(va.and_then(|e| e.get("CHUNK_ANGLEEMITTER_LENGTHINTERPOLATOR"))),
        vel_ang_len_err: Curve::from(va.and_then(|e| e.get("CHUNK_ANGLEEMITTER_LENGTHERRORINTERPOLATOR"))),
        ang_dir: c3(pa.and_then(|e| e.get("CHUNK_ANGLEEMITTER_TARGET_POSITION_INTERPOLATOR3"))),
        ang_min: Curve::from(pa.and_then(|e| e.get("CHUNK_ANGLEEMITTER_MIN_ANGLE"))),
        ang_max: Curve::from(pa.and_then(|e| e.get("CHUNK_ANGLEEMITTER_MAX_ANGLE"))),
        ang_len: Curve::from(pa.and_then(|e| e.get("CHUNK_ANGLEEMITTER_LENGTHINTERPOLATOR"))),
        ang_len_err: Curve::from(pa.and_then(|e| e.get("CHUNK_ANGLEEMITTER_LENGTHERRORINTERPOLATOR"))),
        tex,
        blend,
        size,
        rgb,
        alpha,
        rect_scale,
        rect,
        flat: cp.get("Flat").is_some(),
        along_velocity: get(cp, &["PseudoBillboardChunk", "PseudoDirectionUseVelocity"]).and_then(|v| v.as_bool()).unwrap_or(false),
        pseudo_dir: get(cp, &["PseudoBillboardChunk", "DirectionInterpolator3Chunk"]).map(|d| {
            let c = |k: &str| d.get(k).and_then(|c| c["curve"].get(0)).and_then(|p| p[1].as_f64()).unwrap_or(0.0) as f32;
            conv(Vec3::new(c("Value1Interpolator"), c("Value2Interpolator"), c("Value3Interpolator")))
        }),
        life,
        attract,
        fallback_life,
        low_rate: p["LowDetailParticleRateMultiply"].as_f64().unwrap_or(1.0) as f32,
        budget,
        src,
        mesh_vertices: p["CHUNKDATA_CONST_ACCEPTS_EFFECT_VARIABLES_STRING_VECTOR - Element"].as_str() == Some("ASSIGN_RANDOM_POSITIONS_FROM_MESH_VERTEXES"),
    }
}

/// The imported index, then any other `<name>.json` in the folder (an effect a mod adds is not in the imported index).
fn effect_names(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = crate::data::try_json(&dir.join("index.json")).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let mut extra: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".json").map(String::from))
        .filter(|n| n != "index" && !names.contains(n))
        .collect();
    extra.sort();
    names.extend(extra);
    names
}

impl FromWorld for FxLib {
    fn from_world(_: &mut World) -> Self {
        let dir = assets_dir().join("fx");
        let names = effect_names(&dir);
        let (mut effects, mut list, mut tex_names) = (HashMap::new(), vec![], vec![]);
        for n in names {
            let j = crate::data::read_json(&dir.join(format!("{n}.json")));
            let prims = match &j["Primitive"] {
                Value::Array(a) => a.iter().map(|p| parse_prim(p, &mut tex_names)).collect(),
                _ => vec![],
            };
            let bound = (n == "StarMortar_Heavy_Impact").then(|| j["EffectMaxRadius"].as_f64().unwrap_or(200.0) as f32);
            effects.insert(n, list.len());
            list.push(Effect { prims, bound });
        }
        Self { effects, list, tex_names }
    }
}

/// PROVISIONAL steady live-particle budget per primitive. Every retail effect stays under ~3000 at its per-tick rate
/// except GravCharge_Medium_Impact (180 per tick for 18 s, ~97000 additive quads): it saturates to a white blob and stalls the
/// frame. Over-budget primitives emit proportionally fewer particles, keeping their shape. The original's own pool limit is unknown.
pub(super) const MAX_LIVE: f32 = 4000.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::{ADDITIVE_SORT_BIAS, additive_material};

    #[test]
    #[ignore = "needs imported assets"]
    fn repair_meshes_have_matching_topology_and_deploy_retract_curves() {
        for name in ["Repair_Small", "Repair_Medium", "Repair_Large", "Repair_IC_Medium", "Repair_IC_Large"] {
            let doc: Value = serde_json::from_slice(&std::fs::read(assets_dir().join(format!("fx/{name}.json"))).unwrap()).unwrap();
            let prim = doc["Primitive"].as_array().unwrap().iter().map(|p| parse_prim(p, &mut vec![])).find(|p| p.mesh.is_some()).unwrap();
            let mesh = prim.mesh.unwrap();
            assert_eq!(mesh.source.len(), mesh.destination.len());
            assert!(mesh.source.iter().zip(&mesh.destination).any(|(a, b)| a.distance(*b) > 1.0));
            assert!(mesh.weight.at(prim.duration * 0.4, 0.0) >= 0.5, "{name} must deploy");
            assert!(mesh.weight.at(prim.duration, 0.0) < 0.01, "{name} must retract");
        }
        let material = additive_material(Handle::default());
        assert_eq!(material.base.alpha_mode, AlphaMode::Blend, "shader must leave source RGB unpremultiplied for ONE/ONE");
        assert!(material.base.depth_bias >= ADDITIVE_SORT_BIAS, "additive effects sort after alpha-blended sails (PT-05)");
    }

    #[test]
    fn effects_a_mod_adds_are_loaded_without_being_in_the_index() {
        let dir = std::env::temp_dir().join(format!("procyon-fx-names-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.json"), r#"["A"]"#).unwrap();
        for f in ["A.json", "ModEffect.json", "notes.txt"] {
            std::fs::write(dir.join(f), "{}").unwrap();
        }
        assert_eq!(effect_names(&dir), ["A", "ModEffect"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn malformed_mesh_buffers_fail_without_panicking() {
        let root = std::env::temp_dir().join(format!("procyon-mesh-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("badmesh.glb");
        std::fs::write(&path, []).unwrap();
        assert!(crate::data::mesh_geometry(&root, "badmesh").is_none());
        let json = serde_json::json!({"meshes":[{"primitives":[{"attributes":{"POSITION":0,"TEXCOORD_0":0},"indices":0}]}],"accessors":[{"bufferView":0,"count":1024,"componentType":5126}],"bufferViews":[{"byteOffset":200,"byteLength":12288}]});
        let encoded = serde_json::to_vec(&json).unwrap();
        let mut bytes = vec![0u8; 20];
        bytes[0..4].copy_from_slice(b"glTF");
        bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
        bytes[16..20].copy_from_slice(b"JSON");
        bytes[12..16].copy_from_slice(&(encoded.len() as u32).to_le_bytes());
        bytes.extend(encoded);
        bytes.extend([0u8; 4]);
        bytes.extend(b"BIN\0");
        std::fs::write(&path, bytes).unwrap();
        assert!(crate::data::mesh_geometry(&root, "badmesh").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn lifetime_fallback_preserves_start_only_source_pools_and_authored_expiration() {
        let lib = FxLib::from_world(&mut World::new());
        let marker = &lib.list[lib.effects["Docking_Point"]].prims[8];
        assert_eq!(marker.fallback_life, marker.duration);
        assert!(marker.life.is_none());
        let source = &lib.list[lib.effects["Damaged Section_Heavy"]].prims[0];
        assert!(source.fallback_life.is_infinite(), "start-only source particles remain available");
        let engine = &lib.list[lib.effects["NavyEngine_Light"]].prims[0];
        assert_eq!(engine.life.as_ref().unwrap().at(0.0, 0.0), 1.0, "explicit expiration is unchanged");
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn only_the_gravity_well_is_thinned_to_the_particle_budget() {
        let lib = FxLib::from_world(&mut World::new());
        let well = &lib.list[lib.effects["GravCharge_Medium_Impact"]].prims[0];
        assert!(well.rate.at(0.0, 0.0) * well.budget / TICK * well.fallback_life <= MAX_LIVE + 1.0);
        let thinned: Vec<_> = lib.effects.iter().filter(|&(_, &i)| lib.list[i].prims.iter().any(|p| p.budget < 1.0)).map(|(n, _)| n.as_str()).collect();
        assert_eq!(thinned, ["GravCharge_Medium_Impact"], "other effects keep their authored rates");
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn only_the_star_mortar_glow_is_bounded_to_its_effect_radius() {
        let lib = FxLib::from_world(&mut World::new());
        assert_eq!(lib.list[lib.effects["StarMortar_Heavy_Impact"]].bound, Some(200.0));
        assert!(!lib.effects.iter().any(|(n, &i)| n != "StarMortar_Heavy_Impact" && lib.list[i].bound.is_some()));
    }

    #[test]
    fn curve_repeats_only_the_authored_interval_and_resumes_after_finite_loops() {
        let c = Curve::from(Some(&serde_json::json!({"curve": [[0, 0], [1, 10], [3, 30], [4, 40]],
            "loop": {"StartLoopingT": 1, "EndLoopingT": 3, "NumberLoops": 2}})));
        assert_eq!(c.at(0.5, 0.0), 5.0);
        assert_eq!(c.at(2.0, 0.0), 20.0);
        assert_eq!(c.at(3.0, 0.0), 10.0);
        // Once the repeats are used up (at t = 5) time carries on from the loop's end (decoded), here into the held tail.
        assert_eq!(c.at(5.0, 0.0), 30.0);
        assert_eq!(c.at(5.5, 0.0), 35.0);
        assert_eq!(c.at(7.5, 0.0), 40.0);
        let invalid = Curve::from(Some(&serde_json::json!({"curve": [[0, 1]], "loop": {"StartLoopingT": 1, "EndLoopingT": 1, "NumberLoops": 2}})));
        assert!(invalid.1.is_none());
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn shell_sprites_carry_their_front_texture() {
        let doc: Value = serde_json::from_slice(&std::fs::read(assets_dir().join("fx/CannonBall_Med_Bullet_Particles.json")).unwrap()).unwrap();
        let mut names = vec![];
        let prim = parse_prim(&doc["Primitive"][0], &mut names);
        assert_eq!((names[prim.tex.unwrap()].as_str(), names[prim.tex2.unwrap()].as_str()), ("cannonballpseudo00", "cannonballfrontpseudo00"));
    }
}

#[cfg(test)]
#[cfg(test)]
mod keys_only_tests {
    use super::*;

    #[test]
    fn keys_only_curves_fire_once_per_key_interval_and_loops_continue_from_their_end() {
        let curve = Curve(vec![(0.0, 1.0), (0.5, 2.0), (1.0, 3.0)], Some((0.0, 2.0, 2)), true);
        let idx: Vec<usize> = [0.0, 0.2, 0.5, 0.7, 1.0, 1.9, 2.0, 2.6, 4.1, 5.0].iter().map(|t| curve.key_index(*t)).collect();
        // Second round restarts at key 0; after the two loops time carries on from the end (2.0): no further wraps.
        assert_eq!(idx, [0, 0, 1, 1, 2, 2, 0, 1, 2, 2]);
        assert_eq!(curve.looped(4.5), 2.5);
    }
}
