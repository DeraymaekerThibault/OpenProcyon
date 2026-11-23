//! Particle effects from the original .eft files (exported to JSON by tools/export_effects.py).
//! CPU particles, drawn as camera-facing quads in one dynamic mesh per (texture, blend). The meaning of the emitter
//! parameters is inferred from the data, not measured from the original renderer (see AGENTS.md, rule 5).
//!
//! `lib.rs` parses the exported effects (`FxLib`), `sim.rs` runs the 30 Hz particle simulation and the morphing meshes,
//! `draw.rs` builds the quads. This file holds what they share: the components and resources, `spawn` and the `EffectStarted`
//! message, the additive material.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor, SpecializedMeshPipelineError};

use crate::combat::Transient;

mod draw;
mod lib;
mod sim;

pub use draw::ParticleGroup;
use draw::{Groups, render_fx};
pub use lib::{FxLib, curve_keys};
use sim::{render_mesh_fx, update_fx};

/// Retail ONE/ONE blending must not premultiply texture alpha a second time (Bevy AlphaMode::Add does).
#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct RetailAdd {}
impl MaterialExtension for RetailAdd {
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let component = BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::One, operation: BlendOperation::Add };
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState { color: component, alpha: component });
            }
        }
        // `depth_bias` is only a sort key here (see `additive_material`); keep the GPU depth test unbiased.
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.bias.constant = 0;
        }
        Ok(())
    }
}
pub type EffectMaterial = ExtendedMaterial<StandardMaterial, RetailAdd>;
/// Transparent sort bias for additive effects. Their pooled meshes span the whole map, so Bevy's centre-distance sort could draw
/// them before a nearer sail, which then painted over them (PT-05). PROVISIONAL: drawn after every alpha-blended mesh, so a trail
/// behind a translucent sail shows at full strength instead of dimmed; per-emitter sorting would fix that.
pub const ADDITIVE_SORT_BIAS: f32 = 10_000.0;
pub fn additive_material(texture: Handle<Image>) -> EffectMaterial {
    EffectMaterial {
        base: StandardMaterial {
            base_color_texture: Some(texture),
            unlit: true,
            cull_mode: None,
            double_sided: true,
            alpha_mode: AlphaMode::Blend,
            depth_bias: ADDITIVE_SORT_BIAS,
            ..default()
        },
        extension: RetailAdd {},
    }
}

/// One step of the original's 32-bit linear congruential generator, as a float in 0..1.
pub fn lcg_unit(state: &mut u32) -> f32 {
    *state = state.wrapping_mul(0x19660d).wrapping_add(0x3c6ef35f);
    f32::from_bits(*state & 0x7fffff | 0x3f800000) - 1.0
}

/// Replaces a ribbon mesh's positions, uvs, colours and triangle indices.
pub fn set_ribbons(
    mesh: &mut Mesh,
    pos: impl Into<bevy::mesh::VertexAttributeValues>,
    uv: impl Into<bevy::mesh::VertexAttributeValues>,
    col: impl Into<bevy::mesh::VertexAttributeValues>,
    idx: Vec<u32>,
) {
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_indices(bevy::mesh::Indices::U32(idx));
}

/// `set_ribbons` on the mesh `handle` names, except that nothing is written when the mesh already holds exactly this (the asset is
/// not touched, so Bevy does not re-extract and re-upload it). That covers a mesh that is still empty (rewriting it with nothing
/// makes Bevy warn "no vertices") and a ribbon whose inputs did not change since the last frame.
pub fn set_ribbons_if_changed(meshes: &mut Assets<Mesh>, handle: &Handle<Mesh>, pos: Vec<[f32; 3]>, uv: Vec<[f32; 2]>, col: Vec<[f32; 4]>, idx: Vec<u32>) {
    use bevy::mesh::VertexAttributeValues as V;
    let Some(old) = meshes.get(handle) else { return };
    let same = (matches!(old.attribute(Mesh::ATTRIBUTE_POSITION), Some(V::Float32x3(v)) if *v == pos) || (pos.is_empty() && old.attribute(Mesh::ATTRIBUTE_POSITION).is_none()))
        && (matches!(old.attribute(Mesh::ATTRIBUTE_UV_0), Some(V::Float32x2(v)) if *v == uv) || (uv.is_empty() && old.attribute(Mesh::ATTRIBUTE_UV_0).is_none()))
        && (matches!(old.attribute(Mesh::ATTRIBUTE_COLOR), Some(V::Float32x4(v)) if *v == col) || (col.is_empty() && old.attribute(Mesh::ATTRIBUTE_COLOR).is_none()))
        && (matches!(old.indices(), Some(bevy::mesh::Indices::U32(v)) if *v == idx) || (idx.is_empty() && old.indices().is_none()));
    if !same && let Some(mut m) = meshes.get_mut(handle) {
        set_ribbons(&mut m, pos, uv, col, idx);
    }
}

#[derive(Resource, Default)]
struct FxTextures(Vec<Handle<Image>>);
#[derive(Resource)]
struct FxRng(u64);

/// The effect-detail option's particle factor as it stood when the battle started (`scenario::reset_sim`), not the live option:
/// the particle count feeds the effect random stream, so it must not change under a battle. PROVISIONAL: the original may apply
/// a detail change at once.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct DetailScale(pub f32);

impl Default for DetailScale {
    fn default() -> Self {
        Self(1.0)
    }
}
impl FxRng {
    fn f(&mut self) -> f32 {
        crate::combat::xorshift_unit(&mut self.0)
    }
    fn sphere(&mut self) -> Vec3 {
        loop {
            let v = Vec3::new(self.f() * 2.0 - 1.0, self.f() * 2.0 - 1.0, self.f() * 2.0 - 1.0);
            if v.length_squared() <= 1.0 {
                return v;
            }
        }
    }
}

struct P {
    pos: Vec3,
    vel: Vec3,
    age: f32,
    life: f32,
}
struct PrimState {
    started: bool,
    /// Keys-only rate curves fire once as the time enters each key interval: the interval last fired.
    last_key: Option<usize>,
    parts: Vec<P>,
}

/// On an effect a render profile replaces with its own drawing: it keeps running (particles, timers, lights, sounds) but its quads
/// are not drawn. Presentation only: `render/replace.rs` puts it on.
#[derive(Component, Default)]
pub struct HiddenFx;

/// A running effect. Everlasting effects (engines) never finish; others despawn when done and empty.
#[derive(Component)]
pub struct Fx {
    eff: usize,
    age: f32,
    tick_acc: f32,
    move_acc: f32,
    previous_world: Transform,
    prims: Vec<PrimState>,
    pub rate_scale: f32,
    pub size_scale: f32,
    /// SPHERE_VELOCITY_EMITTER_LENGTH_MULTIPLY: scales the speed of the particles the velocity emitter gives (1 = authored).
    pub vel_scale: f32,
    pub emitting: bool,
    /// The vertices of the mesh the effect is told about (a damaged section's layer), ship-local; see `Prim::mesh_vertices`.
    pub vertices: Vec<Vec3>,
    meshes: Vec<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    /// Per primitive, what its mesh was last written from (morph weight, scale, colour), so an unchanged mesh is left alone.
    mesh_keys: Vec<Option<[f32; 6]>>,
}

impl Fx {
    pub fn particle_count(&self) -> usize {
        self.prims.iter().map(|p| p.parts.len()).sum()
    }
}

pub fn plugin(app: &mut App) {
    app.add_plugins(MaterialPlugin::<EffectMaterial>::default())
        .add_message::<EffectStarted>()
        .init_resource::<FxLib>()
        .init_resource::<DetailScale>()
        .insert_resource(FxRng(0xD1B54A32D192ED03))
        .init_resource::<FxTextures>()
        .init_resource::<Groups>()
        .add_systems(Startup, load_textures)
        .add_systems(PostUpdate, (update_fx, render_mesh_fx, render_fx).chain().after(bevy::transform::TransformSystems::Propagate));
}

fn load_textures(lib: Res<FxLib>, assets: Res<AssetServer>, mut tex: ResMut<FxTextures>) {
    tex.0 = lib.tex_names.iter().map(|n| assets.load(format!("fx/tex/{n}.png"))).collect();
}

/// A retail effect started: what a renderer needs to decorate it. Written by `spawn` on the tick that starts the effect and read in
/// `Update` (messages are retained across frames without a tick). Presentation input only: nothing in the simulation reads it. The
/// optional render-profile blocks (lights, bursts, tracers) use it instead of watching effects appear.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct EffectStarted {
    pub entity: Entity,
    /// `FxLib` index, as `Fx::effect()`.
    pub effect: usize,
    /// World position it starts at.
    pub at: Vec3,
    /// The shot's direction for a muzzle flash, the surface normal for an impact, else the effect node's own forward.
    pub forward: Vec3,
    /// The shell it belongs to: the one a muzzle flash fires, the one a flight effect rides on.
    pub shell: Option<Entity>,
    /// The ship or base an impact struck.
    pub host: Option<Entity>,
    /// The combat material key of the section struck, for presentation only.
    pub material: Option<&'static str>,
}

/// What a caller knows about the effect it starts, beyond its pose: it fills the `EffectStarted` message.
#[derive(Clone, Copy, Debug, Default)]
pub struct Origin {
    /// Replaces the effect node's forward in the message.
    pub forward: Option<Vec3>,
    pub shell: Option<Entity>,
    pub host: Option<Entity>,
    pub material: Option<&'static str>,
}

/// Starts effect `name` at world transform `world` (so the first frame is right), optionally as a child of `parent`
/// (then `local` is relative to it). Unknown names are ignored. Returns the effect entity.
pub fn spawn(commands: &mut Commands, lib: &FxLib, name: &str, parent: Option<Entity>, local: Transform, world: Transform, transient: bool) -> Option<Entity> {
    spawn_with(commands, lib, name, parent, local, world, transient, Origin::default())
}

/// `spawn`, telling the `EffectStarted` message more than the pose says (the shot's direction, the surface normal, the shell, the host).
/// The message is queued as a command, so `spawn` keeps taking `&mut Commands` and its callers stay as they are.
#[allow(clippy::too_many_arguments)]
pub fn spawn_with(commands: &mut Commands, lib: &FxLib, name: &str, parent: Option<Entity>, local: Transform, world: Transform, transient: bool, origin: Origin) -> Option<Entity> {
    let &eff = lib.effects.get(name)?;
    let fx = Fx {
        eff,
        age: 0.0,
        tick_acc: 0.0,
        move_acc: 0.0,
        previous_world: world,
        prims: lib.list[eff].prims.iter().map(|_| PrimState { started: false, last_key: None, parts: vec![] }).collect(),
        rate_scale: 1.0,
        size_scale: 1.0,
        vel_scale: 1.0,
        emitting: true,
        vertices: vec![],
        meshes: lib.list[eff].prims.iter().map(|_| None).collect(),
        mesh_keys: lib.list[eff].prims.iter().map(|_| None).collect(),
    };
    let mut e = commands.spawn((fx, local, GlobalTransform::from(world), Visibility::default()));
    if let Some(p) = parent {
        e.insert(ChildOf(p));
    }
    if transient {
        e.insert(Transient);
    }
    let entity = e.id();
    let started = EffectStarted {
        entity,
        effect: eff,
        at: world.translation,
        forward: origin.forward.unwrap_or_else(|| world.forward().as_vec3()),
        shell: origin.shell,
        host: origin.host,
        material: origin.material,
    };
    commands.queue(move |world: &mut World| {
        // no message store (a test world without the plugin): nobody listens
        if let Some(mut messages) = world.get_resource_mut::<Messages<EffectStarted>>() {
            messages.write(started);
        }
    });
    Some(entity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::PrimitiveTopology;

    #[test]
    fn spawning_an_effect_announces_it_with_the_nodes_forward_unless_the_caller_knows_more() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.init_resource::<Messages<EffectStarted>>();
        world.insert_resource(FxLib::with_names(&["A", "B"]));
        let node = Transform::from_xyz(1.0, 2.0, 3.0).looking_to(Vec3::X, Vec3::Y);
        let (shell, host) = (world.spawn_empty().id(), world.spawn_empty().id());
        let made = world
            .run_system_once(move |mut commands: Commands, lib: Res<FxLib>| {
                let plain = spawn(&mut commands, &lib, "B", None, node, node, true).unwrap();
                let told =
                    spawn_with(&mut commands, &lib, "A", None, node, node, true, Origin { forward: Some(Vec3::Y), shell: Some(shell), host: Some(host), material: Some("Iron") })
                        .unwrap();
                assert!(spawn(&mut commands, &lib, "Unknown", None, node, node, true).is_none());
                (plain, told)
            })
            .unwrap();
        let sent: Vec<_> = world.resource_mut::<Messages<EffectStarted>>().drain().collect();
        assert_eq!(sent.len(), 2, "one per effect started, none for an unknown name");
        let plain = EffectStarted { entity: made.0, effect: 1, at: node.translation, forward: node.forward().as_vec3(), shell: None, host: None, material: None };
        assert_eq!(sent[0], plain, "no shell: the node's own forward");
        assert_eq!(sent[1], EffectStarted { entity: made.1, effect: 0, forward: Vec3::Y, shell: Some(shell), host: Some(host), material: Some("Iron"), ..plain });
    }

    #[test]
    fn a_ribbon_that_did_not_change_is_not_written_and_one_that_did_is() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default())).init_asset::<Mesh>();
        let handle = app.world_mut().resource_mut::<Assets<Mesh>>().add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
        let ribbon = |x: f32| (vec![[x, 0.0, 0.0], [x, 1.0, 0.0], [x + 1.0, 0.0, 0.0]], vec![[0.0, 0.0]; 3], vec![[1.0; 4]; 3], vec![0, 1, 2]);
        let write = |app: &mut App, x: f32| {
            let (p, u, c, i) = ribbon(x);
            set_ribbons_if_changed(&mut app.world_mut().resource_mut::<Assets<Mesh>>(), &handle, p, u, c, i);
            app.update();
            app.world_mut().resource_mut::<Messages<AssetEvent<Mesh>>>().drain().filter(|e| matches!(e, AssetEvent::Modified { .. })).count()
        };
        assert_eq!(write(&mut app, 0.0), 1, "first content");
        assert_eq!(write(&mut app, 0.0), 0, "same content: the asset is not touched");
        assert_eq!(write(&mut app, 0.0), 0);
        assert_eq!(write(&mut app, 2.0), 1, "changed content is written");
        let none = |app: &mut App| {
            let mut empty = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
            empty.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
            let h = app.world_mut().resource_mut::<Assets<Mesh>>().add(empty);
            set_ribbons_if_changed(&mut app.world_mut().resource_mut::<Assets<Mesh>>(), &h, vec![], vec![], vec![], vec![]);
            app.update();
            app.world_mut().resource_mut::<Messages<AssetEvent<Mesh>>>().drain().filter(|e| matches!(e, AssetEvent::Modified { .. })).count()
        };
        assert_eq!(none(&mut app), 0, "an empty mesh is not rewritten with nothing");
    }
}
