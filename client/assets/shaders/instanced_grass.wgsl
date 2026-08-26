// GPU-instanced 3D grass. Fragment shading is the same StandardMaterial PBR
// path as wind_foliage.wgsl; only the transform source differs.

#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    view_transformations::position_world_to_clip,
}
#import bevy_render::globals::Globals

@group(0) @binding(11) var<uniform> globals: Globals;

struct WindParams {
    params: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> wind: WindParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> wind_extra: vec4<f32>;

// The two authored grass meshes always provide POSITION, NORMAL, UV0, UV1 and
// COLOR0. Locations 8+ are reserved for the per-chunk instance buffer.
struct GrassVertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv_b: vec2<f32>,
    @location(5) color: vec4<f32>,
    // xyz: world root, w: stable height multiplier.
    @location(8) instance_position_height: vec4<f32>,
    // x/y: sin/cos yaw, z: uniform scale.
    @location(9) instance_rotation_scale: vec4<f32>,
};

fn rotate_y(value: vec3<f32>, sine: f32, cosine: f32) -> vec3<f32> {
    return vec3<f32>(
        value.x * cosine + value.z * sine,
        value.y,
        -value.x * sine + value.z * cosine,
    );
}

@vertex
fn vertex(vertex: GrassVertex) -> VertexOutput {
    var out: VertexOutput;
    let root = vertex.instance_position_height.xyz;
    let height = vertex.instance_position_height.w;
    let sine = vertex.instance_rotation_scale.x;
    let cosine = vertex.instance_rotation_scale.y;
    let scale = vertex.instance_rotation_scale.z;

    // Root-to-tip factor is computed from the untouched authored position.
    let h = clamp((vertex.position.y - wind.params.z) * wind.params.w, 0.0, 1.0);
    let bend = h * h;
    var shaped = vertex.position;
    shaped.y *= height;
    var world_pos = root + rotate_y(shaped * scale, sine, cosine);

    let dir = vec2<f32>(0.8206, 0.5715);
    let side = vec2<f32>(-dir.y, dir.x);
    let t = globals.time * 1.15;
    let ft = globals.time * wind.params.y;
    let along = dot(world_pos.xz, dir);
    let across = dot(world_pos.xz, side);
    let front_curve = 14.0 * sin(across * 0.045 + t * 0.22)
        + 22.0 * sin(across * 0.013 - t * 0.11);
    let wave_coord = along + front_curve;
    let band = 0.6 * sin(wave_coord * 0.085 - t * 1.35)
        + 0.4 * sin(wave_coord * 0.024 - t * 0.53 + 1.7);
    let patchiness = 0.7 + 0.3 * sin(across * 0.03 + along * 0.017 - t * 0.34);
    let front = smoothstep(0.15, 0.9, band) * patchiness;
    let gust = front * front;
    let phase = world_pos.x * 0.37 + world_pos.z * 0.43;
    let flutter = 0.45 * sin(ft * 2.0 + phase) + 0.25 * sin(ft * 3.7 + phase * 1.7);
    let push = (0.22 + 2.2 * gust + 0.6 * flutter * (0.5 + 0.5 * gust))
        * wind.params.x * bend;
    let wobble = flutter * 0.3 * wind.params.x * bend;
    world_pos.x += push * dir.x + wobble * side.x;
    world_pos.z += push * dir.y + wobble * side.y;
    world_pos.y -= push * push * 0.55;

    out.world_position = vec4<f32>(world_pos, 1.0);
    out.position = position_world_to_clip(world_pos);
    out.world_normal = rotate_y(vertex.normal, sine, cosine);
#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef VERTEX_COLORS
    // Per-tuft dryness tint (instance .w lane): lush tufts sit a touch
    // deeper green, dry tufts go warm straw. Value-and-warmth only - the
    // same discipline as the terrain mottle - so the meadow yellows rather
    // than turning teal or orange.
    let dryness = clamp(vertex.instance_rotation_scale.w, 0.0, 1.0);
    let dry_tint = mix(
        vec3<f32>(0.90, 1.02, 0.88),
        vec3<f32>(1.18, 1.06, 0.74),
        dryness,
    );
    out.color = vec4<f32>(
        vertex.color.rgb * dry_tint * (1.0 + gust * 0.30 * h),
        vertex.color.a,
    );
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    // The source mesh has one Bevy mesh uniform. Grass transforms live in the
    // custom instance buffer, so every fragment deliberately refers to slot 0.
    out.instance_index = 0u;
#endif
    return out;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let half = max(wind_extra.z, 1.0);
    let wobble = 0.045 * sin(in.world_position.x * 0.0039 + wind_extra.w)
        + 0.022 * sin(in.world_position.x * 0.0127 + wind_extra.w * 2.7);
    let north_lat = max(-in.world_position.z / half + wobble, 0.0);
    let south_lat = max(in.world_position.z / half + wobble, 0.0);
    let alt_push = min(max(in.world_position.y, 0.0) * 0.002, 0.30);
    let eff = north_lat + alt_push;
    let snow = smoothstep(0.60, 0.76, eff);
    let frost = smoothstep(0.46, 0.62, eff);
    let dry = smoothstep(0.35, 0.70, south_lat - alt_push) * (1.0 - frost);
    var rgb = pbr_input.material.base_color.rgb;
    let frost_tone = mix(rgb, vec3<f32>(0.72, 0.76, 0.82), 0.45);
    rgb = mix(rgb, frost_tone, frost * 0.7);
    rgb = mix(rgb, vec3<f32>(0.86, 0.90, 0.96), snow * 0.55);
    rgb = mix(rgb, vec3<f32>(0.60, 0.55, 0.32), dry * 0.55);
    pbr_input.material.base_color = vec4<f32>(rgb, pbr_input.material.base_color.a);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
