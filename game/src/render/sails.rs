//! Not retail: sails of the render profile (part of render/mod.rs's look). glTF materials whose name contains one of
//! `sails.match` get a material of their own: the retail StandardMaterial (made solid with `opaque`) extended by the shader
//! `shaders/sail.wesl`, which the mod ships. The engine feeds it the profile's numbers, the sun, the sail's UV
//! stretch and its ship's speed (see `SailGlowUniform` for the contract); with `billow` the mesh is cut finer and bellies
//! out between its edges, more with speed; with `furl` each sail folds up to its top edge (the yard) while its ship is
//! ordered to All Stop, as the film's solar sails fold away. With `fan` a Navy sail does not fold like a roller blind but
//! like a hand fan: its ribs swing in from both sides and up onto the mast while its hub slides onto the mast, so the shut sail
//! lies against it (side sails close flat), and the cloth between the ribs pleats.
//!
//! The bellying and folding run on the GPU: the engine's vertex stage (`sail_vertex.wesl`, `sail_prepass.wesl` for depth and shadows,
//! both through `sail_fold.wesl`) moves the cut mesh from per-vertex data baked once into a storage buffer of the sail's material
//! (`fold_data`), so the mesh is never rebuilt (on the CPU that cost about 1 to 1.7 ms a ship a frame). `belly`, `furled` and
//! `fanned` here are the reference the shader repeats, and give the sail's bounds.

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::NoAutoAabb;
use bevy::gltf::GltfMaterialName;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::render::storage::ShaderBuffer;
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
    /// Sails that fold like a hand fan instead of a roller blind.
    pub fan: Option<Fan>,
}

/// The `fan` part of the `sails` block.
#[derive(Clone, Debug, PartialEq)]
pub struct Fan {
    /// glTF material name fragments (lower case) of the sails that fan; none by default.
    pub matching: Vec<String>,
    /// Ship races (lower case) whose sails all fan (the Navy's sail materials share names with other fleets' canvas).
    pub races: Vec<String>,
    /// Ribs the cloth pleats into.
    pub pleats: f32,
    /// How far the pleats zig-zag out of the sail's plane, a share of the sail's size.
    pub depth: f32,
}

impl Fan {
    fn applies(&self, material: &GltfMaterialName, race: Option<&str>) -> bool {
        let n = material.0.to_lowercase();
        self.matching.iter().any(|m| n.contains(m)) || race.is_some_and(|r| self.races.iter().any(|x| x.eq_ignore_ascii_case(r)))
    }
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
        fan: s["fan"].is_object().then(|| Fan {
            matching: names(&s["fan"]["match"], &[]),
            races: names(&s["fan"]["races"], &["navy"]),
            pleats: num(&s["fan"]["pleats"], 6.0).clamp(1.0, 24.0),
            depth: num(&s["fan"]["depth"], 0.02).clamp(0.0, 0.2),
        }),
    })
}

/// The engine's vertex stages of a sail (they move the mesh the engine cut; the mod's shader only draws the cloth).
const VERTEX_SHADER: &str = "embedded://openprocyon/render/sail_vertex.wesl";
const PREPASS_SHADER: &str = "embedded://openprocyon/render/sail_prepass.wesl";

pub fn plugin(app: &mut App, look: &Look) {
    if look.sails.is_some() {
        bevy::shader::load_shader_library!(app, "sail_fold.wesl");
        bevy::asset::embedded_asset!(app, "sail_vertex.wesl");
        bevy::asset::embedded_asset!(app, "sail_prepass.wesl");
        app.add_plugins(MaterialPlugin::<SailMaterial>::default()).add_systems(Update, (sails, sail_motion, super::surfaces::cloaked::<SailMaterial>).chain());
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
    /// For the engine's vertex stage (`sail_fold.wesl`): x how far the cloth bellies (mesh units, eased out while folding), y how far
    /// it is folded (0 open .. `furl`), z pleats, w pleat depth (mesh units). A sail shader may ignore it.
    pub fold: Vec4,
    /// x: the sail's size (mesh units), y: 1 when `fold_data` holds the sail's fold data (else the mesh is drawn as it is)
    pub shape: Vec4,
}

/// The sail extension; see `SailGlowUniform`.
#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct SailGlow {
    #[uniform(100)]
    pub u: SailGlowUniform,
    /// The frame data (`frame.rs`) at `@binding(99)`.
    #[storage(99, read_only)]
    pub frame: super::frame::FrameBinding,
    /// The sail's fold data (`fold_data`), read by the vertex stages at `@binding(101)`.
    #[storage(101, read_only)]
    pub fold: Handle<ShaderBuffer>,
}

impl MaterialExtension for SailGlow {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn vertex_shader() -> ShaderRef {
        VERTEX_SHADER.into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        PREPASS_SHADER.into()
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
    /// 0 = set, 1 = furled (eased in `fold`).
    furl: f32,
    /// The cut sail's size, pleats and pleat depth (shares of the size); size 0 = not cut, drawn as it is.
    size: f32,
    pleats: f32,
    depth: f32,
}

impl Sail {
    /// The vertex stage's `fold` numbers now: the bulge eased out as the sail folds, and how far it is folded.
    fn fold(&self, s: &Sails) -> Vec4 {
        let f = self.furl * self.furl * (3.0 - 2.0 * self.furl); // smoothstep
        Vec4::new(s.billow * self.size * (1.0 - f), s.furl * f, self.pleats, self.depth * self.size)
    }
}

/// A sail cut into a finer mesh of its own, with the rest shape it bellies out from and folds up from.
struct Billow {
    rest: Vec<Vec3>,
    normal: Vec<Vec3>,
    /// 0 at the sail's edges (held by yards and lines), 1 in its middle.
    pin: Vec<f32>,
    /// Height (mesh up, +Y) of the top edge of the sail each vertex belongs to; one mesh holds several sails.
    top: Vec<f32>,
    size: f32,
    /// For a fan sail, each vertex's place in its fan (None where the sail is too small to have one); empty otherwise.
    fan: Vec<Option<FanPoint>>,
    pleats: f32,
    depth: f32,
}

/// One vertex of a fan sail: where it sits about the fan's pivot, in the sail's plane.
#[derive(Clone, Copy, Debug)]
struct FanPoint {
    pivot: Vec3,
    /// The sail's normal (the pleats zig-zag along it).
    n: Vec3,
    /// This point's place across the fan's span (0..1), for the pleats.
    span: f32,
    /// This radius over the sail's longest rib (pleats are deepest at the rim).
    rim: f32,
    /// The point from the pivot at rest.
    rest: Vec3,
    /// The turn that swings it, about the pivot, onto the line the fan closes onto (the mast, or the spar's line).
    shut: Quat,
    /// Where the pivot goes when the fan is shut, from where it is (onto the mast).
    to_mast: Vec3,
}

/// Materials named like retail sail textures ("sail.tga", "square sails orange.tga") become sail materials, one per sail
/// mesh so each follows its own ship's speed.
pub(super) fn sails(
    mut commands: Commands,
    look: Res<Look>,
    new: Query<(Entity, &GltfMaterialName, &MeshMaterial3d<StandardMaterial>, &Mesh3d), (Added<MeshMaterial3d<StandardMaterial>>, Without<Restyled<SailMaterial>>)>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    siblings: Query<(&Mesh3d, &GltfMaterialName)>,
    ships: Query<&crate::Ship>,
    lib: Option<Res<crate::combat::Library>>,
    standard: Res<Assets<StandardMaterial>>,
    mut sail_mats: ResMut<Assets<SailMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
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
                    ..default()
                },
                frame: default(),
                fold: buffers.add(ShaderBuffer::new(vec![Vec4::ZERO], RenderAssetUsages::RENDER_WORLD)),
            },
        });
        let ship = parents.iter_ancestors(e).find(|a| ships.contains(*a));
        // a ship that starts stopped starts with its sails folded
        let furl = ship.and_then(|e| ships.get(e).ok()).map_or(0.0, |sh| f32::from(u8::from(sh.ordered() == 0.0)));
        let restyled = Restyled { original: mat.0.clone(), styled: handle.clone(), ship };
        let mut entity = super::kit::swap::<StandardMaterial, _>(&mut commands, e, handle.clone());
        entity.try_insert(restyled);
        let mut sail = Sail { ship, wind: 0.0, clock: 0.0, phase, furl, size: 0.0, pleats: 0.0, depth: 0.0 };
        if !s.opaque {
            entity.try_insert(NotShadowCaster);
        }
        let race = ship.and_then(|e| ships.get(e).ok()).and_then(|sh| lib.as_ref().map(|l| l.classes[sh.class].race.as_str()));
        let fan = s.fan.as_ref().filter(|f| f.applies(name, race));
        // the mast the fans fold onto: the centre-line geometry of the other parts of this sail's model node
        let mast: Vec<Vec3> = match (fan, parents.get(e)) {
            (Some(_), Ok(parent)) => children
                .get(parent.parent())
                .into_iter()
                .flat_map(|c| c.iter())
                .filter(|&c| c != e)
                .filter_map(|c| siblings.get(c).ok())
                .filter(|(_, n)| !s.matches(n))
                .filter_map(|(m, _)| meshes.get(&m.0))
                .flat_map(|m| match m.attribute(Mesh::ATTRIBUTE_POSITION) {
                    Some(VertexAttributeValues::Float32x3(p)) => p.iter().map(|&q| Vec3::from(q)).filter(|q| q.x.abs() < MAST_HALF_WIDTH).collect(),
                    _ => vec![],
                })
                .collect(),
            _ => vec![],
        };
        if (s.billow > 0.0 || s.furl > 0.0)
            && let Some((fine, billow)) = meshes.get(&mesh.0).and_then(|m| subdivide(m, fan, &mast))
        {
            // the fold data goes to the GPU once; the bounds hold the sail open, bellied and shut, so culling never cuts it off
            let data = fold_data(&billow, &triangles(&fine, billow.rest.len()));
            let bounds = fold_bounds(&billow, s);
            if let Some(mut m) = sail_mats.get_mut(&handle) {
                m.extension.fold = buffers.add(ShaderBuffer::new(data, RenderAssetUsages::RENDER_WORLD));
                m.extension.u.shape = Vec4::new(billow.size, 1.0, 0.0, 0.0);
            }
            (sail.size, sail.pleats, sail.depth) = (billow.size, billow.pleats, billow.depth);
            // shaded by the cut cloth's own facets at rest (the fold data keeps the retail normal the cloth bellies along)
            let mut fine = fine;
            fine.compute_smooth_normals();
            entity.try_insert((Mesh3d(meshes.add(fine)), bounds, NoAutoAabb));
        }
        entity.try_insert(sail);
    }
}

/// Rows a vertex takes in `fold_data` (`ROWS` in sail_fold.wesl).
const FOLD_ROWS: usize = 5;

/// The gradient of a scalar over each vertex's triangles (area-weighted), in mesh units: how fast `f` changes along the surface.
/// Triangles with a vertex `f` has no value for are left out.
fn gradients(pos: &[Vec3], tris: &[[usize; 3]], f: impl Fn(usize) -> Option<f32>) -> Vec<Vec3> {
    let (mut sum, mut weight) = (vec![Vec3::ZERO; pos.len()], vec![0.0f32; pos.len()]);
    for &[a, b, c] in tris {
        let (Some(fa), Some(fb), Some(fc)) = (f(a), f(b), f(c)) else { continue };
        let (e1, e2) = (pos[b] - pos[a], pos[c] - pos[a]);
        let n = e1.cross(e2);
        let n2 = n.length_squared();
        if n2 < 1e-12 {
            continue;
        }
        // in the triangle's plane, grad . e1 = fb - fa and grad . e2 = fc - fa
        let g = ((fb - fa) * e2.cross(n) + (fc - fa) * n.cross(e1)) / n2;
        let area = n2.sqrt();
        for i in [a, b, c] {
            sum[i] += g * area;
            weight[i] += area;
        }
    }
    sum.iter().zip(&weight).map(|(g, w)| if *w > 0.0 { *g / *w } else { Vec3::ZERO }).collect()
}

/// What the vertex stage reads to belly and fold the cut sail (layout in sail_fold.wesl): a header with the number of fans, each
/// fan (pivot, plane normal, where its pivot goes when shut), then `FOLD_ROWS` rows a vertex. The cut mesh's patches each come
/// from one retail triangle, so a triangle never mixes two fans.
fn fold_data(b: &Billow, tris: &[[usize; 3]]) -> Vec<Vec4> {
    let point = |i: usize| b.fan.get(i).copied().flatten();
    let mut fans: Vec<(Vec3, Vec3, Vec3)> = vec![];
    let fan_of: Vec<Option<usize>> = (0..b.rest.len())
        .map(|i| {
            let p = point(i)?;
            let key = (p.pivot, p.n, p.to_mast);
            Some(fans.iter().position(|f| *f == key).unwrap_or_else(|| {
                fans.push(key);
                fans.len() - 1
            }))
        })
        .collect();
    let pin = gradients(&b.rest, tris, |i| Some(b.pin[i]));
    let span = gradients(&b.rest, tris, |i| point(i).map(|p| p.span));
    let mut out = Vec::with_capacity(1 + 3 * fans.len() + FOLD_ROWS * b.rest.len());
    out.push(Vec4::new(fans.len() as f32, 0.0, 0.0, 0.0));
    for (pivot, n, to_mast) in &fans {
        out.extend([pivot.extend(0.0), n.extend(0.0), to_mast.extend(0.0)]);
    }
    for i in 0..b.rest.len() {
        let p = point(i);
        out.extend([
            Vec4::new(b.pin[i], b.top[i], p.map_or(0.0, |p| p.span), p.map_or(0.0, |p| p.rim)),
            pin[i].extend(if p.is_some() { 1.0 } else { 0.0 }),
            span[i].extend(fan_of[i].map_or(0.0, |f| f as f32)),
            Vec4::from(p.map_or(Quat::IDENTITY, |p| p.shut)),
            b.normal[i].extend(0.0),
        ]);
    }
    out
}

/// Bounds holding the sail in every shape it takes: at rest, bellied out as far as it goes either way, and shut.
fn fold_bounds(b: &Billow, s: &Sails) -> Aabb {
    let bulge = s.billow * b.size * BELLY_MAX;
    let (mut lo, mut hi) = (Vec3::MAX, Vec3::MIN);
    for i in 0..b.rest.len() {
        let shut = match b.fan.get(i).copied().flatten() {
            Some(p) => fanned(&p, s.furl, b.pleats, b.depth * b.size),
            None => furled(b.rest[i], b.top[i], s.furl),
        };
        for q in [b.rest[i] + b.normal[i] * bulge, b.rest[i] - b.normal[i] * bulge, shut] {
            (lo, hi) = (lo.min(q), hi.max(q));
        }
    }
    Aabb::from_min_max(lo, hi)
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
fn subdivide(mesh: &Mesh, fan: Option<&Fan>, mast: &[Vec3]) -> Option<(Mesh, Billow)> {
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
    let (mut out_p, mut out_n, mut out_uv, mut pin, mut idx, mut out_top, mut out_sail) = (vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    for &[a, b, c] in &tris {
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
                out_sail.push(a);
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
    let fans = fan.map(|_| {
        let groups = fan_groups(pos, &tris);
        let mut sails: std::collections::HashMap<usize, Option<FanSail>> = std::collections::HashMap::new();
        for &i in &used {
            if let Some((root, hub)) = groups[i] {
                sails.entry(root).or_insert_with(|| {
                    let verts: Vec<(Vec3, Vec3)> = used.iter().filter(|&&j| groups[j].is_some_and(|g| g.0 == root)).map(|&j| (p(j), Vec3::from(nor[j]))).collect();
                    FanSail::new(&verts, hub)
                });
            }
        }
        fold_lines(&mut sails.values_mut().flatten().collect::<Vec<_>>(), mast);
        out_p.iter().zip(&out_sail).map(|(&q, &src)| groups[src].and_then(|(root, _)| sails[&root].as_ref()).map(|f| f.point(q))).collect::<Vec<_>>()
    });
    let (pleats, depth) = fan.map_or((0.0, 0.0), |f| (f.pleats, f.depth));
    Some((fine, Billow { rest: out_p, normal: out_n, pin, top: out_top, size, fan: fans.unwrap_or_default(), pleats, depth }))
}

/// The fan a sail makes: its pivot (where the ribs meet), plane and span.
struct FanSail {
    pivot: Vec3,
    e1: Vec3,
    e2: Vec3,
    n: Vec3,
    min: f32,
    max: f32,
    closed: f32,
    rim: f32,
    /// The line the fan closes onto, from the pivot: the mast for a main sail (`fold_lines`).
    target: Vec3,
    /// How far the hub moves as the fan shuts: a main sail's hub hangs aft of its mast on the yard, and a folded fan lies
    /// against the mast.
    to_mast: Vec3,
}

/// Half the width (mesh units across the ship) of the centre-line strip the mast is looked for in.
const MAST_HALF_WIDTH: f32 = 0.35;

/// Triangles that meet at one point for it to be the hub of a fan (the ribs' meeting point).
const HUB_MIN: usize = 8;

/// Fewest distinct vertices of a fan sail (the retail Navy fans have 89 to 119; the jib pieces 13 to 26).
const MIN_FAN_VERTS: usize = 40;

/// The fans of a sail mesh: for each vertex, the fan it belongs to (a representative vertex) and the fan's hub, or None for cloth
/// that is not a fan. The retail fans are cut into wedges that share only their hub position, so wedges are joined through it
/// (and through shared vertices); other coincident vertices do not join, so two fans that touch stay two.
fn fan_groups(pos: &[[f32; 3]], tris: &[[usize; 3]]) -> Vec<Option<(usize, Vec3)>> {
    let key = |i: usize| pos[i].map(|c| (c * 1000.0).round() as i64);
    let mut valence: std::collections::HashMap<[i64; 3], usize> = std::collections::HashMap::new();
    for t in tris {
        for &i in t {
            *valence.entry(key(i)).or_default() += 1;
        }
    }
    let mut parent: Vec<usize> = (0..pos.len()).collect();
    let mut first_at_hub: std::collections::HashMap<[i64; 3], usize> = std::collections::HashMap::new();
    for t in tris {
        join(&mut parent, t[0], t[1]);
        join(&mut parent, t[1], t[2]);
        for &i in t {
            if valence[&key(i)] >= HUB_MIN {
                let first = *first_at_hub.entry(key(i)).or_insert(i);
                join(&mut parent, i, first);
            }
        }
    }
    // each group's hub: the most-met position in it
    let mut best: std::collections::HashMap<usize, (usize, usize)> = std::collections::HashMap::new();
    for t in tris {
        for &i in t {
            let v = valence[&key(i)];
            if v >= HUB_MIN {
                let r = root(&mut parent, i);
                let e = best.entry(r).or_insert((i, v));
                if v > e.1 {
                    *e = (i, v);
                }
            }
        }
    }
    let mut out = vec![None; pos.len()];
    for t in tris {
        for &i in t {
            let r = root(&mut parent, i);
            out[i] = best.get(&r).map(|&(h, _)| (r, Vec3::from(pos[h])));
        }
    }
    out
}

impl FanSail {
    /// The fan's vertices (position, normal) and its hub -> the fan, or None for a sail with no area or length to fold. A Navy
    /// fan sail is a half disc: its straight edge (the yard under the main sail, the spar beside a side sail) is the diameter,
    /// the two vertices farthest apart, and the ribs meet at the hub in its middle. The fan closes from both sides onto its
    /// middle rib: the main sail folds up against the mast, the side sails close to a horizontal line.
    fn new(verts: &[(Vec3, Vec3)], hub: Vec3) -> Option<FanSail> {
        // a fan has many wedges; a handful of triangles about a point is a jib or a scrap: it keeps the roller fold
        if verts.len() < MIN_FAN_VERTS {
            return None;
        }
        let n = verts.iter().map(|v| v.1).sum::<Vec3>().normalize_or_zero();
        if n == Vec3::ZERO {
            return None;
        }
        let centre = verts.iter().map(|v| v.0).sum::<Vec3>() / verts.len() as f32;
        let flat = |q: Vec3| q - n * (q - centre).dot(n);
        let (mut a, mut b, mut far) = (centre, centre, 0.0);
        // the straight edge: the two vertices farthest apart in the sail's plane
        for (i, (p, _)) in verts.iter().enumerate() {
            for (q, _) in &verts[i + 1..] {
                let d = flat(*p).distance_squared(flat(*q));
                if d > far {
                    (a, b, far) = (*p, *q, d);
                }
            }
        }
        let pivot = hub;
        // the middle rib points from the pivot into the sail, square to the straight edge
        let edge = flat(b) - flat(a);
        let into = flat(centre) - flat(pivot);
        let e1 = (into - edge * into.dot(edge) / edge.length_squared().max(1e-8)).try_normalize()?;
        let e2 = n.cross(e1);
        let angle = |q: Vec3| (q - pivot).dot(e2).atan2((q - pivot).dot(e1));
        let (min, max) = verts.iter().filter(|(q, _)| (*q - pivot).length() > 1e-4).fold((f32::MAX, f32::MIN), |(lo, hi), (q, _)| (lo.min(angle(*q)), hi.max(angle(*q))));
        // a real fan is about a half disc about its hub; wider is cloth of another shape (jibs, pennants): it keeps the roller fold
        if max - min < 1e-3 || max - min > 190f32.to_radians() {
            return None;
        }
        let closed = 0.5 * (min + max);
        let rim = verts.iter().map(|(q, _)| (*q - pivot).length()).fold(1e-4, f32::max);
        let target = e1 * closed.cos() + e2 * closed.sin();
        Some(FanSail { pivot, e1, e2, n, min, max, closed, rim, target, to_mast: Vec3::ZERO })
    }

    /// The middle rib's direction from the pivot.
    fn middle(&self) -> Vec3 {
        self.e1 * self.closed.cos() + self.e2 * self.closed.sin()
    }

    fn point(&self, q: Vec3) -> FanPoint {
        let d = q - self.pivot;
        let radius = d.length();
        let angle = if radius > 1e-4 { d.dot(self.e2).atan2(d.dot(self.e1)) } else { self.closed };
        let shut = d.try_normalize().map_or(Quat::IDENTITY, |dir| Quat::from_rotation_arc(dir, self.target));
        FanPoint { pivot: self.pivot, n: self.n, span: ((angle - self.min) / (self.max - self.min)).clamp(0.0, 1.0), rim: radius / self.rim, rest: d, shut, to_mast: self.to_mast }
    }
}

/// A fan sail point folded `amount` (0 open, 1 shut) about its pivot: the angle closes onto the edge, the radius stays, and
/// the cloth between the ribs zig-zags out of the plane (`pleats` ribs, `depth` units at the rim) as it closes.
fn fanned(p: &FanPoint, amount: f32, pleats: f32, depth: f32) -> Vec3 {
    let wave = (p.span * pleats).fract();
    let tri = 4.0 * (wave - 0.5).abs() - 1.0;
    p.pivot + p.to_mast * amount + Quat::IDENTITY.slerp(p.shut, amount) * p.rest + p.n * (depth * amount * p.rim * tri)
}

/// Where the fans of one sail mesh close to. A main sail (middle rib pointing up) folds inward and upward onto its mast and
/// its hub slides onto it, so the shut fan lies against the mast; a fan hanging under the hull folds down along the same line,
/// and a side sail closes flat: its middle rib laid level. The mast is the straight line fitted (depth against height) through
/// `mast`, the centre-line points of the model's other parts beside the sail, over the main fans' heights; without enough of
/// them it is the line through the lowest and highest hub (straight up for one), and the hubs stay where they are.
fn fold_lines(fans: &mut [&mut FanSail], mast: &[Vec3]) {
    let mains: Vec<(Vec3, f32)> = fans.iter().filter(|f| f.middle().y > 0.5).map(|f| (f.pivot, f.rim)).collect();
    let line = mast_line(&mains, mast);
    let mut hubs: Vec<Vec3> = mains.iter().map(|m| m.0).collect();
    hubs.sort_by(|a, b| a.y.total_cmp(&b.y));
    let up = match (line, hubs.first(), hubs.last()) {
        (Some((_, dir)), ..) => dir,
        (None, Some(lo), Some(hi)) if hi.y - lo.y > 1e-2 => (*hi - *lo).normalize(),
        _ => Vec3::Y,
    };
    for f in fans.iter_mut() {
        let m = f.middle();
        (f.target, f.to_mast) = if m.y > 0.5 {
            // onto the mast at the hub's height; a hub far off any mast (a sail on a boom) stays put
            let on = line.map(|(at, dir)| at + dir * ((f.pivot.y - at.y) / dir.y) - f.pivot).filter(|d| d.length() < f.rim);
            (up, on.unwrap_or(Vec3::ZERO))
        } else if m.y < -0.5 {
            (-up, Vec3::ZERO)
        } else {
            (Vec3::new(m.x, 0.0, m.z).try_normalize().unwrap_or(m), Vec3::ZERO)
        };
    }
}

/// The mast through the main fans (hub, rim): a point on it and its upward direction, or None when no mast stands beside them. A
/// mast is a long, nearly upright spar of centre-line points (`mast`; a cylinder has vertices only at its ends) at one depth for
/// its height (raked a little), where a yard or a rail only crosses the centre line at about one height: so it is the line,
/// among slopes of up to 0.12 depth per height, whose points span the most height near the fans (at least half of theirs). A stay
/// running up to the masthead spans as much and more, so of the lines spanning nearly the most, the most upright one wins.
fn mast_line(mains: &[(Vec3, f32)], mast: &[Vec3]) -> Option<(Vec3, Vec3)> {
    const BIN: f32 = 0.3; // depth bins
    let lo = mains.iter().map(|m| m.0.y).fold(f32::MAX, f32::min);
    let hi = mains.iter().map(|m| m.0.y + m.1).fold(f32::MIN, f32::max);
    // the mast may run on well above and below the fans: any height, but fore and aft no farther than a fan reaches
    let near = |q: &&Vec3| mains.iter().any(|m| (q.z - m.0.z).abs() < m.1);
    let pts: Vec<&Vec3> = mast.iter().filter(near).collect();
    if pts.len() < 4 || hi <= lo {
        return None;
    }
    // the height a line's points span, counted only over the fans' heights
    let mut lines: Vec<(f32, f32, i32)> = vec![];
    for step in -6..=6 {
        let slope = step as f32 * 0.02;
        let mut bins: std::collections::HashMap<i32, (f32, f32)> = std::collections::HashMap::new();
        for q in &pts {
            let bin = ((q.z - slope * q.y) / BIN).round() as i32;
            let e = bins.entry(bin).or_insert((f32::MAX, f32::MIN));
            *e = (e.0.min(q.y), e.1.max(q.y));
        }
        for (bin, (a, b)) in bins {
            lines.push((b.min(hi) - a.max(lo), slope, bin));
        }
    }
    let most = lines.iter().map(|l| l.0).fold(f32::MIN, f32::max);
    let (span, slope, bin) = lines.into_iter().filter(|l| l.0 >= 0.8 * most).min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()).then(b.0.total_cmp(&a.0)))?;
    if span < 0.5 * (hi - lo) {
        return None;
    }
    let on: Vec<&&Vec3> = pts.iter().filter(|q| (((q.z - slope * q.y) / BIN).round() as i32) == bin).collect();
    let k = on.len() as f32;
    let (x, z0) = (on.iter().map(|q| q.x).sum::<f32>() / k, on.iter().map(|q| q.z - slope * q.y).sum::<f32>() / k);
    Some((Vec3::new(x, 0.0, z0), Vec3::new(0.0, 1.0, slope).normalize()))
}

/// Union-find over vertex indices (path halving).
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

/// Which sail each vertex belongs to (a representative vertex index): triangles joined by shared or coincident corners.
pub(crate) fn sail_of(pos: &[[f32; 3]], tris: &[[usize; 3]]) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..pos.len()).collect();
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

/// The most `belly` gives (full wind, both waves at their crest): the sail's bounds allow for it.
const BELLY_MAX: f32 = 0.65 + 0.25 + 0.12;

/// Each sail follows its ship's speed: the share of full speed (smoothed), a clock that runs faster with it, the sun.
fn sail_motion(
    time: Res<Time>,
    look: Res<Look>,
    lib: Option<Res<crate::combat::Library>>,
    ships: Query<&crate::Ship>,
    frame: Res<super::frame::Frame>,
    mut sails: Query<(&mut Sail, &MeshMaterial3d<SailMaterial>)>,
    mut materials: ResMut<Assets<SailMaterial>>,
) {
    let (Some(s), Some(lib)) = (&look.sails, lib) else { return };
    let sun = frame.sun().map_or(Vec3::NEG_Y, |(travel, _)| travel);
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
            m.extension.u.fold = sail.fold(s);
        }
    }
}

#[cfg(test)]
mod tests;
