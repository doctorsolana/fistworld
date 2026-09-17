//! Baked cloud shape field for terrain cloud shadows.
//!
//! The terrain shader used to evaluate the cloud density noise (two fbm and
//! one ridge after a domain warp: 16 value-noise taps) for every terrain
//! fragment every frame, ~2.7 ms of a 20 ms small-town frame. The *shape*
//! term is a pure function of cloud-space position: coverage is a threshold
//! applied afterwards, the wind is a translation, and the storm folds into
//! coverage. So the shape is baked once, on the CPU, into a 1024² R16Float
//! texture covering a window of cloud space around the map, and the shader
//! samples it (see `cloud_density_baked` in `terrain_splat.wgsl`). The window
//! slides with the wind; a new bake is queued on a background task when the
//! drift approaches the window margin and swapped in together with its lane
//! by `sync_cloud_shadow_params`, so every chunk always pairs a window with
//! the texture it was baked for.
//!
//! `FISTFORCE_CLOUD_FIELD=0` keeps the per-fragment path (measurement A/B).

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{block_on, poll_once, AsyncComputeTaskPool, Task};
use shared::components::{CloudSeed, WorldTime};
use shared::terrain::WorldTerrain;

use super::clouds::hash_to_unit;

/// Texels per side. At the full 8.2 km map this is ~12 m per texel, at the
/// 20 % lab world ~5 m; the shadow band is smoothstepped over ~0.28 of the
/// shape range, so bilinear filtering hides the texel grid.
pub const CLOUD_FIELD_SIZE: u32 = 1024;
/// The baked window extends this fraction of the half extent plus
/// `WINDOW_MARGIN_METRES` beyond the map. Part of that margin is reserved
/// for sun-projected sample points (cloud layer height 350 m times a damped
/// projection, under 500 m); the rest is room for the wind to drift before
/// a re-bake is due, so small maps (560 m half extent in the lab) do not
/// re-bake every minute.
const WINDOW_MARGIN_FRAC: f32 = 0.35;
const WINDOW_MARGIN_METRES: f32 = 1100.0;
/// 350 m cloud height times a damped projection under 1.4, plus ~100 m of
/// drift slack for the half second a re-bake takes on the background task.
const PROJECTION_RESERVE_METRES: f32 = 600.0;

/// Wind drift (metres) after which the current bake is stale.
fn refresh_drift(half_extent: f32) -> f32 {
    half_extent * WINDOW_MARGIN_FRAC + WINDOW_MARGIN_METRES - PROJECTION_RESERVE_METRES
}
/// Must match `cloud_shadows.rs` / `cloud_layer.rs`.
const CLOUD_FIELD_INV_SCALE: f32 = 1.0 / 190.0;

/// The current baked field, its window lane and what it was baked for.
#[derive(Resource, Default)]
pub struct CloudFieldState {
    /// The texture chunk materials should bind; `None` until the first bake.
    pub image: Option<Handle<Image>>,
    /// Palette lane: xy window origin (cloud space), z 1 / size, w mode.
    pub lane: Vec4,
    /// Bumped on every swap so the lane sweep republishes.
    pub generation: u32,
    baked_for: Option<(Vec2, f32, f32)>,
    task: Option<Task<BakedField>>,
    disabled: Option<bool>,
}

struct BakedField {
    origin: Vec2,
    size: f32,
    data: Vec<u8>,
    wind: Vec2,
    seed_phase: f32,
    half_extent: f32,
}

// --- CPU port of the WGSL cloud noise (terrain_splat.wgsl) ----------------
// `fract` is the WGSL one (x - floor(x)), not Rust's truncating `fract`.

fn wfract(x: f32) -> f32 {
    x - x.floor()
}

fn cloud_hash(p: Vec2) -> f32 {
    wfract((p.dot(Vec2::new(127.1, 311.7))).sin() * 43758.5453)
}

fn cloud_vnoise(p: Vec2) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let a = cloud_hash(i);
    let b = cloud_hash(i + Vec2::new(1.0, 0.0));
    let c = cloud_hash(i + Vec2::new(0.0, 1.0));
    let d = cloud_hash(i + Vec2::new(1.0, 1.0));
    let x0 = a + (b - a) * u.x;
    let x1 = c + (d - c) * u.x;
    x0 + (x1 - x0) * u.y
}

/// WGSL `mat2x2(vec2(1.6, -1.2), vec2(1.2, 1.6))` (column vectors) applied to `p`.
fn cloud_m(p: Vec2) -> Vec2 {
    Vec2::new(1.6 * p.x + 1.2 * p.y, -1.2 * p.x + 1.6 * p.y)
}

fn cloud_fbm(p_in: Vec2) -> f32 {
    let mut p = p_in;
    let mut amp = 0.55;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for _ in 0..4 {
        sum += amp * cloud_vnoise(p);
        norm += amp;
        amp *= 0.55;
        p = cloud_m(p);
    }
    sum / norm
}

fn cloud_ridge(p_in: Vec2) -> f32 {
    let mut p = p_in;
    let mut amp = 0.8;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for _ in 0..4 {
        sum += amp * (cloud_vnoise(p) * 2.0 - 1.0).abs();
        norm += amp;
        amp *= 0.7;
        p = cloud_m(p);
    }
    sum / norm
}

/// The shape term of `cloud_density` at cloud-space position `p0` (before the
/// coverage threshold). Range 0..=2.4.
pub fn cloud_shape(p0: Vec2) -> f32 {
    let q = Vec2::new(
        cloud_fbm(p0 * 0.5),
        cloud_fbm(p0 * 0.5 + Vec2::new(5.2, 1.3)),
    );
    let p = p0 + 0.9 * (q - Vec2::splat(0.5));
    cloud_fbm(p) * cloud_ridge(p * 0.9) * 2.4
}

/// Cloud-space position of world `xz` for a given wind offset and seed phase:
/// the shader's `p0` in `cloud_density`.
pub fn cloud_space(world_xz: Vec2, wind_offset: Vec2, seed_phase: f32) -> Vec2 {
    (world_xz + wind_offset) * CLOUD_FIELD_INV_SCALE + Vec2::new(seed_phase, seed_phase * 1.73)
}

/// Window (origin, size) in cloud space that covers the map plus margin for
/// the given wind offset.
pub fn field_window(wind_offset: Vec2, seed_phase: f32, half_extent: f32) -> (Vec2, f32) {
    let center = cloud_space(Vec2::ZERO, wind_offset, seed_phase);
    let half = (half_extent * (1.0 + WINDOW_MARGIN_FRAC) + WINDOW_MARGIN_METRES)
        * CLOUD_FIELD_INV_SCALE;
    (center - Vec2::splat(half), 2.0 * half)
}

/// Palette lane for a window: xy origin, z 1 / size, w mode 1.
pub fn window_lane(origin: Vec2, size: f32) -> Vec4 {
    Vec4::new(origin.x, origin.y, 1.0 / size, 1.0)
}

fn bake(origin: Vec2, size: f32) -> Vec<u8> {
    let n = CLOUD_FIELD_SIZE as usize;
    let step = size / n as f32;
    let mut data = Vec::with_capacity(n * n * 2);
    for y in 0..n {
        for x in 0..n {
            let p0 = origin + Vec2::new((x as f32 + 0.5) * step, (y as f32 + 0.5) * step);
            let bits = half::f16::from_f32(cloud_shape(p0)).to_bits();
            data.extend_from_slice(&bits.to_le_bytes());
        }
    }
    data
}

/// Keep a baked field that covers the map for the current wind offset.
/// Runs before `sync_cloud_shadow_params`, which publishes the swap.
pub fn maintain_cloud_field(
    world_time_query: Query<&WorldTime>,
    seed_query: Query<&CloudSeed>,
    terrain: Option<Res<WorldTerrain>>,
    mut images: ResMut<Assets<Image>>,
    mut state: ResMut<CloudFieldState>,
) {
    let disabled = *state
        .disabled
        .get_or_insert_with(|| std::env::var("FISTFORCE_CLOUD_FIELD").is_ok_and(|v| v == "0"));
    if disabled {
        return;
    }
    // Finish a bake first: swap the texture and publish a new generation.
    if let Some(task) = state.task.as_mut() {
        if let Some(baked) = block_on(poll_once(task)) {
            state.task = None;
            let image = Image::new(
                Extent3d {
                    width: CLOUD_FIELD_SIZE,
                    height: CLOUD_FIELD_SIZE,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                baked.data,
                TextureFormat::R16Float,
                RenderAssetUsages::RENDER_WORLD,
            );
            state.image = Some(images.add(image));
            state.lane = window_lane(baked.origin, baked.size);
            state.generation = state.generation.wrapping_add(1);
            info!(
                "Cloud field baked: generation {} window origin {:?} size {:.2} (wind {:?}, half extent {:.0} m)",
                state.generation, baked.origin, baked.size, baked.wind, baked.half_extent
            );
            state.baked_for = Some((baked.wind, baked.seed_phase, baked.half_extent));
        }
        return;
    }
    let Some(world_time) = world_time_query.iter().next() else {
        return;
    };
    let abs_seconds =
        world_time.day as f32 * world_time.cycle_duration() + world_time.seconds_in_cycle;
    let seed = seed_query.iter().next().map(|s| s.seed).unwrap_or(0);
    let seed_phase = hash_to_unit(seed, 0) * 37.0;
    let (wind_offset, _) = super::clouds::cloud_wind_state(abs_seconds, seed_phase);
    let half_extent = terrain
        .as_ref()
        .map(|terrain| crate::terrain::terrain_climate_for_generator(&terrain.generator).x)
        .unwrap_or(4096.0);
    let stale = match state.baked_for {
        None => true,
        Some((wind, phase, extent)) => {
            phase != seed_phase
                || extent != half_extent
                || (wind_offset - wind).length() > refresh_drift(half_extent)
        }
    };
    if !stale {
        return;
    }
    let (origin, size) = field_window(wind_offset, seed_phase, half_extent);
    state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        BakedField {
            origin,
            size,
            data: bake(origin, size),
            wind: wind_offset,
            seed_phase,
            half_extent,
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_is_deterministic_and_in_range() {
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        for y in 0..64 {
            for x in 0..64 {
                let p = Vec2::new(x as f32 * 0.37 - 7.0, y as f32 * 0.41 - 3.0);
                let s = cloud_shape(p);
                assert_eq!(s, cloud_shape(p));
                min = min.min(s);
                max = max.max(s);
            }
        }
        assert!(min >= 0.0 && max <= 2.4, "shape range {min}..{max}");
        assert!(max - min > 0.5, "the field must vary, not be flat");
    }

    #[test]
    fn wgsl_fract_semantics_for_negative_inputs() {
        assert!((wfract(-0.25) - 0.75).abs() < 1e-6);
        assert!((wfract(2.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn window_covers_the_map_and_its_projected_margin() {
        let wind = Vec2::new(1234.0, -987.0);
        let seed_phase = 11.3;
        let half_extent = 1830.0;
        let (origin, size) = field_window(wind, seed_phase, half_extent);
        let lane = window_lane(origin, size);
        // Corners of the map, pushed 600 m further out (cloud height 350 m
        // times a damped projection well under 2).
        for (sx, sz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let world = Vec2::new(sx, sz) * (half_extent + 600.0);
            let p0 = cloud_space(world, wind, seed_phase);
            let uv = (p0 - Vec2::new(lane.x, lane.y)) * lane.z;
            assert!(uv.x > 0.0 && uv.x < 1.0 && uv.y > 0.0 && uv.y < 1.0, "uv {uv}");
        }
        assert_eq!(lane.w, 1.0);
    }

    #[test]
    fn a_bake_stays_valid_until_the_drift_reaches_the_margin_reserve() {
        // Everything the shader can sample (map + projection reserve) is still
        // inside the window when the drift hits the refresh threshold.
        for half_extent in [560.0, 1830.0, 4096.0] {
            let seed_phase = 3.0;
            let (origin, size) = field_window(Vec2::ZERO, seed_phase, half_extent);
            let drift = Vec2::new(refresh_drift(half_extent), 0.0);
            // 500 m of projection reach; the remaining reserve is drift slack.
            let far_world = Vec2::new(half_extent + 500.0, 0.0);
            let p0 = cloud_space(far_world, drift, seed_phase);
            let uv = (p0 - origin) / size;
            assert!(uv.x < 1.0 && uv.x > 0.0, "half extent {half_extent}: uv {uv}");
            assert!(refresh_drift(half_extent) >= 600.0, "at least minutes between bakes");
        }
    }

    #[test]
    fn bake_writes_one_half_float_per_texel() {
        let (origin, size) = field_window(Vec2::ZERO, 0.0, 256.0);
        let data = bake(origin, size);
        assert_eq!(data.len(), (CLOUD_FIELD_SIZE * CLOUD_FIELD_SIZE * 2) as usize);
        let first = half::f16::from_le_bytes([data[0], data[1]]).to_f32();
        let p0 = origin + Vec2::splat(0.5 * size / CLOUD_FIELD_SIZE as f32);
        assert!((first - cloud_shape(p0)).abs() < 2e-3, "{first} vs {}", cloud_shape(p0));
    }
}
