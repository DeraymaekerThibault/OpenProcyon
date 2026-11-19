//! A mod's glTF models may use retail textures without shipping them: a material named `retail:<texture>` (the retail texture's file
//! name without `.tga`, e.g. `retail:proc_pon_main`) gets that texture from the player's own import when the overlay is built. The
//! rewritten model lives only in the (git-ignored) overlay, so nothing converted from the game is ever committed or redistributed;
//! a texture the import does not have leaves the material untextured, with a warning. The material is renamed to the retail name
//! (`proc_pon_main.tga`), which is what the engine's name rules (sails, render-profile surfaces) match on.
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const PREFIX: &str = "retail:";

/// A retail texture's key: lower case, no `.tga`.
fn key(name: &str) -> String {
    name.trim_end_matches(".tga").to_lowercase()
}

/// The JSON chunk and the offset of the binary chunk's data, read from the start of a GLB without loading the whole file.
fn header(f: &mut std::fs::File) -> Option<(Value, u64)> {
    let mut head = [0u8; 20];
    f.read_exact(&mut head).ok()?;
    if &head[0..4] != b"glTF" || &head[16..20] != b"JSON" {
        return None;
    }
    let size = u32::from_le_bytes(head[12..16].try_into().ok()?) as usize;
    let mut json = vec![0u8; size];
    f.read_exact(&mut json).ok()?;
    Some((serde_json::from_slice(&json).ok()?, 20 + size as u64 + 8))
}

/// Image bytes of the wanted textures (keys), found by material name in the retail ship models under `base/ships`.
fn find(base: &Path, wanted: &[String]) -> HashMap<String, Vec<u8>> {
    let mut found: HashMap<String, Vec<u8>> = HashMap::new();
    let mut dirs: Vec<_> = std::fs::read_dir(base.join("ships")).into_iter().flatten().flatten().map(|e| e.path().join("model.glb")).collect();
    dirs.sort();
    for path in dirs {
        if wanted.iter().all(|w| found.contains_key(w)) {
            break;
        }
        let Ok(mut f) = std::fs::File::open(&path) else { continue };
        let Some((doc, bin)) = header(&mut f) else { continue };
        for m in doc["materials"].as_array().into_iter().flatten() {
            let k = key(m["name"].as_str().unwrap_or_default());
            let Some(t) = m["pbrMetallicRoughness"]["baseColorTexture"]["index"].as_u64() else { continue };
            if !wanted.contains(&k) || found.contains_key(&k) {
                continue;
            }
            let view =
                &doc["bufferViews"][doc["images"][doc["textures"][t as usize]["source"].as_u64().unwrap_or(u64::MAX) as usize]["bufferView"].as_u64().unwrap_or(u64::MAX) as usize];
            let (Some(offset), Some(len)) = (view["byteOffset"].as_u64().or(Some(0)), view["byteLength"].as_u64()) else { continue };
            let mut bytes = vec![0u8; len as usize];
            if f.seek(SeekFrom::Start(bin + offset)).is_ok() && f.read_exact(&mut bytes).is_ok() {
                found.insert(k, bytes);
            }
        }
    }
    found
}

/// The model with its `retail:` materials textured from the retail import under `base`; `None` when it names none.
pub fn bake(bytes: &[u8], base: &Path) -> Option<Vec<u8>> {
    let (mut doc, end) = crate::data::glb_json(bytes)?;
    let wanted: Vec<String> = doc["materials"].as_array()?.iter().filter_map(|m| Some(key(m["name"].as_str()?.strip_prefix(PREFIX)?))).collect();
    if wanted.is_empty() {
        return None;
    }
    let length = u32::from_le_bytes(bytes.get(end..end + 4)?.try_into().ok()?) as usize;
    let mut bin = bytes.get(end + 8..end + 8 + length)?.to_vec();
    let textures = find(base, &wanted);
    let mut slot: HashMap<String, usize> = HashMap::new();
    for (i, m) in doc["materials"].as_array()?.clone().iter().enumerate() {
        let Some(name) = m["name"].as_str().and_then(|n| n.strip_prefix(PREFIX)) else { continue };
        let k = key(name);
        doc["materials"][i]["name"] = json!(format!("{k}.tga"));
        doc["materials"][i]["doubleSided"] = json!(true);
        let Some(image) = textures.get(&k) else {
            eprintln!("mod model: retail texture {name:?} is not in the import; the material stays untextured");
            continue;
        };
        let texture = match slot.get(&k) {
            Some(t) => *t,
            None => {
                while bin.len() % 4 != 0 {
                    bin.push(0);
                }
                let view = doc["bufferViews"].as_array().map_or(0, Vec::len);
                doc["bufferViews"].as_array_mut()?.push(json!({"buffer": 0, "byteOffset": bin.len(), "byteLength": image.len()}));
                bin.extend_from_slice(image);
                let source = doc["images"].as_array().map_or(0, Vec::len);
                if doc["images"].is_null() {
                    doc["images"] = json!([]);
                    doc["textures"] = json!([]);
                }
                doc["images"].as_array_mut()?.push(json!({"bufferView": view, "mimeType": "image/png"}));
                doc["samplers"] = json!([{"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497}]);
                let texture = doc["textures"].as_array().map_or(0, Vec::len);
                doc["textures"].as_array_mut()?.push(json!({"sampler": 0, "source": source}));
                slot.insert(k, texture);
                texture
            }
        };
        doc["materials"][i]["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index": texture});
        doc["materials"][i]["pbrMetallicRoughness"].as_object_mut()?.remove("baseColorFactor");
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    doc["buffers"][0]["byteLength"] = json!(bin.len());
    let mut js = serde_json::to_vec(&doc).ok()?;
    while js.len() % 4 != 0 {
        js.push(b' ');
    }
    let mut out = b"glTF".to_vec();
    out.extend(2u32.to_le_bytes());
    out.extend(((12 + 8 + js.len() + 8 + bin.len()) as u32).to_le_bytes());
    out.extend((js.len() as u32).to_le_bytes());
    out.extend(b"JSON");
    out.extend(&js);
    out.extend((bin.len() as u32).to_le_bytes());
    out.extend(b"BIN\0");
    out.extend(&bin);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::derive::tests::glb;

    /// A retail-style ship folder holding one textured material, and a mod model that asks for it (and for one that is missing).
    #[test]
    fn retail_materials_get_their_texture_from_the_import_and_others_stay_as_they_were() {
        let base = std::env::temp_dir().join(format!("procyon-retex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("ships/Ship_A")).unwrap();
        let image = crate::shipart::png(2, 2, &[7u8; 16]);
        let retail = json!({"asset": {"version": "2.0"}, "buffers": [{"byteLength": image.len()}], "bufferViews": [{"buffer": 0, "byteLength": image.len()}],
            "images": [{"bufferView": 0, "mimeType": "image/png"}], "textures": [{"source": 0}],
            "materials": [{"name": "Hull_Planks.tga", "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}}]});
        std::fs::write(base.join("ships/Ship_A/model.glb"), glb(retail, &image)).unwrap();

        let tri = [0u8; 36];
        let model = json!({"asset": {"version": "2.0"}, "buffers": [{"byteLength": 36}], "bufferViews": [{"buffer": 0, "byteLength": 36}],
            "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "material": 0}]}], "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
            "materials": [{"name": "retail:hull_planks", "pbrMetallicRoughness": {"baseColorFactor": [1, 0, 0, 1]}},
                          {"name": "retail:not_there", "pbrMetallicRoughness": {}}, {"name": "plain", "pbrMetallicRoughness": {}}]});
        let out = bake(&glb(model.clone(), &tri), &base).expect("a model with retail materials is rewritten");
        let (doc, end) = crate::data::glb_json(&out).unwrap();
        assert_eq!(doc["materials"][0]["name"], "hull_planks.tga");
        assert_eq!(doc["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["index"], 0);
        assert!(doc["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"].is_null(), "the tint would darken the texture");
        assert_eq!(doc["materials"][1]["name"], "not_there.tga");
        assert!(doc["materials"][1]["pbrMetallicRoughness"]["baseColorTexture"].is_null(), "a missing texture leaves the material untextured");
        assert_eq!(doc["materials"][2]["name"], "plain");
        let view = &doc["bufferViews"][1];
        let bin = &out[end + 8..];
        let at = view["byteOffset"].as_u64().unwrap() as usize;
        assert_eq!(&bin[at..at + image.len()], &image[..], "the texture's bytes are embedded");
        assert_eq!(at % 4, 0);
        assert!(bake(&glb(json!({"asset": {"version": "2.0"}, "materials": [{"name": "plain"}]}), &[]), &base).is_none());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
