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
    // x: foam scale, y/z: shore-distance swell fade, w: near-shore multiplier
    ring_params: vec4<f32>,
    // x: max swell amplitude, y/z: depth fade, w: authoritative clock offset
    wave_params: vec4<f32>,
    // xyz: direction to the sun (world), w: glint strength (0 at night)
    sun_params: vec4<f32>,
    // Interaction ripples: xy = world xz, z = spawn time, w = strength
    // (0 = slot empty). Rings expand + fade entirely in-shader.
    ripples: array<vec4<f32>, 8>,
    // x: cloud coverage, y: inv world scale, zw: wind offset (world units).
    clouds_a: vec4<f32>,
    // xy: sun projection (sun_dir.xz / sun_dir.y), z: shadow strength, w: seed phase.
    clouds_b: vec4<f32>,
    // x: anchor time (client seconds), z: wind drift speed (client-time
    // units), yw: sun-projection velocity — both extrapolated in-shader.
    clouds_c: vec4<f32>,
    // Reserved (water skips snow); mirrors the terrain palette lane.
    climate: vec4<f32>,
    // xy: storm center at the wind anchor, z: storminess, w: reserved.
    storm: vec4<f32>,
};

@group(3) @binding(0) var<uniform> material: ToonWaterUniform;

const TAU: f32 = 6.28318530718;
const OCEAN_LOOP_SECONDS: f32 = 120.0;
const WATER_DEPTH_FADE_METERS: f32 = 2.5;
const WATER_SURFACE_OFFSET: f32 = 0.02;

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

// === Storm cells (EXACT copy in terrain_splat.wgsl / toon_water.wgsl / cloud_layer.wgsl — keep in sync) ===
// ONE storm system per map: a ~2km ragged disc around a drifting center
// (storm.xy = center at the wind anchor, storm.z = storminess). The caller
// extrapolates the center with the cloud drift so motion is frame-smooth.
// Radii must match STORM_EDGE_RADIUS in clouds.rs.
fn storm_cell(world_xz: vec2<f32>, center: vec2<f32>, storminess: f32) -> f32 {
    if (storminess < 0.01) {
        return 0.0;
    }
    let rel = world_xz - center;
    // Ragged edge: the disc radius wobbles with the cloud fbm so the squall
    // front reads as weather, not a stamped circle.
    let rag = cloud_fbm(rel * (1.0 / 700.0));
    let d = length(rel) * (0.80 + 0.45 * rag);
    return smoothstep(1300.0, 520.0, d) * storminess;
}

fn swell_field(world_xz: vec2<f32>, time: f32) -> f32 {
    let dir_a = vec2<f32>(0.8944272, 0.4472136);
    let dir_b = vec2<f32>(-0.3939193, 0.9191450);
    let dir_c = vec2<f32>(0.1961161, -0.9805807);
    let base_omega = TAU / OCEAN_LOOP_SECONDS;
    let phase_a = dot(world_xz, dir_a) * (TAU / 42.0) + time * base_omega * 9.0;
    let phase_b = dot(world_xz, dir_b) * (TAU / 24.0) - time * base_omega * 14.0;
    let phase_c = dot(world_xz, dir_c) * (TAU / 13.0) + time * base_omega * 21.0;
    return sin(phase_a) * 0.58 + sin(phase_b) * 0.29 + sin(phase_c) * 0.13;
}

fn swell_gradient(world_xz: vec2<f32>, time: f32) -> vec2<f32> {
    let dir_a = vec2<f32>(0.8944272, 0.4472136);
    let dir_b = vec2<f32>(-0.3939193, 0.9191450);
    let dir_c = vec2<f32>(0.1961161, -0.9805807);
    let k_a = TAU / 42.0;
    let k_b = TAU / 24.0;
    let k_c = TAU / 13.0;
    let base_omega = TAU / OCEAN_LOOP_SECONDS;
    let phase_a = dot(world_xz, dir_a) * k_a + time * base_omega * 9.0;
    let phase_b = dot(world_xz, dir_b) * k_b - time * base_omega * 14.0;
    let phase_c = dot(world_xz, dir_c) * k_c + time * base_omega * 21.0;
    return dir_a * cos(phase_a) * k_a * 0.58
        + dir_b * cos(phase_b) * k_b * 0.29
        + dir_c * cos(phase_c) * k_c * 0.13;
}

fn swell_motion_scale(depth: f32, shore_dist: f32) -> f32 {
    let depth_scale = smoothstep(material.wave_params.y, material.wave_params.z, depth);
    let shore_scale = smoothstep(material.ring_params.y, material.ring_params.z, shore_dist);
    return depth_scale * mix(material.ring_params.w, 1.0, shore_scale);
}

fn shore_lap_height(world_xz: vec2<f32>, shore_dist: f32, signed_depth: f32, time: f32) -> f32 {
    // Shore distance is only a fade mask; the PHASE rides the terrain depth
    // under the surface. Depth contours run parallel to the coastline, so
    // constant-phase crests form shore-parallel fronts that march toward
    // land as time advances — whatever direction the beach faces. Shore
    // distance itself must stay out of the phase (its nearest-point jumps
    // sawtooth); bilinearly sampled terrain depth is smooth.
    let shore_zone = 1.0 - smoothstep(0.10, 0.80, shore_dist);
    // Two incommensurate along-shore wobble octaves (~48m and ~120m) push
    // neighbouring stretches of beach out of phase, so arrivals ripple down
    // the coast instead of the whole shoreline surging in lockstep.
    let wob_a = sin(dot(world_xz, vec2<f32>(0.11, 0.073)) + time * 0.26);
    let wob_b = sin(dot(world_xz, vec2<f32>(-0.031, 0.042)) + time * 0.17 + 2.1);
    let wobble = wob_a * 1.05 + wob_b * 1.35;
    // +time moves constant-phase crests toward smaller depth: shoreward.
    let primary_phase = signed_depth * (TAU * 1.55) + time * (TAU / 12.0) + wobble;
    // Second harmonic skews the waveform: fast run-up, lingering retreat.
    let primary = sin(primary_phase) + 0.25 * sin(primary_phase * 2.0);
    let secondary_phase = signed_depth * (TAU * 3.05) + time * (TAU / 7.5) + 1.7 + wobble * 0.6;
    // Shoaling: crests grow as the water thins, like real arriving waves.
    let shoaling = 1.0 + (1.0 - clamp(signed_depth, 0.0, 1.0)) * 0.45;
    return (primary * 0.085 + sin(secondary_phase) * 0.024) * shoaling * shore_zone;
}

fn wave_height(world_xz: vec2<f32>, depth: f32, shore_dist: f32, signed_depth: f32, time: f32) -> f32 {
    let broad = swell_field(world_xz, time)
        * material.wave_params.x
        * swell_motion_scale(depth, shore_dist);
    return broad + shore_lap_height(world_xz, shore_dist, signed_depth, time);
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
    let shore_dist = clamp(vertex.color.g, 0.0, 1.0);
    let signed_depth = vertex.color.b;
#else
    let depth = 1.0;
    let shore_dist = 1.0;
    let signed_depth = 1.0;
#endif
    let wave_time = globals.time + material.wave_params.w;
    world_pos.y += wave_height(world_pos.xz, depth, shore_dist, signed_depth, wave_time);
#ifdef VERTEX_NORMALS
    let swell_slope = swell_gradient(world_pos.xz, wave_time)
        * material.wave_params.x
        * swell_motion_scale(depth, shore_dist);
    out.world_normal = normalize(vec3<f32>(-swell_slope.x, 1.0, -swell_slope.y));
#endif
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
    // Signed base-water depth, including the hidden strip beneath the bank.
    // This lets contact foam follow the moving water/terrain intersection.
    let signed_depth = in.color.b;
    let wave_time = globals.time + material.wave_params.w;
    let swell_value = swell_field(in.world_position.xz, wave_time);
    let surface_displacement = swell_value
        * material.wave_params.x
        * swell_motion_scale(depth, shore_dist)
        + shore_lap_height(in.world_position.xz, shore_dist, signed_depth, wave_time);

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
    let detail_wave = wave_field(flow_uv, t * 0.8, 6.2831, 1.0);
    let crest_wave = swell_value * 0.72 + detail_wave * 0.28;
    let crest01 = crest_wave * 0.5 + 0.5;
    let edge_width = clamp(material.foam_params.x, 0.001, 0.49);
    let edge_smooth = max(material.foam_params.y, 0.0005);
    let crest_threshold = 1.0 - edge_width;
    let crest_foam = smoothstep(crest_threshold - edge_smooth, crest_threshold + edge_smooth, crest01)
        * open_water;

    // --- Shoreline (BotW-style): a solid contact line at the waterline,
    // crisp thin foam lines traveling in toward the coast, and a soft wash
    // underneath. All in shoreline space, wobbled so lines undulate along
    // the coast instead of tracing perfect contours.
    let line_wobble = wave_field(in.world_position.xz * 0.22, globals.time * 1.4, 1.0, 1.0);
    let shore_zone = 1.0 - smoothstep(0.0, 0.5, shore_dist);

    // Contact line follows the animated surface against signed terrain depth,
    // so each crest visibly advances up the bank and each trough retreats.
    let shore_submersion = signed_depth * WATER_DEPTH_FADE_METERS
        + WATER_SURFACE_OFFSET
        + surface_displacement;
    // Preserve the authored world-space softness up close, but widen it to at
    // least a pixel at distance. Without derivative AA the otherwise smooth
    // contour stair-steps as it crosses the screen's pixel grid.
    let contact_half_width = min(max(0.056, fwidth(shore_submersion) * 0.75), 0.10);
    let contact = (1.0 - smoothstep(
        max(0.074 - contact_half_width, 0.0),
        0.074 + contact_half_width,
        abs(shore_submersion),
    ))
        * (1.0 - smoothstep(0.30, 0.62, shore_dist));

    // Traveling foam lines use true horizontal distance from the connected,
    // smoothed shoreline. Vertical depth inherits the terrain grid and turns
    // diagonal beaches into serrated two-axis contours; Euclidean shore
    // distance gives every bank orientation the same shore-normal motion.
    // 6.8 cycles across the normalized 28m field preserves the old ~4m line
    // spacing in the visible near-shore half of that field.
    let band_phase = shore_dist * 6.8 + line_wobble * 0.05 + globals.time * 0.14;
    let band = fract(band_phase);
    // Same line profile as before when nearby (0.34..0.46 and 0.54..0.66),
    // with a screen-space floor that prevents thin distant sections breaking
    // into a jagged dotted staircase.
    let band_half_width = min(max(0.06, fwidth(band_phase) * 0.75), 0.12);
    let line_core = smoothstep(0.40 - band_half_width, 0.40 + band_half_width, band)
        * (1.0 - smoothstep(0.60 - band_half_width, 0.60 + band_half_width, band));
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
#ifdef VERTEX_NORMALS
    let broad_normal = normalize(in.world_normal);
    let safe_normal_y = max(abs(broad_normal.y), 0.001);
    let swell_slope = vec2<f32>(-broad_normal.x, -broad_normal.z) / safe_normal_y;
#else
    let swell_slope = swell_gradient(in.world_position.xz, wave_time)
        * material.wave_params.x
        * swell_motion_scale(depth, shore_dist);
    let broad_normal = normalize(vec3<f32>(-swell_slope.x, 1.0, -swell_slope.y));
#endif
    let ndv = clamp(abs(dot(broad_normal, view_vec)), 0.02, 1.0);
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
    var grad = swell_slope;
    grad += wave_gradient(in.world_position.xz * 0.9, rip_t, 2.4, 1.1) * 0.30;
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

    // Cloud shadows: same sun-projected field the terrain samples, so shade
    // bands cross the waterline without a seam. Multiplied after the glint
    // add so sparkle dies under cloud cover too. Strength (clouds_b.z) is 0
    // when clouds are disabled or the sky is clear, making shade exactly 1.0.
    // Extrapolate BOTH motion sources past the anchor so shadows are
    // frame-smooth between the ~1/sec uniform refreshes: wind drift AND the
    // sun-projection sweep (the sun arcs fast on a 20-min day).
    let cloud_dt = globals.time - material.clouds_c.x;
    let sun_proj = material.clouds_b.xy
        + vec2<f32>(material.clouds_c.y, material.clouds_c.w) * cloud_dt;
    let cloud_shadow_xz = in.world_position.xz
        - (CLOUD_LAYER_HEIGHT - in.world_position.y) * sun_proj;
    let cloud_drift = material.clouds_c.z * cloud_dt;
    let cloud_params = vec4<f32>(
        material.clouds_a.xy,
        material.clouds_a.zw + vec2<f32>(0.8206, 0.5715) * cloud_drift,
    );
    let storminess = material.storm.z;
    let storm_center = material.storm.xy + vec2<f32>(0.8206, 0.5715) * (0.55 * cloud_drift);
    let storm_at_cloud = storm_cell(cloud_shadow_xz, storm_center, storminess);
    let storm_params = vec4<f32>(
        min(cloud_params.x + storm_at_cloud * 0.9, 1.0),
        cloud_params.yzw,
    );
    var cloud_shade = 1.0;
    // Uniform gate: skip the 16-noise-unit density field whenever the
    // shadow strength is zero (night, clear sky, clouds disabled).
    if (material.clouds_b.z > 0.0005) {
        cloud_shade = 1.0
            - material.clouds_b.z
                * smoothstep(
                    0.22,
                    0.62,
                    cloud_density(cloud_shadow_xz, storm_params, material.clouds_b.w),
                );
    }
    // Uniform branch: calm frames skip the storm-cell evaluation entirely.
    if (storminess >= 0.01) {
        cloud_shade *= 1.0 - storm_cell(in.world_position.xz, storm_center, storminess) * 0.30;
    }
    // Night: the water is unlit-custom, so scene lights can't darken it —
    // derive night from the synced sun elevation instead and pull toward a
    // dark blue of itself (matching the moonlit land).
    let day_w = smoothstep(-0.08, 0.12, material.sun_params.y);
    color_rgb = mix(color_rgb * vec3<f32>(0.20, 0.26, 0.45), color_rgb, day_w);
    color_rgb *= cloud_shade;

    return vec4<f32>(color_rgb, alpha);
}
