#define_import_path toon_water

#import bevy_pbr::{
    mesh_bindings::mesh,
    mesh_functions,
    mesh_view_bindings::view,
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
    // xyz: direction to the sun (world), w: glint strength (0 at night)
    sun_params: vec4<f32>,
    // Interaction ripples: xy = world xz, z = spawn time, w = strength
    // (0 = slot empty). Rings expand + fade entirely in-shader.
    ripples: array<vec4<f32>, 8>,
};

@group(3) @binding(0) var<uniform> material: ToonWaterUniform;

fn wave_field(p: vec2<f32>, time: f32, freq: f32, speed: f32) -> f32 {
    let dir_a = normalize(vec2<f32>(0.80, 0.60));
    let dir_b = normalize(vec2<f32>(-0.35, 0.94));
    let a = sin(dot(p, dir_a) * freq + time * speed);
    let b = sin(dot(p, dir_b) * (freq * 1.37) - time * (speed * 0.83));
    return a * 0.62 + b * 0.38;
}

// Analytic gradient of wave_field: gives a rippled surface normal without
// texture lookups or finite differences.
fn wave_gradient(p: vec2<f32>, time: f32, freq: f32, speed: f32) -> vec2<f32> {
    let dir_a = normalize(vec2<f32>(0.80, 0.60));
    let dir_b = normalize(vec2<f32>(-0.35, 0.94));
    let ca = cos(dot(p, dir_a) * freq + time * speed) * freq;
    let cb = cos(dot(p, dir_b) * (freq * 1.37) - time * (speed * 0.83)) * (freq * 1.37);
    return dir_a * ca * 0.62 + dir_b * cb * 0.38;
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

    // Nearly flat at the shoreline: shallow water shouldn't heave, and the
    // shore foam lines read best on calm geometry.
    let depth_scale = mix(0.05, 1.0, smoothstep(depth_start, 1.0, depth));
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
    let shore_flat = smoothstep(0.08, 0.5, vertex.color.g);
#else
    let depth = 1.0;
    let shore_flat = 1.0;
#endif
    world_pos.y += wave_height(world_pos.xz, depth, globals.time) * mix(0.1, 1.0, shore_flat);
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
    // Horizontal distance to the nearest shoreline, 0 at the waterline and
    // 1 at ~28m out (baked by the mesh builder). Depth alone can't zone
    // features on steep banks — deep water starts right at the coast.
    let shore_dist = clamp(in.color.g, 0.0, 1.0);

    // Soft-banded depth gradient: quantize a third of the way toward 3 bands
    // for the stylized "painted shelves of color" read.
    let depth_banded = mix(depth, floor(depth * 3.0 + 0.5) / 3.0, 0.35);
    let base = mix(material.shallow_color, material.deep_color, depth_banded);

    let wave_scale = max(material.ring_params.x, 0.001);
    let flow_speed = material.foam_params.w;
    let t = globals.time * flow_speed;

    // Use world-space UVs so foam patterns look coherent across chunk boundaries.
    let world_uv = in.world_position.xz * (0.05 * wave_scale);
    let flow_uv = world_uv + vec2<f32>(t * 0.35, -t * 0.22);

    // Crest foam bands — an OPEN-WATER feature, gated by DISTANCE to the
    // coast: foam lines own the shore band, then a calm gap, and crests
    // only fade in ~10-20m out regardless of how steep the bank is.
    let open_water = smoothstep(0.35, 0.75, shore_dist) * smoothstep(0.15, 0.4, depth);
    let wave = wave_field(flow_uv, t * 0.8, 6.2831, 1.0);
    let crest01 = wave * 0.5 + 0.5;
    let edge_width = clamp(material.foam_params.x, 0.001, 0.49);
    let edge_smooth = max(material.foam_params.y, 0.0005);
    let crest_threshold = 1.0 - edge_width;
    let crest_foam = smoothstep(crest_threshold - edge_smooth, crest_threshold + edge_smooth, crest01)
        * open_water;

    // --- Shoreline (BotW-style): a solid contact line at the waterline,
    // crisp thin foam lines traveling in toward the coast, and a soft wash
    // underneath. All in shore-depth space, wobbled so lines undulate along
    // the coast instead of tracing perfect contours.
    let line_wobble = wave_field(in.world_position.xz * 0.22, globals.time * 1.4, 1.0, 1.0);
    let shore_zone = 1.0 - smoothstep(0.0, 0.5, shore_dist);

    // Contact line: near-opaque white right where water meets land.
    let contact = 1.0 - smoothstep(0.015, 0.055 + 0.015 * line_wobble, depth);

    // Traveling foam lines: thin bands in shore-DISTANCE space (evenly
    // spaced ~6m apart even on steep banks), drifting shoreward and
    // breaking up in slow patches like arriving wavelets.
    let band = fract(shore_dist * 4.5 + line_wobble * 0.05 + globals.time * 0.16);
    let line_core = smoothstep(0.34, 0.46, band) * (1.0 - smoothstep(0.54, 0.66, band));
    let breakup = smoothstep(
        0.2,
        0.8,
        0.5 + 0.5 * sin(dot(in.world_position.xz, vec2<f32>(0.13, 0.11)) + globals.time * 0.7 + line_wobble * 1.7)
    );
    let travel_lines = line_core * breakup * shore_zone * shore_zone;

    // Soft wash under the lines, kept close to the waterline. The border is
    // wobbled by the SMOOTH wave field — the old floor()+time hash re-rolled
    // a new random value whenever the scrolling cell grid crossed a
    // boundary, which made the fade edge flicker.
    let wash_border = line_wobble * 0.035;
    let wash =
        (1.0 - smoothstep(0.03 + wash_border, 0.16 + wash_border * 1.6, shore_dist)) * 0.55;

    let shore_foam = clamp(contact + travel_lines * 0.95 + wash, 0.0, 1.0);

    // Interaction ripples: expanding foam rings around wading players.
    const RIPPLE_LIFE: f32 = 1.5;
    var ripple_foam = 0.0;
    for (var i = 0u; i < 8u; i++) {
        let ripple = material.ripples[i];
        let age = globals.time - ripple.z;
        if (ripple.w > 0.001 && age > 0.0 && age < RIPPLE_LIFE) {
            let radius = 0.3 + age * 1.6;
            let ring_dist = abs(distance(in.world_position.xz, ripple.xy) - radius);
            let ring = 1.0 - smoothstep(0.05, 0.3, ring_dist);
            let fade = 1.0 - age / RIPPLE_LIFE;
            ripple_foam += ring * fade * fade * ripple.w;
        }
    }

    // Sparkle comes from the sun-glint pass alone — a drifting dot pattern
    // reads as a texture sliding over the surface, worst at the shore edge.
    let foam_mask = clamp(
        shore_foam + crest_foam * 0.25 + ripple_foam * 0.85,
        0.0,
        1.0
    );

    // Crest shading modulation also calms down in the shallows.
    let toon_light = 0.86 + crest01 * 0.14 * mix(0.35, 1.0, open_water);
    var base_rgb = clamp(base.rgb * toon_light, vec3<f32>(0.0), vec3<f32>(1.0));

    // Fresnel: grazing views pick up a pale sky tint and turn more opaque;
    // looking straight down stays clear. Sells the surface as reflective
    // without any actual reflection rendering.
    let view_vec = normalize(view.world_position.xyz - in.world_position.xyz);
    let ndv = clamp(abs(view_vec.y), 0.02, 1.0);
    let fresnel = pow(1.0 - ndv, 3.0);
    // A proper sky BLUE, weakly mixed — a strong whitish tint washes the
    // whole lake milky at grazing angles.
    let sky_tint = vec3<f32>(0.45, 0.68, 0.90);
    base_rgb = mix(base_rgb, sky_tint, fresnel * 0.35);
    var alpha = clamp(base.a + fresnel * 0.22, 0.0, 0.97);

    var color_rgb = mix(base_rgb, material.foam_color.rgb, foam_mask);

    // Sun glints: sparse twinkling sparkle along the sun path.
    // - three decorrelated gradient octaves (two aligned octaves make the
    //   highlights march in lattice rows toward the sun);
    // - a static spatial hash breaks the specular field into glitter points;
    // - distance fade, because a sub-pixel ripple field aliases into huge
    //   slow moire blobs far away.
    let rip_t = globals.time;
    var grad = wave_gradient(in.world_position.xz * 0.9, rip_t, 2.4, 1.1) * 0.30;
    grad += wave_gradient(in.world_position.xz * 2.7 + vec2<f32>(13.7, 71.3), rip_t * 1.35, 3.1, 1.7) * 0.16;
    let swizzled = vec2<f32>(-in.world_position.z, in.world_position.x) * 1.8 + vec2<f32>(51.0, 8.0);
    let grad_c = wave_gradient(swizzled, rip_t * 0.8, 2.9, 1.4) * 0.14;
    grad += vec2<f32>(grad_c.y, -grad_c.x);
    let ripple_normal = normalize(vec3<f32>(-grad.x, 1.0, -grad.y));
    let sun_dir = normalize(material.sun_params.xyz);
    let sparkle = 0.25 + 0.75 * hash12(floor(in.world_position.xz * 1.7));
    let view_dist = distance(view.world_position.xyz, in.world_position.xyz);
    let dist_fade = mix(0.2, 1.0, 1.0 - smoothstep(50.0, 150.0, view_dist));
    let glint = min(
        pow(max(dot(reflect(-view_vec, ripple_normal), sun_dir), 0.0), 130.0)
            * material.sun_params.w * sparkle * dist_fade,
        1.4
    );
    color_rgb += glint * vec3<f32>(1.0, 0.97, 0.88);
    // Foam pushes toward opaque so the white shore lines read solid instead
    // of washing out over the seabed.
    alpha = clamp(alpha + min(glint, 1.0) * 0.25 + foam_mask * 0.5, 0.0, 1.0);

    return vec4<f32>(color_rgb, alpha);
}
