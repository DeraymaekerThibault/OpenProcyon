// Not retail: a black hole of the render profile (game/src/render/holes.rs; contract: `HoleUniform` there, our own work under
// the project licence). Premultiplied-blended on a flat disc of the pull radius lying in the hole's plane, depth-tested (so a
// hull in the plane draws over it) and never written. The bent sky and the round shadow are the sky dome's (sky.wesl).
//  - Inside the horizon (the kill core by default): black. A thin bright ring marks its edge, never thinner than a pixel and a half.
//  - A hot accretion disc, a log-spiral of bright bands turning clockwise seen from above (the way the pull swirls), brighter on
//    the approaching side, white-gold inside and cooler outward, past 1 so it blooms.
//  - A warm halo and a faint outline at the pull radius (so the zone of danger stays readable), and a flash when a ship is swallowed.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
};

struct Hole {
    // xyz: centre (world), w: pull radius
    centre: vec4<f32>,
    // x: horizon radius, y: core radius, z: ring thickness (world units), w: lensing strength
    shape: vec4<f32>,
    // x: disc inner radius, y: outer radius (world units), z: spin (turns a second), w: brightness
    disc: vec4<f32>,
    color: vec4<f32>,
    // x: clock, y: swallow flash 0..1, z: glow, w: stars cube gain (0 = no stars cube)
    params: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> hole: Hole;
fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2(i), hash2(i + vec2<f32>(1.0, 0.0)), u.x), mix(hash2(i + vec2<f32>(0.0, 1.0)), hash2(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    let eye = view.world_position;
    let p = in.world_position.xyz;
    let c = hole.centre.xyz;
    let radius = hole.centre.w;
    let core = hole.shape.y;
    let flash = hole.params.y;
    let t = hole.params.x;

    // where the fragment lies in the hole's plane, from the centre
    let rel = p - c;
    let rho = length(vec2<f32>(rel.x, rel.z));
    let px = max(fwidth(rho), 1e-4);

    // the disc's own edge: everything fades out toward the pull radius
    let edge = 1.0 - smoothstep(0.8 * radius, radius, rho);
    var alpha = 0.0;
    var light = vec3<f32>(0.0);

    // the horizon: black, with a bright ring on its edge
    let horizon = hole.shape.x;
    let inside = 1.0 - smoothstep(horizon - px, horizon + px, rho);
    light = light * (1.0 - inside);
    alpha = max(alpha, inside);

    let ring_w = max(hole.shape.z, 1.5 * px);
    let ring = exp(-pow((rho - horizon) / ring_w, 2.0));
    let ring_light = vec3<f32>(1.0, 0.82, 0.55) * ring * (2.0 + 5.0 * flash) * hole.disc.w;

    // the accretion disc: a log-spiral of bands turning clockwise seen from above (screen right = +x, screen up = -z)
    let screen_angle = atan2(-rel.z, rel.x);
    var disc_light = vec3<f32>(0.0);
    var disc_a = 0.0;
    let d_in = hole.disc.x;
    let d_out = hole.disc.y;
    if rho > d_in * 0.8 && rho < d_out * 1.1 {
        let share = clamp((rho - d_in) / max(d_out - d_in, 1e-3), 0.0, 1.0);
        let profile = smoothstep(0.0, 0.12, share) * (1.0 - smoothstep(0.55, 1.0, share));
        let arms = 3.0;
        // bands: constant along a log spiral; the turning (clockwise: the pattern's angle decreases) is rigid
        let u = screen_angle * arms + 3.2 * log(max(rho, 1.0)) + hole.disc.z * t * 6.2831853 * arms;
        let band = 0.5 + 0.5 * sin(u);
        let grain = noise2(vec2<f32>(u * 1.7, rho / max(core, 1.0) * 6.0));
        let streaks = pow(band, 1.5) * (0.55 + 0.9 * grain);
        // the side moving toward the camera is brighter
        let tangent = normalize(vec3<f32>(-rel.z, 0.0, rel.x));
        let toward_cam = normalize(vec3<f32>(eye.x - p.x, 0.0, eye.z - p.z));
        let doppler = 1.0 + 0.6 * dot(tangent, toward_cam);
        let heat = 1.0 - smoothstep(0.0, 0.8, share);
        let hot = mix(hole.color.rgb, vec3<f32>(1.0, 0.92, 0.7), 0.85);
        let col = mix(hole.color.rgb, hot, heat);
        let inner_boost = 1.0 + 1.6 * heat;
        disc_a = profile * edge;
        disc_light = col * streaks * doppler * inner_boost * hole.disc.w * (1.0 + 2.0 * flash) * disc_a;
    }

    // a warm halo hugging the horizon, and the faint outline of the pull radius
    let halo = exp(-max(rho - horizon, 0.0) / (0.12 * radius)) * (1.0 - inside) * hole.params.z;
    let halo_light = hole.color.rgb * halo * 0.6;
    let outline = exp(-pow((rho - radius * 0.97) / max(1.5 * px, 0.004 * radius), 2.0));
    let outline_light = vec3<f32>(0.5, 0.6, 0.9) * outline * 0.25;

    let emission = (ring_light + disc_light + halo_light) * (1.0 - smoothstep(0.9 * radius, radius, rho)) + outline_light;
    var out: FragmentOutput;
    // light is added; alpha only blacks out the horizon (and a little under the ring), so the disc veils nothing but the core
    out.color = vec4<f32>(light + emission, clamp(max(alpha, ring * 0.5), 0.0, 1.0));
    return out;
}
