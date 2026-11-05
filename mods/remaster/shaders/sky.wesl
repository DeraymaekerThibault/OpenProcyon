// Not retail: the sky dome of the render profile (game/src/render/sky.rs; contract: `SkyUniform` there, our own work under the
// project licence). Premultiplied-blended on an inverted unit sphere around the camera, behind everything the depth test keeps in front.
// Layer 1, the near ether: a sparse soft copy of the sky's own gas (the baked cube, bound at 101), sampled as if it floated
// `distance` units out. The camera's position moves the sample point, so the layer slides against the sky as the camera
// travels (parallax); the map-wide current carries the gas along. Only the brighter gas is kept, softly, so the layer
// stays thin and the sky behind it shows through.
// Layer 2, the stars: a separate cube of point stars (16-bit linear light over the gain, bound at 103), each star twinkling on
// a phase and rate of its own, the bright ones deeper; the brightest stay above 1 so they bloom. Added as light (the dome is
// drawn premultiplied: rgb is what is added, a how much of the picture behind is kept back).
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
    // x: brightness (0 = none), y: twinkle depth, z: twinkles a second, w: gain
    stars: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> sky: Sky;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var sky_cube: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var sky_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var star_cube: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var star_sampler: sampler;

fn hash3(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3<f32>(127.1, 311.7, 74.7))) * 43758.5453);
}

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
    let near_alpha = keep * sky.near.x;
    var stars = vec3<f32>(0.0);
    if sky.stars.x > 0.0 {
        // cubes are left-handed: the baker lays the stars out for the ray with z negated, as Bevy's own skybox samples
        let sd = dir * vec3<f32>(1.0, 1.0, -1.0);
        let light = textureSampleLevel(star_cube, star_sampler, sd, 0.0).rgb * sky.stars.w;
        let strength = max(max(light.r, light.g), light.b);
        // one phase and rate for every star in a small patch of sky; faint stars hardly flicker
        let cell = floor(sd * 400.0);
        let phase = hash3(cell) * 6.2831853;
        let rate = sky.stars.z * (0.6 + 0.8 * hash3(cell + vec3<f32>(7.0)));
        let wave = 0.5 + 0.5 * sin(sky.clock.x * 6.2831853 * rate + phase);
        let depth = sky.stars.y * smoothstep(0.03, 0.6, strength);
        stars = light * (1.0 - depth * wave) * sky.stars.x;
    }
    var out: FragmentOutput;
    out.color = vec4<f32>(gas * near_alpha + stars, near_alpha);
    return out;
}
