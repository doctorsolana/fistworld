// Low-resolution world material. Land keeps StandardMaterial's normal PBR
// response; ocean vertices are a stable, unlit continuation of the detailed
// water surface. COLOR_0 alpha is the authored land mask (1 land, 0 ocean),
// interpolated by the rasterizer to soften the far shoreline.

#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    view_transformations::position_world_to_clip,
}

#ifdef VISIBILITY_RANGE_DITHER
#import bevy_pbr::pbr_functions::visibility_range_dither;
#endif

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> water_params: vec4<f32>;

@vertex
fn vertex(vertex_no_morph: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex_no_morph.instance_index);
    let world_pos = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex_no_morph.position, 1.0),
    );

    out.world_position = world_pos;
    out.position = position_world_to_clip(world_pos.xyz);

    // Geometry remains fixed throughout the transition. The far land mesh is
    // authored as a stable 5 cm underlay, while water is authored at its real
    // surface height. Do not apply a distance-dependent world or clip-space
    // displacement here: either one becomes visible at map-scale distances.

#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex_no_morph.normal,
        vertex_no_morph.instance_index,
    );
#endif
#ifdef VERTEX_UVS_A
    out.uv = vertex_no_morph.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex_no_morph.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex_no_morph.tangent,
        vertex_no_morph.instance_index,
    );
#endif
#ifdef VERTEX_COLORS
    out.color = vertex_no_morph.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex_no_morph.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_no_morph.instance_index,
        world_from_local[3],
    );
#endif

    return out;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
#ifdef VISIBILITY_RANGE_DITHER
    visibility_range_dither(in.position, in.visibility_range_dither);
#endif

    var pbr_input = pbr_input_from_standard_material(in, is_front);

#ifdef VERTEX_COLORS
    let ocean = 1.0 - clamp(in.color.a, 0.0, 1.0);
#else
    let ocean = 0.0;
#endif

    // COLOR_0 RGB already contains the depth ramp. Alpha is metadata here,
    // not transparency, so restore opacity before StandardMaterial shading.
    let authored_rgb = pbr_input.material.base_color.rgb;
    pbr_input.material.base_color = vec4<f32>(authored_rgb, 1.0);

    let inside_detail_hole = water_params.w > 0.0
        && all(abs(in.world_position.xz - water_params.yz) <= vec2<f32>(water_params.w));

    // Far water is the opaque underlay for the entire detailed-water surface.
    // Discarding it inside the water core made translucent water blend over
    // streamed seabed in one chunk and over the clear background in the next,
    // exposing the terrain streaming square as a dark box during fast pans.
    // Retain the land-only hole so coarse ground cannot poke through detail.
    // water_params.yz is the detail-hole center and w its half extent.
    if (inside_detail_hole && ocean < 0.5) {
        discard;
    }

    // Preserve the existing far-land renderer, including its atmospheric
    // processing. Far water intentionally follows the detailed water shader's
    // unlit path instead of being dimmed like a steeply-lit seabed.
    let lit = apply_pbr_lighting(pbr_input);
    let lit_land = main_pass_post_lighting_processing(pbr_input, lit);

    let day_w = smoothstep(-0.08, 0.12, water_params.x);
    let water_rgb = mix(
        authored_rgb * vec3<f32>(0.20, 0.26, 0.45),
        authored_rgb,
        day_w,
    );

    var out: FragmentOutput;
    out.color = vec4<f32>(mix(lit_land.rgb, water_rgb, ocean), 1.0);
    return out;
}
