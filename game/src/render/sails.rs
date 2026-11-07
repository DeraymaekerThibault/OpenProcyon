//! Not retail: sails of the render profile (part of render/mod.rs's look). glTF materials whose name contains one of
//! `sails.match` get a material of their own: the retail StandardMaterial (made solid with `opaque`) extended by the shader
//! `shaders/sail.wesl`, which the mod ships. The engine feeds it the profile's numbers, the sun, the sail's UV
//! stretch and its ship's speed (see `SailGlowUniform` for the contract); with `billow` the mesh is cut finer and bellies
//! out between its edges, more with speed; with `furl` each sail folds up to its top edge (the yard) while its ship is
//! ordered to All Stop, as the film's solar sails fold away.

use bevy::asset::RenderAssetUsages;
use bevy::gltf::GltfMaterialName;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use serde_json::Value;

use super::surfaces::Restyled;
use super::{Look, names, num, rgb};

/// The sail shader's asset path; the mod ships it.
pub const SHADER: &str = "shaders/sail.wesl";

/// The sails' look: which materials are sails, the glow the shader draws and the cloth's motion.
#[derive(Clone, Debug, PartialEq)]
pub struct Sails {
    /// glTF material names (lower case fragments) that are sails; default `["sail"]`.
    pub matching: Vec<String>,
    /// Sails whose material name contains one of these billow but get glow colour 0 (patched pirate cloth, civilian canvas).
    pub plain: Vec<String>,
    /// Glow colour (linear) times strength; black = no glow.
    pub glow: Vec3,
    /// How fast the sail's clock runs at full speed.
    pub speed: f32,
    /// Pattern cells across the sail's longer side; the shader gets cells per UV unit, kept regular however the texture is
    /// stretched.
    pub cells: f32,
    /// Four numbers for the shader, whose meaning is the shader's (aliases `line`, `wave`, `steady`, `fill`).
    pub params: Vec4,
    /// Extra glow when the sun shines through the sail from behind.
    pub backlit: f32,
    /// Sunlight passing through the cloth (StandardMaterial diffuse_transmission).
    pub transmission: f32,
    /// How far the cloth bellies out, as a share of the sail's size; 0 = rigid as in classic.
    pub billow: f32,
    /// Solid cloth instead of retail's see-through sails (they then also cast shadows).
    pub opaque: bool,
    /// Share of a sail's height folded away while its ship is ordered to stop; 0 = never, as in classic.
    pub furl: f32,
}

impl Sails {
    pub fn matches(&self, name: &GltfMaterialName) -> bool {
        let n = name.0.to_lowercase();
        self.matching.iter().any(|m| n.contains(m))
    }
    fn is_plain(&self, name: &GltfMaterialName) -> bool {
        let n = name.0.to_lowercase();
        self.plain.iter().any(|m| n.contains(m))
    }
}

/// The `sails` block. The shader's four numbers are `params`, or one by one `line`, `wave`, `steady` and `fill`.
pub fn parse(s: &Value) -> Option<Sails> {
    let params = match s["params"].as_array() {
        Some(a) => Vec4::from_array(std::array::from_fn(|i| a.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32)),
        None => Vec4::new(num(&s["line"], 0.06), num(&s["wave"], 12.0), num(&s["steady"], 0.05), num(&s["fill"], 0.25)),
    };
    s.is_object().then(|| Sails {
        matching: names(&s["match"], &["sail"]),
        plain: names(&s["plain"], &[]),
        glow: rgb(&s["glow"]).unwrap_or(Vec3::ZERO),
        speed: num(&s["speed"], 0.6),
        cells: num(&s["cells"], 16.0),
        params,
        backlit: num(&s["backlit"], 1.5),
        transmission: num(&s["transmission"], 0.0),
        billow: num(&s["billow"], 0.0),
        opaque: s["opaque"].as_bool().unwrap_or(false),
        furl: num(&s["furl"], 0.0).clamp(0.0, 1.0),
    })
}

pub fn plugin(app: &mut App, look: &Look) {
    if look.sails.is_some() {
        app.add_plugins(MaterialPlugin::<SailMaterial>::default()).add_systems(Update, (sails, sail_motion, billow, super::surfaces::cloaked::<SailMaterial>).chain());
    }
}

/// What the sail shader gets at `@group(MATERIAL_BIND_GROUP) @binding(100)`, besides the StandardMaterial bindings.
#[derive(Clone, Copy, Default, Debug, ShaderType)]
pub struct SailGlowUniform {
    /// rgb: `glow` (0 for a plain sail)
    pub color: Vec4,
    /// the profile's four `params`
    pub params: Vec4,
    /// xyz: direction sunlight travels (world), w: `backlit`
    pub sun: Vec4,
    /// x: the ship's smoothed share of full speed (0..1), y: a per-sail phase (radians), z: the sail's clock (seconds,
    /// running faster with speed)
    pub motion: Vec4,
    /// xy: pattern cells per UV unit along u and v
    pub pattern: Vec4,
}

/// The sail extension; see `SailGlowUniform`.
#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct SailGlow {
    #[uniform(100)]
    pub u: SailGlowUniform,
}

impl MaterialExtension for SailGlow {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

pub type SailMaterial = ExtendedMaterial<StandardMaterial, SailGlow>;

/// One sail mesh (with a material of its own) and the ship whose speed drives its glow and billowing.
#[derive(Component)]
struct Sail {
    ship: Option<Entity>,
    /// Smoothed share of the ship's full speed.
    wind: f32,
    clock: f32,
    phase: f32,
    /// 0 = set, 1 = furled (eased in `billow`).
    furl: f32,
}

/// A sail cut into a finer mesh of its own, with the rest shape it bellies out from and folds up from.
#[derive(Component)]
struct Billow {
    rest: Vec<Vec3>,
    normal: Vec<Vec3>,
    /// 0 at the sail's edges (held by yards and lines), 1 in its middle.
    pin: Vec<f32>,
    /// Height (mesh up, +Y) of the top edge of the sail each vertex belongs to; one mesh holds several sails.
    top: Vec<f32>,
    size: f32,
}

/// Materials named like retail sail textures ("sail.tga", "square sails orange.tga") become sail materials, one per sail
/// mesh so each follows its own ship's speed.
pub(super) fn sails(
    mut commands: Commands,
    look: Res<Look>,
    new: Query<(Entity, &GltfMaterialName, &MeshMaterial3d<StandardMaterial>, &Mesh3d), (Added<MeshMaterial3d<StandardMaterial>>, Without<Restyled<SailMaterial>>)>,
    parents: Query<&ChildOf>,
    ships: Query<&crate::Ship>,
    standard: Res<Assets<StandardMaterial>>,
    mut sail_mats: ResMut<Assets<SailMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Some(s) = &look.sails else { return };
    for (e, name, mat, mesh) in &new {
        if !s.matches(name) {
            continue;
        }
        let Some(base) = standard.get(&mat.0) else { continue };
        let span = meshes.get(&mesh.0).and_then(uv_world_scale).unwrap_or(Vec2::ONE);
        let cells = span / span.max_element().max(1e-6) * s.cells;
        let mut base = StandardMaterial { diffuse_transmission: s.transmission, reflectance: 0.25, ..base.clone() };
        base.perceptual_roughness = base.perceptual_roughness.max(0.7);
        if s.opaque {
            base.alpha_mode = AlphaMode::Opaque;
            base.base_color = base.base_color.with_alpha(1.0);
        }
        let phase = (e.index().index() as f32 * 0.618).fract() * std::f32::consts::TAU;
        let handle = sail_mats.add(SailMaterial {
            base,
            extension: SailGlow {
                u: SailGlowUniform {
                    color: if s.is_plain(name) { Vec4::ZERO } else { s.glow.extend(0.0) },
                    params: s.params,
                    sun: Vec4::new(0.0, -1.0, 0.0, s.backlit),
                    motion: Vec4::new(0.0, phase, 0.0, 0.0),
                    pattern: cells.extend(0.0).extend(0.0),
                },
            },
        });
        let ship = parents.iter_ancestors(e).find(|a| ships.contains(*a));
        // a ship that starts stopped starts with its sails folded
        let furl = ship.and_then(|e| ships.get(e).ok()).map_or(0.0, |sh| f32::from(u8::from(sh.ordered() == 0.0)));
        let mut entity = commands.entity(e);
        let restyled = Restyled { original: mat.0.clone(), styled: handle.clone(), ship };
        entity.try_remove::<MeshMaterial3d<StandardMaterial>>().try_insert((MeshMaterial3d(handle), Sail { ship, wind: 0.0, clock: 0.0, phase, furl }, restyled));
        if !s.opaque {
            entity.try_insert(NotShadowCaster);
        }
        if (s.billow > 0.0 || s.furl > 0.0)
            && let Some((fine, billow)) = meshes.get(&mesh.0).and_then(subdivide)
        {
            entity.try_insert((Mesh3d(meshes.add(fine)), billow));
        }
    }
}

/// The mesh's triangles as vertex index triples.
fn triangles(mesh: &Mesh, vertices: usize) -> Vec<[usize; 3]> {
    match mesh.indices() {
        Some(i) => i.iter().collect::<Vec<_>>().as_chunks::<3>().0.to_vec(),
        None => (0..vertices / 3).map(|t| [3 * t, 3 * t + 1, 3 * t + 2]).collect(),
    }
}

/// World length per UV unit along u and along v over the mesh (area-weighted from each triangle's UV mapping), so the
/// shader's pattern keeps square-ish cells of one size however the sail's texture is stretched.
fn uv_world_scale(mesh: &Mesh) -> Option<Vec2> {
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return None };
    let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else { return None };
    let (mut sum, mut weight) = (Vec2::ZERO, 0.0);
    for [a, b, c] in triangles(mesh, pos.len()) {
        let (e1, e2) = (Vec3::from(pos[b]) - Vec3::from(pos[a]), Vec3::from(pos[c]) - Vec3::from(pos[a]));
        let (d1, d2) = (Vec2::from(uv[b]) - Vec2::from(uv[a]), Vec2::from(uv[c]) - Vec2::from(uv[a]));
        let det = d1.x * d2.y - d2.x * d1.y;
        let area = e1.cross(e2).length();
        if det.abs() < 1e-8 || area < 1e-8 {
            continue;
        }
        // position derivatives along u and v
        let du = (e1 * d2.y - e2 * d1.y) / det;
        let dv = (e2 * d1.x - e1 * d2.x) / det;
        sum += Vec2::new(du.length(), dv.length()) * area;
        weight += area;
    }
    (weight > 0.0).then(|| sum / weight)
}

/// The sail mesh cut so no triangle edge is longer than about a 14th of the sail, keeping position, normal and UV.
fn subdivide(mesh: &Mesh) -> Option<(Mesh, Billow)> {
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return None };
    let Some(VertexAttributeValues::Float32x3(nor)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else { return None };
    let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else { return None };
    let tris = triangles(mesh, pos.len());
    let used: Vec<usize> = tris.iter().flatten().copied().collect();
    if used.is_empty() {
        return None;
    }
    let p = |i: usize| Vec3::from(pos[i]);
    let (lo, hi) = used.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), &i| (lo.min(p(i)), hi.max(p(i))));
    let size = (hi - lo).length().max(1e-3);
    let (ulo, uhi) = used.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), &i| (lo.min(Vec2::from(uv[i])), hi.max(Vec2::from(uv[i]))));
    let uspan = (uhi - ulo).max(Vec2::splat(1e-3));
    let sail = sail_of(pos, &tris);
    let mut top = vec![f32::MIN; pos.len()];
    for &i in &used {
        top[sail[i]] = top[sail[i]].max(p(i).y);
    }
    let (mut out_p, mut out_n, mut out_uv, mut pin, mut idx, mut out_top) = (vec![], vec![], vec![], vec![], vec![], vec![]);
    for [a, b, c] in tris {
        let edge = p(a).distance(p(b)).max(p(b).distance(p(c))).max(p(c).distance(p(a)));
        let n = ((edge / (size / 14.0)).ceil() as usize).clamp(1, 12);
        let first = out_p.len() as u32;
        // row i from corner a toward b, column j toward c
        let at = |i: usize, j: usize| first + (i * (2 * n + 3 - i) / 2 + j) as u32;
        for i in 0..=n {
            for j in 0..=n - i {
                let (wb, wc) = (i as f32 / n as f32, j as f32 / n as f32);
                let wa = 1.0 - wb - wc;
                out_p.push(p(a) * wa + p(b) * wb + p(c) * wc);
                out_n.push((Vec3::from(nor[a]) * wa + Vec3::from(nor[b]) * wb + Vec3::from(nor[c]) * wc).normalize_or_zero());
                let t = Vec2::from(uv[a]) * wa + Vec2::from(uv[b]) * wb + Vec2::from(uv[c]) * wc;
                out_uv.push(t.to_array());
                let s = (t - ulo) / uspan;
                pin.push((16.0 * s.x * (1.0 - s.x) * s.y * (1.0 - s.y)).clamp(0.0, 1.0).sqrt());
                out_top.push(top[sail[a]]);
            }
        }
        for i in 0..n {
            for j in 0..n - i {
                idx.extend([at(i, j), at(i + 1, j), at(i, j + 1)]);
                if j + 1 < n - i {
                    idx.extend([at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)]);
                }
            }
        }
    }
    let mut fine = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    fine.insert_attribute(Mesh::ATTRIBUTE_POSITION, out_p.iter().map(|v| v.to_array()).collect::<Vec<_>>());
    fine.insert_attribute(Mesh::ATTRIBUTE_NORMAL, out_n.iter().map(|v| v.to_array()).collect::<Vec<_>>());
    fine.insert_attribute(Mesh::ATTRIBUTE_UV_0, out_uv);
    fine.insert_indices(Indices::U32(idx));
    Some((fine, Billow { rest: out_p, normal: out_n, pin, top: out_top, size }))
}

/// Which sail each vertex belongs to (a representative vertex index): triangles joined by shared or coincident corners.
fn sail_of(pos: &[[f32; 3]], tris: &[[usize; 3]]) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..pos.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    fn join(parent: &mut [usize], a: usize, b: usize) {
        let (ra, rb) = (root(parent, a), root(parent, b));
        parent[ra] = rb;
    }
    let mut seen = std::collections::HashMap::new();
    for (i, v) in pos.iter().enumerate() {
        let key = v.map(|c| (c * 1000.0).round() as i64);
        let first = *seen.entry(key).or_insert(i);
        join(&mut parent, i, first);
    }
    for &[a, b, c] in tris {
        join(&mut parent, a, b);
        join(&mut parent, b, c);
    }
    (0..pos.len()).map(|i| root(&mut parent, i)).collect()
}

/// A sail point folded up toward its sail's top edge: `amount` 0 leaves it, 1 lifts it onto the top.
fn furled(p: Vec3, top: f32, amount: f32) -> Vec3 {
    Vec3::new(p.x, top - (top - p.y) * (1.0 - amount), p.z)
}

/// How far (a share of the sail size) a sail point bellies out at the sail's clock: a slow breath plus ripples running
/// across it; with little wind the cloth hangs slacker.
fn belly(rest: Vec3, size: f32, phase: f32, clock: f32, wind: f32) -> f32 {
    let k = 6.0 / size;
    let shape = 0.65 + 0.25 * (1.1 * clock + phase).sin() + 0.12 * (3.3 * clock - k * (rest.y + 0.6 * rest.x) + phase).sin();
    shape * (0.3 + 0.7 * wind)
}

/// Each sail follows its ship's speed: the share of full speed (smoothed), a clock that runs faster with it, the sun.
fn sail_motion(
    time: Res<Time>,
    look: Res<Look>,
    lib: Option<Res<crate::combat::Library>>,
    ships: Query<&crate::Ship>,
    suns: Query<&GlobalTransform, With<DirectionalLight>>,
    mut sails: Query<(&mut Sail, &MeshMaterial3d<SailMaterial>)>,
    mut materials: ResMut<Assets<SailMaterial>>,
) {
    let (Some(s), Some(lib)) = (&look.sails, lib) else { return };
    let sun = suns.iter().next().map_or(Vec3::NEG_Y, |g| g.forward().as_vec3());
    let dt = time.delta_secs();
    for (mut sail, mat) in &mut sails {
        let share = sail.ship.and_then(|e| ships.get(e).ok()).map_or(0.0, |sh| (sh.speed.abs() / crate::ship::full_speed(&lib.classes[sh.class]).max(1e-3)).clamp(0.0, 1.0));
        sail.wind += (share - sail.wind) * (dt * 1.2).min(1.0);
        sail.clock += dt * s.speed * (0.35 + 0.65 * sail.wind);
        // PROVISIONAL: the crew folds the sails on the All Stop order (not when the ship has coasted to a stop), in 1.5 s.
        let stopped = sail.ship.and_then(|e| ships.get(e).ok()).is_some_and(|sh| sh.ordered() == 0.0);
        sail.furl = (sail.furl + if stopped { dt / 1.5 } else { -dt / 1.5 }).clamp(0.0, 1.0);
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.extension.u.motion = Vec4::new(sail.wind, sail.phase, sail.clock, 0.0);
            m.extension.u.sun = sun.extend(s.backlit);
        }
    }
}

fn billow(look: Res<Look>, sails: Query<(&Billow, &Sail, &Mesh3d)>, mut meshes: ResMut<Assets<Mesh>>) {
    let Some(s) = &look.sails else { return };
    for (b, sail, mesh) in &sails {
        let Some(mut m) = meshes.get_mut(&mesh.0) else { continue };
        let f = sail.furl * sail.furl * (3.0 - 2.0 * sail.furl); // smoothstep
        let amp = s.billow * b.size * (1.0 - f);
        let pos: Vec<[f32; 3]> = (0..b.rest.len())
            .map(|i| {
                let p = b.rest[i] + b.normal[i] * amp * b.pin[i] * belly(b.rest[i], b.size, sail.phase, sail.clock, sail.wind);
                furled(p, b.top[i], s.furl * f).to_array()
            })
            .collect();
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        m.compute_smooth_normals();
    }
}

#[cfg(test)]
mod tests {
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
        let (fine, b) = subdivide(&quad(10.0, 10.0)).unwrap();
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
        let (_, b) = subdivide(&two).unwrap();
        for (p, top) in b.rest.iter().zip(&b.top) {
            assert_eq!(*top, if p.y >= 20.0 { 25.0 } else { 10.0 }, "{p}");
            let f = furled(*p, *top, 0.9);
            assert!((top - f.y - 0.1 * (top - p.y)).abs() < 1e-4 && f.x == p.x && f.z == p.z, "{p} -> {f}");
        }
        assert_eq!(parse(&serde_json::json!({})).unwrap().furl, 0.0, "classic unless asked");
    }

    #[test]
    fn uv_scale_measures_world_length_per_uv_unit() {
        // 20 wide along u, 5 tall along v, UV 0..1 both ways: a stretched texture
        let s = uv_world_scale(&quad(20.0, 5.0)).unwrap();
        assert!((s - Vec2::new(20.0, 5.0)).length() < 1e-3, "{s}");
    }
}
