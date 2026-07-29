// World-anchored cloud plane: one huge quad at CLOUD_LAYER_HEIGHT shaded by a
// deterministic world-space cloud field. Unlit; alpha comes only from the
// low-frequency warped shape, high-frequency detail only tints brightness.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::{globals, view},
}

// Must match CLOUD_LAYER_HEIGHT in cloud_layer.rs.
const CLOUD_LAYER_HEIGHT: f32 = 350.0;

struct CloudLayerUniform {
    // x: coverage 0..1, y: inv world scale, zw: wind offset at the anchor
    params_a: vec4<f32>,
    // x: seed phase, y: day factor, z: drift anchor time (client seconds),
    // w: alpha scale
    params_b: vec4<f32>,
    // xyz: sun light travel direction, w: drift speed (client-time units)
    sun_dir: vec4<f32>,
    tint_lit: vec4<f32>,
    tint_shadow: vec4<f32>,
    // xy: storm center at the wind anchor, z: storminess, w: reserved.
    storm: vec4<f32>,
};

@group(3) @binding(0) var<uniform> material: CloudLayerUniform;

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

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Extrapolate the wind past the anchored offset so drift is frame-smooth
    // between the ~1/sec uniform refreshes. Must mirror the terrain/water
    // shaders' extrapolation exactly.
    let cloud_drift = material.sun_dir.w * (globals.time - material.params_b.z);
    let params_a = vec4<f32>(
        material.params_a.xy,
        material.params_a.zw + vec2<f32>(0.86, 0.5) * cloud_drift,
    );
    let seed_phase = material.params_b.x;
    let world_xz = in.world_position.xz;

    // THE storm locally thickens the deck into a dark ragged disc.
    let storm_center = material.storm.xy + vec2<f32>(0.86, 0.5) * (0.55 * cloud_drift);
    let storm = storm_cell(world_xz, storm_center, material.storm.z);
    let params_a_storm = vec4<f32>(min(params_a.x + storm * 0.95, 1.0), params_a.yzw);
    let density = cloud_density(world_xz, params_a_storm, seed_phase);

    // Parallax upper deck: 2x blob size, slower wind rotated ~15 deg, fixed
    // offset so it never lines up with the main deck. Alpha shaping only.
    let deck2_wind = mat2x2<f32>(vec2<f32>(0.9659, 0.2588), vec2<f32>(-0.2588, 0.9659))
        * (params_a.zw * 0.6);
    let params_a2 = vec4<f32>(params_a.x, params_a.y * 0.5, deck2_wind.x, deck2_wind.y);
    let density2 = cloud_density(world_xz + vec2<f32>(1000.0, -700.0), params_a2, seed_phase);
    let shape = max(density, density2 * 0.7);

    // Everything below only affects COLOR; wherever the shape is ~zero the
    // alpha is ~zero and the color is blended away. The branch is spatially
    // coherent at the ~190 m blob scale, so the empty sky between clouds
    // (most of the 40 km quad on calm frames) genuinely skips the 68 noise
    // units of brightness detail + bump normal below.
    var col = material.tint_shadow.rgb;
    if (shape > 0.005) {
        // Higher-frequency detail modulates brightness only — never alpha.
        // Kept gentle: strong detail reads as mottling inside the masses.
        let p0 =
            (world_xz + params_a.zw) * params_a.y + vec2<f32>(seed_phase, seed_phase * 1.73);
        let brightness = clamp(0.68 + 0.22 * cloud_fbm(p0 * 2.6), 0.0, 1.0);

        // Fake volume: bump normal from central differences of the density
        // field. Taps use the same storm-boosted params as the drawn deck.
        let e = 6.0;
        let bump = 1.6;
        let ddx = cloud_density(world_xz + vec2<f32>(e, 0.0), params_a_storm, seed_phase)
            - cloud_density(world_xz - vec2<f32>(e, 0.0), params_a_storm, seed_phase);
        let ddz = cloud_density(world_xz + vec2<f32>(0.0, e), params_a_storm, seed_phase)
            - cloud_density(world_xz - vec2<f32>(0.0, e), params_a_storm, seed_phase);
        let n = normalize(vec3<f32>(-ddx * bump, 1.0, -ddz * bump));
        let half_lambert = dot(n, -material.sun_dir.xyz) * 0.5 + 0.5;

        col = mix(material.tint_shadow.rgb, material.tint_lit.rgb * brightness, half_lambert);
        // Storm masses go slate and brooding.
        col = mix(col, vec3<f32>(0.19, 0.21, 0.26), smoothstep(0.0, 0.7, storm) * 0.95);
        // Rim brighten just inside the silhouette.
        col += vec3<f32>(
            0.15 * (smoothstep(0.0, 0.3, density) - smoothstep(0.3, 0.7, density))
        );
    }

    // Fade the layer out as the camera crosses it so the quad never slices
    // the view mid-screen.
    let crossing_fade = smoothstep(0.0, 80.0, abs(view.world_position.y - CLOUD_LAYER_HEIGHT));
    // Clouds all but vanish at night — a moonless dark sky shows silhouettes,
    // not bright shapes, and the dark world below must stay readable.
    let night_fade = smoothstep(0.0, 0.25, clamp(material.params_b.y, 0.0, 1.0));

    // Gamma on the shape thins the wide mid-density haze and solidifies cloud
    // cores, so masses read as bodies instead of washes.
    let body = pow(shape, 1.25);
    let alpha = clamp(body * material.params_b.w * crossing_fade * night_fade, 0.0, 1.0);
    // The storm deck is nearly opaque — a translucent dark cloud blends
    // toward whatever ground is under it (snow washed it to pale lavender)
    // and stops reading as a storm at all.
    let alpha_storm = clamp(alpha * (1.0 + storm * 1.2), 0.0, 0.96);
    return vec4<f32>(col, alpha_storm);
}
