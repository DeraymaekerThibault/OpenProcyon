// Not retail: a black hole of the render profile (game/src/render/holes.rs; contract: `HoleUniform` there, our own work under
// the project licence). In the battle a hole is a whirlpool: it drags ships round and in, and kills at the core. So it is drawn
// as one: a funnel that sinks under the plane the ships sail (`funnel.depth`), nearly flat at the pull radius and plunging toward
// the core, its water swirling clockwise seen from above (the way the pull turns). The water is gas in the manner of the
// profile's nebulae (volume.wesl), made of the same baked noise: looked up along log spirals, so its wisps are drawn out into
// arms that wind into the core, turning with the hole and running inward, bent by a slow warp. Thin torn wisps at the rim, a
// closed glowing sheet toward the lip (the gas thickens with the heat), so the disc has no holes where it matters.
// Faint violet at the rim, pink and orange further in, hot gold at the lip; the lip (the kill core's edge) is a bright line at
// any zoom, and past it the throat falls away into black. The bent sky and the round shadow are the sky dome's (sky.wesl),
// centred at the bottom of the throat, so the plane itself never bends. With no depth the same pattern lies on a flat disc.
// Premultiplied: light is added, alpha darkens what is behind (a faint tint at the rim, black in the throat). Depth-tested, not
// written, so the ships above it always draw over it.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
};
// the battle clock (game/src/render/frame.rs)
import openprocyon::render::frame::frame;

struct Hole {
    // xyz: centre (world), w: pull radius
    centre: vec4<f32>,
    // x: horizon (lip) radius, y: core radius, z: ring thickness (world units), w: lensing strength
    shape: vec4<f32>,
    // x: disc inner radius, y: outer radius (world units), z: spin (turns a second), w: brightness
    disc: vec4<f32>,
    // rgb: the hot colour (toward the core)
    color: vec4<f32>,
    // x: swallow flash 0..1, y: glow (the clock is frame.clock)
    params: vec4<f32>,
    // x: funnel depth, y: throat radius, z: how far under the plane the rim lies (world units)
    funnel: vec4<f32>,
    // rgb: the outer water
    rim: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> hole: Hole;

// the baked noise (game/src/render/volnoise.rs): r the gas (three octaves), g the warp; it tiles every CELLS cells
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var noise_tex: texture_3d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var noise_samp: sampler;
const CELLS: f32 = 8.0;

fn bake(p: vec3<f32>) -> vec4<f32> {
    return textureSampleLevel(noise_tex, noise_samp, p / CELLS, 0.0);
}

// the noise along a log spiral: the angle `a` goes round a circle of `k` cells (so there is no seam), the log of the distance
// runs along the third axis
fn spiral(a: f32, k: f32, along: f32, at: vec2<f32>) -> vec4<f32> {
    return bake(vec3<f32>(cos(a) * k + at.x, sin(a) * k + at.y, along));
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    let p = in.world_position.xyz;
    let c = hole.centre.xyz;
    let radius = hole.centre.w;
    let lip = hole.shape.x;
    let flash = hole.params.x;
    let t = frame.clock.y;
    let bright = hole.disc.w;

    let rel = p - c;
    let rho = length(vec2<f32>(rel.x, rel.z));
    let px = max(fwidth(rho), 1e-4);
    // how far down the funnel this point is (0 at the rim, 1 at the throat)
    let deep = select(0.0, clamp((c.y - hole.funnel.z - p.y) / max(hole.funnel.x, 1e-3), 0.0, 1.0), hole.funnel.x > 0.0);

    // heat: 0 at the rim, 1 at the lip
    let heat = 1.0 - smoothstep(lip, radius, rho);

    // the swirl: gas looked up along log spirals turning clockwise seen from above, sliding inward along them; a turning log
    // spiral looks like water running into the core
    let turn = hole.disc.z * t * 6.2831853;
    let theta = atan2(-rel.z, rel.x) + turn;
    let lr = log(max(rho, 1.0) / max(radius, 1.0));
    let w = spiral(theta + 2.2 * lr, 0.9, lr * 0.7 + t * 0.03 + 1.3, vec2<f32>(4.0)).g - 0.5;
    let broad = spiral(theta + 2.6 * lr + w * 0.9, 1.6, lr * 0.9 + t * 0.05 + w * 0.5, vec2<f32>(4.0)).r;
    let fine = spiral(theta + 3.4 * lr + w * 0.5, 3.7, lr * 2.0 + t * 0.09 + 5.0, vec2<f32>(2.0)).r;
    // thicker toward the lip, where it closes into a sheet
    let dens = smoothstep(0.44, 0.76, 0.6 * broad + 0.4 * fine + 0.16 * heat);
    let flow = dens * (0.2 + 0.8 * dens);

    var col = mix(hole.rim.rgb, vec3<f32>(0.85, 0.25, 0.55), smoothstep(0.15, 0.55, heat));
    col = mix(col, hole.color.rgb, smoothstep(0.5, 0.85, heat));
    col = mix(col, vec3<f32>(1.0, 0.85, 0.55), smoothstep(0.88, 1.0, heat));
    let glow = (0.1 + 0.35 * heat + 1.0 * pow(heat, 4.0)) * bright;

    // past the lip the throat falls away into the dark
    let fall = smoothstep(lip, lip * 0.7, rho);
    // the walls darken as they go down, so the funnel has depth
    let shade = 1.0 - 0.75 * smoothstep(0.05, 0.4, deep);
    var light = col * flow * glow * shade * (1.0 - fall);

    // the lip: a bright, thin line on the kill core's edge
    let ring_w = max(hole.shape.z, 1.5 * px);
    let ring = exp(-pow((rho - lip) / ring_w, 2.0));
    light += vec3<f32>(1.0, 0.88, 0.62) * ring * (1.4 + 5.0 * flash) * bright;
    // a warm glow hugging the lip, outside it
    light += hole.color.rgb * exp(-max(rho - lip, 0.0) / (0.06 * radius)) * (1.0 - fall) * hole.params.y;
    light *= 1.0 + 2.0 * flash;

    // the rim fades in from nothing: no hard edge at the pull radius
    let edge = 1.0 - smoothstep(0.7 * radius, radius, rho);
    // the gas dims what is behind it a little, more toward the core; the throat is black
    let tint = (0.12 + 0.6 * heat * heat) * dens;
    let alpha = clamp(max(fall, max(tint * edge, ring * 0.5)), 0.0, 1.0);
    var out: FragmentOutput;
    out.color = vec4<f32>(light * edge, alpha);
    return out;
}
