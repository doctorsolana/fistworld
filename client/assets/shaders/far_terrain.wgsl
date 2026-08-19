// Low-resolution world material. Land keeps StandardMaterial's normal PBR
// response; ocean vertices are a stable, unlit continuation of the detailed
// water surface. COLOR_0 alpha is authored metadata (1 land, 0 sea-level
// ocean, 0.25 river stamp), interpolated by the rasterizer to soften the far
// shoreline.

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::{globals, view},
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    view_transformations::position_world_to_clip,
}

#ifdef VISIBILITY_RANGE_DITHER
#import bevy_pbr::pbr_functions::visibility_range_dither;
#endif

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> water_params: vec4<f32>;
// xyz: direction to the sun (world), w: glint strength (0 at night).
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> sun_glint: vec4<f32>;

// === Map-scale ocean glitter (EXACT copy in toon_water.wgsl / far_terrain.wgsl — keep in sync) ===
// Sparse world-anchored sparkle that stays a couple of pixels wide at any
// zoom: the cell size tracks the fragment's world footprint across two
// blended power-of-two levels, so the twinkle population per screen area is
// constant and can never alias into the moire blobs a sub-pixel ripple field
// produces. The base 0.588m cell matches the detailed glint's sparkle hash.
// Sinless hash: sin-based hashes lose precision at cell ids in the
// thousands and their error is spatially correlated, which read as diagonal
// rows of sparkle across the open sea.
fn glitter_hash(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, vec3<f32>(p3.y, p3.z, p3.x) + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn glitter_level(
    world_xz: vec2<f32>,
    cell: f32,
    view_vec: vec3<f32>,
    sun_dir: vec3<f32>,
    time: f32,
) -> f32 {
    let id = floor(world_xz / cell);
    let rnd = glitter_hash(id);
    // Sparse population: most cells stay dark.
    if (rnd < 0.86) {
        return 0.0;
    }
    let jitter = vec2<f32>(glitter_hash(id + 17.0), glitter_hash(id + 41.0)) - 0.5;
    let local = fract(world_xz / cell) - 0.5 - jitter * 0.9;
    let d = length(local) * cell;
    let radius = cell * 0.09;
    let aa = max(fwidth(d), cell * 0.03);
    let dot_mask = 1.0 - smoothstep(radius - aa, radius + aa, d);
    // Per-cell pseudo ripple facet. The half-vector specular keeps lit cells
    // concentrated along the sun path without animated surface normals.
    let tilt = (vec2<f32>(glitter_hash(id + 5.0), glitter_hash(id + 9.0)) - 0.5) * 1.1;
    let wobble = 0.12 * vec2<f32>(
        sin(time * (2.0 + 3.0 * fract(rnd * 13.7)) + rnd * 6.2832),
        cos(time * (1.7 + 2.6 * fract(rnd * 7.31)) + rnd * 12.566),
    );
    let facet = normalize(vec3<f32>(tilt.x + wobble.x, 1.0, tilt.y + wobble.y));
    let half_vec = normalize(view_vec + sun_dir);
    let spec = pow(max(dot(facet, half_vec), 0.0), 48.0);
    let twinkle = 0.35
        + 0.65 * (0.5 + 0.5 * sin(time * (2.5 + 4.0 * fract(rnd * 9.17)) + rnd * 25.13));
    return dot_mask * spec * twinkle;
}

fn ocean_glitter(
    world_xz: vec2<f32>,
    view_vec: vec3<f32>,
    sun_dir: vec3<f32>,
    time: f32,
) -> f32 {
    let fp = max(max(fwidth(world_xz.x), fwidth(world_xz.y)), 1.0e-4);
    // Cell spacing ~12 px so each lit dot renders ~2-3 px wide.
    let lod = max(log2(fp * 12.0 / 0.588), 0.0);
    let level = floor(lod);
    let cell0 = 0.588 * exp2(level);
    let g0 = glitter_level(world_xz, cell0, view_vec, sun_dir, time);
    let g1 = glitter_level(world_xz + vec2<f32>(37.0, -11.0), cell0 * 2.0, view_vec, sun_dir, time);
    return mix(g0, g1, fract(lod));
}

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
    // COLOR_0 alpha is authored metadata: 0.0 = sea-level ocean underlay,
    // 0.25 = river stamp, 1.0 = land, interpolated at the boundaries.
    let authored_a = clamp(in.color.a, 0.0, 1.0);
#else
    let authored_a = 1.0;
#endif
    let is_river = authored_a > 0.1 && authored_a < 0.4;
    let ocean = select(1.0 - authored_a, 1.0, is_river);

    // COLOR_0 RGB already contains the depth ramp. Alpha is metadata here,
    // not transparency, so restore opacity before StandardMaterial shading.
    let authored_rgb = pbr_input.material.base_color.rgb;
    pbr_input.material.base_color = vec4<f32>(authored_rgb, 1.0);

    let inside_detail_hole = water_params.w > 0.0
        && all(abs(in.world_position.xz - water_params.yz) <= vec2<f32>(water_params.w));

    // The sea-level ocean is the opaque underlay for the entire detailed-water
    // surface. Discarding it inside the water core made translucent water
    // blend over streamed seabed in one chunk and over the clear background in
    // the next, exposing the terrain streaming square as a dark box during
    // fast pans — so only IT survives inside the hole. Land pokes through
    // detail, and the river stamp (with its interpolated banks) rendered as an
    // opaque band floating over the detailed terrain at middle zoom; both must
    // go. water_params.yz is the detail-hole center and w its half extent.
    if (inside_detail_hole && authored_a > 0.1) {
        discard;
    }

    // Preserve the existing far-land renderer, including its atmospheric
    // processing. Far water intentionally follows the detailed water shader's
    // unlit path instead of being dimmed like a steeply-lit seabed.
    let lit = apply_pbr_lighting(pbr_input);
    let lit_land = main_pass_post_lighting_processing(pbr_input, lit);

    let day_w = smoothstep(-0.08, 0.12, water_params.x);
    var water_rgb = mix(
        authored_rgb * vec3<f32>(0.20, 0.26, 0.45),
        authored_rgb,
        day_w,
    );

    // Map-scale sun sparkle on the far ocean. It fades in across the exact
    // band where the detailed water surface alpha-fades out (map_view.rs
    // WATER_FADE_START/END — keep in sync), so the handoff carries one
    // continuous glimmer instead of doubling it up close or dropping it far.
    let view_vec = normalize(view.world_position.xyz - in.world_position.xyz);
    let view_dist = distance(view.world_position.xyz, in.world_position.xyz);
    let reveal = smoothstep(1250.0, 1800.0, view_dist);
    if (ocean > 0.0 && reveal > 0.0 && sun_glint.w > 0.0) {
        let glint = min(
            ocean_glitter(
                in.world_position.xz,
                view_vec,
                normalize(sun_glint.xyz),
                globals.time,
            ) * sun_glint.w,
            1.4,
        );
        water_rgb += glint * vec3<f32>(1.0, 0.97, 0.88) * day_w * reveal;
    }

    var out: FragmentOutput;
    out.color = vec4<f32>(mix(lit_land.rgb, water_rgb, ocean), 1.0);
    return out;
}
