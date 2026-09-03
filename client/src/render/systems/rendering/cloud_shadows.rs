//! Cloud shadow uniform sync.
//!
//! CPU half of the sun-projected cloud shadows: terrain_splat.wgsl and
//! toon_water.wgsl sample one shared procedural cloud density (the
//! "Cloud field (EXACT copy ...)" block in each shader) and every parameter
//! they need is packed into two vec4s pushed through the existing material
//! uniforms here. Everything derives from replicated state (WorldTime,
//! CloudSeed, CloudCover), so every client shades the same ground under the
//! same cloud.

use super::day_night::smoothstep;
use super::*;
use crate::terrain::{TerrainChunk, TerrainSplatMaterial};
use crate::water::chunks::WaterRenderAssets;
use crate::water::material::ToonWaterMaterial;
use bevy::math::Vec4Swizzles;
use bevy::platform::collections::HashMap;
use shared::components::{CloudSeed, WorldTime};
use shared::terrain::WorldTerrain;

/// Dominant cloud blob scale: the shaders sample the field at world_xz * this.
const CLOUD_FIELD_INV_SCALE: f32 = 1.0 / 190.0;
/// Wind bearing/speed live in `clouds::cloud_wind_state` (shared with the
/// visible deck). 0.30 strength: clearly readable rolling shade — the multiply
/// lands after full lighting (ambient included), and past ~0.35 it stops
/// reading as weather and starts reading as dirty ground.
const CLOUD_SHADOW_STRENGTH: f32 = 0.30;
/// Below this sun height the projection would smear shadows toward the horizon.
const MIN_SUN_Y: f32 = 0.15;

// Diff-gate thresholds: every materials.get_mut re-prepares the material on
// the GPU. Wind is NOT stepped — shaders extrapolate drift from an anchored
// (offset, speed) pair via globals.time, so motion is frame-smooth while the
// anchor refreshes at ~1/sec (plus immediately on gust/warp speed changes).
const ANCHOR_REFRESH_SECS: f32 = 1.0;
const SPEED_WRITE_STEP: f32 = 0.01;
const COVERAGE_WRITE_STEP: f32 = 0.005;
const SUN_PROJ_WRITE_STEP: f32 = 0.01;
const STRENGTH_WRITE_STEP: f32 = 0.005;
/// Chunk materials receiving the newest lane snapshot per frame. Every
/// `materials.get_mut` re-prepares that material on the GPU (five uniform
/// buffers + a bind group), and with view_distance 8 there are up to 289 chunk
/// materials; writing them all in one frame cost ~10 ms on the render thread
/// per write, and at high time warp the sun projection crosses its threshold
/// EVERY frame. Sweeping 48/frame finishes 289 chunks in six frames; chunks
/// mid-sweep hold an anchor at most ~100 ms older than their neighbours, the
/// same disparity the per-chunk top-up of freshly streamed chunks already
/// produces, and the shaders extrapolate drift and sun sweep from the anchor
/// so the field stays continuous across the seam.
const LANE_WRITES_PER_FRAME: usize = 48;

// The shadow field's seed phase must match what the sky derives from the
// same `CloudSeed`, so both use the one hash in clouds.rs.
use super::clouds::hash_to_unit;

/// Last cloud-shadow uniforms written to the GPU materials, plus the previous
/// sun projection so its velocity can be finite-differenced for in-shader
/// extrapolation (the sun completes a full arc within one short game day, so its sweep component
/// of shadow motion is often FASTER than the wind and steps visibly if only
/// refreshed at the anchor rate).
#[derive(Default)]
pub struct CloudShadowParams {
    written: Option<(Vec4, Vec4, Vec4, Vec4)>,
    prev_sun_proj: Option<(Vec2, f32)>,
    /// Newest lane snapshot every chunk material must eventually hold.
    pending: Option<CloudShadowLanes>,
    /// Bumped per snapshot; a chunk is up to date when `applied` says so.
    generation: u32,
    /// Generation each chunk entity last received. Chunks that stream out are
    /// pruned when the map outgrows the live chunk set.
    applied: HashMap<Entity, u32>,
}

/// The five palette lanes `sync_cloud_shadow_params` owns, as one snapshot.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CloudShadowLanes {
    clouds_a: Vec4,
    clouds_b: Vec4,
    clouds_c: Vec4,
    climate: Vec4,
    storm: Vec4,
}

/// Which chunk entities receive the pending lanes THIS frame: every entity not
/// yet at `generation`, at most `budget` of them, in iteration order. Pure so
/// the amortisation contract is testable without materials or a GPU.
fn lane_sweep_targets(
    entities: impl Iterator<Item = Entity>,
    applied: &HashMap<Entity, u32>,
    generation: u32,
    budget: usize,
) -> Vec<Entity> {
    entities
        .filter(|entity| applied.get(entity) != Some(&generation))
        .take(budget)
        .collect()
}

/// Push the shared cloud-field parameters into every terrain chunk material
/// and the water material. Must run after `update_day_night_cycle` so the sun
/// transform is current. Writes are diff-gated; newly streamed chunks are
/// topped up outside the gate so their shade doesn't pop at the next batch.
#[allow(clippy::too_many_arguments)]
pub fn sync_cloud_shadow_params(
    time: Res<Time>,
    world_time_query: Query<&WorldTime>,
    warp_query: Query<&shared::components::TimeWarp>,
    seed_query: Query<&CloudSeed>,
    cover: Res<CloudCover>,
    settings: Res<GraphicsSettings>,
    sun_query: Query<&Transform, With<SunLight>>,
    chunks: Query<(Entity, &TerrainChunk)>,
    water_assets: Option<Res<WaterRenderAssets>>,
    terrain: Option<Res<WorldTerrain>>,
    mut terrain_materials: ResMut<Assets<TerrainSplatMaterial>>,
    mut water_materials: ResMut<Assets<ToonWaterMaterial>>,
    mut state: Local<CloudShadowParams>,
) {
    let Some(world_time) = world_time_query.iter().next() else {
        return;
    };
    let Ok(sun_tf) = sun_query.single() else {
        return;
    };

    // Absolute world seconds: monotonic across the day wrap and warp-aware
    // (the server advances both fields), unlike the 120 s ocean loop — the
    // cloud field drifts forever without a wrap pop.
    let abs_seconds =
        world_time.day as f32 * world_time.cycle_duration() + world_time.seconds_in_cycle;

    let seed = seed_query.iter().next().map(|s| s.seed).unwrap_or(0);
    let seed_phase = hash_to_unit(seed, 0) * 37.0;
    let (wind_offset, wind_speed) = super::clouds::cloud_wind_state(abs_seconds, seed_phase);
    // Shaders advance the drift with globals.time (client seconds, warp-blind),
    // so the anchored speed carries the warp factor.
    let warp = warp_query.iter().next().map(|w| w.0).unwrap_or(1.0);
    let anchor_time = time.elapsed_secs();
    let speed_client = wind_speed * warp;

    // Same elevation curve update_day_night_cycle keys the sun on: shadows
    // fade with the direct light instead of ghosting through dusk, and the
    // meaningless night-time projection never shows.
    let elevation = -world_time.sun_phase().cos();
    let day_factor = smoothstep(-0.05, 0.15, elevation);
    // The coverage ramp pins strength to exactly 0 on a clear sky, so
    // shade == 1.0 bit-exactly when there is nothing overhead to cast it.
    let strength = if settings.clouds_enabled {
        CLOUD_SHADOW_STRENGTH * day_factor * smoothstep(0.0, 0.08, cover.current)
    } else {
        0.0
    };

    // The shaders walk `world_xz - (H - y) * proj` from the fragment up the
    // sun ray to the cloud layer, so proj = -to_sun.xz / to_sun.y. Clamping
    // the height keeps a horizon sun from smearing shadows to infinity.
    let to_sun = Vec3::from(sun_tf.back());
    let sun_y = to_sun.y.max(MIN_SUN_Y);
    // Damped: over a short game day the raw projection sweeps shadows 2-5x
    // faster than the wind moves their clouds, so shadows visibly travel the
    // WRONG WAY relative to the sky. The damp keeps a directional lean (long
    // morning/evening shadows offset away from the sun) while the sweep rate
    // stays below wind speed, so shadows track their clouds. 1.0 = physical.
    const SUN_SWEEP_DAMP: f32 = 0.2;
    let sun_proj = Vec2::new(-to_sun.x, -to_sun.z) / sun_y * SUN_SWEEP_DAMP;

    let clouds_a = Vec4::new(
        cover.current,
        CLOUD_FIELD_INV_SCALE,
        wind_offset.x,
        wind_offset.y,
    );
    let clouds_b = Vec4::new(sun_proj.x, sun_proj.y, strength, seed_phase);
    // Sun-projection velocity by finite difference across anchor writes;
    // includes warp automatically. First write starts at zero.
    let sun_proj_vel = match state.prev_sun_proj {
        Some((prev, prev_t)) if anchor_time - prev_t > 0.05 => {
            (sun_proj - prev) / (anchor_time - prev_t)
        }
        Some(_) => Vec2::ZERO,
        None => Vec2::ZERO,
    };
    let clouds_c = Vec4::new(anchor_time, sun_proj_vel.x, speed_client, sun_proj_vel.y);
    // Static per map: half extent + the world-seed climate phase (NOT the
    // cloud seed — climate is terrain, identical across sessions).
    let half_extent = terrain
        .as_ref()
        .map(|terrain| {
            let bounds = terrain.generator.active_map_bounds();
            (bounds.max[0] - bounds.min[0]) * 0.5
        })
        .unwrap_or(4096.0);
    let climate_phase = terrain
        .as_ref()
        .and_then(|terrain| {
            terrain
                .generator
                .loaded_map()
                .definition
                .generated
                .as_ref()
                .map(|g| shared::worldgen::climate_phase(g.seed))
        })
        .unwrap_or(0.0);
    // zw: the seed's prevailing dune wind, for windward/lee dune shading.
    let dune_dir = terrain
        .as_ref()
        .and_then(|terrain| {
            terrain
                .generator
                .loaded_map()
                .definition
                .generated
                .as_ref()
                .map(|g| shared::worldgen::dune_direction(g.seed))
        })
        .unwrap_or(Vec2::ZERO);
    let climate = Vec4::new(half_extent, climate_phase, dune_dir.x, dune_dir.y);
    // THE storm system: center anchored at this write (shaders extrapolate it
    // with the cloud drift), strength zeroed with clouds disabled so both the
    // rain darkening AND its per-fragment fbm cost vanish with the sky
    // (storm_cell early-outs on storminess < 0.01).
    let storminess = if settings.clouds_enabled {
        cover.storminess
    } else {
        0.0
    };
    let storm_anchor = super::clouds::storm_center(seed_phase, abs_seconds, half_extent);
    let storm = Vec4::new(storm_anchor.x, storm_anchor.y, storminess, 0.0);

    // Diagnostic kill switch: `FISTFORCE_CLOUD_SHADOW_FREEZE=1` stops every
    // lane write after the first, so a perf run can measure the cost of the
    // material churn itself (Bevy re-uploads every mesh whose material asset
    // is modified). Frozen shadows are wrong; this is for measurement only.
    if state.written.is_some()
        && std::env::var("FISTFORCE_CLOUD_SHADOW_FREEZE").is_ok_and(|v| v == "1")
    {
        return;
    }
    let write_due = match state.written {
        None => true,
        Some((a, b, c, s)) => {
            (clouds_a.x - a.x).abs() > COVERAGE_WRITE_STEP
                || clouds_b.xy().distance_squared(b.xy())
                    > SUN_PROJ_WRITE_STEP * SUN_PROJ_WRITE_STEP
                || (clouds_b.z - b.z).abs() > STRENGTH_WRITE_STEP
                || clouds_b.w != b.w
                || anchor_time - c.x > ANCHOR_REFRESH_SECS
                || (speed_client - c.z).abs() > SPEED_WRITE_STEP
                // Storminess normally rides the anchor cadence (slow lerp),
                // but a settings toggle zeroes it instantly — write through.
                || (storm.z - s.z).abs() > STRENGTH_WRITE_STEP
        }
    };

    if write_due {
        if let Some(water_assets) = &water_assets {
            if let Some(mut material) = water_materials.get_mut(&water_assets.material) {
                material.uniform.clouds_a = clouds_a;
                material.uniform.clouds_b = clouds_b;
                material.uniform.clouds_c = clouds_c;
                material.uniform.storm = storm;
            }
        }
        state.written = Some((clouds_a, clouds_b, clouds_c, storm));
        state.prev_sun_proj = Some((sun_proj, anchor_time));
        // Terrain gets the same snapshot, but spread over the coming frames
        // (see LANE_WRITES_PER_FRAME). Freshly streamed chunks are simply
        // entities missing from `applied`, so they are topped up the frame
        // they appear without a separate `is_added` path.
        state.generation = state.generation.wrapping_add(1);
        state.pending = Some(CloudShadowLanes {
            clouds_a,
            clouds_b,
            clouds_c,
            climate,
            storm,
        });
    }

    let Some(lanes) = state.pending else {
        return;
    };
    let generation = state.generation;
    let targets = lane_sweep_targets(
        chunks.iter().map(|(entity, _)| entity),
        &state.applied,
        generation,
        LANE_WRITES_PER_FRAME,
    );
    for entity in targets {
        let Ok((_, chunk)) = chunks.get(entity) else {
            continue;
        };
        if let Some(mut material) = terrain_materials.get_mut(&chunk.material) {
            material.extension.palette.clouds_a = lanes.clouds_a;
            material.extension.palette.clouds_b = lanes.clouds_b;
            material.extension.palette.clouds_c = lanes.clouds_c;
            material.extension.palette.climate = lanes.climate;
            material.extension.palette.storm = lanes.storm;
        }
        state.applied.insert(entity, generation);
    }
    // Streamed-out chunks leave stale keys behind; prune once they outnumber
    // the live set rather than paying a rebuild every frame.
    let live = chunks.iter().len();
    if state.applied.len() > live.saturating_mul(2).max(64) {
        let alive: bevy::platform::collections::HashSet<Entity> =
            chunks.iter().map(|(entity, _)| entity).collect();
        state.applied.retain(|entity, _| alive.contains(entity));
    }
}

#[cfg(test)]
mod lane_sweep_tests {
    use super::*;

    /// 289 chunks at 48 per frame finish in seven frames, no chunk is written
    /// twice for one generation, and a chunk streaming in afterwards is topped
    /// up on its first frame.
    #[test]
    fn the_lane_sweep_covers_every_chunk_once_and_tops_up_newcomers() {
        let chunks: Vec<Entity> = (1..=289u32)
            .map(|i| Entity::from_raw_u32(i).unwrap())
            .collect();
        let mut applied = HashMap::default();
        let generation = 7;
        let mut frames = 0;
        let mut written = 0;
        loop {
            let targets = lane_sweep_targets(
                chunks.iter().copied(),
                &applied,
                generation,
                LANE_WRITES_PER_FRAME,
            );
            if targets.is_empty() {
                break;
            }
            frames += 1;
            for entity in targets {
                assert!(
                    applied.insert(entity, generation).is_none(),
                    "written twice"
                );
                written += 1;
            }
        }
        assert_eq!(written, 289);
        assert_eq!(frames, 7);

        let newcomer = Entity::from_raw_u32(1000).unwrap();
        let targets = lane_sweep_targets(
            chunks.iter().copied().chain(std::iter::once(newcomer)),
            &applied,
            generation,
            LANE_WRITES_PER_FRAME,
        );
        assert_eq!(targets, vec![newcomer]);

        // A new generation re-sweeps everything.
        let targets = lane_sweep_targets(
            chunks.iter().copied(),
            &applied,
            generation + 1,
            LANE_WRITES_PER_FRAME,
        );
        assert_eq!(targets.len(), LANE_WRITES_PER_FRAME);
    }
}
