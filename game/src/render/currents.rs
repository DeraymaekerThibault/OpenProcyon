//! Not retail: Etherium currents of the render profile (`currents` block, drawn with the mod's `shaders/current.wesl`). The retail
//! current is two additive strips scrolling a texture along the path (`environment.rs`, `CurrentRibbon`). This swaps those
//! strips' material for one whose shader draws flowing streaks, sparkles that ride downstream faster than the streaks, soft edges
//! and a little bloom in the core. The mesh, its UVs (u across, v along the path from its first point to its last: the way the
//! force pushes) and the retail scroll are kept, so the flow direction is the retail one.
//!
//! Only reads `environment::CurrentRibbon`; nothing here changes what a current does.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use serde_json::Value;

use super::{Look, num, rgb};
use crate::environment::CurrentRibbon;
use crate::fx::EffectMaterial;

pub const SHADER: &str = "shaders/current.wesl";

/// The `currents` block.
#[derive(Clone, Debug, PartialEq)]
pub struct Currents {
    /// Overall strength; the core is past 1 so it blooms.
    pub brightness: f32,
    /// Streaks across the strip.
    pub streaks: f32,
    /// Strength of the sparkles that ride downstream.
    pub sparkle: f32,
    /// Extra light along the strip's middle line.
    pub core: f32,
    /// How much of the retail texture stays under the streaks.
    pub texture: f32,
    pub color: Vec3,
    /// Strength of the slow pulses that run down the current (they make the direction readable at a glance).
    pub pulse: f32,
}

pub fn parse(v: &Value) -> Option<Currents> {
    v.is_object().then(|| Currents {
        brightness: num(&v["brightness"], 1.4).max(0.0),
        streaks: num(&v["streaks"], 10.0).clamp(1.0, 60.0),
        sparkle: num(&v["sparkle"], 1.0).max(0.0),
        core: num(&v["core"], 1.5).max(0.0),
        texture: num(&v["texture"], 0.5).clamp(0.0, 1.0),
        color: rgb(&v["color"]).unwrap_or(Vec3::new(0.45, 0.8, 1.0)).max(Vec3::ZERO),
        pulse: num(&v["pulse"], 0.35).clamp(0.0, 1.0),
    })
}

/// What the current shader gets at `@group(MATERIAL_BIND_GROUP) @binding(100)`, besides the StandardMaterial bindings.
#[derive(Clone, Copy, Default, Debug, PartialEq, ShaderType)]
pub struct CurrentUniform {
    /// x: brightness, y: streaks across, z: sparkle, w: core
    pub look: Vec4,
    /// rgb: colour, w: share of the retail texture kept
    pub color: Vec4,
    /// x: this strip's retail scroll speed (texture heights a second), y: pulse strength (the clock is the frame data's)
    pub params: Vec4,
}

impl Currents {
    fn uniform(&self, speed: f32) -> CurrentUniform {
        CurrentUniform {
            look: Vec4::new(self.brightness, self.streaks, self.sparkle, self.core),
            color: self.color.extend(self.texture),
            params: Vec4::new(speed, self.pulse, 0.0, 0.0),
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
}

impl MaterialExtension for CurrentGlow {
    fn fragment_shader() -> ShaderRef {
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

/// Swaps the retail material of each current strip, once, keeping its mesh. The shader scrolls the retail texture itself with the
/// frame clock (`frame.rs`), so the material is never touched again (`environment::flow_currents` only scrolls its own).
fn restyle(
    mut commands: Commands,
    look: Res<Look>,
    strips: Query<(Entity, &CurrentRibbon, &MeshMaterial3d<EffectMaterial>)>,
    retail: Res<Assets<EffectMaterial>>,
    mut materials: ResMut<Assets<CurrentMaterial>>,
) {
    let Some(c) = &look.currents else { return };
    for (e, ribbon, mat) in &strips {
        let Some(old) = retail.get(&mat.0) else { continue };
        let base = StandardMaterial { uv_transform: default(), ..old.base.clone() };
        let handle = materials.add(CurrentMaterial { base, extension: CurrentGlow { u: c.uniform(ribbon.0), frame: default() } });
        super::kit::swap::<EffectMaterial, _>(&mut commands, e, handle);
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
        assert_eq!(c.uniform(0.25).params, Vec4::new(0.25, c.pulse, 0.0, 0.0));
    }

    #[test]
    fn a_strip_swaps_its_material_once_keeping_its_mesh_and_the_shader_scrolls_it() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Shader>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Mesh>()
            .init_asset::<EffectMaterial>()
            .init_asset::<CurrentMaterial>()
            .insert_resource(Look::parse(&json!({"currents": {}})))
            .add_systems(Update, restyle);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(Mesh::from(Cuboid::default()));
        let retail = app.world_mut().resource_mut::<Assets<EffectMaterial>>().add(crate::fx::additive_material(Handle::default()));
        let strip = app.world_mut().spawn((CurrentRibbon(0.25), Mesh3d(mesh.clone()), MeshMaterial3d(retail))).id();
        app.update();
        app.update();
        let w = app.world();
        assert!(w.entity(strip).contains::<MeshMaterial3d<CurrentMaterial>>() && !w.entity(strip).contains::<MeshMaterial3d<EffectMaterial>>());
        assert_eq!(w.entity(strip).get::<Mesh3d>().map(|m| m.0.id()), Some(mesh.id()));
        assert_eq!(w.resource::<Assets<CurrentMaterial>>().len(), 1, "once");
        let (_, m) = w.resource::<Assets<CurrentMaterial>>().iter().next().unwrap();
        assert_eq!(m.base.uv_transform, default(), "no scroll in the material: the shader scrolls by the frame clock");
        assert_eq!(m.extension.u.params.x, 0.25, "at the strip's own retail speed");
    }
}
