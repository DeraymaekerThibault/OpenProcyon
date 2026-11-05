// Not retail: the sky dome of the render profile (game/src/render/sky.rs; contract: `SkyUniform` there, our own work under the
// project licence). Alpha-blended on an inverted unit sphere around the camera, behind everything the depth test keeps in front.
// Layer 1, the near ether: a sparse soft copy of the sky's own gas (the baked cube, bound at 101), sampled as if it floated
// `distance` units out. The camera's position moves the sample point, so the layer slides against the sky as the camera
// travels (parallax); the map-wide current carries the gas along. Only the brighter gas is kept, softly, so the layer
// stays thin and the sky behind it shows through.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
};

struct Sky {
    // x: real seconds (not wrapped), y: battle seconds, z: frame delta
    clock: vec4<f32>,
    // x: opacity, y: threshold, z: softness, w: distance
    near: vec4<f32>,
    // xyz: the current, units a second
    current: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> sky: Sky;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var sky_cube: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var sky_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    let dir = normalize(in.world_position.xyz - view.world_position);
    // a point on the layer, moved by the camera and carried by the current
    let p = dir * sky.near.w + view.world_position - sky.current.xyz * sky.clock.y;
    // turned and mirrored, so the layer is not the sky's own picture laid over itself
    let q = normalize(p).zxy * vec3<f32>(1.0, -1.0, 1.0);
    let gas = textureSampleLevel(sky_cube, sky_sampler, q, 0.0).rgb;
    let brightness = dot(gas, vec3<f32>(0.3333));
    let keep = smoothstep(sky.near.y, sky.near.y + sky.near.z, brightness);
    var out: FragmentOutput;
    out.color = vec4<f32>(gas, keep * sky.near.x);
    return out;
}
