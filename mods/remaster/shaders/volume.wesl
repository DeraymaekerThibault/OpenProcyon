// Not retail: a nebula cloud or solar storm of the render profile drawn as a volume (game/src/render/nebulae.rs; contract:
// `VolumeUniform` there, our own work under the project licence). It replaces the retail shell: the cloud's proxy box is drawn on its
// back faces only (so it also draws with the camera inside), without a depth test, and this shader ray-marches the cloud's ellipsoid
// itself with emission and absorption (Beer-Lambert), composited premultiplied over the scene:
//   rgb = light the gas gives off, a = 1 - transmittance (capped by `max_opacity`, lower where the ray ends on a hull or an island).
// Dense gas then hides what is behind it, thin gas glows, dust lanes stay dark. The march ends at the scene's depth (the depth
// prepass), so gas lies in front of a ship only by its real distance. The gas is lit by the sun with a cheap self-shadow (a few
// steps toward the sun), coloured between a thin and a dense colour and tinted by the retail colour loop; a bolt of storm lightning
// lights it from inside for a moment, along its whole length.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
};
import bevy_pbr::prepass::utils as prepass_utils;

struct Volume {
    // xyz: centre, w: radius
    centre: vec4<f32>,
    // xyz: half-size per axis (shares of the radius, breath applied), w: most steps
    axes: vec4<f32>,
    // x: emission, y: noise scale, z: churn, w: absorption (optical depth across a radius at full density)
    look: vec4<f32>,
    // rgb: thin gas, w: most opacity one cloud reaches
    low: vec4<f32>,
    // rgb: dense gas, w: dust (dark lanes)
    high: vec4<f32>,
    // rgb: the retail colour loop, w: steps toward the sun
    tint: vec4<f32>,
    // xyz: toward the sun, w: sun strength
    sun: vec4<f32>,
    // rgb: sun colour, w: share of the gas kept with the camera inside
    sun_color: vec4<f32>,
    // xyz: one end of the last bolt, w: what is left of its light
    flash: vec4<f32>,
    // xyz: its other end
    flash_to: vec4<f32>,
    // x: clock, y: a bolt's reach (world units), z: a bolt's brightness, w: the mission's fadeout factor
    params: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> vol: Volume;

// the baked noise (game/src/render/volnoise.rs): r the gas (three octaves), g the warp, b the dust; it tiles every CELLS cells
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var noise_tex: texture_3d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var noise_samp: sampler;
const CELLS: f32 = 8.0;

fn bake(p: vec3<f32>) -> vec4<f32> {
    return textureSampleLevel(noise_tex, noise_samp, p / CELLS, 0.0);
}

// the slow warp that makes the gas churn in place instead of sliding
fn warp(p: vec3<f32>, t: f32) -> f32 {
    return bake(p * 0.8 + vec3<f32>(t * 0.9, 0.0, 3.1)).g - 0.5;
}

fn bend(p: vec3<f32>, w: f32, t: f32) -> vec3<f32> {
    return p + vec3<f32>(w * 1.9, -w * 1.4, w * 1.7) + vec3<f32>(0.0, t * 0.5, 0.0);
}

// gas density 0..1 at a point of the cloud's unit space, `w` the warp there
fn gas(q: vec3<f32>, w: f32, t: f32) -> f32 {
    return smoothstep(0.36, 0.78, bake(bend(q * vol.look.y, w, t)).r);
}

fn edge_of(q: vec3<f32>) -> f32 {
    return 1.0 - smoothstep(0.5, 1.0, length(q));
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    var out: FragmentOutput;
    out.color = vec4<f32>(0.0);
    let eye = view.world_position;
    let c = vol.centre.xyz;
    let radius = vol.centre.w;
    let axes = vol.axes.xyz * radius;
    let dir = normalize(in.world_position.xyz - eye);

    // the ray against the ellipsoid, in a space where it is a unit sphere
    let o = (eye - c) / axes;
    let d = dir / axes;
    let a = dot(d, d);
    let b = dot(o, d);
    let disc = b * b - a * (dot(o, o) - 1.0);
    if disc <= 0.0 {
        return out;
    }
    let s = sqrt(disc);
    let t0 = max((-b - s) / a, 0.0);
    var t1 = (-b + s) / a;
    let inside_cam = dot(o, o) < 1.0;

    // the scene: the march ends at the nearest hull or island (the depth prepass; none = the whole chord)
    var t_scene = 1.0e9;
    @if(DEPTH_PREPASS)
    {
        let sd = prepass_utils::prepass_depth(in.position, 0u);
        if sd > 0.0 {
            let forward = -view.world_from_view[2].xyz;
            t_scene = (view.clip_from_view[3][2] / sd) / max(dot(dir, forward), 0.05);
        }
    }
    let hit_scene = t_scene < t1;
    t1 = min(t1, t_scene);
    if t1 <= t0 {
        return out;
    }

    // fewer steps on a short chord, a different start per pixel so the steps do not band
    let len = t1 - t0;
    let steps = clamp(len / (radius * 0.05), 4.0, vol.axes.w);
    let dt = len / steps;
    let jitter = fract(52.9829189 * fract(dot(in.position.xy, vec2<f32>(0.06711056, 0.00583715))));
    let clock = vol.params.x * vol.look.z;
    let sun = vol.sun.xyz;
    let light_steps = i32(vol.tint.w);
    let lstep = radius * 0.1;
    let sigma = vol.look.w / radius;
    let share = select(1.0, vol.sun_color.w, inside_cam);
    let phase = 0.65 + 0.9 * pow(max(dot(dir, sun), 0.0), 4.0);

    var light = vec3<f32>(0.0);
    var tr = 1.0;
    for (var i = 0; i < 24; i = i + 1) {
        if f32(i) >= steps {
            break;
        }
        let t = t0 + (f32(i) + jitter) * dt;
        let x = eye + dir * t;
        let q = (x - c) / axes;
        let edge = edge_of(q);
        if edge <= 0.0 {
            continue;
        }
        // nothing pops in right at the lens
        let near = smoothstep(0.0, radius * 0.04, t);
        let w = warp(q * vol.look.y, clock);
        let dens = gas(q, w, clock) * edge * near;
        // dust: dark folds, extinction without light
        let dust = smoothstep(0.5, 0.85, bake(q * vol.look.y * 0.55 + vec3<f32>(5.1, clock * 0.3, 1.7)).b) * edge * near * vol.high.w;
        var tau_dust = dust * sigma * dt * 1.4;
        if dens > 0.01 {
            // the sun's light reaching this point through the gas between
            var toward = 0.0;
            for (var k = 1; k <= 3; k = k + 1) {
                if k > light_steps {
                    break;
                }
                let qs = (x + sun * (f32(k) * lstep) - c) / axes;
                toward += gas(qs, w, clock) * edge_of(qs);
            }
            let shade = exp(-toward * vol.look.w * 0.5);
            let base = mix(vol.low.rgb, vol.high.rgb, smoothstep(0.35, 1.0, dens));
            let lit = 0.3 + vol.sun.w * vol.sun_color.rgb * shade * phase;
            var emitted = base * vol.tint.rgb * lit * vol.look.x;
            // a bolt lights the gas around it from inside
            let ab = vol.flash_to.xyz - vol.flash.xyz;
            let along = clamp(dot(x - vol.flash.xyz, ab) / max(dot(ab, ab), 0.001), 0.0, 1.0);
            let fd = length(x - (vol.flash.xyz + ab * along)) / max(vol.params.y, 1.0);
            emitted += vol.params.z * vol.flash.w * exp(-fd * fd * 2.5) * vec3<f32>(0.7, 0.85, 1.0);
            let tau = dens * sigma * dt * share;
            let step_light = 1.0 - exp(-tau);
            light += tr * emitted * step_light;
            tr *= exp(-tau);
        }
        tr *= exp(-tau_dust);
        if tr < 0.02 {
            break;
        }
    }

    // never a wall: one cloud reaches `max_opacity` at most, and less where the ray ended on a hull
    let cap = select(vol.low.w, vol.low.w * 0.6, hit_scene) * vol.params.w;
    let alpha = 1.0 - tr;
    let k = min(1.0, cap / max(alpha, 1.0e-4));
    out.color = vec4<f32>(light * k * vol.params.w, alpha * k);
    return out;
}
