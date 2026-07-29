// Wind-swayed foliage: StandardMaterial extension that displaces vertices in
// the vertex stage. Fragment shading is inherited from StandardMaterial.
//
// Sway is strongest at the top of the mesh (quadratic falloff to the base so
// trunks stay planted). Motion is the sum of:
//  - traveling gust fronts: long sine bands sweeping the world along the
//    prevailing wind direction, so whole fields ripple in visible waves
//    (the Breath of the Wild look) instead of wobbling independently;
//  - local flutter with per-plant phase from world position;
//  - a constant downwind lean so gusts push rather than oscillate around
//    the rest pose.
// A passing gust also brightens vertex colors toward the tips — the silvery
// sheen that makes the wave readable at a distance.

#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    view_transformations::position_world_to_clip,
}

// Custom fragments must re-apply the LOD crossfade dither StandardMaterial's
// own fragment would have run — without it, foliage pops at range ends and
// double-draws during LOD0/LOD1 crossfade.
#ifdef VISIBILITY_RANGE_DITHER
#import bevy_pbr::pbr_functions::visibility_range_dither;
#endif
#import bevy_render::globals::Globals

@group(0) @binding(11) var<uniform> globals: Globals;

struct WindParams {
    // x: sway strength (meters at the tip)
    // y: wind speed (time multiplier)
    // z: sway_min_y (mesh-local Y where sway begins)
    // w: 1 / sway range (mesh-local)
    params: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> wind: WindParams;
// x: per-instance height jitter fraction, y: height stretch,
// z: map half extent (m), w: climate seed phase.
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> wind_extra: vec4<f32>;

@vertex
fn vertex(vertex_no_morph: Vertex) -> VertexOutput {
    var out: VertexOutput;
    var vertex = vertex_no_morph;

    let mesh_world_from_local = mesh_functions::get_world_from_local(vertex_no_morph.instance_index);
    var world_from_local = mesh_world_from_local;

#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex_no_morph.instance_index
    );
#endif

    var gust_sheen = 0.0;
    var tip = 0.0;

#ifdef VERTEX_POSITIONS
    // Height factor: 0 at the trunk base, 1 at the canopy tip (from the
    // ORIGINAL mesh Y, before the jitter stretch below).
    let h = clamp((vertex.position.y - wind.params.z) * wind.params.w, 0.0, 1.0);
    let bend = h * h;
    tip = h;

    // Height shaping: a base stretch (grass grows taller + slimmer,
    // BotW-style) times a per-instance jitter hashed from the instance
    // origin so identical tufts grow to different heights. Identity for
    // trees/bushes (x = 0, y = 1).
    let origin_xz = mesh_world_from_local[3].xz;
    let hjit = fract(sin(dot(origin_xz, vec2<f32>(127.1, 311.7))) * 43758.5453);
    vertex.position.y *= max(wind_extra.y, 0.001) * (1.0 + (hjit - 0.5) * 2.0 * wind_extra.x);

    var world_pos = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0)
    );

    // Prevailing wind direction; the multi-band gusting hides its constancy.
    let dir = vec2<f32>(0.8206, 0.5715);
    let side = vec2<f32>(-dir.y, dir.x);
    let t = globals.time * wind.params.y;

    // Traveling gust fronts: two wave bands (~75m and ~260m wavelength)
    // sweeping downwind. The front is bowed by a slow cross-wind phase
    // wobble (curved tongues instead of infinite straight bars), sharpened
    // into a narrow rolling crest, and modulated by large-scale patchiness
    // so gusts swell and die out across the field.
    let along = dot(world_pos.xz, dir);
    let across = dot(world_pos.xz, side);
    let front_curve = 14.0 * sin(across * 0.045 + t * 0.22)
        + 22.0 * sin(across * 0.013 - t * 0.11);
    let wave_coord = along + front_curve;
    let band = 0.6 * sin(wave_coord * 0.085 - t * 1.35)
        + 0.4 * sin(wave_coord * 0.024 - t * 0.53 + 1.7);
    // Patchiness floor at 0.4 — gusts vary in strength but never vanish.
    let patchiness = 0.7 + 0.3 * sin(across * 0.03 + along * 0.017 - t * 0.34);
    let front = smoothstep(0.15, 0.9, band) * patchiness;
    let gust = front * front;
    gust_sheen = gust;

    // Local flutter with per-plant phase so neighbors desynchronize.
    let phase = world_pos.x * 0.37 + world_pos.z * 0.43;
    let flutter = 0.45 * sin(t * 2.0 + phase) + 0.25 * sin(t * 3.7 + phase * 1.7);

    // Downwind push (lean + gust + flutter) with a touch of sideways wobble.
    let push = (0.22 + 2.2 * gust + 0.6 * flutter * (0.5 + 0.5 * gust))
        * wind.params.x * bend;
    let wobble = flutter * 0.3 * wind.params.x * bend;
    world_pos.x += push * dir.x + wobble * side.x;
    world_pos.z += push * dir.y + wobble * side.y;
    // Blades bend in an arc: as the tip shears sideways it also dips
    // (drop ≈ d²/2L). Without this, hard gusts look like the grass is
    // stretching instead of bending.
    world_pos.y -= push * push * 0.55;

    out.world_position = world_pos;
    out.position = position_world_to_clip(out.world_position.xyz);
#endif

#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif

#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        vertex_no_morph.instance_index
    );
#endif

#ifdef VERTEX_COLORS
    // Gust sheen: the passing wave brightens tips, making the front visible
    // as a silver ripple rolling across the field.
    out.color = vec4<f32>(
        vertex.color.rgb * (1.0 + gust_sheen * 0.30 * tip),
        vertex.color.a
    );
#endif

#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex_no_morph.instance_index;
#endif

#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_no_morph.instance_index, mesh_world_from_local[3]);
#endif

    return out;
}

// Fragment: StandardMaterial shading with a climate frost tint on the base
// color. Canopy color lives in the material texture (NOT vertex colors), so
// the tint must land here. Compact mirror of shared/worldgen.rs climate_at
// (wind_extra.z = half extent, .w = phase; constants in sync with
// terrain_splat.wgsl).
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
#ifdef VISIBILITY_RANGE_DITHER
    visibility_range_dither(in.position, in.visibility_range_dither);
#endif

    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let half = max(wind_extra.z, 1.0);
    let wobble = 0.045 * sin(in.world_position.x * 0.0039 + wind_extra.w)
        + 0.022 * sin(in.world_position.x * 0.0127 + wind_extra.w * 2.7);
    // Signed hemispheres: north (-z) freezes, south (+z) scorches.
    let north_lat = max(-in.world_position.z / half + wobble, 0.0);
    let south_lat = max(in.world_position.z / half + wobble, 0.0);
    let alt_push = min(max(in.world_position.y, 0.0) * 0.004, 0.30);
    let eff = north_lat + alt_push;
    let snow = smoothstep(0.68, 0.78, eff);
    let frost = smoothstep(0.58, 0.68, eff);
    let dry = smoothstep(0.35, 0.70, south_lat - alt_push) * (1.0 - frost);
    // Frost silvers the foliage; full snow dusts it toward white but keeps
    // enough of the base hue that species stay distinguishable.
    var rgb = pbr_input.material.base_color.rgb;
    let frost_tone = mix(rgb, vec3<f32>(0.72, 0.76, 0.82), 0.45);
    rgb = mix(rgb, frost_tone, frost * 0.7);
    rgb = mix(rgb, vec3<f32>(0.86, 0.90, 0.96), snow * 0.55);
    // Desert scorch: canopies dry toward olive-khaki scrub in the south.
    rgb = mix(rgb, vec3<f32>(0.60, 0.55, 0.32), dry * 0.55);
    pbr_input.material.base_color = vec4<f32>(rgb, pbr_input.material.base_color.a);

    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
