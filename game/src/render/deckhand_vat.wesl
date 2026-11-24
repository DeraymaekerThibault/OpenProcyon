// Not retail: the deck crews' baked moves (game/src/render/deckhands.rs, our own work under the project licence). A bone-transform
// texture holds every clip for every rig: row r is frame r; for a rig at column c and its bone b, texel c + 2b is the bone's rotation
// (quaternion xyzw) and texel c + 2b + 1 its translation (xyz), in the figure's own units. A figure's mesh is its rest pose; each vertex
// carries its bone's rotation texel (uv.x) and its colour slot (vertex colour r x 8), and is posed as rotate(q, rest) + t.
// Each figure's MeshTag says what it plays: bits 0-3 the clip, 4-8 the outfit, 9-15 a phase (share of the clip, 1/128), 16-31 when the
// clip started (battle seconds x 10, wrapping at 65536). The engine rewrites the tag only when the clip changes, so a crowd costs the
// CPU nothing a frame; the frame data's battle clock (`frame.clock.y`) stops while paused.
import openprocyon::render::frame::frame;

struct Deckhand {
    // per clip: x first row, y frames, z frames a second, w 1 when it loops (a one-shot holds its last frame)
    clips: array<vec4<f32>, 16>,
    // rgb per outfit and colour slot (outfit * 8 + slot: skin, coat, trousers, shoes, hat, trim, metal, hair)
    palette: array<vec4<f32>, 256>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> deckhand: Deckhand;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var vat: texture_2d<f32>;

struct Posed {
    position: vec3<f32>,
    normal: vec3<f32>,
    color: vec4<f32>,
}

// v turned by the unit quaternion q (xyzw)
fn rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    return v + 2.0 * cross(q.xyz, cross(q.xyz, v) + q.w * v);
}

// `texel`: the vertex's bone rotation texel (uv.x), `slot`: its colour slot (vertex colour r x 8), `position` and `normal`: its rest pose,
// `tag`: the figure's MeshTag.
fn pose(texel: f32, slot: f32, position: vec3<f32>, normal: vec3<f32>, tag: u32) -> Posed {
    let clip = deckhand.clips[tag & 15u];
    let outfit = (tag >> 4u) & 31u;
    let phase = f32((tag >> 9u) & 127u) / 128.0;
    let started = f32(tag >> 16u);
    let now = frame.clock.y * 10.0;
    let d = now - started;
    let elapsed = (d - 65536.0 * floor(d / 65536.0)) / 10.0;
    let frames = clip.y;
    var f = elapsed * clip.z + phase * frames;
    var a: f32;
    var b: f32;
    if clip.w > 0.5 {
        f = f - frames * floor(f / frames);
        a = floor(f);
        b = a + 1.0;
        if b >= frames {
            b = 0.0;
        }
    } else {
        f = min(elapsed * clip.z, frames - 1.0);
        a = floor(f);
        b = min(a + 1.0, frames - 1.0);
    }
    let t = f - a;
    let x = i32(texel);
    let ra = i32(clip.x + a);
    let rb = i32(clip.x + b);
    // blend the two frames' rotations (the shorter way round: flip by the dot product) and translations
    let qa = textureLoad(vat, vec2<i32>(x, ra), 0);
    var qb = textureLoad(vat, vec2<i32>(x, rb), 0);
    if dot(qa, qb) < 0.0 {
        qb = -qb;
    }
    let q = normalize(mix(qa, qb, t));
    let ta = textureLoad(vat, vec2<i32>(x + 1, ra), 0).xyz;
    let tb = textureLoad(vat, vec2<i32>(x + 1, rb), 0).xyz;
    var out: Posed;
    out.position = rotate(q, position) + mix(ta, tb, t);
    out.normal = rotate(q, normal);
    out.color = vec4<f32>(deckhand.palette[outfit * 8u + u32(slot + 0.5)].rgb, 1.0);
    return out;
}
