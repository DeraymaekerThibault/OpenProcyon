// Not retail: surface detail of the render profile (game/src/render/surfaces.rs; contract: `SurfaceUniform` there, our
// own work under the project licence). The retail models have only
// colour textures, so the relief is read from the colour texture's brightness as it draws: darker texels lie lower (plank
// seams, rivets, cracks in rock). Their slope tilts the normal (in a frame built from the screen-space derivatives, so
// the mesh needs no tangents), the crevices are shaded darker, and brighter texels are a little smoother. The relief fades
// out where the texture is drawn smaller than its texels, so distant ships do not shimmer. When the player has baked surface
// maps (tools/bake_surfaces.py) the slope, crevices and height come from the material's map instead, filtered offline and
// taken at several scales, so rock and asteroids show their larger shapes too. A soft light along the
// silhouettes (the rim) lifts the hulls off the dark sky, the way a painter edges a figure against the night.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::view,
};
@if(BINDLESS)
import bevy_pbr::render::{mesh_bindings::mesh, pbr_bindings::material_indices};
@if(BINDLESS)
import bevy_render::bindless::{bindless_samplers_filtering, bindless_textures_2d};
@if(!BINDLESS)
import bevy_pbr::render::pbr_bindings::{base_color_texture, base_color_sampler};

struct Surface {
    // x: relief strength, y: crevice shading, z: gloss, w: 1 when the surface map is a baked map
    params: vec4<f32>,
    // rgb: rim light colour, a: its strength
    rim: vec4<f32>,
}

// The surface map is linear RGBA: u and v slope (times SLOPE_GAIN, 0.5 flat), crevice, height.
// Bindless (the usual case): every surface shares one bind group, and the mesh's slot picks its row of each table.
@if(BINDLESS) {
struct SurfaceIndices {
    surface: u32,       // 50
    map: u32,           // 51
    map_sampler: u32,   // 52
}
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<storage> surface_indices: array<SurfaceIndices>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<storage> surfaces: array<Surface>;
}
@else {
@group(constants::MATERIAL_BIND_GROUP) @binding(50) var<uniform> surface_uniform: Surface;
@group(constants::MATERIAL_BIND_GROUP) @binding(51) var surface_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(52) var surface_map_sampler: sampler;
}
// `SLOPE_GAIN` in tools/bake_surfaces.py
const SLOPE_GAIN: f32 = 3.0;

fn rgb_luma(c: vec4<f32>) -> f32 {
    return dot(c.rgb, vec3<f32>(0.299, 0.587, 0.114));
}

// The colour texture's brightness at `uv`, and its size in texels.
@if(BINDLESS)
fn luma(slot: u32, uv: vec2<f32>) -> f32 {
    return rgb_luma(textureSample(bindless_textures_2d[material_indices[slot].base_color_texture], bindless_samplers_filtering[material_indices[slot].base_color_sampler], uv));
}
@if(BINDLESS)
fn colour_size(slot: u32) -> vec2<f32> {
    return vec2<f32>(textureDimensions(bindless_textures_2d[material_indices[slot].base_color_texture]));
}
@if(BINDLESS)
fn baked(slot: u32, uv: vec2<f32>) -> vec4<f32> {
    return textureSample(bindless_textures_2d[surface_indices[slot].map], bindless_samplers_filtering[surface_indices[slot].map_sampler], uv);
}
@if(!BINDLESS)
fn luma(slot: u32, uv: vec2<f32>) -> f32 {
    return rgb_luma(textureSample(base_color_texture, base_color_sampler, uv));
}
@if(!BINDLESS)
fn colour_size(slot: u32) -> vec2<f32> {
    return vec2<f32>(textureDimensions(base_color_texture));
}
@if(!BINDLESS)
fn baked(slot: u32, uv: vec2<f32>) -> vec4<f32> {
    return textureSample(surface_map, surface_map_sampler, uv);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // the mesh's row of the material tables (unused without bindless), then derivatives and samples first
@if(BINDLESS)
    let slot = mesh[in.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
@if(BINDLESS)
    let surface = surfaces[surface_indices[slot].surface];
@if(!BINDLESS)
    let slot = 0u;
@if(!BINDLESS)
    let surface = surface_uniform;
    let uv = in.uv;
    let size = colour_size(slot);
    let texel = 1.0 / max(size, vec2<f32>(1.0));
    let dpx = dpdx(in.world_position.xyz);
    let dpy = dpdy(in.world_position.xyz);
    let duvx = dpdx(uv);
    let duvy = dpdy(uv);
    // height, its slope along u and v, and how far below its surroundings it lies; the branch is the same for a whole mesh
    var h: f32;
    var slope: vec2<f32>;
    var below: f32;
    if surface.params.w > 0.5 {
        let m = baked(slot, uv);
        h = m.a;
        slope = (m.rg * 2.0 - 1.0) / SLOPE_GAIN;
        below = m.b;
    } else {
        h = luma(slot, uv);
        let hr = luma(slot, uv + vec2<f32>(texel.x, 0.0));
        let hl = luma(slot, uv - vec2<f32>(texel.x, 0.0));
        let hd = luma(slot, uv + vec2<f32>(0.0, texel.y));
        let hu = luma(slot, uv - vec2<f32>(0.0, texel.y));
        slope = vec2<f32>(hr - hl, hd - hu) * 0.5;
        below = clamp(((hr + hl + hd + hu) * 0.25 - h) * 6.0, 0.0, 1.0);
    }

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);

    // texels per pixel: the relief is gone by the time four texels share a pixel
    let footprint = max(length(duvx * size), length(duvy * size));
    let detail = 1.0 - smoothstep(1.0, 4.0, footprint);

    // cotangent frame: how u and v run across the surface
    let n = pbr_input.N;
    let dp2perp = cross(dpy, n);
    let dp1perp = cross(n, dpx);
    let t = dp2perp * duvx.x + dp1perp * duvy.x;
    let b = dp2perp * duvx.y + dp1perp * duvy.y;
    let inv = inverseSqrt(max(max(dot(t, t), dot(b, b)), 1e-30));
    let bump = surface.params.x * detail;
    pbr_input.N = normalize(n - bump * (slope.x * t * inv + slope.y * b * inv));

    // crevices: lower than their neighbours
    let crevice = below * detail;
    let shade = 1.0 - surface.params.y * crevice;
    pbr_input.material.base_color = vec4<f32>(pbr_input.material.base_color.rgb * shade, pbr_input.material.base_color.a);
    pbr_input.diffuse_occlusion = pbr_input.diffuse_occlusion * shade;

    let r = pbr_input.material.perceptual_roughness * mix(1.0 + surface.params.z, 1.0 - surface.params.z, clamp(h * 1.6, 0.0, 1.0));
    pbr_input.material.perceptual_roughness = clamp(r + crevice * 0.2, 0.05, 1.0);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    let to_cam = normalize(view.world_position.xyz - in.world_position.xyz);
    let rim = pow(1.0 - clamp(dot(pbr_input.N, to_cam), 0.0, 1.0), 3.0);
    out.color = vec4<f32>(out.color.rgb + surface.rim.rgb * surface.rim.a * rim * pbr_input.material.base_color.rgb * view.exposure, out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
