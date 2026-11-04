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

#[derive(Resource, Default)]
struct FxTextures(Vec<Handle<Image>>);
#[derive(Resource)]
struct FxRng(u64);
impl FxRng {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
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
}

impl Fx {
    /// Which effect this is (an index into `FxLib::names`).
    pub fn effect(&self) -> usize {
        self.eff
    }
    pub fn particle_count(&self) -> usize {
        self.prims.iter().map(|p| p.parts.len()).sum()
    }
}

pub fn plugin(app: &mut App) {
    app.add_plugins(MaterialPlugin::<EffectMaterial>::default())
        .add_message::<EffectStarted>()
        .init_resource::<FxLib>()
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
}

/// What a caller knows about the effect it starts, beyond its pose: it fills the `EffectStarted` message.
#[derive(Clone, Copy, Debug, Default)]
pub struct Origin {
    /// Replaces the effect node's forward in the message.
    pub forward: Option<Vec3>,
    pub shell: Option<Entity>,
    pub host: Option<Entity>,
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
    };
    let mut e = commands.spawn((fx, local, GlobalTransform::from(world), Visibility::default()));
    if let Some(p) = parent {
        e.insert(ChildOf(p));
    }
    if transient {
        e.insert(Transient);
    }
    let entity = e.id();
    let started =
        EffectStarted { entity, effect: eff, at: world.translation, forward: origin.forward.unwrap_or_else(|| world.forward().as_vec3()), shell: origin.shell, host: origin.host };
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
                let told = spawn_with(&mut commands, &lib, "A", None, node, node, true, Origin { forward: Some(Vec3::Y), shell: Some(shell), host: Some(host) }).unwrap();
                assert!(spawn(&mut commands, &lib, "Unknown", None, node, node, true).is_none());
                (plain, told)
            })
            .unwrap();
        let sent: Vec<_> = world.resource_mut::<Messages<EffectStarted>>().drain().collect();
        assert_eq!(sent.len(), 2, "one per effect started, none for an unknown name");
        let plain = EffectStarted { entity: made.0, effect: 1, at: node.translation, forward: node.forward().as_vec3(), shell: None, host: None };
        assert_eq!(sent[0], plain, "no shell: the node's own forward");
        assert_eq!(sent[1], EffectStarted { entity: made.1, effect: 0, forward: Vec3::Y, shell: Some(shell), host: Some(host), ..plain });
    }
}
