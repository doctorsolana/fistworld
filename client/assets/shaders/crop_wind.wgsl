// Bevy 0.19 rigid mesh vertex contract. StandardMaterial still owns forward,
// prepass and deferred fragments, including visibility-range dither. The same
// displacement drives depth/shadows and previous-frame TAA motion vectors.
#import bevy_pbr::{
    mesh_functions,
    view_transformations::position_world_to_clip,
}
#import bevy_render::globals::Globals
#ifdef PREPASS_PIPELINE
#import bevy_pbr::prepass_io::{Vertex, VertexOutput}
@group(0) @binding(1) var<uniform> globals: Globals;
#else
#import bevy_pbr::forward_io::{Vertex, VertexOutput}
@group(0) @binding(11) var<uniform> globals: Globals;
#endif

fn swayed_position(position: vec4<f32>, weight: f32, seconds: f32) -> vec4<f32> {
    // Mirrored from client/src/wind.rs; its exact direction parity test covers this shader.
    let dir = vec2<f32>(0.8206, 0.5715);
    let side = vec2<f32>(-dir.y, dir.x);
    let along = dot(position.xz, dir);
    let across = dot(position.xz, side);
    // Bevy's globals clock wraps each hour. Integer cycles per hour preserve
    // continuous wind and previous-frame samples across that rollover; these
    // approximate the shared 1.15 gust rate within 0.04%.
    let cycle = seconds * 0.001745329252;
    // World-space phases are identical for close and distant crop meshes.
    let wave = 0.65 * sin(along * 0.32 - cycle * 1186.0)
        + 0.35 * sin(along * 0.11 + across * 0.19 - cycle * 527.0);
    let push = (0.35 + 0.65 * wave) * 0.10 * weight;
    let flutter = 0.16 * sin(along * 0.7 + across * 0.39 + cycle * 1713.0) * 0.10 * weight;
    let offset = push * dir + flutter * side;
    // Maximum horizontal offset is <0.102m. Mesh weights reserve 0.11m of
    // accepted crop land, with zero weight at soil and rooted stalk vertices.
    return position + vec4<f32>(offset.x, -dot(offset, offset) * 0.4, offset.y, 0.0);
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    var weight = 0.0;
#ifdef VERTEX_UVS_A
    weight = clamp(vertex.uv.x, 0.0, 1.0);
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
    let rest = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
    out.world_position = swayed_position(rest, weight, globals.time);
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
#endif
#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, vertex.tangent, vertex.instance_index);
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
#ifdef PREPASS_PIPELINE
#ifdef MOTION_VECTOR_PREPASS
    let previous_model = mesh_functions::get_previous_world_from_local(vertex.instance_index);
    let previous_rest = mesh_functions::mesh_position_local_to_world(previous_model, vec4<f32>(vertex.position, 1.0));
    out.previous_world_position = swayed_position(previous_rest, weight, globals.time - globals.delta_time);
#endif
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(vertex.instance_index, world_from_local[3]);
#endif
    return out;
}
