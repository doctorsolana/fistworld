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
// x: water level, y: enabled, z: server clock offset, w: surface offset.
@group(#{MATERIAL_BIND_GROUP}) @binding(123) var<uniform> water_params: vec4<f32>;

// Stylised flat palette. One binding, not seven — separate uniforms each allocate their
// own buffer and overrun the Metal vertex-stage buffer limit.
struct TerrainPalette {
    grass: vec4<f32>,
    dirt: vec4<f32>,
    sand: vec4<f32>,
    cobble: vec4<f32>,
    rock: vec4<f32>,
    // x: stylize blend, y: band count, z: band strength, w: slope-rock strength.
    stylize: vec4<f32>,
    // x: band base height, y: band height span, z: texture break-up, w: unused.
    bands: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(124) var<uniform> palette: TerrainPalette;

// The pbr imports already bind view globals; reuse them for caustic time.
#import bevy_pbr::mesh_view_bindings::globals

// Average channel value, floored so the grain divide below cannot blow up on black.
fn luminance_safe(c: vec3<f32>) -> f32 {
    return max((c.r + c.g + c.b) / 3.0, 0.02);
}

fn normalize_weights(weights: vec4<f32>) -> vec4<f32> {
    let clamped = max(weights, vec4<f32>(0.0));
    let sum = max(clamped.x + clamped.y + clamped.z + clamped.w, 0.0001);
    return clamped / sum;
}

fn tiled_uv(world_uv: vec2<f32>, tile_size: f32) -> vec2<f32> {
    let size = max(tile_size, 0.0001);
    return world_uv / size;
}

// Keep these long-wave phases in sync with shore_lap_height in toon_water.wgsl.
// Terrain only needs the displacement at the bank, where shore influence is full.
fn shore_lap_height(world_xz: vec2<f32>, time: f32) -> f32 {
    let tau = 6.28318530718;
    let primary_dir = vec2<f32>(0.8192319, -0.5734623);
    let secondary_dir = vec2<f32>(0.4472136, 0.8944272);
    let primary_phase = dot(world_xz, primary_dir) * (tau / 26.0) - time * (tau / 11.0);
    let secondary_phase = dot(world_xz, secondary_dir) * (tau / 46.0) + time * (tau / 17.0);
    return sin(primary_phase) * 0.095 + sin(secondary_phase) * 0.025;
}

// Reconstruct a short wetness history from the deterministic shore wave.
// This avoids a broad height-based gradient: a point is damp only if water
// actually covered it during the previous five seconds, and its darkness is
// based on how recently that happened.
fn recent_shore_wetness(
    world_xz: vec2<f32>,
    terrain_y: f32,
    base_surface: f32,
    time: f32,
) -> f32 {
    let current_surface = base_surface + shore_lap_height(world_xz, time);
    let above_current_surface = terrain_y - current_surface;

    // Do not shade terrain that is still underwater. The narrow transition
    // avoids a dark seam immediately beneath the foam contact line.
    let exposed = smoothstep(0.003, 0.015, above_current_surface);
    if (exposed <= 0.001 || abs(terrain_y - base_surface) > 0.14) {
        return 0.0;
    }

    const WET_LINGER_SECONDS: f32 = 5.0;
    const HISTORY_STEP_SECONDS: f32 = 0.5;
    const HISTORY_SAMPLES: u32 = 11u;
    var wet_memory = 0.0;

    for (var sample = 0u; sample < HISTORY_SAMPLES; sample++) {
        let age = f32(sample) * HISTORY_STEP_SECONDS;
        let previous_surface = base_surface + shore_lap_height(world_xz, time - age);
        let clearance = terrain_y - previous_surface;
        let was_covered = 1.0 - smoothstep(-0.004, 0.012, clearance);
        let remaining = max(1.0 - age / WET_LINGER_SECONDS, 0.0);
        wet_memory = max(wet_memory, was_covered * remaining);
    }

    return exposed * wet_memory;
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

        // --- Stylised palette ---
        //
        // Photographic splat textures read as "realistic dirt" at any grade, which is what
        // fights the low-poly look. Blend the sampled albedo toward flat per-layer colours,
        // then quantise a height tint into discrete bands — the banding is what actually
        // reads as stylised, more than any model does. A little of the sampled texture is
        // kept (palette.bands.z) so large flat areas do not look untextured.
        let stylize = palette.stylize.x;
        if (stylize > 0.001) {
            var flat_albedo = palette.grass.rgb * weights.x
                + palette.dirt.rgb * weights.y
                + palette.sand.rgb * weights.z
                + palette.cobble.rgb * weights.w;

            // Steep faces read as rock whatever the painted layer says, so cliffs stay
            // legible from directly above where slope is the only shape cue.
            let world_normal = normalize(pbr_input.world_normal);
            let slope = 1.0 - clamp(world_normal.y, 0.0, 1.0);
            let rockiness = smoothstep(0.30, 0.62, slope) * palette.stylize.w;
            flat_albedo = mix(flat_albedo, palette.rock.rgb, rockiness);

            // Discrete height bands: value steps rather than a smooth gradient.
            let band_count = max(palette.stylize.y, 1.0);
            let height_norm = clamp(
                (pbr_input.world_position.y - palette.bands.x) / max(palette.bands.y, 0.001),
                0.0,
                1.0,
            );
            let banded = floor(height_norm * band_count) / band_count;
            // Higher ground drifts lighter and cooler, like aerial perspective baked in.
            let band_tint = mix(vec3<f32>(0.94, 0.96, 0.93), vec3<f32>(1.06, 1.05, 1.02), banded);
            flat_albedo *= mix(vec3<f32>(1.0), band_tint, palette.stylize.z * 4.0);

            // Retain a trace of the sampled texture so the surface has grain.
            let grain = mix(vec3<f32>(1.0), albedo / max(luminance_safe(albedo), 0.001), palette.bands.z);
            flat_albedo *= grain;

            albedo = mix(albedo, flat_albedo, stylize);
        }

        // --- Water interaction ---
        if (water_params.y > 0.5) {
            let water_level = water_params.x;
            let h = pbr_input.world_position.y;

            // The exposed damp strip follows the exact recent path of the
            // shore wave, then dries over five seconds after the water leaves.
            let wave_time = globals.time + water_params.z;
            let wet = recent_shore_wetness(
                pbr_input.world_position.xz,
                h,
                water_level + water_params.w,
                wave_time,
            );
            albedo *= mix(vec3<f32>(1.0), vec3<f32>(0.82, 0.85, 0.87), wet);
            pbr_input.material.perceptual_roughness =
                mix(pbr_input.material.perceptual_roughness, 0.58, wet);

            // Caustics: two drifting interference fields multiplied give a
            // bright cellular web on the submerged bed, fading out both at
            // the waterline and into the depths.
            let submersion = water_level - h;
            let caustic_zone = smoothstep(0.03, 0.35, submersion)
                * (1.0 - smoothstep(1.6, 3.0, submersion));
            if (caustic_zone > 0.002) {
                let p = pbr_input.world_position.xz;
                let ct = globals.time;
                let field_a = sin(dot(p, vec2<f32>(0.86, 0.44)) * 2.1 + ct * 1.25)
                    + sin(dot(p, vec2<f32>(-0.38, 0.95)) * 1.7 - ct * 0.85);
                let field_b = sin(dot(p, vec2<f32>(0.21, -1.07)) * 2.3 + ct * 1.05)
                    + sin(dot(p, vec2<f32>(1.05, 0.57)) * 1.9 - ct * 1.35);
                let web = clamp(field_a * field_b * 0.25, 0.0, 1.0);
                let caustic = web * web * web * caustic_zone;
                albedo += caustic * vec3<f32>(0.42, 0.52, 0.55);
            }
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
