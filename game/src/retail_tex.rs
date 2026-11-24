//! A mod's glTF models may use retail textures without shipping them: a material named `retail:<texture>` (the retail texture's file
//! name without `.tga`, e.g. `retail:proc_pon_main`) gets that texture from the player's own import when the overlay is built. The
//! rewritten model lives only in the (git-ignored) overlay, so nothing converted from the game is ever committed or redistributed;
//! a texture the import does not have leaves the material untextured, with a warning. The material is renamed to the retail name
//! (`proc_pon_main.tga`), which is what the engine's name rules (sails, render-profile surfaces) match on.
//!
//! Not retail: a mod's `textures/<retail texture name>.png` replaces that texture in every model of the overlay that uses it
//! (`retexture`), retail ships and scenery included, so a texture pack ships only its own pictures and no model. The PNG's bytes
//! go into the GLB as they are (no decoding, any size); the rewritten copy lives only in the overlay, like the `retail:` ones.
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const PREFIX: &str = "retail:";

/// A retail texture's key: lower case, no `.tga`.
pub(crate) fn key(name: &str) -> String {
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

/// A GLB taken apart: the JSON document and the binary chunk, to add images to and write back.
struct Glb {
    doc: Value,
    bin: Vec<u8>,
}

impl Glb {
    fn parse(bytes: &[u8]) -> Option<Self> {
        let (doc, end) = crate::data::glb_json(bytes)?;
        let length = u32::from_le_bytes(bytes.get(end..end + 4)?.try_into().ok()?) as usize;
        Some(Glb { doc, bin: bytes.get(end + 8..end + 8 + length)?.to_vec() })
    }

    /// Appends an image's bytes as a new buffer view and image; returns the image's index.
    fn add_image(&mut self, image: &[u8]) -> Option<usize> {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        if self.doc["bufferViews"].is_null() {
            self.doc["bufferViews"] = json!([]);
        }
        let view = self.doc["bufferViews"].as_array().map_or(0, Vec::len);
        self.doc["bufferViews"].as_array_mut()?.push(json!({"buffer": 0, "byteOffset": self.bin.len(), "byteLength": image.len()}));
        self.bin.extend_from_slice(image);
        if self.doc["images"].is_null() {
            self.doc["images"] = json!([]);
        }
        if self.doc["textures"].is_null() {
            self.doc["textures"] = json!([]);
        }
        let source = self.doc["images"].as_array().map_or(0, Vec::len);
        self.doc["images"].as_array_mut()?.push(json!({"bufferView": view, "mimeType": "image/png"}));
        Some(source)
    }

    /// Appends a texture of `source` (an image index) with the shared repeating sampler; returns the texture's index.
    fn add_texture(&mut self, source: usize) -> Option<usize> {
        self.doc["samplers"] = json!([{"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497}]);
        let texture = self.doc["textures"].as_array().map_or(0, Vec::len);
        self.doc["textures"].as_array_mut()?.push(json!({"sampler": 0, "source": source}));
        Some(texture)
    }

    fn finish(mut self) -> Option<Vec<u8>> {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        self.doc["buffers"][0]["byteLength"] = json!(self.bin.len());
        let mut js = serde_json::to_vec(&self.doc).ok()?;
        while !js.len().is_multiple_of(4) {
            js.push(b' ');
        }
        let mut out = b"glTF".to_vec();
        out.extend(2u32.to_le_bytes());
        out.extend(((12 + 8 + js.len() + 8 + self.bin.len()) as u32).to_le_bytes());
        out.extend((js.len() as u32).to_le_bytes());
        out.extend(b"JSON");
        out.extend(&js);
        out.extend((self.bin.len() as u32).to_le_bytes());
        out.extend(b"BIN\0");
        out.extend(&self.bin);
        Some(out)
    }
}

/// The model with its `retail:` materials textured from the retail import under `base`; `None` when it names none.
pub fn bake(bytes: &[u8], base: &Path) -> Option<Vec<u8>> {
    let mut glb = Glb::parse(bytes)?;
    let wanted: Vec<String> = glb.doc["materials"].as_array()?.iter().filter_map(|m| Some(key(m["name"].as_str()?.strip_prefix(PREFIX)?))).collect();
    if wanted.is_empty() {
        return None;
    }
    let textures = find(base, &wanted);
    let mut slot: HashMap<String, usize> = HashMap::new();
    for (i, m) in glb.doc["materials"].as_array()?.clone().iter().enumerate() {
        let Some(name) = m["name"].as_str().and_then(|n| n.strip_prefix(PREFIX)) else { continue };
        let k = key(name);
        glb.doc["materials"][i]["name"] = json!(format!("{k}.tga"));
        glb.doc["materials"][i]["doubleSided"] = json!(true);
        let Some(image) = textures.get(&k) else {
            eprintln!("mod model: retail texture {name:?} is not in the import; the material stays untextured");
            continue;
        };
        let texture = match slot.get(&k) {
            Some(t) => *t,
            None => {
                let source = glb.add_image(image)?;
                let texture = glb.add_texture(source)?;
                slot.insert(k, texture);
                texture
            }
        };
        glb.doc["materials"][i]["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index": texture});
        glb.doc["materials"][i]["pbrMetallicRoughness"].as_object_mut()?.remove("baseColorFactor");
    }
    glb.finish()
}

/// The texture keys the model's materials name (`retail:` prefix and `.tga` ignored).
fn material_keys(doc: &Value) -> impl Iterator<Item = String> + '_ {
    doc["materials"].as_array().into_iter().flatten().filter_map(|m| m["name"].as_str()).map(|n| key(n.strip_prefix(PREFIX).unwrap_or(n)))
}

/// True when the GLB at `path` has a material whose texture key is in `images`. Reads only the JSON chunk, so scanning a model that
/// uses none of them costs a small read, not the model.
pub fn uses_any(path: &Path, images: &HashMap<String, Vec<u8>>) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    header(&mut f).is_some_and(|(doc, _)| material_keys(&doc).any(|k| images.contains_key(&k)))
}

/// Not retail: the model with every material whose texture key is in `images` (PNG bytes by key) showing that picture instead; the
/// keys it replaced, or `None` when no material matches. A material that had a texture keeps its sampler; one that had none (a retail
/// texture the import lacked) gets a repeating one. The bytes are embedded undecoded, so any size works.
pub fn retexture(bytes: &[u8], images: &HashMap<String, Vec<u8>>) -> Option<(Vec<u8>, Vec<String>)> {
    let mut glb = Glb::parse(bytes)?;
    let mut slot: HashMap<String, usize> = HashMap::new();
    for (i, m) in glb.doc["materials"].as_array()?.clone().iter().enumerate() {
        let k = key(m["name"].as_str().map(|n| n.strip_prefix(PREFIX).unwrap_or(n))?);
        let Some(image) = images.get(&k) else { continue };
        let old = m["pbrMetallicRoughness"]["baseColorTexture"]["index"].as_u64().map(|t| t as usize);
        let texture = match slot.get(&k) {
            Some(t) => *t,
            None => {
                let source = glb.add_image(image)?;
                let texture = match old.and_then(|t| glb.doc["textures"].get(t).cloned()) {
                    Some(mut t) => {
                        t["source"] = json!(source);
                        glb.doc["textures"].as_array_mut()?.push(t);
                        glb.doc["textures"].as_array()?.len() - 1
                    }
                    None => glb.add_texture(source)?,
                };
                slot.insert(k.clone(), texture);
                texture
            }
        };
        let pbr = glb.doc["materials"][i]["pbrMetallicRoughness"].as_object_mut();
        let pbr = match pbr {
            Some(p) => p,
            None => {
                glb.doc["materials"][i]["pbrMetallicRoughness"] = json!({});
                glb.doc["materials"][i]["pbrMetallicRoughness"].as_object_mut()?
            }
        };
        pbr.insert("baseColorTexture".into(), json!({"index": texture}));
        if old.is_none() {
            pbr.remove("baseColorFactor");
        }
        // PNG colour type 4 or 6 has an alpha channel: cut-outs (rigging, lace) need the mask a retail texture with alpha gets.
        if matches!(image.get(25), Some(4 | 6)) && glb.doc["materials"][i]["alphaMode"].is_null() {
            glb.doc["materials"][i]["alphaMode"] = json!("MASK");
        }
    }
    if slot.is_empty() {
        return None;
    }
    let mut used: Vec<String> = slot.into_keys().collect();
    used.sort();
    Some((glb.finish()?, used))
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

    fn textured(name: &str, image: &[u8]) -> Vec<u8> {
        let doc = json!({"asset": {"version": "2.0"}, "buffers": [{"byteLength": image.len()}], "bufferViews": [{"buffer": 0, "byteLength": image.len()}],
            "images": [{"bufferView": 0, "mimeType": "image/png"}], "samplers": [{"wrapS": 33071}], "textures": [{"sampler": 0, "source": 0}],
            "materials": [{"name": name, "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}}, {"name": "plain.tga", "pbrMetallicRoughness": {}}]});
        glb(doc, image)
    }

    #[test]
    fn retexture_swaps_the_image_of_a_matching_material_and_keeps_its_sampler() {
        let old = crate::shipart::png(2, 2, &[7u8; 16]);
        let new = crate::shipart::png(4, 4, &[9u8; 64]);
        let images = HashMap::from([("hull_planks".to_string(), new.clone())]);
        let (out, used) = retexture(&textured("Hull_Planks.tga", &old), &images).expect("a match is rewritten");
        assert_eq!(used, ["hull_planks"]);
        let (doc, end) = crate::data::glb_json(&out).unwrap();
        let t = doc["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["index"].as_u64().unwrap() as usize;
        assert_eq!(doc["textures"][t]["sampler"], 0, "the material keeps its sampler");
        let view = &doc["bufferViews"][doc["images"][doc["textures"][t]["source"].as_u64().unwrap() as usize]["bufferView"].as_u64().unwrap() as usize];
        let at = end + 8 + view["byteOffset"].as_u64().unwrap() as usize;
        assert_eq!(&out[at..at + new.len()], &new[..], "the mod's bytes, undecoded");
        assert_eq!(doc["materials"][0]["alphaMode"], "MASK", "an RGBA picture gets the cut-out mask");
        assert!(doc["materials"][1]["pbrMetallicRoughness"]["baseColorTexture"].is_null());
        assert!(retexture(&textured("other.tga", &old), &images).is_none(), "no matching material: nothing to rewrite");
        assert!(retexture(&textured("retail:HULL_PLANKS", &old), &images).is_some(), "a retail: name matches too");
    }

    #[test]
    fn uses_any_reads_only_the_names() {
        let dir = std::env::temp_dir().join(format!("procyon-retex-uses-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.glb"), textured("Hull_Planks.tga", &[1u8; 8])).unwrap();
        let images = HashMap::from([("hull_planks".to_string(), vec![])]);
        assert!(uses_any(&dir.join("a.glb"), &images));
        assert!(!uses_any(&dir.join("a.glb"), &HashMap::from([("x".to_string(), vec![])])));
        assert!(!uses_any(&dir.join("missing.glb"), &images));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
