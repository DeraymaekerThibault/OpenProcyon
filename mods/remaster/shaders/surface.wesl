// Not retail: surface detail of the render profile (game/src/render/surfaces.rs; contract: `SurfaceUniform` there, our
// own work under the project licence). The retail models have only
// colour textures, so the relief is read from the colour texture's brightness as it draws: darker texels lie lower (plank
// seams, rivets, cracks in rock). Their slope tilts the normal (in a frame built from the screen-space derivatives, so
// the mesh needs no tangents), the crevices are shaded darker, and brighter texels are a little smoother. The relief fades
// out where the texture is drawn smaller than its texels, so distant ships do not shimmer. A soft light along the
// silhouettes (the rim) lifts the hulls off the dark sky, the way a painter edges a figure against the night.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_bindings::{base_color_texture, base_color_sampler},
    mesh_view_bindings::view,
};

struct Surface {
    // x: relief strength, y: crevice shading, z: gloss, w: unused
    params: vec4<f32>,
    // rgb: rim light colour, a: its strength
    rim: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> surface: Surface;

fn luma(uv: vec2<f32>) -> f32 {
    return dot(textureSample(base_color_texture, base_color_sampler, uv).rgb, vec3<f32>(0.299, 0.587, 0.114));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // derivatives and samples first, in uniform control flow
    let uv = in.uv;
    let size = vec2<f32>(textureDimensions(base_color_texture));
    let texel = 1.0 / max(size, vec2<f32>(1.0));
    let dpx = dpdx(in.world_position.xyz);
    let dpy = dpdy(in.world_position.xyz);
    let duvx = dpdx(uv);
    let duvy = dpdy(uv);
    let h = luma(uv);
    let hr = luma(uv + vec2<f32>(texel.x, 0.0));
    let hl = luma(uv - vec2<f32>(texel.x, 0.0));
    let hd = luma(uv + vec2<f32>(0.0, texel.y));
    let hu = luma(uv - vec2<f32>(0.0, texel.y));

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
    let slope = vec2<f32>(hr - hl, hd - hu) * 0.5;
    let bump = surface.params.x * detail;
    pbr_input.N = normalize(n - bump * (slope.x * t * inv + slope.y * b * inv));

    // crevices: lower than their neighbours
    let around = (hr + hl + hd + hu) * 0.25;
    let crevice = clamp((around - h) * 6.0, 0.0, 1.0) * detail;
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
