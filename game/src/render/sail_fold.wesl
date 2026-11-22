// Not retail: how a sail of the render profile bellies out and folds, on the GPU (game/src/render/sails.rs, our own work under the
// project licence). The engine ships this stage, since it moves the mesh the engine cut (`subdivide`); the mod's `shaders/sail.wesl`
// only draws the cloth. The math repeats `belly`, `furled` and `fanned` in sails.rs, which stay the tested reference: keep them alike.
//
// Per vertex (mesh-local space, rest shape = the mesh's own position and normal) the sail's storage buffer holds four rows after
// a header and a table of its fans:
//   row 0 (header): x fans in the table
//   per fan, 3 rows: pivot (xyz), the fan's plane normal (xyz), where its pivot goes when it is shut (xyz)
//   per vertex, 5 rows: (pin, top, span, rim), (gradient of pin xyz, 1 for a fan point), (gradient of span xyz, fan index), the
//   quaternion that swings the point about its pivot onto the line the fan closes onto (`FanPoint::shut`), the direction the cloth
//   bellies out along (xyz: the retail normal; here because the depth and shadow passes get no normals)
// The mesh's own normals are the cut cloth's geometric ones at rest (so each fan wedge shades as a facet); the bulge and the fold
// tilt and turn them.
// The normals follow the bulge and the fold analytically (gradients baked once), so nothing is rebuilt on the CPU.

struct SailGlow {
    color: vec4<f32>,
    params: vec4<f32>,
    sun: vec4<f32>,
    // y: phase, w: the sail's row in `sail_state`
    motion: vec4<f32>,
    pattern: vec4<f32>,
    // z: pleats, w: pleat depth (mesh units)
    fold: vec4<f32>,
    // x: the sail's size (mesh units), y: 1 when the buffer holds fold data
    shape: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> sail: SailGlow;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<storage, read> fold_data: array<vec4<f32>>;
// What moves, a row for every sail (`STATE` in sails.rs): x the ship's smoothed share of full speed, y the sail's clock, z how far
// the cloth bellies (mesh units, already eased out while folding), w how far it is folded (0 open .. `furl`)
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var<storage, read> sail_state: array<vec4<f32>>;

struct Folded {
    position: vec3<f32>,
    normal: vec3<f32>,
}

// `Quat::IDENTITY.slerp(q, a)` for a turn `q` of at most half a turn (w >= 0, as `Quat::from_rotation_arc` gives)
fn turn_part(q: vec4<f32>, a: f32) -> vec4<f32> {
    let half = acos(clamp(q.w, -1.0, 1.0));
    let s = sin(half);
    if s < 1.0e-5 {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let h = half * a;
    return vec4<f32>(q.xyz / s * sin(h), cos(h));
}

fn rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

// `n` tilted by the gradient `g` of a displacement along it (only the part of `g` across the surface tilts it)
fn tilted(n: vec3<f32>, g: vec3<f32>) -> vec3<f32> {
    return normalize(n - (g - n * dot(n, g)));
}

/// Rows a vertex takes in the buffer.
const ROWS: u32 = 5u;

// The rest point `p` with its shading normal `rest_normal` (any where the pass draws no normals), vertex `local` of its mesh,
// bellied out and folded as the sail is now.
fn fold(p: vec3<f32>, rest_normal: vec3<f32>, local: u32) -> Folded {
    var out: Folded;
    out.position = p;
    out.normal = rest_normal;
    if sail.shape.y < 0.5 {
        return out;
    }
    let fans = u32(fold_data[0].x);
    let at = 1u + 3u * fans + ROWS * local;
    let n = fold_data[at + 4u].xyz;
    let r0 = fold_data[at];
    let r1 = fold_data[at + 1u];
    let r2 = fold_data[at + 2u];
    let pin = r0.x;
    let top = r0.y;
    let state = sail_state[u32(sail.motion.w)];
    let amp = state.z;
    let a = state.w;

    // sails.rs `belly`: a slow breath plus ripples running across, slacker with little wind
    let size = max(sail.shape.x, 1.0e-3);
    let wind = state.x;
    let phase = sail.motion.y;
    let clock = state.y;
    let k = 6.0 / size;
    let arg = 3.3 * clock - k * (p.y + 0.6 * p.x) + phase;
    let w = 0.3 + 0.7 * wind;
    let belly = (0.65 + 0.25 * sin(1.1 * clock + phase) + 0.12 * sin(arg)) * w;
    let d_belly = w * 0.12 * cos(arg) * -k * vec3<f32>(0.6, 1.0, 0.0);
    let billow = n * (amp * pin * belly);
    let bent = tilted(rest_normal, amp * (belly * r1.xyz + pin * d_belly));

    if r1.w > 0.5 {
        // sails.rs `fanned`: the point swings about its fan's pivot onto the closing line, the pivot slides onto the mast and
        // the cloth between the ribs zig-zags out of the fan's plane
        let f = 1u + 3u * u32(r2.w);
        let pivot = fold_data[f].xyz;
        let plane = fold_data[f + 1u].xyz;
        let to_mast = fold_data[f + 2u].xyz;
        let span = r0.z;
        let rim = r0.w;
        let d = p - pivot;
        let q = turn_part(fold_data[at + 3u], a);
        let pleats = sail.fold.z;
        let depth = sail.fold.w;
        let wave = fract(span * pleats);
        let tri = 4.0 * abs(wave - 0.5) - 1.0;
        out.position = pivot + to_mast * a + rotate(q, d) + plane * (depth * a * rim * tri) + billow;
        let d_tri = 4.0 * sign(wave - 0.5) * pleats;
        // rim = |d| / the fan's longest rib, so its gradient is d * rim / |d|^2 (none at the pivot itself, where d is 0)
        let d_rim = d * (rim / max(dot(d, d), 1.0e-8));
        let pleat = depth * a * (rim * d_tri * r2.xyz + tri * d_rim);
        out.normal = rotate(q, tilted(bent, pleat));
    } else {
        // sails.rs `furled`: rolled up toward the top edge (the yard), the height under it squeezed by the fold
        let b = p + billow;
        out.position = vec3<f32>(b.x, top - (top - b.y) * (1.0 - a), b.z);
        out.normal = normalize(vec3<f32>(bent.x, bent.y / max(1.0 - a, 0.02), bent.z));
    }
    return out;
}
