// Not retail: solar-sail glow of the render profile (game/src/render/sails.rs; contract: `SailGlowUniform` there, our own
// work under the project licence). The sail is lit as a normal StandardMaterial; over it
// runs a honeycomb of light drawn here (the retail sail textures are too small to carry a crisp one), its cells the same
// size on every sail. Bolts of light crawl in from the rim along the cells' edges, wandering a cell or so either way, and
// gather where the cloth is held (a Navy fan's hub; the bottom edge of other sails), which glows; they run quicker the faster
// the ship sails. The pattern glows more at grazing angles and where the sun shines through the cloth from behind, and melts
// into an even glow where its cells get too small to draw.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::view,
};
import openprocyon::render::frame::frame;

struct SailGlow {
    // rgb: glow colour (linear, may exceed 1); black = a plain sail
    color: vec4<f32>,
    // x: edge width (share of a cell), y: wave length in world units, z: steady glow share, w: cell fill share
    params: vec4<f32>,
    // w: how much more the sail glows lit from behind (the sun's direction is the frame data's)
    sun: vec4<f32>,
    // y: phase, w: the sail's row in `sail_state`
    motion: vec4<f32>,
    // xy: honeycomb cells per UV unit along u and v
    pattern: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> glow: SailGlow;
// what moves, a row for every sail: x the ship's share of full speed (smoothed), y the sail's clock
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var<storage, read> sail_state: array<vec4<f32>>;

fn modulo(x: vec2<f32>, y: vec2<f32>) -> vec2<f32> {
    return x - y * floor(x / y);
}

// Honeycomb of cells one unit wide: xy = offset from the nearest cell centre, z = distance to that cell's edge (0.5 at
// the centre, 0 on the edge).
fn hex(p: vec2<f32>) -> vec3<f32> {
    let r = vec2<f32>(1.0, 1.7320508);
    let h = r * 0.5;
    let a = modulo(p, r) - h;
    let b = modulo(p - h, r) - h;
    let g = select(b, a, dot(a, a) < dot(b, b));
    let q = abs(g);
    return vec3<f32>(g, 0.5 - max(dot(q, vec2<f32>(0.5, 0.8660254)), q.x));
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

// Smooth 1D noise in 0..1, one bump a unit.
fn noise(x: f32, seed: f32) -> f32 {
    let i = floor(x);
    return mix(hash(vec2<f32>(i, seed)), hash(vec2<f32>(i + 1.0, seed)), smoothstep(0.0, 1.0, x - i));
}

// Bolt paths about the focus: each fan or sail has this many, one bolt crawling in along each at a time.
const PATHS: f32 = 18.0;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);

    let state = sail_state[u32(glow.motion.w)];
    let energy = state.x;
    let phase = glow.motion.y;
    let clock = state.y;

    let p = in.uv * glow.pattern.xy;
    let h = hex(p);
    let centre = p - h.xy;
    // anti-aliased edge line, and how many cells a pixel spans (the pattern fades out before it would shimmer)
    let aa = max(fwidth(h.z), 1e-4);
    let edge = 1.0 - smoothstep(glow.params.x, glow.params.x + 1.5 * aa, h.z);
    let span = max(length(fwidth(p)), 1e-5);
    let crisp = 1.0 - smoothstep(0.12, 0.35, span);

    // The cloth about its focus (the engine's `uv_b`): x 0 at the focus .. 1 at the rim, y the share of the way around. A
    // sail the engine did not cut has none: no glow where the light gathers, its bolts run down its UVs.
    var focus = vec2<f32>(1.0 - fract(in.uv.y), fract(in.uv.x));
    var core = 0.0;
    @if(VERTEX_UVS_B) {
        focus = in.uv_b;
        core = exp(-focus.x * 9.0);
    }
    let d = focus.x;
    let around = focus.y;
    // how far around one cell reaches here (more toward the focus, where the paths close in); the wrapped copy hides the seam
    let per_cell = max(min(length(fwidth(around)), length(fwidth(fract(around + 0.5)))), 1e-6) / span;

    var bolt = 0.0;
    var head = 0.0;
    for (var j = -1; j <= 1; j++) {
        let k = floor(around * PATHS) + f32(j);
        let kk = k - PATHS * floor(k / PATHS);
        let seed = hash(vec2<f32>(kk, phase));
        // the path wanders around as it runs in, a bump every seventh of the way; on the edges it zig-zags cell to cell
        let line = (kk + 0.5 + (seed - 0.5) * 0.6 + (noise(d * 7.0, seed * 91.0) - 0.5) * 1.4) / PATHS;
        let off = abs(fract(around - line + 0.5) - 0.5) / per_cell;
        let on = 1.0 - smoothstep(0.25, 0.5, off);
        // the bolt: its tip runs from beyond the rim to the focus, a fading tail behind it, then a rest before the next one
        let t = fract(clock * (0.22 + 0.14 * seed) + seed * 7.3);
        let tip = 1.3 - 1.8 * t;
        let behind = d - tip;
        let pulse = select(exp(-behind * 4.0), exp(behind * 60.0), behind < 0.0) * step(0.0, tip + 0.05);
        bolt = max(bolt, on * pulse);
        head = max(head, on * exp(-abs(behind) * 25.0));
    }
    // it crackles: each cell drops out now and then
    let crackle = 0.55 + 0.45 * step(0.3, hash(centre + floor(clock * 14.0)));
    // a bolt starts faint at the rim and brightens as it nears where the light gathers
    let gather = mix(0.06, 1.25, pow(1.0 - clamp(d, 0.0, 1.0), 2.0));
    let lit = (0.2 + 0.8 * energy) * bolt * crackle * gather;
    // cell: its edge, plus a share of its inside where the bolt's tip is
    let cell = edge * (1.0 + 0.5 * core) + glow.params.w * smoothstep(0.0, 0.5, h.z) * (head * gather + core);
    // far away: the cells' average instead of their lines
    let pattern = mix(glow.params.x * 3.0 + glow.params.w * 0.3, cell, crisp);

    // The camera-side normal: grazing angles glow more (a thin, bright film), and so does sun shining through from behind.
    let to_cam = normalize(view.world_position.xyz - in.world_position.xyz);
    let n = normalize(in.world_normal) * select(-1.0, 1.0, dot(in.world_normal, to_cam) >= 0.0);
    let rim = pow(1.0 - clamp(dot(n, to_cam), 0.0, 1.0), 2.0);
    let backlit = clamp(dot(n, frame.sun.xyz), 0.0, 1.0);

    // the gathered light at the focus glows through the cloth, not only along the edges
    let strength = (pattern * (glow.params.z + 1.6 * lit) + core * (0.3 + 0.5 * energy)) * (1.0 + glow.sun.w * backlit) * (0.7 + 0.8 * rim);
    // The bolts' tips and the focus run a hotter amber than the glow colour, never white.
    let tint = mix(glow.color.rgb, vec3<f32>(1.0, 0.52, 0.14) * max(glow.color.r, max(glow.color.g, glow.color.b)), clamp(head + core, 0.0, 1.0) * 0.35);
    out.color = vec4<f32>(out.color.rgb + tint * strength * out.color.a, out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
