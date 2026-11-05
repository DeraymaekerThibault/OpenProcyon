// Not retail: engine exhaust of the render profile (game/src/render/trails.rs; contract: `PlumeUniform` there, our own
// work under the project licence). Drawn additively (ONE/ONE: write
// unpremultiplied light, alpha ignored) on cones trailing astern of each
// engine: a long outer flame in the faction colour and a short white-hot inner core. The cone's u runs around it, v from
// the nozzle (0) to the tail (1). Dense where the camera looks through the cone's middle, licking tongues of flame at its
// outline, turbulence streaming astern, faint shock diamonds near the nozzle and a bright glow right at it; thrust scales
// all of it.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
};

struct Plume {
    // rgb: the hot core colour (linear, may exceed 1)
    core: vec4<f32>,
    // rgb: the flame colour, w: 1 for the inner core layer
    edge: vec4<f32>,
    // x: brightness, y: thrust (0..1, smoothed), z: clock, w: seed
    params: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> plume: Plume;

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

// Value noise that wraps every `period` cells along x (around the cone).
fn noise(p: vec2<f32>, period: f32) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    let x0 = i.x - period * floor(i.x / period);
    let x1 = (x0 + 1.0) - period * floor((x0 + 1.0) / period);
    let a = hash2(vec2<f32>(x0, i.y));
    let b = hash2(vec2<f32>(x1, i.y));
    let c = hash2(vec2<f32>(x0, i.y + 1.0));
    let d = hash2(vec2<f32>(x1, i.y + 1.0));
    return mix(mix(a, b, w.x), mix(c, d, w.x), w.y);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let thrust = plume.params.y;
    let clock = plume.params.z;
    let seed = plume.params.w;
    let inner = plume.edge.w;
    let v = in.uv.y;
    let u = in.uv.x;

    // Turbulence streaming astern, faster with thrust.
    let flow = clock * (2.5 + 5.0 * thrust);
    let n = 0.55 * noise(vec2<f32>(u * 8.0, v * 5.0 - flow + seed), 8.0) + 0.3 * noise(vec2<f32>(u * 16.0, v * 11.0 - flow * 1.6 + seed * 3.0), 16.0)
        + 0.15 * noise(vec2<f32>(u * 32.0, v * 23.0 - flow * 2.3 + seed * 7.0), 32.0);

    // Dense through the middle; the outline breaks into tongues of flame that lick further toward the tail.
    let to_cam = normalize(view.world_position.xyz - in.world_position.xyz);
    let facing = abs(dot(normalize(in.world_normal), to_cam));
    let body = smoothstep(0.05, 0.85, pow(facing, 1.3) + (0.25 + 0.35 * v) * (n - 0.5));

    // Shock diamonds: bright bands in the first half at high thrust.
    let diamonds = 1.0 + 0.5 * thrust * pow(max(sin(v * 34.0 - clock * 1.5), 0.0), 6.0) * (1.0 - smoothstep(0.05, 0.5, v));
    // Bright at the nozzle, gone by the tail; the tail frays with the turbulence.
    let along = smoothstep(0.0, 0.02, v) * pow(1.0 - v, 1.5) * smoothstep(0.0, 0.3, 1.0 - v + 0.3 * (n - 0.5));
    let nozzle = 2.0 * exp(-v * 16.0) * pow(facing, 2.0);

    // The outer flame is the faction colour, a little hotter in its middle; the inner core runs from white to that colour.
    let heat = select(pow(facing, 6.0) * pow(1.0 - v, 4.0) * 0.4, pow(1.0 - v, 1.5), inner > 0.5);
    let colour = mix(plume.edge.rgb, plume.core.rgb, clamp(heat, 0.0, 1.0));
    let glow = plume.params.x * (0.15 + 0.85 * thrust) * (body * along * (0.6 + 0.8 * n) * diamonds + nozzle * (0.3 + 0.7 * inner));

    var out: FragmentOutput;
    out.color = vec4<f32>(colour * glow, 1.0);
    return out;
}
