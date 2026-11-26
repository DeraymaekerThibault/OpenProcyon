// Not retail: a black hole of the render profile drawn as a volume (game/src/render/holes.rs, `volume`; contract: `HoleUniform` there,
// our own work under the project licence). Like the storm clouds (volume.wesl): the box round the hole is drawn on its back faces
// only, and this shader ray-marches the gas inside it with emission and absorption (Beer-Lambert), composited premultiplied over the
// scene, ending at the scene's depth. The gas is a thin accretion disc with real thickness, pinched in the middle and flaring toward
// the rim, so from the side it is a body, not a plate. It does not turn as one texture: it is sampled turned back by how far its
// radius has gone round, faster near the core (Keplerian), in two phases blended so it never winds up, as the storms are. The gas is
// the baked noise looked up along log spirals (two arms, drawn out into fibres), carries thin hot veins, and is hottest at the core:
// dark crimson at the rim, orange, amber, pale cream. In the middle is a solid black ball the size of the kill core (the march ends
// on it, so it hides the gas behind it and is seen from every side) with a thin ring of light round its edge.
import openprocyon::render::frame::frame;
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
};
import bevy_pbr::prepass::utils as prepass_utils;

struct Hole {
    // xyz: centre (world), w: pull radius
    centre: vec4<f32>,
    // x: horizon (lip) radius, y: core radius, z: ring thickness (world units)
    shape: vec4<f32>,
    // x: disc inner radius, y: outer radius, z: spin (turns a second), w: brightness
    disc: vec4<f32>,
    // rgb: the hot colour
    color: vec4<f32>,
    // x: swallow flash 0..1, y: glow
    params: vec4<f32>,
    // x: the box's half height (world units), y: ray steps, z: the gas's half thickness at the rim
    funnel: vec4<f32>,
    // rgb: the outer gas
    rim: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> hole: Hole;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var noise_tex: texture_3d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var noise_samp: sampler;
const CELLS: f32 = 8.0;
const TAU: f32 = 6.2831853;

fn bake(p: vec3<f32>) -> vec4<f32> {
    return textureSampleLevel(noise_tex, noise_samp, p / CELLS, 0.0);
}

// the noise along a log spiral: the angle `a` goes round a circle of `k` cells (no seam), `along` runs along the third axis
fn spiral(a: f32, k: f32, along: f32, at: vec2<f32>) -> vec4<f32> {
    return bake(vec3<f32>(cos(a) * k + at.x, sin(a) * k + at.y, along));
}

// the gas at a point turned back by `turn` about the hole's axis: x density, y the hot veins, z how hot
fn gas(rel: vec3<f32>, rho: f32, half_h: f32, turn: f32, t: f32) -> vec3<f32> {
    let radius = hole.centre.w;
    let core = hole.shape.y;
    let c = cos(turn);
    let s = sin(turn);
    let r = vec2<f32>(c * rel.x - s * rel.z, s * rel.x + c * rel.z);
    // clockwise seen from above, the arms trailing the way the gas turns
    let theta = atan2(-r.y, r.x);
    let lr = log(max(rho, 1.0) / radius);
    // height in half-thicknesses
    let h = rel.y / half_h;
    let vo = h * 0.9;
    let a = spiral(2.0 * (theta - 2.0 * lr), 0.9, lr * 0.7 + 1.3 + vo, vec2<f32>(4.0));
    let wound = theta - 1.9 * lr + (a.g - 0.5) * 1.4;
    let arm = pow(0.5 + 0.5 * cos(2.0 * wound), 1.2);
    let near = 1.0 - smoothstep(1.0 * core, 2.0 * core, rho);
    let arms = mix(0.04 + 0.96 * arm, 1.0, near);
    let f1 = spiral(2.0 * wound, 6.0, lr * 0.5 + t * 0.03 + 2.0 + vo, vec2<f32>(4.0));
    let f2 = spiral(2.0 * (wound + (f1.r - 0.5) * 0.6), 12.0, lr * 0.4 + t * 0.05 + 7.0 + vo * 1.7, vec2<f32>(2.0)).r;
    let strands = smoothstep(0.3, 0.75, 0.6 * f1.r + 0.4 * f2);
    let body = arms * (0.25 + 0.95 * strands) * (0.55 + 0.9 * a.r);
    let dens = smoothstep(0.32, 0.62, body + 0.12 * near);
    let vein = pow(1.0 - smoothstep(0.0, 0.06, abs(f2 - 0.5)), 3.0) * arms;
    return vec3<f32>(dens, vein, strands);
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    var out: FragmentOutput;
    out.color = vec4<f32>(0.0);
    let eye = view.world_position;
    let c = hole.centre.xyz;
    let radius = hole.centre.w;
    let core = hole.shape.y;
    let half_h = hole.funnel.x;
    let flash = hole.params.x;
    let bright = hole.disc.w;
    let dir = normalize(in.world_position.xyz - eye);
    let oc = eye - c;

    // the ray against the box round the hole
    let ext = vec3<f32>(radius, half_h, radius);
    let inv = 1.0 / dir;
    let ta = (-ext - oc) * inv;
    let tb = (ext - oc) * inv;
    let tmin = min(ta, tb);
    let tmax = max(ta, tb);
    let t0 = max(max(max(tmin.x, tmin.y), tmin.z), 0.0);
    var t1 = min(min(tmax.x, tmax.y), tmax.z);

    // the solid ball: the ray ends on it
    let rs = core * 0.97;
    let bb = dot(oc, dir);
    let sd = bb * bb - (dot(oc, oc) - rs * rs);
    var ball = false;
    if sd > 0.0 {
        let ts = -bb - sqrt(sd);
        if ts > 0.0 && ts < t1 {
            t1 = ts;
            ball = true;
        }
    }

    // the scene: the march ends at the nearest hull or island (the depth prepass)
    @if(DEPTH_PREPASS)
    {
        let sdp = prepass_utils::prepass_depth(in.position, 0u);
        if sdp > 0.0 {
            let forward = -view.world_from_view[2].xyz;
            let t_scene = (view.clip_from_view[3][2] / sdp) / max(dot(dir, forward), 0.05);
            if t_scene < t1 {
                t1 = t_scene;
                ball = false;
            }
        }
    }

    // the ring of light round the ball's edge: the ray's closest approach to the centre, in front of the camera only
    let closest = length(oc - dir * bb);
    var ring_light = vec3<f32>(0.0);
    if bb < 0.0 {
        let w = max(0.02 * rs, 1.5 * fwidth(closest));
        let r = exp(-pow((closest - rs * 1.04) / w, 2.0));
        let halo = exp(-max(closest - rs, 0.0) / (0.25 * rs)) * smoothstep(rs * 0.9, rs * 1.0, closest) * 0.25;
        ring_light = vec3<f32>(1.0, 0.8, 0.5) * (r * (0.9 + 5.0 * flash) + halo) * bright;
    }

    var light = vec3<f32>(0.0);
    var tr = 1.0;
    if t1 > t0 {
        let len = t1 - t0;
        let steps = clamp(hole.funnel.y, 8.0, 96.0);
        let dt = len / steps;
        let jitter = fract(52.9829189 * fract(dot(in.position.xy, vec2<f32>(0.06711056, 0.00583715))));
        let t = frame.clock.y;
        // two phases of the turn, blended, so the gas never winds up
        let period = 8.0;
        let pa = fract(t / period);
        let pb = fract(t / period + 0.5);
        let wa = 1.0 - abs(2.0 * pa - 1.0);
        let sigma = 9.0 / radius;
        for (var i = 0; i < 96; i = i + 1) {
            if f32(i) >= steps {
                break;
            }
            let x = eye + dir * (t0 + (f32(i) + jitter) * dt);
            let rel = x - c;
            let rho = length(vec2<f32>(rel.x, rel.z));
            if rho > radius || rho < rs * 0.9 {
                continue;
            }
            let share = rho / radius;
            // thin at the core, flaring toward the rim; gas thins away above and below
            let hh = hole.funnel.z * (0.2 + 0.8 * share);
            let vy = rel.y / hh;
            let vertical = exp(-2.0 * vy * vy);
            if vertical < 0.02 {
                continue;
            }
            let edge = (1.0 - smoothstep(0.35, 0.95, share)) * smoothstep(rs * 0.9, rs * 1.25, rho);
            // faster near the core
            let rate = hole.disc.z * TAU * period * pow(max(hole.shape.x / rho, 0.2), 1.5);
            let ga = gas(rel, rho, hh, -(pa - 0.5) * rate, t);
            let gb = gas(rel, rho, hh, -(pb - 0.5) * rate, t);
            let g = mix(gb, ga, wa);
            let dens = g.x * vertical * edge;
            if dens < 0.01 {
                continue;
            }
            let hot = pow(1.0 - smoothstep(rs, 0.75 * radius, rho), 2.2);
            let eyec = 1.0 - smoothstep(1.05 * hole.shape.x, 1.9 * hole.shape.x, rho);
            let lum = clamp(dens * (0.45 + 0.4 * hot) + 0.4 * eyec * eyec, 0.0, 1.0);
            var col = mix(hole.rim.rgb, vec3<f32>(0.7, 0.14, 0.03), smoothstep(0.05, 0.3, lum));
            col = mix(col, hole.color.rgb, smoothstep(0.25, 0.6, lum));
            col = mix(col, vec3<f32>(1.0, 0.42, 0.08), smoothstep(0.55, 0.9, lum));
            col = mix(col, vec3<f32>(1.0, 0.78, 0.45), eyec * smoothstep(0.5, 1.0, lum));
            // the gas toward the plane burns hotter than its fringes above and below
            let glow = (0.02 + 1.3 * pow(g.x, 1.5) * (0.4 + 0.6 * hot) + 0.6 * eyec) * bright * (0.6 + 0.4 * vertical);
            var emitted = col * glow;
            emitted += vec3<f32>(1.0, 0.6, 0.25) * g.y * vertical * (0.5 + hot) * bright * 0.6;
            emitted *= 1.0 + 2.0 * flash;
            let step_tr = exp(-dens * sigma * dt);
            light += emitted * tr * (1.0 - step_tr);
            tr *= step_tr;
            if tr < 0.02 {
                break;
            }
        }
    }
    // the ball is opaque black: what is behind it, gas included, is hidden
    let alpha = select(1.0 - tr, 1.0, ball);
    // the ring is drawn over what is in front of the ball's edge, not over the gas that is in front of it
    let ring = ring_light * select(tr, 1.0, ball);
    out.color = vec4<f32>(light + ring, alpha);
    return out;
}
