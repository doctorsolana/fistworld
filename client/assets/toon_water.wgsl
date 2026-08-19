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
    // xyz: reserved (legacy crest-disc tuning), w: foam flow speed.
    foam_params: vec4<f32>,
    // x: foam scale, y/z: shore-distance swell fade, w: near-shore multiplier
    ring_params: vec4<f32>,
    // x: max swell amplitude, y/z: depth fade, w: authoritative clock offset
    wave_params: vec4<f32>,
    // xyz: direction to the sun (world), w: glint strength (0 at night)
    sun_params: vec4<f32>,
    // Wake/interaction foam: xy = world xz, z = spawn time, w = strength
    // (0 = slot empty). Rings expand + fade entirely in-shader; the wake
    // spawner drops stern breadcrumbs into these slots round-robin.
    ripples: array<vec4<f32>, 16>,
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
    // xy: clean per-pixel distance fade start/end. This replaces Bevy's
    // 4x4 visibility dither, which reads as a dark checkerboard on water.
    // zw: camera-distance fade for the foam family (washes/crests).
    distance_fade: vec4<f32>,
    // xy: playable min xz, zw: playable max xz. Only the visual map-edge
    // continuation uses this; ordinary water has signed depth <= 1.
    map_bounds: vec4<f32>,
    // xy: streamed detail centre; zw: square edge fade start/end.
    detail_bounds: vec4<f32>,
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
    if (rnd < 0.90) {
        return 0.0;
    }
    let jitter = vec2<f32>(glitter_hash(id + 17.0), glitter_hash(id + 41.0)) - 0.5;
    let local = fract(world_xz / cell) - 0.5 - jitter * 0.9;
    let d = length(local) * cell;
    let radius = cell * 0.12;
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
    // The dots are round in WORLD space; when the view tilts toward the
    // horizon the screen footprint stretches and rows of dots smear into a
    // dashed grid. Fade the glitter out as the footprint turns anisotropic
    // and let the soft analytic glint own grazing angles.
    let px = vec2<f32>(dpdx(world_xz.x), dpdx(world_xz.y));
    let py = vec2<f32>(dpdy(world_xz.x), dpdy(world_xz.y));
    let long_axis = max(length(px), length(py));
    let aniso = min(length(px), length(py)) / max(long_axis, 1.0e-6);
    return mix(g0, g1, fract(lod)) * smoothstep(0.35, 0.65, aniso);
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
// Keep the radius literals in all three shader copies identical.
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

// Stokes-style crest sharpening for the two dominant swells: sin(p)-e*cos(2p)
// narrows crests and widens troughs as a pure HEIGHT function (no horizontal
// displacement), so the CPU buoyancy mirror in shared/src/water.rs stays a
// trivial exact copy — keep sharpness constants in sync with
// WATER_SWELL_SHARPNESS_A/B there. Divided by (1+e) so |profile| <= 1.
const SWELL_SHARP_A: f32 = 0.24;
const SWELL_SHARP_B: f32 = 0.18;

fn sharp_sin(phase: f32, sharpness: f32) -> f32 {
    return (sin(phase) - sharpness * cos(2.0 * phase)) / (1.0 + sharpness);
}

// d/dphase of sharp_sin.
fn sharp_sin_slope(phase: f32, sharpness: f32) -> f32 {
    return (cos(phase) + 2.0 * sharpness * sin(2.0 * phase)) / (1.0 + sharpness);
}

fn swell_field(world_xz: vec2<f32>, time: f32) -> f32 {
    let dir_a = vec2<f32>(0.8944272, 0.4472136);
    let dir_b = vec2<f32>(-0.3939193, 0.9191450);
    let dir_c = vec2<f32>(0.1961161, -0.9805807);
    let base_omega = TAU / OCEAN_LOOP_SECONDS;
    let phase_a = dot(world_xz, dir_a) * (TAU / 64.0) + time * base_omega * 14.0;
    let phase_b = dot(world_xz, dir_b) * (TAU / 24.0) - time * base_omega * 31.0;
    let phase_c = dot(world_xz, dir_c) * (TAU / 13.0) + time * base_omega * 42.0;
    return sharp_sin(phase_a, SWELL_SHARP_A) * 0.52
        + sharp_sin(phase_b, SWELL_SHARP_B) * 0.31
        + sin(phase_c) * 0.17;
}

fn swell_gradient(world_xz: vec2<f32>, time: f32) -> vec2<f32> {
    let dir_a = vec2<f32>(0.8944272, 0.4472136);
    let dir_b = vec2<f32>(-0.3939193, 0.9191450);
    let dir_c = vec2<f32>(0.1961161, -0.9805807);
    let k_a = TAU / 64.0;
    let k_b = TAU / 24.0;
    let k_c = TAU / 13.0;
    let base_omega = TAU / OCEAN_LOOP_SECONDS;
    let phase_a = dot(world_xz, dir_a) * k_a + time * base_omega * 14.0;
    let phase_b = dot(world_xz, dir_b) * k_b - time * base_omega * 31.0;
    let phase_c = dot(world_xz, dir_c) * k_c + time * base_omega * 42.0;
    return dir_a * sharp_sin_slope(phase_a, SWELL_SHARP_A) * k_a * 0.52
        + dir_b * sharp_sin_slope(phase_b, SWELL_SHARP_B) * k_b * 0.31
        + dir_c * cos(phase_c) * k_c * 0.17;
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

fn wave_height(
    world_xz: vec2<f32>,
    depth: f32,
    shore_dist: f32,
    signed_depth: f32,
    ocean_factor: f32,
    time: f32,
) -> f32 {
    let broad = swell_field(world_xz, time)
        * material.wave_params.x
        * swell_motion_scale(depth, shore_dist);
    let river_wave = broad + shore_lap_height(world_xz, shore_dist, signed_depth, time);
    // The old depth-phased lap works well in narrow channels, but along a long
    // ocean coast its independent spatial phases make neighbouring pieces of
    // the water edge rise and fall against each other. Keep the ocean's final
    // few metres physically calm; its run-up is represented by coherent foam
    // contours below rather than by folding the mesh itself.
    let ocean_shore_fade = smoothstep(0.10, 0.42, shore_dist);
    let ocean_wave = broad * ocean_shore_fade;
    return mix(river_wave, ocean_wave, ocean_factor);
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
    let ocean_factor = clamp(vertex.color.r, 0.0, 1.0);
    let depth = clamp(vertex.color.a, 0.0, 1.0);
    let shore_dist = clamp(vertex.color.g, 0.0, 1.0);
    let signed_depth = vertex.color.b;
#else
    let ocean_factor = 0.0;
    let depth = 1.0;
    let shore_dist = 1.0;
    let signed_depth = 1.0;
#endif
    let wave_time = globals.time + material.wave_params.w;
    world_pos.y += wave_height(
        world_pos.xz,
        depth,
        shore_dist,
        signed_depth,
        ocean_factor,
        wave_time,
    );
#ifdef VERTEX_NORMALS
    let ocean_shore_fade = smoothstep(0.10, 0.42, shore_dist);
    let normal_motion_scale = mix(1.0, ocean_shore_fade, ocean_factor);
    let swell_slope = swell_gradient(world_pos.xz, wave_time)
        * material.wave_params.x
        * swell_motion_scale(depth, shore_dist)
        * normal_motion_scale;
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

    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // One camera-local square covers sides and corners without gaps. Its
    // private B > 1 vertex tag keeps it strictly outside gameplay terrain.
    let is_edge_extension = in.color.b > 1.5;
    if (is_edge_extension
        && in.world_position.x >= material.map_bounds.x
        && in.world_position.z >= material.map_bounds.y
        && in.world_position.x <= material.map_bounds.z
        && in.world_position.z <= material.map_bounds.w) {
        discard;
    }
    // R is an ocean/river blend baked by the mesh. It lets this pass fix the
    // long-coast folding without disturbing the river look.
    let ocean_factor = clamp(in.color.r, 0.0, 1.0);
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
    let surface_displacement = wave_height(
        in.world_position.xz,
        depth,
        shore_dist,
        signed_depth,
        ocean_factor,
        wave_time,
    );

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

    // Camera-distance response for the foam family. distance_fade.zw carries
    // the foam fade band (client/src/water/material.rs): washes and crests are
    // gone well before the 1250-1800m water crossfade begins, so the coarse
    // far-mesh shoreline never fights an aliased bright rim on the way out.
    let view_dist = distance(view.world_position.xyz, in.world_position.xyz);
    let foam_dist_fade =
        1.0 - smoothstep(material.distance_fade.z, material.distance_fade.w, view_dist);

    // Crest foam bands — an OPEN-WATER feature, gated by DISTANCE to the
    // coast: foam lines own the shore band, then a calm gap, and crests
    // only fade in ~10-20m out regardless of how steep the bank is.
    let open_water = smoothstep(0.35, 0.75, shore_dist) * smoothstep(0.15, 0.4, depth);
    let detail_wave = wave_field(flow_uv, t * 0.8, 6.2831, 1.0);
    let crest_wave = swell_value * 0.72 + detail_wave * 0.28;
    let crest01 = crest_wave * 0.5 + 0.5;

    // Organic churn foam, Valheim-style: the patches come from a
    // wind-drifting world-space noise field and are merely made LIKELIER by
    // high water and storms — foam locked to the analytic crests renders as
    // mechanical rows marching in step across the whole sea (tried; read as
    // a marching band, not an ocean).
    let wind_dir = vec2<f32>(0.8944272, 0.4472136);
    let wind_drift = wind_dir * (globals.time * 0.55);
    let patches = cloud_fbm((in.world_position.xz + wind_drift) * (1.0 / 17.0));
    let lace = cloud_fbm(
        (in.world_position.xz - wind_drift * 0.6) * (1.0 / 4.2) + vec2<f32>(37.0, -11.0),
    );
    // Waves and weather favor churn without dictating its shape.
    let crest_favor = 0.62 + 0.38 * crest01;
    let churn_cover = mix(0.70, 0.52, material.storm.z);
    let churn_core = smoothstep(churn_cover, churn_cover + 0.14, patches * crest_favor);
    // Small-scale lace tears each patch open so it reads as sea foam, not
    // spilled paint.
    let churn_lace = mix(0.30, 1.0, smoothstep(0.35, 0.68, lace));
    // Footprint kill: lace is a 2-4m feature; drop churn before it shimmers.
    let crest_resolve = 1.0
        - smoothstep(0.7, 1.8, fwidth(in.world_position.x) + fwidth(in.world_position.z));
    let churn_foam = churn_core * churn_lace * open_water * foam_dist_fade * crest_resolve;

    // --- Shoreline: rivers retain the authored depth-phased lap below. Ocean
    // foam instead uses one stable distance field for contact, traveling lines
    // and wash. Noise modulates opacity only, never the line position, so a
    // diagonal coast cannot fold back across itself while the wave advances.
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
    let river_contact = (1.0 - smoothstep(
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
    // Kill traveling lines as their period approaches the pixel grid. This is
    // energy-conserving, unlike the half-width floor alone, which widened the
    // undersampled lines into one solid shimmering band at middle zoom.
    let river_line_resolve = 1.0 - smoothstep(0.15, 0.45, fwidth(band_phase));
    let river_travel_lines = line_core * breakup * shore_zone * shore_zone * river_line_resolve;

    // Soft wash under the lines, kept close to the waterline. The border is
    // wobbled by the SMOOTH wave field — the old floor()+time hash re-rolled
    // a new random value whenever the scrolling cell grid crossed a
    // boundary, which made the fade edge flicker.
    let wash_border = line_wobble * 0.035;
    let river_wash = (1.0 - smoothstep(0.03 + wash_border, 0.16 + wash_border * 1.6, shore_dist))
        * 0.55
        * foam_dist_fade;

    let river_shore_foam = clamp(
        river_contact + river_travel_lines * 0.95 + river_wash,
        0.0,
        1.0,
    );

    // About 0.5-1.2m of soft contact foam at the visible water edge.
    let ocean_contact_width = max(0.018, fwidth(shore_dist) * 1.25);
    let ocean_contact = 1.0 - smoothstep(
        0.010 + ocean_contact_width,
        0.035 + ocean_contact_width,
        shore_dist,
    );

    let ocean_wash = (1.0 - smoothstep(0.018, 0.145, shore_dist)) * 0.42 * foam_dist_fade;
    // The contact line persists as the stylized one-pixel coast outline, but
    // its full strength at map distances read as a hard white rim around
    // every landmass; let it recede without disappearing. The old family of
    // traveling shore-normal foam fronts is gone: with the sea genuinely
    // rolling, marching parallel stripes read as artificial.
    let contact_strength = mix(0.95, 0.35, smoothstep(500.0, 1500.0, view_dist));
    let ocean_shore_foam = clamp(
        ocean_contact * contact_strength + ocean_wash,
        0.0,
        1.0,
    );
    let shore_foam = mix(river_shore_foam, ocean_shore_foam, ocean_factor);

    // Wake and interaction foam: stern breadcrumbs written by
    // spawn_boat_wake_ripples grow into overlapping rings with a dissolving
    // churn core, reading as a foam trail behind every moving boat.
    const RIPPLE_LIFE: f32 = 5.0;
    var ripple_foam = 0.0;
    for (var i = 0u; i < 16u; i++) {
        let ripple = material.ripples[i];
        let age = globals.time - ripple.z;
        if (ripple.w > 0.001 && age > 0.0 && age < RIPPLE_LIFE) {
            let life01 = age / RIPPLE_LIFE;
            let radius = 0.45 + age * 0.85;
            let d = distance(in.world_position.xz, ripple.xy);
            let ring = 1.0 - smoothstep(0.12, 0.55, abs(d - radius));
            // Filled churn core that dissolves faster than its ring spreads.
            let core = (1.0 - smoothstep(0.0, radius, d))
                * (1.0 - smoothstep(0.0, 0.45, life01));
            let fade = (1.0 - life01) * (1.0 - life01);
            ripple_foam += (ring * 0.8 + core * 0.7) * fade * ripple.w;
        }
    }

    // Sparkle comes from the sun-glint pass alone — a drifting dot pattern
    // reads as a texture sliding over the surface, worst at the shore edge.
    // Crest DISCS are fully retired (they read as suds smeared across the
    // sea); the thin breaking crest lines carry the wave energy instead.
    let foam_mask = clamp(
        shore_foam + churn_foam * 0.85 + ripple_foam * 0.85,
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
#else
    let fallback_slope = swell_gradient(in.world_position.xz, wave_time)
        * material.wave_params.x
        * swell_motion_scale(depth, shore_dist);
    let broad_normal = normalize(vec3<f32>(-fallback_slope.x, 1.0, -fallback_slope.y));
#endif
    let safe_normal_y = max(abs(broad_normal.y), 0.001);
    let swell_slope = vec2<f32>(-broad_normal.x, -broad_normal.z) / safe_normal_y;
    let ndv = clamp(abs(dot(broad_normal, view_vec)), 0.02, 1.0);
    let fresnel = pow(1.0 - ndv, 3.0);
    // A proper sky BLUE, weakly mixed. Deep ocean retains its authored navy
    // at grazing RTS angles; shallows keep more of the bright reflection.
    let sky_tint = vec3<f32>(0.45, 0.68, 0.90);
    let reflection_strength = mix(0.30, 0.14, depth);
    base_rgb = mix(base_rgb, sky_tint, fresnel * reflection_strength);
    var alpha = clamp(base.a + fresnel * 0.22, 0.0, 0.97);

    // Fake subsurface scattering: a crest is thin water, and when its slope
    // faces away from the sun light passes through and the tip glows
    // sea-glass green. The RTS camera rarely aligns view with -sun, so the
    // backlit term rides the surface slope rather than the view vector.
    let sun_xz = normalize(material.sun_params.xz + vec2<f32>(1.0e-4, 0.0));
    let away_slope =
        clamp(-(broad_normal.x * sun_xz.x + broad_normal.z * sun_xz.y) * 9.0, 0.0, 1.0);
    let crest_tip = smoothstep(0.30, 0.72, swell_value);
    let sss = crest_tip * away_slope * open_water * smoothstep(0.45, 0.9, depth)
        * clamp(material.sun_params.w, 0.0, 1.0);
    // The noise field also breaks the glow into organic patches; tied only
    // to the crests it formed the same mechanical rows as the old foam.
    base_rgb = mix(base_rgb, vec3<f32>(0.16, 0.62, 0.58), sss * 0.42 * mix(0.45, 1.0, patches));

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
    // SPARSE twinkle mask: amplitude-modulating every 0.6m cell (the old
    // 0.25+0.75*hash) lit the whole specular lobe into a dashed grid at
    // oblique views; most cells must contribute nothing at all.
    let sparkle_cell = floor(in.world_position.xz * 1.7);
    let sparkle = smoothstep(0.60, 0.80, glitter_hash(sparkle_cell))
        * (0.35 + 0.65 * glitter_hash(sparkle_cell + 7.0));
    // The analytic ripple field goes sub-pixel past ~150m, where it used to
    // alias into slow moire blobs (its old fix — a flat 0.2 floor — just made
    // the glimmer vanish at middle zoom). Hand it off to the footprint-aware
    // glitter instead; the far-ocean underlay continues the same field beyond
    // the detailed-water fade.
    let near_fade = 1.0 - smoothstep(50.0, 150.0, view_dist);
    let glitter_fade = smoothstep(90.0, 170.0, view_dist);
    // 0.8: the sharpened swell steepens the ripple normals, which widened
    // the specular footprint; without this trim the near glint reads as a
    // dashed grid at oblique angles.
    let analytic = pow(max(dot(reflect(-view_vec, ripple_normal), sun_dir), 0.0), 130.0)
        * sparkle * near_fade;
    let glitter = ocean_glitter(in.world_position.xz, view_vec, sun_dir, globals.time)
        * glitter_fade;
    let glint = min((analytic + glitter) * material.sun_params.w, 1.4);
    color_rgb += glint * vec3<f32>(1.0, 0.97, 0.88);
    // Foam pushes toward opaque so the white shore lines read solid instead
    // of washing out over the seabed. The push relaxes with camera distance:
    // forcing aliased far foam opaque defeated the translucency that would
    // otherwise soften it.
    alpha = clamp(
        alpha + min(glint, 1.0) * 0.25 + foam_mask * (0.20 + 0.30 * foam_dist_fade),
        0.0,
        1.0,
    );

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

    // Smoothly reveal the matching far-ocean surface. Alpha blending is
    // continuous, so there is no screen-door pattern and no chunk-shaped box.
    let camera_detail = 1.0 - smoothstep(
        material.distance_fade.x,
        material.distance_fade.y,
        view_dist,
    );
    let square_distance = max(
        abs(in.world_position.x - material.detail_bounds.x),
        abs(in.world_position.z - material.detail_bounds.y),
    );
    let streamed_detail = select(
        1.0,
        1.0 - smoothstep(
            material.detail_bounds.z,
            material.detail_bounds.w,
            square_distance,
        ),
        material.detail_bounds.w > material.detail_bounds.z && !is_edge_extension,
    );
    alpha *= camera_detail * streamed_detail;

    return vec4<f32>(color_rgb, alpha);
}
