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
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
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
    // x: the strip's scroll speed (texture heights a second), y: pulse strength, z: fill (gas the middle always holds)
    params: vec4<f32>,
    // rgb: colour of dense gas
    high: vec4<f32>,
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
    let px = dpdx(in.world_position.xyz);
    let py = dpdy(in.world_position.xyz);
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

    // a few glints ride downstream faster than the gas
    let glint = smoothstep(0.74, 0.8, bake(vec3<f32>(x * current.look.y * 0.9, (v - flow * 4.0) * current.look.y * 2.4, 0.3)).r);
    light += vec3<f32>(1.0, 0.97, 0.9) * glint * current.look.z;

    // the strip's own edge never shows
    let edge = smoothstep(1.0, 0.7, abs(x));
    var out: FragmentOutput;
    out.color = vec4<f32>((tex + light) * edge * current.look.x, 1.0);
    return out;
}
