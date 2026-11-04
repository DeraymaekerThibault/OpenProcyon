//! Not retail: the generic `materials` block of the render profile (part of render/mod.rs's look). A rule matches glTF
//! material names (the retail texture names, as `surfaces.rules` does) and gives the matching retail StandardMaterials a
//! shader of the mod's choosing, so a new kind of restyled material is a rule and a shader file in the mod, not a new Rust
//! block. Every rule uses the same extension and the same uniform (`RuleUniform`); only the fragment shader differs.
//!
//! Which shader a rule draws with is chosen in the pipeline's specialisation: the extension names its shader by a slot
//! number, which is part of the material's key (`RuleKey`; Bevy needs the key `Copy`, so it cannot hold the handle), and
//! `specialize` swaps that slot's shader into the main-pass fragment stage, so each distinct shader gets its own pipeline
//! and the prepass and shadow passes stay Bevy's. That needs one Rust type for any number of shaders, where a type per
//! shader path would have to be fixed at compile time.
//!
//! A mesh the block restyles gets one material asset per (retail material, rule, ship): the instances of one ship's
//! material share it and batch, while the ship's values (`RuleUniform::ship`) can differ from the next ship's. Meshes of
//! no ship share one asset per retail material. A cloaking ship's hull is faded by cloak.rs, which works on
//! StandardMaterials: while it is see-through its meshes wear their retail material and come back after (`Restyled`).
//! A material a rule claims is not offered to `sails` or `surfaces`.

use bevy::gltf::GltfMaterialName;
use bevy::light::NotShadowCaster;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::Shader;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use super::names;
use super::surfaces::Restyled;

/// The `materials` block: rules in order, and the names left retail.
#[derive(Clone, Debug, PartialEq)]
pub struct Materials {
    pub rules: Vec<Rule>,
    /// Name fragments of materials no rule may restyle.
    pub skip: Vec<String>,
}

/// One rule: the materials it matches, the shader they get and the numbers handed to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    /// glTF material name fragments (lower case).
    pub matching: Vec<String>,
    /// The fragment shader's asset path in the mod.
    pub shader: String,
    /// The shader's own numbers.
    pub params: Vec4,
    pub params2: Vec4,
    /// StandardMaterial overrides; `None` keeps the retail value.
    pub roughness: Option<f32>,
    pub metallic: Option<f32>,
    pub reflectance: Option<f32>,
    /// Solid instead of retail's blended or masked material.
    pub opaque: bool,
    /// Whether the meshes cast shadows (when the sun has them on).
    pub shadows: bool,
}

/// Up to four numbers; missing ones are 0.
fn vec4(v: &Value) -> Vec4 {
    let a = v.as_array();
    Vec4::from_array(std::array::from_fn(|i| a.and_then(|a| a.get(i)).and_then(Value::as_f64).unwrap_or(0.0) as f32))
}

fn rule(v: &Value) -> Option<Rule> {
    let matching = names(&v["match"], &[]);
    let shader = v["shader"].as_str().map(str::to_string)?;
    let safe = !shader.starts_with('/') && !shader.starts_with('\\') && !shader.contains("..") && !shader.contains(':');
    if matching.is_empty() || !safe {
        warn!("render profile: a `materials` rule needs `match` names and a `shader` path inside the mod; ignored");
        return None;
    }
    let opt = |k: &str| v[k].as_f64().map(|x| x as f32);
    Some(Rule {
        matching,
        shader,
        params: vec4(&v["params"]),
        params2: vec4(&v["params2"]),
        roughness: opt("roughness").map(|x| x.clamp(0.02, 1.0)),
        metallic: opt("metallic").map(|x| x.clamp(0.0, 1.0)),
        reflectance: opt("reflectance"),
        opaque: v["opaque"].as_bool().unwrap_or(false),
        shadows: v["shadows"].as_bool().unwrap_or(true),
    })
}

pub fn parse(v: &Value) -> Option<Materials> {
    v.is_object().then(|| Materials { rules: v["rules"].as_array().into_iter().flatten().filter_map(rule).collect(), skip: names(&v["skip"], &[]) })
}

impl Materials {
    /// The index of the first rule matching a glTF material name; none for a skipped name.
    pub fn find(&self, name: &str) -> Option<usize> {
        let n = name.to_lowercase();
        if self.skip.iter().any(|s| n.contains(s.as_str())) {
            return None;
        }
        self.rules.iter().position(|r| r.matching.iter().any(|f| n.contains(f.as_str())))
    }

    /// The block without the rules whose shader the overlay (`root`) lacks (each missing file is warned about once); `None`
    /// when no rule is left, so the block is off.
    pub fn with_shaders(mut self, root: &Path) -> Option<Materials> {
        let mut checked: HashMap<String, bool> = HashMap::new();
        self.rules.retain(|r| *checked.entry(r.shader.clone()).or_insert_with(|| super::has_shader(root, "materials", &r.shader)));
        (!self.rules.is_empty()).then_some(self)
    }
}

/// What every rule's shader gets at `@group(MATERIAL_BIND_GROUP) @binding(100)`, besides the StandardMaterial bindings.
#[derive(Clone, Copy, Default, Debug, PartialEq, ShaderType)]
pub struct RuleUniform {
    /// the rule's `params`
    pub params: Vec4,
    /// the rule's `params2`
    pub params2: Vec4,
    /// x: elapsed seconds, y: the frame's delta
    pub clock: Vec4,
    /// xyz: the direction sunlight travels (world), w: its illuminance in units of 10000 lux
    pub sun: Vec4,
    /// x: the owning ship's smoothed share of full speed (0..1), y: its heading (radians), z: a seed 0..1 of the ship;
    /// all 0 for a mesh of no ship
    pub ship: Vec4,
}

/// The shaders rules have asked for, by asset path; a material names one by its index (`RuleShader::slot`). The pipeline
/// key has to be `Copy`, so it holds the index and `specialize` (in the render world, hence a static) looks the handle up.
/// The list only grows by one entry per distinct shader of the profile and keeps the handles alive.
static SHADERS: Mutex<Vec<(String, Handle<Shader>)>> = Mutex::new(Vec::new());

fn shader_slot(server: &AssetServer, path: &str) -> u32 {
    let mut shaders = SHADERS.lock().unwrap_or_else(PoisonError::into_inner);
    let i = shaders.iter().position(|(p, _)| p == path).unwrap_or_else(|| {
        shaders.push((path.to_string(), server.load::<Shader>(path.to_string())));
        shaders.len() - 1
    });
    i as u32
}

/// Which shader a material draws with: part of the pipeline key.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuleKey(u32);

impl From<&RuleShader> for RuleKey {
    fn from(e: &RuleShader) -> RuleKey {
        RuleKey(e.slot)
    }
}

/// The extension every rule uses; see `RuleUniform`.
#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
#[bind_group_data(RuleKey)]
pub struct RuleShader {
    #[uniform(100)]
    pub u: RuleUniform,
    /// Index into `SHADERS`.
    pub slot: u32,
}

/// The label Bevy gives the prepass and shadow pipelines, which keep Bevy's shaders.
const PREPASS: &str = "prepass_pipeline";

impl MaterialExtension for RuleShader {
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &bevy::mesh::MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if descriptor.label.as_deref() != Some(PREPASS)
            && let Some(fragment) = descriptor.fragment.as_mut()
            && let Some((_, shader)) = SHADERS.lock().unwrap_or_else(PoisonError::into_inner).get(key.bind_group_data.0 as usize)
        {
            fragment.shader = shader.clone();
        }
        Ok(())
    }
}

pub type RuleMaterial = ExtendedMaterial<StandardMaterial, RuleShader>;

pub fn plugin(app: &mut App, look: &super::Look) {
    if look.materials.is_some() {
        // before `sails` and `surfaces`, so a material a rule claims is no longer a StandardMaterial when they look
        app.add_plugins(MaterialPlugin::<RuleMaterial>::default())
            .init_resource::<Made>()
            .add_systems(Update, (restyle.before(super::sails::sails).before(super::surfaces::surfaces), motion, super::surfaces::cloaked::<RuleMaterial>).chain());
    }
}

/// What a material asset was made for.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Key {
    material: AssetId<StandardMaterial>,
    rule: usize,
    ship: Option<Entity>,
}

/// The material assets in use, weakly (an asset goes when no mesh wears it), and each ship's smoothed speed share.
#[derive(Resource, Default)]
struct Made {
    assets: HashMap<Key, AssetId<RuleMaterial>>,
    wind: HashMap<Entity, f32>,
}

/// The rule's material for a retail one: its StandardMaterial with the overrides, and the rule's shader and numbers.
fn style(rule: &Rule, retail: &StandardMaterial, slot: u32) -> RuleMaterial {
    let mut base = retail.clone();
    base.perceptual_roughness = rule.roughness.unwrap_or(base.perceptual_roughness);
    base.metallic = rule.metallic.unwrap_or(base.metallic);
    base.reflectance = rule.reflectance.unwrap_or(base.reflectance);
    if rule.opaque {
        base.alpha_mode = AlphaMode::Opaque;
        base.base_color = base.base_color.with_alpha(1.0);
    }
    ExtendedMaterial { base, extension: RuleShader { u: RuleUniform { params: rule.params, params2: rule.params2, ..default() }, slot } }
}

/// Materials a rule matches get its material, one per retail material, rule and ship.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn restyle(
    mut commands: Commands,
    look: Res<super::Look>,
    new: Query<(Entity, &GltfMaterialName, &MeshMaterial3d<StandardMaterial>), (Added<MeshMaterial3d<StandardMaterial>>, Without<Restyled<RuleMaterial>>)>,
    parents: Query<&ChildOf>,
    ships: Query<(), With<crate::Ship>>,
    standard: Res<Assets<StandardMaterial>>,
    mut rule_mats: ResMut<Assets<RuleMaterial>>,
    mut made: ResMut<Made>,
    server: Res<AssetServer>,
) {
    let Some(m) = &look.materials else { return };
    for (e, name, mat) in &new {
        let Some(i) = m.find(&name.0) else { continue };
        let Some(retail) = standard.get(&mat.0) else { continue };
        let rule = &m.rules[i];
        let ship = parents.iter_ancestors(e).find(|a| ships.contains(*a));
        let key = Key { material: mat.0.id(), rule: i, ship };
        let styled = match made.assets.get(&key).and_then(|id| rule_mats.get_strong_handle(*id)) {
            Some(h) => h,
            None => {
                let h = rule_mats.add(style(rule, retail, shader_slot(&server, &rule.shader)));
                made.assets.insert(key, h.id());
                h
            }
        };
        let mut entity = commands.entity(e);
        entity.try_remove::<MeshMaterial3d<StandardMaterial>>().try_insert((MeshMaterial3d(styled.clone()), Restyled { original: mat.0.clone(), styled, ship }));
        if !rule.shadows {
            entity.try_insert(NotShadowCaster);
        }
    }
}

/// Fills the clock, the sun and each ship's values into the materials in use, and forgets those no mesh wears any more.
fn motion(
    time: Res<Time>,
    lib: Option<Res<crate::combat::Library>>,
    ships: Query<(Entity, &crate::Ship)>,
    suns: Query<(&DirectionalLight, &GlobalTransform)>,
    mut made: ResMut<Made>,
    mut mats: ResMut<Assets<RuleMaterial>>,
) {
    let made = &mut *made;
    made.assets.retain(|_, id| mats.contains(*id));
    let dt = time.delta_secs();
    made.wind.retain(|e, _| ships.contains(*e));
    if let Some(lib) = lib {
        for (e, sh) in &ships {
            let share = (sh.speed.abs() / crate::ship::full_speed(&lib.classes[sh.class]).max(1e-3)).clamp(0.0, 1.0);
            let wind = made.wind.entry(e).or_insert(share);
            *wind += (share - *wind) * (dt * 1.2).min(1.0);
        }
    }
    let clock = Vec4::new(time.elapsed_secs(), dt, 0.0, 0.0);
    let sun = suns.iter().next().map_or(Vec4::new(0.0, -1.0, 0.0, 1.0), |(l, t)| t.forward().as_vec3().extend(l.illuminance / bevy::light::light_consts::lux::AMBIENT_DAYLIGHT));
    for (key, id) in &made.assets {
        let ship = key
            .ship
            .and_then(|e| ships.get(e).ok().map(|(_, sh)| (e, sh)))
            .map_or(Vec4::ZERO, |(e, sh)| Vec4::new(made.wind.get(&e).copied().unwrap_or(0.0), sh.heading, (e.index().index() as f32 * 0.618).fract(), 0.0));
        if let Some(mut m) = mats.get_mut(*id) {
            m.extension.u.clock = clock;
            m.extension.u.sun = sun;
            m.extension.u.ship = ship;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Look;

    fn block() -> Materials {
        parse(&serde_json::json!({
            "rules": [
                {"match": ["Hull", "deck"], "shader": "shaders/hull.wesl", "params": [0.35, 0.15, 0.2], "roughness": 0.75, "opaque": true, "shadows": false},
                {"match": ["hull_special"], "shader": "shaders/other.wesl"},
                {"match": ["rock"], "shader": "shaders/rock.wesl", "params2": [1, 2]},
            ],
            "skip": ["Water"],
        }))
        .unwrap()
    }

    #[test]
    fn rules_match_material_names_first_wins_and_skip_leaves_retail() {
        let m = block();
        assert_eq!(m.find("piratehull1.tga"), Some(0));
        assert_eq!(m.find("Deck_Planks.tga"), Some(0));
        assert_eq!(m.find("hull_special.tga"), Some(0), "the first rule that matches, not the best");
        assert_eq!(m.find("Rock_cliff.tga"), Some(2));
        assert_eq!(m.find("sail.tga"), None, "no rule, stays retail");
        assert_eq!(m.find("water_hull.tga"), None, "skip beats every rule");
    }

    #[test]
    fn every_rule_key_is_optional_and_bad_rules_are_dropped() {
        let m = block();
        let r = &m.rules[0];
        assert_eq!(
            (r.params, r.params2, r.roughness, r.metallic, r.reflectance, r.opaque, r.shadows),
            (Vec4::new(0.35, 0.15, 0.2, 0.0), Vec4::ZERO, Some(0.75), None, None, true, false)
        );
        let r = &m.rules[1];
        assert_eq!((r.params, r.roughness, r.opaque, r.shadows), (Vec4::ZERO, None, false, true), "retail values and shadows unless the rule says otherwise");
        let m = parse(&serde_json::json!({"rules": [
            {"match": ["a"]}, {"shader": "shaders/x.wesl"}, {"match": [], "shader": "shaders/x.wesl"},
            {"match": ["a"], "shader": "../x.wesl"}, {"match": ["a"], "shader": "/etc/x.wesl"}, {"match": ["a"], "shader": "shaders/ok.wesl"},
        ]}))
        .unwrap();
        assert_eq!(m.rules.len(), 1, "a rule needs names and a shader inside the mod");
        assert!(parse(&serde_json::json!({})).unwrap().rules.is_empty() && parse(&Value::Null).is_none());
    }

    #[test]
    fn a_rule_whose_shader_the_mod_does_not_ship_stays_off() {
        let root = std::env::temp_dir().join(format!("procyon-rules-{}", std::process::id()));
        std::fs::create_dir_all(root.join("shaders")).unwrap();
        std::fs::write(root.join("shaders/hull.wesl"), "").unwrap();
        let m = block().with_shaders(&root).unwrap();
        assert_eq!(m.rules.len(), 1, "only the rule whose shader is there");
        assert_eq!((m.find("hull.tga"), m.find("rock.tga")), (Some(0), None));
        assert!(parse(&serde_json::json!({"rules": [{"match": ["a"], "shader": "shaders/none.wesl"}]})).unwrap().with_shaders(&root).is_none(), "no rule left: the block is off");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn without_a_materials_key_classic_is_untouched() {
        let look = Look::parse(&serde_json::json!({"surfaces": {}}));
        assert!(look.materials.is_none());
        let mut app = App::new();
        plugin(&mut app, &look);
        assert!(app.world().get_resource::<Made>().is_none(), "no systems, no resources");
    }

    fn app(block: &Value) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Shader>()
            .init_asset::<StandardMaterial>()
            .init_asset::<RuleMaterial>()
            .init_resource::<Made>()
            .insert_resource(Look::parse(&serde_json::json!({"materials": block})))
            .add_systems(Update, restyle);
        app
    }

    fn ship() -> crate::Ship {
        crate::Ship {
            class: 0,
            side: crate::Side::Player,
            heading: 0.0,
            speed: 0.0,
            ang_vel: 0.0,
            order: 0,
            waypoints: vec![],
            goal: None,
            cap: 1.0,
            throttle: None,
            sail_energy: 0.0,
            crew_speed: 1.0,
            crew_turn: 1.0,
            net: 1.0,
            avoid: 1.0,
        }
    }

    #[test]
    fn one_asset_per_retail_material_and_ship_not_one_per_mesh() {
        let mut app = app(&serde_json::json!({"rules": [{"match": ["hull", "deck"], "shader": "shaders/hull.wesl", "roughness": 0.5}], "skip": ["water"]}));
        let mut std_mats = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
        let (hull, deck, water) = (std_mats.add(StandardMaterial::default()), std_mats.add(StandardMaterial::default()), std_mats.add(StandardMaterial::default()));
        let mesh = |app: &mut App, name: &str, m: &Handle<StandardMaterial>, parent: Option<Entity>| {
            let e = app.world_mut().spawn((GltfMaterialName(name.into()), MeshMaterial3d(m.clone()))).id();
            if let Some(p) = parent {
                app.world_mut().entity_mut(e).insert(ChildOf(p));
            }
            e
        };
        let (a, b) = (app.world_mut().spawn(ship()).id(), app.world_mut().spawn(ship()).id());
        let island: Vec<Entity> = (0..3).map(|_| mesh(&mut app, "hull.tga", &hull, None)).collect();
        let on_a: Vec<Entity> = (0..3).map(|_| mesh(&mut app, "hull.tga", &hull, Some(a))).collect();
        let on_b = mesh(&mut app, "hull.tga", &hull, Some(b));
        let on_a_deck = mesh(&mut app, "deck.tga", &deck, Some(a));
        let wet = mesh(&mut app, "water.tga", &water, None);
        app.update();
        let handle = |app: &App, e: Entity| app.world().entity(e).get::<MeshMaterial3d<RuleMaterial>>().map(|m| m.0.id());
        assert!(island.iter().all(|e| handle(&app, *e) == handle(&app, island[0])) && handle(&app, island[0]).is_some(), "meshes of no ship share one asset");
        assert!(on_a.iter().all(|e| handle(&app, *e) == handle(&app, on_a[0])) && handle(&app, on_a[0]).is_some(), "one ship's meshes share one");
        let ids = [handle(&app, island[0]), handle(&app, on_a[0]), handle(&app, on_b), handle(&app, on_a_deck)];
        assert!(ids.iter().all(Option::is_some) && ids.iter().collect::<std::collections::HashSet<_>>().len() == 4, "another ship or material, another asset");
        assert_eq!(app.world().resource::<Assets<RuleMaterial>>().len(), 4);
        assert!(handle(&app, wet).is_none() && app.world().entity(wet).contains::<MeshMaterial3d<StandardMaterial>>(), "skipped: stays retail");
        let restyled = app.world().entity(on_a[0]).get::<Restyled<RuleMaterial>>().unwrap();
        assert_eq!((restyled.original.id(), restyled.ship), (hull.id(), Some(a)), "cloaked::<M> can put the retail material back");
        let m = app.world().resource::<Assets<RuleMaterial>>().get(handle(&app, on_a[0]).unwrap()).unwrap();
        assert_eq!(m.base.perceptual_roughness, 0.5);
    }

    #[test]
    fn the_rule_sets_opaque_and_shadows() {
        let rule = &block().rules[0];
        let retail = StandardMaterial { alpha_mode: AlphaMode::Blend, base_color: Color::srgba(1.0, 1.0, 1.0, 0.3), metallic: 0.6, ..default() };
        let m = style(rule, &retail, 0);
        assert_eq!((m.base.alpha_mode, m.base.base_color.alpha(), m.base.perceptual_roughness, m.base.metallic), (AlphaMode::Opaque, 1.0, 0.75, 0.6));
        assert_eq!(m.extension.u.params, rule.params);
        let mut app = app(&serde_json::json!({"rules": [{"match": ["hull"], "shader": "shaders/hull.wesl", "shadows": false}]}));
        let mat = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial::default());
        let e = app.world_mut().spawn((GltfMaterialName("hull".into()), MeshMaterial3d(mat))).id();
        app.update();
        assert!(app.world().entity(e).contains::<NotShadowCaster>());
    }
}
