// Not retail: the deck crews' baked moves (game/src/render/deckhands.rs, our own work under the project licence). One vertex
// animation texture holds every clip: row r < frames is frame r's vertex positions (mesh-local xyz), row frames + r its normals; column
// = the vertex (its uv.x). Each figure's MeshTag says what it plays: bits 0-3 the clip, 4-7 the outfit, 8-15 a phase (share of the
// clip, 1/256), 16-31 when the clip started (battle seconds x 10, wrapping at 65536). The engine rewrites the tag only when the clip
// changes, so a crowd costs the CPU nothing a frame; the frame data's battle clock (`frame.clock.y`) stops while paused.
import openprocyon::render::frame::frame;

struct Deckhand {
    // per clip: x first row, y frames, z frames a second, w 1 when it loops (a one-shot holds its last frame)
    clips: array<vec4<f32>, 8>,
    // x: frames in all (the normal rows start here)
    info: vec4<f32>,
    // rgb per outfit and part (outfit * 5 + part: skin, coat, trousers, shoes, hat)
    palette: array<vec4<f32>, 80>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> deckhand: Deckhand;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var vat: texture_2d<f32>;

struct Posed {
    position: vec3<f32>,
    normal: vec3<f32>,
    color: vec4<f32>,
}

// `column`: the vertex (uv.x), `part`: its colour index (vertex colour r x 8), `tag`: the figure's MeshTag.
fn pose(column: f32, part: f32, tag: u32) -> Posed {
    let clip = deckhand.clips[tag & 15u];
    let outfit = (tag >> 4u) & 15u;
    let phase = f32((tag >> 8u) & 255u) / 256.0;
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
    let x = i32(column);
    let ra = i32(clip.x + a);
    let rb = i32(clip.x + b);
    let n = i32(deckhand.info.x);
    var out: Posed;
    out.position = mix(textureLoad(vat, vec2<i32>(x, ra), 0).xyz, textureLoad(vat, vec2<i32>(x, rb), 0).xyz, t);
    out.normal = normalize(mix(textureLoad(vat, vec2<i32>(x, ra + n), 0).xyz, textureLoad(vat, vec2<i32>(x, rb + n), 0).xyz, t));
    out.color = vec4<f32>(deckhand.palette[outfit * 5u + u32(part + 0.5)].rgb, 1.0);
    return out;
}
