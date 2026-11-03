//! Not retail: engine plumes and wakes of the render profile (part of render/mod.rs's look). Each jet engine (an exhaust
//! whose effect name contains `engines.effects`) gets one cone per `engines.layers` entry trailing astern, drawn additively
//! with the mod's `shaders/plume.wesl`; their length and brightness follow the exhaust's thrust, and the retail flame sprites can be
//! thinned (`combat::ExhaustParticles`). Wakes keep their retail motion (wake.rs): the retail ribbon is no longer drawn but
//! rebuilt each frame as one ribbon per `wakes.planes` tilt along the same path, drawn with the mod's `shaders/wake.wesl`.
//! The shaders' uniforms are `PlumeUniform` and `WakeUniform`.

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology, VertexAttributeValues};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use serde_json::Value;
use std::collections::HashMap;

use super::{Look, num, rgb};
use crate::fx::{EffectMaterial, Fx};

/// The shaders' asset paths; the mod ships them.
pub const PLUME_SHADER: &str = "shaders/plume.wesl";
pub const WAKE_SHADER: &str = "shaders/wake.wesl";

/// The engine plumes' look.
#[derive(Clone, Debug, PartialEq)]
pub struct Engines {
    /// Exhausts whose effect name contains this are jets and get a plume (Procyon funnels' `SmokeStack_Smoke` does not).
    pub effects: String,
    /// Outer flame colour per fleet type (`race`), linear; others get `default`.
    pub colors: HashMap<String, Vec3>,
    pub default: Vec3,
    /// White-hot core colour.
    pub core: Vec3,
    /// Plume length and radius at full thrust, as shares of the ship's length.
    pub length: f32,
    pub width: f32,
    /// The cones drawn per engine, each a share of that size; `inner` tells the shader which is the core.
    pub layers: Vec<Layer>,
    pub brightness: f32,
    /// Share of the retail flame sprites kept.
    pub particles: f32,
}

/// The wake ribbons' look.
#[derive(Clone, Debug, PartialEq)]
pub struct Wakes {
    pub brightness: f32,
    /// Colour near the ship, cooling into the wake's tint as it ages.
    pub hot: Vec3,
    /// Strength of the bright thread down the middle.
    pub core: f32,
    /// Tilt of each ribbon about the trail, radians (the profile gives degrees); 0 is the flat retail ribbon.
    pub planes: Vec<f32>,
    /// Squashes the tilted ribbons' height (1 = their true angle).
    pub height: f32,
}

/// One plume cone: its share of the plume's width and length, and whether it is the inner core.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    pub inner: bool,
    pub width: f32,
    pub length: f32,
}

pub fn parse_engines(v: &Value) -> Option<Engines> {
    v.is_object().then(|| Engines {
        effects: v["effects"].as_str().unwrap_or("Engine").to_string(),
        colors: v["colors"].as_object().into_iter().flatten().filter_map(|(k, c)| Some((k.clone(), rgb(c)?))).collect(),
        default: rgb(&v["default"]).unwrap_or(Vec3::new(1.0, 0.6, 0.3)),
        core: rgb(&v["core"]).unwrap_or(Vec3::new(1.0, 0.95, 0.85)),
        length: num(&v["length"], 0.45),
        width: num(&v["width"], 0.05),
        layers: match v["layers"].as_array() {
            Some(a) => a.iter().map(|l| Layer { inner: l["inner"].as_bool().unwrap_or(false), width: num(&l["width"], 1.0), length: num(&l["length"], 1.0) }).collect(),
            None => vec![Layer { inner: false, width: 1.0, length: 1.0 }, Layer { inner: true, width: 0.42, length: 0.5 }],
        },
        brightness: num(&v["brightness"], 3.0),
        particles: num(&v["particles"], 1.0),
    })
}

pub fn parse_wakes(v: &Value) -> Option<Wakes> {
    v.is_object().then(|| Wakes {
        brightness: num(&v["brightness"], 1.0),
        hot: rgb(&v["hot"]).unwrap_or(Vec3::new(0.9, 0.95, 1.0)),
        core: num(&v["core"], 0.8),
        planes: match v["planes"].as_array() {
            Some(a) => a.iter().filter_map(|d| Some((d.as_f64()? as f32).to_radians())).collect(),
            None => [0.0f32, 60.0, -60.0].map(f32::to_radians).to_vec(),
        },
        height: num(&v["height"], 0.58),
    })
}

/// ONE/ONE blending like the retail additive effects (fx::RetailAdd): the shaders write unpremultiplied light.
fn additive(descriptor: &mut RenderPipelineDescriptor) {
    let component = BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::One, operation: BlendOperation::Add };
    if let Some(fragment) = descriptor.fragment.as_mut() {
        for target in fragment.targets.iter_mut().flatten() {
            target.blend = Some(BlendState { color: component, alpha: component });
        }
    }
    if let Some(depth) = descriptor.depth_stencil.as_mut() {
        depth.bias.constant = 0;
    }
}

/// Unlit, two-sided, drawn after the scene's blended meshes like the retail additive effects.
fn additive_base(texture: Option<Handle<Image>>) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: texture,
        unlit: true,
        cull_mode: None,
        double_sided: true,
        alpha_mode: AlphaMode::Blend,
        depth_bias: crate::fx::ADDITIVE_SORT_BIAS,
        ..default()
    }
}

#[derive(Clone, Copy, Default, Debug, ShaderType)]
pub struct PlumeUniform {
    pub core: Vec4,
    pub edge: Vec4,
    /// x: brightness, y: thrust, z: clock, w: seed
    pub params: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct PlumeGlow {
    #[uniform(100)]
    pub u: PlumeUniform,
}

impl MaterialExtension for PlumeGlow {
    fn fragment_shader() -> ShaderRef {
        PLUME_SHADER.into()
    }
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        additive(descriptor);
        Ok(())
    }
}

pub type PlumeMaterial = ExtendedMaterial<StandardMaterial, PlumeGlow>;

#[derive(Clone, Copy, Default, Debug, ShaderType)]
pub struct WakeUniform {
    /// rgb: the wake's tint, a: brightness
    pub tint: Vec4,
    pub hot: Vec4,
    /// x: v at the ship end, y: clock, z: core strength
    pub params: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct WakeGlow {
    #[uniform(100)]
    pub u: WakeUniform,
}

impl MaterialExtension for WakeGlow {
    fn fragment_shader() -> ShaderRef {
        WAKE_SHADER.into()
    }
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        additive(descriptor);
        Ok(())
    }
}

pub type WakeMaterial = ExtendedMaterial<StandardMaterial, WakeGlow>;

pub fn plugin(app: &mut App, look: &Look) {
    if let Some(e) = &look.engines {
        app.insert_resource(crate::combat::ExhaustParticles { scale: e.particles, matching: e.effects.clone() })
            .add_plugins(MaterialPlugin::<PlumeMaterial>::default())
            .add_systems(Update, (plumes, plume_motion).chain());
    }
    if look.wakes.is_some() {
        app.add_plugins(MaterialPlugin::<WakeMaterial>::default()).add_systems(Update, (wakes, wake_tubes).chain());
    }
}

/// One engine's plume: its size at full thrust and the smoothed thrust that scales it.
#[derive(Component)]
struct Plume {
    length: f32,
    width: f32,
    thrust: f32,
    clock: f32,
    seed: f32,
}

const RINGS: usize = 16;
const AROUND: usize = 20;

/// A unit plume along +Z (astern in the ship frame), base at the origin: it swells just past the nozzle, then tapers.
fn plume_mesh() -> Mesh {
    let (mut pos, mut nor, mut uv) = (vec![], vec![], vec![]);
    for i in 0..=RINGS {
        let v = i as f32 / RINGS as f32;
        let r = (0.5 + 0.5 * (v / 0.15).min(1.0)) * (1.0 - 0.75 * v);
        for j in 0..=AROUND {
            let a = j as f32 / AROUND as f32 * std::f32::consts::TAU;
            pos.push([r * a.cos(), r * a.sin(), v]);
            nor.push([a.cos(), a.sin(), 0.0]);
            uv.push([j as f32 / AROUND as f32, v]);
        }
    }
    let row = (AROUND + 1) as u32;
    let idx: Vec<u32> = (0..RINGS as u32)
        .flat_map(|i| {
            (0..AROUND as u32).flat_map(move |j| {
                let a = i * row + j;
                [a, a + row, a + 1, a + 1, a + row, a + row + 1]
            })
        })
        .collect();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nor);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    m.insert_indices(Indices::U32(idx));
    m
}

fn plumes(
    mut commands: Commands,
    look: Res<Look>,
    lib: Option<Res<crate::combat::Library>>,
    new: Query<(Entity, &crate::combat::Engine, &ChildOf, &Transform), Added<crate::combat::Engine>>,
    ships: Query<&crate::Ship>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PlumeMaterial>>,
    mut mesh: Local<Option<Handle<Mesh>>>,
) {
    let (Some(e), Some(lib)) = (&look.engines, lib) else { return };
    for (engine, exhaust, parent, local) in &new {
        let Ok(ship) = ships.get(parent.parent()) else { continue };
        if !exhaust.effect().contains(e.effects.as_str()) {
            continue;
        }
        let class = &lib.classes[ship.class];
        let edge = e.colors.get(&class.race).copied().unwrap_or(e.default);
        let seed = (engine.index().index() as f32 * 0.618).fract() * 10.0;
        let mesh = mesh.get_or_insert_with(|| meshes.add(plume_mesh())).clone();
        for &Layer { inner, width: w, length: l } in &e.layers {
            let material = materials.add(PlumeMaterial {
                base: additive_base(None),
                extension: PlumeGlow {
                    u: PlumeUniform { core: e.core.extend(0.0), edge: edge.extend(if inner { 1.0 } else { 0.0 }), params: Vec4::new(e.brightness, 0.0, 0.0, seed + l) },
                },
            });
            // the engine's own rotation is undone, so the plume trails straight astern of the ship
            let plume = (
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                Transform { rotation: local.rotation.inverse(), scale: Vec3::ZERO, ..default() },
                Plume { length: class.length * e.length * l, width: class.length * e.width * w, thrust: 0.0, clock: 0.0, seed },
                NotShadowCaster,
            );
            commands.entity(engine).queue_silenced(move |mut fx: EntityWorldMut| {
                fx.with_child(plume);
            });
        }
    }
}

/// The plume's scale at a thrust: longer and a little wider the harder the engine pushes, its length flickering a little.
fn plume_scale(p: &Plume, thrust: f32) -> Vec3 {
    let w = p.width * (0.65 + 0.35 * thrust);
    let t = p.clock;
    let flicker = 1.0 + 0.06 * (23.0 * t + p.seed).sin() * (13.7 * t + 2.0 * p.seed).sin();
    Vec3::new(w, w, p.length * (0.2 + 0.8 * thrust) * flicker)
}

fn plume_motion(
    time: Res<Time>,
    engines: Query<(&crate::combat::Engine, &Fx)>,
    mut plumes: Query<(&mut Plume, &ChildOf, &mut Transform, &mut Visibility, &MeshMaterial3d<PlumeMaterial>)>,
    mut materials: ResMut<Assets<PlumeMaterial>>,
) {
    let dt = time.delta_secs();
    for (mut p, parent, mut tf, mut vis, mat) in &mut plumes {
        let target = engines.get(parent.parent()).map_or(0.0, |(e, fx)| if fx.emitting { e.thrust() } else { 0.0 });
        p.thrust += (target - p.thrust) * (dt * 2.5).min(1.0);
        p.clock += dt;
        tf.scale = plume_scale(&p, p.thrust);
        vis.set_if_neq(if p.thrust < 0.02 { Visibility::Hidden } else { Visibility::Inherited });
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.extension.u.params.y = p.thrust;
            m.extension.u.params.z = p.clock;
        }
    }
}

/// The tube drawn in place of a wake's retail ribbon, rebuilt from that ribbon every frame.
#[derive(Component)]
struct WakeTube {
    wake: Entity,
    ribbon: Handle<Mesh>,
}

/// A new wake stops drawing its retail ribbon (wake.rs keeps updating it) and gets crossed ribbons with the wake material,
/// keeping the retail texture and the faction tint.
fn wakes(
    mut commands: Commands,
    look: Res<Look>,
    lib: Option<Res<crate::combat::Library>>,
    new: Query<(Entity, &crate::wake::Wake, &Mesh3d, &MeshMaterial3d<EffectMaterial>), Added<crate::wake::Wake>>,
    ships: Query<&crate::Ship>,
    effect: Res<Assets<EffectMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<WakeMaterial>>,
) {
    let (Some(w), Some(lib)) = (&look.wakes, lib) else { return };
    for (e, wake, ribbon, old) in &new {
        let race = ships.get(wake.ship()).map(|s| lib.classes[s.class].race.clone()).unwrap_or_default();
        let tint = crate::wake::wake_tint(&race).map_or(Vec3::ONE, Vec3::from);
        let texture = effect.get(&old.0).and_then(|m| m.base.base_color_texture.clone());
        let material = materials.add(WakeMaterial {
            base: additive_base(texture),
            extension: WakeGlow { u: WakeUniform { tint: tint.extend(w.brightness), hot: w.hot.extend(0.0), params: Vec4::new(1.0, 0.0, w.core, 0.0) } },
        });
        commands.entity(e).try_remove::<MeshMaterial3d<EffectMaterial>>();
        let tube = meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
        commands.spawn((
            crate::Scenario,
            WakeTube { wake: e, ribbon: ribbon.0.clone() },
            Mesh3d(tube),
            MeshMaterial3d(material),
            Transform::default(),
            Visibility::default(),
            bevy::camera::visibility::NoFrustumCulling,
            NotShadowCaster,
        ));
    }
}

/// The ribbon as three crossed ribbons along the same path: each segment's (a, b) edge pair, tilted about the trail by each
/// of `planes` (radians; their height squashed by `height`), keeping the ribbon's u across, v and fade. None
/// for a ribbon with fewer than two segments.
fn crossed_ribbons(
    pos: &[[f32; 3]],
    uv: &[[f32; 2]],
    col: &[[f32; 4]],
    planes: &[f32],
    height: f32,
) -> Option<(Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[f32; 4]>, Vec<u32>)> {
    let n = pos.len() / 2;
    if n < 2 || uv.len() < 2 * n || col.len() < 2 * n {
        return None;
    }
    let centre = |i: usize| (Vec3::from(pos[2 * i]) + Vec3::from(pos[2 * i + 1])) * 0.5;
    let (mut p, mut nor, mut tuv, mut tcol, mut idx) = (vec![], vec![], vec![], vec![], vec![]);
    for &tilt in planes {
        let first = p.len() as u32;
        for i in 0..n {
            let (a, b) = (Vec3::from(pos[2 * i]), Vec3::from(pos[2 * i + 1]));
            let c = centre(i);
            let half = (a - b) * 0.5;
            let along = (centre((i + 1).min(n - 1)) - centre(i.saturating_sub(1))).normalize_or(Vec3::Z);
            let up = half.cross(along).normalize_or(Vec3::Y) * half.length();
            let side = half * tilt.cos() + up * (tilt.sin() * height);
            let normal = side.cross(along).normalize_or(Vec3::Y);
            for (end, u) in [(1.0, uv[2 * i][0]), (-1.0, uv[2 * i + 1][0])] {
                p.push((c + side * end).to_array());
                nor.push(normal.to_array());
                tuv.push([u, uv[2 * i][1]]);
                tcol.push(col[2 * i]);
            }
        }
        for i in 0..(n - 1) as u32 {
            let a = first + 2 * i;
            idx.extend([a, a + 1, a + 2, a + 1, a + 3, a + 2]);
        }
    }
    Some((p, nor, tuv, tcol, idx))
}

/// Rebuilds each tube from its wake's ribbon; drops the tube with its wake.
fn wake_tubes(
    mut commands: Commands,
    time: Res<Time>,
    look: Res<Look>,
    tubes: Query<(Entity, &WakeTube, &Mesh3d, &MeshMaterial3d<WakeMaterial>)>,
    wakes: Query<(), With<crate::wake::Wake>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<WakeMaterial>>,
) {
    let Some(w) = &look.wakes else { return };
    for (e, tube, mesh, mat) in &tubes {
        if !wakes.contains(tube.wake) {
            commands.entity(e).despawn();
            continue;
        }
        let built = meshes.get(&tube.ribbon).and_then(|r| match (r.attribute(Mesh::ATTRIBUTE_POSITION), r.attribute(Mesh::ATTRIBUTE_UV_0), r.attribute(Mesh::ATTRIBUTE_COLOR)) {
            (Some(VertexAttributeValues::Float32x3(p)), Some(VertexAttributeValues::Float32x2(uv)), Some(VertexAttributeValues::Float32x4(c))) => {
                crossed_ribbons(p, uv, c, &w.planes, w.height)
            }
            _ => None,
        });
        let len = built.as_ref().and_then(|b| b.2.last().map(|v| v[1])).unwrap_or(1.0);
        if let Some(mut m) = meshes.get_mut(&mesh.0) {
            let (p, nor, uv, col, idx) = built.unwrap_or_default();
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, p);
            m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nor);
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
            m.insert_indices(Indices::U32(idx));
        }
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.extension.u.params.x = len.max(0.5);
            m.extension.u.params.y = time.elapsed_secs();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plume_profile_parses_with_defaults_and_faction_colours() {
        let e = parse_engines(&serde_json::json!({"colors": {"Navy": [0.3, 0.6, 1.0]}, "particles": 0.3})).unwrap();
        assert_eq!(e.colors.get("Navy"), Some(&Vec3::new(0.3, 0.6, 1.0)));
        assert_eq!((e.particles, e.length, e.layers.len()), (0.3, 0.45, 2));
        let one = parse_engines(&serde_json::json!({"layers": [{"width": 0.5}]})).unwrap();
        assert_eq!(one.layers, [Layer { inner: false, width: 0.5, length: 1.0 }]);
        assert!(parse_engines(&Value::Null).is_none() && parse_wakes(&Value::Null).is_none());
        assert_eq!(parse_wakes(&serde_json::json!({"core": 2})).map(|w| (w.core, w.height)), Some((2.0, 0.58)));
        assert_eq!(e.effects, "Engine", "jets by default; funnel smoke gets no plume");
    }

    #[test]
    fn a_wake_ribbon_becomes_three_crossed_ribbons_along_the_same_path() {
        // two segments, each 2 wide across x, one unit apart along z
        let pos = [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [-1.0, 0.0, 1.0]];
        let uv = [[0.0, 0.0], [1.0, 0.0], [0.0, 0.5], [1.0, 0.5]];
        let col = [[0.5, 0.5, 0.5, 1.0]; 4];
        let planes = parse_wakes(&serde_json::json!({})).unwrap().planes;
        assert!(crossed_ribbons(&pos[..2], &uv[..2], &col[..2], &planes, 1.0).is_none(), "one segment draws nothing");
        let (p, nor, tuv, tcol, idx) = crossed_ribbons(&pos, &uv, &col, &planes, 0.5).unwrap();
        assert_eq!(p.len(), 3 * 4);
        assert!(idx.iter().all(|&i| (i as usize) < p.len()) && idx.len() == 3 * 6);
        assert_eq!(&p[..4], &pos[..], "the first is the retail ribbon itself");
        let tallest = p.iter().map(|v| v[1].abs()).fold(0.0, f32::max);
        assert!((tallest - 0.5 * 60f32.to_radians().sin()).abs() < 1e-3, "tilted 60 degrees, squashed by half: {tallest}");
        let flat = crossed_ribbons(&pos, &uv, &col, &[0.0], 1.0).unwrap();
        assert_eq!(flat.0, pos.to_vec(), "planes [0]: just the retail ribbon");
        assert!(nor.iter().all(|n| (Vec3::from(*n).length() - 1.0).abs() < 1e-3));
        assert_eq!((tuv[0], tuv[1], tuv[2][1], tcol[0][0]), ([0.0, 0.0], [1.0, 0.0], 0.5, 0.5), "keeps the ribbon's u, v and fade");
    }

    #[test]
    fn a_plume_grows_with_thrust_and_trails_astern() {
        let p = Plume { length: 10.0, width: 1.0, thrust: 0.0, clock: 0.0, seed: 0.0 };
        assert!(plume_scale(&p, 0.0).z < plume_scale(&p, 1.0).z);
        assert_eq!(plume_scale(&p, 1.0), Vec3::new(1.0, 1.0, 10.0), "no flicker at clock 0 and seed 0");
        let m = plume_mesh();
        let Some(VertexAttributeValues::Float32x3(pos)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        assert!(pos.iter().all(|p| (0.0..=1.0).contains(&p[2])), "the unit plume runs from the nozzle (z 0) astern (z 1)");
        assert!(m.indices().unwrap().iter().all(|i| i < pos.len()));
    }
}
