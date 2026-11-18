// Not retail: the sky dome of the render profile (game/src/render/sky.rs; contract: `SkyUniform` there, our own work under the
// project licence). Premultiplied-blended on an inverted unit sphere around the camera, behind everything the depth test keeps in front.
// Near layers (up to 3): a sparse soft copy of the sky's own gas (the baked cube, bound at 101), sampled as if it floated
// `distance` units out. The camera's position moves the sample point, so each layer slides against the sky as the camera
// travels (parallax, two speeds for two distances); the map-wide current carries the gas along. Only the brighter gas is kept,
// softly, and a slowly churning 3-D noise (domain-warped by the clock) breaks the copy into wisps and patches.
// Base (optional): the far sky itself, drawn at alpha 1 from two samples that flow along a noise field and cross-fade (the
// flow-map trick), with a slow breath in the bright cores. The stars are never warped.
// Events light the gas, not the stars: a storm's flash and an explosion's warmth (CPU: direction and strength), sheet lightning
// and shooting stars (this clock only: a hash of the time slot picks direction and moment).
// Stars: a separate cube of point stars (16-bit linear light over the gain, bound at 103), each twinkling on a phase and rate
// of its own, the bright ones deeper; the brightest stay above 1 so they bloom. Added as light (the dome is drawn
// premultiplied: rgb is what is added, a how much of the picture behind is kept back).
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
};

struct Layer {
    // x: opacity, y: threshold, z: softness, w: distance
    a: vec4<f32>,
    // x: noise scale, yzw: tint
    b: vec4<f32>,
    // xyz: the current, units a second
    c: vec4<f32>,
}

struct Sky {
    // x: real seconds (not wrapped), y: battle seconds, z: frame delta
    clock: vec4<f32>,
    // x: brightness (0 = none), y: twinkle depth, z: twinkles a second, w: gain
    stars: vec4<f32>,
    // x: layer count, y: 1 when the far sky is redrawn
    info: vec4<f32>,
    // x: flow (radians), y: flow cycle seconds, z: breath depth, w: breath seconds
    base: vec4<f32>,
    base_tint: vec4<f32>,
    // rgb: storm flash colour times what is left; xyz of flash_dir: its world direction
    flash: vec4<f32>,
    flash_dir: vec4<f32>,
    warm: vec4<f32>,
    warm_dir: vec4<f32>,
    // x: shooting star slot seconds (0 = none), y: brightness
    shoot: vec4<f32>,
    // x: sheet lightning slot seconds (0 = none), y: strength
    sheet: vec4<f32>,
    sheet_color: vec4<f32>,
    layers: array<Layer, 3>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> sky: Sky;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var sky_cube: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var sky_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var star_cube: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var star_sampler: sampler;

fn hash3(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3<f32>(127.1, 311.7, 74.7))) * 43758.5453);
}

fn hash1(n: f32) -> f32 {
    return fract(sin(n * 12.9898 + 4.1414) * 43758.5453);
}

// value noise, 0..1
fn noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(mix(hash3(i), hash3(i + vec3<f32>(1.0, 0.0, 0.0)), u.x), mix(hash3(i + vec3<f32>(0.0, 1.0, 0.0)), hash3(i + vec3<f32>(1.0, 1.0, 0.0)), u.x), u.y);
    let b = mix(mix(hash3(i + vec3<f32>(0.0, 0.0, 1.0)), hash3(i + vec3<f32>(1.0, 0.0, 1.0)), u.x), mix(hash3(i + vec3<f32>(0.0, 1.0, 1.0)), hash3(i + vec3<f32>(1.0, 1.0, 1.0)), u.x), u.y);
    return mix(a, b, u.z);
}

// three octaves, 0..1 (about)
fn fbm3(p: vec3<f32>) -> f32 {
    return 0.55 * noise3(p) + 0.3 * noise3(p * 2.03 + vec3<f32>(5.2, 1.3, 8.1)) + 0.15 * noise3(p * 4.1 + vec3<f32>(2.7, 9.2, 3.3));
}

// the cube is left-handed: the baker lays the sky out for the ray with z negated, as Bevy's own skybox samples
fn sample_sky(d: vec3<f32>) -> vec3<f32> {
    return textureSampleLevel(sky_cube, sky_sampler, d * vec3<f32>(1.0, 1.0, -1.0), 0.0).rgb;
}

// the light the events put on the gas: a soft lobe around each direction, in its colour
fn event_light(dir: vec3<f32>) -> vec3<f32> {
    var l = vec3<f32>(0.0);
    l += sky.flash.rgb * pow(max(dot(dir, sky.flash_dir.xyz), 0.0), 3.0);
    l += sky.warm.rgb * pow(max(dot(dir, sky.warm_dir.xyz), 0.0), 2.0);
    if sky.sheet.x > 0.0 {
        let slot = floor(sky.clock.x / sky.sheet.x);
        let at = (sky.clock.x - slot * sky.sheet.x) - hash1(slot) * sky.sheet.x * 0.9;
        if at > 0.0 {
            let d = normalize(vec3<f32>(hash1(slot + 1.0), hash1(slot + 2.0), hash1(slot + 3.0)) - vec3<f32>(0.5));
            // two quick flickers, then out
            let env = exp(-at * 5.0) * (0.6 + 0.4 * sin(at * 38.0));
            l += sky.sheet_color.rgb * sky.sheet.y * env * pow(max(dot(dir, d), 0.0), 2.5);
        }
    }
    return l;
}

// a shooting star: a bright head with a short tail along a great circle, once in a slot of `shoot.x` seconds
fn shooting_star(dir: vec3<f32>) -> f32 {
    if sky.shoot.x <= 0.0 {
        return 0.0;
    }
    let slot = floor(sky.clock.x / sky.shoot.x);
    let life = 1.4;
    let t = (sky.clock.x - slot * sky.shoot.x) - hash1(slot + 11.0) * max(sky.shoot.x - life, 1.0);
    if t < 0.0 || t > life {
        return 0.0;
    }
    let a = normalize(vec3<f32>(hash1(slot + 21.0), hash1(slot + 22.0), hash1(slot + 23.0)) - vec3<f32>(0.5));
    let r = normalize(vec3<f32>(hash1(slot + 31.0), hash1(slot + 32.0), hash1(slot + 33.0)) - vec3<f32>(0.5));
    let b = normalize(cross(a, r));
    let n = cross(a, b);
    let perp = dot(dir, n);
    let s = atan2(dot(dir, b), dot(dir, a));
    let head = t * 0.28;
    let u = head - s;
    if dot(dir, a) < 0.0 || u < 0.0 || u > 0.14 {
        return 0.0;
    }
    let line = exp(-(perp * perp) / (0.0012 * 0.0012));
    return sky.shoot.y * line * (1.0 - u / 0.14) * sin(3.14159 * t / life);
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    let dir = normalize(in.world_position.xyz - view.world_position);
    let clock = sky.clock.x;
    let lit = vec3<f32>(1.0) + event_light(dir);
    var rgb = vec3<f32>(0.0);
    var alpha = 0.0;

    if sky.info.y > 0.5 {
        // two lookups pushed along a slow flow in opposite phases, cross-faded so the gas shimmers without ever jumping
        let t = clock / sky.base.y;
        let p0 = fract(t);
        let p1 = fract(t + 0.5);
        let w = 1.0 - abs(2.0 * p0 - 1.0);
        let flow = vec3<f32>(noise3(dir * 3.0 + vec3<f32>(0.0, 0.0, 1.7)), noise3(dir * 3.0 + vec3<f32>(4.1, 0.0, 0.0)), noise3(dir * 3.0 + vec3<f32>(0.0, 8.3, 0.0))) - vec3<f32>(0.5);
        let c0 = sample_sky(normalize(dir + flow * sky.base.x * p0 * 2.0));
        let c1 = sample_sky(normalize(dir + flow * sky.base.x * p1 * 2.0));
        var col = mix(c1, c0, w);
        // the bright cores breathe
        let core = smoothstep(0.35, 1.2, dot(col, vec3<f32>(0.3333)));
        let breath = sin(clock * 6.2831853 / sky.base.w);
        col = col * (vec3<f32>(1.0) + sky.base.z * breath * core * sky.base_tint.xyz);
        rgb = col * lit;
        alpha = 1.0;
    }

    let count = i32(sky.info.x);
    for (var i = 0; i < 3; i = i + 1) {
        if i >= count {
            break;
        }
        let l = sky.layers[i];
        // a point on the layer, moved by the camera and carried by the current
        let p = dir * l.a.w + view.world_position - l.c.xyz * sky.clock.y;
        // turned and mirrored, so the layer is not the sky's own picture laid over itself
        let q = normalize(p).zxy * vec3<f32>(1.0, -1.0, 1.0);
        let gas = textureSampleLevel(sky_cube, sky_sampler, q, 0.0).rgb * l.b.yzw;
        let brightness = dot(gas, vec3<f32>(0.3333));
        var keep = smoothstep(l.a.y, l.a.y + l.a.z, brightness);
        if l.b.x > 0.0 {
            // wisps: noise on the lookup point, its domain warped slowly so the patches churn in place
            let n = q * l.b.x + vec3<f32>(f32(i) * 7.3);
            let warp = vec3<f32>(noise3(n * 0.7 + vec3<f32>(clock * 0.011, 0.0, 0.0)), noise3(n * 0.7 + vec3<f32>(0.0, clock * 0.013, 3.1)), noise3(n * 0.7 + vec3<f32>(5.7, 1.9, clock * 0.009))) - vec3<f32>(0.5);
            let f = fbm3(n + warp * 1.6 + vec3<f32>(0.0, 0.0, clock * 0.006));
            keep = keep * smoothstep(0.32, 0.68, f);
        }
        let a = keep * l.a.x;
        // premultiplied: this layer over what is below it
        rgb = rgb * (1.0 - a) + gas * lit * a;
        alpha = alpha * (1.0 - a) + a;
    }

    var stars = vec3<f32>(0.0);
    if sky.stars.x > 0.0 {
        let sd = dir * vec3<f32>(1.0, 1.0, -1.0);
        let light = textureSampleLevel(star_cube, star_sampler, sd, 0.0).rgb * sky.stars.w;
        let strength = max(max(light.r, light.g), light.b);
        // one phase and rate for every star in a small patch of sky; faint stars hardly flicker
        let cell = floor(sd * 400.0);
        let phase = hash3(cell) * 6.2831853;
        let rate = sky.stars.z * (0.6 + 0.8 * hash3(cell + vec3<f32>(7.0)));
        let wave = 0.5 + 0.5 * sin(clock * 6.2831853 * rate + phase);
        let depth = sky.stars.y * smoothstep(0.03, 0.6, strength);
        stars = light * (1.0 - depth * wave) * sky.stars.x;
    }
    stars += vec3<f32>(0.85, 0.92, 1.0) * shooting_star(dir);

    var out: FragmentOutput;
    out.color = vec4<f32>(rgb + stars, alpha);
    return out;
}
