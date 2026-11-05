// Not retail: glowing burst particles of the render profile (game/src/render/bursts.rs; contract: `BurstUniform` and
// `Built::quad` there, our own work under the project licence). Drawn additively (ONE/ONE: write unpremultiplied light).
// Every glowing kind shares one mesh: uv.x = kind * 4 + u, uv.y = seed * 2 + v (u, v 0..1 across the quad), vertex
// colour rgb the particle's colour, a its age as a share of its life (for ether dust: how faded it is).
// Kinds: 0 flash, 1 spark (u runs tail to head), 2 ember, 3 fireball, 4 shockwave ring (flat), 5 hull scorch, 6 dust.
import bevy_pbr::render::forward_io::{VertexOutput, FragmentOutput};

struct Burst {
    // x: clock
    params: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> burst: Burst;

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2(i), hash2(i + vec2<f32>(1.0, 0.0)), w.x), mix(hash2(i + vec2<f32>(0.0, 1.0)), hash2(i + vec2<f32>(1.0, 1.0)), w.x), w.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    return 0.55 * noise(p) + 0.3 * noise(p * 2.1 + 3.7) + 0.15 * noise(p * 4.3 + 9.1);
}

// Black body-ish ramp: 1 white-hot, 0.6 yellow-orange, 0.3 deep red, 0 dark.
fn heat(t: f32) -> vec3<f32> {
    let x = clamp(t, 0.0, 1.0);
    return vec3<f32>(smoothstep(0.0, 0.35, x), smoothstep(0.2, 0.75, x), smoothstep(0.6, 1.0, x)) * (0.3 + 1.2 * x);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let kind = floor(in.uv.x / 4.0 + 0.001);
    let u = in.uv.x - kind * 4.0;
    let seed = floor(in.uv.y / 2.0 + 0.001);
    let v = in.uv.y - seed * 2.0;
    let q = vec2<f32>(u, v) * 2.0 - 1.0;
    let d = length(q);
    let age = clamp(in.color.a, 0.0, 1.0);
    let colour = in.color.rgb;
    let clock = burst.params.x;
    let s = seed * 7.31;
    var light = vec3<f32>(0.0);

    if kind < 0.5 {
        // flash: a white core and a few uneven rays, gone in a blink
        let a = atan2(q.y, q.x);
        let rays = pow(abs(cos(a * 3.0 + s)), 18.0) * 0.6 + pow(abs(cos(a * 5.0 + s * 1.7)), 30.0) * 0.4;
        let core = exp(-d * d * 9.0);
        let f = pow(1.0 - age, 2.0);
        light = (mix(colour, vec3<f32>(1.0), core) * (core * 3.0 + rays * max(1.0 - d, 0.0) * 1.5)) * f;
    } else if kind < 1.5 {
        // spark: a streak bright at its head, white-hot cooling to the spark's colour, then red
        let across = exp(-q.y * q.y * 5.0);
        let along = pow(u, 1.6) * smoothstep(0.0, 0.15, 1.0 - u + 0.15);
        let hot = mix(vec3<f32>(1.2, 1.1, 0.9), colour, smoothstep(0.0, 0.5, age));
        let c = mix(hot, colour * vec3<f32>(1.0, 0.4, 0.15), smoothstep(0.5, 1.0, age));
        light = c * across * along * 2.5 * (1.0 - age * age);
    } else if kind < 2.5 {
        // ember: a speck that flickers as it drifts and dims
        let flicker = 0.55 + 0.45 * sin(clock * (14.0 + seed * 0.3) + s);
        let dot = exp(-d * d * 7.0);
        light = colour * dot * flicker * 1.8 * smoothstep(0.0, 0.08, age) * (1.0 - age);
    } else if kind < 3.5 {
        // fireball: a churning ball cooling from white through orange to a dull red as it swells
        let n = fbm(q * 2.2 + vec2<f32>(s, -age * 2.5));
        let edge = d + 0.45 * (n - 0.5);
        let body = smoothstep(1.0, 0.35, edge);
        let t = (1.0 - age) * (0.55 + 0.6 * n) * (1.2 - edge);
        light = mix(heat(t), heat(t) * colour * 1.4, 0.4) * body * 2.2 * pow(1.0 - age, 1.2);
    } else if kind < 4.5 {
        // shockwave: a thin bright ring spreading over the plane, breaking up as it goes
        let w = 0.06 + 0.1 * age;
        let ring = exp(-pow((d - 0.85) / w, 2.0));
        let a = atan2(q.y, q.x);
        let broken = 0.6 + 0.4 * noise(vec2<f32>(a * 6.0 + s, age * 3.0));
        light = colour * ring * broken * 2.0 * pow(1.0 - age, 1.6);
    } else if kind < 5.5 {
        // scorch: the struck plating glows white-orange and cools to a dull red, unevenly
        let n = fbm(q * 3.0 + s);
        let blob = smoothstep(1.0, 0.1, d + 0.5 * (n - 0.5));
        let t = (1.0 - age) * (0.5 + 0.7 * n);
        let flicker = 0.9 + 0.1 * sin(clock * 9.0 + s);
        light = heat(t * 0.9) * colour * 1.6 * blob * pow(1.0 - age, 1.5) * flicker;
    } else {
        // ether dust: faint specks twinkling in the light
        let tw = 0.6 + 0.4 * sin(clock * (0.7 + seed * 0.05) + s);
        light = colour * exp(-d * d * 6.0) * 0.35 * tw * (1.0 - age);
    }

    var out: FragmentOutput;
    out.color = vec4<f32>(max(light, vec3<f32>(0.0)), 1.0);
    return out;
}
