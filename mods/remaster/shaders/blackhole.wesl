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
    // the black pit at the bottom: only the throat is black, the rest of the kill core is hot gas
    let pit = lip;

    let rel = p - c;
    let rho = length(vec2<f32>(rel.x, rel.z));
    let px = max(fwidth(rho), 1e-4);
    let deep = select(0.0, clamp((c.y - hole.funnel.z - p.y) / max(hole.funnel.x, 1e-3), 0.0, 1.0), hole.funnel.x > 0.0);

    // heat: 0 at the rim, 1 at the lip
    let heat = 1.0 - smoothstep(lip, radius, rho);
    // hotness by distance from the pit alone (the lip is the kill zone, not a colour boundary): pale only in the eye
    let hot = pow(1.0 - smoothstep(pit, 0.75 * radius, rho), 2.2);
    let eye = 1.0 - smoothstep(1.05 * lip, 1.9 * lip, rho);

    // gas looked up along log spirals turning clockwise seen from above, sliding inward: water running into the core
    let turn = hole.disc.z * t * 6.2831853;
    let theta = atan2(-rel.z, rel.x) + turn;
    let lr = log(max(rho, 1.0) / max(radius, 1.0));
    let w = spiral(2.0 * (theta - 2.0 * lr), 0.9, lr * 0.7 + t * 0.03 + 1.3, vec2<f32>(4.0)).g - 0.5;
    let wound = theta - 1.9 * lr + w * 1.4;

    // two broad arms with dark gaps (the film's hole is two opposing streams of gas wound round a pale core); the gaps close
    // toward the lip so the eye is a full sheet
    let arm = pow(0.5 + 0.5 * cos(2.0 * wound), 1.2);
    let arms = mix(0.08 + 0.92 * arm, 1.0, eye);

    // fibres: gas drawn out into long strands along the flow (fine across it), at three scales, sharpened so the light sits in
    // thin bright threads on a dark red ground
    let f1 = spiral(2.0 * wound, 4.0, lr * 0.5 + t * 0.03 + 2.0, vec2<f32>(4.0)).r;
    let f2 = spiral(2.0 * (wound + w * 0.4 + (f1 - 0.5) * 0.6), 7.5, lr * 0.4 + t * 0.05 + 7.0, vec2<f32>(2.0)).r;
    let f3 = spiral(2.0 * wound, 12.0, lr * 0.3 + t * 0.07 + 3.0, vec2<f32>(6.0)).r;
    let strands = clamp(0.45 * f1 + 0.35 * f2 + 0.2 * f3, 0.0, 1.0);
    let fibre = smoothstep(0.3, 0.75, strands);
    // a coarse billow so the bands are not even
    let billow = spiral(2.0 * (theta - 1.2 * lr), 1.3, lr * 0.8 + t * 0.02 + 5.0, vec2<f32>(4.0)).r;

    let body = arms * (0.25 + 0.95 * fibre) * (0.55 + 0.9 * billow);
    let dens = smoothstep(0.1, 0.8, body + 0.2 * eye);

    // one fire ramp, by how dense and hot the gas is: dark crimson, deep orange, amber, and only in the eye pale cream
    let lum = clamp(dens * (0.45 + 0.4 * hot) + 0.4 * eye * eye, 0.0, 1.0);
    var col = mix(hole.rim.rgb, vec3<f32>(0.7, 0.14, 0.03), smoothstep(0.05, 0.3, lum));
    col = mix(col, hole.color.rgb, smoothstep(0.25, 0.6, lum));
    col = mix(col, vec3<f32>(1.0, 0.42, 0.08), smoothstep(0.55, 0.9, lum));
    col = mix(col, vec3<f32>(1.0, 0.78, 0.45), eye * smoothstep(0.5, 1.0, lum));
    let glow = (0.02 + 0.75 * pow(dens, 1.5) * (0.4 + 0.6 * hot) + 0.6 * eye) * bright;

    // the pit falls away into the dark, softly
    let fall = smoothstep(pit * 1.02, pit * 0.8, rho);
    // the walls darken a little as they go down
    let shade = 1.0 - 0.35 * smoothstep(0.05, 0.5, deep);
    var light = col * glow * shade * (1.0 - fall);

    // thin hot veins crawling through the gas, brightest on the arms: the lightning of the film
    let vein = pow(1.0 - smoothstep(0.0, 0.06, abs(f2 - 0.5)), 3.0) * arms * smoothstep(0.1, 0.5, lum);
    light += vec3<f32>(1.0, 0.6, 0.25) * vein * (0.5 + heat) * bright * 0.6 * (1.0 - fall);

    // the lip marks the kill zone at any zoom: a thin line, faint unless something is swallowed
    let ring_w = max(hole.shape.z, 1.5 * px);
    let ring = exp(-pow((rho - lip) / ring_w, 2.0));
    light += vec3<f32>(1.0, 0.8, 0.5) * ring * (0.18 + 5.0 * flash) * bright;
    light += hole.color.rgb * exp(-max(rho - lip, 0.0) / (0.05 * radius)) * (1.0 - fall) * hole.params.y * 0.5;
    light *= 1.0 + 2.0 * flash;

    // the rim breaks into wisps instead of ending on a smooth line
    let edge = (1.0 - smoothstep(0.55 * radius, radius, rho + (fibre - 0.5) * 0.25 * radius)) * (1.0 - smoothstep(0.78 * radius, radius, rho));
    // the gas dims what is behind it, dark crimson in the gaps; the pit is black
    let tint = (0.25 + 0.6 * heat) * dens;
    let alpha = clamp(max(fall, max(tint * edge, ring * 0.4)), 0.0, 1.0);
    var out: FragmentOutput;
    out.color = vec4<f32>(light * edge, alpha);
    return out;
}
