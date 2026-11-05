// Not retail: wake trail of the render profile (game/src/render/trails.rs; contract: `WakeUniform` there, our own work
// under the project licence). Drawn additively (ONE/ONE: write
// unpremultiplied light). The retail ribbon (wake.rs) is drawn as crossed
// ribbons along the same path (the flat retail one and two leaning either way), so the trail has depth from every angle:
// u runs across a ribbon, v along it (0.5 per segment, the newest at the ship), vertex colour carries the retail fade.
// Each ribbon shows the retail wake texture's streaks with soft edges and drifting ripples, fades as it turns edge-on, and
// runs hot near the ship, cooling into the faction tint.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_bindings::{base_color_texture, base_color_sampler},
    mesh_view_bindings::view,
};

struct WakeGlow {
    // rgb: the wake's colour (the faction tint), a: brightness
    tint: vec4<f32>,
    // rgb: colour near the ship
    hot: vec4<f32>,
    // x: v at the ship end (the trail's length), y: clock, z: core thread strength, w: unused
    params: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> wake: WakeGlow;

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
    let clock = wake.params.y;
    let fade = in.color.r;
    let across = abs(in.uv.x * 2.0 - 1.0);
    // 0 at the ship, 1 at the oldest end
    let age = clamp(1.0 - in.uv.y / max(wake.params.x, 1e-3), 0.0, 1.0);

    // Ripples drifting slowly back along the trail; they also bend the soft edge.
    let q = vec2<f32>(in.uv.x * 3.0, in.uv.y * 1.2 - clock * 0.3);
    let n = 0.6 * noise(q) + 0.4 * noise(q * 2.3 + 4.1);
    let ripples = 0.55 + 0.9 * n;
    let edge = 1.0 - smoothstep(0.35, 1.0, across + 0.3 * (n - 0.5));

    // A ribbon turning edge-on fades out, so the crossed ribbons never show as hard lines.
    let to_cam = normalize(view.world_position.xyz - in.world_position.xyz);
    let facing = abs(dot(normalize(in.world_normal), to_cam));
    let face = smoothstep(0.05, 0.45, facing);

    // The retail streaks, in their own hue; the faction tint colours them. A thin hot thread down the middle near the ship.
    let tex = textureSample(base_color_texture, base_color_sampler, in.uv).rgb;
    let peak = max(tex.r, max(tex.g, tex.b));
    let hue = tex / max(peak, 0.05);
    let core = exp(-across * across * 80.0) * wake.params.z * pow(1.0 - age, 2.0);
    let colour = mix(wake.tint.rgb * hue, wake.hot.rgb, pow(1.0 - age, 3.0) * 0.6);
    // three overlapping ribbons: each carries a third of the light
    let glow = wake.tint.a * fade * face * (edge * ripples * peak * 0.45 + core * 0.35);

    var out: FragmentOutput;
    out.color = vec4<f32>(colour * glow, 1.0);
    return out;
}
