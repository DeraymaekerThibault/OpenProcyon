// Not retail: the infall ribbons of a black hole of the render profile (game/src/render/holes.rs; contract: `RibbonUniform` there, our
// own work under the project licence). The retail game draws the hole's vortex trails (blackhole.rs: strips that spiral in from the
// pull radius, widening as they go) as broad, soft orange sheets. The profile keeps that geometry, and with it the spiral motion of
// the original, but draws each strip as a thin, hot, streaking thread: bright toward the core, cooling and fading outward, and gone
// at the kill core, so nothing retail sits in the black. Additive (ONE/ONE: write unpremultiplied light). Vertex colour a: the trail's
// own fade in and out; u runs 0..1 across the strip, v along it (growing with the trail's age).
import bevy_pbr::render::forward_io::{VertexOutput, FragmentOutput};

struct Ribbon {
    // xyz: the hole's centre (world), w: the pull radius
    centre: vec4<f32>,
    // x: the kill core's radius, y: brightness
    shape: vec4<f32>,
    // rgb: the cooler colour
    color: vec4<f32>,
    // x: clock
    params: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> ribbon: Ribbon;

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
    let across = abs(in.uv.x * 2.0 - 1.0);
    let rel = in.world_position.xyz - ribbon.centre.xyz;
    let rho = length(vec2<f32>(rel.x, rel.z));
    let radius = ribbon.centre.w;
    let core = ribbon.shape.x;
    let t = ribbon.params.x;
    // a thin bright thread with a faint fringe, broken into streaks that run along the strip
    let thread = exp(-across * across * 38.0);
    let fringe = pow(max(1.0 - across, 0.0), 4.0) * 0.18;
    let n = noise2(vec2<f32>(in.uv.x * 5.0 + floor(in.uv.y) * 3.7, in.uv.y * 0.9 - t * 1.4));
    let streak = (thread + fringe) * (0.35 + 1.1 * n);
    // hot and white toward the core, red-orange out at the rim
    let heat = 1.0 - smoothstep(core, 0.9 * radius, rho);
    let col = mix(ribbon.color.rgb, vec3<f32>(1.0, 0.9, 0.65), heat * heat);
    let gain = 0.3 + 3.2 * heat * heat;
    // fading out toward the pull radius, gone at the kill core
    let edge = (1.0 - smoothstep(0.75 * radius, radius, rho)) * smoothstep(core * 0.9, core * 1.15, rho);
    var out: FragmentOutput;
    out.color = vec4<f32>(col * streak * gain * edge * in.color.a * ribbon.shape.y, 1.0);
    return out;
}
