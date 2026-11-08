//! Not retail: render extensions a graphics mod switches on with a render profile, `data/render/look.json` in the overlay.
//! Absent, everything renders as classic. A profile changes how the battle is drawn, never what happens. The engine
//! provides the building blocks; the mod tunes them in the profile and ships the shaders the blocks draw with
//! (`shaders/*.wesl` in the mod); a block whose shader is missing stays off. Reference: docs/modding.md "Graphics mods".
//!
//! - here: the camera (HDR, bloom, tonemapping, grading, SMAA, ambient occlusion), sun shadows, and the baked HD sky
//!   (`view::hd_sky`) as image-based light standing in for the flat ambient light;
//! - `lights.rs`: point lights the profile puts on effects (flashes fade, fires and engines flicker);
//! - `materials.rs`: brighter additive effects and shells for the bloom, smoke lit by the scene, shadow rules for effects;
//! - `sails.rs`: sail materials with a shader (`shaders/sail.wesl`) and cloth that bellies out with the ship's speed;
//! - `trails.rs`: engine plumes (`shaders/plume.wesl`) and wakes drawn as crossed ribbons (`shaders/wake.wesl`);
//! - `tracers.rs`: streaks of light behind shells in flight (`shaders/tracer.wesl`);
//! - `surfaces.rs`: relief, roughness and metal for the retail models' textured materials (`shaders/surface.wesl`);
//! - `sky.rs`: a dome over the baked sky with a near layer of gas that slides against it (`shaders/sky.wesl`);
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
mod bursts;
mod lights;
mod materials;
mod rules;
pub(crate) mod sails;
mod sky;
mod surfaces;
mod tracers;
mod trails;

/// The render profile's asset path.
pub const PROFILE: &str = "data/render/look.json";

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
    /// Multiplies the flat ambient light while the sky light is on: the sky takes over its role, in the sky's colours.
    pub ambient: f32,
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
    pub flags: crate::flag::Cloth,
    pub engines: Option<trails::Engines>,
    pub wakes: Option<trails::Wakes>,
    pub tracers: Option<tracers::Tracers>,
    pub surfaces: Option<surfaces::Surfaces>,
    pub bursts: Option<bursts::Bursts>,
    pub materials: Option<rules::Materials>,
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
            shadows: v["sun"]["shadows"].as_bool().unwrap_or(false),
            shadow_distance: num(&v["sun"]["shadow_distance"], 2000.0),
            fx_gain: num(&v["fx_gain"], 1.0),
            shell_gain: num(&v["shell_gain"], 1.0),
            lit_smoke: v["lit_smoke"].as_bool().unwrap_or(false),
            lights: lights::parse(&v["lights"]),
            sails: sails::parse(&v["sails"]),
            engines: trails::parse_engines(&v["engines"]),
            wakes: trails::parse_wakes(&v["wakes"]),
            tracers: tracers::parse(&v["tracers"]),
            surfaces: surfaces::parse(&v["surfaces"]),
            bursts: bursts::parse(&v["bursts"]),
            materials: rules::parse(&v["materials"]),
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
    let Some(v) = crate::data::try_json(&root.join(PROFILE)) else { return };
    let look = Look::parse(&v).with_shaders(root);
    lights::plugin(app);
    materials::plugin(app);
    sails::plugin(app, &look);
    trails::plugin(app, &look);
    tracers::plugin(app, &look);
    surfaces::plugin(app, &look);
    bursts::plugin(app, &look);
    rules::plugin(app, &look);
    sky::plugin(app, &look);
    app.insert_resource(look.flags).insert_resource(look).add_systems(Update, (camera, sun, sky_light));
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
    }
}

fn sun(mut commands: Commands, look: Res<Look>, mut suns: Query<(Entity, &mut DirectionalLight), Without<Styled>>) {
    for (e, mut sun) in &mut suns {
        sun.shadow_maps_enabled = look.shadows;
        sun.contact_shadows_enabled = look.contact_shadows;
        let c = sun.color.to_linear().to_vec3() * look.sun_color;
        sun.color = Color::linear_rgb(c.x, c.y, c.z);
        sun.illuminance *= look.sun_intensity;
        let cascades = CascadeShadowConfigBuilder { num_cascades: 4, first_cascade_far_bound: look.shadow_distance / 20.0, maximum_distance: look.shadow_distance, ..default() };
        commands.entity(e).insert((Styled, cascades.build()));
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

/// The camera's skybox (the baked sky) also lights the scene, following it on and off; meanwhile the flat ambient light
/// is scaled by `ambient`.
fn sky_light(
    mut commands: Commands,
    look: Res<Look>,
    mut cams: Query<(Entity, Option<&Skybox>, Option<&mut SkyLit>, Has<EnvironmentMapLight>), With<Camera3d>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut base: Local<Option<f32>>,
) {
    let base = *base.get_or_insert(ambient.brightness);
    let lit = look.sky_light > 0.0 && cams.iter().any(|(_, sky, ..)| sky.is_some_and(|s| s.image.is_some()));
    let brightness = if lit { base * look.ambient } else { base };
    if ambient.brightness != brightness {
        ambient.brightness = brightness;
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
    fn profile_keys_are_optional() {
        let look = Look::parse(&serde_json::json!({}));
        assert_eq!((look.bloom, look.fx_gain, look.sky_light, look.shadows, look.smaa, look.lit_smoke), (0.15, 1.0, 0.0, false, true, false));
        assert!(look.sky.is_none());
        assert!(look.lights.is_empty() && look.sails.is_none() && look.engines.is_none() && look.wakes.is_none());
        assert!(look.tracers.is_none() && look.surfaces.is_none() && look.bursts.is_none() && look.materials.is_none());
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
        }));
        assert!(look.tracers.is_some() && look.surfaces.is_some());
        assert_eq!(look.materials.as_ref().map(|m| (m.rules.len(), m.find("hull.tga"), m.find("water"))), Some((1, Some(0), None)));
        assert!(look.lights.iter().any(|l| "Explosion_Medium".contains(&l.effects)));
        assert!(look.sails.is_some() && look.engines.is_some() && look.wakes.is_some());
        assert_eq!((look.bloom_threshold, look.sky_light, look.sky.as_ref().map(|s| s.distance), look.flags.wind), (1.0, 600.0, Some(900.0), true));
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
}
