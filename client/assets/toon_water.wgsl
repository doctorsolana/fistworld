#define_import_path toon_water

#import bevy_pbr::{
    mesh_bindings::mesh,
    mesh_functions,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}
#import bevy_render::globals::Globals

@group(0) @binding(11) var<uniform> globals: Globals;

struct ToonWaterUniform {
    shallow_color: vec4<f32>,
    deep_color: vec4<f32>,
    foam_color: vec4<f32>,
    // x: foam edge width, y: foam smoothness, z: cell scale, w: flow speed
    foam_params: vec4<f32>,
    // x: wave scale, y: shore min depth, z: shore max depth, w: shore noise amount
    ring_params: vec4<f32>,
    // x: wave amplitude, y: wave frequency, z: wave speed, w: depth start
    wave_params: vec4<f32>,
};

@group(3) @binding(0) var<uniform> material: ToonWaterUniform;

fn wave_field(p: vec2<f32>, time: f32, freq: f32, speed: f32) -> f32 {
    let dir_a = normalize(vec2<f32>(0.80, 0.60));
    let dir_b = normalize(vec2<f32>(-0.35, 0.94));
    let a = sin(dot(p, dir_a) * freq + time * speed);
    let b = sin(dot(p, dir_b) * (freq * 1.37) - time * (speed * 0.83));
    return a * 0.62 + b * 0.38;
}

fn hash12(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453);
}

fn wave_height(world_xz: vec2<f32>, depth: f32, time: f32) -> f32 {
    let amp = material.wave_params.x;
    let freq = material.wave_params.y;
    let speed = material.wave_params.z;
    let depth_start = material.wave_params.w;

    let depth_scale = mix(0.2, 1.0, smoothstep(depth_start, 1.0, depth));
    let wave = wave_field(world_xz, time, freq, speed);
    return wave * amp * depth_scale;
}

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

#ifdef VERTEX_POSITIONS
    var world_pos = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0)
    );

#ifdef VERTEX_COLORS
    let depth = clamp(vertex.color.a, 0.0, 1.0);
#else
    let depth = 1.0;
#endif
    world_pos.y += wave_height(world_pos.xz, depth, globals.time);
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
    out.color = vertex.color;
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

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let depth = clamp(in.color.a, 0.0, 1.0);
    let base = mix(material.shallow_color, material.deep_color, depth);

    let wave_scale = max(material.ring_params.x, 0.001);
    let flow_speed = material.foam_params.w;
    let t = globals.time * flow_speed;

    // Use world-space UVs so foam patterns look coherent across chunk boundaries.
    let world_uv = in.world_position.xz * (0.05 * wave_scale);
    let flow_uv = world_uv + vec2<f32>(t * 0.35, -t * 0.22);

    // Crest foam bands.
    let wave = wave_field(flow_uv, t * 0.8, 6.2831, 1.0);
    let crest01 = wave * 0.5 + 0.5;
    let edge_width = clamp(material.foam_params.x, 0.001, 0.49);
    let edge_smooth = max(material.foam_params.y, 0.0005);
    let crest_threshold = 1.0 - edge_width;
    let crest_foam = smoothstep(crest_threshold - edge_smooth, crest_threshold + edge_smooth, crest01);

    // Shore foam with noisy border to avoid perfectly smooth contour lines.
    let shore_min = material.ring_params.y;
    let shore_max = max(material.ring_params.z, shore_min + 0.001);
    let shore_noise = (hash12(floor(world_uv * 5.0) + vec2<f32>(t * 0.4, -t * 0.3)) - 0.5)
        * (material.ring_params.w * 2.0);
    let shore_mask = 1.0 - smoothstep(shore_min + shore_noise, shore_max + shore_noise, depth);
    let shore_wave = 0.75 + 0.25 * sin(dot(world_uv, vec2<f32>(3.1, 2.4)) * 6.2831 + t * 1.2);
    let shore_foam = shore_mask * shore_wave;

    // Small white flecks drifting over water.
    let fleck_scale = max(material.foam_params.z, 0.25);
    let fleck_cell = floor((world_uv + vec2<f32>(t * 0.18, -t * 0.14)) * (12.0 * fleck_scale));
    let fleck_rand = hash12(fleck_cell);
    let fleck_seed = hash12(fleck_cell + vec2<f32>(17.0, 59.0));
    let fleck_twinkle = 0.55 + 0.45 * sin(t * 6.0 + fleck_seed * 6.2831);
    let fleck_mask = smoothstep(0.965, 0.996, fleck_rand) * fleck_twinkle;
    let fleck_depth = smoothstep(0.25, 0.9, depth);

    let foam_mask = clamp(
        shore_foam * 0.95 + crest_foam * (1.0 - depth) * 0.45 + fleck_mask * fleck_depth * 0.45,
        0.0,
        1.0
    );

    let toon_light = 0.86 + crest01 * 0.14;
    let base_rgb = clamp(base.rgb * toon_light, vec3<f32>(0.0), vec3<f32>(1.0));
    let color = mix(vec4<f32>(base_rgb, base.a), material.foam_color, foam_mask);
    return vec4<f32>(color.rgb, base.a);
}
