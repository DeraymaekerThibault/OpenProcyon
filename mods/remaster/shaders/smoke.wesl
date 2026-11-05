// Not retail: smoke puffs of the render profile (game/src/render/bursts.rs; contract: `SmokeUniform` and `Built::quad`
// there, our own work under the project licence). Alpha-blended camera-facing quads, sorted far to near:
// uv.x = 7 * 4 + u, uv.y = seed * 2 + v, vertex colour rgb the smoke's colour, a its age as a share of its life.
// Each puff is shaded as a soft ball: lit on the sun's side, the sky's colour on the other, with ragged, curling edges;
// it thins out as it spreads.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
};

struct Smoke {
    // xyz: direction the sunlight travels, w: clock
    sun: vec4<f32>,
    // rgb: sunlight colour
    light: vec4<f32>,
    // rgb: the shadowed side's colour
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
    let clock = smoke.sun.w;

    // ragged edge that curls slowly
    let p = q * 1.8 + vec2<f32>(s, s * 0.7);
    let n = 0.6 * noise(p + vec2<f32>(clock * 0.15, -age * 1.5)) + 0.4 * noise(p * 2.3 - vec2<f32>(age, clock * 0.1));
    let d = length(q) + 0.5 * (n - 0.5);
    let shape = smoothstep(1.0, 0.25, d);

    // a ball's normal in the camera's frame, turned to the world
    let z = sqrt(max(1.0 - dot(q, q), 0.0));
    let right = view.world_from_view[0].xyz;
    let up = view.world_from_view[1].xyz;
    let back = view.world_from_view[2].xyz;
    let normal = normalize(right * q.x + up * q.y + back * (z + 0.3));
    let lit = clamp(dot(normal, -smoke.sun.xyz) * 0.6 + 0.4, 0.0, 1.0);
    let rgb = in.color.rgb * mix(smoke.shade.rgb + vec3<f32>(0.08), smoke.light.rgb, lit) * (0.85 + 0.3 * n);

    let alpha = shape * smoothstep(0.0, 0.06, age) * pow(1.0 - age, 1.3) * 0.75;
    var out: FragmentOutput;
    out.color = vec4<f32>(rgb, alpha);
    return out;
}
