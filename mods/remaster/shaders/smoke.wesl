// Not retail: smoke puffs of the render profile (game/src/render/bursts.rs; contract: `SmokeUniform` and `Built::quad`
// there, our own work under the project licence). Alpha-blended quads, sorted far to near:
// uv.x = kind * 4 + u (7 smoke, 9 splinter, 10 fiber, 11 smoke ribbon), uv.y = seed * 2 + v, vertex colour rgb = colour, a = age share.
// Each puff is shaded as a soft ball: lit on the sun's side, the sky's colour on the other, with ragged, curling edges;
// it thins out as it spreads. A ribbon (kind 11: a shell's smoke trail, u across the strip) is the same ball seen as a
// cylinder: its noise is read from where it hangs in the world, so it curls in place instead of sliding with the strip.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
};

// the battle clock and the sun (game/src/render/frame.rs)
import openprocyon::render::frame::frame;

struct Smoke {
    // rgb: the shadowed side's colour (the sun and the clock are frame.sun, frame.sun_color and frame.clock)
    shade: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> smoke: Smoke;

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
    let kind = floor(in.uv.x / 4.0 + 0.001);
    let u = in.uv.x - kind * 4.0;
    let seed = floor(in.uv.y / 2.0 + 0.001);
    let v = in.uv.y - seed * 2.0;
    let q = vec2<f32>(u, v) * 2.0 - 1.0;
    let age = clamp(in.color.a, 0.0, 1.0);
    let s = seed * 5.17;
    let clock = frame.clock.y;
    if kind > 8.5 && kind < 10.5 {
        var mask: f32;
        var shade: f32;
        if kind < 9.5 {
            // Uneven wedge with a bright cut face and a darker side as it tumbles.
            let edge = (0.85 - u * 0.74) * (0.8 + 0.2 * noise(vec2<f32>(u * 5.0, s)));
            mask = (1.0 - smoothstep(edge - 0.13, edge, abs(q.y - 0.12 * q.x))) * smoothstep(0.0, 0.07, u) * (1.0 - smoothstep(0.94, 1.0, u));
            shade = (0.73 + 0.27 * sin(s + 14.0 * age)) * mix(0.72, 1.0, smoothstep(-0.2, 0.2, q.y));
        } else {
            // A narrow curling strand rather than a luminous trail: easy to distinguish from a spark.
            let curl = 0.50 * sin(3.4 * q.x + s + 4.5 * age);
            mask = (1.0 - smoothstep(0.18, 0.41, abs(q.y - curl))) * smoothstep(0.0, 0.15, u) * (1.0 - smoothstep(0.82, 1.0, u));
            shade = 0.88 + 0.12 * sin(s + 12.0 * age);
        }
        var out: FragmentOutput;
        out.color = vec4<f32>(in.color.rgb * shade, mask * (1.0 - smoothstep(0.48, 1.0, age)));
        return out;
    }

    // ragged edge that curls slowly
    var p = q * 1.8 + vec2<f32>(s, s * 0.7);
    var disc = q;
    if kind > 10.5 {
        let w = in.world_position.xyz;
        p = vec2<f32>(w.x + w.y * 0.6, w.z) * 0.9 + q.x * 0.8 + vec2<f32>(s, s * 0.7);
        disc = vec2<f32>(q.x, 0.0);
    }
    let n = 0.6 * noise(p + vec2<f32>(clock * 0.15, -age * 1.5)) + 0.4 * noise(p * 2.3 - vec2<f32>(age, clock * 0.1));
    let d = length(disc) + 0.5 * (n - 0.5);
    let shape = smoothstep(1.0, 0.25, d);

    // a ball's normal in the camera's frame, turned to the world
    let z = sqrt(max(1.0 - dot(disc, disc), 0.0));
    let right = view.world_from_view[0].xyz;
    let up = view.world_from_view[1].xyz;
    let back = view.world_from_view[2].xyz;
    let normal = normalize(right * disc.x + up * disc.y + back * (z + 0.3));
    let lit = clamp(dot(normal, -frame.sun.xyz) * 0.6 + 0.4, 0.0, 1.0);
    let rgb = in.color.rgb * mix(smoke.shade.rgb + vec3<f32>(0.08), frame.sun_color.rgb, lit) * (0.85 + 0.3 * n);

    let alpha = shape * smoothstep(0.0, 0.06, age) * pow(1.0 - age, 1.3) * 0.75;
    var out: FragmentOutput;
    out.color = vec4<f32>(rgb, alpha);
    return out;
}
