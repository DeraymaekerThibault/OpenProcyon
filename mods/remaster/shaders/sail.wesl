// Not retail: solar-sail glow of the render profile (game/src/render/sails.rs; contract: `SailGlowUniform` there, our own
// work under the project licence). A mod replaces it by shipping shaders/sail.wesl. The sail is lit as a normal StandardMaterial; over it
// runs a honeycomb of light drawn here (the retail sail textures are too small to carry a crisp one), its cells the same
// size on every sail. Bands of light sweep across the cloth and light whole cells as they pass, each a moment early or
// late; the sweep is stronger and quicker the faster the ship sails. The pattern glows more at grazing angles and where
// the sun shines through the cloth from behind, and melts into an even glow where its cells get too small to draw.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::view,
};

struct SailGlow {
    // rgb: glow colour (linear, may exceed 1); black = a plain sail
    color: vec4<f32>,
    // x: edge width (share of a cell), y: wave length in world units, z: steady glow share, w: cell fill share
    params: vec4<f32>,
    // xyz: direction the sunlight travels (world), w: how much more the sail glows lit from behind
    sun: vec4<f32>,
    // x: the ship's share of full speed (smoothed), y: phase, z: the sail's clock
    motion: vec4<f32>,
    // xy: honeycomb cells per UV unit along u and v
    pattern: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> glow: SailGlow;

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

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);

    let energy = glow.motion.x;
    let phase = glow.motion.y;
    let clock = glow.motion.z;

    let p = in.uv * glow.pattern.xy;
    let h = hex(p);
    let centre = p - h.xy;
    let rnd = hash(centre);
    // anti-aliased edge line, and how many cells a pixel spans (the pattern fades out before it would shimmer)
    let aa = max(fwidth(h.z), 1e-4);
    let edge = 1.0 - smoothstep(glow.params.x, glow.params.x + 1.5 * aa, h.z);
    let span = length(fwidth(p));
    let crisp = 1.0 - smoothstep(0.12, 0.35, span);

    // The band is sampled at each cell's centre, so whole cells light at once, each a little early or late.
    let cuv = centre / glow.pattern.xy;
    let q = fract(cuv.y * 0.55 + cuv.x * 0.25 - clock * 0.22 + phase * 0.159 + rnd * 0.08);
    let band = exp(-pow((q - 0.5) * 6.0, 2.0));
    let twinkle = 0.5 + 0.5 * sin(clock * 2.5 + rnd * 6.2831853);
    let sweep = (0.2 + 0.8 * energy) * (1.5 * band + 0.25 * twinkle);
    // cell: its edge, plus a share of its inside that brightens toward the centre
    let cell = edge + glow.params.w * smoothstep(0.0, 0.5, h.z) * band;
    // far away: the cells' average instead of their lines
    let pattern = mix(glow.params.x * 3.0 + glow.params.w * 0.3, cell, crisp);

    // The camera-side normal: grazing angles glow more (a thin, bright film), and so does sun shining through from behind.
    let to_cam = normalize(view.world_position.xyz - in.world_position.xyz);
    let n = normalize(in.world_normal) * select(-1.0, 1.0, dot(in.world_normal, to_cam) >= 0.0);
    let rim = pow(1.0 - clamp(dot(n, to_cam), 0.0, 1.0), 2.0);
    let backlit = clamp(dot(n, glow.sun.xyz), 0.0, 1.0);

    let strength = pattern * (glow.params.z + sweep) * (1.0 + glow.sun.w * backlit) * (0.7 + 0.8 * rim);
    // The brightest part of the band runs whiter than the glow colour.
    let tint = mix(glow.color.rgb, vec3<f32>(1.0, 0.95, 0.85) * max(glow.color.r, max(glow.color.g, glow.color.b)), band * energy * 0.4);
    out.color = vec4<f32>(out.color.rgb + tint * strength * out.color.a, out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
