// Not retail: the deck crews of the render profile (game/src/render/deckhands.rs; the engine's vertex stages pose each figure and
// hand over its outfit colour as the vertex colour; our own work under the project licence). Lit as a plain StandardMaterial, with a
// faint cool rim so a crew of 5-20 px figures still reads as people against the planks, and never sinks to black in the hull's shade.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::view,
};

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let pbr_input = pbr_input_from_standard_material(in, is_front);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    let to_cam = normalize(view.world_position.xyz - in.world_position.xyz);
    let rim = pow(1.0 - clamp(dot(normalize(in.world_normal), to_cam), 0.0, 1.0), 3.0);
    out.color = vec4<f32>(out.color.rgb * (1.0 + vec3<f32>(0.15, 0.2, 0.3) * rim), 1.0);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
