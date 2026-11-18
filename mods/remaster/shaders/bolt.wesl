// Not retail: a bolt of storm lightning of the render profile (game/src/render/bolts.rs; contract: `BoltUniform` and the mesh built
// there, our own work under the project licence). Drawn additively (ONE/ONE: write unpremultiplied light) on a camera-facing ribbon
// along the bolt: u 0..1 across (0.5 on the centre line), vertex colour rgb the glow's colour and a how bright the bolt is now.
// A white-hot thread (its share of the ribbon is `params.x`) inside a glow that fades out to the ribbon's edge; past 1 so it blooms.
import bevy_pbr::render::forward_io::{VertexOutput, FragmentOutput};

struct Bolt {
    // rgb: the thread's colour, a: brightness
    core: vec4<f32>,
    // x: the thread's share of the ribbon's half width
    params: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> bolt: Bolt;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let across = abs(in.uv.x * 2.0 - 1.0);
    let x = across / max(bolt.params.x, 0.001);
    let thread = exp(-x * x * 1.6);
    let glow = pow(max(1.0 - across, 0.0), 2.2);
    let light = bolt.core.rgb * thread * 1.6 + in.color.rgb * glow * 0.9;
    var out: FragmentOutput;
    out.color = vec4<f32>(light * bolt.core.a * in.color.a, 1.0);
    return out;
}
