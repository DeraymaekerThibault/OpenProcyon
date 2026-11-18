// Not retail: the infall ribbons of a black hole of the render profile (game/src/render/holes.rs; contract: `RibbonUniform` there, our
// own work under the project licence). The retail game draws the hole's vortex trails (blackhole.rs: strips that spiral in from the
// pull radius, widening as they go) as broad, soft orange sheets. The profile keeps that geometry, and with it the spiral motion of
// the original, but draws each strip as a thin, hot, streaking thread: bright toward the core, cooling and fading outward, and gone
// at the kill core, so nothing retail sits in the black. Additive (ONE/ONE: write unpremultiplied light). Vertex colour a: the trail's
// own fade in and out; u runs 0..1 across the strip, v along it (growing with the trail's age).
import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    mesh_functions,
    view_transformations::position_world_to_clip,
};

struct Ribbon {
    // xyz: the hole's centre (world), w: the pull radius
    centre: vec4<f32>,
    // x: the kill core's radius, y: brightness, z: the whirlpool's depth, w: its throat radius (world units)
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

// holes.rs `funnel_drop`: 0 at the pull radius, 1 at the throat
fn funnel_drop(share: f32, throat: f32) -> f32 {
    if share <= throat {
        return 1.0;
    }
    return clamp((1.0 / share - 1.0) / (1.0 / throat - 1.0), 0.0, 1.0);
}

// the strips sink along the whirlpool's funnel, just above its wall, so they spiral down into the throat
@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let v = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    var wp = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0));
    let rel = wp.xyz - ribbon.centre.xyz;
    let share = length(vec2<f32>(rel.x, rel.z)) / max(ribbon.centre.w, 1.0);
    let throat = max(ribbon.shape.w / max(ribbon.centre.w, 1.0), 0.01);
    wp.y -= ribbon.shape.z * funnel_drop(share, throat);
    out.world_position = wp;
    out.position = position_world_to_clip(wp.xyz);
    out.world_normal = vec3<f32>(0.0, 1.0, 0.0);
@if(VERTEX_UVS_A)
    out.uv = v.uv;
@if(VERTEX_COLORS)
    out.color = v.color;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(vertex_in.instance_index, world_from_local[3]);
    return out;
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
    // fading out toward the pull radius; with a whirlpool they dive past the lip and fade into the dark throat, else gone at the core
    let gone = select(core * 0.9, core * 0.35, ribbon.shape.z > 0.0);
    let edge = (1.0 - smoothstep(0.75 * radius, radius, rho)) * smoothstep(gone, gone + core * 0.25, rho);
    var out: FragmentOutput;
    out.color = vec4<f32>(col * streak * gain * edge * in.color.a * ribbon.shape.y, 1.0);
    return out;
}
