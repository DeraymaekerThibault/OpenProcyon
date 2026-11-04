//! Not retail: surface detail of the render profile (part of render/mod.rs's look). The retail models carry only colour
//! textures; each textured glTF material (hulls, decks, metal fittings, islands, ...) gets the retail StandardMaterial
//! extended by the mod's `shaders/surface.wesl`, which derives relief from the colour texture as it draws (nothing is
//! generated or stored, so a mod ships no maps made from game art). The profile's rules, matched against the glTF
//! material name (the retail texture name), set how rough or metallic each surface is and how strong the shader's
//! relief, crevice shading and gloss are (`SurfaceUniform`). Sails are left to `sails`.
//!
//! A cloaking ship's hull is faded by cloak.rs, which works on StandardMaterials: while it is see-through its meshes go
//! back to their retail material and come back after (`Restyled`); the same holds for sails.

use bevy::gltf::GltfMaterialName;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use serde_json::Value;
use std::collections::HashMap;

use super::{Look, names, num};

/// The shader's asset path; the mod ships it.
pub const SHADER: &str = "shaders/surface.wesl";

#[derive(Clone, Debug, PartialEq)]
pub struct Surfaces {
    /// First rule with a name fragment in the material's name wins; `default` for the rest.
    pub rules: Vec<(Vec<String>, Surface)>,
    pub default: Surface,
    /// Materials left retail.
    pub skip: Vec<String>,
    /// Light along the silhouettes (rgb, linear) and its strength: lifts ships off the dark sky.
    pub rim: Vec4,
}

/// One kind of surface: StandardMaterial roughness, metallic and reflectance, and the shader's numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    pub roughness: f32,
    pub metallic: f32,
    pub reflectance: f32,
    /// Relief strength.
    pub bump: f32,
    /// How much darker the crevices are.
    pub cavity: f32,
    /// How much smoother the brighter parts of the texture are than the darker.
    pub gloss: f32,
}

fn surface(v: &Value, d: Surface) -> Surface {
    Surface {
        roughness: num(&v["roughness"], d.roughness).clamp(0.02, 1.0),
        metallic: num(&v["metallic"], d.metallic).clamp(0.0, 1.0),
        reflectance: num(&v["reflectance"], d.reflectance),
        bump: num(&v["bump"], d.bump),
        cavity: num(&v["cavity"], d.cavity),
        gloss: num(&v["gloss"], d.gloss),
    }
}

pub fn parse(v: &Value) -> Option<Surfaces> {
    v.is_object().then(|| {
        let default = surface(&v["default"], Surface { roughness: 0.8, metallic: 0.0, reflectance: 0.4, bump: 1.0, cavity: 0.3, gloss: 0.2 });
        Surfaces {
            rules: v["rules"].as_array().into_iter().flatten().map(|r| (names(&r["match"], &[]), surface(r, default))).filter(|(m, _)| !m.is_empty()).collect(),
            default,
            skip: names(&v["skip"], &[]),
            rim: super::rgb(&v["rim"]).unwrap_or(Vec3::new(0.6, 0.7, 1.0)).extend(num(&v["rim_strength"], 0.0)),
        }
    })
}

impl Surfaces {
    /// The surface for a glTF material name; none for a skipped one.
    pub fn of(&self, name: &str) -> Option<Surface> {
        let n = name.to_lowercase();
        if self.skip.iter().any(|s| n.contains(s.as_str())) {
            return None;
        }
        Some(self.rules.iter().find(|(m, _)| m.iter().any(|f| n.contains(f.as_str()))).map_or(self.default, |(_, s)| *s))
    }
}

/// What the shader gets at `@group(MATERIAL_BIND_GROUP) @binding(100)`, besides the StandardMaterial bindings.
#[derive(Clone, Copy, Default, Debug, ShaderType)]
pub struct SurfaceUniform {
    /// x: relief, y: crevice shading, z: gloss, w: unused
    pub params: Vec4,
    /// rgb: rim light colour, a: its strength
    pub rim: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct SurfaceDetail {
    #[uniform(100)]
    pub u: SurfaceUniform,
}

impl MaterialExtension for SurfaceDetail {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

pub type SurfaceMaterial = ExtendedMaterial<StandardMaterial, SurfaceDetail>;

pub fn plugin(app: &mut App, look: &Look) {
    if look.surfaces.is_some() {
        app.add_plugins(MaterialPlugin::<SurfaceMaterial>::default()).add_systems(Update, (surfaces, cloaked::<SurfaceMaterial>).chain());
    }
}

/// A mesh the profile gave a material of its own: its retail material and its own, and the ship it belongs to.
#[derive(Component)]
pub(super) struct Restyled<M: Material> {
    pub original: Handle<StandardMaterial>,
    pub styled: Handle<M>,
    pub ship: Option<Entity>,
}

/// Textured, lit, not see-through retail materials get a surface material, one per retail material (instances share it).
fn surfaces(
    mut commands: Commands,
    look: Res<Look>,
    new: Query<(Entity, &GltfMaterialName, &MeshMaterial3d<StandardMaterial>), (Added<MeshMaterial3d<StandardMaterial>>, Without<Restyled<SurfaceMaterial>>)>,
    parents: Query<&ChildOf>,
    ships: Query<(), With<crate::Ship>>,
    standard: Res<Assets<StandardMaterial>>,
    mut surface_mats: ResMut<Assets<SurfaceMaterial>>,
    mut made: Local<HashMap<AssetId<StandardMaterial>, Handle<SurfaceMaterial>>>,
) {
    let Some(s) = &look.surfaces else { return };
    for (e, name, mat) in &new {
        if look.sails.as_ref().is_some_and(|sails| sails.matches(name)) {
            continue;
        }
        let Some(surface) = s.of(&name.0) else { continue };
        let Some(base) = standard.get(&mat.0) else { continue };
        if base.unlit || base.base_color_texture.is_none() || !matches!(base.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)) {
            continue;
        }
        let styled = made
            .entry(mat.0.id())
            .or_insert_with(|| {
                let base = StandardMaterial { perceptual_roughness: surface.roughness, metallic: surface.metallic, reflectance: surface.reflectance, ..base.clone() };
                surface_mats.add(SurfaceMaterial {
                    base,
                    extension: SurfaceDetail { u: SurfaceUniform { params: Vec4::new(surface.bump, surface.cavity, surface.gloss, 0.0), rim: s.rim } },
                })
            })
            .clone();
        let ship = parents.iter_ancestors(e).find(|a| ships.contains(*a));
        commands.entity(e).try_remove::<MeshMaterial3d<StandardMaterial>>().try_insert((MeshMaterial3d(styled.clone()), Restyled { original: mat.0.clone(), styled, ship }));
    }
}

/// While the local player sees a ship's hull faded by a cloak, its restyled meshes wear their retail material (which
/// cloak.rs fades), and their own again once it is solid.
pub(super) fn cloaked<M: Material>(mut commands: Commands, cloaks: Query<&crate::cloak::Cloak>, meshes: Query<(Entity, &Restyled<M>, Has<MeshMaterial3d<M>>)>) {
    for (e, r, styled) in &meshes {
        let faded = r.ship.and_then(|s| cloaks.get(s).ok()).is_some_and(|c| c.alpha_for(crate::Side::Player) < 1.0);
        if faded && styled {
            commands.entity(e).try_remove::<MeshMaterial3d<M>>().try_insert(MeshMaterial3d(r.original.clone()));
        } else if !faded && !styled {
            commands.entity(e).try_remove::<MeshMaterial3d<StandardMaterial>>().try_insert(MeshMaterial3d(r.styled.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surfaces_take_the_first_matching_rule_by_material_name() {
        let s = parse(&serde_json::json!({
            "default": {"roughness": 0.9},
            "rules": [{"match": ["metal", "gold"], "metallic": 0.8, "roughness": 0.35}, {"match": ["wood"], "bump": 2}],
            "skip": ["water"],
        }))
        .unwrap();
        let gold = s.of("Gold Rail.tga").unwrap();
        assert_eq!((gold.metallic, gold.roughness, gold.bump), (0.8, 0.35, 1.0), "unset keys come from the default");
        assert_eq!(s.of("plain_dark_wood.tga").map(|w| (w.bump, w.roughness)), Some((2.0, 0.9)));
        assert_eq!(s.of("piratehull1.tga"), Some(s.default));
        assert!(s.of("Water.tga").is_none());
        assert!(parse(&Value::Null).is_none());
    }
}
