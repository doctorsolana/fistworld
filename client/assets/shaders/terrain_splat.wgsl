#define_import_path terrain_splat

#import bevy_pbr::{
    pbr_types,
    pbr_functions::{alpha_discard, apply_normal_mapping, calculate_tbn_mikktspace},
    pbr_fragment::pbr_input_from_standard_material,
    decal::clustered::apply_decals,
}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#endif

#ifdef VISIBILITY_RANGE_DITHER
#import bevy_pbr::pbr_functions::visibility_range_dither;
#endif

#ifdef MESHLET_MESH_MATERIAL_PASS
#import bevy_pbr::meshlet_visibility_buffer_resolve::resolve_vertex_output
#endif

#ifdef OIT_ENABLED
#import bevy_core_pipeline::oit::oit_draw
#endif

#ifdef FORWARD_DECAL
#import bevy_pbr::decal::forward::get_forward_decal_info
#endif

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var weight_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var weight_map_sampler: sampler;

@group(#{MATERIAL_BIND_GROUP}) @binding(102) var albedo_array: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var albedo_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var normal_array: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var normal_sampler: sampler;

@group(#{MATERIAL_BIND_GROUP}) @binding(120) var<uniform> layer_tiling: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(121) var<uniform> debug_mode: u32;
@group(#{MATERIAL_BIND_GROUP}) @binding(122) var<uniform> normal_strength: f32;

fn normalize_weights(weights: vec4<f32>) -> vec4<f32> {
    let clamped = max(weights, vec4<f32>(0.0));
    let sum = max(clamped.x + clamped.y + clamped.z + clamped.w, 0.0001);
    return clamped / sum;
}

fn tiled_uv(world_uv: vec2<f32>, tile_size: f32) -> vec2<f32> {
    let size = max(tile_size, 0.0001);
    return world_uv / size;
}

@fragment
fn fragment(
#ifdef MESHLET_MESH_MATERIAL_PASS
    @builtin(position) frag_coord: vec4<f32>,
#else
    vertex_output: VertexOutput,
    @builtin(front_facing) is_front: bool,
#endif
) -> FragmentOutput {
#ifdef MESHLET_MESH_MATERIAL_PASS
    let vertex_output = resolve_vertex_output(frag_coord);
    let is_front = true;
#endif

    var in = vertex_output;

#ifdef VISIBILITY_RANGE_DITHER
    visibility_range_dither(in.position, in.visibility_range_dither);
#endif

#ifdef FORWARD_DECAL
    let forward_decal_info = get_forward_decal_info(in);
    in.world_position = forward_decal_info.world_position;
    in.uv = forward_decal_info.uv;
#endif

    var pbr_input = pbr_input_from_standard_material(in, is_front);

    var weight_uv = vec2<f32>(0.0);
#ifdef VERTEX_UVS
    weight_uv = in.uv;
#endif
    let sampled_weights = textureSample(weight_map, weight_map_sampler, weight_uv);
    let base_weights = normalize_weights(sampled_weights);

    if (debug_mode == 1u) {
        pbr_input.material.flags = pbr_input.material.flags | pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT;
        pbr_input.material.base_color = vec4<f32>(base_weights.xyz, 1.0);
        pbr_input.N = normalize(pbr_input.world_normal);
    } else {
        var weights = base_weights;
        if (debug_mode == 2u) {
            weights = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        } else if (debug_mode == 3u) {
            weights = vec4<f32>(1.0, 0.0, 0.0, 0.0);
        }

        let world_uv = pbr_input.world_position.xz;
        let uv_grass = tiled_uv(world_uv, layer_tiling.x);
        let uv_dirt = tiled_uv(world_uv, layer_tiling.y);
        let uv_sand = tiled_uv(world_uv, layer_tiling.z);
        let uv_cobble = tiled_uv(world_uv, layer_tiling.w);

        var dominant_layer: u32 = 0u;
        var dominant_weight = weights.x;
        if (weights.y > dominant_weight) {
            dominant_weight = weights.y;
            dominant_layer = 1u;
        }
        if (weights.z > dominant_weight) {
            dominant_weight = weights.z;
            dominant_layer = 2u;
        }
        if (weights.w > dominant_weight) {
            dominant_weight = weights.w;
            dominant_layer = 3u;
        }
        // Favor the single-layer path more aggressively to reduce texture fetch cost.
        let use_single_layer_fast_path = dominant_weight >= 0.70;

        var albedo = vec3<f32>(0.0, 0.0, 0.0);
        if (use_single_layer_fast_path) {
            if (dominant_layer == 0u) {
                albedo = textureSample(albedo_array, albedo_sampler, uv_grass, 0).rgb;
            } else if (dominant_layer == 1u) {
                albedo = textureSample(albedo_array, albedo_sampler, uv_dirt, 1).rgb;
            } else if (dominant_layer == 2u) {
                albedo = textureSample(albedo_array, albedo_sampler, uv_sand, 2).rgb;
            } else {
                albedo = textureSample(albedo_array, albedo_sampler, uv_cobble, 3).rgb;
            }
        } else {
            let grass_albedo = textureSample(albedo_array, albedo_sampler, uv_grass, 0).rgb;
            let dirt_albedo = textureSample(albedo_array, albedo_sampler, uv_dirt, 1).rgb;
            let sand_albedo = textureSample(albedo_array, albedo_sampler, uv_sand, 2).rgb;
            let cobble_albedo = textureSample(albedo_array, albedo_sampler, uv_cobble, 3).rgb;
            albedo = grass_albedo * weights.x
                + dirt_albedo * weights.y
                + sand_albedo * weights.z
                + cobble_albedo * weights.w;
        }

        pbr_input.material.base_color = vec4<f32>(albedo, 1.0);

#ifdef VERTEX_TANGENTS
        if (normal_strength > 0.001) {
            var blended_nt = vec3<f32>(0.5, 0.5, 1.0);
            if (use_single_layer_fast_path) {
                if (dominant_layer == 0u) {
                    blended_nt = textureSample(normal_array, normal_sampler, uv_grass, 0).xyz;
                } else if (dominant_layer == 1u) {
                    blended_nt = textureSample(normal_array, normal_sampler, uv_dirt, 1).xyz;
                } else if (dominant_layer == 2u) {
                    blended_nt = textureSample(normal_array, normal_sampler, uv_sand, 2).xyz;
                } else {
                    blended_nt = textureSample(normal_array, normal_sampler, uv_cobble, 3).xyz;
                }
            } else {
                let grass_nt = textureSample(normal_array, normal_sampler, uv_grass, 0).xyz;
                let dirt_nt = textureSample(normal_array, normal_sampler, uv_dirt, 1).xyz;
                let sand_nt = textureSample(normal_array, normal_sampler, uv_sand, 2).xyz;
                let cobble_nt = textureSample(normal_array, normal_sampler, uv_cobble, 3).xyz;
                blended_nt = normalize(
                    grass_nt * weights.x
                        + dirt_nt * weights.y
                        + sand_nt * weights.z
                        + cobble_nt * weights.w
                );
            }

            let double_sided =
                (pbr_input.material.flags & pbr_types::STANDARD_MATERIAL_FLAGS_DOUBLE_SIDED_BIT) != 0u;
            let tbn = calculate_tbn_mikktspace(in.world_normal, in.world_tangent);
            let mapped_n = apply_normal_mapping(
                pbr_input.material.flags,
                tbn,
                double_sided,
                is_front,
                blended_nt,
            );
            pbr_input.N = normalize(mix(normalize(pbr_input.world_normal), mapped_n, clamp(normal_strength, 0.0, 1.0)));
        } else {
            pbr_input.N = normalize(pbr_input.world_normal);
        }
#else
        pbr_input.N = normalize(pbr_input.world_normal);
#endif
    }

    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    apply_decals(&pbr_input);

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    if (pbr_input.material.flags & pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif

#ifdef OIT_ENABLED
    let alpha_mode = pbr_input.material.flags & pbr_types::STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS;
    if alpha_mode != pbr_types::STANDARD_MATERIAL_FLAGS_ALPHA_MODE_OPAQUE {
        oit_draw(in.position, out.color);
        discard;
    }
#endif

#ifdef FORWARD_DECAL
    out.color.a = min(forward_decal_info.alpha, out.color.a);
#endif

    return out;
}
