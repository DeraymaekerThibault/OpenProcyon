//! Not retail: render extensions a graphics mod switches on with a render profile, `data/render/look.json` in the overlay.
//! Absent, everything renders as classic. A profile changes how the battle is drawn, never what happens. The engine
//! provides the building blocks; the mod tunes them in the profile and ships the shaders the blocks draw with
//! (`shaders/*.wesl` in the mod); a block whose shader is missing stays off. Reference: docs/modding.md "Graphics mods".
//!
//! - here: the camera (HDR, bloom, tonemapping, grading, SMAA, ambient occlusion), sun shadows, and the baked HD sky
//!   (`view::hd_sky`) as image-based light standing in for the flat ambient light;
//! - `frame.rs`: the frame data every mod shader can read at `@binding(99)` (clock, sun, camera, the storm's last bolt, black holes);
//! - `kit.rs`: what every block repeats: swapping a retail material for the block's, additive (ONE/ONE) drawing, no depth write;
//! - `lights.rs`: point lights the profile puts on effects (flashes fade, fires and engines flicker);
//! - `materials.rs`: brighter additive effects and shells for the bloom, smoke lit by the scene, shadow rules for effects;
//! - `deckhands.rs`: crews on the decks, figures posed on the GPU from baked moves (`shaders/deckhands.wesl`, `data/render/deckhand.json`);
//! - `sails.rs`: sail materials with a shader (`shaders/sail.wesl`) and cloth that bellies out with the ship's speed;
//! - `trails.rs`: engine plumes (`shaders/plume.wesl`) and wakes drawn as crossed ribbons (`shaders/wake.wesl`);
//! - `tracers.rs`: streaks of light behind shells in flight (`shaders/tracer.wesl`);
//! - `surfaces.rs`: relief, roughness and metal for the retail models' textured materials (`shaders/surface.wesl`);
//! - `sky.rs`: a dome over the baked sky with a near layer of gas that slides against it (`shaders/sky.wesl`);
//! - `holes.rs`: black holes as a horizon, a bent sky and a spinning accretion disc (`shaders/blackhole.wesl`);
//! - `nebulae.rs`: nebula clouds and solar storms as churning, glowing gas inside their retail shell (`shaders/nebula.wesl`), or with
//!   `replace` as volumes (`shaders/volume.wesl`, `volnoise.rs`); `bolts.rs`: storm bolts as jagged ribbons (`shaders/bolt.wesl`);
//!   `replace.rs`: hides the retail drawing of what the profile replaces;
//! - `currents.rs`: Etherium currents as flowing streaks and sparkles (`shaders/current.wesl`);
//! - `rules.rs`: the generic `materials` block, any shader of the mod on any matched retail material, one uniform for all;
//! - `bursts.rs`: sparks, embers, fireballs, shockwaves and smoke thrown by retail effects, glowing hull scars, ether dust
//!   and the camera jolt with its colour fringing (`shaders/burst.wesl`, `shaders/smoke.wesl`).
//!
//! Flags are drawn by flag.rs; the profile only sets its `flag::Cloth`.

use bevy::anti_alias::smaa::{Smaa, SmaaPreset};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::light::{CascadeShadowConfigBuilder, EnvironmentMapLight, GeneratedEnvironmentMapLight, Skybox};
use bevy::pbr::ScreenSpaceAmbientOcclusion;
use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};
use bevy::prelude::*;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};
use serde_json::Value;

pub(crate) mod batch;
mod bolts;
mod bursts;
mod currents;
mod deckhands;
mod frame;
mod holes;
mod kit;
mod lights;
mod materials;
mod nebulae;
mod replace;
mod rules;
pub(crate) mod sails;
mod sky;
mod surfaces;
mod tracers;
mod trails;
mod volnoise;

/// The render profile's asset path.
pub const PROFILE: &str = "data/render/look.json";
/// Sky accents that content mods ship for skies of their own (`{"<sky>": {...sky keys...}}`, merged across mods): a mission mod
/// cannot add to a graphics mod's `look.json`, which replaces as a whole file.
pub const SKIES: &str = "data/render/skies.json";

/// Puts the mods' sky accents under the profile's `sky.skies`; the profile's own entry for a sky is merged over the mod's.
/// A profile without a `sky` block draws no dome, so nothing is added.
fn add_mod_skies(look: &mut serde_json::Value, skies: Option<serde_json::Value>) {
    let (Some(sky), Some(mut skies)) = (look.get_mut("sky").and_then(|s| s.as_object_mut()), skies.filter(|s| s.is_object())) else { return };
    if let Some(own) = sky.get("skies") {
        crate::modding::merge(&mut skies, own);
    }
    sky.insert("skies".into(), skies);
}

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Look {
    pub bloom: f32,
    /// Only light brighter than this (linear) blooms; 0 = everything a little (Bevy's natural bloom), which hazes the image.
    pub bloom_threshold: f32,
    /// Colour grading: saturation and contrast multipliers (1 = unchanged), exposure in stops.
    pub saturation: f32,
    pub contrast: f32,
    pub exposure: f32,
    pub tonemapping: Tonemapping,
    pub smaa: bool,
    pub ssao: bool,
    /// Image-based light from the baked sky, cd/m²; 0 = none.
    pub sky_light: f32,
    /// The sky dome (a near layer of gas with parallax) over the baked sky; none = off.
    pub sky: Option<sky::SkyDome>,
    /// Multiplies the flat ambient light, including on maps without a baked sky.
    pub ambient: f32,
    /// Minimum linear RGB for the map's flat ambient colour, before `ambient` scales its brightness.
    pub ambient_floor: Vec3,
    pub shadows: bool,
    pub shadow_distance: f32,
    /// Multiplies additive (ONE/ONE) effect colour, so flashes and glows exceed 1 and bloom.
    pub fx_gain: f32,
    /// Multiplies the colour of the shells' own spheres.
    pub shell_gain: f32,
    /// Smoke and other alpha-blended particles shaded by the scene's lights instead of drawn flat.
    pub lit_smoke: bool,
    pub lights: Vec<lights::EffectLight>,
    pub sails: Option<sails::Sails>,
    /// Crews on the decks of ships whose model marks their places (`Crew_*` nodes); none = empty decks, as classic.
    pub deckhands: Option<deckhands::Deckhands>,
    pub flags: crate::flag::Cloth,
    pub engines: Option<trails::Engines>,
    pub wakes: Option<trails::Wakes>,
    pub tracers: Option<tracers::Tracers>,
    pub surfaces: Option<surfaces::Surfaces>,
    pub bursts: Option<bursts::Bursts>,
    pub materials: Option<rules::Materials>,
    /// Black holes drawn as a horizon, lensed sky and accretion disc (`shaders/blackhole.wesl`); none = the retail ribbons only.
    pub black_holes: Option<holes::BlackHoles>,
    /// Nebula clouds and solar storms drawn as glowing, churning gas inside the retail shell (`shaders/nebula.wesl`).
    pub nebulae: Option<nebulae::Nebulae>,
    /// The bolts of storm lightning drawn as jagged, branching ribbons (`nebulae.bolt`, `shaders/bolt.wesl`); none = the retail sprite only.
    pub bolts: Option<bolts::Bolts>,
    /// Etherium currents drawn as flowing streaks and sparkles (`shaders/current.wesl`).
    pub currents: Option<currents::Currents>,
    /// Retail effects (lower-cased fragments of the effect's file name) whose quads are not drawn, for a mod that draws its own:
    /// the effect keeps running (particles, timers, lights, sounds), only its drawing goes (`replace.rs`).
    pub hide_effects: Vec<String>,
    /// Darkened screen corners (Bevy `Vignette` intensity; 0 = none).
    pub vignette: f32,
    /// Colour fringing at rest and how much a big blast's jolt adds.
    pub aberration: f32,
    pub aberration_pulse: f32,
    /// Screen-space contact shadows from the sun (fine shade where parts touch; needs a depth prepass).
    pub contact_shadows: bool,
    /// The sun's colour is multiplied by this and its strength by `sun_intensity`.
    pub sun_color: Vec3,
    pub sun_intensity: f32,
}

/// A number from the profile, or `d` when absent.
fn num(v: &Value, d: f32) -> f32 {
    v.as_f64().map_or(d, |x| x as f32)
}

/// An `[r, g, b]` colour (linear) from the profile.
fn rgb(v: &Value) -> Option<Vec3> {
    let c: Vec<f32> = v.as_array()?.iter().filter_map(|x| x.as_f64().map(|x| x as f32)).collect();
    (c.len() >= 3).then(|| Vec3::new(c[0], c[1], c[2]))
}

/// A list of lower-cased name fragments, or `default` when absent.
fn names(v: &Value, default: &[&str]) -> Vec<String> {
    match v.as_array() {
        Some(a) => a.iter().filter_map(|x| Some(x.as_str()?.to_lowercase())).collect(),
        None => default.iter().map(|s| s.to_string()).collect(),
    }
}

impl Look {
    /// The profile from its JSON; every key is optional.
    pub fn parse(v: &Value) -> Look {
        let cam = &v["camera"];
        let tonemapping = match cam["tonemapping"].as_str() {
            Some("agx") => Tonemapping::AgX,
            Some("aces") => Tonemapping::AcesFitted,
            Some("blender_filmic") => Tonemapping::BlenderFilmic,
            Some("reinhard_luminance") => Tonemapping::ReinhardLuminance,
            _ => Tonemapping::TonyMcMapface,
        };
        Look {
            bloom: num(&cam["bloom"], 0.15),
            bloom_threshold: num(&cam["bloom_threshold"], 0.0),
            saturation: num(&cam["saturation"], 1.0),
            contrast: num(&cam["contrast"], 1.0),
            exposure: num(&cam["exposure"], 0.0),
            tonemapping,
            smaa: cam["smaa"].as_bool().unwrap_or(true),
            ssao: cam["ssao"].as_bool().unwrap_or(false),
            sky_light: num(&v["sky_light"], 0.0),
            sky: sky::parse(&v["sky"]),
            ambient: num(&v["ambient"], 1.0),
            ambient_floor: rgb(&v["ambient_floor"]).unwrap_or(Vec3::ZERO).clamp(Vec3::ZERO, Vec3::ONE),
            shadows: v["sun"]["shadows"].as_bool().unwrap_or(false),
            shadow_distance: num(&v["sun"]["shadow_distance"], 2000.0),
            fx_gain: num(&v["fx_gain"], 1.0),
            shell_gain: num(&v["shell_gain"], 1.0),
            lit_smoke: v["lit_smoke"].as_bool().unwrap_or(false),
            lights: lights::parse(&v["lights"]),
            sails: sails::parse(&v["sails"]),
            deckhands: deckhands::parse(&v["deckhands"]),
            engines: trails::parse_engines(&v["engines"]),
            wakes: trails::parse_wakes(&v["wakes"]),
            tracers: tracers::parse(&v["tracers"]),
            surfaces: surfaces::parse(&v["surfaces"]),
            bursts: bursts::parse(&v["bursts"]),
            materials: rules::parse(&v["materials"]),
            black_holes: holes::parse(&v["black_holes"]),
            nebulae: nebulae::parse(&v["nebulae"]),
            bolts: bolts::parse(&v["nebulae"]["bolt"]),
            currents: currents::parse(&v["currents"]),
            hide_effects: names(&v["hide_effects"], &[]).into_iter().filter(|n| !n.is_empty()).collect(),
            vignette: num(&cam["vignette"], 0.0),
            aberration: num(&cam["aberration"], 0.0),
            aberration_pulse: num(&cam["aberration_pulse"], 0.0),
            contact_shadows: cam["contact_shadows"].as_bool().unwrap_or(false),
            sun_color: rgb(&v["sun"]["color"]).unwrap_or(Vec3::ONE),
            sun_intensity: num(&v["sun"]["intensity"], 1.0),
            flags: crate::flag::Cloth {
                flutter: num(&v["flags"]["flutter"], 0.0),
                lit: v["flags"]["lit"].as_bool().unwrap_or(false),
                wind: v["flags"]["wind"].as_bool().unwrap_or(false),
            },
        }
    }
}

impl Look {
    /// The profile without the blocks whose shader the overlay (`root`) lacks.
    fn with_shaders(mut self, root: &std::path::Path) -> Look {
        if self.sails.is_some() && !has_shader(root, "sails", sails::SHADER) {
            self.sails = None;
        }
        if self.deckhands.is_some() && !has_shader(root, "deckhands", deckhands::SHADER) {
            self.deckhands = None;
        }
        if self.engines.is_some() && !has_shader(root, "engines", trails::PLUME_SHADER) {
            self.engines = None;
        }
        if self.wakes.is_some() && !has_shader(root, "wakes", trails::WAKE_SHADER) {
            self.wakes = None;
        }
        if self.tracers.is_some() && !has_shader(root, "tracers", tracers::SHADER) {
            self.tracers = None;
        }
        if self.surfaces.is_some() && !has_shader(root, "surfaces", surfaces::SHADER) {
            self.surfaces = None;
        }
        if self.bursts.is_some() && !(has_shader(root, "bursts", bursts::SHADER) & has_shader(root, "bursts", bursts::SMOKE_SHADER)) {
            self.bursts = None;
        }
        if self.sky.is_some() && !has_shader(root, "sky", sky::SHADER) {
            self.sky = None;
        }
        if self.black_holes.is_some() && !has_shader(root, "black_holes", holes::SHADER) {
            self.black_holes = None;
        }
        if let Some(b) = self.black_holes.as_mut()
            && b.replace
            && !has_shader(root, "black_holes", holes::RIBBON_SHADER)
        {
            b.replace = false;
        }
        if self.currents.is_some() && !has_shader(root, "currents", currents::SHADER) {
            self.currents = None;
        }
        if self.nebulae.is_some() && !has_shader(root, "nebulae", nebulae::SHADER) {
            self.nebulae = None;
        }
        if self.bolts.is_some() && !has_shader(root, "nebulae.bolt", bolts::SHADER) {
            self.bolts = None;
        }
        if let Some(n) = self.nebulae.as_mut()
            && n.rules.iter().any(|r| r.replace)
            && !has_shader(root, "nebulae", nebulae::VOLUME_SHADER)
        {
            n.rules.iter_mut().for_each(|r| r.replace = false);
        }
        self.materials = self.materials.and_then(|m| m.with_shaders(root));
        self
    }
}

/// Whether the overlay has a block's shader; without it the block stays off (a warning names the file the mod should ship).
fn has_shader(root: &std::path::Path, block: &str, shader: &str) -> bool {
    let found = root.join(shader).is_file();
    if !found {
        warn!("render profile: `{block}` needs {shader} in the mod; left off");
    }
    found
}

pub fn plugin(app: &mut App) {
    let root = crate::assets_dir();
    let Some(mut v) = crate::data::try_json(&root.join(PROFILE)) else { return };
    add_mod_skies(&mut v, crate::data::try_json(&root.join(SKIES)));
    let look = Look::parse(&v).with_shaders(root);
    frame::plugin(app);
    lights::plugin(app);
    materials::plugin(app);
    sails::plugin(app, &look);
    deckhands::plugin(app, &look);
    trails::plugin(app, &look);
    tracers::plugin(app, &look);
    surfaces::plugin(app, &look);
    bursts::plugin(app, &look);
    rules::plugin(app, &look);
    sky::plugin(app, &look);
    holes::plugin(app, &look);
    replace::plugin(app, &look);
    nebulae::plugin(app, &look);
    bolts::plugin(app, &look);
    currents::plugin(app, &look);
    // one baked noise volume for every block that samples it
    if look.sky.is_some() || look.nebulae.is_some() {
        app.add_systems(Startup, volnoise::bake);
    }
    app.insert_resource(look.flags).insert_resource(look).add_systems(Update, (camera, (sun, sky_light).after(crate::scenario::apply_lighting)));
}

/// Marks a camera or sun the profile has set up (or that must keep plain settings, like the Ships screen's off-screen preview).
#[derive(Component)]
pub struct Styled;

fn camera(mut commands: Commands, look: Res<Look>, cams: Query<Entity, (With<Camera3d>, Without<Styled>)>) {
    for cam in &cams {
        let mut e = commands.entity(cam);
        e.insert((Styled, bevy::camera::Hdr, look.tonemapping, DebandDither::Enabled));
        if look.bloom > 0.0 {
            let mut bloom = Bloom { intensity: look.bloom, ..Bloom::NATURAL };
            if look.bloom_threshold > 0.0 {
                bloom.prefilter = BloomPrefilter { threshold: look.bloom_threshold, threshold_softness: 0.4 };
                bloom.composite_mode = BloomCompositeMode::Additive;
            }
            e.insert(bloom);
        }
        let section = ColorGradingSection { saturation: look.saturation, contrast: look.contrast, ..default() };
        e.insert(ColorGrading { global: ColorGradingGlobal { exposure: look.exposure, ..default() }, shadows: section, midtones: section, highlights: section });
        // SMAA and SSAO both replace the default MSAA (SSAO does not work with it).
        if look.smaa || look.ssao {
            e.insert(Msaa::Off);
        }
        if look.smaa {
            e.insert(Smaa { preset: SmaaPreset::High });
        }
        if look.ssao {
            e.insert(ScreenSpaceAmbientOcclusion::default());
        }
        if look.vignette > 0.0 {
            e.insert(bevy::post_process::effect_stack::Vignette { intensity: look.vignette, ..default() });
        }
        if look.contact_shadows {
            e.insert(bevy::pbr::ContactShadows::default());
        }
        // the volumes of replaced clouds end their rays at the scene's depth
        if look.nebulae.as_ref().is_some_and(|n| n.rules.iter().any(|r| r.replace)) {
            e.insert(bevy::core_pipeline::prepass::DepthPrepass);
        }
    }
}

/// Distinguishes a sun we styled from a preview rig that opted out via `Styled`.
#[derive(Component)]
struct StyledSun;

fn sun(
    mut commands: Commands,
    look: Res<Look>,
    map: Res<crate::scenario::MapLighting>,
    mut suns: Query<(Entity, &mut DirectionalLight, Has<StyledSun>), Or<(Without<Styled>, With<StyledSun>)>>,
) {
    for (e, mut sun, styled) in &mut suns {
        if !styled || map.is_changed() {
            // Map changes reset the key colour. Use the authored colour, not the already tinted light.
            let authored = map.0.map_or(Color::WHITE, |l| l.sun);
            let c = authored.to_linear().to_vec3() * look.sun_color;
            sun.color = Color::linear_rgb(c.x, c.y, c.z);
        }
        if !styled {
            sun.shadow_maps_enabled = look.shadows;
            sun.contact_shadows_enabled = look.contact_shadows;
            sun.illuminance *= look.sun_intensity;
            let cascades =
                CascadeShadowConfigBuilder { num_cascades: 4, first_cascade_far_bound: look.shadow_distance / 20.0, maximum_distance: look.shadow_distance, ..default() };
            commands.entity(e).insert((Styled, StyledSun, cascades.build()));
        }
    }
}

/// Which sky the camera's image-based light was made from, and for how many frames Bevy has been filtering it.
#[derive(Component)]
struct SkyLit {
    image: Handle<Image>,
    frames: u32,
}

/// Frames Bevy's GPU filter gets to turn the sky into diffuse and specular light. It refilters the whole cube with its mip
/// chain every frame while `GeneratedEnvironmentMapLight` is present (7 ms a frame at 2048), so it is removed afterwards and
/// the finished `EnvironmentMapLight` stays.
const SKY_LIGHT_FRAMES: u32 = 30;

/// The camera's skybox (the baked sky) also lights the scene, following it on and off. Scale the flat ambient
/// independently so the profile keeps the same baseline fill even without a baked sky.
fn sky_light(
    mut commands: Commands,
    look: Res<Look>,
    mut cams: Query<(Entity, Option<&Skybox>, Option<&mut SkyLit>, Has<EnvironmentMapLight>), With<Camera3d>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    map: Res<crate::scenario::MapLighting>,
    mut base: Local<Option<f32>>,
) {
    let base = *base.get_or_insert(ambient.brightness);
    let brightness = base * look.ambient;
    if ambient.brightness != brightness {
        ambient.brightness = brightness;
    }
    if look.ambient_floor != Vec3::ZERO && map.is_changed() {
        let authored = map.0.map_or(Color::WHITE, |l| l.ambient).to_linear().to_vec3();
        let c = authored.max(look.ambient_floor);
        ambient.color = Color::linear_rgb(c.x, c.y, c.z);
    }
    for (cam, sky, done, has_light) in &mut cams {
        let want = sky.and_then(|s| s.image.clone()).filter(|_| look.sky_light > 0.0);
        let mut e = commands.entity(cam);
        match (want, done) {
            (None, Some(_)) => {
                e.remove::<(SkyLit, GeneratedEnvironmentMapLight, EnvironmentMapLight)>();
            }
            (Some(image), done) if done.as_ref().is_none_or(|d| d.image != image) => {
                // a new sky: drop the old light so Bevy generates one from this sky
                e.remove::<EnvironmentMapLight>();
                e.insert((GeneratedEnvironmentMapLight { environment_map: image.clone(), intensity: look.sky_light, ..default() }, SkyLit { image, frames: 0 }));
            }
            (Some(_), Some(mut d)) if has_light && d.frames < SKY_LIGHT_FRAMES => {
                d.frames += 1;
                if d.frames == SKY_LIGHT_FRAMES {
                    e.remove::<GeneratedEnvironmentMapLight>();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mods_add_accents_for_their_own_skies() {
        use serde_json::json;
        let mut look = json!({"sky": {"near": {"distance": 3000}, "skies": {"skybox_05": {"base": {"breath": 0.1}}, "ours": {"near": {"opacity": 0.2}}}}});
        add_mod_skies(&mut look, Some(json!({"ours": {"near": {"source": "clouds", "opacity": 0.6}}})));
        let skies = &look["sky"]["skies"];
        assert_eq!(skies["ours"]["near"], json!({"source": "clouds", "opacity": 0.2}), "the profile's entry is merged over the mod's");
        assert!(skies["skybox_05"].is_object(), "the profile's other skies stay");
        let mut plain = json!({"camera": {}});
        add_mod_skies(&mut plain, Some(json!({"ours": {}})));
        assert_eq!(plain, json!({"camera": {}}), "no sky block, no dome");
    }

    #[test]
    fn ambient_fill_also_applies_without_baked_skies() {
        use crate::scenario::{Lighting, MapLighting};
        let mut app = App::new();
        let map = Lighting { direction: Vec3::NEG_Z, sun: Color::WHITE, ambient: Color::linear_rgb(0.6, 0.1, 0.2) };
        app.insert_resource(GlobalAmbientLight { brightness: 450.0, ..default() })
            .insert_resource(MapLighting(Some(map)))
            .insert_resource(Look::parse(&serde_json::json!({"ambient": 0.5, "ambient_floor": [0.2, 0.25, 0.3], "sky_light": 180})))
            .add_systems(Update, sky_light);
        app.world_mut().spawn(Camera3d::default());
        app.update();
        app.update();
        let ambient = app.world().resource::<GlobalAmbientLight>();
        assert_eq!(ambient.brightness, 225.0, "no sky required, and the scale does not compound");
        assert_eq!(ambient.color.to_linear().to_vec3(), Vec3::new(0.6, 0.25, 0.3), "the floor preserves stronger authored channels");
        app.world_mut().resource_mut::<MapLighting>().0 = None;
        app.update();
        assert_eq!(app.world().resource::<GlobalAmbientLight>().color.to_linear().to_vec3(), Vec3::ONE, "the next map does not inherit the previous fill");
    }

    #[test]
    fn map_changes_keep_the_sun_tinted_without_scaling_its_strength_twice() {
        use crate::scenario::{Lighting, MapLighting, apply_lighting};
        let mut app = App::new();
        app.init_resource::<MapLighting>()
            .init_resource::<GlobalAmbientLight>()
            .insert_resource(Look::parse(&serde_json::json!({"sun": {"color": [1, 0.8, 0.5], "intensity": 0.5}})))
            .add_systems(Update, (apply_lighting.run_if(resource_changed::<MapLighting>), sun).chain());
        let key = app.world_mut().spawn((DirectionalLight { illuminance: 9000.0, ..default() }, Transform::default())).id();
        let read_key = |app: &App| {
            let l = app.world().get::<DirectionalLight>(key).unwrap();
            (l.color.to_linear().to_vec3(), l.illuminance)
        };
        app.update();
        assert_eq!(read_key(&app), (Vec3::new(1.0, 0.8, 0.5), 4500.0));
        app.world_mut().resource_mut::<MapLighting>().0 = Some(Lighting { direction: Vec3::NEG_Z, sun: Color::linear_rgb(0.5, 1.0, 0.5), ambient: Color::WHITE });
        app.update();
        app.update();
        assert_eq!(read_key(&app), (Vec3::new(0.5, 0.8, 0.25), 4500.0));
        app.world_mut().resource_mut::<MapLighting>().0 = None;
        app.update();
        assert_eq!(read_key(&app), (Vec3::new(1.0, 0.8, 0.5), 4500.0));
        let preview = app.world_mut().spawn((DirectionalLight::default(), Styled)).id();
        app.update();
        assert_eq!(app.world().get::<DirectionalLight>(preview).unwrap().color, DirectionalLight::default().color);
        assert!(app.world().get::<StyledSun>(preview).is_none());
    }

    #[test]
    fn profile_keys_are_optional() {
        let look = Look::parse(&serde_json::json!({}));
        assert_eq!((look.bloom, look.fx_gain, look.sky_light, look.shadows, look.smaa, look.lit_smoke), (0.15, 1.0, 0.0, false, true, false));
        assert!(look.sky.is_none());
        assert!(look.hide_effects.is_empty());
        assert!(look.lights.is_empty() && look.sails.is_none() && look.engines.is_none() && look.wakes.is_none());
        assert!(
            look.tracers.is_none()
                && look.surfaces.is_none()
                && look.bursts.is_none()
                && look.materials.is_none()
                && look.black_holes.is_none()
                && look.nebulae.is_none()
                && look.bolts.is_none()
                && look.currents.is_none()
        );
        assert_eq!((look.vignette, look.aberration_pulse, look.contact_shadows, look.sun_color, look.sun_intensity), (0.0, 0.0, false, Vec3::ONE, 1.0));
        assert_eq!(look.flags, crate::flag::Cloth::default());
        let look = Look::parse(&serde_json::json!({
            "camera": {"bloom": 0.3, "tonemapping": "agx", "ssao": true},
            "sun": {"shadows": true}, "fx_gain": 2.5, "flags": {"flutter": 1, "lit": true},
        }));
        assert_eq!((look.bloom, look.tonemapping, look.ssao, look.shadows, look.fx_gain), (0.3, Tonemapping::AgX, true, true, 2.5));
        assert_eq!(look.flags, crate::flag::Cloth { flutter: 1.0, lit: true, wind: false });
    }

    #[test]
    fn name_lists_are_lower_case_with_a_default() {
        assert_eq!(names(&serde_json::json!(["Sail", "DES_"]), &["x"]), ["sail", "des_"]);
        assert_eq!(names(&Value::Null, &["sail"]), ["sail"]);
    }

    #[test]
    fn a_profile_with_every_block_parses() {
        let look = Look::parse(&serde_json::json!({
            "camera": {"bloom": 0.1, "bloom_threshold": 1.0}, "sky_light": 600, "sky": {"near": {"distance": 900}}, "ambient": 0.4, "sun": {"shadows": true},
            "lights": [{"effects": "Explosion_", "lumens": 1e8, "seconds": 1.5}],
            "sails": {"glow": [1, 0.7, 0.3], "opaque": true, "billow": 0.03},
            "flags": {"flutter": 1, "lit": true, "wind": true},
            "engines": {"colors": {"Navy": [0.3, 0.6, 1.0]}}, "wakes": {"planes": [0, 60, -60]},
            "tracers": {"bullets": [{"match": "Lancer", "color": [0.4, 0.7, 1]}]}, "surfaces": {"rules": [{"match": ["metal"], "metallic": 0.8}]},
            "materials": {"rules": [{"match": ["hull"], "shader": "shaders/hull.wesl", "params": [1, 2, 3, 4]}], "skip": ["water"]},
            "black_holes": {"lensing": 2}, "nebulae": {"rules": [{"match": "Nebula"}]}, "currents": {"core": 2},
        }));
        assert_eq!(look.nebulae.as_ref().map(|n| n.rules.len()), Some(1));
        assert_eq!(look.currents.as_ref().map(|c| c.core), Some(2.0));
        assert_eq!(look.black_holes.as_ref().map(|b| b.lensing), Some(2.0));
        assert!(look.tracers.is_some() && look.surfaces.is_some());
        assert_eq!(look.materials.as_ref().map(|m| (m.rules.len(), m.find("hull.tga"), m.find("water"))), Some((1, Some(0), None)));
        assert!(look.lights.iter().any(|l| "Explosion_Medium".contains(&l.effects)));
        assert!(look.sails.is_some() && look.engines.is_some() && look.wakes.is_some());
        assert_eq!((look.bloom_threshold, look.sky_light, look.sky.as_ref().map(|s| s.layers[0].distance), look.flags.wind), (1.0, 600.0, Some(900.0), true));
    }

    #[test]
    fn a_block_whose_shader_the_mod_does_not_ship_stays_off() {
        let root = std::env::temp_dir().join(format!("procyon-render-{}", std::process::id()));
        std::fs::create_dir_all(root.join("shaders")).unwrap();
        assert!(!has_shader(&root, "sails", sails::SHADER), "the engine ships no shaders");
        std::fs::write(root.join(sails::SHADER), "").unwrap();
        assert!(has_shader(&root, "sails", sails::SHADER));
        let look = Look::parse(&serde_json::json!({"sails": {}, "engines": {}, "wakes": {}, "tracers": {}, "surfaces": {}, "bursts": {}, "fx_gain": 2})).with_shaders(&root);
        assert!(look.sails.is_some() && look.engines.is_none() && look.wakes.is_none(), "only the block whose shader is there");
        assert!(look.tracers.is_none() && look.surfaces.is_none() && look.bursts.is_none());
        assert_eq!(look.fx_gain, 2.0, "blocks without shaders are unaffected");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replacing_needs_the_volume_shader_or_the_retail_cloud_stays() {
        let root = std::env::temp_dir().join(format!("procyon-render-volume-{}", std::process::id()));
        std::fs::create_dir_all(root.join("shaders")).unwrap();
        std::fs::write(root.join(nebulae::SHADER), "").unwrap();
        let profile = serde_json::json!({"nebulae": {"replace": true, "rules": [{"match": "Nebula"}]}});
        let look = Look::parse(&profile).with_shaders(&root);
        assert!(!look.nebulae.as_ref().unwrap().rules[0].replace, "no volume shader: the retail shell keeps drawing");
        std::fs::write(root.join(nebulae::VOLUME_SHADER), "").unwrap();
        assert!(Look::parse(&profile).with_shaders(&root).nebulae.unwrap().rules[0].replace);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_replaced_black_hole_needs_the_ribbon_shader_or_the_retail_layers_stay() {
        let root = std::env::temp_dir().join(format!("procyon-render-hole-{}", std::process::id()));
        std::fs::create_dir_all(root.join("shaders")).unwrap();
        std::fs::write(root.join(holes::SHADER), "").unwrap();
        let profile = serde_json::json!({"black_holes": {"replace": true}});
        assert!(!Look::parse(&profile).with_shaders(&root).black_holes.unwrap().replace, "no ribbon shader: nothing is hidden");
        std::fs::write(root.join(holes::RIBBON_SHADER), "").unwrap();
        assert!(Look::parse(&profile).with_shaders(&root).black_holes.unwrap().replace);
        std::fs::remove_dir_all(root).unwrap();
    }
}
