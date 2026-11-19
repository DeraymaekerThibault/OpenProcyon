// Not retail: the prepass vertex stage (depth prepass and shadow maps) of a sail of the render profile (game/src/render/sails.rs, our
// own work under the project licence): Bevy's prepass vertex stage (bevy_pbr prepass/prepass.wesl, without skinning and morph targets)
// with the same bellied and folded shape as the main pass (`sail_fold.wesl`), so depth, shadows and the drawn cloth agree.
import bevy_pbr::{
    prepass::io::{Vertex, VertexOutput, decompress_vertex},
    render::{
        mesh_bindings::mesh,
        mesh_functions,
        view_transformations::position_world_to_clip,
    },
};
import openprocyon::render::sail_fold::fold;

@vertex
fn vertex(vertex_in: Vertex, @builtin(vertex_index) index: u32) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    var normal = vec3<f32>(0.0, 1.0, 0.0);
@if(VERTEX_NORMALS)
    normal = vertex.normal;
    let shaped = fold(vertex.position, normal, index - mesh[vertex_in.instance_index].first_vertex_index);

    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(shaped.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
}
@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(shaped.normal, vertex_in.instance_index);
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_TANGENTS)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, vertex.tangent, vertex_in.instance_index);
@if(VERTEX_COLORS)
    out.color = vertex.color;
@if(MOTION_VECTOR_PREPASS)
    out.previous_world_position = mesh_functions::mesh_position_local_to_world(
        mesh_functions::get_previous_world_from_local(vertex_in.instance_index), vec4<f32>(shaped.position, 1.0));
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(vertex_in.instance_index, world_from_local[3]);
    return out;
}
