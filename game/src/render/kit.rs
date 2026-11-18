//! Not retail: the pieces every render block repeats, in one place. A block that restyles a retail mesh swaps its material
//! (`swap`), draws added light like the retail ONE/ONE effects (`additive`, `additive_base`) or stays behind the world without
//! writing depth (`no_depth_write`). New blocks use these instead of their own copies.

use bevy::ecs::system::EntityCommands;
use bevy::prelude::*;
use bevy::render::render_resource::{BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor};

/// Retail ONE/ONE: the shader writes unpremultiplied light and it is only ever added; no depth bias of its own.
pub fn additive(descriptor: &mut RenderPipelineDescriptor) {
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

/// Depth-tested, never written: something drawn behind or under the world that must not hide what comes after it.
pub fn no_depth_write(descriptor: &mut RenderPipelineDescriptor) {
    if let Some(depth) = descriptor.depth_stencil.as_mut() {
        depth.depth_write_enabled = Some(false);
    }
}

/// Unlit, two-sided, drawn after the scene's blended meshes like the retail additive effects.
pub fn additive_base(texture: Option<Handle<Image>>) -> StandardMaterial {
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

/// Puts material `to` on a mesh in place of its `From` material, keeping the mesh; the entity may be gone by the time the
/// commands apply (short effects), so nothing fails. Returns the entity's commands for whatever the block marks it with.
pub fn swap<'a, From: Material, To: Material>(commands: &'a mut Commands, e: Entity, to: Handle<To>) -> EntityCommands<'a> {
    let mut entity = commands.entity(e);
    entity.try_remove::<MeshMaterial3d<From>>().try_insert(MeshMaterial3d(to));
    entity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_swap_replaces_the_material_and_keeps_the_rest() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default())).init_asset::<StandardMaterial>().init_asset::<Mesh>();
        let (mesh, old, new) = {
            let w = app.world_mut();
            let mesh = w.resource_mut::<Assets<Mesh>>().add(Mesh::from(Cuboid::default()));
            let mut mats = w.resource_mut::<Assets<StandardMaterial>>();
            (mesh, mats.add(StandardMaterial::default()), mats.add(StandardMaterial::default()))
        };
        let e = app.world_mut().spawn((Mesh3d(mesh.clone()), MeshMaterial3d(old))).id();
        let target = new.clone();
        app.world_mut()
            .run_system_once(move |mut c: Commands| {
                swap::<StandardMaterial, StandardMaterial>(&mut c, e, target.clone()).try_insert(Name::new("styled"));
            })
            .unwrap();
        let w = app.world();
        assert_eq!(w.entity(e).get::<MeshMaterial3d<StandardMaterial>>().map(|m| m.0.id()), Some(new.id()));
        assert_eq!(w.entity(e).get::<Mesh3d>().map(|m| m.0.id()), Some(mesh.id()));
        assert!(w.entity(e).contains::<Name>(), "the caller's marker goes on too");
    }

    use bevy::ecs::system::RunSystemOnce;
}
