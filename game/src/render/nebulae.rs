//! Not retail: nebula clouds and solar storms of the render profile (`nebulae` block, drawn with the mod's
//! `shaders/nebula.wesl`). The retail cloud is one additive textured shell whose vertices breathe (`nebula.rs`: the shifting
//! sim, the colour loop and the edge fade, written into the vertex colours every tick). That stays as the envelope; this swaps
//! the shell's material for one whose shader also marches a short ray through the cloud's ellipsoid and adds glowing, churning
//! gas (domain-warped 3-D noise) to it. Added light only (ONE/ONE), so it can never hide a ship. A bolt of storm lightning
//! (`nebula::StormFlash`) lights the gas around it for a moment from inside. With the camera inside the cloud the far wall
//! of the shell is marched from the camera, so the gas fills the screen gently (`inside` caps it).
//!
//! Only reads `nebula::Cloud`; nothing here changes what a cloud does.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use serde_json::Value;

use super::{Look, num, rgb};
use crate::fx::EffectMaterial;
use crate::nebula::{Cloud, StormFlash};

pub const SHADER: &str = "shaders/nebula.wesl";

/// Most steps a ray takes through a cloud.
pub const MAX_STEPS: f32 = 24.0;

/// Half the cloud's size along each axis, shares of its radius (`docs/re/nebula-landmark.md`; the same box for a solar storm's
/// mesh, PROVISIONAL).
const AXES: Vec3 = Vec3::new(1.0, 0.7, 0.99);

/// Seconds a bolt's light lasts inside the cloud.
const FLASH_SECONDS: f32 = 0.3;

/// The `nebulae` block.
#[derive(Clone, Debug, PartialEq)]
pub struct Nebulae {
    pub rules: Vec<Rule>,
}

/// How the clouds whose effect name contains `matching` are drawn (first rule that matches wins).
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    /// A fragment of the cloud's effect name (`Nebula`, `SolarStorm`), lower case.
    pub matching: String,
    /// Strength of the gas; above 1 the cores bloom.
    pub density: f32,
    /// Ray steps through the cloud (4..=24).
    pub steps: f32,
    /// Noise patches across the cloud.
    pub scale: f32,
    /// How fast the gas churns (noise units a second).
    pub churn: f32,
    pub color_low: Vec3,
    pub color_high: Vec3,
    /// How much of the retail shell (its texture and edge fade) stays under the gas.
    pub shell: f32,
    /// Share of the gas drawn when the camera is inside the cloud (keeps the battle readable).
    pub inside: f32,
    /// How bright a bolt's light is in the gas, and how far it reaches (a share of the radius).
    pub flash: f32,
    pub flash_reach: f32,
}

pub fn parse(v: &Value) -> Option<Nebulae> {
    v.is_object().then(|| Nebulae {
        rules: v["rules"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|r| {
                        Some(Rule {
                            matching: r["match"].as_str()?.to_lowercase(),
                            density: num(&r["density"], 1.0).clamp(0.0, 8.0),
                            steps: num(&r["steps"], 16.0).clamp(4.0, MAX_STEPS).round(),
                            scale: num(&r["scale"], 2.2).clamp(0.2, 12.0),
                            churn: num(&r["churn"], 0.02).clamp(0.0, 1.0),
                            color_low: rgb(&r["color_low"]).unwrap_or(Vec3::new(0.35, 0.15, 0.6)).max(Vec3::ZERO),
                            color_high: rgb(&r["color_high"]).unwrap_or(Vec3::new(1.0, 0.85, 1.0)).max(Vec3::ZERO),
                            shell: num(&r["shell"], 0.5).clamp(0.0, 1.0),
                            inside: num(&r["inside"], 0.6).clamp(0.0, 1.0),
                            flash: num(&r["flash"], 3.0).max(0.0),
                            flash_reach: num(&r["flash_reach"], 0.5).clamp(0.05, 2.0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

impl Nebulae {
    /// The rule for a cloud made by the effect `effect`, if any.
    pub fn find(&self, effect: &str) -> Option<&Rule> {
        let e = effect.to_lowercase();
        self.rules.iter().find(|r| e.contains(&r.matching))
    }
}

/// What the nebula shader gets at `@group(MATERIAL_BIND_GROUP) @binding(100)`, besides the StandardMaterial bindings.
#[derive(Clone, Copy, Default, Debug, PartialEq, ShaderType)]
pub struct NebulaUniform {
    /// xyz: the cloud's centre (world), w: its radius
    pub centre: Vec4,
    /// xyz: half-size per axis as shares of the radius, w: ray steps
    pub axes: Vec4,
    /// x: density, y: noise scale, z: churn, w: share of the retail shell kept
    pub look: Vec4,
    /// rgb: colour of thin gas, w: share of the gas drawn with the camera inside
    pub low: Vec4,
    /// rgb: colour of dense gas, w: the bolt's brightness in the gas
    pub high: Vec4,
    /// xyz: where the last bolt struck (world), w: how much of its light is left (0 = none)
    pub flash: Vec4,
    /// x: clock (battle seconds), y: how far a bolt reaches (world units)
    pub params: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct NebulaGlow {
    #[uniform(100)]
    pub u: NebulaUniform,
}

impl MaterialExtension for NebulaGlow {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // retail ONE/ONE: light is only ever added
        let component = BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::One, operation: BlendOperation::Add };
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState { color: component, alpha: component });
            }
        }
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.bias.constant = 0;
        }
        Ok(())
    }
}

pub type NebulaMaterial = ExtendedMaterial<StandardMaterial, NebulaGlow>;

pub fn plugin(app: &mut App, look: &Look) {
    if look.nebulae.is_some() {
        app.add_plugins(MaterialPlugin::<NebulaMaterial>::default()).init_resource::<Bolt>().add_systems(Update, (bolts, restyle, update).chain());
    }
}

/// A cloud that has the volume material.
#[derive(Component)]
pub struct Volumetric;

/// The last bolt of storm lightning: where, and how much of its light is left (1 at the strike, 0 when out).
#[derive(Resource, Default)]
struct Bolt {
    at: Vec3,
    left: f32,
}

fn bolts(time: Res<Time<Real>>, mut flashes: MessageReader<StormFlash>, mut bolt: ResMut<Bolt>) {
    bolt.left = (bolt.left - time.delta_secs() / FLASH_SECONDS).max(0.0);
    for f in flashes.read() {
        *bolt = Bolt { at: f.at, left: 1.0 };
    }
}

impl Rule {
    fn uniform(&self, centre: Vec3, radius: f32, clock: f32, bolt: &Bolt) -> NebulaUniform {
        NebulaUniform {
            centre: centre.extend(radius),
            axes: AXES.extend(self.steps),
            look: Vec4::new(self.density, self.scale, self.churn, self.shell),
            low: self.color_low.extend(self.inside),
            high: self.color_high.extend(self.flash),
            flash: bolt.at.extend(bolt.left * bolt.left),
            params: Vec4::new(clock, self.flash_reach * radius, 0.0, 0.0),
        }
    }
}

/// Swaps the retail material of every cloud a rule matches, once, keeping its mesh (which still carries the retail colour loop
/// and the edge fade in its vertex colours) and its texture.
fn restyle(
    mut commands: Commands,
    look: Res<Look>,
    time: Res<Time>,
    bolt: Res<Bolt>,
    clouds: Query<(Entity, &Cloud, &GlobalTransform, &MeshMaterial3d<EffectMaterial>), Without<Volumetric>>,
    retail: Res<Assets<EffectMaterial>>,
    mut materials: ResMut<Assets<NebulaMaterial>>,
) {
    let Some(n) = &look.nebulae else { return };
    for (e, cloud, at, mat) in &clouds {
        let Some(rule) = n.find(cloud.effect()) else { continue };
        let Some(old) = retail.get(&mat.0) else { continue };
        let base = StandardMaterial { base_color: Color::WHITE, base_color_texture: old.base.base_color_texture.clone(), ..old.base.clone() };
        let handle = materials.add(NebulaMaterial { base, extension: NebulaGlow { u: rule.uniform(at.translation(), cloud.radius(), time.elapsed_secs(), &bolt) } });
        commands.entity(e).try_remove::<MeshMaterial3d<EffectMaterial>>().try_insert((MeshMaterial3d(handle), Volumetric));
    }
}

fn update(
    look: Res<Look>,
    time: Res<Time>,
    bolt: Res<Bolt>,
    clouds: Query<(&Cloud, &GlobalTransform, &MeshMaterial3d<NebulaMaterial>)>,
    mut materials: ResMut<Assets<NebulaMaterial>>,
) {
    let Some(n) = &look.nebulae else { return };
    for (cloud, at, mat) in &clouds {
        let (Some(rule), Some(mut m)) = (n.find(cloud.effect()), materials.get_mut(&mat.0)) else { continue };
        m.extension.u = rule.uniform(at.translation(), cloud.radius(), time.elapsed_secs(), &bolt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn absent_without_the_key_and_a_rule_needs_its_match() {
        assert!(parse(&Value::Null).is_none());
        assert!(parse(&json!({})).unwrap().rules.is_empty());
        assert!(parse(&json!({"rules": [{"density": 2}]})).unwrap().rules.is_empty(), "a rule without `match` is dropped");
    }

    #[test]
    fn defaults_and_clamping() {
        let n = parse(&json!({"rules": [{"match": "Nebula"}, {"match": "SolarStorm", "steps": 999, "density": -3, "inside": 5}]})).unwrap();
        assert_eq!((n.rules[0].steps, n.rules[0].density, n.rules[0].matching.as_str()), (16.0, 1.0, "nebula"));
        assert_eq!((n.rules[1].steps, n.rules[1].density, n.rules[1].inside), (MAX_STEPS, 0.0, 1.0));
    }

    #[test]
    fn rules_match_by_effect_name_first_wins() {
        let n = parse(&json!({"rules": [{"match": "SolarStorm", "density": 4}, {"match": "Nebula", "density": 1}, {"match": "s", "density": 9}]})).unwrap();
        assert_eq!(n.find("SolarStorm").map(|r| r.density), Some(4.0));
        assert_eq!(n.find("Nebula").map(|r| r.density), Some(1.0));
        assert!(n.find("Meteor").is_none(), "no rule, the retail material stays");
    }

    #[test]
    fn the_uniform_carries_the_boxes_axes_and_a_fading_bolt() {
        let rule = &parse(&json!({"rules": [{"match": "n", "steps": 20}]})).unwrap().rules[0];
        let bolt = Bolt { at: Vec3::new(1.0, 2.0, 3.0), left: 0.5 };
        let u = rule.uniform(Vec3::new(10.0, 0.0, 0.0), 300.0, 4.0, &bolt);
        assert_eq!((u.centre, u.axes), (Vec4::new(10.0, 0.0, 0.0, 300.0), Vec4::new(1.0, 0.7, 0.99, 20.0)));
        assert_eq!(u.flash, Vec4::new(1.0, 2.0, 3.0, 0.25));
        assert_eq!(rule.uniform(Vec3::ZERO, 1.0, 0.0, &Bolt::default()).flash.w, 0.0);
    }

    #[test]
    fn a_cloud_swaps_its_material_once_keeping_its_mesh_and_unmatched_clouds_keep_theirs() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Shader>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Mesh>()
            .init_asset::<EffectMaterial>()
            .init_asset::<NebulaMaterial>()
            .init_resource::<Bolt>()
            .insert_resource(Look::parse(&json!({"nebulae": {"rules": [{"match": "Nebula"}]}})))
            .add_systems(Update, restyle);
        let (mesh, retail) = {
            let w = app.world_mut();
            (w.resource_mut::<Assets<Mesh>>().add(Mesh::from(Cuboid::default())), w.resource_mut::<Assets<EffectMaterial>>().add(crate::fx::additive_material(Handle::default())))
        };
        let (cloud, storm) = (Cloud::for_tests("Nebula", 100.0), Cloud::for_tests("MeteorStorm", 50.0));
        let a = app.world_mut().spawn((cloud, GlobalTransform::default(), Mesh3d(mesh.clone()), MeshMaterial3d(retail.clone()))).id();
        let b = app.world_mut().spawn((storm, GlobalTransform::default(), Mesh3d(mesh.clone()), MeshMaterial3d(retail.clone()))).id();
        app.update();
        app.update();
        let w = app.world();
        assert!(w.entity(a).contains::<MeshMaterial3d<NebulaMaterial>>() && w.entity(a).contains::<Volumetric>() && !w.entity(a).contains::<MeshMaterial3d<EffectMaterial>>());
        assert_eq!(w.entity(a).get::<Mesh3d>().map(|m| m.0.id()), Some(mesh.id()), "the mesh, with its vertex colours, is kept");
        assert!(w.entity(b).contains::<MeshMaterial3d<EffectMaterial>>() && !w.entity(b).contains::<Volumetric>());
        assert_eq!(w.resource::<Assets<NebulaMaterial>>().len(), 1, "once");
    }
}
