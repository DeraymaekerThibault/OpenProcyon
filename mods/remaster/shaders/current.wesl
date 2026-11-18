// Not retail: an Etherium current of the render profile (game/src/render/currents.rs; contract: `CurrentUniform` there, our own
// work under the project licence). Drawn additively (ONE/ONE: write unpremultiplied light) on each of the current's two strips:
// u 0..1 across, v along the path in the direction the force pushes (increasing downstream, like the retail scroll).
// Flowing noise streaks stretched along v with soft edges across; sparkles riding downstream faster than the streaks; slow pulses
// running down the current, so the flow direction reads at a glance; a core along the middle past 1 so it blooms. The retail
// texture stays underneath in part, scrolled here at the strip's retail speed by the frame clock, as the retail game scrolls it.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
};
// the battle clock (game/src/render/frame.rs)
import openprocyon::render::frame::frame;

struct Current {
    // x: brightness, y: streaks across, z: sparkle, w: core
    look: vec4<f32>,
    // rgb: colour, w: share of the retail texture kept
    color: vec4<f32>,
    // x: the strip's scroll speed (texture heights a second), y: pulse strength
    params: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> current: Current;

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2(i), hash2(i + vec2<f32>(1.0, 0.0)), u.x), mix(hash2(i + vec2<f32>(0.0, 1.0)), hash2(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // the retail scroll: the texture slides back along v at the strip's speed (environment.rs `flow_currents`)
    var scrolled = in;
    scrolled.uv.y = in.uv.y - fract(frame.clock.y * current.params.x);
    let input = pbr_input_from_standard_material(scrolled, is_front);
    let tex = input.material.base_color.rgb * current.color.w;

    let u = in.uv.x;
    let v = in.uv.y;
    let flow = frame.clock.y * current.params.x;
    // soft edges across the strip
    let across = sin(3.14159265 * clamp(u, 0.0, 1.0));
    let edge = across * across;

    // streaks: noise stretched along the flow, drifting downstream (v - flow); the two octaves move at slightly different speeds
    let s1 = noise2(vec2<f32>(u * current.look.y, (v - flow * 1.4) * 1.6));
    let s2 = noise2(vec2<f32>(u * current.look.y * 2.3 + 9.0, (v - flow * 2.1) * 3.1));
    let streak = smoothstep(0.35, 0.95, 0.65 * s1 + 0.35 * s2);

    // sparkles ride downstream faster than the streaks
    let sp = vec2<f32>(u * 46.0, (v - flow * 4.0) * 24.0);
    let cell = floor(sp);
    let local = fract(sp) - vec2<f32>(0.5);
    let on = step(0.93, hash2(cell));
    let sparkle = on * smoothstep(0.35, 0.0, length(local)) * (0.6 + 0.4 * sin(frame.clock.y * 7.0 + hash2(cell + vec2<f32>(3.0)) * 6.28));

    // slow pulses running down the current
    let pulse = pow(0.5 + 0.5 * sin((v - flow * 2.6) * 6.2831853 * 0.8), 6.0) * current.params.y;

    // the core along the middle
    let mid = (u - 0.5) / 0.22;
    let core = exp(-mid * mid) * current.look.w;

    let body = current.color.rgb * (streak * (0.5 + pulse) + core * (0.25 + streak * 0.5)) + vec3<f32>(1.0, 0.97, 0.9) * sparkle * current.look.z;
    var out: FragmentOutput;
    out.color = vec4<f32>((tex + body) * edge * current.look.x, 1.0);
    return out;
}
