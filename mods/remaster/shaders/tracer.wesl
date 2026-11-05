// Not retail: projectile tracers of the render profile (game/src/render/tracers.rs; contract: `TracerUniform` and the
// mesh described at `ribbon` there, our own work under the project licence). Drawn additively (ONE/ONE: write
// unpremultiplied light). Two kinds of quads share the mesh, told apart by u:
// - the streak (u 0..1 across, v the point's age over the tracer's seconds: 0 at the shell): a white-hot thread in a
//   sheath of the bullet's colour that frays into wisps toward the tail, cooling as it goes, with heat running back along it;
// - the head (u 2..3 along the flight, 3 at the front; v 0..1 across): an energy bolt, its white-hot nose at the front
//   in a coloured glow that trails behind, flickering.
// Vertex colour: rgb the bullet's colour, a how much of a gone shell's streak is left.
import bevy_pbr::render::forward_io::{VertexOutput, FragmentOutput};

struct Tracer {
    // rgb: white-hot core colour, a: brightness
    core: vec4<f32>,
    // x: clock
    params: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> tracer: Tracer;

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
    let left = in.color.a;
    let clock = tracer.params.x;
    // out of step from shell to shell
    let seed = fract(dot(floor(in.world_position.xyz * 0.05), vec3<f32>(0.131, 0.217, 0.173))) * 50.0;
    var light: vec3<f32>;
    if in.uv.x >= 2.0 {
        let q = (in.uv - vec2<f32>(2.5, 0.5)) * 2.0;
        // nose near the front, glow trailing behind it
        let nose = length(vec2<f32>((q.x - 0.55) * 2.2, q.y * 1.1));
        let body = length(vec2<f32>((q.x - 0.2) * 0.9, q.y));
        let core = exp(-nose * nose * 6.0);
        let glow = pow(max(1.0 - body, 0.0), 1.8);
        let flicker = 0.85 + 0.15 * sin(clock * 50.0 + seed);
        light = (tracer.core.rgb * core * 2.4 + colour * glow * 1.1) * flicker;
    } else {
        let across = abs(in.uv.x * 2.0 - 1.0);
        let age = clamp(in.uv.y, 0.0, 1.0);
        let fresh = 1.0 - age;
        // wisps: noise streaming back along the streak eats into the sheath, more toward the tail
        let n = noise(vec2<f32>(in.uv.x * 3.0 + seed, age * 7.0 - clock * 6.0)) * 0.65 + noise(vec2<f32>(in.uv.x * 7.0 - seed, age * 15.0 - clock * 11.0)) * 0.35;
        let x = across / mix(1.0, 0.45, age);
        let sheath = pow(max(1.0 - x, 0.0), 1.4) * smoothstep(age * 0.9, age * 0.9 + 0.35, n + 0.25);
        let thread = exp(-x * x * 26.0) * pow(fresh, 2.5);
        let cool = mix(colour, colour * colour * vec3<f32>(1.0, 0.6, 0.45), age);
        light = cool * sheath * pow(fresh, 1.2) * 0.9 + tracer.core.rgb * thread * 1.5;
    }
    var out: FragmentOutput;
    out.color = vec4<f32>(light * tracer.core.a * left, 1.0);
    return out;
}
