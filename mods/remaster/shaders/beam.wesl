// Not retail: beam guns of the render profile (game/src/render/beams.rs; contract: `BeamUniform` and the mesh built there, our own
// work under the project licence). Drawn additively (ONE/ONE: write unpremultiplied light). Five kinds of quads share the mesh, told
// apart by the whole part of u / 2 (u = 2 * kind + 0..1); vertex colour rgb is the gun's colour, a how bright the quad is now.
// - 0 the beam (u across, v the distance from the muzzle in beam widths): a white-hot thread (its share of the ribbon's half width
//   is `params.x`) in a sheath of the gun's colour, heat streaming out along it in streaks and knots, two strands winding round it;
// - 1 a flare (muzzle, impact; u and v 0..1): a hot centre in a soft halo with a four-pointed star;
// - 2 a ring (the charge closing on the muzzle);
// - 3 a mote (charge motes, impact sparks): a soft dot with a hot centre;
// - 4 an arc (u across, v 0..1 along): a crackling thread that fades to its ends.
import bevy_pbr::render::forward_io::{VertexOutput, FragmentOutput};

// the battle clock (game/src/render/frame.rs)
import openprocyon::render::frame::frame;

struct Beam {
    // rgb: the thread's colour, a: brightness
    core: vec4<f32>,
    // x: the thread's share of the ribbon's half width
    params: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> beam: Beam;

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2(i), hash2(i + vec2<f32>(1.0, 0.0)), w.x), mix(hash2(i + vec2<f32>(0.0, 1.0)), hash2(i + vec2<f32>(1.0, 1.0)), w.x), w.y);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let colour = in.color.rgb;
    let clock = frame.clock.y;
    let kind = floor(in.uv.x * 0.5);
    let u = in.uv.x - 2.0 * kind;
    // centred coordinates of the round kinds
    let q = (vec2<f32>(u, in.uv.y) - vec2<f32>(0.5)) * 2.0;
    let r = length(q);
    var light: vec3<f32>;
    if kind < 0.5 {
        let s = u * 2.0 - 1.0;
        let across = abs(s);
        let y = in.uv.y;
        let x = across / max(beam.params.x, 0.001);
        let thread = exp(-x * x * 1.3);
        // heat streams out along the beam: long streaks, and brighter knots that outrun them
        let streaks = noise(vec2<f32>(s * 2.5, y * 0.09 - clock * 9.0)) * 0.6 + noise(vec2<f32>(s * 6.0, y * 0.3 - clock * 22.0)) * 0.4;
        let knots = pow(0.5 + 0.5 * sin(y * 0.16 - clock * 26.0), 6.0);
        let sheath = pow(max(1.0 - across, 0.0), 2.0) * (0.45 + 0.9 * streaks + 0.5 * knots);
        // two strands winding round the thread (a helix seen side on)
        let wind = 0.3 * sin(y * 0.55 - clock * 17.0);
        let strand = exp(-pow((s - wind) / 0.05, 2.0)) + exp(-pow((s + wind) / 0.05, 2.0));
        light = beam.core.rgb * thread * (1.7 + 0.5 * knots) + colour * (sheath + strand * 0.55);
    } else if kind < 1.5 {
        let halo = pow(max(1.0 - r, 0.0), 2.4);
        let hot = exp(-r * r * 16.0);
        // a four-pointed star, its arms thinning outward, breathing a little
        let arms = exp(-abs(q.x * q.y) * 60.0) * pow(max(1.0 - r, 0.0), 1.5) * (0.75 + 0.25 * sin(clock * 31.0));
        light = beam.core.rgb * hot * 2.2 + colour * (halo * 0.8 + arms * 1.2);
    } else if kind < 2.5 {
        let ring = exp(-pow((r - 0.82) / 0.06, 2.0));
        light = colour * ring * 1.3 + beam.core.rgb * ring * ring * 0.6;
    } else if kind < 3.5 {
        let soft = pow(max(1.0 - r, 0.0), 2.0);
        light = beam.core.rgb * exp(-r * r * 12.0) * 1.6 + colour * soft;
    } else {
        let across = abs(u * 2.0 - 1.0);
        let thread = exp(-across * across * 22.0);
        let glow = pow(max(1.0 - across, 0.0), 2.5);
        let ends = smoothstep(0.0, 0.08, in.uv.y) * (1.0 - smoothstep(0.55, 1.0, in.uv.y));
        light = (beam.core.rgb * thread * 1.5 + colour * glow * 0.6) * ends;
    }
    var out: FragmentOutput;
    out.color = vec4<f32>(light * beam.core.a * in.color.a, 1.0);
    return out;
}
