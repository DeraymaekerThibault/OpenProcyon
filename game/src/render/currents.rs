//! Not retail: Etherium currents of the render profile (`currents` block, drawn with the mod's `shaders/current.wesl`). The retail
//! current is two additive strips scrolling a texture along the path (`environment.rs`, `CurrentRibbon`). This swaps those
//! strips' material for one whose shader draws a river of glowing gas in the manner of the profile's nebulae: wisps of the same
//! baked noise (`volnoise.rs`), drawn out along the flow and running downstream, in a few layers at different heights so the river
//! has depth, with soft edges and a little bloom where the gas is dense. The mesh, its UVs (u across, v along the path from its
//! first point to its last: the way the force pushes) and the retail scroll are kept, so the flow direction is the retail one.
//!
//! The gas also rolls: the strip is drawn from a denser copy of the retail mesh (`refine`: the same quads cut finer, so the footprint
//! is exactly the retail one) whose vertex stage lifts it into swells travelling downstream, with sharp crests and broad troughs and
//! normals from the displacement, so the crests catch the sun and the slopes facing away dim. Only up and down: the current's
//! outline, seen from above, does not move. The wave's height, length and speed are engine choices (no retail current rolls).
//!
//! Only reads `environment::CurrentRibbon`; nothing here changes what a current does.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology, VertexAttributeValues};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use serde_json::Value;

use super::volnoise::NoiseVolume;
use super::{Look, num, rgb};
use crate::environment::CurrentRibbon;
use crate::fx::EffectMaterial;

pub const SHADER: &str = "shaders/current.wesl";

/// The `currents` block.
#[derive(Clone, Debug, PartialEq)]
pub struct Currents {
    /// Overall strength; the core is past 1 so it blooms.
    pub brightness: f32,
    /// Wisps across the strip.
    pub streaks: f32,
    /// Strength of the glints that ride downstream.
    pub sparkle: f32,
    /// Extra light where the gas is dense.
    pub core: f32,
    /// How much of the retail texture stays under the streaks.
    pub texture: f32,
    pub color: Vec3,
    /// Strength of the slow pulses that run down the current (they make the direction readable at a glance).
    pub pulse: f32,
    /// Colour of dense gas (linear); the default is `color` half way to white.
    pub color_high: Vec3,
    /// How much gas the middle of the current always holds, whatever the noise says there, so the river never breaks (0 = the
    /// noise alone).
    pub fill: f32,
    /// Not retail: the rolling swells (engine choices), in world units. Crest height above the mean (0: a flat strip as before).
    pub wave_height: f32,
    /// Distance between the main swell's crests.
    pub wave_length: f32,
    /// How fast the main swell runs downstream (units a second).
    pub wave_speed: f32,
    /// How much the crests catch the sun (and the far slopes dim).
    pub crest: f32,
}

pub fn parse(v: &Value) -> Option<Currents> {
    let color = rgb(&v["color"]).unwrap_or(Vec3::new(0.45, 0.8, 1.0)).max(Vec3::ZERO);
    v.is_object().then(|| Currents {
        brightness: num(&v["brightness"], 1.4).max(0.0),
        streaks: num(&v["streaks"], 10.0).clamp(1.0, 60.0),
        sparkle: num(&v["sparkle"], 1.0).max(0.0),
        core: num(&v["core"], 1.5).max(0.0),
        texture: num(&v["texture"], 0.5).clamp(0.0, 1.0),
        color,
        pulse: num(&v["pulse"], 0.35).clamp(0.0, 1.0),
        color_high: rgb(&v["color_high"]).unwrap_or(color.lerp(Vec3::ONE, 0.5)).max(Vec3::ZERO),
        fill: num(&v["fill"], 0.1).clamp(0.0, 0.5),
        wave_height: num(&v["wave_height"], 6.0).clamp(0.0, 60.0),
        wave_length: num(&v["wave_length"], 110.0).max(1.0),
        wave_speed: num(&v["wave_speed"], 18.0),
        crest: num(&v["crest"], 0.8).max(0.0),
    })
}

/// What the current shader gets at `@group(MATERIAL_BIND_GROUP) @binding(100)`, besides the StandardMaterial bindings.
#[derive(Clone, Copy, Default, Debug, PartialEq, ShaderType)]
pub struct CurrentUniform {
    /// x: brightness, y: streaks across, z: sparkle, w: core
    pub look: Vec4,
    /// rgb: colour, w: share of the retail texture kept
    pub color: Vec4,
    /// x: this strip's retail scroll speed (texture heights a second), y: pulse strength, z: fill, w: world units per texture
    /// height along the strip (the clock is the frame data's)
    pub params: Vec4,
    /// rgb: colour of dense gas, w: the strip's width (world units)
    pub high: Vec4,
    /// x: wave height, y: wave length, z: wave speed, w: crest light
    pub wave: Vec4,
}

impl Currents {
    fn uniform(&self, speed: f32, strip: Strip) -> CurrentUniform {
        CurrentUniform {
            look: Vec4::new(self.brightness, self.streaks, self.sparkle, self.core),
            color: self.color.extend(self.texture),
            params: Vec4::new(speed, self.pulse, self.fill, strip.per_v),
            high: self.color_high.extend(strip.width),
            wave: Vec4::new(self.wave_height, self.wave_length, self.wave_speed, self.crest),
        }
    }
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct CurrentGlow {
    #[uniform(100)]
    pub u: CurrentUniform,
    /// The frame data (`frame.rs`) at `@binding(99)`.
    #[storage(99, read_only)]
    pub frame: super::frame::FrameBinding,
    /// The baked noise (`volnoise.rs`), the one the clouds are made of.
    #[texture(101, dimension = "3d")]
    #[sampler(102)]
    pub noise: Handle<Image>,
}

impl MaterialExtension for CurrentGlow {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    // the strip rolls into swells
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        super::kit::additive(descriptor);
        Ok(())
    }
}

pub type CurrentMaterial = ExtendedMaterial<StandardMaterial, CurrentGlow>;

pub fn plugin(app: &mut App, look: &Look) {
    if look.currents.is_some() {
        app.add_plugins(MaterialPlugin::<CurrentMaterial>::default()).add_systems(Update, restyle);
    }
}

/// The size of a retail strip, as the shader needs it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Strip {
    /// Full width across (world units).
    pub width: f32,
    /// World units per texture height along the path.
    pub per_v: f32,
}

/// Columns across the refined strip, and its rows' spacing as a share of its width (engine choices: fine enough for the shortest
/// swell to stay round), with a cap on its rows for a very long path.
const COLUMNS: usize = 16;
const ROW_STEP: f32 = 1.0 / 14.0;
const MAX_ROWS: usize = 6000;

/// A denser copy of a retail current strip (`environment::current_mesh_of`: a left and a right vertex per path point, u 0 on the
/// left, 1 on the right, v growing along the path): every retail quad cut into `COLUMNS` across and rows about `ROW_STEP` widths
/// apart, by bilinear interpolation of its corners, so every new vertex lies on the retail surface and the footprint, UVs and the
/// flow's direction stay exactly the retail ones. Tangents carry the direction of the flow (left-to-right turned a quarter) for the
/// shader's normals. `None` for a mesh that is not such a strip (it is then drawn as it is, flat).
pub fn refine(mesh: &Mesh) -> Option<(Mesh, Strip)> {
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return None };
    let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else { return None };
    if pos.len() < 4 || !pos.len().is_multiple_of(2) || uv.len() != pos.len() {
        return None;
    }
    let pos: Vec<Vec3> = pos.iter().map(|&p| Vec3::from(p)).collect();
    let uv: Vec<Vec2> = uv.iter().map(|&t| Vec2::from(t)).collect();
    let points = pos.len() / 2;
    if (0..points).any(|i| uv[2 * i].x != 0.0 || uv[2 * i + 1].x != 1.0 || uv[2 * i].y != uv[2 * i + 1].y) {
        return None;
    }
    let width = pos[0].distance(pos[1]);
    if width <= 1e-3 {
        return None;
    }
    let mid = |i: usize| (pos[2 * i] + pos[2 * i + 1]) / 2.0;
    let per_v = (1..points)
        .find_map(|i| {
            let dv = uv[2 * i].y - uv[2 * i - 2].y;
            (dv > 1e-6).then(|| mid(i).distance(mid(i - 1)) / dv)
        })
        .unwrap_or(width * 2.0);
    // the flow's direction at each path point: left-to-right turned a quarter (the retail side is (-d.z, 0, d.x))
    let along = |i: usize| {
        let side = pos[2 * i + 1] - pos[2 * i];
        Vec3::new(side.z, 0.0, -side.x).normalize_or(Vec3::NEG_Z)
    };
    let segment = |i: usize| (mid(i + 1).distance(mid(i)) / (width * ROW_STEP)).ceil().max(1.0) as usize;
    let total: usize = (0..points - 1).map(segment).sum();
    let thin = (total as f32 / MAX_ROWS as f32).max(1.0);
    let (mut p_out, mut uv_out, mut tan_out) = (vec![], vec![], vec![]);
    let mut row = |i: usize, t: f32| {
        let j = (i + 1).min(points - 1);
        let dir = along(i).lerp(along(j), t).normalize_or(along(i));
        let (left, right) = (pos[2 * i].lerp(pos[2 * j], t), pos[2 * i + 1].lerp(pos[2 * j + 1], t));
        let v = uv[2 * i].y + (uv[2 * j].y - uv[2 * i].y) * t;
        for c in 0..=COLUMNS {
            let s = c as f32 / COLUMNS as f32;
            p_out.push(left.lerp(right, s).to_array());
            uv_out.push([s, v]);
            tan_out.push([dir.x, dir.y, dir.z, 1.0]);
        }
    };
    for i in 0..points - 1 {
        let pieces = ((segment(i) as f32 / thin).ceil() as usize).max(1);
        for k in 0..pieces {
            row(i, k as f32 / pieces as f32);
        }
    }
    row(points - 1, 0.0);
    let rows = p_out.len() / (COLUMNS + 1);
    let stride = COLUMNS as u32 + 1;
    let mut indices = Vec::with_capacity((rows - 1) * COLUMNS * 6);
    for r in 0..rows as u32 - 1 {
        let (a, b) = (r * stride, (r + 1) * stride);
        for c in 0..COLUMNS as u32 {
            // the retail winding: (left, next left, right), (right, next left, next right)
            indices.extend([a + c, b + c, a + c + 1, a + c + 1, b + c, b + c + 1]);
        }
    }
    let count = p_out.len();
    let mut fine = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    fine.insert_attribute(Mesh::ATTRIBUTE_POSITION, p_out);
    fine.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count]);
    fine.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv_out);
    fine.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tan_out);
    fine.insert_indices(Indices::U32(indices));
    Some((fine, Strip { width, per_v }))
}

/// A strip's denser mesh, made once per retail mesh (a current's two strips share one) and the size of its strip.
type Refined = Option<(Handle<Mesh>, Strip)>;

/// Swaps the retail material of each current strip, once, and its mesh for the denser copy that rolls (`refine`). The shader
/// scrolls the retail texture itself with the frame clock (`frame.rs`), so the material is never touched again
/// (`environment::flow_currents` only scrolls its own).
fn restyle(
    mut commands: Commands,
    look: Res<Look>,
    noise: Option<Res<NoiseVolume>>,
    strips: Query<(Entity, &CurrentRibbon, &MeshMaterial3d<EffectMaterial>, &Mesh3d)>,
    retail: Res<Assets<EffectMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CurrentMaterial>>,
) {
    let (Some(c), Some(noise)) = (&look.currents, noise) else { return };
    let mut made: Vec<(AssetId<Mesh>, Refined)> = vec![];
    for (e, ribbon, mat, mesh) in &strips {
        let Some(old) = retail.get(&mat.0) else { continue };
        let refined = match made.iter().find(|(id, _)| *id == mesh.0.id()) {
            Some((_, r)) => r.clone(),
            None => {
                let r = meshes.get(&mesh.0).and_then(refine).map(|(m, strip)| (meshes.add(m), strip));
                made.push((mesh.0.id(), r.clone()));
                r
            }
        };
        let strip = refined.as_ref().map_or_else(Strip::default, |r| r.1);
        let base = StandardMaterial { uv_transform: default(), ..old.base.clone() };
        let handle = materials.add(CurrentMaterial { base, extension: CurrentGlow { u: c.uniform(ribbon.0, strip), frame: default(), noise: noise.0.clone() } });
        let mut entity = super::kit::swap::<EffectMaterial, _>(&mut commands, e, handle);
        if let Some((m, _)) = refined {
            entity.try_insert(Mesh3d(m));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn absent_without_the_key_and_clamped_with_it() {
        assert!(parse(&Value::Null).is_none());
        let c = parse(&json!({})).unwrap();
        assert!(c.brightness > 0.0 && c.texture <= 1.0);
        let c = parse(&json!({"streaks": 999, "texture": 4, "brightness": -1})).unwrap();
        assert_eq!((c.streaks, c.texture, c.brightness), (60.0, 1.0, 0.0));
        let strip = Strip { width: 120.0, per_v: 240.0 };
        assert_eq!(c.uniform(0.25, strip).params, Vec4::new(0.25, c.pulse, c.fill, 240.0));
        assert_eq!(c.uniform(0.25, strip).high.w, 120.0);
        assert_eq!(c.uniform(0.0, strip).wave, Vec4::new(c.wave_height, c.wave_length, c.wave_speed, c.crest));
        assert!(c.wave_height > 0.0, "the remaster current rolls unless the profile flattens it");
        assert_eq!(parse(&json!({"wave_height": -3, "wave_length": 0})).map(|c| (c.wave_height, c.wave_length)), Some((0.0, 1.0)));
        assert_eq!(c.color_high, c.color.lerp(Vec3::ONE, 0.5), "dense gas is the colour half way to white unless the profile names one");
        let c = parse(&json!({"color_high": [2, 0, 0], "fill": 9})).unwrap();
        assert_eq!((c.uniform(0.0, Strip::default()).high, c.fill), (Vec4::new(2.0, 0.0, 0.0, 0.0), 0.5));
    }

    fn attr2(m: &Mesh, a: bevy::mesh::MeshVertexAttribute) -> Vec<Vec2> {
        match m.attribute(a) {
            Some(VertexAttributeValues::Float32x2(v)) => v.iter().map(|&t| Vec2::from(t)).collect(),
            _ => vec![],
        }
    }
    fn attr3(m: &Mesh, a: bevy::mesh::MeshVertexAttribute) -> Vec<Vec3> {
        match m.attribute(a) {
            Some(VertexAttributeValues::Float32x3(v)) => v.iter().map(|&t| Vec3::from(t)).collect(),
            Some(VertexAttributeValues::Float32x4(v)) => v.iter().map(|&t| Vec4::from(t).truncate()).collect(),
            _ => vec![],
        }
    }

    #[test]
    fn the_rolling_strip_keeps_the_retail_footprint_uvs_and_flow() {
        // a bent path running -z then +x, at a height
        let path = [Vec3::new(0.0, 5.0, 0.0), Vec3::new(0.0, 5.0, -300.0), Vec3::new(200.0, 5.0, -500.0)];
        let retail = crate::environment::current_mesh_of(&path, 60.0);
        let (fine, strip) = refine(&retail).unwrap();
        assert_eq!(strip, Strip { width: 120.0, per_v: 240.0 });
        let (rp, ruv) = (attr3(&retail, Mesh::ATTRIBUTE_POSITION), attr2(&retail, Mesh::ATTRIBUTE_UV_0));
        let (fp, fuv, ft) = (attr3(&fine, Mesh::ATTRIBUTE_POSITION), attr2(&fine, Mesh::ATTRIBUTE_UV_0), attr3(&fine, Mesh::ATTRIBUTE_TANGENT));
        let rows = fp.len() / (COLUMNS + 1);
        assert!(rows > 40, "fine enough along to round the swells: {rows} rows");
        // the strip's corners at every path point are the retail ones, with the retail UVs
        for (i, (p, t)) in rp.iter().zip(&ruv).enumerate() {
            let k = fp.iter().position(|q| q.distance(*p) < 1e-3).unwrap_or_else(|| panic!("retail vertex {i} kept"));
            assert!(fuv[k].distance(*t) < 1e-5);
        }
        // every vertex is on the retail strip: at its height, inside its lane, UVs in its range
        let v_end = ruv.last().unwrap().y;
        for (p, t) in fp.iter().zip(&fuv) {
            assert!((p.y - 5.0).abs() < 1e-4 && (0.0..=1.0).contains(&t.x) && (0.0..=v_end + 1e-5).contains(&t.y));
            let centre = Vec3::new(p.x, 0.0, p.z);
            let lane = path.windows(2).map(|w| {
                let (a, b) = (w[0].with_y(0.0), w[1].with_y(0.0));
                let q = (centre - a).dot(b - a) / (b - a).length_squared();
                centre.distance(a + (b - a) * q.clamp(0.0, 1.0))
            });
            assert!(lane.fold(f32::MAX, f32::min) <= 60.0 * 1.5, "near the lane: {p}");
        }
        // the tangent runs the way the force pushes: -z on the first leg, toward +x on the last
        assert!(ft[0].distance(Vec3::NEG_Z) < 1e-4);
        assert!(ft.last().unwrap().x > 0.5);
        // v grows downstream along the rows
        assert!((1..rows).all(|r| fuv[r * (COLUMNS + 1)].y > fuv[(r - 1) * (COLUMNS + 1)].y));
        assert!(refine(&Mesh::from(Cuboid::default())).is_none(), "anything else is drawn as it is");
    }

    #[test]
    fn a_strip_swaps_its_material_once_and_its_mesh_for_the_rolling_one() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Shader>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Mesh>()
            .init_asset::<EffectMaterial>()
            .init_asset::<CurrentMaterial>()
            .init_asset::<Image>()
            .insert_resource(Look::parse(&json!({"currents": {}})))
            .add_systems(Startup, crate::render::volnoise::bake)
            .add_systems(Update, restyle);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(crate::environment::current_mesh_of(&[Vec3::ZERO, Vec3::new(0.0, 0.0, -400.0)], 60.0));
        let retail = app.world_mut().resource_mut::<Assets<EffectMaterial>>().add(crate::fx::additive_material(Handle::default()));
        let strip = app.world_mut().spawn((CurrentRibbon(0.25), Mesh3d(mesh.clone()), MeshMaterial3d(retail.clone()))).id();
        let other = app.world_mut().spawn((CurrentRibbon(0.15), Mesh3d(mesh.clone()), MeshMaterial3d(retail))).id();
        app.update();
        app.update();
        let w = app.world();
        assert!(w.entity(strip).contains::<MeshMaterial3d<CurrentMaterial>>() && !w.entity(strip).contains::<MeshMaterial3d<EffectMaterial>>());
        let fine = w.entity(strip).get::<Mesh3d>().unwrap().0.id();
        assert_ne!(fine, mesh.id(), "drawn from the denser copy");
        assert_eq!(w.entity(other).get::<Mesh3d>().map(|m| m.0.id()), Some(fine), "the two strips share it");
        assert_eq!(w.resource::<Assets<CurrentMaterial>>().len(), 2, "once per strip");
        let m = w.entity(strip).get::<MeshMaterial3d<CurrentMaterial>>().and_then(|h| w.resource::<Assets<CurrentMaterial>>().get(&h.0)).unwrap();
        assert_eq!(m.extension.u.high.w, 120.0, "the shader knows the strip's width");
        assert_eq!(m.base.uv_transform, default(), "no scroll in the material: the shader scrolls by the frame clock");
        assert_eq!(m.extension.u.params.x, 0.25, "at the strip's own retail speed");
        assert_eq!(m.extension.noise, w.resource::<NoiseVolume>().0, "the gas is the clouds' baked noise");
    }
}
