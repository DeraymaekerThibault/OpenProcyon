// Not retail: a nebula cloud or solar storm of the render profile (game/src/render/nebulae.rs; contract: `NebulaUniform` there,
// our own work under the project licence). Drawn additively (ONE/ONE: write unpremultiplied light, so it can only add light and
// never hides a ship) on the retail shell, whose vertex colours still carry the retail colour loop and the edge fade.
// Under the retail look (kept in part: `shell`) a short ray is marched through the cloud's ellipsoid: domain-warped 3-D fBm gas
// that churns slowly, tinted between a thin and a dense colour, the dense cores past 1 so they bloom. A storm's bolt lights the gas
// around it from inside for a moment (a point-light term in the march). With the camera outside, only the near side of the shell
// marches (from the cloud's front to its back); with the camera inside, the far wall marches from the camera, so the gas fills the
// view gently (`inside` caps it), and a short fine march streams wisps past the camera.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    mesh_view_bindings::view,
};

struct Nebula {
    // xyz: centre, w: radius
    centre: vec4<f32>,
    // xyz: half-size per axis (shares of the radius), w: steps
    axes: vec4<f32>,
    // x: density, y: noise scale, z: churn, w: share of the retail shell kept
    look: vec4<f32>,
    // rgb: thin gas, w: share drawn with the camera inside
    low: vec4<f32>,
    // rgb: dense gas, w: a bolt's brightness
    high: vec4<f32>,
    // xyz: the last bolt, w: what is left of its light
    flash: vec4<f32>,
    // x: clock, y: a bolt's reach (world units)
    params: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> nebula: Nebula;

fn hash13(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.1031);
    q += dot(q, q.zyx + 31.32);
    return fract((q.x + q.y) * q.z);
}

fn noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(mix(hash13(i), hash13(i + vec3<f32>(1.0, 0.0, 0.0)), u.x), mix(hash13(i + vec3<f32>(0.0, 1.0, 0.0)), hash13(i + vec3<f32>(1.0, 1.0, 0.0)), u.x), u.y);
    let b = mix(mix(hash13(i + vec3<f32>(0.0, 0.0, 1.0)), hash13(i + vec3<f32>(1.0, 0.0, 1.0)), u.x), mix(hash13(i + vec3<f32>(0.0, 1.0, 1.0)), hash13(i + vec3<f32>(1.0, 1.0, 1.0)), u.x), u.y);
    return mix(a, b, u.z);
}

// gas density at a point given in the cloud's unit space (the ellipsoid is |q| <= 1), 0..1
fn gas(q: vec3<f32>, t: f32) -> f32 {
    let p = q * nebula.look.y;
    // domain warping: one slow noise bends the lookup, so the gas churns in place rather than sliding
    let w = vec3<f32>(noise3(p * 0.8 + vec3<f32>(t * 0.9, 0.0, 3.1)), noise3(p * 0.8 + vec3<f32>(7.7, t * 1.1, 0.0)), noise3(p * 0.8 + vec3<f32>(0.0, 5.3, t * 0.8))) - vec3<f32>(0.5);
    let r = p + w * 1.8 + vec3<f32>(0.0, t * 0.5, 0.0);
    let n = 0.58 * noise3(r) + 0.3 * noise3(r * 2.1 + vec3<f32>(3.3, 1.7, 9.1)) + 0.12 * noise3(r * 4.3 + vec3<f32>(8.2, 4.4, 2.0));
    return smoothstep(0.42, 0.85, n);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // the retail shell: texture and vertex colour (the colour loop and the edge fade)
    let input = pbr_input_from_standard_material(in, is_front);
    let shell = input.material.base_color.rgb * nebula.look.w;
    let vcol = in.color.rgb;

    let eye = view.world_position;
    let c = nebula.centre.xyz;
    let radius = nebula.centre.w;
    let axes = nebula.axes.xyz * radius;
    let p = in.world_position.xyz;
    let dir = normalize(p - eye);

    // the ray against the ellipsoid, in a space where it is a unit sphere
    let o = (eye - c) / axes;
    let d = dir / axes;
    let a = dot(d, d);
    let b = dot(o, d);
    let disc = b * b - a * (dot(o, o) - 1.0);
    var light = vec3<f32>(0.0);
    let inside_cam = dot(o, o) < 1.0;
    // the near side of the shell when outside; the far wall when inside
    let rel = p - c;
    let facing = dot(rel, dir);
    var wanted = facing < 0.0;
    if inside_cam {
        wanted = facing > 0.0;
    }
    if disc > 0.0 && wanted {
        let s = sqrt(disc);
        let t0 = max((-b - s) / a, 0.0);
        let t1 = (-b + s) / a;
        let steps = i32(nebula.axes.w);
        let dt = (t1 - t0) / f32(steps);
        // a different start per pixel, so the steps do not band
        let jitter = fract(52.9829189 * fract(dot(in.position.xy, vec2<f32>(0.06711056, 0.00583715))));
        let clock = nebula.params.x * nebula.look.z;
        var acc = vec3<f32>(0.0);
        for (var i = 0; i < 24; i = i + 1) {
            if i >= steps {
                break;
            }
            let t = t0 + (f32(i) + jitter) * dt;
            let x = eye + dir * t;
            let q = (x - c) / axes;
            let edge = 1.0 - smoothstep(0.55, 1.0, length(q));
            if edge <= 0.0 {
                continue;
            }
            let dens = gas(q, clock) * edge;
            var col = mix(nebula.low.rgb, nebula.high.rgb, smoothstep(0.1, 0.9, dens));
            // a bolt lights the gas around it from inside
            let fd = length(x - nebula.flash.xyz) / max(nebula.params.y, 1.0);
            col += nebula.high.w * nebula.flash.w * exp(-fd * fd * 2.5) * vec3<f32>(0.7, 0.85, 1.0);
            acc += col * dens * dt / radius;
        }
        light = acc * nebula.look.x * vcol * 2.2;
        // soft shoulder: thin gas adds a little, only the dense cores climb past 1 to bloom
        light = light / (vec3<f32>(1.0) + 0.5 * light);
        if inside_cam {
            light = light * nebula.low.w;
            // wisps drifting past the camera: a short march through the first few dozen units, at finer, faster noise, so the
            // gas close by streams while the cloud beyond churns slowly
            let near_len = min(40.0, t1);
            var wisps = 0.0;
            for (var j = 0; j < 4; j = j + 1) {
                let x = eye + dir * ((f32(j) + jitter) * near_len * 0.25);
                wisps += gas((x - c) / axes * 7.0, clock * 4.0);
            }
            light += nebula.low.rgb * vcol * (wisps * 0.25) * near_len / 40.0 * 0.35 * nebula.look.x;
        }
    }
    var out: FragmentOutput;
    out.color = vec4<f32>(shell * select(1.0, 0.0, inside_cam) + light, 1.0);
    return out;
}
