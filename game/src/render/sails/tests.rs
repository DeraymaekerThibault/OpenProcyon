use super::*;

fn quad(w: f32, h: f32) -> Mesh {
    let mut quad = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    quad.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 0.0, 0.0], [w, 0.0, 0.0], [w, h, 0.0], [0.0, h, 0.0]]);
    quad.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4]);
    quad.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
    quad.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    quad
}

#[test]
fn sail_rules_match_by_name_and_take_shader_numbers_either_way() {
    let s = parse(&serde_json::json!({"plain": ["Pirate"], "line": 0.04})).unwrap();
    let name = |n: &str| GltfMaterialName(n.into());
    assert!(s.matches(&name("111front Sail.tga")) && !s.matches(&name("hull.tga")), "default rule: names containing 'sail'");
    assert!(s.is_plain(&name("sails pirate orange.tga")) && !s.is_plain(&name("sail.tga")));
    assert_eq!(s.params.x, 0.04);
    let own = parse(&serde_json::json!({"match": ["canvas"], "params": [1, 2, 3]})).unwrap();
    assert!(own.matches(&name("Canvas_01")) && !own.matches(&name("sail.tga")));
    assert_eq!(own.params, Vec4::new(1.0, 2.0, 3.0, 0.0), "a mod's own shader takes raw params");
    assert!(parse(&Value::Null).is_none());
}

#[test]
fn a_sail_quad_is_cut_finely_with_its_edges_pinned() {
    let (fine, b) = subdivide(&quad(10.0, 10.0), None, &[]).unwrap();
    let tris = fine.indices().unwrap().len() / 3;
    assert!(tris >= 2 * 10 * 10, "two triangles become many: {tris}");
    assert!(fine.indices().unwrap().iter().all(|i| i < b.rest.len()));
    for (p, pin) in b.rest.iter().zip(&b.pin) {
        let edge = p.x.min(10.0 - p.x).min(p.y).min(10.0 - p.y) < 1e-4;
        assert!(!edge || *pin < 1e-3, "edges stay where the yards hold them: {p} {pin}");
    }
    assert!(b.pin.iter().any(|&x| x > 0.9), "the middle bellies out");
    assert!((0.0..=1.1).contains(&belly(Vec3::ZERO, 14.0, 0.3, 2.0, 1.0)));
    assert!(belly(Vec3::ZERO, 14.0, 0.3, 2.0, 0.0) < belly(Vec3::ZERO, 14.0, 0.3, 2.0, 1.0), "slack without wind");
}

#[test]
fn each_sail_of_a_mesh_folds_up_to_its_own_top() {
    // two sails in one mesh, one above the other: 0..10 and 20..25 high
    let mut two = quad(10.0, 10.0);
    let upper = [[0.0, 20.0, 0.0], [10.0, 20.0, 0.0], [10.0, 25.0, 0.0], [0.0, 25.0, 0.0]];
    let Some(VertexAttributeValues::Float32x3(p)) = two.attribute(Mesh::ATTRIBUTE_POSITION).cloned() else { panic!() };
    two.insert_attribute(Mesh::ATTRIBUTE_POSITION, [p, upper.to_vec()].concat());
    two.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 8]);
    two.insert_attribute(Mesh::ATTRIBUTE_UV_0, [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]].repeat(2));
    two.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]));
    let (_, b) = subdivide(&two, None, &[]).unwrap();
    for (p, top) in b.rest.iter().zip(&b.top) {
        assert_eq!(*top, if p.y >= 20.0 { 25.0 } else { 10.0 }, "{p}");
        let f = furled(*p, *top, 0.9);
        assert!((top - f.y - 0.1 * (top - p.y)).abs() < 1e-4 && f.x == p.x && f.z == p.z, "{p} -> {f}");
    }
    assert_eq!(parse(&serde_json::json!({})).unwrap().furl, 0.0, "classic unless asked");
}

/// A half disc of radius 10 standing on its straight edge (the yard, along x), 48 wedges from the centre, as many vertices as a
/// retail fan has.
fn fan_mesh() -> Mesh {
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    let rib = |k: usize| {
        let a = std::f32::consts::PI * k as f32 / 48.0;
        [10.0 * a.cos(), 10.0 * a.sin(), 0.0]
    };
    let mut pos = vec![[0.0, 0.0, 0.0]];
    pos.extend((0..=48).map(rib));
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 50]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, (0..50).map(|i| [i as f32 / 50.0, 0.5 * (i % 2) as f32]).collect::<Vec<_>>());
    m.insert_indices(Indices::U32((1..49).flat_map(|k| [0, k, k + 1]).collect()));
    m
}

#[test]
fn a_fan_sail_folds_from_both_sides_onto_its_middle_rib_keeping_every_ribs_length() {
    let fan = Fan { matching: vec![], races: vec![], pleats: 4.0, depth: 0.0 };
    let (_, b) = subdivide(&fan_mesh(), Some(&fan), &[]).unwrap();
    assert_eq!(b.fan.len(), b.rest.len());
    let pivot = Vec3::ZERO;
    for (rest, pt) in b.rest.iter().zip(&b.fan) {
        let pt = pt.expect("a real fan");
        assert!((pt.pivot - pivot).length() < 1e-3, "the ribs meet in the middle of the straight edge: {}", pt.pivot);
        assert!((fanned(&pt, 0.0, 4.0, 0.0) - *rest).length() < 1e-3, "open is unchanged: {rest}");
        let shut = fanned(&pt, 1.0, 4.0, 0.0);
        assert!((shut.distance(pivot) - rest.distance(pivot)).abs() < 1e-3, "the rib keeps its length: {rest} -> {shut}");
        assert!(shut.x.abs() < 1e-3 && shut.y >= -1e-3, "{rest} folds up onto the mast, not {shut}");
        let half = fanned(&pt, 0.5, 4.0, 0.0);
        assert!((half.distance(pivot) - rest.distance(pivot)).abs() < 1e-3);
    }
}

#[test]
fn fans_on_a_raked_mast_fold_onto_the_mast_not_onto_their_own_lean() {
    // two half discs on a mast that leans 10 degrees aft (+z), each sail leaning back a further 25 degrees
    let mast = Vec3::new(0.0, 10f32.to_radians().cos(), 10f32.to_radians().sin());
    let lean = Quat::from_rotation_x(35f32.to_radians());
    let fan = |hub: Vec3| -> Vec<(Vec3, Vec3)> {
        let mut v = vec![(hub, lean * Vec3::Z)];
        v.extend((0..=48).map(|k| {
            let a = std::f32::consts::PI * k as f32 / 48.0;
            (hub + lean * Vec3::new(4.0 * a.cos(), 4.0 * a.sin(), 0.0), lean * Vec3::Z)
        }));
        v
    };
    let (lo, hi) = (mast * 5.0, mast * 12.0);
    let (mut a, mut b) = (FanSail::new(&fan(lo), lo).unwrap(), FanSail::new(&fan(hi), hi).unwrap());
    assert!(a.middle().angle_between(mast) > 20f32.to_radians(), "the sail leans off its mast at rest");
    fold_lines(&mut [&mut a, &mut b], &[]);
    for f in [&a, &b] {
        for (q, _) in fan(f.pivot) {
            let shut = fanned(&f.point(q), 1.0, 0.0, 0.0) - f.pivot;
            assert!((shut.length() - (q - f.pivot).length()).abs() < 1e-3, "ribs keep their length");
            assert!(shut.length() < 1e-3 || shut.normalize().dot(mast) > 0.999, "{q} shut at {shut}, not along the mast");
        }
    }
}

#[test]
fn the_mast_is_found_beside_its_fans_not_the_yard_or_a_stay_and_shut_fans_lie_against_it() {
    // a vertical mast at depth -4 (a cylinder: points only at its ends), a yard crossing the centre line at each hub, and a
    // stay from the deck up to the masthead; the fans hang aft of the mast on their yards
    let mut pts = vec![Vec3::new(0.1, 3.0, -4.0), Vec3::new(-0.1, 3.0, -4.0), Vec3::new(0.1, 16.0, -4.0), Vec3::new(-0.1, 16.0, -4.0)];
    let hubs = [Vec3::new(0.0, 6.0, -6.8), Vec3::new(0.0, 11.0, -5.9)];
    for h in hubs {
        pts.extend([h + Vec3::new(-0.2, 0.0, 0.0), h + Vec3::new(0.2, 0.0, 0.0)]);
    }
    pts.extend([Vec3::new(0.0, 2.0, -10.0), Vec3::new(0.0, 16.0, -4.0)]);
    let mains: Vec<(Vec3, f32)> = hubs.iter().map(|&h| (h, 4.0)).collect();
    let (at, up) = mast_line(&mains, &pts).expect("a mast");
    assert!((at.z - -4.0).abs() < 0.2 && up.dot(Vec3::Y) > 0.999, "{at} {up}");
    let fan = |hub: Vec3| -> Vec<(Vec3, Vec3)> {
        let mut v = vec![(hub, Vec3::X)];
        v.extend((0..=48).map(|k| {
            let a = std::f32::consts::PI * k as f32 / 48.0;
            (hub + Vec3::new(0.0, 4.0 * a.sin(), 4.0 * a.cos()), Vec3::X)
        }));
        v
    };
    let (mut a, mut b) = (FanSail::new(&fan(hubs[0]), hubs[0]).unwrap(), FanSail::new(&fan(hubs[1]), hubs[1]).unwrap());
    fold_lines(&mut [&mut a, &mut b], &pts);
    for f in [&a, &b] {
        for (q, _) in fan(f.pivot) {
            let shut = fanned(&f.point(q), 1.0, 0.0, 0.0);
            assert!((shut.z - -4.0).abs() < 0.2 && shut.y >= f.pivot.y - 1e-3, "{q} shut at {shut}, off the mast");
        }
        assert!((fanned(&f.point(f.pivot), 0.0, 0.0, 0.0) - f.pivot).length() < 1e-4, "open, the hub stays on its yard");
    }
}

#[test]
fn a_jib_sized_fan_keeps_the_roller_fold() {
    // 12 wedges about a hub, 13 vertices: the size of the cutters' front sail pieces
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    let mut pos = vec![[0.0, 0.0, 0.0]];
    pos.extend((0..=11).map(|k| {
        let a = std::f32::consts::PI * k as f32 / 11.0;
        [2.0 * a.cos(), 2.0 * a.sin(), 0.0]
    }));
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 13]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, (0..13).map(|i| [i as f32 / 13.0, 0.5 * (i % 2) as f32]).collect::<Vec<_>>());
    m.insert_indices(Indices::U32((1..12).flat_map(|k| [0, k, k + 1]).collect()));
    let fan = Fan { matching: vec![], races: vec![], pleats: 4.0, depth: 0.0 };
    let (_, b) = subdivide(&m, Some(&fan), &[]).unwrap();
    assert!(b.fan.iter().all(Option::is_none), "too small to be a fan sail");
}

#[test]
fn a_side_fan_standing_on_a_vertical_edge_closes_to_horizontal() {
    // the same half disc turned so its straight edge is upright and it opens outward along +x
    let mut m = fan_mesh();
    if let Some(VertexAttributeValues::Float32x3(pos)) = m.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
        pos.iter_mut().for_each(|p| *p = [p[1], -p[0], 0.0]);
    }
    let fan = Fan { matching: vec![], races: vec![], pleats: 4.0, depth: 0.0 };
    let (_, b) = subdivide(&m, Some(&fan), &[]).unwrap();
    for pt in b.fan.iter().flatten() {
        let shut = fanned(pt, 1.0, 4.0, 0.0);
        assert!(shut.y.abs() < 1e-3 && shut.x >= -1e-3, "closed flat along the spar: {shut}");
    }
}

#[test]
fn the_belly_never_passes_its_bound_and_its_slope_is_the_shaders() {
    let (size, phase, wind) = (12.0, 0.7, 1.0);
    for i in 0..400 {
        let (clock, p) = (i as f32 * 0.37, Vec3::new((i % 17) as f32 * 0.6, (i % 23) as f32 * 0.5, 0.0));
        assert!(belly(p, size, phase, clock, wind) <= BELLY_MAX + 1e-6);
        let e = 1e-3;
        let fd = Vec3::new(
            belly(p + Vec3::X * e, size, phase, clock, wind) - belly(p - Vec3::X * e, size, phase, clock, wind),
            belly(p + Vec3::Y * e, size, phase, clock, wind) - belly(p - Vec3::Y * e, size, phase, clock, wind),
            0.0,
        ) / (2.0 * e);
        assert!((fd - belly_slope(p, size, phase, clock, wind)).length() < 2e-3, "{fd} vs {}", belly_slope(p, size, phase, clock, wind));
    }
}

#[test]
fn gradients_are_exact_for_a_plane_ramp() {
    let (fine, b) = subdivide(&quad(10.0, 10.0), None, &[]).unwrap();
    let g = gradients(&b.rest, &triangles(&fine, b.rest.len()), |i| Some(2.0 * b.rest[i].x - 3.0 * b.rest[i].y));
    assert!(g.iter().all(|g| (*g - Vec3::new(2.0, -3.0, 0.0)).length() < 1e-3));
    let none = gradients(&b.rest, &triangles(&fine, b.rest.len()), |_| None);
    assert!(none.iter().all(|g| *g == Vec3::ZERO), "no values, no slope");
}

#[test]
fn the_fold_data_is_laid_out_as_the_vertex_stage_reads_it() {
    // a roller sail: no fans, five rows a vertex
    let (fine, b) = subdivide(&quad(10.0, 10.0), None, &[]).unwrap();
    let d = fold_data(&b, &triangles(&fine, b.rest.len()));
    assert_eq!(d.len(), 1 + FOLD_ROWS * b.rest.len());
    assert_eq!(d[0].x, 0.0);
    for i in 0..b.rest.len() {
        let r = &d[1 + FOLD_ROWS * i..1 + FOLD_ROWS * (i + 1)];
        assert_eq!((r[0].x, r[0].y, r[1].w), (b.pin[i], b.top[i], 0.0), "pin, top, not a fan");
        assert_eq!(r[3], Vec4::from(Quat::IDENTITY));
        assert_eq!(r[4], b.normal[i].extend(0.0), "the rest normal, for the passes that get none");
    }
    // a fan: its table first, then each point's span, rim, fan and turn
    let fan = Fan { matching: vec![], races: vec![], pleats: 6.0, depth: 0.02 };
    let (fine, b) = subdivide(&fan_mesh(), Some(&fan), &[]).unwrap();
    let d = fold_data(&b, &triangles(&fine, b.rest.len()));
    let fans = d[0].x as usize;
    assert!(fans >= 1);
    assert_eq!(d.len(), 1 + 3 * fans + FOLD_ROWS * b.rest.len());
    for (i, p) in b.fan.iter().enumerate() {
        let r = &d[1 + 3 * fans + FOLD_ROWS * i..];
        let Some(p) = p else { continue };
        let f = r[2].w as usize;
        assert_eq!((r[0].z, r[0].w, r[1].w), (p.span, p.rim, 1.0));
        assert_eq!((d[1 + 3 * f].truncate(), d[2 + 3 * f].truncate(), d[3 + 3 * f].truncate()), (p.pivot, p.n, p.to_mast), "its fan's row");
        assert_eq!(r[3], Vec4::from(p.shut));
    }
}

#[test]
fn the_bounds_hold_the_sail_open_bellied_and_shut() {
    let s = parse(&serde_json::json!({"billow": 0.04, "furl": 0.9, "fan": {}})).unwrap();
    let fan = s.fan.clone().unwrap();
    for (mesh, fan) in [(quad(10.0, 10.0), None), (fan_mesh(), Some(&fan))] {
        let (_, b) = subdivide(&mesh, fan, &[]).unwrap();
        let bounds = fold_bounds(&b, &s);
        let (lo, hi) = (Vec3::from(bounds.min()), Vec3::from(bounds.max()));
        let inside = |q: Vec3| q.cmpge(lo - 1e-4).all() && q.cmple(hi + 1e-4).all();
        for i in 0..b.rest.len() {
            for a in [0.0, 0.3, 0.6, 0.9] {
                let q = match b.fan.get(i).copied().flatten() {
                    Some(p) => fanned(&p, a, b.pleats, b.depth * b.size),
                    None => furled(b.rest[i], b.top[i], a),
                };
                assert!(inside(q), "{q} at fold {a} outside {lo}..{hi}");
            }
            assert!(inside(b.rest[i] + b.normal[i] * s.billow * b.size * BELLY_MAX));
        }
    }
}

#[test]
fn a_sail_eases_its_bulge_out_as_it_folds() {
    let s = parse(&serde_json::json!({"billow": 0.04, "furl": 0.9})).unwrap();
    let mut sail = Sail { ship: None, wind: 0.0, clock: 0.0, phase: 0.0, furl: 0.0, size: 10.0, pleats: 6.0, depth: 0.02 };
    assert!(sail.fold(&s).abs_diff_eq(Vec4::new(0.4, 0.0, 6.0, 0.2), 1e-6), "open: the full bulge, not folded");
    sail.furl = 1.0;
    assert!(sail.fold(&s).abs_diff_eq(Vec4::new(0.0, 0.9, 6.0, 0.2), 1e-6), "shut: no bulge, folded by `furl`");
}

#[test]
fn a_fan_pleats_out_of_its_plane_only_while_it_is_closing() {
    let fan = Fan { matching: vec![], races: vec![], pleats: 4.0, depth: 0.1 };
    let (_, b) = subdivide(&fan_mesh(), Some(&fan), &[]).unwrap();
    let depth = b.depth * b.size;
    assert!(depth > 0.0);
    let open = b.fan.iter().flatten().map(|pt| fanned(pt, 0.0, 4.0, depth).z.abs()).fold(0.0, f32::max);
    let shut = b.fan.iter().flatten().map(|pt| fanned(pt, 1.0, 4.0, depth).z.abs()).fold(0.0, f32::max);
    assert!(open < 1e-4 && shut > 0.01, "open {open}, shut {shut}");
}

#[test]
fn fan_rules_match_by_material_or_race_and_stay_off_without_the_key() {
    let s = parse(&serde_json::json!({"fan": {"match": ["gold"], "pleats": 99}})).unwrap();
    let fan = s.fan.unwrap();
    assert_eq!(fan.pleats, 24.0, "clamped");
    let name = |n: &str| GltfMaterialName(n.into());
    assert!(fan.applies(&name("gold sails.tga"), None) && fan.applies(&name("sail.tga"), Some("Navy")) && !fan.applies(&name("sail.tga"), Some("Pirate")));
    assert!(parse(&serde_json::json!({})).unwrap().fan.is_none());
    // a sail that does not fan keeps the roller-blind fold
    let (_, b) = subdivide(&quad(10.0, 10.0), None, &[]).unwrap();
    assert!(b.fan.is_empty());
}

/// Every sail primitive (material name contains "sail") of a ship GLB: positions, normals and triangles.
#[allow(clippy::type_complexity)]
fn glb_sails(bytes: &[u8]) -> Vec<(String, Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[usize; 3]>, Vec<Vec3>)> {
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let doc: Value = serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
    let bin_at = 20 + json_len + 8;
    let bin = &bytes[bin_at..];
    let read = |acc: usize, width: usize| -> Vec<u8> {
        let a = &doc["accessors"][acc];
        let v = &doc["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
        let off = v["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
        bin[off..off + a["count"].as_u64().unwrap() as usize * width].to_vec()
    };
    let f3 = |b: Vec<u8>| -> Vec<[f32; 3]> { b.as_chunks::<12>().0.iter().map(|c| std::array::from_fn(|i| f32::from_le_bytes(c[4 * i..4 * i + 4].try_into().unwrap()))).collect() };
    let mut out = vec![];
    for mesh in doc["meshes"].as_array().unwrap() {
        let prims = mesh["primitives"].as_array().unwrap();
        let name_of = |p: &Value| doc["materials"][p["material"].as_u64().unwrap_or(0) as usize]["name"].as_str().unwrap_or("").to_string();
        let mast: Vec<Vec3> = prims
            .iter()
            .filter(|p| !name_of(p).to_lowercase().contains("sail"))
            .flat_map(|p| f3(read(p["attributes"]["POSITION"].as_u64().unwrap() as usize, 12)))
            .map(Vec3::from)
            .filter(|q| q.x.abs() < MAST_HALF_WIDTH)
            .collect();
        for p in prims {
            let name = name_of(p);
            if !name.to_lowercase().contains("sail") {
                continue;
            }
            let pos = f3(read(p["attributes"]["POSITION"].as_u64().unwrap() as usize, 12));
            let nor = f3(read(p["attributes"]["NORMAL"].as_u64().unwrap() as usize, 12));
            let ia = p["indices"].as_u64().unwrap() as usize;
            let wide = doc["accessors"][ia]["componentType"].as_u64() == Some(5125);
            let raw = read(ia, if wide { 4 } else { 2 });
            let idx: Vec<usize> = if wide {
                raw.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c) as usize).collect()
            } else {
                raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c) as usize).collect()
            };
            out.push((name, pos, nor, idx.as_chunks::<3>().0.to_vec(), mast.clone()));
        }
    }
    out
}

#[test]
#[ignore = "needs imported assets"]
fn every_navy_sail_is_one_connected_fan() {
    let (mut fans, mut main_fans, mut mains, mut on_mast) = (0, 0, 0, 0);
    for dir in std::fs::read_dir(crate::assets_dir().join("ships")).unwrap().flatten() {
        let ship: Value = crate::data::read_json(&dir.path().join("ship.json"));
        if ship["race"] != "Navy" {
            continue;
        }
        let Ok(bytes) = std::fs::read(dir.path().join("model.glb")) else { continue };
        for (name, pos, nor, tris, mast) in glb_sails(&bytes) {
            let groups = fan_groups(&pos, &tris);
            let mut by: std::collections::BTreeMap<usize, (Vec3, Vec<(Vec3, Vec3)>)> = Default::default();
            let mut scraps = 0;
            for t in &tris {
                for &i in t {
                    match groups[i] {
                        Some((k, hub)) => {
                            let g = by.entry(k).or_insert((hub, vec![]));
                            if !g.1.iter().any(|v| v.0 == Vec3::from(pos[i])) {
                                g.1.push((Vec3::from(pos[i]), Vec3::from(nor[i])));
                            }
                        }
                        None => scraps += 1,
                    }
                }
            }
            println!("{:?} {name}: {} fans, {scraps} corners of non-fan cloth (roller-blind fold)", dir.file_name(), by.len());
            let mut built: Vec<(usize, usize, FanSail)> = by.iter().filter_map(|(k, (hub, vs))| FanSail::new(vs, *hub).map(|f| (*k, vs.len(), f))).collect();
            for (k, (_, vs)) in &by {
                if !built.iter().any(|b| b.0 == *k) {
                    println!("   #{k}: {} verts, not a fan: roller-blind fold", vs.len());
                }
            }
            fold_lines(&mut built.iter_mut().map(|b| &mut b.2).collect::<Vec<_>>(), &mast);
            for (k, n, f) in &built {
                fans += 1;
                main_fans += usize::from(*n >= 80);
                println!(
                    "   #{k}: {n} verts, hub {:.2?}, span {:.0}..{:.0} deg, middle rib {:.0} deg from up, folds onto {:.2?}, hub moves {:.2?}",
                    f.pivot,
                    f.min.to_degrees(),
                    f.max.to_degrees(),
                    f.middle().angle_between(Vec3::Y).to_degrees(),
                    f.target,
                    f.to_mast
                );
                if f.middle().y > 0.5 {
                    mains += 1;
                    on_mast += usize::from(f.to_mast != Vec3::ZERO);
                }
            }
        }
    }
    println!("{on_mast} of {mains} main fans slide onto a mast");
    assert!(on_mast * 10 >= mains * 9, "only {on_mast} of {mains} main fans found their mast");
    assert!(fans > 20 && main_fans > 20, "{fans} fans, {main_fans} big ones");
}

#[test]
fn uv_scale_measures_world_length_per_uv_unit() {
    // 20 wide along u, 5 tall along v, UV 0..1 both ways: a stretched texture
    let s = uv_world_scale(&quad(20.0, 5.0)).unwrap();
    assert!((s - Vec2::new(20.0, 5.0)).length() < 1e-3, "{s}");
}

/// How far (a share of the sail size) a sail point bellies out at the sail's clock: a slow breath plus ripples running
/// across it; with little wind the cloth hangs slacker. The reference `sail_fold.wesl` repeats (with its gradient, `belly_slope`).
fn belly(rest: Vec3, size: f32, phase: f32, clock: f32, wind: f32) -> f32 {
    let k = 6.0 / size;
    let shape = 0.65 + 0.25 * (1.1 * clock + phase).sin() + 0.12 * (3.3 * clock - k * (rest.y + 0.6 * rest.x) + phase).sin();
    shape * (0.3 + 0.7 * wind)
}

/// The gradient of `belly` along the rest position, as `sail_fold.wesl` works it out for the bellied cloth's normal.
fn belly_slope(rest: Vec3, size: f32, phase: f32, clock: f32, wind: f32) -> Vec3 {
    let k = 6.0 / size;
    let arg = 3.3 * clock - k * (rest.y + 0.6 * rest.x) + phase;
    (0.3 + 0.7 * wind) * 0.12 * arg.cos() * -k * Vec3::new(0.6, 1.0, 0.0)
}

#[test]
fn a_cut_sail_maps_its_cloth_from_the_hub_for_the_shader() {
    let fan = Fan { matching: vec![], races: vec![], pleats: 4.0, depth: 0.0 };
    for (mesh, fan) in [(fan_mesh(), Some(&fan)), (quad(10.0, 10.0), None)] {
        let (fine, b) = subdivide(&mesh, fan, &[]).unwrap();
        let Some(VertexAttributeValues::Float32x2(place)) = fine.attribute(Mesh::ATTRIBUTE_UV_1) else { panic!("no uv_b") };
        for (i, [d, around]) in place.iter().enumerate() {
            match b.fan.get(i).copied().flatten() {
                Some(p) => assert_eq!((*d, *around), (p.rim, p.span), "a fan: from its hub, across its span"),
                None => assert!((b.rest[i].y - 10.0 * *d).abs() < 1e-4 && (b.rest[i].x - 10.0 * *around).abs() < 1e-4, "else up from the bottom edge, across"),
            }
        }
    }
}
