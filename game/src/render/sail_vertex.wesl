// Not retail: the main-pass vertex stage of a sail of the render profile (game/src/render/sails.rs, our own work under the project
// licence): Bevy's mesh vertex stage (bevy_pbr render/mesh.wesl, without skinning and morph targets, which sails never have) with the
// rest shape bellied and folded by `sail_fold.wesl` first.
import bevy_pbr::render::{
    mesh_bindings::mesh,
    mesh_functions,
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
};
import openprocyon::render::sail_fold::fold;

@vertex
fn vertex(vertex_in: Vertex, @builtin(vertex_index) index: u32) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let shaped = fold(vertex.position, vertex.normal, index - mesh[vertex_in.instance_index].first_vertex_index);

    out.world_normal = mesh_functions::mesh_normal_local_to_world(shaped.normal, vertex_in.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(shaped.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);

@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
@if(VERTEX_TANGENTS)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, vertex.tangent, vertex_in.instance_index);
@if(VERTEX_COLORS)
    out.color = vertex.color;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(vertex_in.instance_index, world_from_local[3]);
    return out;
}
