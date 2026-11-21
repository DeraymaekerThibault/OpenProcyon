// Not retail: the sky dome of the render profile (game/src/render/sky.rs; contract: `SkyUniform` there, our own work under the
// project licence). Premultiplied-blended on an inverted unit sphere around the camera, behind everything the depth test keeps in front.
// Near layers (up to 3): a sparse soft copy of the sky's own gas (the baked cube, bound at 101), sampled as if it floated
// `distance` units out. The camera's position moves the sample point, so each layer slides against the sky as the camera
// travels (parallax, two speeds for two distances); the map-wide current carries the gas along. Only the brighter gas is kept,
// softly, and a slowly churning 3-D noise (domain-warped by the clock) breaks the copy into wisps and patches.
// Base (optional): the far sky itself, drawn at alpha 1 from two samples that flow along a noise field and cross-fade (the
// flow-map trick), with a slow breath in the bright cores. The stars are never warped.
// Events light the gas, not the stars: a storm's flash and an explosion's warmth (CPU: direction and strength), sheet lightning
// and shooting stars (this clock only: a hash of the time slot picks direction and moment).
// Stars: a separate cube of point stars (16-bit linear light over the gain, bound at 103), each twinkling on a phase and rate
// of its own, the bright ones deeper; the brightest stay above 1 so they bloom. Added as light (the dome is drawn
// premultiplied: rgb is what is added, a how much of the picture behind is kept back).
// Cloud layers (`source: clouds`): the same layers, but the lookup reads a painted cloud cube (bound at 107) instead of the sky's gas:
// rgb is the lit cloud colour, alpha the coverage, each layer turned its own way. Haze sinks a far layer into the backdrop; their
// coverage hides the stars behind them.
// Black holes: every lookup (far sky, near gas, stars) goes along the view ray bent around each hole, toward its centre by
// lensing * core / b (b: the ray's closest approach), fading out by the pull radius, so the clouds and stars smear into an
// Einstein ring seamlessly; inside the core the sky is black (a round shadow at any camera angle). Drawn here, behind every
// ship and the hole's own disc (render/holes.rs), so the bending never covers anything in the battle.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
};
// the clock, the storm's last bolt and the black holes (game/src/render/frame.rs)
import openprocyon::render::frame::frame;

struct Layer {
    // x: opacity, y: threshold, z: softness, w: distance
    a: vec4<f32>,
    // x: noise scale, yzw: tint
    b: vec4<f32>,
    // xyz: the current, units a second
    c: vec4<f32>,
    // x: 1 for a painted cloud layer (the clouds cube), 0 for the sky's own gas, y: haze
    d: vec4<f32>,
}

struct Sky {
    // x: brightness (0 = none), y: twinkle depth, z: twinkles a second, w: gain
    stars: vec4<f32>,
    // x: layer count, y: 1 when the far sky is redrawn
    info: vec4<f32>,
    // x: flow (radians), y: flow cycle seconds, z: breath depth, w: breath seconds
    base: vec4<f32>,
    // xyz: the breath's colour bias, w: far backdrop brightness minus 1
    base_tint: vec4<f32>,
    // rgb: a storm flash's colour times its strength, w: seconds it lasts (the bolt is the frame's)
    flash: vec4<f32>,
    // x: distance from the camera at which a flash lights the gas at half strength (0: no falloff)
    flash_reach: vec4<f32>,
    warm: vec4<f32>,
    warm_dir: vec4<f32>,
    // x: shooting star slot seconds (0 = none), y: brightness
    shoot: vec4<f32>,
    // x: sheet lightning slot seconds (0 = none), y: strength
    sheet: vec4<f32>,
    sheet_color: vec4<f32>,
    layers: array<Layer, 3>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> sky: Sky;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var sky_cube: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var sky_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var star_cube: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var star_sampler: sampler;
// the baked tiling noise (game/src/render/volnoise.rs): r the gas (three octaves), g the warp, b a third field; CELLS cells a tile
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var noise_tex: texture_3d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var noise_samp: sampler;
// the painted clouds (`<sky>_clouds.png`, straight-alpha rgba, laid out like the sky cube); bound but unread when no layer asks for it
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var cloud_cube: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var cloud_sampler: sampler;
const CELLS: f32 = 8.0;

fn hash3(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3<f32>(127.1, 311.7, 74.7))) * 43758.5453);
}

fn hash1(n: f32) -> f32 {
    return fract(sin(n * 12.9898 + 4.1414) * 43758.5453);
}

// the baked noise at `p` (in cells): one read where value noise would cost eight hashes an octave
fn bake(p: vec3<f32>) -> vec4<f32> {
    return textureSampleLevel(noise_tex, noise_samp, p / CELLS, 0.0);
}

// the cube is left-handed: the baker lays the sky out for the ray with z negated, as Bevy's own skybox samples
fn sample_sky(d: vec3<f32>) -> vec3<f32> {
    return textureSampleLevel(sky_cube, sky_sampler, d * vec3<f32>(1.0, 1.0, -1.0), 0.0).rgb;
}

// the light the events put on the gas: a soft lobe around each direction, in its colour
fn event_light(dir: vec3<f32>) -> vec3<f32> {
    var l = vec3<f32>(0.0);
    // the storm's last bolt: a quadratic fade over the flash's seconds, weaker the farther the storm
    let left = max(1.0 - frame.bolt_at.w / max(sky.flash.w, 0.01), 0.0);
    if left > 0.0 {
        let to_bolt = frame.bolt_at.xyz - view.world_position;
        let distance = max(length(to_bolt), 0.001);
        let reach = sky.flash_reach.x;
        var local = 1.0;
        if reach > 0.0 {
            local = 1.0 - smoothstep(0.0, 2.0 * reach, distance);
        }
        l += sky.flash.rgb * (left * left * local) * pow(max(dot(dir, to_bolt / distance), 0.0), 8.0);
    }
    l += sky.warm.rgb * pow(max(dot(dir, sky.warm_dir.xyz), 0.0), 2.0);
    if sky.sheet.x > 0.0 {
        let slot = floor(frame.clock.x / sky.sheet.x);
        let at = (frame.clock.x - slot * sky.sheet.x) - hash1(slot) * sky.sheet.x * 0.9;
        if at > 0.0 {
            let d = normalize(vec3<f32>(hash1(slot + 1.0), hash1(slot + 2.0), hash1(slot + 3.0)) - vec3<f32>(0.5));
            // two quick flickers, then out
            let env = exp(-at * 5.0) * (0.6 + 0.4 * sin(at * 38.0));
            l += sky.sheet_color.rgb * sky.sheet.y * env * pow(max(dot(dir, d), 0.0), 2.5);
        }
    }
    return l;
}

// a shooting star: a bright head with a short tail along a great circle, once in a slot of `shoot.x` seconds
fn shooting_star(dir: vec3<f32>) -> f32 {
    if sky.shoot.x <= 0.0 {
        return 0.0;
    }
    let slot = floor(frame.clock.x / sky.shoot.x);
    let life = 1.4;
    let t = (frame.clock.x - slot * sky.shoot.x) - hash1(slot + 11.0) * max(sky.shoot.x - life, 1.0);
    if t < 0.0 || t > life {
        return 0.0;
    }
    let a = normalize(vec3<f32>(hash1(slot + 21.0), hash1(slot + 22.0), hash1(slot + 23.0)) - vec3<f32>(0.5));
    let r = normalize(vec3<f32>(hash1(slot + 31.0), hash1(slot + 32.0), hash1(slot + 33.0)) - vec3<f32>(0.5));
    let b = normalize(cross(a, r));
    let n = cross(a, b);
    let perp = dot(dir, n);
    let s = atan2(dot(dir, b), dot(dir, a));
    let head = t * 0.28;
    let u = head - s;
    if dot(dir, a) < 0.0 || u < 0.0 || u > 0.14 {
        return 0.0;
    }
    let line = exp(-(perp * perp) / (0.0012 * 0.0012));
    return sky.shoot.y * line * (1.0 - u / 0.14) * sin(3.14159 * t / life);
}

// each cloud layer looks the painting up through a turn of its own, so three layers of one painting are not copies
fn cloud_turn(i: i32, v: vec3<f32>) -> vec3<f32> {
    if i == 0 {
        return v;
    }
    if i == 1 {
        return v.yzx * vec3<f32>(-1.0, 1.0, 1.0);
    }
    return v.zxy * vec3<f32>(1.0, 1.0, -1.0);
}

// 0 with the camera inside a hole's shadow sphere or right beside it, 1 from two and a half core radii out
fn camera_clear(distance: f32, core: f32) -> f32 {
    return smoothstep(1.2 * core, 2.5 * core, distance);
}

// the view ray bent around the black holes (xyz), how much it bent (w, radians)
fn lensed(d: vec3<f32>) -> vec4<f32> {
    var dir = d;
    var total = 0.0;
    let eye = view.world_position;
    let count = i32(frame.holes_info.x);
    for (var i = 0; i < 4; i = i + 1) {
        if i >= count {
            break;
        }
        let h = frame.holes[i];
        let to_c = h.xyz - eye;
        let along = dot(to_c, dir);
        if along <= 0.0 {
            continue;
        }
        let perp = to_c - dir * along;
        let b = max(length(perp), 1e-3);
        let reach = frame.hole_reach[i].x;
        // a camera at the hole (a cutscene flying in, a ship at the core's edge) sees no bending: it would fill the screen
        let fade = (1.0 - smoothstep(0.35 * reach, reach, b)) * camera_clear(length(to_c), h.w);
        let ratio = h.w / b;
        // falls off with the square of the distance: a ring at the horizon, a gentle bend across the rest of the pull circle
        let bend = min(frame.holes_info.y * ratio * ratio, 1.4) * fade;
        dir = normalize(dir * cos(bend) + (perp / b) * sin(bend));
        total += bend;
    }
    return vec4<f32>(dir, total);
}

// how deep the view ray is in a black hole's shadow (0 outside, 1 inside): round at any camera angle, a little past the core
// (the photon sphere), on the unbent ray
fn hole_shadow(d: vec3<f32>) -> f32 {
    var s = 0.0;
    let count = i32(frame.holes_info.x);
    for (var i = 0; i < 4; i = i + 1) {
        if i >= count {
            break;
        }
        let h = frame.holes[i];
        let to_c = h.xyz - view.world_position;
        let along = dot(to_c, d);
        if along > 0.0 {
            let b = length(to_c - d * along);
            s = max(s, (1.0 - smoothstep(h.w * 0.9, h.w * 1.05, b)) * camera_clear(length(to_c), h.w));
        }
    }
    return s;
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    let straight = normalize(in.world_position.xyz - view.world_position);
    let bent = lensed(straight);
    let dir = bent.xyz;
    // 1 in a hole's shadow, fading at its edge
    let shadow = hole_shadow(straight);
    let clock = frame.clock.x;
    let lit = vec3<f32>(1.0) + event_light(dir);
    var rgb = vec3<f32>(0.0);
    var alpha = 0.0;

    if sky.info.y > 0.5 {
        // two lookups pushed along a slow flow in opposite phases, cross-faded so the gas shimmers without ever jumping
        let t = clock / sky.base.y;
        let p0 = fract(t);
        let p1 = fract(t + 0.5);
        let w = 1.0 - abs(2.0 * p0 - 1.0);
        let flow = bake(dir * 3.0 + vec3<f32>(0.0, 0.0, 1.7)).rgb - vec3<f32>(0.5);
        let c0 = sample_sky(normalize(dir + flow * sky.base.x * p0 * 2.0));
        let c1 = sample_sky(normalize(dir + flow * sky.base.x * p1 * 2.0));
        var col = mix(c1, c0, w);
        // the bright cores breathe
        let core = smoothstep(0.35, 1.2, dot(col, vec3<f32>(0.3333)));
        let breath = sin(clock * 6.2831853 / sky.base.w);
        col = col * (vec3<f32>(1.0) + sky.base.z * breath * core * sky.base_tint.xyz);
        rgb = col * lit * (1.0 + sky.base_tint.w);
        alpha = 1.0;
    } else if bent.w > 0.0 {
        // no redrawn far sky: the bent part of it is drawn here, fading in with the bend, so nothing shows a seam
        let a = smoothstep(0.0, 0.03, bent.w);
        rgb = sample_sky(dir) * lit * a;
        alpha = a;
    }

    // how much of the sky the painted clouds cover, so the stars behind them are hidden
    var cover = 0.0;
    let count = i32(sky.info.x);
    for (var i = 0; i < 3; i = i + 1) {
        if i >= count {
            break;
        }
        let l = sky.layers[i];
        if l.a.x <= 0.0 {
            continue;
        }
        // a point on the layer, moved by the camera and carried by the current
        let p = dir * l.a.w + view.world_position - l.c.xyz * frame.clock.y;
        if l.d.x > 0.5 {
            // painted clouds: one read of the clouds cube; alpha is the coverage, rgb the lit colour (straight alpha)
            let q = cloud_turn(i, normalize(p));
            let tex = textureSampleLevel(cloud_cube, cloud_sampler, q * vec3<f32>(1.0, 1.0, -1.0), 0.0);
            var cov = tex.a;
            if l.a.y > 0.0 || l.a.z > 0.0 {
                cov = smoothstep(l.a.y, l.a.y + max(l.a.z, 0.01), cov);
            }
            if l.b.x > 0.0 {
                // churning noise eats the edges: the dense cores stay, thin parts break into wisps
                let n = q * l.b.x + vec3<f32>(f32(i) * 7.3);
                let warp = bake(n * 0.7 + vec3<f32>(clock * 0.011, clock * 0.013, clock * 0.009)).rgb - vec3<f32>(0.5);
                let f = bake(n + warp * 1.6 + vec3<f32>(0.0, 0.0, clock * 0.006)).r;
                cov = cov * mix(smoothstep(0.32, 0.68, f), 1.0, cov * cov);
            }
            var col = tex.rgb * l.b.yzw;
            // aerial perspective: flatter, and toward the backdrop behind it
            let haze = l.d.y;
            col = mix(col, vec3<f32>(dot(col, vec3<f32>(0.3333))), haze * 0.5);
            col = mix(col, sample_sky(dir), haze);
            let a = cov * l.a.x;
            cover = 1.0 - (1.0 - cover) * (1.0 - a);
            rgb = rgb * (1.0 - a) + col * lit * a;
            alpha = alpha * (1.0 - a) + a;
            continue;
        }
        // turned and mirrored, so the layer is not the sky's own picture laid over itself
        let q = normalize(p).zxy * vec3<f32>(1.0, -1.0, 1.0);
        let gas = textureSampleLevel(sky_cube, sky_sampler, q, 0.0).rgb * l.b.yzw;
        let brightness = dot(gas, vec3<f32>(0.3333));
        var keep = smoothstep(l.a.y, l.a.y + l.a.z, brightness);
        if l.b.x > 0.0 {
            // wisps: noise on the lookup point, its domain warped slowly so the patches churn in place
            let n = q * l.b.x + vec3<f32>(f32(i) * 7.3);
            let warp = bake(n * 0.7 + vec3<f32>(clock * 0.011, clock * 0.013, clock * 0.009)).rgb - vec3<f32>(0.5);
            let f = bake(n + warp * 1.6 + vec3<f32>(0.0, 0.0, clock * 0.006)).r;
            keep = keep * smoothstep(0.32, 0.68, f);
        }
        let a = keep * l.a.x;
        // premultiplied: this layer over what is below it
        rgb = rgb * (1.0 - a) + gas * lit * a;
        alpha = alpha * (1.0 - a) + a;
    }

    var stars = vec3<f32>(0.0);
    if sky.stars.x > 0.0 {
        let sd = dir * vec3<f32>(1.0, 1.0, -1.0);
        let light = textureSampleLevel(star_cube, star_sampler, sd, 0.0).rgb * sky.stars.w;
        let strength = max(max(light.r, light.g), light.b);
        // one phase and rate for every star in a small patch of sky; faint stars hardly flicker
        let cell = floor(sd * 400.0);
        let phase = hash3(cell) * 6.2831853;
        let rate = sky.stars.z * (0.6 + 0.8 * hash3(cell + vec3<f32>(7.0)));
        let wave = 0.5 + 0.5 * sin(clock * 6.2831853 * rate + phase);
        let depth = sky.stars.y * smoothstep(0.03, 0.6, strength);
        stars = light * (1.0 - depth * wave) * sky.stars.x * (1.0 - cover);
    }
    stars += vec3<f32>(0.85, 0.92, 1.0) * shooting_star(dir);

    // the shadow: black, and nothing of the sky behind it
    let keep = 1.0 - shadow;
    var out: FragmentOutput;
    out.color = vec4<f32>((rgb + stars) * keep, alpha * keep + shadow);
    return out;
}
