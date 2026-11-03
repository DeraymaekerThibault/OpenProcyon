//! Not retail: material tweaks of the render profile (part of render/mod.rs's look): additive effects and shells pushed
//! past 1 so the bloom catches them (`fx_gain`, `shell_gain`), blended particles lit by the scene (`lit_smoke`), and the
//! rule that effects, wakes and other unlit or see-through meshes cast no shadow.

use bevy::gltf::GltfMaterialName;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::Look;
use crate::fx::{EffectMaterial, ParticleGroup};

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (fx_gain, shell_glow, no_effect_shadows, lit_smoke));
}

fn fx_gain(look: Res<Look>, mut materials: ResMut<Assets<EffectMaterial>>) {
    let want = Color::linear_rgb(look.fx_gain, look.fx_gain, look.fx_gain);
    let stale: Vec<_> = materials.iter().filter(|(_, m)| m.base.base_color != want).map(|(id, _)| id).collect();
    for id in stale {
        materials.get_mut(id).unwrap().base.base_color = want;
    }
}

/// The shells' spheres are unlit: past 1 they glow through the bloom like hot shot.
fn shell_glow(look: Res<Look>, shells: Option<Res<crate::combat::Look>>, mut materials: ResMut<Assets<StandardMaterial>>, mut done: Local<bool>) {
    let Some(shells) = shells else { return };
    if *done {
        return;
    }
    *done = true;
    for h in [&shells.shell, &shells.enemy_shell] {
        if let Some(mut m) = materials.get_mut(h) {
            m.base_color = Color::from(m.base_color.to_linear() * look.shell_gain);
        }
    }
}

/// Effects, wakes and other unlit or see-through meshes cast no shadow: several are built empty and filled each frame,
/// which the shadow pass cannot draw, and a flash or a smoke puff would not shade a hull anyway. Sails the profile makes
/// solid are left to `sails`, which keeps their shadow.
fn no_effect_shadows(
    mut commands: Commands,
    standard: Res<Assets<StandardMaterial>>,
    look: Res<Look>,
    plain: Query<(Entity, &MeshMaterial3d<StandardMaterial>, Option<&GltfMaterialName>), (Added<MeshMaterial3d<StandardMaterial>>, Without<NotShadowCaster>)>,
    additive: Query<Entity, (Added<MeshMaterial3d<EffectMaterial>>, Without<NotShadowCaster>)>,
) {
    for (e, m, name) in &plain {
        if look.sails.as_ref().is_some_and(|s| s.opaque && name.is_some_and(|n| s.matches(n))) {
            continue;
        }
        if standard.get(&m.0).is_some_and(|m| m.unlit || !matches!(m.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_))) {
            commands.entity(e).try_insert(NotShadowCaster);
        }
    }
    for e in &additive {
        commands.entity(e).try_insert(NotShadowCaster);
    }
}

/// Smoke, dust and debris particles take the scene's light (sun, sky, flashes), so an explosion lights its own smoke.
fn lit_smoke(look: Res<Look>, groups: Query<(&ParticleGroup, &MeshMaterial3d<StandardMaterial>), Added<ParticleGroup>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    if !look.lit_smoke {
        return;
    }
    for (g, m) in &groups {
        if g.additive {
            continue;
        }
        if let Some(mut m) = materials.get_mut(&m.0)
            && m.alpha_mode == AlphaMode::Blend
        {
            m.unlit = false;
            m.perceptual_roughness = 1.0;
            m.reflectance = 0.0;
        }
    }
}
