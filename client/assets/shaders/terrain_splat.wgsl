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
    // x: cloud coverage, y: inv world scale, zw: wind offset (world units).
    clouds_a: vec4<f32>,
    // xy: sun projection (sun_dir.xz / sun_dir.y), z: shadow strength, w: seed phase.
    clouds_b: vec4<f32>,
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

// EXACT copy of the depth-phased wave in toon_water.wgsl's shore_lap_height —
// the wet-sand memory below reconstructs which ground the waves covered, so
// any drift between the two formulas shows up as wet patches out of sync with
// the visible water. Terrain evaluates this only inside the ±0.14m waterline
// strip, where the water's horizontal shore fade is ~1, so shore_zone is
// taken as 1 and the phase comes from the point's own signed depth.
fn shore_lap_height(world_xz: vec2<f32>, signed_depth: f32, time: f32) -> f32 {
    let tau = 6.28318530718;
    let wob_a = sin(dot(world_xz, vec2<f32>(0.11, 0.073)) + time * 0.26);
    let wob_b = sin(dot(world_xz, vec2<f32>(-0.031, 0.042)) + time * 0.17 + 2.1);
    let wobble = wob_a * 1.05 + wob_b * 1.35;
    let primary_phase = signed_depth * (tau * 1.55) + time * (tau / 12.0) + wobble;
    let primary = sin(primary_phase) + 0.25 * sin(primary_phase * 2.0);
    let secondary_phase = signed_depth * (tau * 3.05) + time * (tau / 7.5) + 1.7 + wobble * 0.6;
    let shoaling = 1.0 + (1.0 - clamp(signed_depth, 0.0, 1.0)) * 0.45;
    return (primary * 0.059 + sin(secondary_phase) * 0.017) * shoaling;
}

// Altitude of the procedural cloud field the shadows are projected from.
const CLOUD_LAYER_HEIGHT: f32 = 350.0;

// === Cloud field (EXACT copy in terrain_splat.wgsl / toon_water.wgsl / cloud_layer.wgsl — keep in sync) ===
fn cloud_hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}
fn cloud_vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(cloud_hash(i), cloud_hash(i + vec2<f32>(1.0, 0.0)), u.x),
        mix(cloud_hash(i + vec2<f32>(0.0, 1.0)), cloud_hash(i + vec2<f32>(1.0, 1.0)), u.x),
        u.y,
    );
}
// Rotation + ~2x lacunarity in one matrix kills axis-aligned streaking.
const CLOUD_M: mat2x2<f32> = mat2x2<f32>(vec2<f32>(1.6, -1.2), vec2<f32>(1.2, 1.6));
fn cloud_fbm(p_in: vec2<f32>) -> f32 {
    var p = p_in;
    var amp = 0.55;
    var sum = 0.0;
    var norm = 0.0;
    for (var i = 0; i < 4; i++) {
        sum += amp * cloud_vnoise(p);
        norm += amp;
        amp *= 0.55;
        p = CLOUD_M * p;
    }
    return sum / norm;
}
fn cloud_ridge(p_in: vec2<f32>) -> f32 {
    var p = p_in;
    var amp = 0.8;
    var sum = 0.0;
    var norm = 0.0;
    for (var i = 0; i < 4; i++) {
        sum += amp * abs(cloud_vnoise(p) * 2.0 - 1.0);
        norm += amp;
        amp *= 0.7;
        p = CLOUD_M * p;
    }
    return sum / norm;
}
// Density 0..1 at a world XZ position. params_a: (coverage 0..1, inv world scale, wind_offset.x, wind_offset.y)
fn cloud_density(world_xz: vec2<f32>, params_a: vec4<f32>, seed_phase: f32) -> f32 {
    let p0 = (world_xz + params_a.zw) * params_a.y + vec2<f32>(seed_phase, seed_phase * 1.73);
    let q = vec2<f32>(cloud_fbm(p0 * 0.5), cloud_fbm(p0 * 0.5 + vec2<f32>(5.2, 1.3)));
    let p = p0 + 0.9 * (q - vec2<f32>(0.5, 0.5));
    let shape = cloud_fbm(p) * cloud_ridge(p * 0.9) * 2.4;
    let cover = clamp(params_a.x, 0.0, 1.0);
    let thresh = mix(0.78, 0.34, cover);
    return smoothstep(thresh, thresh + 0.28, shape);
}

// Reconstruct a short wetness history from the deterministic shore wave.
// This avoids a broad height-based gradient: a point is damp only if water
// actually covered it during the previous five seconds, and its darkness is
// based on how recently that happened.
fn recent_shore_wetness(
    world_xz: vec2<f32>,
    terrain_y: f32,
    base_surface: f32,
    signed_depth: f32,
    time: f32,
) -> f32 {
    let current_surface = base_surface + shore_lap_height(world_xz, signed_depth, time);
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
        let previous_surface =
            base_surface + shore_lap_height(world_xz, signed_depth, time - age);
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
            // Same signed-depth definition the water mesh bakes per vertex
            // (WATER_DEPTH_FADE_METERS = 2.5), so both shaders phase their
            // waves off the identical field.
            let signed_depth = clamp((water_level - h) / 2.5, -1.0, 1.0);
            let wet = recent_shore_wetness(
                pbr_input.world_position.xz,
                h,
                water_level + water_params.w,
                signed_depth,
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

        // Cloud shadows: sample the shared cloud field at the point where the
        // sun ray through this fragment crosses the cloud layer, so shade
        // tracks the visible clouds and shifts with sun angle. Applied before
        // the fog pass so aerial haze lays over the darkened ground. The
        // shadow band is wider/softer than the cloud alpha band (penumbra).
        // Strength (clouds_b.z) is 0 when clouds are disabled or the sky is
        // clear, making shade exactly 1.0.
        let cloud_shadow_xz = pbr_input.world_position.xz
            - (CLOUD_LAYER_HEIGHT - pbr_input.world_position.y) * palette.clouds_b.xy;
        let cloud_shade = 1.0
            - palette.clouds_b.z
                * smoothstep(
                    0.22,
                    0.62,
                    cloud_density(cloud_shadow_xz, palette.clouds_a, palette.clouds_b.w),
                );
        out.color = vec4<f32>(out.color.rgb * cloud_shade, out.color.a);
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
