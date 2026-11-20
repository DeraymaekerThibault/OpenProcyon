// Not retail: the main-pass vertex stage of the deck crews (game/src/render/deckhands.rs, our own work under the project licence):
// Bevy's mesh vertex stage without skinning and morph targets, with the figure posed from the baked moves (`deckhand_vat.wesl`).
import bevy_pbr::render::{
    mesh_bindings::mesh,
    mesh_functions,
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
};
import openprocyon::render::deckhand_vat::pose;

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    var column = 0.0;
@if(VERTEX_UVS_A)
    column = vertex.uv.x;
    var part = 0.0;
@if(VERTEX_COLORS)
    part = vertex.color.r * 8.0;
    let posed = pose(column, part, mesh[vertex_in.instance_index].tag);

    out.world_normal = mesh_functions::mesh_normal_local_to_world(posed.normal, vertex_in.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(posed.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
@if(VERTEX_COLORS)
    out.color = posed.color;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(vertex_in.instance_index, world_from_local[3]);
    return out;
}
