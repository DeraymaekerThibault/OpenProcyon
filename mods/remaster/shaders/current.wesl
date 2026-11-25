// Not retail: an Etherium current of the render profile (game/src/render/currents.rs; contract: `CurrentUniform` there, our own
// work under the project licence). Drawn additively (ONE/ONE: write unpremultiplied light) on each of the current's two strips:
// u 0..1 across, v along the path in the direction the force pushes (increasing downstream, like the retail scroll).
// A river of glowing gas, in the manner of the profile's nebulae (volume.wesl) and made of the same baked noise: wisps drawn out
// along the flow and running downstream, bent by a slow warp so the streamlines meander. The gas is looked up in a few layers at
// different heights, each shifted by where the view ray crosses it, the upper layers running a little faster, so the river has
// depth and is not a flat card. The middle of the river always holds gas (`fill`) and the noise tears its edges into wisps, so
// the current never breaks. Dense gas turns to the second colour and climbs past 1 so it blooms; slow swells run down the
// current, so the flow direction reads at a glance; a few glints ride downstream faster than the gas. The retail texture stays
// underneath in part, scrolled here at the strip's retail speed by the frame clock, as the retail game scrolls it.
// The river rolls (engine choice, no retail current does): the vertex stage lifts the strip (a denser copy of the retail one, cut in
// currents.rs `refine`; its tangent is the flow's direction) into a few swells running downstream, sharp-crested and broad in the
// troughs, a little skewed across so the crests are not ruler-straight, lower toward the edges. Only up and down, so the outline
// seen from above is the retail one. The normals come from the same formula, so the crests catch the sun and the far slopes dim.
import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    mesh_functions,
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    mesh_view_bindings::view,
};
// the battle clock (game/src/render/frame.rs)
import openprocyon::render::frame::frame;

struct Current {
    // x: brightness, y: wisps across, z: glints, w: core (extra light in dense gas)
    look: vec4<f32>,
    // rgb: colour of thin gas, w: share of the retail texture kept
    color: vec4<f32>,
    // x: the strip's scroll speed (texture heights a second), y: pulse strength, z: fill (gas the middle always holds), w: world
    // units per texture height along the strip
    params: vec4<f32>,
    // rgb: colour of dense gas, w: the strip's width (world units)
    high: vec4<f32>,
    // x: wave height (world units, crest above the mean), y: the main swell's length, z: its speed downstream, w: crest light
    wave: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> current: Current;

// the baked noise (game/src/render/volnoise.rs): r the gas (three octaves), g the warp; it tiles every CELLS cells
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var noise_tex: texture_3d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var noise_samp: sampler;
const CELLS: f32 = 8.0;
// layers the gas is looked up in, and the river's half-height as a share of its width
const LAYERS: i32 = 4;
const HALF_HEIGHT: f32 = 0.16;

fn bake(p: vec3<f32>) -> vec4<f32> {
    return textureSampleLevel(noise_tex, noise_samp, p / CELLS, 0.0);
}

// gas density 0..1 at `x` across (-1..1), `v` along and `h` up (-1..1 of the river's height)
fn gas(x: f32, v: f32, h: f32, flow: f32, t: f32) -> f32 {
    let n = current.look.y;
    // the slow warp bends the streamlines
    let w = bake(vec3<f32>(x * n * 0.07, (v - flow) * n * 0.06, t * 0.05 + h)).g - 0.5;
    // drawn out along the flow; the upper layers run faster
    let along = (v - flow * (1.5 + 0.4 * h)) * n * 0.1;
    let raw = bake(vec3<f32>(x * n * 0.22 + w * 0.9, along + w * 0.5, h * 0.6 + 2.0)).r;
    let body = current.params.z * clamp(1.0 - x * x, 0.0, 1.0) * (1.0 - h * h);
    return smoothstep(0.47, 0.8, raw + body) * smoothstep(1.0, 0.45, abs(x));
}

// the swells' shape: sharp crests (1) and broad, shallow troughs (about -0.6); `.y` its slope per radian
fn swell_shape(a: f32) -> vec2<f32> {
    let e = exp(sin(a));
    return vec2<f32>((e - 1.266) / 1.452, cos(a) * e / 1.452);
}

// the surface's lift at `x` across (-1..1) and `s` world units along the flow: x the height (world units), y its slope along s,
// z its slope across x (per unit of x), w the height as a share of the crest (about -0.6..1)
fn wave_at(x: f32, s: f32, t: f32) -> vec4<f32> {
    let height = current.wave.x;
    let k = 6.2831853 / max(current.wave.y, 1.0);
    let c = current.wave.z;
    // three swells of different lengths and speeds, each a little skewed across
    let ks = vec3<f32>(k, k * 1.73, k * 0.61);
    let a = vec3<f32>(ks.x * (s - c * t) + 0.35 * x, ks.y * (s - 1.2 * c * t) - 0.6 * x + 1.3, ks.z * (s - 0.8 * c * t) + 0.9 * x + 4.1);
    let amp = vec3<f32>(1.0, 0.45, 0.25);
    let skew = vec3<f32>(0.35, -0.6, 0.9);
    let w0 = swell_shape(a.x);
    let w1 = swell_shape(a.y);
    let w2 = swell_shape(a.z);
    let shape = (amp.x * w0.x + amp.y * w1.x + amp.z * w2.x) / 1.7;
    let d_s = (amp.x * w0.y * ks.x + amp.y * w1.y * ks.y + amp.z * w2.y * ks.z) / 1.7;
    let d_x = (amp.x * w0.y * skew.x + amp.y * w1.y * skew.y + amp.z * w2.y * skew.z) / 1.7;
    // lower toward the edges, so the strip's edge stays near the retail one
    let env = 1.0 - 0.55 * x * x;
    return vec4<f32>(height * env * shape, height * env * d_s, height * (env * d_x - 1.1 * x * shape), env * shape);
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let v = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    var wp = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0));
    var n = vec3<f32>(0.0, 1.0, 0.0);
@if(VERTEX_UVS_A) {
    let x = v.uv.x * 2.0 - 1.0;
    let lift = wave_at(x, v.uv.y * current.params.w, frame.clock.y);
    wp.y += lift.x;
@if(VERTEX_TANGENTS) {
    // the flow's direction and the strip's left-to-right one (world, level)
    let along = normalize((world_from_local * vec4<f32>(v.tangent.xyz, 0.0)).xyz + vec3<f32>(0.0, 0.0, 1.0e-6));
    let side = vec3<f32>(-along.z, 0.0, along.x);
    // the slope over the level plane: along s, and across (x runs -1..1 over the width)
    let grad = along * lift.y + side * lift.z * 2.0 / max(current.high.w, 1.0);
    n = normalize(vec3<f32>(-grad.x, 1.0, -grad.z));
    // w carries the lift (no normal map reads it), so the fragment can find the level strip under the swell
    out.world_tangent = vec4<f32>(along, lift.x);
}
    out.uv = v.uv;
}
    out.world_position = wp;
    out.position = position_world_to_clip(wp.xyz);
    out.world_normal = n;
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
    // the retail scroll: the texture slides back along v at the strip's speed (environment.rs `flow_currents`)
    var scrolled = in;
    scrolled.uv.y = in.uv.y - fract(frame.clock.y * current.params.x);
    let input = pbr_input_from_standard_material(scrolled, is_front);
    let tex = input.material.base_color.rgb * current.color.w;

    let x = in.uv.x * 2.0 - 1.0;
    let v = in.uv.y;
    let t = frame.clock.y;
    let flow = t * current.params.x;

    // how far a point one world unit above the strip, seen through this pixel, lies from it across and along (in x and v): the
    // strip's own axes in the world, from how position and uv change over the screen
    // measured on the level strip under the swells: a swell's own slope, which steps from one triangle to the next, would tear the
    // layers apart
    var level = in.world_position.xyz;
@if(VERTEX_TANGENTS)
    level.y -= in.world_tangent.w;
    let px = dpdx(level);
    let py = dpdy(level);
    let ux = dpdx(in.uv);
    let uy = dpdy(in.uv);
    let det = ux.x * uy.y - ux.y * uy.x;
    var shift = vec2<f32>(0.0);
    var width = 1.0;
    if abs(det) > 1.0e-12 {
        let across = (px * uy.y - py * ux.y) / det;
        let down = (py * ux.x - px * uy.x) / det;
        let dir = normalize(in.world_position.xyz - view.world_position);
        // never flatter than a shallow glance, so the layers do not slide apart at the horizon
        let steep = select(-1.0, 1.0, dir.y >= 0.0) * max(abs(dir.y), 0.3);
        let back = dir / steep;
        width = length(across);
        shift = vec2<f32>(2.0 * dot(back, across) / max(dot(across, across), 1.0e-6), dot(back, down) / max(dot(down, down), 1.0e-6));
    }

    var light = vec3<f32>(0.0);
    for (var i = 0; i < LAYERS; i = i + 1) {
        let h = (f32(i) + 0.5) / f32(LAYERS) * 2.0 - 1.0;
        let up = h * HALF_HEIGHT * width;
        let dens = gas(x + shift.x * up, v + shift.y * up, h, flow, t);
        let col = mix(current.color.rgb, current.high.rgb, smoothstep(0.2, 0.9, dens));
        // dense gas past 1, so it blooms
        light += col * dens * (0.25 + 1.5 * current.look.w * dens * dens);
    }
    light = light / f32(LAYERS);

    // slow swells running down the current
    let swell = 0.5 + 0.5 * sin((v - flow * 2.6) * 6.2831853 * 0.8);
    light *= 1.0 - 0.5 * current.params.y + current.params.y * swell * swell;

    // the crests catch the sun, the slopes facing away dim, and the tops of the crests glow a little (none on a flat strip)
    if current.wave.x > 0.0 {
        let n = normalize(in.world_normal);
        let to_sun = select(normalize(vec3<f32>(0.3, 0.8, 0.5)), -normalize(frame.sun.xyz), frame.sun.w > 0.0);
        let dir = normalize(in.world_position.xyz - view.world_position);
        let crest = wave_at(x, v * current.params.w, t).w;
        let facing = dot(n, to_sun) - to_sun.y;
        let gain = current.wave.w;
        // only the glint the tilt adds over a level strip, so a flat stretch seen from above does not wash out
        let half = normalize(to_sun - dir);
        let sheen = max(pow(max(dot(n, half), 0.0), 8.0) - pow(max(half.y, 0.0), 8.0), 0.0);
        // all of it scales the gas that is there (never a glow of its own) and within bounds, so the wisps stay readable through the
        // swells and the crests do not bloom into bands
        let crest_light = 0.4 * (smoothstep(-0.3, 1.0, crest) - 0.4);
        light *= clamp(1.0 + gain * (1.5 * facing + crest_light + 0.6 * sheen), 0.35, 1.7);
    }

    // a few glints ride downstream faster than the gas
    let glint = smoothstep(0.74, 0.8, bake(vec3<f32>(x * current.look.y * 0.9, (v - flow * 4.0) * current.look.y * 2.4, 0.3)).r);
    light += vec3<f32>(1.0, 0.97, 0.9) * glint * current.look.z;

    // the strip's own edge never shows
    let edge = smoothstep(1.0, 0.7, abs(x));
    var out: FragmentOutput;
    out.color = vec4<f32>((tex + light) * edge * current.look.x, 1.0);
    return out;
}
