// Not retail: a nebula cloud or solar storm of the render profile drawn as a volume (game/src/render/nebulae.rs; contract:
// `VolumeUniform` there, our own work under the project licence). It replaces the retail shell: the cloud's proxy box is drawn on its
// back faces only (so it also draws with the camera inside), without a depth test, and this shader ray-marches the cloud's ellipsoid
// itself with emission and absorption (Beer-Lambert), composited premultiplied over the scene:
//   rgb = light the gas gives off, a = 1 - transmittance (capped by `max_opacity`, lower where the ray ends on a hull or an island).
// Dense gas then hides what is behind it, thin gas glows, dust lanes stay dark. The march ends at the scene's depth (the depth
// prepass), so gas lies in front of a ship only by its real distance. The gas is lit by the sun with a cheap self-shadow (a few
// steps toward the sun), coloured between a thin and a dense colour and tinted by the retail colour loop; a bolt of storm lightning
// lights it from inside for a moment, along its whole length. Optional HDR emission makes dense pockets bloom independently
// of the veil's opacity, still occluded by the dust in front. A cloud's colour drifts into a second hue in large patches
// (`variation`), so it is not one flat wash. A solar storm is not a quiet cloud: its gas wheels round the centre like a cyclone
// (`swirl`, faster near the centre; two phases of the turn are blended so the gas never winds up), the edges of its billows burn
// as hot plasma filaments (`filaments`), and lightning keeps flickering deep inside it (`flicker`).
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
    // x: swirl (turns a second near the centre), y: filaments, z: flicker, w: hue variation
    storm: vec4<f32>,
    // rgb: the second hue, w: HDR core glow
    alt: vec4<f32>,
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

// the raw noise the gas is made of, at a point of the cloud's unit space, `w` the warp there
fn raw(q: vec3<f32>, w: f32, t: f32) -> f32 {
    return bake(bend(q * vol.look.y, w, t)).r;
}

// gas density 0..1
fn gas(q: vec3<f32>, w: f32, t: f32) -> f32 {
    return smoothstep(0.36, 0.78, raw(q, w, t));
}

// a point turned about the cloud's vertical axis
fn spin(q: vec3<f32>, a: f32) -> vec3<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec3<f32>(c * q.x - s * q.z, q.y, s * q.x + c * q.z);
}

// the gas at a point: x density, y the hot filaments on the edges of the billows, z the warp
fn sample(q: vec3<f32>, clock: f32) -> vec3<f32> {
    let w = warp(q * vol.look.y, clock);
    let n = raw(q, w, clock);
    // the filaments are finer than the billows: thin bright seams where a finer noise crosses its middle
    var fil = 0.0;
    if vol.storm.y > 0.0 {
        let n2 = raw(q * 2.6 + vec3<f32>(3.3, 1.1, 5.7), w * 1.5, clock * 1.7);
        fil = pow(max(1.0 - abs(n2 - 0.5) * 7.0, 0.0), 3.0) * smoothstep(0.3, 0.6, n);
    }
    return vec3<f32>(smoothstep(0.36, 0.78, n), fil, w);
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
    let steps = ceil(clamp(len / (radius * 0.05), 4.0, vol.axes.w));
    let dt = len / steps;
    let jitter = fract(52.9829189 * fract(dot(in.position.xy, vec2<f32>(0.06711056, 0.00583715))));
    let clock = vol.params.x * vol.look.z;
    let sun = vol.sun.xyz;
    let light_steps = i32(vol.tint.w);
    let lstep = radius * 0.1;
    let sigma = vol.look.w / radius;
    let share = select(1.0, vol.sun_color.w, inside_cam);
    let phase = 0.65 + 0.9 * pow(max(dot(dir, sun), 0.0), 4.0);
    let flash = vol.params.z * vol.flash.w;
    let ab = vol.flash_to.xyz - vol.flash.xyz;
    let inv_ab2 = 1.0 / max(dot(ab, ab), 0.001);
    let reach = max(vol.params.y, 1.0);

    var light = vec3<f32>(0.0);
    var core_light = vec3<f32>(0.0);
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
        // a storm wheels: the gas is sampled turned back by how far it has gone round, in two phases blended
        var g: vec3<f32>;
        if vol.storm.x == 0.0 {
            g = sample(q, clock);
        } else {
            let period = 8.0;
            let pa = fract(vol.params.x / period);
            let pb = fract(vol.params.x / period + 0.5);
            let rate = vol.storm.x * 6.2831853 * period / (0.4 + 1.6 * length(q.xz));
            let ga = sample(spin(q, -(pa - 0.5) * rate), clock);
            let gb = sample(spin(q, -(pb - 0.5) * rate), clock);
            g = mix(gb, ga, 1.0 - abs(2.0 * pa - 1.0));
        }
        let w = g.z;
        let dens = g.x * edge * near;
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
            var base = mix(vol.low.rgb, vol.high.rgb, smoothstep(0.35, 1.0, dens));
            var core_color = mix(vol.low.rgb, vol.high.rgb, 0.38);
            // the second hue, in large patches
            if vol.storm.w > 0.0 {
                let hue = smoothstep(0.35, 0.65, bake(q * vol.look.y * 0.22 + vec3<f32>(11.0, 3.0, 7.0)).g);
                base = mix(base, vol.alt.rgb * (0.35 + 0.9 * smoothstep(0.35, 1.0, dens)), hue * vol.storm.w);
                core_color = mix(core_color, vol.alt.rgb, hue * vol.storm.w);
            }
            // dense cores glow hotter
            base += vol.high.rgb * 0.5 * dens * dens * dens;
            let lit = 0.3 + vol.sun.w * vol.sun_color.rgb * shade * phase;
            var emitted = base * vol.tint.rgb * lit * vol.look.x;
            // a bolt lights the gas around it from inside
            if flash > 0.0 {
                let from_start = x - vol.flash.xyz;
                let along = clamp(dot(from_start, ab) * inv_ab2, 0.0, 1.0);
                let offset = (from_start - ab * along) / reach;
                emitted += flash * exp(-dot(offset, offset) * 2.5) * vec3<f32>(0.7, 0.85, 1.0);
            }
            // hot plasma burning along the edges of the billows
            emitted += vol.high.rgb * vec3<f32>(1.3, 1.0, 0.7) * vol.storm.y * g.y * edge;
            // lightning flickering deep inside: patches that light up and die out a few times a second
            if vol.storm.z > 0.0 {
                let tf = vol.params.x * 2.3;
                let k0 = floor(tf);
                let at0 = bake(q * 0.9 + vec3<f32>(k0 * 3.7, k0 * 1.3, 0.0)).b;
                let at1 = bake(q * 0.9 + vec3<f32>((k0 + 1.0) * 3.7, (k0 + 1.0) * 1.3, 0.0)).b;
                let f = fract(tf);
                let pulse = select(at1 * smoothstep(0.7, 1.0, f), at0 * (1.0 - smoothstep(0.0, 0.5, f)), f < 0.5);
                emitted += vec3<f32>(0.75, 0.85, 1.0) * vol.storm.z * pow(smoothstep(0.45, 0.8, pulse), 2.0) * edge;
            }
            let tau = dens * sigma * dt * share;
            let step_tr = exp(-tau);
            let weight = tr * (1.0 - step_tr);
            light += emitted * weight;
            if vol.alt.w > 0.0 && dens > 0.28 {
                // Break up the pocket with finer knots of the same warped noise instead of blooming as one smooth blob.
                let fine = raw(q * 2.35 + vec3<f32>(2.4, 0.7, 1.8), w * 0.7, clock * 1.4);
                let knots = smoothstep(0.37, 0.72, fine);
                let core = smoothstep(0.33, 0.74, dens) * (0.5 + 0.5 * knots);
                let hot = mix(core_color, vol.high.rgb, 0.22 * knots);
                // The same transmittance keeps intervening dust and folds dark.
                core_light += hot * (vol.alt.w * core * core * weight);
            }
            tr *= step_tr;
        }
        tr *= exp(-tau_dust);
        if tr < 0.02 {
            break;
        }
    }

    // never a wall: one cloud reaches `max_opacity` at most, and less where the ray ended on a hull; the mission's fadeout then
    // scales light and cover alike (premultiplied), so a fading cloud thins out instead of turning into a dark smudge
    let cap = select(vol.low.w, vol.low.w * 0.6, hit_scene);
    let alpha = 1.0 - tr;
    let k = min(1.0, cap / max(alpha, 1.0e-4)) * vol.params.w;
    let glow = core_light * select(1.0, 0.35, hit_scene) * share * vol.params.w;
    out.color = vec4<f32>(light * k + glow, alpha * k);
    return out;
}
