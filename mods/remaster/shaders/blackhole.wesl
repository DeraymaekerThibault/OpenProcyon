// Not retail: a black hole of the render profile (game/src/render/holes.rs; contract: `HoleUniform` there, our own work under
// the project licence). Premultiplied-blended on a flat disc of the pull radius lying in the hole's plane, depth-tested (so a
// hull in the plane draws over it) and never written. The bent sky and the round shadow are the sky dome's (sky.wesl).
//  - Inside the horizon (the kill core by default): black. A thin, sharp photon ring marks its edge, never thinner than a pixel and a half.
//  - A thin, crisp accretion disc just outside it: a hot white-gold inner edge cooling to orange and red, fine bright arms on a log
//    spiral turning clockwise seen from above (the way the pull swirls), brighter on the side that moves toward the camera, past 1 so
//    it blooms. No broad soft band: the light falls off fast with radius and the outer edge is cut.
//  - A tight warm halo on the horizon, a faint outline at the pull radius (so the zone of danger stays readable), and a flash when
//    a ship is swallowed.
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

    // the horizon: black
    let horizon = hole.shape.x;
    let inside = 1.0 - smoothstep(horizon - px, horizon + px, rho);

    // the photon ring: thin and sharp on the horizon's edge
    let ring_w = max(hole.shape.z, 1.5 * px);
    let ring = exp(-pow((rho - horizon) / ring_w, 2.0));
    let ring_light = vec3<f32>(1.0, 0.86, 0.6) * ring * (1.2 + 5.0 * flash) * hole.disc.w;

    // the accretion disc: a log-spiral of fine bright arms turning clockwise seen from above (screen right = +x, screen up = -z)
    let screen_angle = atan2(-rel.z, rel.x);
    var disc_light = vec3<f32>(0.0);
    let d_in = hole.disc.x;
    let d_out = hole.disc.y;
    if rho > d_in - 2.0 * px && rho < d_out {
        let share = clamp((rho - d_in) / max(d_out - d_in, 1e-3), 0.0, 1.0);
        // a hard inner edge and a cut outer one: thin and crisp, not a blur
        let edge = smoothstep(0.0, 1.5 * px, rho - d_in) * (1.0 - smoothstep(0.82, 1.0, share));
        // the light falls off fast outward
        let fall = pow(1.0 - share, 2.4);
        // arms: constant along a log spiral, sharpened; the turning (clockwise: the pattern's angle decreases) is rigid
        // tightly wound (a pitch of about 12 degrees), so the arms read as streaks wrapping round the hole, not spokes
        let arms = 9.0;
        let winding = arms / 0.21;
        let turn = hole.disc.z * t * 6.2831853;
        let u = screen_angle * arms + winding * log(max(rho, 1.0)) + turn * arms;
        let band = pow(0.5 + 0.5 * sin(u), 10.0);
        // fine grain along the arms, periodic in the angle (no seam where atan2 wraps), turning with them
        let turned = screen_angle + turn;
        let grain = noise2(vec2<f32>(cos(turned), sin(turned)) * 9.0 + vec2<f32>(rho / max(core, 1.0) * 7.0));
        let fine = 0.5 + 0.5 * sin(u * 2.3 + grain * 4.0);
        let streaks = mix(0.06, 1.0, band) * (0.55 + 0.9 * grain) * (0.7 + 0.5 * fine);
        // the side moving toward the camera is brighter
        let tangent = normalize(vec3<f32>(-rel.z, 0.0, rel.x));
        let toward_cam = normalize(vec3<f32>(eye.x - p.x, 0.0, eye.z - p.z));
        let doppler = 1.0 + 0.8 * dot(tangent, toward_cam);
        // white-gold at the inner edge, orange, then red
        let gold = vec3<f32>(1.0, 0.9, 0.6);
        let col = mix(mix(gold, hole.color.rgb, smoothstep(0.0, 0.3, share)), hole.color.rgb * vec3<f32>(1.0, 0.35, 0.15), smoothstep(0.3, 1.0, share));
        disc_light = col * streaks * doppler * (0.15 + 3.2 * fall) * hole.disc.w * (1.0 + 2.0 * flash) * edge;
    }

    // a tight warm halo hugging the horizon, and the faint outline of the pull radius
    let halo = exp(-max(rho - horizon, 0.0) / (0.05 * radius)) * (1.0 - inside) * hole.params.z;
    let halo_light = hole.color.rgb * halo * 0.6;
    let outline = exp(-pow((rho - radius * 0.97) / max(1.5 * px, 0.004 * radius), 2.0));
    let outline_light = vec3<f32>(0.5, 0.6, 0.9) * outline * 0.25;

    let emission = (ring_light + disc_light + halo_light) * (1.0 - smoothstep(0.9 * radius, radius, rho)) + outline_light;
    var out: FragmentOutput;
    // light is added; alpha only blacks out the horizon (and a little under the ring), so the disc veils nothing but the core
    out.color = vec4<f32>(emission, clamp(max(inside, ring * 0.5), 0.0, 1.0));
    return out;
}
