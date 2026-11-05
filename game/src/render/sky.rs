//! Not retail: the sky dome of the render profile (`sky` block, drawn with the mod's `shaders/sky.wesl`). The baked sky cube
//! (`view::hd_sky`) stays the base picture and the image-based light; this adds a camera-centred dome just inside it
//! (an inverted unit sphere, child of the `view::Sky` entity, so it follows the camera and never veils a ship: the depth
//! test keeps it behind the world, and its sort bias draws it before every other blended effect).
//!
//! The first layer is the near ether: a sparse, soft copy of the sky's own gas, sampled as if it floated `distance` units
//! out, so it slides against the sky as the camera moves (parallax), and carried along a map-wide current (`drift`, world
//! units a second). A rigid turn of the whole sky reads as the camera rolling; relative motion reads as depth.
//! Off without the `sky` key and without a baked sky: classic never sees it.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, TextureViewDimension};
use bevy::shader::ShaderRef;
use serde_json::Value;

use super::{Look, num, rgb};
use crate::view::{HdSky, Sky};

pub const SHADER: &str = "shaders/sky.wesl";

/// Sorts the dome before every other blended thing (Bevy sorts blended meshes by distance plus this bias).
const SORT_BIAS: f32 = -50_000.0;

/// The dome sits a little inside the skybox's sphere.
const DOME_SCALE: f32 = 0.98;

/// The `sky` block.
#[derive(Clone, Debug, PartialEq)]
pub struct SkyDome {
    /// How far out (world units) the near layer floats: smaller slides more as the camera moves.
    pub distance: f32,
    /// Strength of the near layer, 0..1; 0 draws no dome.
    pub opacity: f32,
    /// Brightness (0..1) below which the gas is left out of the layer, and the width of the fade above it.
    pub threshold: f32,
    pub softness: f32,
    /// Map-wide current: how far the near layer's gas travels a second, world units (game axes, as everywhere in a profile).
    pub drift: Vec3,
}

pub fn parse(v: &Value) -> Option<SkyDome> {
    v.is_object().then(|| {
        let near = &v["near"];
        SkyDome {
            distance: num(&near["distance"], 4000.0).max(1.0),
            opacity: num(&near["opacity"], 0.5).clamp(0.0, 1.0),
            threshold: num(&near["threshold"], 0.3).clamp(0.0, 1.0),
            softness: num(&near["softness"], 0.35).max(0.01),
            drift: rgb(&near["drift"]).map_or(Vec3::ZERO, |d| Vec3::new(d.x, d.z, -d.y)),
        }
    })
}

#[derive(Clone, Copy, Default, Debug, PartialEq, ShaderType)]
pub struct SkyUniform {
    /// x: real seconds since start (not wrapped), y: battle seconds, z: this frame's delta
    pub clock: Vec4,
    /// x: opacity, y: threshold, z: softness, w: distance
    pub near: Vec4,
    /// xyz: the current, units a second
    pub current: Vec4,
}

impl SkyDome {
    fn uniform(&self, real: f32, battle: f32, delta: f32) -> SkyUniform {
        SkyUniform { clock: Vec4::new(real, battle, delta, 0.0), near: Vec4::new(self.opacity, self.threshold, self.softness, self.distance), current: self.drift.extend(0.0) }
    }
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct SkyGlow {
    #[uniform(100)]
    pub u: SkyUniform,
    #[texture(101, dimension = "cube")]
    #[sampler(102)]
    pub cube: Option<Handle<Image>>,
}

impl MaterialExtension for SkyGlow {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // behind everything: the depth test does that, so the dome writes none
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

pub type SkyMaterial = ExtendedMaterial<StandardMaterial, SkyGlow>;

pub fn plugin(app: &mut App, look: &Look) {
    if look.sky.is_some() {
        app.add_plugins(MaterialPlugin::<SkyMaterial>::default()).add_systems(Update, dome);
    }
}

/// Marks the dome, the child of the `Sky` entity.
#[derive(Component)]
struct Dome;

/// Whether the baked sky has been turned into a cube (`view::hd_sky` does it once the image has loaded): binding it as a
/// cube before then would not match its texture.
fn is_cube(image: &Image) -> bool {
    image.texture_view_descriptor.as_ref().is_some_and(|d| d.dimension == Some(TextureViewDimension::Cube))
}

/// Gives each baked sky its dome once the sky is a cube, and moves the clock on.
#[allow(clippy::too_many_arguments)]
fn dome(
    mut commands: Commands,
    look: Res<Look>,
    real: Res<Time<Real>>,
    time: Res<Time>,
    skies: Query<(Entity, &HdSky), With<Sky>>,
    domes: Query<(&ChildOf, &MeshMaterial3d<SkyMaterial>), With<Dome>>,
    images: Res<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
) {
    let Some(sky) = &look.sky else { return };
    let uniform = sky.uniform(real.elapsed_secs(), time.elapsed_secs(), time.delta_secs());
    for (_, mat) in &domes {
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.extension.u = uniform;
        }
    }
    if sky.opacity <= 0.0 {
        return;
    }
    for (e, hd) in &skies {
        if domes.iter().any(|(p, _)| p.parent() == e) || !images.get(&hd.0).is_some_and(is_cube) {
            continue;
        }
        let base = StandardMaterial { unlit: true, cull_mode: None, double_sided: true, alpha_mode: AlphaMode::Blend, depth_bias: SORT_BIAS, ..default() };
        let material = materials.add(SkyMaterial { base, extension: SkyGlow { u: uniform, cube: Some(hd.0.clone()) } });
        commands.entity(e).with_child((
            Dome,
            Mesh3d(meshes.add(Sphere::new(1.0).mesh().uv(48, 24))),
            MeshMaterial3d(material),
            Transform::from_scale(Vec3::splat(DOME_SCALE)),
            bevy::light::NotShadowCaster,
            bevy::camera::visibility::NoFrustumCulling,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn absent_without_the_key_and_defaults_with_it() {
        assert!(parse(&Value::Null).is_none());
        let d = parse(&json!({})).unwrap();
        assert_eq!((d.distance, d.opacity, d.drift), (4000.0, 0.5, Vec3::ZERO));
    }

    #[test]
    fn the_current_is_given_in_game_axes() {
        let d = parse(&json!({"near": {"drift": [1.0, 2.0, 3.0], "opacity": 4.0}})).unwrap();
        assert_eq!(d.drift, Vec3::new(1.0, 3.0, -2.0), "x starboard, y forward, z up become Bevy's x, y up, -z forward");
        assert_eq!(d.opacity, 1.0, "clamped");
    }

    #[test]
    fn the_clock_is_not_wrapped() {
        // the retired sky_drift wrapped at the hour and the sky snapped back
        let u = parse(&json!({})).unwrap().uniform(7200.5, 12.0, 0.016);
        assert_eq!(u.clock, Vec4::new(7200.5, 12.0, 0.016, 0.0));
    }

    #[test]
    fn only_a_cube_view_can_be_bound() {
        let mut image = Image::default();
        assert!(!is_cube(&image));
        image.texture_view_descriptor = Some(bevy::render::render_resource::TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
        assert!(is_cube(&image));
    }
}
