//! Not retail: sails of the render profile (part of render/mod.rs's look). glTF materials whose name contains one of
//! `sails.match` get a material of their own: the retail StandardMaterial (made solid with `opaque`) extended by the shader
//! `shaders/sail.wesl`, which the mod ships. The engine feeds it the profile's numbers, the sun, the sail's UV
//! stretch and its ship's speed (see `SailGlowUniform` for the contract); with `billow` the mesh is cut finer and bellies
//! out between its edges, more with speed; with `furl` each sail folds up to its top edge (the yard) while its ship is
//! ordered to All Stop, as the film's solar sails fold away. With `fan` a Navy sail does not fold like a roller blind but
//! like a hand fan: its ribs swing in from both sides onto the middle rib, about the middle of its straight edge, and the cloth
//! between them pleats.

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
    /// For a fan sail, each vertex's place in its fan (None where the sail is too small to have one); empty otherwise.
    fan: Vec<Option<FanPoint>>,
    pleats: f32,
    depth: f32,
}

/// One vertex of a fan sail: where it sits about the fan's pivot, in the sail's plane.
#[derive(Clone, Copy, Debug)]
struct FanPoint {
    pivot: Vec3,
    /// The plane's axes (e1 points from the pivot into the sail) and normal.
    e1: Vec3,
    e2: Vec3,
    n: Vec3,
    /// Angle about the pivot from e1, the angle the fan closes onto (its middle rib), and this point's place across the span (0..1).
    angle: f32,
    closed: f32,
    span: f32,
    radius: f32,
    /// This radius over the sail's longest rib (pleats are deepest at the rim).
    rim: f32,
    /// Offset out of the plane at rest (kept).
    height: f32,
}

/// Materials named like retail sail textures ("sail.tga", "square sails orange.tga") become sail materials, one per sail
/// mesh so each follows its own ship's speed.
pub(super) fn sails(
    mut commands: Commands,
    look: Res<Look>,
    new: Query<(Entity, &GltfMaterialName, &MeshMaterial3d<StandardMaterial>, &Mesh3d), (Added<MeshMaterial3d<StandardMaterial>>, Without<Restyled<SailMaterial>>)>,
    parents: Query<&ChildOf>,
    ships: Query<&crate::Ship>,
    lib: Option<Res<crate::combat::Library>>,
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
            && let Some((fine, billow)) = meshes.get(&mesh.0).and_then(|m| {
                let race = ship.and_then(|e| ships.get(e).ok()).and_then(|sh| lib.as_ref().map(|l| l.classes[sh.class].race.as_str()));
                subdivide(m, s.fan.as_ref().filter(|f| f.applies(name, race)))
            })
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
fn subdivide(mesh: &Mesh, fan: Option<&Fan>) -> Option<(Mesh, Billow)> {
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
}

/// Triangles that meet at one point for it to be the hub of a fan (the ribs' meeting point).
const HUB_MIN: usize = 8;

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
        if verts.len() < 3 {
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
        Some(FanSail { pivot, e1, e2, n, min, max, closed, rim })
    }

    fn point(&self, q: Vec3) -> FanPoint {
        let d = q - self.pivot;
        let radius = d.dot(self.e1).hypot(d.dot(self.e2));
        let angle = if radius > 1e-4 { d.dot(self.e2).atan2(d.dot(self.e1)) } else { self.closed };
        FanPoint {
            pivot: self.pivot,
            e1: self.e1,
            e2: self.e2,
            n: self.n,
            angle,
            closed: self.closed,
            span: ((angle - self.min) / (self.max - self.min)).clamp(0.0, 1.0),
            radius,
            rim: radius / self.rim,
            height: d.dot(self.n),
        }
    }
}

/// A fan sail point folded `amount` (0 open, 1 shut) about its pivot: the angle closes onto the edge, the radius stays, and
/// the cloth between the ribs zig-zags out of the plane (`pleats` ribs, `depth` units at the rim) as it closes.
fn fanned(p: &FanPoint, amount: f32, pleats: f32, depth: f32) -> Vec3 {
    let angle = p.closed + (p.angle - p.closed) * (1.0 - amount);
    let wave = (p.span * pleats).fract();
    let tri = 4.0 * (wave - 0.5).abs() - 1.0;
    let out = p.height + depth * amount * p.rim * tri;
    p.pivot + p.e1 * (p.radius * angle.cos()) + p.e2 * (p.radius * angle.sin()) + p.n * out
}

/// Which sail each vertex belongs to (a representative vertex index): triangles joined by shared or coincident corners.
pub(crate) fn sail_of(pos: &[[f32; 3]], tris: &[[usize; 3]]) -> Vec<usize> {
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
                let billow = b.normal[i] * amp * b.pin[i] * belly(b.rest[i], b.size, sail.phase, sail.clock, sail.wind);
                match b.fan.get(i).copied().flatten() {
                    Some(pt) => (fanned(&pt, s.furl * f, b.pleats, b.depth * b.size) + billow).to_array(),
                    None => furled(b.rest[i] + billow, b.top[i], s.furl * f).to_array(),
                }
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
        let (fine, b) = subdivide(&quad(10.0, 10.0), None).unwrap();
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
        let (_, b) = subdivide(&two, None).unwrap();
        for (p, top) in b.rest.iter().zip(&b.top) {
            assert_eq!(*top, if p.y >= 20.0 { 25.0 } else { 10.0 }, "{p}");
            let f = furled(*p, *top, 0.9);
            assert!((top - f.y - 0.1 * (top - p.y)).abs() < 1e-4 && f.x == p.x && f.z == p.z, "{p} -> {f}");
        }
        assert_eq!(parse(&serde_json::json!({})).unwrap().furl, 0.0, "classic unless asked");
    }

    /// A half disc of radius 10 standing on its straight edge (the yard, along x), ribs every 22.5 degrees from the centre.
    fn fan_mesh() -> Mesh {
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        let rib = |k: usize| {
            let a = std::f32::consts::PI * k as f32 / 8.0;
            [10.0 * a.cos(), 10.0 * a.sin(), 0.0]
        };
        let mut pos = vec![[0.0, 0.0, 0.0]];
        pos.extend((0..=8).map(rib));
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 10]);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, (0..10).map(|i| [i as f32 / 10.0, 0.5 * (i % 2) as f32]).collect::<Vec<_>>());
        m.insert_indices(Indices::U32((1..9).flat_map(|k| [0, k, k + 1]).collect()));
        m
    }

    #[test]
    fn a_fan_sail_folds_from_both_sides_onto_its_middle_rib_keeping_every_ribs_length() {
        let fan = Fan { matching: vec![], races: vec![], pleats: 4.0, depth: 0.0 };
        let (_, b) = subdivide(&fan_mesh(), Some(&fan)).unwrap();
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
    fn a_side_fan_standing_on_a_vertical_edge_closes_to_horizontal() {
        // the same half disc turned so its straight edge is upright and it opens outward along +x
        let mut m = fan_mesh();
        if let Some(VertexAttributeValues::Float32x3(pos)) = m.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
            pos.iter_mut().for_each(|p| *p = [p[1], -p[0], 0.0]);
        }
        let fan = Fan { matching: vec![], races: vec![], pleats: 4.0, depth: 0.0 };
        let (_, b) = subdivide(&m, Some(&fan)).unwrap();
        for pt in b.fan.iter().flatten() {
            let shut = fanned(pt, 1.0, 4.0, 0.0);
            assert!(shut.y.abs() < 1e-3 && shut.x >= -1e-3, "closed flat along the spar: {shut}");
        }
    }

    #[test]
    fn a_fan_pleats_out_of_its_plane_only_while_it_is_closing() {
        let fan = Fan { matching: vec![], races: vec![], pleats: 4.0, depth: 0.1 };
        let (_, b) = subdivide(&fan_mesh(), Some(&fan)).unwrap();
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
        let (_, b) = subdivide(&quad(10.0, 10.0), None).unwrap();
        assert!(b.fan.is_empty());
    }

    /// Every sail primitive (material name contains "sail") of a ship GLB: positions, normals and triangles.
    fn glb_sails(bytes: &[u8]) -> Vec<(String, Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[usize; 3]>)> {
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
        let f3 = |b: Vec<u8>| -> Vec<[f32; 3]> { b.chunks_exact(12).map(|c| std::array::from_fn(|i| f32::from_le_bytes(c[4 * i..4 * i + 4].try_into().unwrap()))).collect() };
        let mut out = vec![];
        for mesh in doc["meshes"].as_array().unwrap() {
            for p in mesh["primitives"].as_array().unwrap() {
                let name = doc["materials"][p["material"].as_u64().unwrap_or(0) as usize]["name"].as_str().unwrap_or("").to_string();
                if !name.to_lowercase().contains("sail") {
                    continue;
                }
                let pos = f3(read(p["attributes"]["POSITION"].as_u64().unwrap() as usize, 12));
                let nor = f3(read(p["attributes"]["NORMAL"].as_u64().unwrap() as usize, 12));
                let ia = p["indices"].as_u64().unwrap() as usize;
                let wide = doc["accessors"][ia]["componentType"].as_u64() == Some(5125);
                let raw = read(ia, if wide { 4 } else { 2 });
                let idx: Vec<usize> = if wide {
                    raw.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap()) as usize).collect()
                } else {
                    raw.chunks_exact(2).map(|c| u16::from_le_bytes(c.try_into().unwrap()) as usize).collect()
                };
                out.push((name, pos, nor, idx.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect()));
            }
        }
        out
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn every_navy_sail_is_one_connected_fan() {
        let (mut fans, mut main_fans) = (0, 0);
        for dir in std::fs::read_dir(crate::assets_dir().join("ships")).unwrap().flatten() {
            let ship: Value = crate::data::read_json(&dir.path().join("ship.json"));
            if ship["race"] != "Navy" {
                continue;
            }
            let Ok(bytes) = std::fs::read(dir.path().join("model.glb")) else { continue };
            for (name, pos, nor, tris) in glb_sails(&bytes) {
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
                for (k, (hub, vs)) in &by {
                    match FanSail::new(vs, *hub) {
                        Some(f) => {
                            fans += 1;
                            main_fans += usize::from(vs.len() >= 80);
                            println!("   #{k}: {} verts, hub {:.2?}, span {:.0}..{:.0} deg, rim {:.2}", vs.len(), f.pivot, f.min.to_degrees(), f.max.to_degrees(), f.rim);
                        }
                        None => println!("   #{k}: {} verts, not a half disc: roller-blind fold", vs.len()),
                    }
                }
            }
        }
        assert!(fans > 20 && main_fans > 20, "{fans} fans, {main_fans} big ones");
    }

    #[test]
    fn uv_scale_measures_world_length_per_uv_unit() {
        // 20 wide along u, 5 tall along v, UV 0..1 both ways: a stretched texture
        let s = uv_world_scale(&quad(20.0, 5.0)).unwrap();
        assert!((s - Vec2::new(20.0, 5.0)).length() < 1e-3, "{s}");
    }
}
